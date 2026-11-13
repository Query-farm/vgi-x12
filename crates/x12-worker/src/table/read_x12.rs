//! Complete segment representation shared by SELECT and COPY FROM.

use std::sync::Arc;

use arrow_schema::{DataType, Schema, SchemaRef};
use vgi::table_function::{TableFunction, TableProducer};
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams};
use vgi_rpc::Result;

use super::RowsProducer;
use crate::arrow_io::{commented, varchar_list, Cell};
use crate::source::{self, InputDoc};

pub struct ReadX12;

pub fn output_schema() -> SchemaRef {
    let mut fields = source::envelope_key_fields();
    fields.extend([
        commented("document_index", DataType::Int64, "Zero-based document ordinal within this invocation; distinguishes inline and repeated input paths."),
        commented("segment_index", DataType::Int64, "Zero-based segment ordinal across the whole document, including all envelopes. Order by document_index, segment_index before COPY TO."),
        commented("segment_id", DataType::Utf8, "Raw segment identifier, including ISA, GS, ST, SE, GE, and IEA."),
        commented("elements", varchar_list(), "All positional element values, preserving empty values, padding, composites, and repetitions as unsplit text."),
        commented("byte_offset", DataType::Int64, "Start byte offset in the source document; informational after edits."),
        commented("element_separator", DataType::Utf8, "Element separator in force for this row's interchange."),
        commented("segment_terminator", DataType::Utf8, "Segment terminator in force for this row's interchange. Component and repetition separators remain in ISA16 and ISA11."),
    ]);
    Arc::new(Schema::new(fields))
}

pub fn rows(docs: &[InputDoc]) -> Vec<Vec<Cell>> {
    let mut rows = Vec::new();
    for (document_index, doc) in docs.iter().enumerate() {
        for (segment_index, row) in x12_core::serialization::read_segments(&doc.bytes)
            .into_iter()
            .enumerate()
        {
            let mut cells = source::envelope_key_cells(
                &row.interchange_ctrl,
                &row.group_ctrl,
                &row.transaction_ctrl,
                &row.transaction_type,
                &doc.source_path,
            );
            cells.extend([
                Cell::I64(Some(document_index as i64)),
                Cell::I64(Some(segment_index as i64)),
                Cell::s(row.segment.id()),
                Cell::StrList(row.segment.data_elements().to_vec()),
                Cell::I64(Some(row.segment.byte_offset as i64)),
                Cell::s((row.delimiters.element as char).to_string()),
                Cell::s((row.delimiters.segment as char).to_string()),
            ]);
            rows.push(cells);
        }
    }
    rows
}

pub fn metadata() -> FunctionMetadata {
    let description = "Read every X12 segment, including ISA/GS/GE/IEA envelopes, with positional \
        elements, document/segment ordering, source offsets, and per-interchange delimiters. \
        Envelope keys apply only to the current scope (NULL outside it). Accepts paths/globs, \
        inline content, or bytes. Use this representation for COPY FROM/TO FORMAT 'x12.x12'. \
        Order by document_index, segment_index before writing. Text round trips preserve element \
        values, empty elements, and ISA padding, but normalize inter-segment whitespace. \
        The reader is permissive; the writer requires complete, structurally valid ASCII X12. \
        Shaped read_* projections cannot reconstruct an interchange.";
    let mut tags = crate::meta::object_tags(
        "Complete X12 Segments",
        description,
        "## Complete X12 segments\n\n\
         Use this reader to edit or inspect an entire interchange before COPY export. Unlike \
         the transaction-only segments view, every ISA, GS, GE, and IEA row remains available.\n\n\
         **Ordering.** Each invocation assigns a document_index to each resolved input and a \
         document-wide segment_index to every row. Order by both when exporting. Keep a separate \
         import identifier when appending several calls to one staging table.\n\n\
         **Values and delimiters.** Positional elements retain empty strings, padding, composites, \
         and repetitions as unsplit text. The row carries its element separator and segment \
         terminator; ISA11 and ISA16 retain the other delimiters. A new ISA starts a new delimiter \
         scope, including concatenated interchanges with different separators.\n\n\
         **Envelope keys.** Keys are NULL outside their scope. A trailer carries the keys of the \
         scope it closes. Source paths are NULL for inline content; byte offsets refer to the \
         original document and become informational after edits.\n\n\
         **Round trips.** The reader is permissive and materializes its input. COPY export checks \
         complete ASCII envelopes and control numbers, with optional trailer-count recalculation. \
         Inter-segment whitespace is normalized; binary payloads and EDIFACT writing are unsupported.",
        "x12, edi, read_x12, copy, round trip, segments, envelopes, delimiters",
        "Segment & element explode",
    );
    let examples = vec![crate::meta::table_example(
        "read_x12",
        "segment_index, segment_id, elements",
        "837",
        "CLM*ACCT1*500~",
        "Read all segments of an inline X12 interchange, including its envelopes.",
    )];
    tags.push((
        "vgi.result_columns_schema".into(),
        crate::meta::result_columns_schema(&output_schema()),
    ));
    tags.push((
        "vgi.example_queries".into(),
        crate::meta::example_queries_tag(&examples),
    ));
    FunctionMetadata {
        description:
            "Read complete X12 segment rows, including envelopes, ordering, and delimiter metadata."
                .into(),
        examples,
        tags,
        ..Default::default()
    }
}

impl TableFunction for ReadX12 {
    fn name(&self) -> &str {
        "read_x12"
    }
    fn metadata(&self) -> FunctionMetadata {
        metadata()
    }
    fn argument_specs(&self) -> Vec<ArgSpec> {
        source::input_arg_specs()
    }
    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse {
            output_schema: output_schema(),
            opaque_data: Vec::new(),
        })
    }
    fn producer(&self, params: &ProcessParams) -> Result<Box<dyn TableProducer>> {
        Ok(Box::new(RowsProducer::new(
            params.output_schema.clone(),
            rows(&source::resolve(&params.arguments)?),
        )))
    }
}
