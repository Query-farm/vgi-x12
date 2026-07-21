//! Shared helpers for the per-object discovery/description metadata the
//! `vgi-lint` strict profile expects on every function and table.
//!
//! Per-object `vgi.source_url` is intentionally NOT emitted here — `vgi.source_url`
//! belongs on the catalog object only (VGI139). The catalog's `source_url` field
//! already points at the repo.

/// Encode comma-separated keywords as the JSON array of strings that
/// `vgi.keywords` requires (VGI138).
pub fn keywords_json(keywords: &str) -> String {
    let items: Vec<String> = keywords
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|k| {
            let escaped = k.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// One analyst task in the `vgi.agent_test_tasks` suite: a `name`, the `prompt`
/// shown to the simulated analyst, the canonical `reference_sql`, and the two
/// result-comparison relaxations the grader honors — `unordered` (row order is
/// insignificant) and `ignore_column_names` (compare VALUES only, not column
/// labels). Keeping these explicit per task makes each reference deterministic
/// under the linter's strict result compare.
pub struct AgentTask {
    pub name: String,
    pub prompt: String,
    pub reference_sql: String,
    pub unordered: bool,
    pub ignore_column_names: bool,
}

/// Build one agent test task whose `reference_sql` runs
/// `SELECT <select> FROM x12.main.<fn_name>('<interchange>')` over an inline X12
/// interchange, with a prompt that embeds the same interchange so the simulated
/// analyst can reproduce the exact single answer. `interchange` must be X12
/// (its `*`/`~`/`:`/`^` delimiters carry no single quote, so it embeds directly
/// in the SQL string literal).
pub fn table_task(
    name: &str,
    ask: &str,
    fn_name: &str,
    select: &str,
    interchange: &str,
) -> AgentTask {
    AgentTask {
        name: name.to_string(),
        prompt: format!(
            "{ask} Use the worker's inline-content support (do not write it to a file) over this \
             X12 interchange: '{interchange}'."
        ),
        reference_sql: format!("SELECT {select} FROM x12.main.{fn_name}('{interchange}')"),
        unordered: true,
        ignore_column_names: true,
    }
}

/// Like [`table_task`] but for a UN/EDIFACT `interchange`, whose `'` segment
/// terminator is doubled for the SQL string literal (the prompt keeps it raw).
pub fn edifact_task(
    name: &str,
    ask: &str,
    fn_name: &str,
    select: &str,
    interchange: &str,
) -> AgentTask {
    AgentTask {
        name: name.to_string(),
        prompt: format!(
            "{ask} Use the worker's inline-content support (do not write it to a file) over this \
             UN/EDIFACT interchange: '{interchange}'."
        ),
        reference_sql: format!(
            "SELECT {select} FROM x12.main.{fn_name}('{}')",
            interchange.replace('\'', "''")
        ),
        unordered: true,
        ignore_column_names: true,
    }
}

/// Build the `vgi.agent_test_tasks` JSON value: a fixed suite of analyst tasks
/// that `vgi-lint simulate` runs. Each [`AgentTask`] becomes a task object,
/// emitting the `unordered` / `ignore_column_names` grading flags so a
/// single-answer prompt grades deterministically.
pub fn agent_test_tasks_json(tasks: &[AgentTask]) -> String {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }
    let items: Vec<String> = tasks
        .iter()
        .map(|t| {
            format!(
                "{{\"name\":\"{}\",\"prompt\":\"{}\",\"reference_sql\":\"{}\",\
                 \"unordered\":{},\"ignore_column_names\":{}}}",
                esc(&t.name),
                esc(&t.prompt),
                esc(&t.reference_sql),
                t.unordered,
                t.ignore_column_names,
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Escape a string for embedding in a JSON string literal.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render the `vgi.result_columns_schema` JSON (VGI307) from a table function's
/// output schema, using each field's `comment` metadata as the description.
/// Function-backed tables have a schema DuckDB can't expose statically, so this
/// documents the returned columns as a JSON array of `{name, type, description}`
/// objects for discovery.
pub fn result_columns_schema(schema: &arrow_schema::SchemaRef) -> String {
    let items: Vec<String> = schema
        .fields()
        .iter()
        .map(|field| {
            let comment = field
                .metadata()
                .get("comment")
                .map(String::as_str)
                .unwrap_or("");
            format!(
                "{{\"name\":{},\"type\":{},\"description\":{}}}",
                json_str(field.name()),
                json_str(sql_type(field.data_type())),
                json_str(comment),
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Build the `vgi.example_queries` JSON tag (VGI515) from a function's native
/// [`vgi::FunctionExample`] list: a JSON array of `{description, sql}` objects.
/// The native `duckdb_functions().examples` carrier drops per-example
/// descriptions, so this described-JSON tag is the carrier the linter reads;
/// keep the `sql` byte-identical to the native example so the two dedupe.
pub fn example_queries_tag(examples: &[vgi::FunctionExample]) -> String {
    let items: Vec<String> = examples
        .iter()
        .map(|ex| {
            format!(
                "{{\"description\":{},\"sql\":{}}}",
                json_str(&ex.description),
                json_str(&ex.sql),
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Map an Arrow type to the SQL type name DuckDB exposes it as.
fn sql_type(ty: &arrow_schema::DataType) -> &'static str {
    use arrow_schema::DataType;
    match ty {
        DataType::Utf8 | DataType::LargeUtf8 => "VARCHAR",
        DataType::Int64 => "BIGINT",
        DataType::Int32 => "INTEGER",
        DataType::Boolean => "BOOLEAN",
        DataType::List(_) | DataType::LargeList(_) => "VARCHAR[]",
        _ => "VARCHAR",
    }
}

/// Build the standard per-object discovery/description tags (title, doc_llm,
/// doc_md, keywords) plus the `vgi.category` that places the object in the
/// schema's `vgi.categories` registry (VGI413). `category` MUST name one of the
/// categories declared on the schema in `main.rs`.
pub fn object_tags(
    title: &str,
    description_llm: &str,
    description_md: &str,
    keywords: &str,
    category: &str,
) -> Vec<(String, String)> {
    vec![
        ("vgi.title".to_string(), title.to_string()),
        ("vgi.doc_llm".to_string(), description_llm.to_string()),
        ("vgi.doc_md".to_string(), description_md.to_string()),
        ("vgi.keywords".to_string(), keywords_json(keywords)),
        ("vgi.category".to_string(), category.to_string()),
    ]
}

/// The canonical fixed-width X12 ISA header (interchange control `000000001`)
/// used to build inline, runnable examples. It declares the standard
/// `*` (element) / `~` (segment) / `:` (component) / `^` (repetition) delimiters
/// the worker sniffs back out, and MUST stay on a single source line so its
/// fixed-width spacing is preserved byte-for-byte.
const EXAMPLE_ISA: &str = "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~";

/// Wrap a transaction-set `body` (its segments, each terminated by `~`) in a
/// minimal, valid X12 `ISA`/`GS`/`ST`…`SE`/`GE`/`IEA` envelope, yielding one
/// complete interchange string. Because the worker's table functions
/// auto-detect inline content by its `ISA` prefix, the result is parsed directly
/// — no file needed — so it drives a self-contained, runnable example.
pub fn example_interchange(st01: &str, body: &str) -> String {
    // SE01 is the count of segments in the set, inclusive of ST and SE. `body`
    // holds one `~`-terminated segment per terminator, so the count is the number
    // of `~` in `body` plus the ST and SE segments themselves. Computing it keeps
    // the example's `se_count_ok` structural flag TRUE.
    let se_count = body.matches('~').count() + 2;
    format!(
        "{EXAMPLE_ISA}GS*XX*SEND*RECV*20240101*1200*1*X*005010~\
         ST*{st01}*0001~{body}SE*{se_count}*0001~GE*1*1~IEA*1*000000001~"
    )
}

/// Build one runnable [`vgi::FunctionExample`] for a table function: a
/// `SELECT <select> FROM x12.main.<fn_name>('<inline interchange>')` over the
/// `st01`/`body` wrapped in the standard envelope. The interchange contains no
/// single quotes, so it embeds directly in the SQL string literal.
pub fn table_example(
    fn_name: &str,
    select: &str,
    st01: &str,
    body: &str,
    description: &str,
) -> vgi::FunctionExample {
    let interchange = example_interchange(st01, body);
    vgi::FunctionExample {
        sql: format!("SELECT {select} FROM x12.main.{fn_name}('{interchange}')"),
        description: description.to_string(),
        expected_output: None,
    }
}

/// Build one runnable [`vgi::FunctionExample`] for an EDIFACT table function. EDIFACT
/// terminates segments with `'`, which must be doubled to embed in a SQL string
/// literal; `interchange` is the raw UNA/UNB… text and is escaped here.
pub fn edifact_example(
    fn_name: &str,
    select: &str,
    interchange: &str,
    description: &str,
) -> vgi::FunctionExample {
    let escaped = interchange.replace('\'', "''");
    vgi::FunctionExample {
        sql: format!("SELECT {select} FROM x12.main.{fn_name}('{escaped}')"),
        description: description.to_string(),
        expected_output: None,
    }
}

/// A small, valid UN/EDIFACT ORDERS interchange (UNA service-string advice +
/// UNB…UNZ) used for the EDIFACT table-function examples. Segment terminator is
/// `'`; the release character is `?`.
pub const EXAMPLE_EDIFACT: &str = "UNA:+.? 'UNB+UNOA:1+SENDER+RECEIVER+240101:1200+REF1'UNH+MSG1+ORDERS:D:96A:UN'BGM+220+PO12345+9'DTM+137:20240101:102'NAD+BY+++ACME CORP'UNT+4+MSG1'UNZ+1+REF1'";
