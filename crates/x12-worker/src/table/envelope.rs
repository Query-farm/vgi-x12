//! `x12.envelope(input)` — one row per ST transaction: the
//! ISA/GS/ST routing-and-triage metadata plus the structural validity flags.

use std::sync::Arc;

use arrow_schema::{DataType, Schema, SchemaRef};
use vgi::table_function::{TableFunction, TableProducer};
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams};
use vgi_rpc::Result;

use crate::arrow_io::{commented, Cell};
use crate::source::{self};
use crate::table::RowsProducer;

pub struct Envelope;

/// Map an `Option<bool>` validity flag to a boolean cell (NULL when unknown,
/// e.g. a truncated interchange with no trailer).
fn flag(v: Option<bool>) -> Cell {
    Cell::Bool(v)
}

fn output_schema() -> SchemaRef {
    let fields = vec![
        commented(
            "interchange_ctrl",
            DataType::Utf8,
            "ISA13 — interchange control number.",
        ),
        commented(
            "sender_qual",
            DataType::Utf8,
            "ISA05 — interchange sender ID qualifier.",
        ),
        commented(
            "sender_id",
            DataType::Utf8,
            "ISA06 — interchange sender ID (15-char, raw).",
        ),
        commented(
            "receiver_qual",
            DataType::Utf8,
            "ISA07 — interchange receiver ID qualifier.",
        ),
        commented(
            "receiver_id",
            DataType::Utf8,
            "ISA08 — interchange receiver ID (raw).",
        ),
        commented(
            "isa_date",
            DataType::Utf8,
            "ISA09 — interchange date (YYMMDD).",
        ),
        commented(
            "isa_time",
            DataType::Utf8,
            "ISA10 — interchange time (HHMM).",
        ),
        commented(
            "isa_version",
            DataType::Utf8,
            "ISA12 — interchange control version number (e.g. '00501').",
        ),
        commented(
            "usage_indicator",
            DataType::Utf8,
            "ISA15 — usage indicator ('P' production, 'T' test).",
        ),
        commented(
            "group_ctrl",
            DataType::Utf8,
            "GS06 — functional group control number.",
        ),
        commented(
            "gs_functional_id",
            DataType::Utf8,
            "GS01 — functional identifier code (e.g. HP/HC/HS/HB/PO/FA).",
        ),
        commented(
            "gs_app_sender",
            DataType::Utf8,
            "GS02 — application sender code.",
        ),
        commented(
            "gs_app_receiver",
            DataType::Utf8,
            "GS03 — application receiver code.",
        ),
        commented(
            "gs_date",
            DataType::Utf8,
            "GS04 — functional group date (CCYYMMDD).",
        ),
        commented(
            "gs_version",
            DataType::Utf8,
            "GS08 — version / release / industry identifier (raw string).",
        ),
        commented(
            "transaction_ctrl",
            DataType::Utf8,
            "ST02 — transaction set control number.",
        ),
        commented(
            "transaction_type",
            DataType::Utf8,
            "ST01 — transaction set identifier ('835','837',…).",
        ),
        commented(
            "st_impl_ref",
            DataType::Utf8,
            "ST03 — implementation convention reference (optional).",
        ),
        commented(
            "segment_count",
            DataType::Int64,
            "Actual ST..SE segment count (what SE01 must equal).",
        ),
        commented(
            "se_count_ok",
            DataType::Boolean,
            "Whether SE01 equals the actual segment count; NULL if no SE.",
        ),
        commented(
            "ge_count_ok",
            DataType::Boolean,
            "Whether GE01 equals the group's transaction count; NULL if no GE.",
        ),
        commented(
            "iea_count_ok",
            DataType::Boolean,
            "Whether IEA01 equals the interchange's group count; NULL if no IEA.",
        ),
        commented(
            "se_ctrl_match",
            DataType::Boolean,
            "Whether SE02 matches ST02; NULL if no SE.",
        ),
        commented(
            "ge_ctrl_match",
            DataType::Boolean,
            "Whether GE02 matches GS06; NULL if no GE.",
        ),
        commented(
            "iea_ctrl_match",
            DataType::Boolean,
            "Whether IEA02 matches ISA13; NULL if no IEA.",
        ),
        commented(
            "source_path",
            DataType::Utf8,
            "The file the row came from; NULL for inline content.",
        ),
    ];
    Arc::new(Schema::new(fields))
}

impl TableFunction for Envelope {
    fn name(&self) -> &str {
        "envelope"
    }

    fn metadata(&self) -> FunctionMetadata {
        let mut tags = crate::meta::object_tags(
            "X12 Envelope",
            "Summarize ANSI ASC X12 EDI file(s) as one row per ST transaction set — the routing / \
             triage view. The single `input` argument is a file path/glob or inline content \
             (auto-detected by the ISA/UNA/UNB magic prefix). Columns expose the ISA interchange \
             header (control number, sender/receiver qualifiers + IDs, date/time, version, P/T \
             usage indicator), the GS functional group header (functional id, app sender/receiver, \
             date, GS08 version), the ST header (control number, transaction type, impl reference), \
             the actual ST..SE segment_count, and the structural validity flags se_count_ok / \
             ge_count_ok / iea_count_ok / se_ctrl_match / ge_ctrl_match / iea_ctrl_match (NULL when \
             the matching trailer is absent — e.g. a truncated interchange). This is structural \
             validation only (segment counts + control-number matching) — no loop or code-set \
             rules.",
            "One row per X12 transaction set with ISA/GS/ST metadata and the SE/GE/IEA structural \
             count + control-match validity flags. Reads a file path/glob or inline content.",
            "x12, edi, envelope, isa, gs, st, routing, triage, control number, validation, \
             se count, ge count, iea, sender, receiver, usage indicator, table function",
        );
        tags.push((
            "vgi.result_columns_md".into(),
            crate::meta::result_columns_md(&output_schema()),
        ));
        FunctionMetadata {
            description:
                "One row per ST transaction: ISA/GS/ST envelope metadata + structural validity flags"
                    .into(),
            examples: vec![crate::meta::table_example(
                "envelope",
                "interchange_ctrl, transaction_type, segment_count, se_count_ok",
                "837",
                "BHT*0019*00*REF01*20240101*1200*CH~CLM*ACCT777*500***11:B:1*Y~\
                 SV1*HC:99213*200*UN*1~",
                "Summarize an inline 837 interchange as one envelope row with structural \
                 validity flags.",
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
                let isa = &inter.isa;
                for group in &inter.groups {
                    let gs = &group.gs;
                    for tx in &group.transactions {
                        let row = vec![
                            Cell::s_opt(isa.elem(13)),
                            Cell::s_opt(isa.elem(5)),
                            Cell::s_opt(isa.elem(6).trim()),
                            Cell::s_opt(isa.elem(7)),
                            Cell::s_opt(isa.elem(8).trim()),
                            Cell::s_opt(isa.elem(9)),
                            Cell::s_opt(isa.elem(10)),
                            Cell::s_opt(isa.elem(12)),
                            Cell::s_opt(isa.elem(15)),
                            Cell::s_opt(gs.elem(6)),
                            Cell::s_opt(gs.elem(1)),
                            Cell::s_opt(gs.elem(2)),
                            Cell::s_opt(gs.elem(3)),
                            Cell::s_opt(gs.elem(4)),
                            Cell::s_opt(gs.elem(8)),
                            Cell::s_opt(tx.control()),
                            Cell::s_opt(tx.type_code()),
                            Cell::s_opt(tx.st.elem(3)),
                            Cell::I64(Some(tx.segment_count() as i64)),
                            flag(tx.se_count_ok()),
                            flag(group.ge_count_ok()),
                            flag(inter.iea_count_ok()),
                            flag(tx.se_ctrl_match()),
                            flag(group.ge_ctrl_match()),
                            flag(inter.iea_ctrl_match()),
                            Cell::Str(doc.source_path.clone()),
                        ];
                        rows.push(row);
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
