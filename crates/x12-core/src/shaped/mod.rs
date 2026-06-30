//! Positional shaped extractors over **public segment IDs**.
//!
//! Each shaped view is a flat, relational projection of one transaction set
//! (`835`, `837`, `270`, `271`, `850`, `997`, `999`). A **parent** segment
//! (`CLP`, `CLM`, `HL`, `PO1`, `AK2`) starts a logical row group; repeating
//! **children** under it fan out to one row each, all sharing the parent's
//! columns — a classic header/detail layout the caller re-aggregates with
//! `GROUP BY`. `NM1`/`N1` loops are pivoted by their entity-identifier qualifier
//! into named parent columns.
//!
//! Column names use **only** the public segment ID and the element's position
//! (`clp_total_paid` = `CLP04`); raw codes are surfaced verbatim. No copyrighted
//! TR3 loop names or code-value descriptions are embedded.

mod acks;
mod t270_271;
mod t835;
mod t837;
mod t850;

use crate::delimiters::Delimiters;
use crate::envelope::Transaction;

/// One output column of a shaped view: its name and a human comment (surfaced
/// to DuckDB's `duckdb_columns().comment` and the metadata linter).
pub struct Col {
    pub name: &'static str,
    pub comment: &'static str,
}

/// A single shaped row: one optional VARCHAR per data column, in `cols` order.
/// `None` is SQL NULL. (Envelope keys are prepended by the worker, not here.)
pub type Row = Vec<Option<String>>;

/// A registered shaped view: the SQL function name, the transaction-set
/// identifier (ST01) it shapes, its data columns, and the extractor.
pub struct ShapedDef {
    pub fn_name: &'static str,
    pub st01: &'static str,
    pub cols: &'static [Col],
    pub extract: fn(&Transaction, &Delimiters) -> Vec<Row>,
}

/// All shaped views the worker exposes.
pub const REGISTRY: &[ShapedDef] = &[
    ShapedDef {
        fn_name: "read_835",
        st01: "835",
        cols: t835::COLS,
        extract: t835::rows,
    },
    ShapedDef {
        fn_name: "read_837",
        st01: "837",
        cols: t837::COLS,
        extract: t837::rows,
    },
    ShapedDef {
        fn_name: "read_270",
        st01: "270",
        cols: t270_271::COLS,
        extract: t270_271::rows,
    },
    ShapedDef {
        fn_name: "read_271",
        st01: "271",
        cols: t270_271::COLS,
        extract: t270_271::rows,
    },
    ShapedDef {
        fn_name: "read_850",
        st01: "850",
        cols: t850::COLS,
        extract: t850::rows,
    },
    ShapedDef {
        fn_name: "read_997",
        st01: "997",
        cols: acks::COLS_997,
        extract: acks::rows_997,
    },
    ShapedDef {
        fn_name: "read_999",
        st01: "999",
        cols: acks::COLS_999,
        extract: acks::rows_999,
    },
];

/// Look up a shaped view by its SQL function name.
pub fn def(fn_name: &str) -> Option<&'static ShapedDef> {
    REGISTRY.iter().find(|d| d.fn_name == fn_name)
}

/// A tiny order-independent row builder: `set` records a non-empty value under a
/// column name (empties become SQL NULL); `build` materializes the row in the
/// view's declared column order.
pub(crate) struct RowB {
    fields: Vec<(&'static str, String)>,
}

impl RowB {
    pub(crate) fn new() -> Self {
        RowB { fields: Vec::new() }
    }

    /// Record `value` for column `name`, unless it is empty (→ left NULL). A
    /// later `set` for the same column overrides an earlier one.
    pub(crate) fn set(&mut self, name: &'static str, value: &str) {
        if value.is_empty() {
            return;
        }
        if let Some(slot) = self.fields.iter_mut().find(|(k, _)| *k == name) {
            slot.1 = value.to_string();
        } else {
            self.fields.push((name, value.to_string()));
        }
    }

    /// Materialize the row against `cols` (column order), filling NULL for any
    /// column never `set`.
    pub(crate) fn build(&self, cols: &[Col]) -> Row {
        cols.iter()
            .map(|c| {
                self.fields
                    .iter()
                    .find(|(k, _)| *k == c.name)
                    .map(|(_, v)| v.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::parse_x12;

    /// Build a single-transaction interchange wrapper around a body and return
    /// its first transaction, for exercising the extractors directly.
    pub(crate) fn one_tx(isa_ctrl: &str, st01: &str, body: &str) -> Vec<u8> {
        format!(
            "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*{isa_ctrl}*0*P*:~\
             GS*XX*SEND*RECV*20240101*1200*1*X*005010~\
             ST*{st01}*0001~{body}SE*1*0001~GE*1*1~IEA*1*{isa_ctrl}~"
        )
        .into_bytes()
    }

    #[test]
    fn registry_unique_names() {
        let mut names: Vec<&str> = REGISTRY.iter().map(|d| d.fn_name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), REGISTRY.len(), "duplicate shaped fn name");
        assert!(def("read_835").is_some());
        assert!(def("nope").is_none());
    }

    #[test]
    fn rowb_orders_and_nulls() {
        const COLS: &[Col] = &[
            Col {
                name: "a",
                comment: "",
            },
            Col {
                name: "b",
                comment: "",
            },
            Col {
                name: "c",
                comment: "",
            },
        ];
        let mut r = RowB::new();
        r.set("c", "3");
        r.set("a", "1");
        r.set("a", "1b"); // override
        r.set("b", ""); // empty stays NULL
        let row = r.build(COLS);
        assert_eq!(
            row,
            vec![Some("1b".to_string()), None, Some("3".to_string())]
        );
    }

    /// Shared helper for the per-view test modules: parse a fixture and run the
    /// named view's extractor over its first transaction.
    pub(crate) fn extract(fn_name: &str, bytes: &[u8]) -> Vec<Row> {
        let d = def(fn_name).unwrap();
        let inters = parse_x12(bytes);
        let tx = &inters[0].groups[0].transactions[0];
        let delims = inters[0].delimiters;
        (d.extract)(tx, &delims)
    }

    /// Column index helper for assertions.
    pub(crate) fn idx(fn_name: &str, col: &str) -> usize {
        def(fn_name)
            .unwrap()
            .cols
            .iter()
            .position(|c| c.name == col)
            .unwrap_or_else(|| panic!("no column {col}"))
    }
}
