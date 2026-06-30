//! `x12.segments_elements(input)` — the workhorse: one row
//! per element (further split into composite components and repetitions).

use std::sync::Arc;

use arrow_schema::{DataType, Schema, SchemaRef};
use vgi::table_function::{TableFunction, TableProducer};
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams};
use vgi_rpc::Result;
use x12_core::delimiters::Delimiters;
use x12_core::segment::split_repetitions;

use crate::arrow_io::{commented, Cell};
use crate::source::{self, envelope_key_cells, envelope_key_fields};
use crate::table::RowsProducer;

pub struct SegmentsElements;

fn output_schema() -> SchemaRef {
    let mut fields = envelope_key_fields();
    fields.push(commented(
        "segment_index",
        DataType::Int64,
        "0-based ordinal of this element's segment within its ST..SE transaction.",
    ));
    fields.push(commented(
        "segment_id",
        DataType::Utf8,
        "Raw segment identifier (e.g. 'CLM', 'NM1').",
    ));
    fields.push(commented(
        "element_index",
        DataType::Int32,
        "1-based element position (so CLP02 is element_index = 2).",
    ));
    fields.push(commented(
        "component_index",
        DataType::Int32,
        "1-based sub-element position; NULL unless the element is a composite.",
    ));
    fields.push(commented(
        "repetition_index",
        DataType::Int32,
        "1-based repetition occurrence; NULL unless the element repeats.",
    ));
    fields.push(commented(
        "value",
        DataType::Utf8,
        "The raw element/component value (EDIFACT release char applied).",
    ));
    Arc::new(Schema::new(fields))
}

/// Explode one raw element value into its leaf (repetition, component, value)
/// rows under the interchange's delimiters.
fn explode_value(raw: &str, delims: &Delimiters) -> Vec<(Option<i32>, Option<i32>, String)> {
    let reps = split_repetitions(raw, delims);
    let multi_rep = reps.len() > 1;
    let comp = delims.component as char;
    let mut out = Vec::new();
    for (ri, rep) in reps.iter().enumerate() {
        let rep_idx = if multi_rep { Some(ri as i32 + 1) } else { None };
        if rep.contains(comp) {
            for (ci, c) in rep.split(comp).enumerate() {
                out.push((rep_idx, Some(ci as i32 + 1), c.to_string()));
            }
        } else {
            out.push((rep_idx, None, rep.clone()));
        }
    }
    out
}

impl TableFunction for SegmentsElements {
    fn name(&self) -> &str {
        "segments_elements"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "X12 Segments & Elements",
            "Fully explode ANSI ASC X12 EDI file(s) into one row per element — the workhorse \
             generic view. The single `input` argument is a file path/glob or inline content \
             (auto-detected by the ISA/UNA/UNB magic prefix). Each row carries the four envelope \
             keys plus source_path, the segment_index, raw segment_id, the 1-based element_index \
             (CLP02 is element_index 2), a component_index (NULL unless the element is a composite \
             split on the sub-element separator), a repetition_index (NULL unless the element \
             repeats on the repetition separator), and the raw value. Delimiters are sniffed per \
             interchange from the ISA. Filter with WHERE segment_id = 'CLM' to pull specific \
             segments.",
            "One row per X12 element, split into composite components and repetitions, with \
             envelope keys carried down. The `input` is a file path/glob or inline content.",
            "x12, edi, elements, explode, element, component, repetition, segments_elements, \
             parse edi, 837, 835, claim, healthcare edi, table function",
        );
        tags.push((
            "vgi.result_columns_md".into(),
            crate::meta::result_columns_md(&output_schema()),
        ));
        FunctionMetadata {
            description:
                "Explode an X12 interchange into one row per element (component/repetition split)"
                    .into(),
            examples: vec![crate::meta::table_example(
                "segments_elements",
                "segment_id, element_index, component_index, value",
                "837",
                "CLM*ACCT777*500***11:B:1*Y~HI*ABK:Z1234~SV1*HC:99213*200*UN*1~",
                "Explode an inline 837 CLM segment into element/component rows (CLM05 is the \
                 composite '11:B:1').",
            )],
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
                let delims = inter.delimiters;
                let ictrl = inter.isa.elem(13).to_string();
                for group in &inter.groups {
                    let gctrl = group.gs.elem(6).to_string();
                    for tx in &group.transactions {
                        let tctrl = tx.control().to_string();
                        let ttype = tx.type_code().to_string();
                        for (seg_idx, seg) in tx.segments.iter().enumerate() {
                            let id = seg.id().to_string();
                            for (n, raw) in seg.data_elements().iter().enumerate() {
                                let element_index = n as i32 + 1;
                                for (rep_idx, comp_idx, value) in explode_value(raw, &delims) {
                                    let mut row = envelope_key_cells(
                                        &ictrl,
                                        &gctrl,
                                        &tctrl,
                                        &ttype,
                                        &doc.source_path,
                                    );
                                    row.push(Cell::I64(Some(seg_idx as i64)));
                                    row.push(Cell::s(id.clone()));
                                    row.push(Cell::I32(Some(element_index)));
                                    row.push(Cell::I32(comp_idx));
                                    row.push(Cell::I32(rep_idx));
                                    row.push(Cell::Str(Some(value)));
                                    rows.push(row);
                                }
                            }
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
