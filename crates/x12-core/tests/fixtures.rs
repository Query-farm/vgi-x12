//! Golden-fixture integration tests over the committed synthetic interchanges in
//! `data/` (non-PHI, deterministic). Asserts delimiter sniffing, ISA13/GS06/ST02
//! capture, exact shaped-column positions, parent/child row counts, the
//! SE/GE/IEA structural flags, multi-GS / multi-ST resume, non-canonical
//! delimiters, EDIFACT, and that malformed input never panics.

use std::path::PathBuf;

use x12_core::delimiters::{detect_family, sniff_x12, Family};
use x12_core::envelope::parse_x12;
use x12_core::shaped;

fn fixture(name: &str) -> Vec<u8> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../data");
    p.push(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read fixture {}: {e}", p.display()))
}

/// Run a shaped view over the first matching transaction in `bytes`.
fn shaped_rows(fn_name: &str, bytes: &[u8]) -> Vec<Vec<Option<String>>> {
    let def = shaped::def(fn_name).unwrap();
    let mut out = Vec::new();
    for inter in parse_x12(bytes) {
        for g in &inter.groups {
            for tx in &g.transactions {
                if tx.type_code() == def.st01 {
                    out.extend((def.extract)(tx, &inter.delimiters));
                }
            }
        }
    }
    out
}

fn col(fn_name: &str, name: &str) -> usize {
    shaped::def(fn_name)
        .unwrap()
        .cols
        .iter()
        .position(|c| c.name == name)
        .unwrap()
}

#[test]
fn era_835_golden() {
    let bytes = fixture("era_835.835");
    let d = sniff_x12(&bytes).expect("sniff ISA");
    assert_eq!(d.element, b'*');
    assert_eq!(d.segment, b'~');
    assert_eq!(d.component, b':');
    assert_eq!(d.repetition, Some(b'^'));

    let inters = parse_x12(&bytes);
    assert_eq!(inters.len(), 1);
    assert_eq!(inters[0].control(), "000000123");
    let g = &inters[0].groups[0];
    assert_eq!(g.control(), "1");
    let tx = &g.transactions[0];
    assert_eq!(tx.type_code(), "835");
    assert_eq!(tx.segment_count(), 11);
    assert_eq!(tx.se_count_ok(), Some(true));
    assert_eq!(g.ge_count_ok(), Some(true));
    assert_eq!(inters[0].iea_count_ok(), Some(true));

    let rows = shaped_rows("read_835", &bytes);
    // CLP1 (CAS, SVC, DTM) = 3 rows + CLP2 bare = 1 → 4.
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0][col("read_835", "bpr_total_paid")].as_deref(),
        Some("1500")
    );
    assert_eq!(
        rows[0][col("read_835", "payer_name")].as_deref(),
        Some("ACME HEALTH PLAN")
    );
    assert_eq!(
        rows[0][col("read_835", "payee_npi")].as_deref(),
        Some("1999999999")
    );
    assert_eq!(
        rows[0][col("read_835", "clp_claim_id")].as_deref(),
        Some("PCN1001")
    );
    assert_eq!(
        rows[0][col("read_835", "clp_total_paid")].as_deref(),
        Some("400")
    );
    assert_eq!(
        rows[3][col("read_835", "clp_claim_id")].as_deref(),
        Some("PCN1002")
    );
}

#[test]
fn claim_837_golden() {
    let bytes = fixture("claim_837.837");
    let rows = shaped_rows("read_837", &bytes);
    assert_eq!(rows.len(), 3); // HI, SV1, DTP
    assert_eq!(
        rows[0][col("read_837", "billing_provider_npi")].as_deref(),
        Some("1122334455")
    );
    assert_eq!(
        rows[0][col("read_837", "subscriber_id")].as_deref(),
        Some("MEMBER123")
    );
    assert_eq!(
        rows[0][col("read_837", "clm_place_of_service")].as_deref(),
        Some("11:B:1")
    );
    assert_eq!(
        rows[0][col("read_837", "hi_diagnosis_code")].as_deref(),
        Some("Z1234")
    );
    assert_eq!(
        rows[1][col("read_837", "sv1_charge")].as_deref(),
        Some("200")
    );
}

#[test]
fn eligibility_271_golden() {
    let bytes = fixture("eligibility_271.271");
    let rows = shaped_rows("read_271", &bytes);
    // HL1 bare, HL2 bare, HL3 (2 EB + 1 DTP) = 3 → 5.
    assert_eq!(rows.len(), 5);
    assert_eq!(
        rows[2][col("read_271", "entity_id")].as_deref(),
        Some("MEMBER99")
    );
    assert_eq!(
        rows[2][col("read_271", "eb_plan_description")].as_deref(),
        Some("GOLD PPO")
    );
    assert_eq!(
        rows[3][col("read_271", "eb_benefit_amount")].as_deref(),
        Some("27.5")
    );
}

#[test]
fn po_850_golden() {
    let bytes = fixture("po_850.850");
    let rows = shaped_rows("read_850", &bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0][col("read_850", "beg_po_number")].as_deref(),
        Some("PO9988")
    );
    assert_eq!(
        rows[0][col("read_850", "po1_product_id")].as_deref(),
        Some("WIDGET-A")
    );
    assert_eq!(
        rows[1][col("read_850", "po1_unit_price")].as_deref(),
        Some("9.99")
    );
}

#[test]
fn ack_997_golden() {
    let bytes = fixture("ack_997.997");
    let rows = shaped_rows("read_997", &bytes);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0][col("read_997", "ak5_status")].as_deref(), Some("E"));
    assert_eq!(
        rows[0][col("read_997", "ak3_segment_id")].as_deref(),
        Some("CLM")
    );
    assert_eq!(
        rows[0][col("read_997", "ak9_sets_accepted")].as_deref(),
        Some("1")
    );
    assert_eq!(rows[2][col("read_997", "ak5_status")].as_deref(), Some("A"));
}

#[test]
fn edifact_orders_golden() {
    let bytes = fixture("orders_edifact.edi");
    assert_eq!(detect_family(&bytes), Family::Edifact);
    let inters = x12_core::edifact::parse_edifact(&bytes);
    assert_eq!(inters.len(), 1);
    assert_eq!(inters[0].control(), "REF0001");
    let m = &inters[0].messages[0];
    assert_eq!(m.message_type(inters[0].delimiters.component), "ORDERS");
    assert_eq!(m.unt_count_ok(), Some(true));
    assert_eq!(x12_core::envelope::first_transaction_type(&bytes), "ORDERS");
}

#[test]
fn multi_group_resume() {
    let bytes = fixture("multi_group.edi");
    let inters = parse_x12(&bytes);
    assert_eq!(inters.len(), 1);
    let i = &inters[0];
    assert_eq!(i.groups.len(), 2);
    assert_eq!(i.iea_count_ok(), Some(true)); // IEA01 = 2 groups
    assert_eq!(i.groups[0].transactions.len(), 2); // two 837 ST in the first GS
    assert_eq!(i.groups[0].ge_count_ok(), Some(true));
    assert_eq!(i.groups[1].transactions[0].type_code(), "835");
}

#[test]
fn noncanonical_delimiters() {
    let bytes = fixture("pipe_delims.edi");
    let d = sniff_x12(&bytes).expect("sniff pipe ISA");
    assert_eq!(d.element, b'|');
    assert_eq!(d.component, b'>');
    assert_eq!(d.segment, b'\n');
    assert_eq!(d.repetition, None); // 'U' placeholder
    let rows = shaped_rows("read_837", &bytes);
    assert_eq!(rows.len(), 1); // bare CLM (no detail children)
    assert_eq!(rows[0][col("read_837", "clm_id")].as_deref(), Some("ACCT9"));
    assert_eq!(
        rows[0][col("read_837", "clm_place_of_service")].as_deref(),
        Some("11>B>1")
    );
}

#[test]
fn truncated_never_panics() {
    let bytes = fixture("truncated.edi");
    let inters = parse_x12(&bytes); // must not panic
    assert_eq!(inters.len(), 1);
    let tx = &inters[0].groups[0].transactions[0];
    assert!(tx.se.is_none());
    assert_eq!(tx.se_count_ok(), None);
    assert_eq!(inters[0].iea_count_ok(), None);
    // The shaped extractor still surfaces what it parsed.
    let rows = shaped_rows("read_837", &bytes);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0][col("read_837", "clm_id")].as_deref(),
        Some("ACCTTRUNC")
    );
}

#[test]
fn arbitrary_bytes_never_panic() {
    // Fuzz-ish: a pile of bytes that is not a valid interchange yields no rows,
    // never a panic (per-row error capture / robustness).
    for junk in [
        &b""[..],
        b"ISA",
        b"ISA*too*short",
        b"\x00\x01\x02ISA*\xff\xff",
        b"UNAUNBUNH garbage ' + : ?",
    ] {
        let _ = parse_x12(junk);
        let _ = x12_core::edifact::parse_edifact(junk);
        let _ = x12_core::envelope::first_transaction_type(junk);
    }
}
