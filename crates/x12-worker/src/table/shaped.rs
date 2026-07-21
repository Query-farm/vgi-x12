//! The shaped views (`read_835` / `read_837` / `read_270` / `read_271` /
//! `read_850` / `read_997` / `read_999`) — one generic [`TableFunction`] adapter
//! driving the per-set positional extractors in `x12_core::shaped`. Each view
//! emits the five envelope keys followed by the set's positional data columns.

use std::sync::Arc;

use arrow_schema::{Schema, SchemaRef};
use vgi::table_function::{TableFunction, TableProducer};
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams};
use vgi_rpc::Result;
use x12_core::shaped::ShapedDef;

use crate::arrow_io::{commented, Cell};
use crate::source::{self, envelope_key_cells, envelope_key_fields};
use crate::table::RowsProducer;

/// A shaped view bound to one [`ShapedDef`].
pub struct Shaped {
    def: &'static ShapedDef,
}

impl Shaped {
    pub fn new(def: &'static ShapedDef) -> Self {
        Shaped { def }
    }

    fn output_schema(&self) -> SchemaRef {
        let mut fields = envelope_key_fields();
        for col in self.def.cols {
            fields.push(commented(
                col.name,
                arrow_schema::DataType::Utf8,
                col.comment,
            ));
        }
        Arc::new(Schema::new(fields))
    }
}

impl TableFunction for Shaped {
    fn name(&self) -> &str {
        self.def.fn_name
    }

    fn metadata(&self) -> FunctionMetadata {
        let st = self.def.st01;
        let title = format!("X12 {} ({})", self.def.fn_name, st);
        let hints = set_hints(st);
        let doc_llm = format!(
            "Shaped, relational view of X12 transaction set {st} ({fn_name}) — {set_desc}. The \
             single `input` argument takes a file path or glob (e.g. '/data/*.{st}'), or inline \
             `VARCHAR`/`BLOB` content (auto-detected by the ISA/UNA/UNB prefix). Every row carries \
             the \
             five envelope keys (interchange_ctrl, group_ctrl, transaction_ctrl, transaction_type, \
             source_path), then this set's positional columns — for example {sample_cols}. Columns \
             are named by the PUBLIC segment ID and element position only (e.g. {illustration}); \
             raw codes are surfaced verbatim, so code-value translation needs your own licensed \
             X12 reference. Each {parent} parent starts a logical row group; repeating children \
             fan out to one row each sharing the parent's keys, so re-aggregate with GROUP BY on \
             the parent id. Structural / positional only — no TR3 loop or code-set rules.",
            st = st,
            fn_name = self.def.fn_name,
            set_desc = hints.set_desc,
            sample_cols = hints.sample_cols,
            illustration = hints.illustration,
            parent = hints.parent,
        );
        let doc_md = format!(
            "Shaped positional view of X12 `{st}` ({set_desc}). Carries the five envelope keys \
             plus positional columns named by public segment ID and element position (e.g. \
             {illustration}) — such as {sample_cols}; raw codes verbatim. Reads a file path/glob, \
             inline `VARCHAR` content, or a `BLOB`.",
            st = st,
            set_desc = hints.set_desc,
            illustration = hints.illustration,
            sample_cols = hints.sample_cols,
        );
        let keywords = format!(
            "x12, edi, {st}, read_{st}, shaped, healthcare edi, claim, remittance, eligibility, \
             purchase order, acknowledgement, parse edi, table function",
            st = st
        );
        let mut tags = crate::meta::object_tags(
            &title,
            &doc_llm,
            &doc_md,
            &keywords,
            "Shaped transaction views",
        );
        tags.push((
            "vgi.result_columns_schema".into(),
            crate::meta::result_columns_schema(&self.output_schema()),
        ));
        let examples = match example_for(st) {
            Some((select, body)) => vec![crate::meta::table_example(
                self.def.fn_name,
                select,
                st,
                body,
                &format!(
                    "Project an inline {st} interchange through the {} shaped view.",
                    self.def.fn_name
                ),
            )],
            None => Vec::new(),
        };
        if !examples.is_empty() {
            tags.push((
                "vgi.example_queries".into(),
                crate::meta::example_queries_tag(&examples),
            ));
        }
        FunctionMetadata {
            description: format!("Shaped positional view of X12 transaction set {st}"),
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
            output_schema: self.output_schema(),
            opaque_data: Vec::new(),
        })
    }

    fn producer(&self, params: &ProcessParams) -> Result<Box<dyn TableProducer>> {
        let docs = source::resolve(&params.arguments)?;
        let def = self.def;
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        for doc in &docs {
            for inter in x12_core::envelope::parse_x12(&doc.bytes) {
                let delims = inter.delimiters;
                let ictrl = inter.isa.elem(13).to_string();
                for group in &inter.groups {
                    let gctrl = group.gs.elem(6).to_string();
                    for tx in &group.transactions {
                        if tx.type_code() != def.st01 {
                            continue;
                        }
                        let tctrl = tx.control().to_string();
                        let ttype = tx.type_code().to_string();
                        for shaped_row in (def.extract)(tx, &delims) {
                            let mut row = envelope_key_cells(
                                &ictrl,
                                &gctrl,
                                &tctrl,
                                &ttype,
                                &doc.source_path,
                            );
                            for v in shaped_row {
                                row.push(Cell::Str(v));
                            }
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

/// Per-set doc hints so each shaped view's `doc_llm`/`doc_md` describes the
/// ACTUAL set (its parent segment, representative real columns, and a
/// name→position illustration drawn from that set) rather than a shared 835
/// template. `set_desc` uses only general industry terms — no copyrighted TR3
/// loop or code-value names. `sample_cols` and `illustration` reference real
/// columns emitted by the set's extractor in `x12_core::shaped`.
struct SetHints {
    set_desc: &'static str,
    parent: &'static str,
    illustration: &'static str,
    sample_cols: &'static str,
}

fn set_hints(st01: &str) -> SetHints {
    match st01 {
        "835" => SetHints {
            set_desc: "an 835 remittance advice / ERA",
            parent: "CLP",
            illustration: "clp_total_paid = CLP04",
            sample_cols: "clp_claim_id, clp_total_paid, payer_name, cas_reason_code, svc_paid",
        },
        "837" => SetHints {
            set_desc: "an 837 health-care claim",
            parent: "CLM",
            illustration: "clm_total_charge = CLM02",
            sample_cols: "billing_provider_npi, subscriber_id, clm_total_charge, sv1_procedure",
        },
        "270" => SetHints {
            set_desc: "a 270 eligibility / benefit inquiry",
            parent: "HL",
            illustration: "hl_level_code = HL03",
            sample_cols: "hl_id, hl_level_code, entity_name, eb_service_type",
        },
        "271" => SetHints {
            set_desc: "a 271 eligibility / benefit response",
            parent: "HL",
            illustration: "eb_plan_description = EB05",
            sample_cols: "hl_id, entity_name, eb_plan_description, eb_benefit_amount",
        },
        "850" => SetHints {
            set_desc: "an 850 purchase order",
            parent: "PO1",
            illustration: "po1_quantity = PO102",
            sample_cols: "po1_line_number, po1_product_id, po1_quantity, po1_unit_price, n1_name",
        },
        "997" => SetHints {
            set_desc: "a 997 functional acknowledgment",
            parent: "AK2",
            illustration: "ak5_status = AK501",
            sample_cols: "ak2_transaction_control, ak5_status, ak9_status, ak3_segment_id",
        },
        "999" => SetHints {
            set_desc: "a 999 implementation acknowledgment",
            parent: "AK2",
            illustration: "ik5_status = IK501",
            sample_cols: "ak2_transaction_control, ik5_status, ik3_segment_id, ak9_status",
        },
        _ => SetHints {
            set_desc: "a shaped X12 transaction set",
            parent: "the set's",
            illustration: "clp_total_paid = CLP04",
            sample_cols: "the set's positional data columns",
        },
    }
}

/// A runnable example for the shaped view of transaction set `st01`: the
/// `(select, body)` pair feeds [`crate::meta::table_example`], which wraps `body`
/// in a valid interchange envelope. The bodies mirror the committed `data/`
/// fixtures and the `x12-core` extractor unit tests, so each is guaranteed to
/// produce rows when the linter executes it inline (no file needed).
fn example_for(st01: &str) -> Option<(&'static str, &'static str)> {
    let pair = match st01 {
        "835" => (
            "clp_claim_id, clp_total_paid, payer_name",
            "BPR*I*1500*C*ACH~TRN*1*TRACE0001*1512345678~\
             NM1*PR*2*ACME HEALTH PLAN*****PI*PAYER001~\
             NM1*PE*2*WELLNESS CLINIC LLC*****XX*1999999999~\
             CLP*PCN1001*1*500*400*100*MC*CCN0001*11~CAS*CO*45*100~\
             SVC*HC:99213*200*160**1~DTM*232*20240110~\
             CLP*PCN1002*4*1000*0*0*MC*CCN0002*11~",
        ),
        "837" => (
            "billing_provider_npi, subscriber_id, clm_place_of_service",
            "BHT*0019*00*REF01*20240101*1200*CH~\
             NM1*85*2*BILLING CLINIC*****XX*1122334455~\
             NM1*IL*1*DOE*JOHN****MI*MEMBER123~SBR*P*18**GROUP PLAN~\
             NM1*QC*1*DOE*JANE~CLM*ACCT777*500***11:B:1*Y~\
             HI*ABK:Z1234~SV1*HC:99213*200*UN*1~DTP*472*D8*20240105~",
        ),
        "270" => (
            "hl_id, hl_level_code, entity_name",
            "HL*1**20*1~NM1*PR*2*ACME PAYER*****PI*PAYER01~\
             HL*2*1*21*1~NM1*1P*2*PROVIDER GRP*****XX*1444444444~\
             HL*3*2*22*0~NM1*IL*1*DOE*JOHN****MI*MEMBER99~\
             TRN*1*TRACE70*9999999999~EQ*30~",
        ),
        "271" => (
            "hl_id, entity_name, eb_plan_description",
            "HL*1**20*1~NM1*PR*2*ACME PAYER*****PI*PAYER01~\
             HL*2*1*21*1~NM1*1P*2*PROVIDER GRP*****XX*1444444444~\
             HL*3*2*22*0~NM1*IL*1*DOE*JOHN****MI*MEMBER99~\
             TRN*2*TRACE55*9999999999~EB*1*IND*30**GOLD PPO~\
             EB*B*IND*30****27.5~DTP*291*D8*20240101~",
        ),
        "850" => (
            "po1_line_number, po1_product_id, po1_quantity",
            "BEG*00*SA*PO9988**20240101~N1*ST*ACME WAREHOUSE*92*DC07~\
             PER*BD*JANE BUYER*TE*5551234~PO1*1*10*EA*4.50**VP*WIDGET-A~\
             PO1*2*5*EA*9.99**VP*WIDGET-B~",
        ),
        "997" => (
            "ak2_transaction_control, ak5_status, ak3_segment_id",
            "AK1*HC*1~AK2*837*0001~AK3*CLM*22**8~AK4*1*1028*1~AK5*E~\
             AK2*837*0002~AK5*A~AK9*P*2*2*1~",
        ),
        "999" => (
            "ak2_transaction_control, ik3_segment_id, ik5_status",
            "AK1*HC*1~AK2*837*0001~IK3*NM1*8**8~IK4*2*1037*7~IK5*R~\
             AK9*R*1*1*0~",
        ),
        _ => return None,
    };
    Some(pair)
}
