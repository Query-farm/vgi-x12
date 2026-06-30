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
        let doc_llm = format!(
            "Shaped, relational view of X12 transaction set {st} ({fn_name}). `path` may be a glob \
             (e.g. '/data/*.{st}'); or inline VARCHAR/BLOB content (auto-detected by the ISA/UNA/UNB prefix). Every row \
             carries the four envelope keys (interchange_ctrl, group_ctrl, transaction_ctrl, \
             transaction_type) plus source_path, then this set's positional columns. Columns are \
             named by the PUBLIC segment ID and the element position only (e.g. clp_total_paid = \
             CLP04); raw codes are surfaced verbatim — code-value translation needs your own \
             licensed X12 reference. A parent segment starts each logical row group; repeating \
             children fan out to one row each sharing the parent's keys, so re-aggregate with \
             GROUP BY on the parent id. Structural / positional only — no TR3 loop or code-set \
             rules.",
            st = st,
            fn_name = self.def.fn_name,
        );
        let doc_md = format!(
            "Shaped positional view of X12 `{st}`. Envelope keys + positional columns named by \
             public segment ID and element position; raw codes verbatim. Reads a file path/glob \
             or inline content.",
            st = st
        );
        let keywords = format!(
            "x12, edi, {st}, read_{st}, shaped, healthcare edi, claim, remittance, eligibility, \
             purchase order, acknowledgement, parse edi, table function",
            st = st
        );
        let mut tags = crate::meta::object_tags(&title, &doc_llm, &doc_md, &keywords);
        tags.push((
            "vgi.result_columns_md".into(),
            crate::meta::result_columns_md(&self.output_schema()),
        ));
        FunctionMetadata {
            description: format!("Shaped positional view of X12 transaction set {st}"),
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
