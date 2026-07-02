//! `x12.edifact_segments` / `x12.edifact_envelope` — the UN/EDIFACT variant,
//! kept separate so the two delimiter conventions never cross-contaminate. The
//! release/escape byte is un-escaped during the explode.

use std::sync::Arc;

use arrow_schema::{DataType, Schema, SchemaRef};
use vgi::table_function::{TableFunction, TableProducer};
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams};
use vgi_rpc::Result;

use crate::arrow_io::{commented, Cell};
use crate::source::{self, envelope_key_cells, envelope_key_fields};
use crate::table::RowsProducer;

// ---------------------------------------------------------------------------
// edifact_segments
// ---------------------------------------------------------------------------

pub struct EdifactSegments;

fn segments_schema() -> SchemaRef {
    let mut fields = envelope_key_fields();
    fields.push(commented(
        "segment_index",
        DataType::Int64,
        "0-based ordinal of this element's segment within its UNH..UNT message.",
    ));
    fields.push(commented(
        "segment_id",
        DataType::Utf8,
        "Raw EDIFACT segment tag (e.g. 'BGM', 'DTM', 'NAD').",
    ));
    fields.push(commented(
        "element_index",
        DataType::Int32,
        "1-based data element position within the segment.",
    ));
    fields.push(commented(
        "component_index",
        DataType::Int32,
        "1-based component (sub-element) position; NULL unless the element is composite.",
    ));
    fields.push(commented(
        "value",
        DataType::Utf8,
        "The raw element/component value, with the EDIFACT release char applied.",
    ));
    Arc::new(Schema::new(fields))
}

impl TableFunction for EdifactSegments {
    fn name(&self) -> &str {
        "edifact_segments"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "EDIFACT Segments & Elements",
            "Explode UN/EDIFACT interchange file(s) into one row per element. The input is a file \
             path/glob or inline content (auto-detected by the ISA/UNA/UNB prefix). The delimiters \
             come from the optional UNA service-string advice (overriding the EDIFACT defaults), \
             and the release/escape byte (default '?') is un-escaped so an escaped delimiter is \
             treated as data. Each row carries the envelope keys (interchange_ctrl = UNB05 control \
             reference, transaction_ctrl = UNH01, transaction_type = UNH02 message type) plus \
             source_path, segment_index, the raw segment_id (tag), the 1-based element_index, a \
             component_index (NULL unless the element is composite), and the raw value.",
            "One row per UN/EDIFACT element, with UNA-driven delimiters, release-char un-escaping, \
             and envelope keys carried down. Reads a file path/glob or inline content.",
            "edifact, un/edifact, edi, segments, elements, explode, una, unb, unh, orders, \
             invoic, desadv, release character, table function",
            "UN/EDIFACT",
        );
        tags.push((
            "vgi.result_columns_md".into(),
            crate::meta::result_columns_md(&segments_schema()),
        ));
        FunctionMetadata {
            description: "Explode a UN/EDIFACT interchange into one row per element".into(),
            examples: vec![crate::meta::edifact_example(
                "edifact_segments",
                "segment_id, element_index, value",
                crate::meta::EXAMPLE_EDIFACT,
                "Explode an inline EDIFACT ORDERS interchange into one row per element.",
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
            output_schema: segments_schema(),
            opaque_data: Vec::new(),
        })
    }

    fn producer(&self, params: &ProcessParams) -> Result<Box<dyn TableProducer>> {
        let docs = source::resolve(&params.arguments)?;
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        for doc in &docs {
            for inter in x12_core::edifact::parse_edifact(&doc.bytes) {
                let comp = inter.delimiters.component as char;
                let ictrl = inter.control().to_string();
                for msg in &inter.messages {
                    let gctrl = msg.group_ref.clone().unwrap_or_default();
                    let tctrl = msg.control().to_string();
                    let ttype = msg.message_type(inter.delimiters.component).to_string();
                    for (seg_idx, seg) in msg.segments.iter().enumerate() {
                        for (n, raw) in seg.data_elements().iter().enumerate() {
                            let element_index = n as i32 + 1;
                            if raw.contains(comp) {
                                for (ci, c) in raw.split(comp).enumerate() {
                                    rows.push(edi_row(
                                        &ictrl,
                                        &gctrl,
                                        &tctrl,
                                        &ttype,
                                        &doc.source_path,
                                        seg_idx,
                                        seg.id(),
                                        element_index,
                                        Some(ci as i32 + 1),
                                        c,
                                    ));
                                }
                            } else {
                                rows.push(edi_row(
                                    &ictrl,
                                    &gctrl,
                                    &tctrl,
                                    &ttype,
                                    &doc.source_path,
                                    seg_idx,
                                    seg.id(),
                                    element_index,
                                    None,
                                    raw,
                                ));
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

#[allow(clippy::too_many_arguments)]
fn edi_row(
    ictrl: &str,
    gctrl: &str,
    tctrl: &str,
    ttype: &str,
    source_path: &Option<String>,
    seg_idx: usize,
    seg_id: &str,
    element_index: i32,
    component_index: Option<i32>,
    value: &str,
) -> Vec<Cell> {
    let mut row = envelope_key_cells(ictrl, gctrl, tctrl, ttype, source_path);
    row.push(Cell::I64(Some(seg_idx as i64)));
    row.push(Cell::s(seg_id));
    row.push(Cell::I32(Some(element_index)));
    row.push(Cell::I32(component_index));
    row.push(Cell::Str(Some(value.to_string())));
    row
}

// ---------------------------------------------------------------------------
// edifact_envelope
// ---------------------------------------------------------------------------

pub struct EdifactEnvelope;

fn envelope_schema() -> SchemaRef {
    let fields = vec![
        commented(
            "interchange_ctrl",
            DataType::Utf8,
            "UNB05 — interchange control reference.",
        ),
        commented(
            "syntax_id",
            DataType::Utf8,
            "UNB01-1 — syntax identifier (e.g. 'UNOA').",
        ),
        commented(
            "syntax_version",
            DataType::Utf8,
            "UNB01-2 — syntax version number.",
        ),
        commented(
            "sender_id",
            DataType::Utf8,
            "UNB02-1 — interchange sender identification.",
        ),
        commented(
            "receiver_id",
            DataType::Utf8,
            "UNB03-1 — interchange recipient identification.",
        ),
        commented(
            "unb_date",
            DataType::Utf8,
            "UNB04-1 — preparation date (YYMMDD).",
        ),
        commented(
            "unb_time",
            DataType::Utf8,
            "UNB04-2 — preparation time (HHMM).",
        ),
        commented(
            "group_ctrl",
            DataType::Utf8,
            "UNG05 — functional group reference (NULL when no UNG).",
        ),
        commented(
            "transaction_ctrl",
            DataType::Utf8,
            "UNH01 — message reference number.",
        ),
        commented(
            "transaction_type",
            DataType::Utf8,
            "UNH02-1 — message type ('ORDERS','INVOIC',…).",
        ),
        commented(
            "message_version",
            DataType::Utf8,
            "UNH02-2 — message version number.",
        ),
        commented(
            "message_release",
            DataType::Utf8,
            "UNH02-3 — message release number.",
        ),
        commented(
            "controlling_agency",
            DataType::Utf8,
            "UNH02-4 — controlling agency (e.g. 'UN').",
        ),
        commented(
            "segment_count",
            DataType::Int64,
            "Actual UNH..UNT segment count (what UNT01 must equal).",
        ),
        commented(
            "unt_count_ok",
            DataType::Boolean,
            "Whether UNT01 equals the actual segment count; NULL if no UNT.",
        ),
        commented(
            "source_path",
            DataType::Utf8,
            "The file the row came from; NULL for inline content.",
        ),
    ];
    Arc::new(Schema::new(fields))
}

impl TableFunction for EdifactEnvelope {
    fn name(&self) -> &str {
        "edifact_envelope"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "EDIFACT Envelope",
            "Summarize UN/EDIFACT interchange file(s) as one row per UNH message — the routing / \
             triage view. The single `input` argument is a file path/glob or inline content \
             (auto-detected). Columns expose the UNB interchange header (control reference, syntax \
             id/version, sender/recipient, date/time), the optional UNG group reference, the UNH \
             message header (reference number, message type / version / release / controlling \
             agency), the actual UNH..UNT segment_count, and the unt_count_ok structural flag (NULL \
             when no UNT). Structural validation only.",
            "One row per UN/EDIFACT message with UNB/UNG/UNH metadata and the UNT structural count \
             flag. Reads a file path/glob or inline content.",
            "edifact, un/edifact, envelope, unb, ung, unh, unt, message type, control \
             reference, orders, invoic, routing, table function",
            "UN/EDIFACT",
        );
        tags.push((
            "vgi.result_columns_md".into(),
            crate::meta::result_columns_md(&envelope_schema()),
        ));
        FunctionMetadata {
            description:
                "One row per UN/EDIFACT message: UNB/UNG/UNH envelope metadata + UNT validity"
                    .into(),
            examples: vec![crate::meta::edifact_example(
                "edifact_envelope",
                "interchange_ctrl, syntax_id, sender_id, receiver_id",
                crate::meta::EXAMPLE_EDIFACT,
                "Summarize an inline EDIFACT ORDERS interchange as one envelope row.",
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
            output_schema: envelope_schema(),
            opaque_data: Vec::new(),
        })
    }

    fn producer(&self, params: &ProcessParams) -> Result<Box<dyn TableProducer>> {
        let docs = source::resolve(&params.arguments)?;
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        for doc in &docs {
            for inter in x12_core::edifact::parse_edifact(&doc.bytes) {
                let comp = inter.delimiters.component;
                let unb = &inter.unb;
                for msg in &inter.messages {
                    let unh = &msg.unh;
                    let row = vec![
                        Cell::s_opt(inter.control()),
                        Cell::s_opt(unb.elem_comp(1, 1, comp)),
                        Cell::s_opt(unb.elem_comp(1, 2, comp)),
                        Cell::s_opt(unb.elem_comp(2, 1, comp)),
                        Cell::s_opt(unb.elem_comp(3, 1, comp)),
                        Cell::s_opt(unb.elem_comp(4, 1, comp)),
                        Cell::s_opt(unb.elem_comp(4, 2, comp)),
                        Cell::Str(msg.group_ref.clone().filter(|s| !s.is_empty())),
                        Cell::s_opt(unh.elem(1)),
                        Cell::s_opt(msg.message_type(comp)),
                        Cell::s_opt(unh.elem_comp(2, 2, comp)),
                        Cell::s_opt(unh.elem_comp(2, 3, comp)),
                        Cell::s_opt(unh.elem_comp(2, 4, comp)),
                        Cell::I64(Some(msg.segments.len() as i64)),
                        Cell::Bool(msg.unt_count_ok()),
                        Cell::Str(doc.source_path.clone()),
                    ];
                    rows.push(row);
                }
            }
        }
        Ok(Box::new(RowsProducer::new(
            params.output_schema.clone(),
            rows,
        )))
    }
}
