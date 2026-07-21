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

use vgi::catalog::{CatSchema, CatView, CatalogModel};
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
                 `.835` / `.837`, which may glob), inline `VARCHAR` content, or a `BLOB`. No \
                 staging, no external service.\n\n\
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
                 needs your own licensed X12 reference."
                    .to_string(),
            ),
            (
                "vgi.agent_test_tasks".to_string(),
                crate::meta::agent_test_tasks_json(&agent_test_tasks()),
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
        // The worker's own build version (the crate's Cargo version), published as
        // catalog metadata rather than as a parameterless *_version() scalar (VGI328).
        implementation_version: Some(x12_core::version().to_string()),
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
                         edifact_segments, edifact_envelope, transaction_sets, delimiters, \
                         transaction_type",
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
                     glob), inline `VARCHAR` content, or a `BLOB` — auto-detected by the \
                     interchange's `ISA`/`UNA`/`UNB` magic prefix.\n\n\
                     Start from the browsable `transaction_sets` reference view to see which \
                     shaped `read_*` view covers each transaction set."
                        .to_string(),
                ),
                (
                    "vgi.categories".to_string(),
                    r#"[
  {"name": "Interchange sniffers", "description": "Scalar routing helpers that read inline content to report its delimiters and transaction-set type — cheap triage before a full parse."},
  {"name": "Segment & element explode", "description": "Full-fidelity raw views: one row per segment (elements as a LIST) or one row per element with composite components and repetitions split out."},
  {"name": "Envelope & structure", "description": "One row per transaction with ISA/GS/ST control metadata and the SE/GE/IEA structural validity flags for completeness checks."},
  {"name": "Shaped transaction views", "description": "Flat relational projections of the common healthcare and B2B transaction sets, keyed by public segment ID and element position."},
  {"name": "UN/EDIFACT", "description": "UNA/UNB/UNH interchange parsing with release-character un-escaping, mirroring the X12 raw and envelope surfaces."},
  {"name": "Reference", "description": "Zero-argument browsable views that describe the worker's own capabilities — the cheapest discovery entry point."}
]"#
                        .to_string(),
                ),
                (
                    "vgi.example_queries".to_string(),
                    r#"[
  {"description": "Pull the elements of every CLM (claim) segment from a folder of 837 claim files.", "sql": "SELECT source_path, segment_index, element_index, value FROM x12.main.segments_elements('/data/claims/*.837') WHERE segment_id = 'CLM';"},
  {"description": "Flag interchanges whose SE segment count fails to validate across an inbound folder.", "sql": "SELECT source_path, interchange_ctrl, transaction_type, se_count_ok FROM x12.main.envelope('/data/inbound/*.edi') WHERE se_count_ok IS DISTINCT FROM TRUE;"},
  {"description": "Extract claim id, total paid, and payer from a folder of 835 remittance files.", "sql": "SELECT clp_claim_id, clp_total_paid, payer_name FROM x12.main.read_835('/data/era/*.835');"},
  {"description": "Classify inbound files by transaction set type for routing.", "sql": "SELECT filename, x12.main.transaction_type(content) AS tx_type FROM read_text('/data/inbound/*.edi');"},
  {"description": "Count exploded EDIFACT elements per message type across an orders folder.", "sql": "SELECT transaction_type, count(*) AS n_elements FROM x12.main.edifact_segments('/data/orders/*.edi') GROUP BY transaction_type;"}
]"#
                        .to_string(),
                ),
            ],
            views: vec![transaction_sets_view()],
            macros: Vec::new(),
            tables: Vec::new(),
        }],
        ..Default::default()
    }
}

/// A zero-argument, browsable **reference** view (VGI146) that maps each X12
/// transaction set the worker projects a shaped view for to that `read_*`
/// function, plus a short general-industry description (no copyrighted TR3
/// text). Backed by inline `VALUES`, so `SELECT * FROM x12.main.transaction_sets`
/// works cold — the cheapest discovery entry point for an analyst or agent who
/// has no interchange to hand yet.
fn transaction_sets_view() -> CatView {
    let definition = "SELECT * FROM (VALUES \
        ('270', 'X12', 'read_270', 'Eligibility, coverage, or benefit inquiry'), \
        ('271', 'X12', 'read_271', 'Eligibility, coverage, or benefit response'), \
        ('835', 'X12', 'read_835', 'Health-care claim payment / remittance advice (ERA)'), \
        ('837', 'X12', 'read_837', 'Health-care claim (professional / institutional / dental)'), \
        ('850', 'X12', 'read_850', 'Purchase order'), \
        ('997', 'X12', 'read_997', 'Functional acknowledgment'), \
        ('999', 'X12', 'read_999', 'Implementation acknowledgment')) \
        AS t(transaction_set, family, shaped_function, summary)"
        .to_string();
    let mut tags = crate::meta::object_tags(
        "Shaped Transaction Sets",
        "A browsable reference view of the X12 transaction sets the worker projects a shaped, \
         flat relational view for. One row per set gives its ST01 identifier, EDI family, the \
         `x12.main` read_* table function that shapes it, and a short general-industry \
         description. Zero-argument — query it cold to discover which shaped function to call \
         before you have an interchange in hand. Descriptions are general industry terms only; \
         no copyrighted TR3 implementation-guide text.",
        "Reference view: one row per shaped X12 transaction set (ST01, family, its read_* \
         function, and a short description). Zero-argument — the cheapest discovery entry point.",
        "transaction sets, reference, discovery, shaped, read_835, read_837, read_270, read_271, \
         read_850, read_997, read_999, 835, 837, 270, 271, 850, 997, 999",
        "Reference",
    );
    // Bare classifying tags for faceting (VGI123), reusing the schema's vocabulary.
    tags.push(("domain".to_string(), "edi-and-interchange".to_string()));
    tags.push(("topic".to_string(), "x12-edifact".to_string()));
    tags.push((
        "vgi.example_queries".to_string(),
        r#"[
  {"description": "List the shaped X12 transaction sets and their read_* function.", "sql": "SELECT transaction_set, shaped_function FROM x12.main.transaction_sets WHERE family = 'X12' ORDER BY transaction_set;"},
  {"description": "Find which shaped function parses 835 remittance advice.", "sql": "SELECT shaped_function, summary FROM x12.main.transaction_sets WHERE transaction_set = '835';"}
]"#
        .to_string(),
    ));
    // At least one guaranteed-runnable, verified example the worker ships (VGI509).
    tags.push((
        "vgi.executable_examples".to_string(),
        r#"[
  {"description": "List every shaped transaction set and its read_* function.", "sql": "SELECT transaction_set, shaped_function FROM x12.main.transaction_sets ORDER BY transaction_set", "expected_result": [{"transaction_set": "270", "shaped_function": "read_270"}, {"transaction_set": "271", "shaped_function": "read_271"}, {"transaction_set": "835", "shaped_function": "read_835"}, {"transaction_set": "837", "shaped_function": "read_837"}, {"transaction_set": "850", "shaped_function": "read_850"}, {"transaction_set": "997", "shaped_function": "read_997"}, {"transaction_set": "999", "shaped_function": "read_999"}]}
]"#
        .to_string(),
    ));
    CatView {
        name: "transaction_sets".to_string(),
        definition,
        comment: Some(
            "Reference view mapping each shaped X12 transaction set to its read_* function."
                .to_string(),
        ),
        tags,
        column_comments: vec![
            (
                "transaction_set".to_string(),
                "X12 ST01 transaction set identifier (e.g. '835').".to_string(),
            ),
            (
                "family".to_string(),
                "EDI family — 'X12' for these shaped views.".to_string(),
            ),
            (
                "shaped_function".to_string(),
                "The x12.main read_* table function that projects a relational view of this set."
                    .to_string(),
            ),
            (
                "summary".to_string(),
                "Short general-industry description of the set (no copyrighted TR3 text)."
                    .to_string(),
            ),
        ],
    }
}

/// The `vgi.agent_test_tasks` suite: at least one deterministic task per catalog
/// object (VGI520), each embedding its own inline interchange so the simulated
/// analyst can reproduce a single canonical answer.
fn agent_test_tasks() -> Vec<crate::meta::AgentTask> {
    use crate::meta::{edifact_task, example_interchange, table_task, AgentTask, EXAMPLE_EDIFACT};

    // Inline bodies mirror the committed data/ fixtures and the shaped example
    // bodies; wrapped in a valid ISA/GS/ST envelope they parse with no file.
    let body_837 = "BHT*0019*00*REF01*20240101*1200*CH~\
        NM1*85*2*BILLING CLINIC*****XX*1122334455~NM1*IL*1*DOE*JOHN****MI*MEMBER123~\
        SBR*P*18**GROUP PLAN~NM1*QC*1*DOE*JANE~CLM*ACCT777*500***11:B:1*Y~\
        HI*ABK:Z1234~SV1*HC:99213*200*UN*1~DTP*472*D8*20240105~";
    let body_835 = "BPR*I*1500*C*ACH~TRN*1*TRACE0001*1512345678~\
        NM1*PR*2*ACME HEALTH PLAN*****PI*PAYER001~NM1*PE*2*WELLNESS CLINIC LLC*****XX*1999999999~\
        CLP*PCN1001*1*500*400*100*MC*CCN0001*11~CAS*CO*45*100~SVC*HC:99213*200*160**1~\
        DTM*232*20240110~CLP*PCN1002*4*1000*0*0*MC*CCN0002*11~";
    let body_270 = "HL*1**20*1~NM1*PR*2*ACME PAYER*****PI*PAYER01~\
        HL*2*1*21*1~NM1*1P*2*PROVIDER GRP*****XX*1444444444~\
        HL*3*2*22*0~NM1*IL*1*DOE*JOHN****MI*MEMBER99~TRN*1*TRACE70*9999999999~EQ*30~";
    let body_271 = "HL*1**20*1~NM1*PR*2*ACME PAYER*****PI*PAYER01~\
        HL*2*1*21*1~NM1*1P*2*PROVIDER GRP*****XX*1444444444~\
        HL*3*2*22*0~NM1*IL*1*DOE*JOHN****MI*MEMBER99~TRN*2*TRACE55*9999999999~\
        EB*1*IND*30**GOLD PPO~EB*B*IND*30****27.5~DTP*291*D8*20240101~";
    let body_850 = "BEG*00*SA*PO9988**20240101~N1*ST*ACME WAREHOUSE*92*DC07~\
        PER*BD*JANE BUYER*TE*5551234~PO1*1*10*EA*4.50**VP*WIDGET-A~PO1*2*5*EA*9.99**VP*WIDGET-B~";
    let body_997 = "AK1*HC*1~AK2*837*0001~AK3*CLM*22**8~AK4*1*1028*1~AK5*E~\
        AK2*837*0002~AK5*A~AK9*P*2*2*1~";
    let body_999 = "AK1*HC*1~AK2*837*0001~IK3*NM1*8**8~IK4*2*1037*7~IK5*R~AK9*R*1*1*0~";

    let env_837 = example_interchange("837", body_837);
    let env_835 = example_interchange("835", body_835);
    let env_270 = example_interchange("270", body_270);
    let env_271 = example_interchange("271", body_271);
    let env_850 = example_interchange("850", body_850);
    let env_997 = example_interchange("997", body_997);
    let env_999 = example_interchange("999", body_999);

    // Canonical ISA (standard delimiters) for the delimiter sniffer task.
    let isa = "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       \
        *240101*1200*^*00501*000000001*0*P*:~";

    vec![
        AgentTask {
            name: "sniff_element_separator".to_string(),
            prompt: format!(
                "Sniff the element separator character used by this ISA header string: '{isa}'. \
                 Return one row with a single column named element_sep."
            ),
            reference_sql: format!(
                "SELECT (x12.main.delimiters('{isa}')).element AS element_sep"
            ),
            unordered: true,
            ignore_column_names: true,
        },
        AgentTask {
            name: "detect_transaction_type".to_string(),
            prompt: format!(
                "Detect the transaction set type of this X12 interchange using the worker's \
                 inline-content support (do not write it to a file): '{env_835}'. Return one \
                 column named tx_type."
            ),
            reference_sql: format!(
                "SELECT x12.main.transaction_type('{env_835}') AS tx_type"
            ),
            unordered: true,
            ignore_column_names: true,
        },
        AgentTask {
            name: "list_shaped_transaction_sets".to_string(),
            prompt: "Using the transaction_sets reference view, which shaped read_* function \
                     parses X12 transaction set 835? Return its shaped_function."
                .to_string(),
            reference_sql:
                "SELECT shaped_function FROM x12.main.transaction_sets WHERE transaction_set = '835'"
                    .to_string(),
            unordered: true,
            ignore_column_names: true,
        },
        table_task(
            "count_segments",
            "Count the segments in the interchange. Return one column named n.",
            "segments",
            "count(*) AS n",
            &env_837,
        ),
        table_task(
            "count_elements",
            "Count the exploded elements in the interchange. Return one column named n.",
            "segments_elements",
            "count(*) AS n",
            &env_837,
        ),
        table_task(
            "check_envelope",
            "Report the transaction type and whether the SE segment count validates. Return \
             columns transaction_type and se_count_ok.",
            "envelope",
            "transaction_type, se_count_ok",
            &env_837,
        ),
        table_task(
            "shaped_835_claims",
            "Project the claim ids and total-paid amounts from the 835 remittance interchange. \
             Return columns clp_claim_id and clp_total_paid.",
            "read_835",
            "clp_claim_id, clp_total_paid",
            &env_835,
        ),
        table_task(
            "shaped_837_claims",
            "Project the billing provider NPI and subscriber id from the 837 claim interchange. \
             Return columns billing_provider_npi and subscriber_id.",
            "read_837",
            "billing_provider_npi, subscriber_id",
            &env_837,
        ),
        table_task(
            "shaped_270_inquiry",
            "Project the hierarchical-level ids and level codes from the 270 inquiry. Return \
             columns hl_id and hl_level_code.",
            "read_270",
            "hl_id, hl_level_code",
            &env_270,
        ),
        table_task(
            "shaped_271_response",
            "Project the hierarchical-level ids and plan descriptions from the 271 response. \
             Return columns hl_id and eb_plan_description.",
            "read_271",
            "hl_id, eb_plan_description",
            &env_271,
        ),
        table_task(
            "shaped_850_po",
            "Project the purchase-order line numbers and product ids from the 850. Return \
             columns po1_line_number and po1_product_id.",
            "read_850",
            "po1_line_number, po1_product_id",
            &env_850,
        ),
        table_task(
            "shaped_997_ack",
            "Project the acknowledged transaction control numbers and their statuses from the \
             997. Return columns ak2_transaction_control and ak5_status.",
            "read_997",
            "ak2_transaction_control, ak5_status",
            &env_997,
        ),
        table_task(
            "shaped_999_ack",
            "Project the acknowledged transaction control numbers and their statuses from the \
             999. Return columns ak2_transaction_control and ik5_status.",
            "read_999",
            "ak2_transaction_control, ik5_status",
            &env_999,
        ),
        edifact_task(
            "edifact_explode",
            "Explode the EDIFACT interchange and project each segment tag and value. Return \
             columns segment_id and value.",
            "edifact_segments",
            "segment_id, value",
            EXAMPLE_EDIFACT,
        ),
        edifact_task(
            "edifact_summary",
            "Summarize the EDIFACT interchange and return its message transaction_type.",
            "edifact_envelope",
            "transaction_type",
            EXAMPLE_EDIFACT,
        ),
    ]
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
