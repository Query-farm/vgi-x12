# Copyright 2026 Query Farm LLC - https://query.farm
#
# Single image that serves every network transport of the `x12` VGI worker:
#   docker run ... IMG            -> HTTP server on $PORT      (default; Fly.io / local)
#   docker run ... IMG tcp        -> raw Arrow-IPC over TCP on $PORT_TCP
#   docker run -i ... IMG stdio   -> stdio worker DuckDB spawns on-host
# See docker-entrypoint.sh.
#
# Like vgi-units this worker is STATELESS: the X12/EDIFACT parser is pure
# in-binary compute (no model registry, no per-attach state), and every table
# function reads a caller-supplied path/content/blob — the host already exposes
# any files. So there is no /data volume and no `farm.query.vgi.volumes`
# mount-discovery label. The image is just the binary + a tiny entrypoint.
# syntax=docker/dockerfile:1

# ---- build stage -----------------------------------------------------------
# Pinned glibc (bookworm) so the binary links against the same libc the slim
# runtime ships. The workspace resolves `vgi` from crates.io, so no git is
# needed at build time beyond what the rust image ships.
FROM rust:1-bookworm AS build
WORKDIR /src

# Copy the whole workspace (manifests + sources + lockfile). The cargo registry
# and target dir are BuildKit cache mounts, so the binary is copied OUT to a
# non-cache path before the layer ends (cache mounts don't persist in the image).
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin x12-worker \
    && cp target/release/x12-worker /x12-worker

# ---- runtime stage ---------------------------------------------------------
# debian-slim (not distroless) so the HEALTHCHECK below has a real `curl`. The
# HTTP transport is plain inbound HTTP (no TLS in the dependency tree), so no
# libssl / ca-certificates are needed.
FROM debian:bookworm-slim

# Build metadata, wired from docker/metadata-action outputs in CI.
ARG VERSION=0.0.0
ARG GIT_COMMIT=unknown
ARG SOURCE_URL=https://github.com/Query-farm/vgi-x12

# Standard OCI labels + the VGI transport-advertisement label. `transports`
# lists the NETWORK transports this image serves (http + raw tcp); stdio is a
# spawn mode, not a network transport, so it is not listed.
LABEL org.opencontainers.image.title="vgi-x12" \
      org.opencontainers.image.description="Parse ANSI ASC X12 EDI and UN/EDIFACT interchanges into queryable rows as a VGI worker for DuckDB/SQL (stdio + HTTP + TCP)" \
      org.opencontainers.image.source="${SOURCE_URL}" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${GIT_COMMIT}" \
      org.opencontainers.image.licenses="MIT" \
      farm.query.vgi.transports='["http","tcp"]'

ENV PORT=8000 \
    PORT_TCP=8001 \
    # Build provenance only; the version the worker advertises over VGI comes
    # from the compiled CARGO_PKG_VERSION, not this.
    VGI_X12_GIT_COMMIT=${GIT_COMMIT}

WORKDIR /app

# curl backs the HEALTHCHECK below; nothing else is needed at runtime.
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*

# `--chmod` sets the mode in the COPY layer itself. A separate `RUN chmod` would
# rewrite the whole binary into a second layer (overlayfs copies the file on a
# metadata change), needlessly doubling its on-disk footprint in the image.
COPY --from=build --chmod=0755 /x12-worker /usr/local/bin/x12-worker
COPY --chmod=0755 docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

# Run unprivileged. No state, no volume — there is nothing to own or persist.
RUN useradd --create-home --uid 10001 app
USER app

EXPOSE 8000 8001

# Readiness probe for HTTP mode. Inert for a short-lived stdio container, which
# has no HTTP server (the probe just fails harmlessly there).
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS "http://localhost:${PORT:-8000}/health" || exit 1

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["http"]
