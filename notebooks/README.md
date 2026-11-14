# X12 and EDIFACT examples

- [01 · Extracted lines still need an input audit](01_extracted_lines_need_an_input_audit.ipynb) preserves the synthetic purchase-order audit, adds type routing and complete-segment accounting, and traces shaped quantities/prices back to raw positions. [Open in Colab](https://colab.research.google.com/github/Query-farm/vgi-x12/blob/main/notebooks/01_extracted_lines_need_an_input_audit.ipynb).
- [02 · A field boundary can change while segment counts pass](02_escape_characters_change_fields_not_segment_counts.ipynb) compares two authored EDIFACT messages differing by one release character. [Open in Colab](https://colab.research.google.com/github/Query-farm/vgi-x12/blob/main/notebooks/02_escape_characters_change_fields_not_segment_counts.ipynb).

Choose **Runtime → Run all** in Colab. Its base-package bootstrap retains compatible
preloaded kernel/data/plotting libraries and downloads immutable, SHA256-verified
companions. Locally, install [uv](https://docs.astral.sh/uv/) and run inside this
`notebooks/` directory:

```sh
uv sync --locked
uv run --locked vgi-notebooks
uv run --locked vgi-notebooks 01_extracted_lines_need_an_input_audit.ipynb --parameters '{"ALLOW_INCOMPLETE":true}' --output-dir artifacts/incomplete-preview
uv run --locked vgi-notebooks 02_escape_characters_change_fields_not_segment_counts.ipynb --parameters '{"ESCAPE_PLUS":false}' --output-dir artifacts/unescaped
```

For Jupyter select `.venv/bin/python`. Local publication uses the shared `[runner]`
extra and lock. The runner saves only successful defaults, rejects unknown parameter
names and requires a separate destination for overrides. If an older bootstrap
replaced kernel libraries, save your work and open the current notebook in a fresh runtime.

Visible SQL starts the verified native release:

```sql
INSTALL vgi FROM community;
LOAD vgi;
ATTACH 'x12' (TYPE vgi,LOCATION 'github-auto://Query-farm/vgi-x12@v0.5.0');
```

VGI downloads the platform archive, checks its SHA256 and caches it. No worker
checkout, Rust build, credentials, real data service or live filesystem inspection
is required. The notebook directory runs independently. Initial extension/package/
companion/release retrieval needs network access. The local lock uses **Haybarn
2.0.0rc11**, tested on macOS arm64; other published platforms were not executed.
Upgrade the literal worker release only after rechecking catalogs, defaults and experiments.

The [shared v0.3.20 standard](https://github.com/Query-farm/vgi-example-notebooks/blob/v0.3.20/docs/standard.md)
supplies the proper catalog title/subtitle, branding, isolated SQL secrets, SVG
presentation and execution protections. Keep `charts/` beside saved notebooks for
GitHub's content-addressed image links; interactive runs retain native SVG. Review
PNGs, logs and experiment copies remain ignored. Hosted Colab was not manually
operated; the bootstrap test uses a fresh compatible kernel with eight libraries preloaded.

Coverage is **9 of 17 named public SQL surfaces**, meeting the at-least-half target.
Covered: `envelope`, `read_850`, `read_x12`, `segments_elements`, `transaction_type`,
`transaction_sets`, `delimiters`, `edifact_envelope` and `edifact_segments`.
Uncovered: `copy_x12`, `segments`, `read_270`, `read_271`, `read_835`, `read_837`,
`read_997` and `read_999`. The two catalog forms of `copy_x12` count once; this
registered read/write handler stays in the denominator even though it is invoked
through COPY rather than SELECT. These examples perform no COPY or file write.

`coverage.json` binds complete runtime catalogs, all SELECT output schemas, COPY
format metadata, execution receipts and reviewed SQL/narrative/result mappings.
Notebooks execute v0.5.0, including the new complete `read_x12` representation.
Current public source has matching interfaces and unchanged parser/dependencies;
compatible mappings do not claim current-main notebook execution. The initial
static nine-candidate audit missed shaped-reader registrations and the COPY handler.

## Synthetic inputs and scope

The six original X12 alternatives are unchanged. `fixtures/complete.edi` matches
`data/po_850.850` from source revision `38a2fe8a5604566e23971413ed6c7800c6bd389c`.
`build_fixtures.py` derives wrong-count, nonnumeric-quantity, missing-outer-trailer,
byte-truncated and no-transaction alternatives. SQL adds an explicit missing-input
inventory record. These are test alternatives of one order, not independent purchases.

Optional regeneration covers those original six files only:

```sh
uv run --locked python build_fixtures.py /tmp/vgi-x12-fixtures
```

The two new `fixtures/edifact/` files are authored examples of public UNA/UNB/UNH/
FTX/UNT/UNZ syntax. Their only difference is removal of the `?` preceding a plus
inside FTX text. Both have three message segments. The provenance manifest records
all eight sizes/hashes, original source revision and the exact transformation;
setup verifies every byte image. Party IDs, controls, clocks and text are invented.
No real business records or PHI are included, and internal dates are not collection dates.
The MIT worker license is bundled.

The analysis uses public segment IDs and raw positional fields. It embeds no
copyrighted ASC X12 implementation-guide text, loop names or code-value descriptions.
Raw codes remain uninterpreted. Price calculations use a declared fictional common
scale; they infer no currency, tax, charge, unit-code meaning or complete order total.

## Reading the results

Notebook 01 inventories seven inputs before decoding. Six supplied files contain
55 complete-projection segment rows, five transaction envelopes and eight shaped
PO1 lines. The no-transaction file still contains four envelope segments. The
`no-se` fixture is byte-truncated **inside N1, before any PO1 line**, as well as
missing SE and outer trailers. Its zero shaped rows do not prove a general SE-required
rule. The complete projection retains five source segments for that input.

Transaction-type routing maps five inputs to `read_850`; this does not validate
structure or numeric fields. Source path remains part of every identity because
test alternatives reuse envelope control numbers. Complete document segment indices
and transaction-local element indices have different scopes and are not joined.
All eight shaped line quantities/prices agree with the raw positional projection,
including the unparseable quantity. Repeated/composite fields or nonunique line
numbers would require fuller keys than this fixture-specific reconciliation.

Default `ALLOW_INCOMPLETE=False` selects three lines totaling **139.95 fictional
price units**. Setting it to True selects five totaling **234.90** by previewing
numeric lines from the file missing only outer trailers. Known failed checks remain
excluded and the invalid numeric value stays NULL. No lines are created for the
byte-truncated input. This is an audit sensitivity experiment, not acceptance policy
or procurement spend. Zero selected amount means no selected amount, not a zero-value
order or an estimate for missing lines.

Notebook 02 preserves both input hashes and envelopes, then selects one alternative.
Both pass the UNT count check with **three message segments**. With `ESCAPE_PLUS=True`,
FTX position 4 contains `Subtotal 5+ fee`; four top-level FTX positions include two
empty strings, and the message has 11 decoded element/component rows. False splits
that text into positions 4 and 5, giving five FTX positions and 12 decoded value rows.
The separator tuple and control-segment values remain unchanged. Empty strings are
explicit positions, not NULLs. UNH contains a composite element, so decoded value-row
counts differ from top-level positions and segment counts.

A passing envelope count cannot establish intended field boundaries, full EDI
business-rule compliance or agreement of every outer control. The delimiter scalar
does not expose a release-character field; the authored UNA prefix supplies that
context. The shaped-reader reference has no ORDERS row, which is a coverage distinction
rather than a rejection of the dedicated EDIFACT parser. No write or repair is performed.

Existing green [source CI at 73f1215](https://github.com/Query-farm/vgi-x12/actions/runs/43580885430)
is reused. Worker source and root dependencies are unchanged, so no rebuild,
full-suite repetition or source/container CI is triggered. Prior gates passed
52 Rust tests, 217 stdio / 235 HTTP / 217 Unix SQL assertions, build/Clippy/rustfmt/
rustdoc and metadata 100/100 L2 with zero findings. Notebook validation includes both
defaults and detached experiments, three default SVGs and matching experiment
figures, runner protections, coverage receipts and one fresh preloaded bootstrap.

The initial v0.5.0 cold download encountered GitHub's shared anonymous API rate limit.
A test subprocess used existing authorized GitHub authentication solely to fetch and
verify that public release; no credential was written, printed or added to the
notebooks. Subsequent anonymous ATTACH and SQL succeeded from that verified binary
cache. Cold companion/bootstrap tests are separate from a cold anonymous worker download.
