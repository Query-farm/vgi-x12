# ci/ — integration harness

Scripts that run this repo's SQLLogic suite (`test/sql/*.test`) against the
compiled `x12-worker` over each VGI transport, using a prebuilt standalone
`haybarn-unittest` and the signed community `vgi` extension — no C++ build from
source.

| file | purpose |
| --- | --- |
| `run-integration.sh` | Build/locate the worker, stage the tests, and run the suite for one `TRANSPORT` (`subprocess` \| `http` \| `unix`). Sets `VGI_X12_WORKER` (the ATTACH `LOCATION`) and exports `VGI_X12_DATA` (the `data/` fixtures dir, used by the path-mode `files.test`). |
| `preprocess-require.awk` | Rewrite each `LOAD vgi;` / `require <ext>` into an explicit signed `INSTALL … FROM community/core; LOAD …`, and inject `httpfs` after `LOAD vgi;` on the http transport (the vgi extension's HTTP client needs it; a missing httpfs otherwise gets *silently auto-skipped* by the runner). |
| `check-version.sh` | Assert the `[workspace.package]` version equals a release tag before publishing. |

## Run locally

```bash
uv tool install haybarn-unittest          # one-time: the DuckDB unittest runner
cargo build --release --bin x12-worker

HAYBARN_UNITTEST="$(command -v haybarn-unittest)" \
WORKER_BIN="$PWD/target/release/x12-worker" \
TRANSPORT=subprocess ci/run-integration.sh
```

`TRANSPORT=http` / `TRANSPORT=unix` exercise the same suite over the other two
transports. `make test-sql` runs all three.

The tests `LOAD vgi;` (NOT `require vgi`) and gate on `require-env
VGI_X12_WORKER`; almost every assertion runs over **inline** interchange content
(CWD-independent), while `files.test` covers the path/glob hot path over the
committed `data/` fixtures via `VGI_X12_DATA`.
