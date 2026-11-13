//! X12 COPY adapters. Shards are execution-scoped and survive worker rotation.

use std::io::Write;
use std::path::Path;

use arrow_array::cast::AsArray;
use arrow_array::{Array, RecordBatch};
use arrow_schema::{DataType, Schema};
use vgi::copy_from::{CopyFromFunction, CopyFromReadContext};
use vgi::copy_to::{CopyToCloseContext, CopyToFunction, CopyToWriteContext};
use vgi::{ipc, ArgSpec, FunctionMetadata, Worker};
use vgi_rpc::{OutputCollector, Result, RpcError};

use crate::{arrow_io, source, table::read_x12};
use x12_core::serialization::X12Writer;

const SHARDS: &[u8] = b"x12-copy-segments";

pub fn register(worker: &mut Worker) {
    worker.register_copy_from(X12CopyFrom);
    worker.register_copy_to(X12CopyTo);
}

fn error(message: impl Into<String>) -> RpcError {
    RpcError::value_error(message.into())
}

fn metadata() -> FunctionMetadata {
    let llm = "Import and export X12 with the alias-scoped x12 COPY format. FROM emits the \
        complete read_x12 schema. TO consumes ordered segment_id, elements, element_separator, \
        and segment_terminator columns. Paths are worker-local. The writer validates complete \
        ASCII envelopes and publishes only after validation; recalculate_counts updates trailer \
        counts without changing control numbers. Use read_x12 for ordinary SELECT queries.";
    let md = "## X12 COPY format\n\n\
        The registered handler is invoked by DuckDB's COPY machinery. It cannot be called \
        directly as a table function. Discover the alias-scoped format using the example query.\n\n\
        **Import.** Create the destination table from the schema of `read_x12(input)` with zero \
        rows, then import a worker-local file or glob. Column names, order, and types must match. \
        Reads retain malformed segments for inspection and materialize the input in memory.\n\n\
        **Export.** Supply the four named columns `segment_id`, `elements`, `element_separator`, \
        and `segment_terminator`, ordered by document and segment index. Other columns are \
        informational. ISA11 and ISA16 preserve repetition and component delimiters. \
        Empty element strings are retained; NULL values are rejected.\n\n\
        **Validation and publication.** The writer checks ASCII text, fixed-width ISA, envelope \
        nesting, counts, and control matches across batches. The explicit `recalculate_counts` \
        option updates SE01/GE01/IEA01 only. A same-directory temporary file replaces the target \
        after validation completes; failure preserves the old target. Empty input writes an \
        empty file. This is structural validation; transaction-specific content rules remain \
        outside scope. Formatting whitespace is normalized. EDIFACT and binary payloads are \
        unsupported. See the repository README for the complete edit-and-export workflow.";
    let example = vgi::FunctionExample {
        description: "Discover the X12 COPY format, supported directions, ordering, and options.".into(),
        sql: "SELECT DISTINCT format_name, direction, ordered, option_name FROM vgi_copy_formats() WHERE handler = 'copy_x12' ORDER BY format_name, option_name;".into(),
        expected_output: None,
    };
    let mut tags = crate::meta::object_tags(
        "X12 COPY",
        llm,
        md,
        "x12, edi, copy, segments, envelopes, local files",
        "Segment & element explode",
    );
    tags.push((
        "vgi.result_columns_schema".into(),
        crate::meta::result_columns_schema(&read_x12::output_schema()),
    ));
    tags.push((
        "vgi.example_queries".into(),
        crate::meta::example_queries_tag(std::slice::from_ref(&example)),
    ));
    FunctionMetadata {
        description:
            "Import and export complete X12 segment rows through the catalog's COPY format.".into(),
        examples: vec![example],
        tags,
        ..Default::default()
    }
}

pub struct X12CopyFrom;

impl CopyFromFunction for X12CopyFrom {
    fn format(&self) -> &str {
        "x12"
    }
    fn handler_name(&self) -> &str {
        "copy_x12"
    }
    fn comment(&self) -> Option<String> {
        Some("Complete X12 segment rows (read_x12 schema)".into())
    }
    fn metadata(&self) -> FunctionMetadata {
        metadata()
    }
    fn argument_specs(&self) -> Vec<ArgSpec> {
        Vec::new()
    }
    fn read(
        &self,
        ctx: &CopyFromReadContext,
        _out: &mut OutputCollector,
    ) -> Result<Vec<RecordBatch>> {
        let schema = read_x12::output_schema();
        if ctx.expected_schema.fields().len() != schema.fields().len()
            || ctx
                .expected_schema
                .fields()
                .iter()
                .zip(schema.fields())
                .any(|(a, b)| a.name() != b.name() || !a.data_type().equals_datatype(b.data_type()))
        {
            return Err(error("COPY FROM X12 target must match read_x12 column names, order, and types; create it with CREATE TABLE ... AS SELECT * FROM x12.read_x12(...) LIMIT 0"));
        }
        let docs: Vec<_> = source::expand_local(ctx.path)?
            .into_iter()
            .map(|path| {
                let bytes = std::fs::read(&path).map_err(|e| error(format!("read {path}: {e}")))?;
                Ok(source::InputDoc {
                    source_path: Some(path),
                    bytes,
                })
            })
            .collect::<Result<_>>()?;
        // The SDK's CopyFromProducer does not expose resume state, including
        // when read_stream supplies a producer. HTTP therefore requires one
        // response batch. This reader already materializes the entire input.
        Ok(vec![arrow_io::build_batch(
            ctx.expected_schema,
            &read_x12::rows(&docs),
        )?])
    }
}

pub struct X12CopyTo;

impl CopyToFunction for X12CopyTo {
    fn format(&self) -> &str {
        "x12"
    }
    fn handler_name(&self) -> &str {
        "copy_x12"
    }
    fn comment(&self) -> Option<String> {
        Some("Validated, ordered X12 text interchanges".into())
    }
    fn metadata(&self) -> FunctionMetadata {
        metadata()
    }
    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![ArgSpec::const_arg("recalculate_counts", -1, "boolean",
            "Recompute SE01, GE01, and IEA01 after edits. Defaults to false (validate existing counts). Never changes control numbers or synthesizes missing envelopes.")]
    }
    fn ordered(&self) -> bool {
        true
    }
    fn write(&self, ctx: &CopyToWriteContext, batch: &RecordBatch) -> Result<()> {
        validate_schema(batch.schema().as_ref())?;
        let blob = ipc::write_batch(batch)?;
        ctx.storage.append(ctx.execution_id, SHARDS, b"", blob);
        Ok(())
    }
    fn close(&self, ctx: &CopyToCloseContext) -> Result<i64> {
        let schema = ctx
            .input_schema
            .ok_or_else(|| error("COPY TO X12 requires an input schema"))?;
        validate_schema(schema.as_ref())?;
        let path = Path::new(ctx.path);
        if ctx.path.contains("://") {
            return Err(error("COPY TO X12 supports worker-local file paths only"));
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut output = tempfile::NamedTempFile::new_in(parent)
            .map_err(|e| error(format!("create X12 temporary output: {e}")))?;
        let mut writer = X12Writer::new(
            ctx.options
                .named_bool("recalculate_counts")
                .unwrap_or(false),
        );
        let (mut after, mut count) = (-1, 0);
        loop {
            // Bound memory to a few batches, even when the input is large.
            let shards = ctx.storage.scan(ctx.execution_id, SHARDS, b"", after, 8);
            if shards.is_empty() {
                break;
            }
            for (id, blob) in shards {
                let batch = ipc::read_batch(&blob)?;
                count += encode_batch(&mut writer, &batch, output.as_file_mut())?;
                after = id;
            }
        }
        writer.finish().map_err(error)?;
        output
            .as_file_mut()
            .flush()
            .map_err(|e| error(format!("flush X12 output: {e}")))?;
        output
            .as_file()
            .sync_all()
            .map_err(|e| error(format!("sync X12 output: {e}")))?;
        output
            .persist(path)
            .map_err(|e| error(format!("publish X12 output: {e}")))?;
        Ok(count)
    }
}

/// Accept the four writing columns by name, in any order. Envelope keys and
/// source offsets are informational; the actual segment values are authoritative.
fn validate_schema(schema: &Schema) -> Result<()> {
    for (name, ty) in [
        ("segment_id", DataType::Utf8),
        ("elements", arrow_io::varchar_list()),
        ("element_separator", DataType::Utf8),
        ("segment_terminator", DataType::Utf8),
    ] {
        let fields: Vec<_> = schema
            .fields()
            .iter()
            .filter(|f| f.name() == name)
            .collect();
        if fields.len() != 1 || !fields[0].data_type().equals_datatype(&ty) {
            return Err(error(format!(
                "COPY TO X12 requires exactly one {name} column of type {ty}"
            )));
        }
    }
    Ok(())
}

fn encode_batch(
    writer: &mut X12Writer,
    batch: &RecordBatch,
    output: &mut impl Write,
) -> Result<i64> {
    validate_schema(batch.schema().as_ref())?;
    let strings = |name: &str| batch.column_by_name(name).unwrap().as_string::<i32>();
    let ids = strings("segment_id");
    let elements = batch.column_by_name("elements").unwrap().as_list::<i32>();
    let element_seps = strings("element_separator");
    let segment_seps = strings("segment_terminator");
    for row in 0..batch.num_rows() {
        if ids.is_null(row)
            || elements.is_null(row)
            || element_seps.is_null(row)
            || segment_seps.is_null(row)
        {
            return Err(error("COPY TO X12 writing columns must not be NULL"));
        }
        let values = elements.value(row);
        let values = values.as_string::<i32>();
        if values.null_count() != 0 {
            return Err(error(
                "X12 elements must not contain NULL; use an empty string for an empty element",
            ));
        }
        let element_sep = element_seps.value(row).as_bytes();
        let segment_sep = segment_seps.value(row).as_bytes();
        if element_sep.len() != 1 || segment_sep.len() != 1 {
            return Err(error("X12 delimiters must each be one ASCII byte"));
        }
        let values: Vec<_> = values.iter().map(|v| v.unwrap().to_string()).collect();
        let encoded = writer
            .push(ids.value(row), &values, element_sep[0], segment_sep[0])
            .map_err(error)?;
        output
            .write_all(&encoded)
            .map_err(|e| error(format!("write X12 output: {e}")))?;
    }
    Ok(batch.num_rows() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use vgi::buffering::BufferingParams;
    use vgi::storage::{FsStorage, FunctionStorage, MemoryStorage};

    fn params(storage: Arc<dyn FunctionStorage>) -> BufferingParams {
        BufferingParams {
            execution_id: b"copy-test".to_vec(),
            storage,
            output_schema: Arc::new(Schema::empty()),
            arguments: Default::default(),
            settings: Default::default(),
            secrets: Default::default(),
            attach_opaque_data: None,
            batch_index: None,
            copy_to: None,
            input_schema: Some(read_x12::output_schema()),
            logs: Default::default(),
        }
    }

    fn fixture() -> RecordBatch {
        let rows = read_x12::rows(&[source::InputDoc {
            source_path: None,
            bytes: include_bytes!("../../../data/era_835.835").to_vec(),
        }]);
        arrow_io::build_batch(&read_x12::output_schema(), &rows).unwrap()
    }

    fn write(params: &BufferingParams, batch: &RecordBatch) -> Result<()> {
        X12CopyTo.write(
            &CopyToWriteContext {
                path: "unused.edi",
                options: &params.arguments,
                storage: &params.storage,
                execution_id: &params.execution_id,
                params,
            },
            batch,
        )
    }

    fn close(params: &BufferingParams, path: &Path) -> Result<i64> {
        X12CopyTo.close(&CopyToCloseContext {
            path: path.to_str().unwrap(),
            options: &params.arguments,
            storage: &params.storage,
            execution_id: &params.execution_id,
            input_schema: params.input_schema.as_ref(),
            params,
        })
    }

    #[test]
    fn shards_survive_new_storage_handle_and_preserve_batch_order() {
        let dir = tempfile::tempdir().unwrap();
        let execution_id = dir.path().to_str().unwrap().as_bytes().to_vec();
        let mut p = params(Arc::new(FsStorage::new()));
        p.execution_id = execution_id.clone();
        let batch = fixture();
        // One row per shard crosses both envelope and storage pagination boundaries.
        for i in 0..batch.num_rows() {
            write(&p, &batch.slice(i, 1)).unwrap();
        }
        drop(p);
        let mut p = params(Arc::new(FsStorage::new()));
        p.execution_id = execution_id;
        let target = dir.path().join("out.edi");
        assert_eq!(close(&p, &target).unwrap(), batch.num_rows() as i64);
        let rows = x12_core::serialization::read_segments(&std::fs::read(target).unwrap());
        assert_eq!(rows.len(), batch.num_rows());
        assert_eq!(rows[3].segment.id(), "BPR");
        assert_eq!(rows.last().unwrap().segment.id(), "IEA");
        p.storage.clear(&p.execution_id);
    }

    #[test]
    fn invalid_output_preserves_existing_destination_and_removes_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = params(Arc::new(MemoryStorage::new()));
        let batch = fixture();
        write(&p, &batch.slice(0, batch.num_rows() - 1)).unwrap();
        let target = dir.path().join("out.edi");
        std::fs::write(&target, b"original").unwrap();
        assert!(close(&p, &target).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn empty_output_and_schema_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = params(Arc::new(MemoryStorage::new()));
        let target = dir.path().join("empty.edi");
        assert_eq!(close(&p, &target).unwrap(), 0);
        assert!(std::fs::read(&target).unwrap().is_empty());
        p.input_schema = Some(Arc::new(Schema::empty()));
        assert!(close(&p, &target).is_err());
    }

    #[test]
    fn null_elements_are_rejected() {
        use arrow_array::builder::{ListBuilder, StringBuilder};
        let batch = fixture();
        let mut builder = ListBuilder::new(StringBuilder::new());
        for _ in 0..batch.num_rows() {
            builder.values().append_null();
            builder.append(true);
        }
        let mut columns = batch.columns().to_vec();
        columns[batch.schema().index_of("elements").unwrap()] = Arc::new(builder.finish());
        let batch = RecordBatch::try_new(batch.schema(), columns).unwrap();
        assert!(
            encode_batch(&mut X12Writer::new(false), &batch, &mut Vec::new())
                .unwrap_err()
                .to_string()
                .contains("must not contain NULL")
        );
    }
}
