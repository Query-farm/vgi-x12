//! `read_835` — remittance / ERA (ST01 `835`). Parent = `CLP`; financial header
//! carried down from `BPR`/`TRN`; payer/payee names pivoted from the `NM1` loops
//! by NM101 qualifier (`PR` payer, `PE` payee). Repeating `CAS`/`SVC`/`REF`/`DTM`
//! under one `CLP` fan out to one row each. Positional, public-segment-ID only.

use super::{Col, Row, RowB};
use crate::delimiters::Delimiters;
use crate::envelope::Transaction;

pub const COLS: &[Col] = &[
    Col {
        name: "bpr_total_paid",
        comment: "BPR02 — total actual provider payment amount (raw VARCHAR; cast as needed).",
    },
    Col {
        name: "bpr_credit_debit",
        comment: "BPR03 — credit/debit flag (raw code).",
    },
    Col {
        name: "bpr_payment_method",
        comment: "BPR04 — payment method code (raw, e.g. ACH/CHK).",
    },
    Col {
        name: "bpr_payment_date",
        comment: "BPR16 — payment effective date (raw CCYYMMDD).",
    },
    Col {
        name: "trn_trace_number",
        comment: "TRN02 — reassociation trace number.",
    },
    Col {
        name: "trn_payer_id",
        comment: "TRN03 — originating company / payer identifier.",
    },
    Col {
        name: "payer_name",
        comment: "NM103 of the NM1*PR loop — payer name (raw).",
    },
    Col {
        name: "payee_name",
        comment: "NM103 of the NM1*PE loop — payee name (raw).",
    },
    Col {
        name: "payee_npi",
        comment: "NM109 of the NM1*PE loop — payee identifier (e.g. NPI).",
    },
    Col {
        name: "clp_claim_id",
        comment: "CLP01 — patient control number (claim id).",
    },
    Col {
        name: "clp_status_code",
        comment: "CLP02 — claim status code (raw).",
    },
    Col {
        name: "clp_total_charge",
        comment: "CLP03 — total submitted charge (raw VARCHAR).",
    },
    Col {
        name: "clp_total_paid",
        comment: "CLP04 — total paid amount (raw VARCHAR).",
    },
    Col {
        name: "clp_patient_resp",
        comment: "CLP05 — patient responsibility amount (raw VARCHAR).",
    },
    Col {
        name: "clp_filing_code",
        comment: "CLP06 — claim filing indicator code (raw).",
    },
    Col {
        name: "clp_payer_claim_control",
        comment: "CLP07 — payer claim control number.",
    },
    Col {
        name: "clp_facility_code",
        comment: "CLP08 — facility type / place-of-service code (raw).",
    },
    Col {
        name: "cas_group_code",
        comment: "CAS01 — claim-adjustment group code (raw).",
    },
    Col {
        name: "cas_reason_code",
        comment: "CAS02 — claim-adjustment reason code (raw).",
    },
    Col {
        name: "cas_amount",
        comment: "CAS03 — adjustment amount (raw VARCHAR).",
    },
    Col {
        name: "svc_procedure",
        comment: "SVC01 — service procedure composite (raw, component-joined).",
    },
    Col {
        name: "svc_charge",
        comment: "SVC02 — line item charge amount (raw VARCHAR).",
    },
    Col {
        name: "svc_paid",
        comment: "SVC03 — line item paid amount (raw VARCHAR).",
    },
    Col {
        name: "svc_units",
        comment: "SVC05 — units of service (raw VARCHAR).",
    },
    Col {
        name: "ref_qualifier",
        comment: "REF01 — reference identification qualifier (raw).",
    },
    Col {
        name: "ref_value",
        comment: "REF02 — reference identification value.",
    },
    Col {
        name: "dtm_qualifier",
        comment: "DTM01 — date/time qualifier (raw).",
    },
    Col {
        name: "dtm_date",
        comment: "DTM02 — date value (raw CCYYMMDD).",
    },
];

#[derive(Default, Clone)]
struct Header {
    bpr_total_paid: String,
    bpr_credit_debit: String,
    bpr_payment_method: String,
    bpr_payment_date: String,
    trn_trace_number: String,
    trn_payer_id: String,
    payer_name: String,
    payee_name: String,
    payee_npi: String,
}

impl Header {
    fn apply(&self, b: &mut RowB) {
        b.set("bpr_total_paid", &self.bpr_total_paid);
        b.set("bpr_credit_debit", &self.bpr_credit_debit);
        b.set("bpr_payment_method", &self.bpr_payment_method);
        b.set("bpr_payment_date", &self.bpr_payment_date);
        b.set("trn_trace_number", &self.trn_trace_number);
        b.set("trn_payer_id", &self.trn_payer_id);
        b.set("payer_name", &self.payer_name);
        b.set("payee_name", &self.payee_name);
        b.set("payee_npi", &self.payee_npi);
    }
}

#[derive(Default, Clone)]
struct Clp {
    claim_id: String,
    status: String,
    total_charge: String,
    total_paid: String,
    patient_resp: String,
    filing: String,
    payer_claim_control: String,
    facility: String,
}

impl Clp {
    fn apply(&self, b: &mut RowB) {
        b.set("clp_claim_id", &self.claim_id);
        b.set("clp_status_code", &self.status);
        b.set("clp_total_charge", &self.total_charge);
        b.set("clp_total_paid", &self.total_paid);
        b.set("clp_patient_resp", &self.patient_resp);
        b.set("clp_filing_code", &self.filing);
        b.set("clp_payer_claim_control", &self.payer_claim_control);
        b.set("clp_facility_code", &self.facility);
    }
}

pub fn rows(tx: &Transaction, _d: &Delimiters) -> Vec<Row> {
    let mut out = Vec::new();
    let mut hdr = Header::default();
    let mut clp: Option<Clp> = None;
    let mut clp_emitted_child = false;

    // Emit a bare parent row for a CLP that had no detail children, so every
    // claim appears at least once.
    macro_rules! flush_bare {
        () => {
            if let Some(c) = &clp {
                if !clp_emitted_child {
                    let mut b = RowB::new();
                    hdr.apply(&mut b);
                    c.apply(&mut b);
                    out.push(b.build(COLS));
                }
            }
        };
    }

    for seg in tx.body() {
        match seg.id() {
            "BPR" => {
                hdr.bpr_total_paid = seg.elem(2).to_string();
                hdr.bpr_credit_debit = seg.elem(3).to_string();
                hdr.bpr_payment_method = seg.elem(4).to_string();
                hdr.bpr_payment_date = seg.elem(16).to_string();
            }
            "TRN" => {
                hdr.trn_trace_number = seg.elem(2).to_string();
                hdr.trn_payer_id = seg.elem(3).to_string();
            }
            "NM1" => match seg.elem(1) {
                "PR" => hdr.payer_name = seg.elem(3).to_string(),
                "PE" => {
                    hdr.payee_name = seg.elem(3).to_string();
                    hdr.payee_npi = seg.elem(9).to_string();
                }
                _ => {}
            },
            "CLP" => {
                flush_bare!();
                clp = Some(Clp {
                    claim_id: seg.elem(1).to_string(),
                    status: seg.elem(2).to_string(),
                    total_charge: seg.elem(3).to_string(),
                    total_paid: seg.elem(4).to_string(),
                    patient_resp: seg.elem(5).to_string(),
                    filing: seg.elem(6).to_string(),
                    payer_claim_control: seg.elem(7).to_string(),
                    facility: seg.elem(8).to_string(),
                });
                clp_emitted_child = false;
            }
            "CAS" => {
                if let Some(c) = &clp {
                    let mut b = RowB::new();
                    hdr.apply(&mut b);
                    c.apply(&mut b);
                    b.set("cas_group_code", seg.elem(1));
                    b.set("cas_reason_code", seg.elem(2));
                    b.set("cas_amount", seg.elem(3));
                    out.push(b.build(COLS));
                    clp_emitted_child = true;
                }
            }
            "SVC" => {
                if let Some(c) = &clp {
                    let mut b = RowB::new();
                    hdr.apply(&mut b);
                    c.apply(&mut b);
                    b.set("svc_procedure", seg.elem(1));
                    b.set("svc_charge", seg.elem(2));
                    b.set("svc_paid", seg.elem(3));
                    b.set("svc_units", seg.elem(5));
                    out.push(b.build(COLS));
                    clp_emitted_child = true;
                }
            }
            "REF" => {
                if let Some(c) = &clp {
                    let mut b = RowB::new();
                    hdr.apply(&mut b);
                    c.apply(&mut b);
                    b.set("ref_qualifier", seg.elem(1));
                    b.set("ref_value", seg.elem(2));
                    out.push(b.build(COLS));
                    clp_emitted_child = true;
                }
            }
            "DTM" => {
                if let Some(c) = &clp {
                    let mut b = RowB::new();
                    hdr.apply(&mut b);
                    c.apply(&mut b);
                    b.set("dtm_qualifier", seg.elem(1));
                    b.set("dtm_date", seg.elem(2));
                    out.push(b.build(COLS));
                    clp_emitted_child = true;
                }
            }
            _ => {}
        }
    }
    flush_bare!();
    out
}

#[cfg(test)]
mod tests {
    use super::super::tests::{extract, idx, one_tx};

    #[test]
    fn extracts_835_fanout() {
        let body = "BPR*I*1000*C*ACH*CCP*01*999*DA*123*1512345678**01*999*DA*456*20240115~\
                    TRN*1*TRACE123*1234567890~\
                    NM1*PR*2*ACME HEALTH*****PI*PAYER01~\
                    NM1*PE*2*CLINIC LLC*****XX*1999999999~\
                    CLP*PCN1*1*500*400*100*MC*CCN777*11~\
                    CAS*CO*45*100~\
                    SVC*HC:99213*200*160**1~\
                    DTM*232*20240110~\
                    CLP*PCN2*4*300*0*0*MC*CCN888*11~";
        let rows = extract("read_835", &one_tx("000000001", "835", body));
        // CLP1 has 3 children (CAS, SVC, DTM) → 3 rows; CLP2 has none → 1 bare row.
        assert_eq!(rows.len(), 4);
        let ci = |c| idx("read_835", c);
        // Header carried on every row.
        assert_eq!(rows[0][ci("bpr_total_paid")].as_deref(), Some("1000"));
        assert_eq!(rows[0][ci("bpr_payment_date")].as_deref(), Some("20240115"));
        assert_eq!(rows[0][ci("trn_trace_number")].as_deref(), Some("TRACE123"));
        assert_eq!(rows[0][ci("payer_name")].as_deref(), Some("ACME HEALTH"));
        assert_eq!(rows[0][ci("payee_name")].as_deref(), Some("CLINIC LLC"));
        assert_eq!(rows[0][ci("payee_npi")].as_deref(), Some("1999999999"));
        // CLP1 first child = CAS.
        assert_eq!(rows[0][ci("clp_claim_id")].as_deref(), Some("PCN1"));
        assert_eq!(rows[0][ci("clp_total_charge")].as_deref(), Some("500"));
        assert_eq!(rows[0][ci("clp_total_paid")].as_deref(), Some("400"));
        assert_eq!(rows[0][ci("cas_reason_code")].as_deref(), Some("45"));
        assert_eq!(rows[0][ci("cas_amount")].as_deref(), Some("100"));
        // SVC row.
        assert_eq!(rows[1][ci("svc_procedure")].as_deref(), Some("HC:99213"));
        assert_eq!(rows[1][ci("svc_charge")].as_deref(), Some("200"));
        assert_eq!(rows[1][ci("svc_units")].as_deref(), Some("1"));
        assert!(rows[1][ci("cas_reason_code")].is_none());
        // Bare CLP2 row.
        assert_eq!(rows[3][ci("clp_claim_id")].as_deref(), Some("PCN2"));
        assert!(rows[3][ci("svc_procedure")].is_none());
    }
}
