# Changelog

All notable changes to the `vgi-x12` worker are documented here. The format is
loosely based on [Keep a Changelog](https://keepachangelog.com/); versions track
the `[workspace.package]` version in `Cargo.toml`.

## 0.1.0 — unreleased

Initial release. A VGI worker that parses ANSI ASC X12 EDI and UN/EDIFACT
interchanges into queryable rows for DuckDB over Apache Arrow. Public X12 syntax
only; parsing is 100% local (no network surface).

### Generic functions

- `segments(input)` — one row per segment, positional element values as a
  `LIST<VARCHAR>`, envelope keys carried down.
- `segments_elements(input)` — one row per element, split into composite
  components and repetitions (the workhorse view).
- `envelope(input)` — one row per ST transaction with ISA/GS/ST metadata and the
  `SE`/`GE`/`IEA` structural count + control-number validity flags.

### Shaped views (positional, public-segment-ID only)

- `read_835` (remittance / ERA), `read_837` (claim), `read_270` / `read_271`
  (eligibility), `read_850` (purchase order), `read_997` / `read_999`
  (functional acknowledgements).

### UN/EDIFACT

- `edifact_segments(input)` / `edifact_envelope(input)` — UNA/UNB/UNG/UNH variant
  with release-character un-escaping.

### Scalars

- `delimiters(content)` → `STRUCT(element, segment, component, repetition)`,
  `transaction_type(content)`.

### Reference

- `transaction_sets` — a zero-argument, browsable reference view mapping each
  shaped X12 transaction set to its `read_*` function. The worker's build version
  is published as the catalog `implementation_version` (there is no `*_version()`
  scalar).

### Notes

- Every table function's `input` argument is overloaded across **path | text |
  bytes** modes, auto-detected by the `ISA`/`UNA`/`UNB` magic prefix (override
  with `mode => 'path' | 'content'`).
- Delimiters are sniffed per interchange from the fixed-width ISA (or the EDIFACT
  UNA). Non-canonical delimiter sets and the 4010 `U` repetition placeholder are
  handled.
- Malformed / truncated EDI never aborts the query: parsing is total and
  degrades to fewer rows / NULL validity flags.
- Ships **public X12 syntax only** — no copyrighted ASC X12 TR3 implementation-
  guide content. Shaped columns are named by public segment ID + element position
  (`clp_total_paid` = `CLP04`); raw codes are surfaced verbatim. License: MIT.
