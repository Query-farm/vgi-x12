# vgi-x12 worker — dev, test, and lint targets.
#
# Usage:
#   make test         # cargo unit/fixture tests + SQL E2E (all transports)
#   make test-unit    # cargo test --workspace (pure-Rust + Arrow-boundary tests)
#   make test-sql     # build the release worker, run the DuckDB sqllogictest
#                     #   suite over every transport (subprocess, http, unix)
#   make test-sql-subprocess / test-sql-http / test-sql-unix   # one transport
#   make lint         # clippy (deny warnings) + rustfmt --check
#   make vgi-lint     # metadata-quality lint at fail-on=info (uvx)
#   make fmt          # rustfmt the workspace
#
# The SQL E2E suite drives the compiled worker through DuckDB via
# `haybarn-unittest` (install with: `uv tool install haybarn-unittest`).

WORKER         ?= $(CURDIR)/target/release/x12-worker
SQL_RUNNER     ?= haybarn-unittest

.PHONY: test test-unit test-sql test-sql-subprocess test-sql-http test-sql-unix lint vgi-lint fmt build clean

# Full local gate: everything CI runs.
test: test-unit test-sql

test-unit:
	cargo test --workspace --all-features

test-sql: test-sql-subprocess test-sql-http test-sql-unix

test-sql-subprocess: build
	HAYBARN_UNITTEST="$(SQL_RUNNER)" WORKER_BIN="$(WORKER)" TRANSPORT=subprocess ci/run-integration.sh

test-sql-http: build
	HAYBARN_UNITTEST="$(SQL_RUNNER)" WORKER_BIN="$(WORKER)" TRANSPORT=http ci/run-integration.sh

test-sql-unix: build
	HAYBARN_UNITTEST="$(SQL_RUNNER)" WORKER_BIN="$(WORKER)" TRANSPORT=unix ci/run-integration.sh

lint:
	cargo clippy --all-targets --all-features -- -D warnings
	cargo fmt --all -- --check

# Metadata-quality gate (same rule set CI runs via Query-farm/vgi-lint-check).
vgi-lint: build
	uvx --from vgi-lint-check vgi-lint lint "$(WORKER)" --fail-on info

fmt:
	cargo fmt --all

build:
	cargo build --release --bin x12-worker

clean:
	cargo clean
