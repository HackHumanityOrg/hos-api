# syntax=docker/dockerfile:1.9
# Ultra-optimized multi-platform Dockerfile for NEAR indexer following 2025 best practices
# Supports x64 and ARM on both Linux and macOS via Docker Desktop

FROM rust:1.88-bookworm AS base

# Install cargo-chef and sccache for build optimization
RUN cargo install --locked cargo-chef sccache

# Performance optimizations
ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_DIR=/sccache
ENV CARGO_TERM_COLOR=always

# Static OpenSSL for cross-platform compatibility
ENV OPENSSL_STATIC=1
ENV OPENSSL_VENDORED=1

# Faster linking
ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld"

# Build deps
RUN apt-get update && apt-get install -y \
    lld \
    clang \
    pkg-config \
    libssl-dev \
    ca-certificates \
    build-essential \
  && rm -rf /var/lib/apt/lists/* \
  && apt-get clean

# Planner stage
FROM base AS planner
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY migration/ migration/
COPY api/ api/
COPY indexers/ indexers/
RUN cargo chef prepare --recipe-path recipe.json

# Builder stage
FROM base AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY migration/ migration/
COPY api/ api/
COPY indexers/ indexers/

# Build near-indexer binary
RUN cargo build --release -p near-indexer && \
    cp target/release/near-indexer /tmp/near-indexer

# Runtime (distroless)
FROM gcr.io/distroless/cc-debian12 AS runtime

# Copy binary and configs
COPY --from=builder /tmp/near-indexer /near-indexer
COPY config.toml /config.toml
COPY indexers/near-indexer/config.toml /near-indexer-config.toml

LABEL maintainer="data-commons-team" \
  version="1.0" \
  description="Data Commons NEAR Indexer - 2025 Cross-Platform Optimized" \
  rust.version="1.88" \
  service="near-indexer" \
  platforms="linux/amd64,linux/arm64"

ENTRYPOINT ["/near-indexer"]

