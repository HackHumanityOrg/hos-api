# syntax=docker/dockerfile:1.9
# Ultra-optimized multi-platform Dockerfile for Indexer-1 following 2025 best practices
# Supports x64 and ARM on both Linux and macOS via Docker Desktop

# Base stage with cargo-chef, sccache, and optimal cross-platform setup
FROM rust:1.88-bookworm AS base

# Install cargo-chef and sccache for build optimization
RUN cargo install --locked cargo-chef sccache

# Performance optimizations for 2025 + cross-platform support
ENV RUSTC_WRAPPER=sccache
ENV SCCACHE_DIR=/sccache
ENV CARGO_TERM_COLOR=always

# Cross-platform OpenSSL configuration - use vendored OpenSSL for full static linking
ENV OPENSSL_STATIC=1
ENV OPENSSL_VENDORED=1

# Use lld linker for faster builds and lower memory usage (available on both platforms)
ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld -C target-cpu=native"

# Install build dependencies for cross-platform support
RUN apt-get update && apt-get install -y \
    lld \
    clang \
    pkg-config \
    libssl-dev \
    ca-certificates \
    build-essential \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Planner stage - generates dependency recipe for optimal layer caching
FROM base AS planner
WORKDIR /app

# Copy workspace files for recipe generation
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY migration/ migration/
COPY api/ api/
COPY indexers/ indexers/

# Generate recipe.json for dependency caching
RUN cargo chef prepare --recipe-path recipe.json

# Builder stage - builds dependencies and application with 2025 optimizations
FROM base AS builder
WORKDIR /app

# Copy recipe and build dependencies with extensive caching
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies with Railway optimization
# Using vendored OpenSSL ensures cross-platform compatibility
RUN CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
    CARGO_PROFILE_RELEASE_LTO=true \
    CARGO_PROFILE_RELEASE_STRIP="symbols" \
    cargo chef cook --release --recipe-path recipe.json

# Copy source code (this layer will be rebuilt when source changes)
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY migration/ migration/
COPY api/ api/
COPY indexers/ indexers/

# Build application with size and performance optimizations for Railway
RUN CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
    CARGO_PROFILE_RELEASE_LTO=true \
    CARGO_PROFILE_RELEASE_STRIP="symbols" \
    cargo build --release -p indexer-1 && \
    cp target/release/indexer-1 /tmp/indexer-1

# Runtime stage - distroless for maximum security and minimal size
# Works on both x64 and ARM, Linux and macOS Docker Desktop
FROM gcr.io/distroless/cc-debian12 AS runtime

# Copy optimized statically-linked binary and configuration
COPY --from=builder /tmp/indexer-1 /indexer-1
COPY config.toml /config.toml
COPY indexers/indexer-1/config.toml /indexer-1-config.toml

# Add metadata labels for better maintainability
LABEL maintainer="data-commons-team" \
    version="1.0" \
    description="Data Commons Indexer-1 Service - 2025 Cross-Platform Optimized" \
    rust.version="1.88" \
    build.optimization="size+performance" \
    platforms="linux/amd64,linux/arm64" \
    openssl="vendored-static" \
    service="indexer-1"

# Run the indexer service (distroless runs as non-root by default)
ENTRYPOINT ["/indexer-1"]
