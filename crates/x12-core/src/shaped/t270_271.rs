//! `read_270` / `read_271` — eligibility inquiry / response (ST01 `270` / `271`).
//! Parent = `HL`; the HL's `NM1` entity and `TRN` are carried down; benefit
//! lines from `EB` (271 only) and `DTP`/`AAA` fan out to one row each. The two
//! share one column set — `eb_*` simply stays NULL for a 270 inquiry.

use super::{Col, Row, RowB};
use crate::delimiters::Delimiters;
use crate::envelope::Transaction;

pub const COLS: &[Col] = &[
    Col {
        name: "hl_id",
        comment: "HL01 — hierarchical ID number.",
    },
    Col {
        name: "hl_parent",
        comment: "HL02 — hierarchical parent ID number.",
    },
    Col {
        name: "hl_level_code",
        comment: "HL03 — hierarchical level code (raw, e.g. 20/21/22/23).",
    },
    Col {
        name: "hl_child_code",
        comment: "HL04 — hierarchical child code (raw).",
    },
    Col {
        name: "entity_qualifier",
        comment: "NM101 of the HL's NM1 — entity identifier code (raw).",
    },
    Col {
        name: "entity_name",
        comment: "NM103 of the HL's NM1 — entity name (raw).",
    },
    Col {
        name: "entity_id_qualifier",
        comment: "NM108 of the HL's NM1 — identification code qualifier (raw).",
    },
    Col {
        name: "entity_id",
        comment: "NM109 of the HL's NM1 — entity identification code.",
    },
    Col {
        name: "trn_trace",
        comment: "TRN02 — trace number under the HL.",
    },
    Col {
        name: "eb_eligibility_code",
        comment: "EB01 — eligibility/benefit information code (271, raw).",
    },
    Col {
        name: "eb_coverage_level",
        comment: "EB02 — coverage level code (271, raw).",
    },
    Col {
        name: "eb_service_type",
        comment: "EB03 — service type code (271, raw).",
    },
    Col {
        name: "eb_insurance_type",
        comment: "EB04 — insurance type code (271, raw).",
    },
    Col {
        name: "eb_plan_description",
        comment: "EB05 — plan coverage description (271, raw).",
    },
    Col {
        name: "eb_time_qualifier",
        comment: "EB06 — time period qualifier (271, raw).",
    },
    Col {
        name: "eb_benefit_amount",
        comment: "EB07 — benefit amount (271, raw VARCHAR).",
    },
    Col {
        name: "dtp_qualifier",
        comment: "DTP01 — date/time qualifier (raw).",
    },
    Col {
        name: "dtp_date",
        comment: "DTP03 — date value (raw).",
    },
    Col {
        name: "aaa_reject_reason",
        comment: "AAA03 — request validation / reject reason code (raw).",
    },
];

#[derive(Default, Clone)]
struct Hl {
    id: String,
    parent: String,
    level: String,
    child: String,
    entity_qualifier: String,
    entity_name: String,
    entity_id_qualifier: String,
    entity_id: String,
    trn_trace: String,
}

impl Hl {
    fn apply(&self, b: &mut RowB) {
        b.set("hl_id", &self.id);
        b.set("hl_parent", &self.parent);
        b.set("hl_level_code", &self.level);
        b.set("hl_child_code", &self.child);
        b.set("entity_qualifier", &self.entity_qualifier);
        b.set("entity_name", &self.entity_name);
        b.set("entity_id_qualifier", &self.entity_id_qualifier);
        b.set("entity_id", &self.entity_id);
        b.set("trn_trace", &self.trn_trace);
    }
}

pub fn rows(tx: &Transaction, _d: &Delimiters) -> Vec<Row> {
    let mut out = Vec::new();
    let mut hl: Option<Hl> = None;
    let mut emitted_child = false;

    macro_rules! flush_bare {
        () => {
            if let Some(h) = &hl {
                if !emitted_child {
                    let mut b = RowB::new();
                    h.apply(&mut b);
                    out.push(b.build(COLS));
                }
            }
        };
    }

    for seg in tx.body() {
        match seg.id() {
            "HL" => {
                flush_bare!();
                hl = Some(Hl {
                    id: seg.elem(1).to_string(),
                    parent: seg.elem(2).to_string(),
                    level: seg.elem(3).to_string(),
                    child: seg.elem(4).to_string(),
                    ..Default::default()
                });
                emitted_child = false;
            }
            "NM1" => {
                if let Some(h) = hl.as_mut() {
                    // Only the first NM1 of the HL populates the entity columns.
                    if h.entity_qualifier.is_empty() && h.entity_name.is_empty() {
                        h.entity_qualifier = seg.elem(1).to_string();
                        h.entity_name = seg.elem(3).to_string();
                        h.entity_id_qualifier = seg.elem(8).to_string();
                        h.entity_id = seg.elem(9).to_string();
                    }
                }
            }
            "TRN" => {
                if let Some(h) = hl.as_mut() {
                    h.trn_trace = seg.elem(2).to_string();
                }
            }
            "EB" => {
                if let Some(h) = &hl {
                    let mut b = RowB::new();
                    h.apply(&mut b);
                    b.set("eb_eligibility_code", seg.elem(1));
                    b.set("eb_coverage_level", seg.elem(2));
                    b.set("eb_service_type", seg.elem(3));
                    b.set("eb_insurance_type", seg.elem(4));
                    b.set("eb_plan_description", seg.elem(5));
                    b.set("eb_time_qualifier", seg.elem(6));
                    b.set("eb_benefit_amount", seg.elem(7));
                    out.push(b.build(COLS));
                    emitted_child = true;
                }
            }
            "DTP" => {
                if let Some(h) = &hl {
                    let mut b = RowB::new();
                    h.apply(&mut b);
                    b.set("dtp_qualifier", seg.elem(1));
                    b.set("dtp_date", seg.elem(3));
                    out.push(b.build(COLS));
                    emitted_child = true;
                }
            }
            "AAA" => {
                if let Some(h) = &hl {
                    let mut b = RowB::new();
                    h.apply(&mut b);
                    b.set("aaa_reject_reason", seg.elem(3));
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
    fn extracts_271_benefits() {
        let body = "HL*1**20*1~\
                    NM1*PR*2*ACME PAYER*****PI*PAYER01~\
                    HL*2*1*21*1~\
                    NM1*1P*2*PROVIDER GRP*****XX*1444444444~\
                    HL*3*2*22*0~\
                    NM1*IL*1*DOE*JOHN****MI*MEMBER99~\
                    TRN*2*TRACE55*9999999999~\
                    EB*1*IND*30**GOLD PPO~\
                    EB*B*IND*30****27.5~\
                    DTP*291*D8*20240101~";
        let rows = extract("read_271", &one_tx("000000003", "271", body));
        let ci = |c| idx("read_271", c);
        // HL1 bare, HL2 bare, HL3 has 3 children (2 EB + 1 DTP).
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0][ci("hl_id")].as_deref(), Some("1"));
        assert_eq!(rows[0][ci("hl_level_code")].as_deref(), Some("20"));
        assert_eq!(rows[0][ci("entity_name")].as_deref(), Some("ACME PAYER"));
        // HL3 subscriber rows.
        assert_eq!(rows[2][ci("hl_id")].as_deref(), Some("3"));
        assert_eq!(rows[2][ci("entity_name")].as_deref(), Some("DOE"));
        assert_eq!(rows[2][ci("entity_id")].as_deref(), Some("MEMBER99"));
        assert_eq!(rows[2][ci("trn_trace")].as_deref(), Some("TRACE55"));
        assert_eq!(rows[2][ci("eb_eligibility_code")].as_deref(), Some("1"));
        assert_eq!(
            rows[2][ci("eb_plan_description")].as_deref(),
            Some("GOLD PPO")
        );
        assert_eq!(rows[3][ci("eb_benefit_amount")].as_deref(), Some("27.5"));
        assert_eq!(rows[4][ci("dtp_date")].as_deref(), Some("20240101"));
    }
}
