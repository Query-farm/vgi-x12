# VGI X12 notebooks

[01 · Extracted lines still need an input audit](01_extracted_lines_need_an_input_audit.ipynb) uses visible SQL to separate structural envelope checks from numeric quality, retain inputs that emit no shaped rows, and calculate a selected-line subtotal over synthetic purchase-order alternatives.

## Run

Install [uv](https://docs.astral.sh/uv/). From the repository root:

```sh
uv sync --project notebooks --frozen
uv run --project notebooks --frozen python notebooks/execute.py
```

The complete `notebooks/` directory also runs independently of this checkout:

```sh
cd notebooks
uv sync --frozen
uv run --frozen python execute.py
```

Initial setup downloads Python dependencies, the VGI community extension and the native release. Parsing then reads only the six committed synthetic files. No Rust toolchain, local worker build, credentials or external data service is needed. The native release was executed on macOS Apple silicon; other supported archives are listed on the release page.

In VS Code select `notebooks/.venv/bin/python` and use `notebooks/` as the working directory. Other Jupyter editors can register the environment:

```sh
uv run --project notebooks --frozen python -m ipykernel install --user --name vgi-x12-notebooks --display-name 'Python (vgi-x12 notebooks)'
```

Restart and run all after changing the visible parameter. Connection and analytical SQL stay expanded. Imports, plotting, interpretation generation, assertions and cleanup source are folded, while charts remain visible.

## Experiment

```sh
uv run --project notebooks --frozen python notebooks/execute.py \
  --parameters '{"ALLOW_INCOMPLETE":true}' \
  --output-dir notebooks/validation/incomplete-preview
```

The default requires all six envelope checks to pass. The experiment permits unknown checks when none failed; known failures remain excluded. This is a reporting preview, not interchange acceptance. Numeric eligibility remains a separate requirement. Reset to false for the saved default.

The runner requires a separate destination for overrides, rejects unknown parameter names and only saves after complete successful execution. Failures preserve the previous saved notebook. Experiment copies include the assets, fixtures, helper/builder, README and locked environment. Logs, review charts and experiment copies are ignored.

## Worker and fixture provenance

The visible literal `ATTACH` uses `github-auto://Query-farm/vgi-x12@v0.4.0`. VGI downloads, verifies and caches the platform release. Haybarn 2.0.0rc11 and notebook packages are pinned separately from the worker's Cargo files. Upgrades require an explicit tag change and both parameter runs.

`fixtures/complete.edi` is an unchanged copy of the hand-authored [po_850.850 fixture at 38a2fe8](https://github.com/Query-farm/vgi-x12/blob/38a2fe8a5604566e23971413ed6c7800c6bd389c/data/po_850.850). The fixture contains synthetic party/contact/order data, no real business records or PHI. `build_fixtures.py` derives five alternatives: a wrong SE count, a nonnumeric quantity, missing GE/IEA trailers, missing SE and outer trailers, and an interchange containing no ST transaction. SQL adds a missing-input inventory record. These are alternative test cases of one order, not independent purchases.

Regenerate all six files from the bundled complete input:

```sh
uv run --project notebooks --frozen python notebooks/build_fixtures.py /tmp/vgi-x12-fixtures
```

The analysis uses public segment IDs and positional fields. It embeds no copyrighted ASC X12 implementation-guide text, loop names or code-value descriptions. Raw codes remain uninterpreted. Amounts use a declared fictional common price scale; no currency, tax, charge, unit-code interpretation or full purchase-order total is inferred.

## Reading the results

Seven inventoried inputs produce five envelope rows and eight shaped lines. The wrong-count file yields two lines but fails a structural check. The bad-quantity file passes envelope checks but has one unparseable quantity; that value and its line amount remain NULL.

The pinned release emits line items when SE is present but GE/IEA are missing. When SE is missing too, it emits an envelope row but no shaped lines. The no-transaction file emits neither; the missing record has no file. Starting from the input inventory preserves all of these distinct states.

Joins include source path and the full carried envelope key. The deliberately repeated control identifiers across files therefore do not multiply lines. The matrix distinguishes true, false and unavailable flags. Passing these flags establishes only structural count/control agreement, not full EDI business compliance.

The selected subtotal is computed only from nonnegative, parseable decimal quantities and unit prices in eligible envelopes. A zero selected subtotal means no selected contribution, not that an order is worth zero. Separate columns retain extracted, invalid and withheld line counts; an input with no known numeric line amounts has a NULL full-known subtotal.

The default selects three lines with an audit sum of 139.95 fixture price units. Previewing incomplete outer envelopes selects five lines and 234.90 units. The wrong count, invalid quantity, no-transaction record, missing input and missing-SE extraction behavior remain unchanged. These sums reconcile the synthetic alternatives and must not be interpreted as procurement spend or estimated value of omitted data.

## Validation

The default completed in 8.1 seconds and the detached incomplete-envelope preview in 12.8 seconds. Both passed 12 SQL checks, input/line reconciliation and exact decimal totals. Four chart renderings and three runner preservation guards were reviewed. Raw envelope and line displays and the evidence chart are identical across settings. All six EDI fixtures regenerate byte-for-byte; the complete fixture matches the original source.

Source validation passed 52 Rust tests, 217 stdio / 235 HTTP / 217 Unix SQL assertions (nine cases per transport), the locked release build, Clippy, rustfmt and warning-clean rustdoc. The repository metadata gate passed at 100/100 L2 with zero findings. Notebook Ruff and structural review passed. Worker source, tests, data, Cargo manifests and root lockfile remain unchanged.

Official Query.Farm Strata Sun and VGI artwork is copied unchanged from the approved assets. Linked Query.Farm branding appears at the top and final bottom.
