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

/// Build the `vgi.agent_test_tasks` JSON value: a fixed suite of analyst tasks
/// that `vgi-lint simulate` runs. Each `(name, prompt, reference_sql)` triple
/// becomes a task object.
pub fn agent_test_tasks_json(tasks: &[(&str, &str, &str)]) -> String {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }
    let items: Vec<String> = tasks
        .iter()
        .map(|(name, prompt, reference_sql)| {
            format!(
                "{{\"name\":\"{}\",\"prompt\":\"{}\",\"reference_sql\":\"{}\"}}",
                esc(name),
                esc(prompt),
                esc(reference_sql)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Render a `vgi.result_columns_md` Markdown table (VGI307) from a table
/// function's output schema, using each field's `comment` metadata as the
/// description. Function-backed tables have a schema DuckDB can't expose
/// statically, so this documents the returned columns for discovery.
pub fn result_columns_md(schema: &arrow_schema::SchemaRef) -> String {
    let mut md = String::from("| column | type | description |\n|---|---|---|\n");
    for field in schema.fields() {
        let comment = field
            .metadata()
            .get("comment")
            .map(String::as_str)
            .unwrap_or("");
        md.push_str(&format!(
            "| `{}` | {} | {} |\n",
            field.name(),
            sql_type(field.data_type()),
            comment
        ));
    }
    md
}

/// Map an Arrow type to the SQL type name DuckDB exposes it as.
fn sql_type(ty: &arrow_schema::DataType) -> &'static str {
    use arrow_schema::DataType;
    match ty {
        DataType::Utf8 | DataType::LargeUtf8 => "VARCHAR",
        DataType::Int64 => "BIGINT",
        DataType::Int32 => "INTEGER",
        DataType::Boolean => "BOOLEAN",
        DataType::List(_) | DataType::LargeList(_) => "LIST(VARCHAR)",
        _ => "VARCHAR",
    }
}

/// Build the four standard per-object discovery/description tags (title,
/// doc_llm, doc_md, keywords).
pub fn object_tags(
    title: &str,
    description_llm: &str,
    description_md: &str,
    keywords: &str,
) -> Vec<(String, String)> {
    vec![
        ("vgi.title".to_string(), title.to_string()),
        ("vgi.doc_llm".to_string(), description_llm.to_string()),
        ("vgi.doc_md".to_string(), description_md.to_string()),
        ("vgi.keywords".to_string(), keywords_json(keywords)),
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
