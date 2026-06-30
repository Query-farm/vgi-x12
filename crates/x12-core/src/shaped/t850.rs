//! `read_850` — purchase order (ST01 `850`). Parent = `PO1` (line item); header
//! from `BEG`; the most-recent `N1` party and `PER` contact are carried down onto
//! each line. One row per `PO1`. Positional, public-segment-ID only.

use super::{Col, Row, RowB};
use crate::delimiters::Delimiters;
use crate::envelope::Transaction;

pub const COLS: &[Col] = &[
    Col {
        name: "beg_purpose_code",
        comment: "BEG01 — transaction set purpose code (raw).",
    },
    Col {
        name: "beg_po_type",
        comment: "BEG02 — purchase order type code (raw).",
    },
    Col {
        name: "beg_po_number",
        comment: "BEG03 — purchase order number.",
    },
    Col {
        name: "beg_date",
        comment: "BEG05 — purchase order date (raw CCYYMMDD).",
    },
    Col {
        name: "n1_entity_code",
        comment: "N101 of the most recent N1 — entity identifier code (raw, e.g. BT/ST/SU/VN).",
    },
    Col {
        name: "n1_name",
        comment: "N102 of the most recent N1 — party name (raw).",
    },
    Col {
        name: "n1_id_qualifier",
        comment: "N103 of the most recent N1 — identification code qualifier (raw).",
    },
    Col {
        name: "n1_id",
        comment: "N104 of the most recent N1 — party identification code.",
    },
    Col {
        name: "per_contact_function",
        comment: "PER01 of the most recent PER — contact function code (raw).",
    },
    Col {
        name: "per_name",
        comment: "PER02 of the most recent PER — contact name.",
    },
    Col {
        name: "per_comm_qualifier",
        comment: "PER03 of the most recent PER — communication number qualifier (raw).",
    },
    Col {
        name: "per_comm_number",
        comment: "PER04 of the most recent PER — communication number.",
    },
    Col {
        name: "po1_line_number",
        comment: "PO101 — assigned line item identifier.",
    },
    Col {
        name: "po1_quantity",
        comment: "PO102 — quantity ordered (raw VARCHAR).",
    },
    Col {
        name: "po1_uom",
        comment: "PO103 — unit/basis of measurement code (raw).",
    },
    Col {
        name: "po1_unit_price",
        comment: "PO104 — unit price (raw VARCHAR).",
    },
    Col {
        name: "po1_product_qualifier",
        comment: "PO106 — product/service ID qualifier (raw).",
    },
    Col {
        name: "po1_product_id",
        comment: "PO107 — product/service identifier.",
    },
];

#[derive(Default, Clone)]
struct State {
    beg_purpose: String,
    beg_type: String,
    beg_number: String,
    beg_date: String,
    n1_code: String,
    n1_name: String,
    n1_id_qual: String,
    n1_id: String,
    per_func: String,
    per_name: String,
    per_comm_qual: String,
    per_comm_num: String,
}

impl State {
    fn apply(&self, b: &mut RowB) {
        b.set("beg_purpose_code", &self.beg_purpose);
        b.set("beg_po_type", &self.beg_type);
        b.set("beg_po_number", &self.beg_number);
        b.set("beg_date", &self.beg_date);
        b.set("n1_entity_code", &self.n1_code);
        b.set("n1_name", &self.n1_name);
        b.set("n1_id_qualifier", &self.n1_id_qual);
        b.set("n1_id", &self.n1_id);
        b.set("per_contact_function", &self.per_func);
        b.set("per_name", &self.per_name);
        b.set("per_comm_qualifier", &self.per_comm_qual);
        b.set("per_comm_number", &self.per_comm_num);
    }
}

pub fn rows(tx: &Transaction, _d: &Delimiters) -> Vec<Row> {
    let mut out = Vec::new();
    let mut st = State::default();

    for seg in tx.body() {
        match seg.id() {
            "BEG" => {
                st.beg_purpose = seg.elem(1).to_string();
                st.beg_type = seg.elem(2).to_string();
                st.beg_number = seg.elem(3).to_string();
                st.beg_date = seg.elem(5).to_string();
            }
            "N1" => {
                st.n1_code = seg.elem(1).to_string();
                st.n1_name = seg.elem(2).to_string();
                st.n1_id_qual = seg.elem(3).to_string();
                st.n1_id = seg.elem(4).to_string();
                // A fresh N1 starts a fresh party; clear any stale PER contact.
                st.per_func = String::new();
                st.per_name = String::new();
                st.per_comm_qual = String::new();
                st.per_comm_num = String::new();
            }
            "PER" => {
                st.per_func = seg.elem(1).to_string();
                st.per_name = seg.elem(2).to_string();
                st.per_comm_qual = seg.elem(3).to_string();
                st.per_comm_num = seg.elem(4).to_string();
            }
            "PO1" => {
                let mut b = RowB::new();
                st.apply(&mut b);
                b.set("po1_line_number", seg.elem(1));
                b.set("po1_quantity", seg.elem(2));
                b.set("po1_uom", seg.elem(3));
                b.set("po1_unit_price", seg.elem(4));
                b.set("po1_product_qualifier", seg.elem(6));
                b.set("po1_product_id", seg.elem(7));
                out.push(b.build(COLS));
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::tests::{extract, idx, one_tx};

    #[test]
    fn extracts_850_lines() {
        let body = "BEG*00*SA*PO9988**20240101~\
                    N1*ST*ACME WAREHOUSE*92*DC07~\
                    PER*BD*JANE BUYER*TE*5551234~\
                    PO1*1*10*EA*4.50**VP*WIDGET-A~\
                    PO1*2*5*EA*9.99**VP*WIDGET-B~";
        let rows = extract("read_850", &one_tx("000000004", "850", body));
        let ci = |c| idx("read_850", c);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][ci("beg_po_number")].as_deref(), Some("PO9988"));
        assert_eq!(rows[0][ci("beg_date")].as_deref(), Some("20240101"));
        assert_eq!(rows[0][ci("n1_entity_code")].as_deref(), Some("ST"));
        assert_eq!(rows[0][ci("n1_name")].as_deref(), Some("ACME WAREHOUSE"));
        assert_eq!(rows[0][ci("per_name")].as_deref(), Some("JANE BUYER"));
        assert_eq!(rows[0][ci("per_comm_number")].as_deref(), Some("5551234"));
        assert_eq!(rows[0][ci("po1_line_number")].as_deref(), Some("1"));
        assert_eq!(rows[0][ci("po1_quantity")].as_deref(), Some("10"));
        assert_eq!(rows[0][ci("po1_unit_price")].as_deref(), Some("4.50"));
        assert_eq!(rows[0][ci("po1_product_id")].as_deref(), Some("WIDGET-A"));
        assert_eq!(rows[1][ci("po1_product_id")].as_deref(), Some("WIDGET-B"));
    }
}
