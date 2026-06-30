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
