//! The Arrow boundary: a small typed [`Cell`] model plus a generic
//! schema-driven [`build_batch`] so every table function shares one
//! row-to-RecordBatch path, and helpers for reading scalar input cells and the
//! `delimiters` STRUCT return type.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::builder::{
    BooleanBuilder, Int32Builder, Int64Builder, ListBuilder, StringBuilder,
};
use arrow_array::cast::AsArray;
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Fields, SchemaRef};
use vgi_rpc::{Result, RpcError};

/// One output cell. The variant must match the column's declared Arrow type or
/// [`build_batch`] errors (a programming error, surfaced clearly).
#[derive(Debug, Clone)]
pub enum Cell {
    Str(Option<String>),
    I64(Option<i64>),
    I32(Option<i32>),
    Bool(Option<bool>),
    /// A non-null `LIST<VARCHAR>` of element values.
    StrList(Vec<String>),
}

impl Cell {
    /// A non-null string cell from any displayable value.
    pub fn s(v: impl Into<String>) -> Cell {
        Cell::Str(Some(v.into()))
    }
    /// A string cell that is NULL when `v` is empty.
    pub fn s_opt(v: &str) -> Cell {
        if v.is_empty() {
            Cell::Str(None)
        } else {
            Cell::Str(Some(v.to_string()))
        }
    }
}

/// A field carrying a `comment` (surfaced via `duckdb_columns().comment`).
pub fn commented(name: &str, ty: DataType, comment: &str) -> Field {
    Field::new(name, ty, true).with_metadata(HashMap::from([(
        "comment".to_string(),
        comment.to_string(),
    )]))
}

/// Build a `RecordBatch` from `rows` (each a full row of [`Cell`]s in column
/// order) against `schema`, dispatching per column on the field's Arrow type.
pub fn build_batch(schema: &SchemaRef, rows: &[Vec<Cell>]) -> Result<RecordBatch> {
    let ncols = schema.fields().len();
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(ncols);
    for (c, field) in schema.fields().iter().enumerate() {
        columns.push(build_column(field.data_type(), rows, c)?);
    }
    RecordBatch::try_new(schema.clone(), columns)
        .map_err(|e| RpcError::runtime_error(e.to_string()))
}

fn type_err(col: usize, want: &str) -> RpcError {
    RpcError::runtime_error(format!("column {col}: cell type does not match {want}"))
}

fn build_column(ty: &DataType, rows: &[Vec<Cell>], c: usize) -> Result<ArrayRef> {
    match ty {
        DataType::Utf8 => {
            let mut b = StringBuilder::new();
            for row in rows {
                match &row[c] {
                    Cell::Str(Some(s)) => b.append_value(s),
                    Cell::Str(None) => b.append_null(),
                    _ => return Err(type_err(c, "VARCHAR")),
                }
            }
            Ok(Arc::new(b.finish()))
        }
        DataType::Int64 => {
            let mut b = Int64Builder::new();
            for row in rows {
                match &row[c] {
                    Cell::I64(Some(v)) => b.append_value(*v),
                    Cell::I64(None) => b.append_null(),
                    _ => return Err(type_err(c, "BIGINT")),
                }
            }
            Ok(Arc::new(b.finish()))
        }
        DataType::Int32 => {
            let mut b = Int32Builder::new();
            for row in rows {
                match &row[c] {
                    Cell::I32(Some(v)) => b.append_value(*v),
                    Cell::I32(None) => b.append_null(),
                    _ => return Err(type_err(c, "INTEGER")),
                }
            }
            Ok(Arc::new(b.finish()))
        }
        DataType::Boolean => {
            let mut b = BooleanBuilder::new();
            for row in rows {
                match &row[c] {
                    Cell::Bool(Some(v)) => b.append_value(*v),
                    Cell::Bool(None) => b.append_null(),
                    _ => return Err(type_err(c, "BOOLEAN")),
                }
            }
            Ok(Arc::new(b.finish()))
        }
        DataType::List(_) => {
            let mut b = ListBuilder::new(StringBuilder::new());
            for row in rows {
                match &row[c] {
                    Cell::StrList(items) => {
                        for it in items {
                            b.values().append_value(it);
                        }
                        b.append(true);
                    }
                    _ => return Err(type_err(c, "LIST<VARCHAR>")),
                }
            }
            Ok(Arc::new(b.finish()))
        }
        other => Err(RpcError::runtime_error(format!(
            "column {c}: unsupported output type {other:?}"
        ))),
    }
}

/// The `LIST<VARCHAR>` element type used by the `elements` column.
pub fn varchar_list() -> DataType {
    DataType::List(Arc::new(Field::new("item", DataType::Utf8, true)))
}

/// Read the raw bytes of a VARCHAR or BLOB cell at `row`, or `None` if null —
/// so a scalar can accept inline content either as text or as bytes.
pub fn content_bytes(col: &ArrayRef, row: usize) -> Result<Option<Vec<u8>>> {
    if col.is_null(row) {
        return Ok(None);
    }
    Ok(Some(match col.data_type() {
        DataType::Utf8 => col.as_string::<i32>().value(row).as_bytes().to_vec(),
        DataType::LargeUtf8 => col.as_string::<i64>().value(row).as_bytes().to_vec(),
        DataType::Binary => col.as_binary::<i32>().value(row).to_vec(),
        DataType::LargeBinary => col.as_binary::<i64>().value(row).to_vec(),
        other => {
            return Err(RpcError::value_error(format!(
                "expected VARCHAR or BLOB content, got {other:?}"
            )))
        }
    }))
}

/// The fixed `STRUCT(element, segment, component, repetition)` fields the
/// `delimiters` scalar returns — each a 1-char VARCHAR (repetition is NULL when
/// the interchange uses no repetition separator). Shared by `on_bind` + `process`.
pub fn delimiters_struct_fields() -> Fields {
    Fields::from(vec![
        Field::new("element", DataType::Utf8, true),
        Field::new("segment", DataType::Utf8, true),
        Field::new("component", DataType::Utf8, true),
        Field::new("repetition", DataType::Utf8, true),
    ])
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use vgi::arguments::Arguments;
    use vgi::{BindParams, ProcessParams, ScalarFunction};

    /// Build a `ProcessParams` carrying the given output schema and arguments.
    pub fn process_params(output_schema: SchemaRef, arguments: Arguments) -> ProcessParams {
        ProcessParams {
            substream_id: None,
            if_none_match: None,
            if_modified_since: None,
            output_schema,
            input_schema: None,
            execution_id: Vec::new(),
            init_opaque_data: Vec::new(),
            arguments,
            settings: Default::default(),
            secrets: Default::default(),
            auth_principal: None,
            projection_ids: None,
            pushdown_filters: None,
            join_keys: Vec::new(),
            storage: None,
            order_by_column: None,
            order_by_direction: None,
            order_by_null_order: None,
            order_by_limit: None,
            tablesample_percentage: None,
            tablesample_seed: None,
            attach_opaque_data: None,
            at_unit: None,
            at_value: None,
            copy_from: None,
        }
    }

    /// Build a single-column Utf8 input batch from optional strings.
    pub fn text_batch(rows: &[Option<&str>]) -> RecordBatch {
        let mut b = StringBuilder::new();
        for r in rows {
            match r {
                Some(s) => b.append_value(s),
                None => b.append_null(),
            }
        }
        let arr: ArrayRef = Arc::new(b.finish());
        let schema = Arc::new(arrow_schema::Schema::new(vec![Field::new(
            "content",
            DataType::Utf8,
            true,
        )]));
        RecordBatch::try_new(schema, vec![arr]).unwrap()
    }

    /// Run a scalar over a single-column VARCHAR batch, returning the result column.
    pub fn run_scalar_text<F: ScalarFunction>(f: &F, rows: &[Option<&str>]) -> Result<ArrayRef> {
        let batch = text_batch(rows);
        let bind = BindParams {
            input_schema: Some(batch.schema()),
            ..Default::default()
        };
        let bound = f.on_bind(&bind)?;
        let params = process_params(bound.output_schema.clone(), Arguments::default());
        Ok(f.process(&params, &batch)?.column(0).clone())
    }
}
