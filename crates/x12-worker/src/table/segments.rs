//! `x12.segments(input)` — one row per segment, with the raw
//! positional element values as a `LIST<VARCHAR>`. Envelope keys carried down.

use std::sync::Arc;

use arrow_schema::{DataType, Schema, SchemaRef};
use vgi::table_function::{TableFunction, TableProducer};
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams};
use vgi_rpc::Result;

use crate::arrow_io::{commented, varchar_list, Cell};
use crate::source::{self, envelope_key_cells, envelope_key_fields};
use crate::table::RowsProducer;

pub struct Segments;

fn output_schema() -> SchemaRef {
    let mut fields = envelope_key_fields();
    fields.push(commented(
        "segment_index",
        DataType::Int64,
        "0-based ordinal of this segment within its ST..SE transaction (ST is 0).",
    ));
    fields.push(commented(
        "segment_id",
        DataType::Utf8,
        "Raw segment identifier (e.g. 'CLP', 'NM1', 'HL').",
    ));
    fields.push(commented(
        "elements",
        varchar_list(),
        "Positional data-element values of the segment (the values after the segment ID).",
    ));
    fields.push(commented(
        "byte_offset",
        DataType::Int64,
        "Start offset of the segment in the source document (debug/replay).",
    ));
    Arc::new(Schema::new(fields))
}

impl TableFunction for Segments {
    fn name(&self) -> &str {
        "segments"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "X12 Segments",
            "Explode ANSI ASC X12 EDI file(s) into one row per segment. The input is a file \
             path/glob (e.g. '/data/*.edi') or inline content (auto-detected by the ISA/UNA/UNB \
             prefix). Each row carries the four envelope keys (interchange_ctrl=ISA13, \
             group_ctrl=GS06, transaction_ctrl=ST02, transaction_type=ST01) plus source_path, the \
             0-based segment_index within its ST..SE transaction, the raw segment_id, an elements \
             list of the positional data-element values, and the byte_offset. The interchange's \
             own delimiters are sniffed from its fixed-width ISA. Malformed input never aborts the \
             query.",
            "One row per X12 segment, with the positional element values as a list and the \
             envelope keys carried down. Reads a file path/glob or inline content.",
            "x12, edi, segments, explode, segment, parse edi, 837, 835, healthcare edi, \
             elements, isa, gs, st, table function",
            "Segment & element explode",
        );
        tags.push((
            "vgi.result_columns_schema".into(),
            crate::meta::result_columns_schema(&output_schema()),
        ));
        let examples = vec![crate::meta::table_example(
            "segments",
            "segment_index, segment_id, elements",
            "837",
            "BHT*0019*00*REF01*20240101*1200*CH~NM1*85*2*BILLING CLINIC*****XX*1122334455~\
             CLM*ACCT777*500***11:B:1*Y~HI*ABK:Z1234~SV1*HC:99213*200*UN*1~",
            "Explode an inline 837 claim interchange into one row per segment.",
        )];
        tags.push((
            "vgi.example_queries".into(),
            crate::meta::example_queries_tag(&examples),
        ));
        FunctionMetadata {
            description: "Explode an X12 interchange into one row per segment (elements as a LIST)"
                .into(),
            examples,
            tags,
            ..Default::default()
        }
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
        let docs = source::resolve(&params.arguments)?;
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        for doc in &docs {
            for inter in x12_core::envelope::parse_x12(&doc.bytes) {
                let ictrl = inter.isa.elem(13).to_string();
                for group in &inter.groups {
                    let gctrl = group.gs.elem(6).to_string();
                    for tx in &group.transactions {
                        let tctrl = tx.control().to_string();
                        let ttype = tx.type_code().to_string();
                        for (idx, seg) in tx.segments.iter().enumerate() {
                            let mut row = envelope_key_cells(
                                &ictrl,
                                &gctrl,
                                &tctrl,
                                &ttype,
                                &doc.source_path,
                            );
                            row.push(Cell::I64(Some(idx as i64)));
                            row.push(Cell::s(seg.id()));
                            row.push(Cell::StrList(seg.data_elements().to_vec()));
                            row.push(Cell::I64(Some(seg.byte_offset as i64)));
                            rows.push(row);
                        }
                    }
                }
            }
        }
        Ok(Box::new(RowsProducer::new(
            params.output_schema.clone(),
            rows,
        )))
    }
}
