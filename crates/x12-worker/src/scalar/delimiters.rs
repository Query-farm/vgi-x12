//! `delimiters(content) -> STRUCT(element, segment, component, repetition)` —
//! sniff the four delimiter bytes out of inline interchange content (the ISA for
//! X12, the UNA / defaults for EDIFACT). Each field is a 1-char VARCHAR;
//! `repetition` is NULL when the interchange uses no repetition separator. A row
//! whose content is not a recognizable interchange yields a NULL struct.

use std::sync::Arc;

use arrow_array::builder::StringBuilder;
use arrow_array::{ArrayRef, RecordBatch, StructArray};
use arrow_buffer::NullBuffer;
use arrow_schema::DataType;
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};
use x12_core::delimiters::{self, Delimiters, Family};

use crate::arrow_io::{content_bytes, delimiters_struct_fields};

pub struct DelimitersFn;

/// Sniff the delimiters for `bytes`, branching on the EDI family.
fn sniff(bytes: &[u8]) -> Option<Delimiters> {
    match delimiters::detect_family(bytes) {
        Family::X12 => delimiters::sniff_x12(bytes),
        Family::Edifact => Some(x12_core::edifact::edifact_delimiters(bytes)),
        Family::Unknown => None,
    }
}

fn byte_str(b: u8) -> String {
    (b as char).to_string()
}

impl ScalarFunction for DelimitersFn {
    fn name(&self) -> &str {
        "delimiters"
    }

    fn metadata(&self) -> FunctionMetadata {
        FunctionMetadata {
            description: "Sniff the X12/EDIFACT delimiters from inline content into a \
                          STRUCT(element, segment, component, repetition)"
                .into(),
            examples: vec![FunctionExample {
                sql: "SELECT x12.main.delimiters('ISA*00*          *00*          *ZZ*S              *ZZ*R              *240101*1200*^*00501*000000001*0*P*:~');".into(),
                description: "Sniff the four delimiter bytes from an ISA header.".into(),
                expected_output: None,
            }],
            tags: {
                let mut tags = crate::meta::object_tags(
                    "Sniff EDI Delimiters",
                    "Discover the delimiter bytes governing an inline X12 or UN/EDIFACT interchange \
                     and return them as a STRUCT(element, segment, component, repetition), each a \
                     1-char VARCHAR. For X12 the bytes are read deterministically from the \
                     fixed-width ISA (element = byte after 'ISA', component = ISA16, segment = byte \
                     after ISA16, repetition = ISA11 or NULL for the version-4010 'U' placeholder); \
                     for EDIFACT they come from the optional UNA service-string advice (or the \
                     EDIFACT defaults). `repetition` is NULL when no repetition separator is in \
                     use. Content that is not a recognizable interchange returns a NULL struct \
                     (never an error). Pass the content from a column, e.g. via read_text().",
                    "Sniff the EDI delimiters from inline content into a STRUCT(element, segment, \
                     component, repetition); `repetition` is NULL when unused, and unrecognized \
                     content returns NULL.",
                    "delimiters, sniff, x12, edifact, isa, una, element separator, segment \
                     terminator, component separator, repetition separator, struct",
                    "Interchange sniffers",
                );
                tags.push((
                    "vgi.executable_examples".into(),
                    r#"[
  {
    "description": "Sniff the delimiters from a canonical ISA header.",
    "sql": "SELECT (x12.main.delimiters('ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~')).element AS element_sep"
  }
]"#
                    .into(),
                ));
                tags
            },
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![ArgSpec::column(
            "content",
            0,
            "varchar",
            "Inline interchange content whose delimiters to sniff — e.g. the first bytes of an ISA \
             or UNA/UNB header. Typically the content column from read_text(...). NULL or \
             unrecognized content yields a NULL struct.",
        )]
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Struct(
            delimiters_struct_fields(),
        )))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let col = batch.column(0);
        let rows = batch.num_rows();
        let mut element = StringBuilder::new();
        let mut segment = StringBuilder::new();
        let mut component = StringBuilder::new();
        let mut repetition = StringBuilder::new();
        let mut valid: Vec<bool> = Vec::with_capacity(rows);

        for i in 0..rows {
            let d = match content_bytes(col, i)? {
                Some(bytes) => sniff(&bytes),
                None => None,
            };
            match d {
                Some(d) => {
                    element.append_value(byte_str(d.element));
                    segment.append_value(byte_str(d.segment));
                    component.append_value(byte_str(d.component));
                    match d.repetition {
                        Some(r) => repetition.append_value(byte_str(r)),
                        None => repetition.append_null(),
                    }
                    valid.push(true);
                }
                None => {
                    element.append_null();
                    segment.append_null();
                    component.append_null();
                    repetition.append_null();
                    valid.push(false);
                }
            }
        }

        let arrays: Vec<ArrayRef> = vec![
            Arc::new(element.finish()),
            Arc::new(segment.finish()),
            Arc::new(component.finish()),
            Arc::new(repetition.finish()),
        ];
        let out: ArrayRef = Arc::new(StructArray::new(
            delimiters_struct_fields(),
            arrays,
            Some(NullBuffer::from(valid)),
        ));
        RecordBatch::try_new(params.output_schema.clone(), vec![out])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_io::test_support::run_scalar_text;
    use arrow_array::cast::AsArray;
    use arrow_array::Array;

    #[test]
    fn sniffs_isa_and_nulls() {
        let isa = "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~";
        let out = run_scalar_text(&DelimitersFn, &[Some(isa), Some("garbage"), None]).unwrap();
        let s = out.as_struct();
        let element = s.column(0).as_string::<i32>();
        let repetition = s.column(3).as_string::<i32>();
        assert!(!out.is_null(0));
        assert_eq!(element.value(0), "*");
        assert_eq!(repetition.value(0), "^");
        assert!(out.is_null(1), "unrecognized content → NULL struct");
        assert!(out.is_null(2), "NULL input → NULL struct");
    }
}
