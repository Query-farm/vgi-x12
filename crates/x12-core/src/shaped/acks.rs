//! `read_997` / `read_999` — functional acknowledgements (inbound parsing only;
//! generation is out of scope, see the spec). Both use the AK1 group response +
//! AK2 transaction-set loop; 997 reports errors with AK3/AK4/AK5, 999 with
//! IK3/IK4/IK5. Parent = `AK2`; the group-level AK1/AK9 are carried down. Public
//! syntax only — the AK*/IK* element layouts carry no copyrighted TR3 prose.

use super::{Col, Row, RowB};
use crate::delimiters::Delimiters;
use crate::envelope::Transaction;
use crate::segment::Segment;

pub const COLS_997: &[Col] = &[
    Col {
        name: "ak1_functional_id",
        comment: "AK1*01 — functional identifier code of the acknowledged group (raw).",
    },
    Col {
        name: "ak1_group_control",
        comment: "AK1*02 — group control number being acknowledged.",
    },
    Col {
        name: "ak9_status",
        comment: "AK9*01 — functional group acknowledge code (raw, A/E/R/...).",
    },
    Col {
        name: "ak9_sets_included",
        comment: "AK9*02 — number of transaction sets included (raw).",
    },
    Col {
        name: "ak9_sets_received",
        comment: "AK9*03 — number of received transaction sets (raw).",
    },
    Col {
        name: "ak9_sets_accepted",
        comment: "AK9*04 — number of accepted transaction sets (raw).",
    },
    Col {
        name: "ak2_transaction_set_id",
        comment: "AK2*01 — transaction set identifier code being acknowledged (raw).",
    },
    Col {
        name: "ak2_transaction_control",
        comment: "AK2*02 — transaction set control number being acknowledged.",
    },
    Col {
        name: "ak5_status",
        comment: "AK5*01 — transaction set acknowledge code (raw, A/E/R/...).",
    },
    Col {
        name: "ak3_segment_id",
        comment: "AK3*01 — segment ID in error (raw).",
    },
    Col {
        name: "ak3_segment_position",
        comment: "AK3*02 — segment position in the transaction set (raw).",
    },
    Col {
        name: "ak3_error_code",
        comment: "AK3*04 — segment syntax error code (raw).",
    },
    Col {
        name: "ak4_element_position",
        comment: "AK4*01 — element position in the segment (raw, composite).",
    },
    Col {
        name: "ak4_error_code",
        comment: "AK4*03 — data element syntax error code (raw).",
    },
];

pub const COLS_999: &[Col] = &[
    Col {
        name: "ak1_functional_id",
        comment: "AK1*01 — functional identifier code of the acknowledged group (raw).",
    },
    Col {
        name: "ak1_group_control",
        comment: "AK1*02 — group control number being acknowledged.",
    },
    Col {
        name: "ak9_status",
        comment: "AK9*01 — functional group acknowledge code (raw).",
    },
    Col {
        name: "ak9_sets_included",
        comment: "AK9*02 — number of transaction sets included (raw).",
    },
    Col {
        name: "ak9_sets_received",
        comment: "AK9*03 — number of received transaction sets (raw).",
    },
    Col {
        name: "ak9_sets_accepted",
        comment: "AK9*04 — number of accepted transaction sets (raw).",
    },
    Col {
        name: "ak2_transaction_set_id",
        comment: "AK2*01 — transaction set identifier code being acknowledged (raw).",
    },
    Col {
        name: "ak2_transaction_control",
        comment: "AK2*02 — transaction set control number being acknowledged.",
    },
    Col {
        name: "ik5_status",
        comment: "IK5*01 — transaction set acknowledge code (raw).",
    },
    Col {
        name: "ik3_segment_id",
        comment: "IK3*01 — segment ID in error (raw).",
    },
    Col {
        name: "ik3_segment_position",
        comment: "IK3*02 — segment position in the transaction set (raw).",
    },
    Col {
        name: "ik3_error_code",
        comment: "IK3*04 — implementation segment syntax error code (raw).",
    },
    Col {
        name: "ik4_element_position",
        comment: "IK4*01 — element position in the segment (raw, composite).",
    },
    Col {
        name: "ik4_error_code",
        comment: "IK4*03 — implementation data element syntax error code (raw).",
    },
];

/// The segment IDs that vary between 997 and 999 (status / segment-error /
/// element-error) and the matching output column names.
struct Variant {
    status_seg: &'static str,
    seg_err: &'static str,
    elem_err: &'static str,
    status_col: &'static str,
    seg_id_col: &'static str,
    seg_pos_col: &'static str,
    seg_err_col: &'static str,
    elem_pos_col: &'static str,
    elem_err_col: &'static str,
    cols: &'static [Col],
}

const V997: Variant = Variant {
    status_seg: "AK5",
    seg_err: "AK3",
    elem_err: "AK4",
    status_col: "ak5_status",
    seg_id_col: "ak3_segment_id",
    seg_pos_col: "ak3_segment_position",
    seg_err_col: "ak3_error_code",
    elem_pos_col: "ak4_element_position",
    elem_err_col: "ak4_error_code",
    cols: COLS_997,
};

const V999: Variant = Variant {
    status_seg: "IK5",
    seg_err: "IK3",
    elem_err: "IK4",
    status_col: "ik5_status",
    seg_id_col: "ik3_segment_id",
    seg_pos_col: "ik3_segment_position",
    seg_err_col: "ik3_error_code",
    elem_pos_col: "ik4_element_position",
    elem_err_col: "ik4_error_code",
    cols: COLS_999,
};

pub fn rows_997(tx: &Transaction, _d: &Delimiters) -> Vec<Row> {
    rows_ack(tx, &V997)
}

pub fn rows_999(tx: &Transaction, _d: &Delimiters) -> Vec<Row> {
    rows_ack(tx, &V999)
}

fn rows_ack(tx: &Transaction, v: &Variant) -> Vec<Row> {
    let body = tx.body();
    // Group-level AK1 / AK9 are shared by every AK2 loop; AK9 trails the loops,
    // so prescan for both before walking the AK2 detail.
    let ak1 = body.iter().find(|s| s.id() == "AK1");
    let ak9 = body.iter().find(|s| s.id() == "AK9");

    let apply_group = |b: &mut RowB| {
        if let Some(ak1) = ak1 {
            b.set("ak1_functional_id", ak1.elem(1));
            b.set("ak1_group_control", ak1.elem(2));
        }
        if let Some(ak9) = ak9 {
            b.set("ak9_status", ak9.elem(1));
            b.set("ak9_sets_included", ak9.elem(2));
            b.set("ak9_sets_received", ak9.elem(3));
            b.set("ak9_sets_accepted", ak9.elem(4));
        }
    };

    // The status segment (AK5/IK5) trails the AK3/AK4 detail at the end of each
    // AK2 loop, so buffer one loop's rows and stamp the status when the loop
    // closes (at the next AK2, the trailing AK9, or end of body).
    let mut out = Vec::new();
    let mut ak2: Option<Segment> = None;
    let mut status = String::new();
    let mut loop_rows: Vec<RowB> = Vec::new();

    let make_base = |ak2: &Option<Segment>, apply_group: &dyn Fn(&mut RowB)| {
        let mut b = RowB::new();
        apply_group(&mut b);
        if let Some(ak2) = ak2 {
            b.set("ak2_transaction_set_id", ak2.elem(1));
            b.set("ak2_transaction_control", ak2.elem(2));
        }
        b
    };

    macro_rules! close_loop {
        () => {
            if ak2.is_some() {
                if loop_rows.is_empty() {
                    // Accepted set with no errors: one bare row carrying status.
                    loop_rows.push(make_base(&ak2, &apply_group));
                }
                for mut b in loop_rows.drain(..) {
                    b.set(v.status_col, &status);
                    out.push(b.build(v.cols));
                }
            }
            loop_rows.clear();
        };
    }

    for seg in body {
        let id = seg.id();
        if id == "AK2" {
            close_loop!();
            ak2 = Some(seg.clone());
            status = String::new();
        } else if id == v.status_seg {
            status = seg.elem(1).to_string();
        } else if id == v.seg_err {
            let mut b = make_base(&ak2, &apply_group);
            b.set(v.seg_id_col, seg.elem(1));
            b.set(v.seg_pos_col, seg.elem(2));
            b.set(v.seg_err_col, seg.elem(4));
            loop_rows.push(b);
        } else if id == v.elem_err {
            let mut b = make_base(&ak2, &apply_group);
            b.set(v.elem_pos_col, seg.elem(1));
            b.set(v.elem_err_col, seg.elem(3));
            loop_rows.push(b);
        }
    }
    close_loop!();
    out
}

#[cfg(test)]
mod tests {
    use super::super::tests::{extract, idx, one_tx};

    #[test]
    fn extracts_997_errors() {
        let body = "AK1*HC*1~\
                    AK2*837*0001~\
                    AK3*CLM*22**8~\
                    AK4*1*1028*1~\
                    AK5*E~\
                    AK2*837*0002~\
                    AK5*A~\
                    AK9*P*2*2*1~";
        let rows = extract("read_997", &one_tx("000000005", "997", body));
        let ci = |c| idx("read_997", c);
        // AK2#1 → AK3 row + AK4 row (2); AK2#2 → bare (1). Total 3.
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0][ci("ak1_functional_id")].as_deref(), Some("HC"));
        assert_eq!(rows[0][ci("ak9_status")].as_deref(), Some("P"));
        assert_eq!(rows[0][ci("ak9_sets_accepted")].as_deref(), Some("1"));
        assert_eq!(
            rows[0][ci("ak2_transaction_control")].as_deref(),
            Some("0001")
        );
        assert_eq!(rows[0][ci("ak5_status")].as_deref(), Some("E"));
        assert_eq!(rows[0][ci("ak3_segment_id")].as_deref(), Some("CLM"));
        assert_eq!(rows[0][ci("ak3_error_code")].as_deref(), Some("8"));
        assert_eq!(rows[1][ci("ak4_element_position")].as_deref(), Some("1"));
        assert_eq!(rows[1][ci("ak4_error_code")].as_deref(), Some("1"));
        // Second AK2 accepted, no errors.
        assert_eq!(
            rows[2][ci("ak2_transaction_control")].as_deref(),
            Some("0002")
        );
        assert_eq!(rows[2][ci("ak5_status")].as_deref(), Some("A"));
        assert!(rows[2][ci("ak3_segment_id")].is_none());
    }

    #[test]
    fn extracts_999_errors() {
        let body = "AK1*HC*1~\
                    AK2*837*0001~\
                    IK3*NM1*8**8~\
                    IK4*2*1037*7~\
                    IK5*R~\
                    AK9*R*1*1*0~";
        let rows = extract("read_999", &one_tx("000000006", "999", body));
        let ci = |c| idx("read_999", c);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][ci("ik3_segment_id")].as_deref(), Some("NM1"));
        assert_eq!(rows[0][ci("ik5_status")].as_deref(), Some("R"));
        assert_eq!(rows[1][ci("ik4_element_position")].as_deref(), Some("2"));
        assert_eq!(rows[1][ci("ik4_error_code")].as_deref(), Some("7"));
        assert_eq!(rows[0][ci("ak9_status")].as_deref(), Some("R"));
    }
}
