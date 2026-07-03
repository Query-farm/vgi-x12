//! The `x12` VGI worker.
//!
//! A standalone binary that DuckDB launches and talks to over Apache Arrow IPC
//! (`ATTACH 'x12' (TYPE vgi, COMMAND 'x12-worker')`). It parses ANSI ASC X12 EDI
//! and UN/EDIFACT interchanges into queryable segment / element / envelope /
//! shaped rows under the catalog `x12`, schema `main`:
//!
//! ```sql
//! ATTACH 'x12' AS x12 (TYPE vgi, COMMAND 'x12-worker');
//!
//! SELECT * FROM x12.segments_elements('/data/claims/*.837') WHERE segment_id = 'CLM';
//! SELECT * FROM x12.envelope('/data/inbound/*.edi');
//! SELECT clp_claim_id, clp_total_paid FROM x12.read_835('/data/era/*.835');
//! SELECT x12.delimiters(content), x12.transaction_type(content) FROM read_text('/data/*.edi');
//! ```
//!
//! All parsing is local (no network surface) and ships **public X12 syntax
//! only** — no copyrighted TR3 implementation-guide content. The pure parser
//! lives in the `x12-core` crate; the `scalar/` and `table/` modules are the
//! thin Arrow adapters over it.

mod arrow_io;
mod meta;
mod scalar;
mod source;
mod table;

use vgi::catalog::{CatSchema, CatalogModel};
use vgi::Worker;

/// Catalog + schema metadata (descriptions, provenance, tags) surfaced to DuckDB
/// and the `vgi-lint` metadata-quality linter. The function objects themselves
/// are served from the registered scalars/tables; this adds catalog/schema-level
/// comments and tags.
fn catalog_metadata(name: &str) -> CatalogModel {
    CatalogModel {
        name: name.to_string(),
        comment: Some(
            "Parse ANSI ASC X12 EDI and UN/EDIFACT interchanges into queryable segment / \
             element / envelope / shaped rows. Public syntax only; parsing is 100% local."
                .to_string(),
        ),
        tags: vec![
            (
                "vgi.title".to_string(),
                "X12 / EDIFACT EDI Parser".to_string(),
            ),
            (
                "vgi.keywords".to_string(),
                crate::meta::keywords_json(
                    "x12, edi, edifact, ansi asc x12, healthcare edi, 837, 835, 270, 271, 850, \
                     997, 999, claim, remittance, era, eligibility, purchase order, isa, gs, st, \
                     segment, element, envelope, hipaa, b2b",
                ),
            ),
            (
                "vgi.doc_llm".to_string(),
                "Parse ANSI ASC X12 EDI (and UN/EDIFACT) interchanges directly in SQL: sniff the \
                 interchange's own delimiters from the fixed-width ISA, explode the ISA/GS/ST \
                 envelope into segment / element rows, validate structural segment counts and \
                 control numbers, detect the transaction type, and project shaped relational views \
                 for the common healthcare / B2B sets (837 claims, 835 remittance, 270/271 \
                 eligibility, 850 purchase order, 997/999 acknowledgements). Parsing is 100% local \
                 — no outbound calls — which suits PHI/PII workloads. Ships public X12 syntax only; \
                 raw codes are surfaced verbatim and code-value translation needs the user's own \
                 licensed X12 reference."
                    .to_string(),
            ),
            (
                "vgi.doc_md".to_string(),
                "# x12 — ANSI ASC X12 / UN-EDIFACT EDI in SQL\n\n\
                 **Query raw EDI interchanges directly from DuckDB** — files (`.edi` / `.x12` / \
                 `.835` / `.837`, which may glob), inline VARCHAR content, or a BLOB. No staging, \
                 no external service.\n\n\
                 **What it does.** It sniffs each interchange's own delimiters from the fixed-width \
                 ISA header, walks the ISA/GS/ST nesting, and hands back rows you can filter, join, \
                 and aggregate in SQL. Structural counts and control numbers are validated, so you \
                 can tell a complete interchange from a truncated one.\n\n\
                 **Pick your level of shaping.** Work at the *raw* level — one row per segment, or \
                 one row per element with composite components and repetitions split out — for \
                 full-fidelity inspection; at the *interchange-summary* level — one row per \
                 transaction with its ISA/GS/ST control metadata and the SE/GE/IEA structural \
                 validity flags — for envelope checks; or at the *shaped* level — flat relational \
                 projections of the common healthcare and B2B transaction sets, keyed by public \
                 segment ID and element position. Inline delimiter- and type-sniffing scalars let \
                 you route or triage content before a full parse, and UN/EDIFACT (UNA/UNB/UNH, \
                 release-character un-escaping) is handled alongside X12.\n\n\
                 **When to reach for it.** Ad-hoc inspection and structural validation of \
                 inbound/outbound EDI, extracting claim / remittance / eligibility / \
                 purchase-order fields into tables, or classifying files by transaction type \
                 before loading.\n\n\
                 **Data residency.** Parsing is 100% local with no outbound calls — a feature for \
                 PHI/PII workloads. Ships **public X12 syntax only**: it embeds no copyrighted ASC \
                 X12 TR3 implementation-guide content, so human-readable code-value translation \
                 needs your own licensed X12 reference. Part of the \
                 [Query.Farm](https://query.farm) VGI ecosystem — see the \
                 [source repository](https://github.com/Query-farm/vgi-x12)."
                    .to_string(),
            ),
            (
                "vgi.agent_test_tasks".to_string(),
                crate::meta::agent_test_tasks_json(&[
                    crate::meta::AgentTask {
                        name: "worker_version",
                        prompt: "What version of the x12 worker is running? Return one row with a \
                                 single column named version.",
                        reference_sql: "SELECT x12.main.x12_version() AS version",
                        unordered: true,
                        ignore_column_names: false,
                    },
                    crate::meta::AgentTask {
                        name: "detect_transaction_type",
                        // Embed the exact interchange in the prompt so the answer
                        // is a single deterministic value ('835') the analyst can
                        // reproduce — an open "I have a string" prompt has no fixed
                        // answer and can't be graded against a canonical reference.
                        prompt: "Detect the transaction set type of this X12 interchange string, \
                                 using the worker's inline content support (do not write it to a \
                                 file): \
                                 'ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*1*0*P*:~GS*HP*S*R*20240101*1200*1*X*005010X221A1~ST*835*0001~SE*1*0001~GE*1*1~IEA*1*1~'. \
                                 Return one row with a single column named tx_type.",
                        reference_sql: "SELECT x12.main.transaction_type('ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*1*0*P*:~GS*HP*S*R*20240101*1200*1*X*005010X221A1~ST*835*0001~SE*1*0001~GE*1*1~IEA*1*1~') AS tx_type",
                        unordered: true,
                        ignore_column_names: false,
                    },
                    crate::meta::AgentTask {
                        name: "sniff_element_separator",
                        prompt: "Sniff the element separator character used by this ISA header \
                                 string: \
                                 'ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~'. \
                                 Return one row with a single column named element_sep.",
                        reference_sql: "SELECT (x12.main.delimiters('ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~')).element AS element_sep",
                        unordered: true,
                        ignore_column_names: false,
                    },
                ]),
            ),
            ("vgi.author".to_string(), "Query.Farm".to_string()),
            (
                "vgi.copyright".to_string(),
                "Copyright 2026 Query Farm LLC - https://query.farm".to_string(),
            ),
            ("vgi.license".to_string(), "MIT".to_string()),
            (
                "vgi.support_contact".to_string(),
                "https://github.com/Query-farm/vgi-x12/issues".to_string(),
            ),
            (
                "vgi.support_policy_url".to_string(),
                "https://github.com/Query-farm/vgi-x12/blob/main/README.md".to_string(),
            ),
        ],
        source_url: Some("https://github.com/Query-farm/vgi-x12".to_string()),
        schemas: vec![CatSchema {
            name: "main".to_string(),
            comment: Some(
                "X12 / EDIFACT parsing functions: generic segment/element explode, envelope \
                 metadata, shaped healthcare/B2B views, and delimiter/type sniffers."
                    .to_string(),
            ),
            tags: vec![
                ("vgi.title".to_string(), "x12 — main".to_string()),
                (
                    "vgi.keywords".to_string(),
                    crate::meta::keywords_json(
                        "x12, edi, edifact, segments, segments_elements, envelope, read_835, \
                         read_837, read_270, read_271, read_850, read_997, read_999, \
                         edifact_segments, delimiters, transaction_type",
                    ),
                ),
                ("domain".to_string(), "edi-and-interchange".to_string()),
                ("category".to_string(), "parsing".to_string()),
                ("topic".to_string(), "x12-edifact".to_string()),
                (
                    "vgi.doc_llm".to_string(),
                    "X12 / EDIFACT parsing functions: explode interchanges into segment/element \
                     rows, summarize the ISA/GS/ST envelope with structural validity flags, \
                     project shaped relational views for 835/837/270/271/850/997/999, and sniff \
                     delimiters / transaction type from inline content."
                        .to_string(),
                ),
                (
                    "vgi.doc_md".to_string(),
                    "## The `main` schema\n\n\
                     The single schema for the `x12` worker. It groups every parsing surface for \
                     ANSI ASC X12 and UN/EDIFACT interchanges, so you attach one worker and reach \
                     the raw, envelope, and shaped views from the same place.\n\n\
                     **Choose a function by how much shaping you want:**\n\n\
                     - *Raw* — segment- and element-level explosion for full-fidelity inspection, \
                     with composite components and repetitions split out.\n\
                     - *Interchange summary* — one row per transaction carrying the ISA/GS/ST \
                     control metadata and the SE/GE/IEA structural validity flags, for \
                     envelope-level completeness checks.\n\
                     - *Shaped* — flat relational projections of the common healthcare and B2B \
                     transaction sets, keyed by public segment ID and element position, for direct \
                     column access.\n\n\
                     Inline delimiter- and type-sniffing scalars let you route or triage content \
                     before committing to a full parse, and the UN/EDIFACT family (UNA/UNB/UNH, \
                     release-character un-escaping) is handled alongside X12.\n\n\
                     Every function accepts the same overloaded input: a file path (which may \
                     glob), inline VARCHAR content, or a BLOB — auto-detected by the \
                     interchange's `ISA`/`UNA`/`UNB` magic prefix."
                        .to_string(),
                ),
                (
                    "vgi.categories".to_string(),
                    r#"[
  {"name": "Interchange sniffers", "description": "Scalar routing helpers that read inline content to report its delimiters, transaction-set type, and the worker version — cheap triage before a full parse."},
  {"name": "Segment & element explode", "description": "Full-fidelity raw views: one row per segment (elements as a LIST) or one row per element with composite components and repetitions split out."},
  {"name": "Envelope & structure", "description": "One row per transaction with ISA/GS/ST control metadata and the SE/GE/IEA structural validity flags for completeness checks."},
  {"name": "Shaped transaction views", "description": "Flat relational projections of the common healthcare and B2B transaction sets, keyed by public segment ID and element position."},
  {"name": "UN/EDIFACT", "description": "UNA/UNB/UNH interchange parsing with release-character un-escaping, mirroring the X12 raw and envelope surfaces."}
]"#
                        .to_string(),
                ),
                (
                    "vgi.example_queries".to_string(),
                    "SELECT * FROM x12.segments_elements('/data/claims/*.837') WHERE segment_id = 'CLM';\n\
                     SELECT * FROM x12.envelope('/data/inbound/*.edi');\n\
                     SELECT clp_claim_id, clp_total_paid FROM x12.read_835('/data/era/*.835');\n\
                     SELECT x12.transaction_type(content) FROM read_text('/data/inbound/*.edi');\n\
                     SELECT * FROM x12.edifact_segments('/data/orders/*.edi');"
                        .to_string(),
                ),
            ],
            views: Vec::new(),
            macros: Vec::new(),
            tables: Vec::new(),
        }],
        ..Default::default()
    }
}

fn main() {
    // Logs MUST go to stderr — stdout is the Arrow-IPC channel.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().filter_or("VGI_LOG", "info"))
        .format_timestamp_millis()
        .try_init();

    // The catalog name DuckDB sees in `ATTACH 'x12' (TYPE vgi, …)`. Default to
    // `x12`, but honor an explicit override so a test harness can rename it.
    if std::env::var_os("VGI_WORKER_CATALOG_NAME").is_none() {
        std::env::set_var("VGI_WORKER_CATALOG_NAME", "x12");
    }
    let catalog_name =
        std::env::var("VGI_WORKER_CATALOG_NAME").unwrap_or_else(|_| "x12".to_string());

    let mut worker = Worker::new();
    scalar::register(&mut worker);
    table::register(&mut worker);
    worker.set_catalog(catalog_metadata(&catalog_name));
    worker.run();
}
