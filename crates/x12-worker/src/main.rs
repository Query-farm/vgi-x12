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
                 **Query raw `.edi` / `.x12` / `.835` / `.837` files directly from DuckDB.** The \
                 `x12` worker sniffs each interchange's delimiters from its fixed-width ISA, \
                 explodes the ISA/GS/ST envelope into segment and element rows, validates the \
                 structural segment counts and control numbers, and — for the common healthcare \
                 and B2B transaction sets — projects shaped, relational views keyed by public \
                 segment IDs.\n\n\
                 **Generic surface.** `segments` (one row per segment, elements as a LIST), \
                 `segments_elements` (one row per element, split into composite components and \
                 repetitions), and `envelope` (one row per ST transaction with ISA/GS/ST metadata \
                 and the SE/GE/IEA structural validity flags). Two scalars, `delimiters(content)` \
                 and `transaction_type(content)`, sniff inline content for routing.\n\n\
                 **Shaped views.** `read_835` (remittance / ERA), `read_837` (claim), `read_270` / \
                 `read_271` (eligibility), `read_850` (purchase order), and `read_997` / `read_999` \
                 (functional acknowledgements). Each carries the four envelope keys plus positional \
                 columns named only by public segment ID and element position (`clp_total_paid` = \
                 `CLP04`); raw codes are surfaced verbatim.\n\n\
                 **UN/EDIFACT.** `edifact_segments` and `edifact_envelope` handle the UNA/UNB/UNH \
                 variant, including release-character un-escaping.\n\n\
                 Every table function accepts a file `path` (which may glob) or inline content via \
                 inline VARCHAR content or a BLOB. Parsing makes **no outbound calls** — a feature for \
                 PHI/PII data-residency, not a footnote. The worker ships the **public X12 \
                 syntax** only; it embeds no copyrighted ASC X12 TR3 implementation-guide content, \
                 so human-readable code translation requires your own licensed X12 reference. \
                 Part of the [Query.Farm](https://query.farm) VGI ecosystem — see the \
                 [source repository](https://github.com/Query-farm/vgi-x12)."
                    .to_string(),
            ),
            (
                "vgi.agent_test_tasks".to_string(),
                crate::meta::agent_test_tasks_json(&[
                    (
                        "worker_version",
                        "What version of the x12 worker is running? Return one row with a single \
                         column named version.",
                        "SELECT x12.main.x12_version() AS version",
                    ),
                    (
                        "detect_transaction_type",
                        "I have an X12 interchange in a string. Detect its transaction set type. \
                         Return one column named tx_type.",
                        "SELECT x12.main.transaction_type('ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*1*0*P*:~GS*HP*S*R*20240101*1200*1*X*005010X221A1~ST*835*0001~SE*1*0001~GE*1*1~IEA*1*1~') AS tx_type",
                    ),
                    (
                        "sniff_element_separator",
                        "Sniff the element separator byte from an ISA header string. Return one \
                         column named element_sep.",
                        "SELECT (x12.main.delimiters('ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~')).element AS element_sep",
                    ),
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
                    "The single schema for the `x12` worker. It holds the generic explode \
                     functions (`segments`, `segments_elements`, `envelope`), the shaped views \
                     (`read_835`/`read_837`/`read_270`/`read_271`/`read_850`/`read_997`/`read_999`), \
                     the EDIFACT functions (`edifact_segments`, `edifact_envelope`), and the \
                     `delimiters` / `transaction_type` / `x12_version` scalars."
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
