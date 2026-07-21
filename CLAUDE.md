# CLAUDE.md — vgi-x12

Guidance for working in this repo.

## What this is

A VGI worker (Rust, the published `vgi` SDK 0.9.5 / `vgi-rpc` 0.7 / arrow 59)
that parses ANSI ASC X12 EDI and UN/EDIFACT interchanges into queryable rows for
DuckDB over Apache Arrow. DuckDB `ATTACH 'x12' (TYPE vgi, COMMAND 'x12-worker')`.

## Workspace layout

```
crates/
  x12-core/        pure-compute parser — NO arrow/vgi deps, serde only
    src/
      delimiters.rs   ISA fixed-width sniff + EDIFACT UNA; Family detection
      segment.rs      Segment row model + delimiter-driven split (release-char aware)
      envelope.rs     ISA/GS/ST nesting walk, control numbers, SE/GE/IEA validation
      edifact.rs      UNB/UNG/UNH variant + release-char un-escape
      shaped/         positional extractors: t835, t837, t270_271, t850, acks (997/999)
    tests/fixtures.rs golden-fixture integration tests over data/
  x12-worker/      thin Arrow adapter over x12-core
    src/
      main.rs         Worker::new(); register scalars + tables; set_catalog; run
      meta.rs         object_tags / keywords_json / result_columns_schema / example helpers
      arrow_io.rs     Cell enum + generic schema-driven build_batch; scalar cell reads
      source.rs       path|text|bytes input resolution + the 5 envelope-key columns
      scalar/         delimiters, transaction_type
      table/          segments, segments_elements, envelope, edifact, shaped (registry-driven)
```

All parsing correctness lives in `x12-core` and is unit-tested there directly
(no Arrow/RPC needed). The worker crate only marshals rows to Arrow via the
generic `Cell` / `build_batch` path, so adding a function is: write the
extractor in core, declare the output schema + metadata in a `table/` module.

## Conventions

- **Input modes.** Every table function takes ONE overloaded positional `input`
  arg (`source::input_arg_specs()` / `source::resolve()`). DuckDB requires the
  positional to be present, so path | text | bytes are overloaded onto it and
  auto-detected by the `ISA`/`UNA`/`UNB` magic prefix (`mode => 'path'|'content'`
  forces it). Do NOT add separate `text =>` / `bytes =>` named args — a required
  positional makes named-only calls fail to bind.
- **Envelope-key carry-down.** Every emitted row starts with the 5 keys from
  `source::envelope_key_fields()` / `envelope_key_cells()`.
- **Shaped views** are registered from `x12_core::shaped::REGISTRY`; the worker's
  `table::shaped::Shaped` adapter drives every entry. Add a set by adding a
  `ShapedDef` there.
- **Robustness.** Parsing is total — never panic on arbitrary bytes; a missing
  trailer leaves an `Option` `None` and surfaces as a NULL validity flag. A
  non-existent file *path* IS a legitimate error; malformed *content* is not.
- **Metadata.** Every function needs title/doc_llm/doc_md/keywords
  (`meta::object_tags`), a described `vgi.example_queries` JSON tag
  (`meta::example_queries_tag`) — VGI515, and table functions need a
  `vgi.result_columns_schema` JSON tag (`meta::result_columns_schema(&schema)`) —
  VGI307. Argument descriptions must NOT restate the data type — VGI313.

## HARD RULE — licensing / IP

Ship **public X12 syntax only**. Embed NO copyrighted ASC X12 TR3 text — no loop
names, no code-value descriptions. Shaped columns are named by public segment ID
+ element position (`clp_total_paid` = `CLP04`); raw codes pass through verbatim.
The parser is in-house (no `x12-stream-parser`/`x12-types` dependency was needed,
and the GPL-3.0 `edi` crate is excluded). License is **MIT**.

## Gates (all must pass)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
make vgi-lint                      # uvx vgi-lint-check, fail-on=info  → 100/100
make test-sql                      # haybarn SQLLogic E2E, all 3 transports
```

The SQLLogic suite (`test/sql/*.test`) uses `LOAD vgi;` (not `require vgi`),
gates on `require-env VGI_X12_WORKER`, and runs over **inline** content
(CWD-independent); `files.test` covers the path/glob hot path over `data/` via
`VGI_X12_DATA` (exported by `ci/run-integration.sh`).

## Fixtures

`data/` holds hand-authored, non-PHI synthetic interchanges (835/837/270/271/850/
997 + EDIFACT ORDERS + truncated / multi-group / non-canonical-delimiter cases).
The SE/GE/IEA counts in each are exact, so tests assert the validity flags.
