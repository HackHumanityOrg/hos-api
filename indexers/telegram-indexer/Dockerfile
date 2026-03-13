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
RUN cargo chef prepare --recipe-path recipe.json --bin telegram-indexer

FROM base AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin telegram-indexer

COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY migration/ migration/
COPY api/ api/
COPY indexers/ indexers/
RUN cargo build --release --bin telegram-indexer && \
    cp target/release/telegram-indexer /tmp/telegram-indexer

FROM gcr.io/distroless/cc-debian12 AS runtime
COPY --from=builder /tmp/telegram-indexer /telegram-indexer
COPY indexers/telegram-indexer/config.toml /telegram-indexer-config.toml

LABEL version="1.0" \
  rust.version="1.88" \
  service="telegram-indexer" \
  platforms="linux/amd64,linux/arm64"

ENTRYPOINT ["/telegram-indexer"]
