//! Composed example queries: realistic SQL that combines two or more of the
//! worker's own objects in one query (a scalar feeding a reference-view join,
//! an envelope summary reconciled against an acknowledgment, a claim file
//! reconciled against its remittance).
//!
//! Each SQL is defined ONCE here and referenced from both the schema-level
//! `vgi.example_queries` list (`main.rs`) and the example list of the object
//! that drives the query, so the two copies stay byte-identical. Every query
//! parses inline interchanges only: offline, deterministic, no files.
//!
//! The ISA segments are kept whole inside single string pieces so their
//! fixed-width padding survives `concat!` byte-for-byte.

use vgi::FunctionExample;

/// Route an inbox of payloads: `transaction_type` sniffs each payload and the
/// `transaction_sets` reference view names the shaped `read_*` function to
/// call (NULL for a type with no shaped view, here an EDIFACT ORDERS message).
pub const ROUTE_INBOX_SQL: &str = concat!(
    "SELECT inbox.filename, x12.main.transaction_type(inbox.content) AS tx_type, t.shaped_function ",
    "FROM (VALUES ",
    "('remit_0115.edi', '",
    "ISA*00*          *00*          *ZZ*PAYER          *ZZ*CLINIC         *240115*0900*^*00501*000000301*0*P*:~",
    "GS*HP*PAYER*CLINIC*20240115*0900*1*X*005010X221A1~ST*835*0001~BPR*I*400*C*ACH~",
    "CLP*ACCT777*1*500*400*100*MC*CCN0001*11~SE*4*0001~GE*1*1~IEA*1*000000301~'), ",
    "('ack_0101.edi', '",
    "ISA*00*          *00*          *ZZ*PAYER          *ZZ*CLINIC         *240101*1300*^*00501*000000302*0*P*:~",
    "GS*FA*PAYER*CLINIC*20240101*1300*1*X*005010~ST*997*0001~AK1*HC*1~AK2*837*0001~AK5*A~",
    "AK9*A*1*1*1~SE*6*0001~GE*1*1~IEA*1*000000302~'), ",
    "('orders_0102.edi', '",
    "UNA:+.? ''UNB+UNOA:1+SENDER+RECEIVER+240102:1200+REF1''UNH+MSG1+ORDERS:D:96A:UN''",
    "BGM+220+PO12345+9''UNT+3+MSG1''UNZ+1+REF1''')",
    ") AS inbox(filename, content) ",
    "LEFT JOIN x12.main.transaction_sets AS t ",
    "ON t.transaction_set = x12.main.transaction_type(inbox.content) ",
    "ORDER BY inbox.filename;",
);

/// Reconcile a sent 837 batch (`envelope`) against the trading partner's 997
/// (`read_997`): one row per sent transaction with its raw AK5 status, the number
/// of AK3 segment errors, and 'NOT ACKNOWLEDGED' when the 997 has no AK2 for it.
pub const ACK_RECONCILE_SQL: &str = concat!(
    "WITH sent AS (SELECT group_ctrl, transaction_type, transaction_ctrl FROM x12.main.envelope('",
    "ISA*00*          *00*          *ZZ*CLINIC         *ZZ*PAYER          *240101*1200*^*00501*000000101*0*P*:~",
    "GS*HC*CLINIC*PAYER*20240101*1200*1*X*005010X222A1~",
    "ST*837*0001~BHT*0019*00*REF01*20240101*1200*CH~CLM*ACCT777*500***11:B:1*Y~SE*4*0001~",
    "ST*837*0002~BHT*0019*00*REF02*20240101*1200*CH~CLM*ACCT778*1000***11:B:1*Y~SE*4*0002~",
    "ST*837*0003~BHT*0019*00*REF03*20240101*1200*CH~CLM*ACCT779*250***11:B:1*Y~SE*4*0003~",
    "GE*3*1~IEA*1*000000101~')), ",
    "acks AS (SELECT ak1_group_control, ak2_transaction_set_id, ak2_transaction_control, ",
    "any_value(ak5_status) AS ak5_status, count(ak3_segment_id) AS segment_errors ",
    "FROM x12.main.read_997('",
    "ISA*00*          *00*          *ZZ*PAYER          *ZZ*CLINIC         *240101*1300*^*00501*000000501*0*P*:~",
    "GS*FA*PAYER*CLINIC*20240101*1300*5*X*005010~ST*997*0001~AK1*HC*1~",
    "AK2*837*0001~AK5*A~AK2*837*0002~AK3*CLM*3**8~AK4*2*782*1~AK5*R*5~",
    "AK9*P*3*2*1~SE*10*0001~GE*1*5~IEA*1*000000501~') GROUP BY ALL) ",
    "SELECT s.transaction_ctrl, coalesce(a.ak5_status, 'NOT ACKNOWLEDGED') AS ack_status, ",
    "coalesce(a.segment_errors, 0) AS segment_errors ",
    "FROM sent AS s LEFT JOIN acks AS a ON a.ak1_group_control = s.group_ctrl ",
    "AND a.ak2_transaction_set_id = s.transaction_type ",
    "AND a.ak2_transaction_control = s.transaction_ctrl ",
    "ORDER BY s.transaction_ctrl;",
);

/// Reconcile billed claims (`read_837`) against the payer's remittance
/// (`read_835`): billed amount, CLP status code, amount paid, and the unpaid
/// balance per claim; a claim with no CLP yet has NULL status and paid.
/// DISTINCT collapses the SV1 / CAS child fan-out to one row per claim.
pub const CLAIM_REMIT_SQL: &str = concat!(
    "WITH billed AS (SELECT DISTINCT clm_id, CAST(clm_total_charge AS DECIMAL(12, 2)) AS billed ",
    "FROM x12.main.read_837('",
    "ISA*00*          *00*          *ZZ*CLINIC         *ZZ*PAYER          *240101*1200*^*00501*000000201*0*P*:~",
    "GS*HC*CLINIC*PAYER*20240101*1200*1*X*005010X222A1~ST*837*0001~",
    "BHT*0019*00*REF01*20240101*1200*CH~NM1*85*2*BILLING CLINIC*****XX*1122334455~",
    "NM1*IL*1*DOE*JOHN****MI*MEMBER123~",
    "CLM*ACCT777*500***11:B:1*Y~SV1*HC:99213*200*UN*1~SV1*HC:85025*300*UN*1~",
    "CLM*ACCT778*1000***11:B:1*Y~SV1*HC:99214*1000*UN*1~",
    "CLM*ACCT779*250***11:B:1*Y~SV1*HC:99212*250*UN*1~",
    "SE*12*0001~GE*1*1~IEA*1*000000201~')), ",
    "paid AS (SELECT DISTINCT clp_claim_id, clp_status_code, ",
    "CAST(clp_total_paid AS DECIMAL(12, 2)) AS paid FROM x12.main.read_835('",
    "ISA*00*          *00*          *ZZ*PAYER          *ZZ*CLINIC         *240115*0900*^*00501*000000301*0*P*:~",
    "GS*HP*PAYER*CLINIC*20240115*0900*1*X*005010X221A1~ST*835*0001~BPR*I*400*C*ACH~",
    "CLP*ACCT777*1*500*400*100*MC*CCN0001*11~CAS*PR*1*100~",
    "CLP*ACCT778*4*1000*0*0*MC*CCN0002*11~CAS*CO*50*1000~",
    "SE*7*0001~GE*1*1~IEA*1*000000301~')) ",
    "SELECT b.clm_id, b.billed, p.clp_status_code, p.paid, ",
    "b.billed - coalesce(p.paid, 0) AS unpaid ",
    "FROM billed AS b LEFT JOIN paid AS p ON p.clp_claim_id = b.clm_id ",
    "ORDER BY b.clm_id;",
);

/// Description for [`ROUTE_INBOX_SQL`].
pub const ROUTE_INBOX_DESC: &str = "Route an inbox of inline payloads: sniff each one's \
     transaction type with transaction_type and join the transaction_sets reference view to \
     name the shaped read_* function to call (835 -> read_835, 997 -> read_997; the EDIFACT \
     ORDERS message has no shaped view, so NULL).";

/// Description for [`ACK_RECONCILE_SQL`].
pub const ACK_RECONCILE_DESC: &str = "Reconcile a sent three-transaction 837 batch (envelope) \
     against the partner's 997 acknowledgment (read_997), matched on group and transaction \
     control numbers: 0001 has AK5 status A, 0002 has AK5 status R with one AK3 segment error, \
     and 0003 is NOT ACKNOWLEDGED because the 997 has no AK2 for it.";

/// Description for [`CLAIM_REMIT_SQL`].
pub const CLAIM_REMIT_DESC: &str = "Reconcile billed claims (read_837) against the payer's \
     remittance (read_835) by claim id: ACCT777 billed 500 and paid 400 (CLP02 status 1, 100 \
     unpaid), ACCT778 billed 1000 and paid 0 (CLP02 status 4), ACCT779 billed 250 with no \
     remittance yet (NULL status and paid).";

fn example(sql: &str, description: &str) -> FunctionExample {
    FunctionExample {
        sql: sql.to_string(),
        description: description.to_string(),
        expected_output: None,
    }
}

/// Composed example driven by the `transaction_type` scalar.
pub fn route_inbox() -> FunctionExample {
    example(ROUTE_INBOX_SQL, ROUTE_INBOX_DESC)
}

/// Composed example driven by the `envelope` table function.
pub fn ack_reconcile() -> FunctionExample {
    example(ACK_RECONCILE_SQL, ACK_RECONCILE_DESC)
}

/// Composed example driven by the `read_837` shaped view.
pub fn claim_remit() -> FunctionExample {
    example(CLAIM_REMIT_SQL, CLAIM_REMIT_DESC)
}

/// Every composed example, for the schema-level `vgi.example_queries` list.
pub fn all() -> Vec<FunctionExample> {
    vec![route_inbox(), ack_reconcile(), claim_remit()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each composed SQL calls at least two of the worker's own objects, is
    /// catalog-qualified, ordered, and never a bare `SELECT *`.
    #[test]
    fn composed_sql_is_lint_shaped() {
        for ex in all() {
            let objects = [
                "x12.main.transaction_type(",
                "x12.main.transaction_sets",
                "x12.main.envelope(",
                "x12.main.read_997(",
                "x12.main.read_837(",
                "x12.main.read_835(",
            ]
            .into_iter()
            .filter(|o| ex.sql.contains(*o))
            .count();
            assert!(objects >= 2, "not composed: {}", ex.sql);
            assert!(ex.sql.contains("ORDER BY"), "unordered: {}", ex.sql);
            assert!(!ex.sql.contains("SELECT *"), "bare SELECT *: {}", ex.sql);
            assert!(!ex.description.is_empty());
        }
    }

    /// The ISA segments must keep their fixed 106-byte width (the delimiter
    /// sniffer reads ISA16 by position), so a stray edit to the padding breaks
    /// parsing silently.
    #[test]
    fn isa_segments_are_fixed_width() {
        for ex in all() {
            for (i, _) in ex.sql.match_indices("ISA*") {
                let isa = &ex.sql[i..i + 106];
                assert!(isa.ends_with(":~"), "ISA not 106 bytes: {isa}");
            }
        }
    }
}
