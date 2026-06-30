//! Input resolution shared by every table function: the **path | text | bytes**
//! modes overloaded onto one positional `input` argument, plus the five
//! envelope-key columns every emitted row carries.
//!
//! - a VARCHAR file path or glob (or a `LIST<VARCHAR>` of them) — the streaming
//!   hot path; the worker reads local files the host already exposes.
//! - inline VARCHAR content (text) — parsed directly; its `source_path` is NULL.
//! - inline BLOB content (bytes) — parsed directly; its `source_path` is NULL.
//!
//! The mode is auto-detected from the `ISA` / `UNA` / `UNB` magic prefix (an
//! explicit `mode =>` argument overrides it).
//!
//! There is **no** network / object-store surface here — parsing is 100% local
//! (the data-residency feature for PHI/PII workloads, not an omission).

use arrow_array::cast::AsArray;
use arrow_array::Array;
use arrow_schema::{DataType, Field};
use vgi::arguments::Arguments;
use vgi::ArgSpec;
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::{commented, Cell};

/// One resolved input document: its source path (NULL for inline content) and
/// the raw bytes to parse.
pub struct InputDoc {
    pub source_path: Option<String>,
    pub bytes: Vec<u8>,
}

fn ve(msg: impl Into<String>) -> RpcError {
    RpcError::value_error(msg.into())
}

/// The argument specs every table function shares: a single overloaded `input`
/// argument (path | text | bytes) plus an optional `mode` override.
///
/// DuckDB requires a positional argument to be present, so the three input modes
/// are overloaded onto **one** positional `input` argument whose meaning is
/// auto-detected: content beginning with the `ISA` (X12) or `UNA`/`UNB`
/// (EDIFACT) magic prefix is parsed as **inline content**; anything else is
/// treated as a file **path or glob** (or a list of them). Pass
/// `mode => 'path'` or `mode => 'content'` to force the interpretation.
pub fn input_arg_specs() -> Vec<ArgSpec> {
    vec![
        ArgSpec::const_arg(
            "input",
            0,
            "any",
            "The EDI to parse, overloaded across three auto-detected input modes: a file path or \
             glob (e.g. '/data/claims/*.837'), or a list of paths — the streaming hot path, read \
             locally in sorted order; OR an inline interchange string; OR inline interchange \
             bytes. Inline content is recognized by its ISA / UNA / UNB magic prefix; anything \
             else is treated as a file path. Inline content has a NULL source_path. Parsing is \
             100% local — no outbound calls. Use `mode =>` to force path versus content.",
        ),
        ArgSpec::const_arg(
            "mode",
            -1,
            "varchar",
            "Force how `input` is interpreted: 'path' (read file(s)/glob) or 'content' (parse the \
             value inline). Omit to auto-detect from the ISA/UNA/UNB magic prefix.",
        ),
    ]
}

/// Resolve the call's arguments to one or more input documents from the single
/// overloaded `input` argument.
pub fn resolve(args: &Arguments) -> Result<Vec<InputDoc>> {
    let mode = args.named_str("mode");
    let forced_content = matches!(mode.as_deref(), Some("content") | Some("text"));
    let forced_path = matches!(mode.as_deref(), Some("path") | Some("file"));

    // A BLOB positional is always inline content (you never address a file by
    // its bytes).
    if let Some(bytes) = args.const_bytes(0) {
        if forced_path {
            return Err(ve("mode => 'path' is incompatible with BLOB content"));
        }
        return Ok(vec![InputDoc {
            source_path: None,
            bytes,
        }]);
    }

    let strs = string_args(args, 0)?;
    let mut out = Vec::new();
    for s in strs {
        let is_content = if forced_content {
            true
        } else if forced_path {
            false
        } else {
            // Auto-detect: a recognizable interchange prefix means inline content.
            x12_core::delimiters::detect_family(s.as_bytes())
                != x12_core::delimiters::Family::Unknown
        };
        if is_content {
            out.push(InputDoc {
                source_path: None,
                bytes: s.into_bytes(),
            });
        } else {
            for f in expand_local(&s)? {
                let bytes = std::fs::read(&f).map_err(|e| ve(format!("read {f}: {e}")))?;
                out.push(InputDoc {
                    source_path: Some(f),
                    bytes,
                });
            }
        }
    }
    Ok(out)
}

/// Read the positional argument at `pos` as one or more strings: a single
/// VARCHAR, or a `LIST(VARCHAR)`.
fn string_args(args: &Arguments, pos: usize) -> Result<Vec<String>> {
    if let Some(s) = args.const_str(pos) {
        return Ok(vec![s]);
    }
    let Some(arr) = args.arg(pos) else {
        return Err(ve(
            "an input (file path/glob, inline content, or BLOB) is required",
        ));
    };
    let elems = if let Some(l) = arr.as_list_opt::<i32>() {
        l.value(0)
    } else if let Some(l) = arr.as_list_opt::<i64>() {
        l.value(0)
    } else {
        return Err(ve("input must be a VARCHAR, a LIST(VARCHAR), or a BLOB"));
    };
    let mut out = Vec::with_capacity(elems.len());
    if let Some(s) = elems.as_string_opt::<i32>() {
        for i in 0..s.len() {
            if s.is_valid(i) {
                out.push(s.value(i).to_string());
            }
        }
    } else {
        return Err(ve("input list elements must be VARCHAR"));
    }
    if out.is_empty() {
        return Err(ve("input list is empty"));
    }
    Ok(out)
}

/// Expand a local path spec to a sorted list of files. A glob expands (possibly
/// to nothing — zero rows, not an error); a literal path must exist.
fn expand_local(spec: &str) -> Result<Vec<String>> {
    if spec.contains('*') || spec.contains('?') || spec.contains('[') {
        let mut out = Vec::new();
        let entries = glob::glob(spec).map_err(|e| ve(format!("bad glob '{spec}': {e}")))?;
        for entry in entries.flatten() {
            out.push(entry.to_string_lossy().into_owned());
        }
        out.sort();
        Ok(out)
    } else if std::path::Path::new(spec).exists() {
        Ok(vec![spec.to_string()])
    } else {
        Err(ve(format!("File not found: {spec}")))
    }
}

/// The five envelope-key fields prepended to every emitted row (carry-down rule).
pub fn envelope_key_fields() -> Vec<Field> {
    vec![
        commented(
            "interchange_ctrl",
            DataType::Utf8,
            "ISA13 — interchange control number of the row's interchange.",
        ),
        commented(
            "group_ctrl",
            DataType::Utf8,
            "GS06 — functional group control number of the row's group.",
        ),
        commented(
            "transaction_ctrl",
            DataType::Utf8,
            "ST02 — transaction set control number of the row's transaction.",
        ),
        commented(
            "transaction_type",
            DataType::Utf8,
            "ST01 — transaction set identifier ('835','837',…); UNH02 message type for EDIFACT.",
        ),
        commented(
            "source_path",
            DataType::Utf8,
            "The file the row came from; NULL for inline content.",
        ),
    ]
}

/// Build the five envelope-key cells for a row.
pub fn envelope_key_cells(
    interchange_ctrl: &str,
    group_ctrl: &str,
    transaction_ctrl: &str,
    transaction_type: &str,
    source_path: &Option<String>,
) -> Vec<Cell> {
    vec![
        Cell::s_opt(interchange_ctrl),
        Cell::s_opt(group_ctrl),
        Cell::s_opt(transaction_ctrl),
        Cell::s_opt(transaction_type),
        Cell::Str(source_path.clone()),
    ]
}
