# syntax=docker/dockerfile:1.9

FROM lukemathwalker/cargo-chef:0.1.72-rust-1.88-bookworm@sha256:d184d41fd934afd282bf8f04b602aaa952af88341c48b8738b0c0a85ac7a6cec AS base

ENV CARGO_TERM_COLOR=always
ENV OPENSSL_STATIC=1
ENV OPENSSL_VENDORED=1
ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld"

RUN apt-get update && apt-get install -y \
    lld \
    clang \
    pkg-config \
    libssl-dev \
    ca-certificates \
    build-essential \
  && rm -rf /var/lib/apt/lists/* \
  && apt-get clean

FROM base AS planner
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY migration/ migration/
COPY api/ api/
COPY indexers/ indexers/
RUN cargo chef prepare --recipe-path recipe.json --bin api

FROM base AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin api

COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY migration/ migration/
COPY api/ api/
COPY indexers/ indexers/
RUN cargo build --release --bin api && \
    cp target/release/api /tmp/api

FROM gcr.io/distroless/cc-debian12 AS runtime
COPY --from=builder /tmp/api /api-bin
COPY api/config.toml /api/config.toml

EXPOSE 3000

LABEL maintainer="data-commons-team" \
    version="1.0" \
    description="Data Commons API Server - 2025 Cross-Platform Optimized" \
    rust.version="1.88" \
    build.optimization="size+performance" \
    platforms="linux/amd64,linux/arm64" \
    openssl="vendored-static"

ENTRYPOINT ["/api-bin"]
