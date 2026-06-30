//! `read_837` — claim (ST01 `837`, P/I/D variants by GS08). Parent = `CLM`;
//! loop-keyed `NM1`s pivoted by entity-identifier qualifier (`85` billing
//! provider, `IL` subscriber, `QC` patient); `SBR` carried down. Repeating
//! `HI`/`SV1`/`SV2`/`DTP` under one `CLM` fan out to one row each.

use super::{Col, Row, RowB};
use crate::delimiters::Delimiters;
use crate::envelope::Transaction;

pub const COLS: &[Col] = &[
    Col {
        name: "billing_provider_name",
        comment: "NM103 of the NM1*85 loop — billing provider name (raw).",
    },
    Col {
        name: "billing_provider_npi",
        comment: "NM109 of the NM1*85 loop — billing provider identifier (NPI).",
    },
    Col {
        name: "subscriber_name",
        comment: "NM103 of the NM1*IL loop — subscriber name (raw).",
    },
    Col {
        name: "subscriber_id",
        comment: "NM109 of the NM1*IL loop — subscriber primary identifier.",
    },
    Col {
        name: "patient_name",
        comment: "NM103 of the NM1*QC loop — patient name (raw).",
    },
    Col {
        name: "sbr_payer_responsibility",
        comment: "SBR01 — payer responsibility sequence code (raw).",
    },
    Col {
        name: "sbr_relationship",
        comment: "SBR02 — individual relationship code (raw).",
    },
    Col {
        name: "sbr_plan_name",
        comment: "SBR04 — insured group / plan name (raw).",
    },
    Col {
        name: "clm_id",
        comment: "CLM01 — patient account / claim submitter identifier.",
    },
    Col {
        name: "clm_total_charge",
        comment: "CLM02 — total claim charge amount (raw VARCHAR).",
    },
    Col {
        name: "clm_place_of_service",
        comment: "CLM05 — health-care service location composite (raw, component-joined).",
    },
    Col {
        name: "clm_provider_signature",
        comment: "CLM06 — provider-signature-on-file indicator (raw).",
    },
    Col {
        name: "hi_diagnosis_qualifier",
        comment: "HI01-1 — diagnosis code-list qualifier (raw).",
    },
    Col {
        name: "hi_diagnosis_code",
        comment: "HI01-2 — diagnosis code (raw).",
    },
    Col {
        name: "sv1_procedure",
        comment: "SV101 — professional service procedure composite (raw, component-joined).",
    },
    Col {
        name: "sv1_charge",
        comment: "SV102 — professional line item charge (raw VARCHAR).",
    },
    Col {
        name: "sv1_units",
        comment: "SV104 — professional service unit count (raw VARCHAR).",
    },
    Col {
        name: "sv2_revenue_code",
        comment: "SV201 — institutional revenue code (raw).",
    },
    Col {
        name: "sv2_procedure",
        comment: "SV202 — institutional procedure composite (raw, component-joined).",
    },
    Col {
        name: "sv2_charge",
        comment: "SV203 — institutional line item charge (raw VARCHAR).",
    },
    Col {
        name: "dtp_qualifier",
        comment: "DTP01 — date/time qualifier (raw).",
    },
    Col {
        name: "dtp_date",
        comment: "DTP03 — date value (raw).",
    },
];

#[derive(Default, Clone)]
struct Header {
    billing_provider_name: String,
    billing_provider_npi: String,
    subscriber_name: String,
    subscriber_id: String,
    patient_name: String,
    sbr_payer_responsibility: String,
    sbr_relationship: String,
    sbr_plan_name: String,
}

impl Header {
    fn apply(&self, b: &mut RowB) {
        b.set("billing_provider_name", &self.billing_provider_name);
        b.set("billing_provider_npi", &self.billing_provider_npi);
        b.set("subscriber_name", &self.subscriber_name);
        b.set("subscriber_id", &self.subscriber_id);
        b.set("patient_name", &self.patient_name);
        b.set("sbr_payer_responsibility", &self.sbr_payer_responsibility);
        b.set("sbr_relationship", &self.sbr_relationship);
        b.set("sbr_plan_name", &self.sbr_plan_name);
    }
}

#[derive(Default, Clone)]
struct Clm {
    id: String,
    total_charge: String,
    place_of_service: String,
    provider_signature: String,
}

impl Clm {
    fn apply(&self, b: &mut RowB) {
        b.set("clm_id", &self.id);
        b.set("clm_total_charge", &self.total_charge);
        b.set("clm_place_of_service", &self.place_of_service);
        b.set("clm_provider_signature", &self.provider_signature);
    }
}

pub fn rows(tx: &Transaction, d: &Delimiters) -> Vec<Row> {
    let comp = d.component;
    let mut out = Vec::new();
    let mut hdr = Header::default();
    let mut clm: Option<Clm> = None;
    let mut emitted_child = false;

    macro_rules! flush_bare {
        () => {
            if let Some(c) = &clm {
                if !emitted_child {
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
            "NM1" => match seg.elem(1) {
                "85" => {
                    hdr.billing_provider_name = seg.elem(3).to_string();
                    hdr.billing_provider_npi = seg.elem(9).to_string();
                }
                "IL" => {
                    hdr.subscriber_name = seg.elem(3).to_string();
                    hdr.subscriber_id = seg.elem(9).to_string();
                }
                "QC" => hdr.patient_name = seg.elem(3).to_string(),
                _ => {}
            },
            "SBR" => {
                hdr.sbr_payer_responsibility = seg.elem(1).to_string();
                hdr.sbr_relationship = seg.elem(2).to_string();
                hdr.sbr_plan_name = seg.elem(4).to_string();
            }
            "CLM" => {
                flush_bare!();
                clm = Some(Clm {
                    id: seg.elem(1).to_string(),
                    total_charge: seg.elem(2).to_string(),
                    place_of_service: seg.elem(5).to_string(),
                    provider_signature: seg.elem(6).to_string(),
                });
                emitted_child = false;
            }
            "HI" => {
                if let Some(c) = &clm {
                    let mut b = RowB::new();
                    hdr.apply(&mut b);
                    c.apply(&mut b);
                    b.set("hi_diagnosis_qualifier", seg.elem_comp(1, 1, comp));
                    b.set("hi_diagnosis_code", seg.elem_comp(1, 2, comp));
                    out.push(b.build(COLS));
                    emitted_child = true;
                }
            }
            "SV1" => {
                if let Some(c) = &clm {
                    let mut b = RowB::new();
                    hdr.apply(&mut b);
                    c.apply(&mut b);
                    b.set("sv1_procedure", seg.elem(1));
                    b.set("sv1_charge", seg.elem(2));
                    b.set("sv1_units", seg.elem(4));
                    out.push(b.build(COLS));
                    emitted_child = true;
                }
            }
            "SV2" => {
                if let Some(c) = &clm {
                    let mut b = RowB::new();
                    hdr.apply(&mut b);
                    c.apply(&mut b);
                    b.set("sv2_revenue_code", seg.elem(1));
                    b.set("sv2_procedure", seg.elem(2));
                    b.set("sv2_charge", seg.elem(3));
                    out.push(b.build(COLS));
                    emitted_child = true;
                }
            }
            "DTP" => {
                if let Some(c) = &clm {
                    let mut b = RowB::new();
                    hdr.apply(&mut b);
                    c.apply(&mut b);
                    b.set("dtp_qualifier", seg.elem(1));
                    b.set("dtp_date", seg.elem(3));
                    out.push(b.build(COLS));
                    emitted_child = true;
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
    fn extracts_837_pivot_and_fanout() {
        let body = "NM1*85*2*BILLING CLINIC*****XX*1122334455~\
                    NM1*IL*1*DOE*JOHN****MI*MEMBER123~\
                    SBR*P*18**GROUP PLAN~\
                    NM1*QC*1*DOE*JANE~\
                    CLM*ACCT777*500***11:B:1*Y~\
                    HI*ABK:Z1234~\
                    SV1*HC:99213*200*UN*1~\
                    DTP*472*D8*20240105~";
        let rows = extract("read_837", &one_tx("000000002", "837", body));
        // CLM has 3 children (HI, SV1, DTP).
        assert_eq!(rows.len(), 3);
        let ci = |c| idx("read_837", c);
        assert_eq!(
            rows[0][ci("billing_provider_name")].as_deref(),
            Some("BILLING CLINIC")
        );
        assert_eq!(
            rows[0][ci("billing_provider_npi")].as_deref(),
            Some("1122334455")
        );
        assert_eq!(rows[0][ci("subscriber_name")].as_deref(), Some("DOE"));
        assert_eq!(rows[0][ci("subscriber_id")].as_deref(), Some("MEMBER123"));
        assert_eq!(rows[0][ci("patient_name")].as_deref(), Some("DOE"));
        assert_eq!(
            rows[0][ci("sbr_payer_responsibility")].as_deref(),
            Some("P")
        );
        assert_eq!(rows[0][ci("clm_id")].as_deref(), Some("ACCT777"));
        assert_eq!(rows[0][ci("clm_total_charge")].as_deref(), Some("500"));
        assert_eq!(
            rows[0][ci("clm_place_of_service")].as_deref(),
            Some("11:B:1")
        );
        // HI row.
        assert_eq!(
            rows[0][ci("hi_diagnosis_qualifier")].as_deref(),
            Some("ABK")
        );
        assert_eq!(rows[0][ci("hi_diagnosis_code")].as_deref(), Some("Z1234"));
        // SV1 row.
        assert_eq!(rows[1][ci("sv1_procedure")].as_deref(), Some("HC:99213"));
        assert_eq!(rows[1][ci("sv1_charge")].as_deref(), Some("200"));
        assert_eq!(rows[1][ci("sv1_units")].as_deref(), Some("1"));
        // DTP row.
        assert_eq!(rows[2][ci("dtp_qualifier")].as_deref(), Some("472"));
        assert_eq!(rows[2][ci("dtp_date")].as_deref(), Some("20240105"));
    }
}
