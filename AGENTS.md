# Repository Guidelines

## Project Structure & Module Organization
The workspace is a multi-crate Cargo project. Service code lives under `api/` (Axum REST server with Swagger UI) and `indexers/{indexer-1,indexer-2,indexer-3}/` for ingestion services. Shared libraries (`shared/common`, `shared/db-core`, `shared/entities`) hold reusable logic and schema definitions. Database migrations reside in `migration/`, while service-specific settings inherit from the root `config.toml` alongside `api/config.toml` and `indexers/<name>/config.toml`.

## Build, Test, and Development Commands
Use `cargo check-all` to compile every crate before opening a PR. Run `cargo test-all` for the full workspace test suite; scope to a crate with commands such as `cargo test -p api` or `cargo test -p api handlers::health`. Development binaries include `cargo run-api` (serves at http://127.0.0.1:3000) and `cargo run-indexer-1` (similar for other indexers). Apply schema updates with `cargo migrate-up`, generate new migrations via `cargo migrate-generate <name>`, and rebuild SeaORM entities with `cargo generate-entities`.

## Coding Style & Naming Conventions
Target Rust 2021 idioms with 4-space indentation, `snake_case` for modules/functions, and `CamelCase` types. Libraries favor `thiserror` for typed errors; binaries use `anyhow` for rich context. Always format with `cargo fmt` and consider `cargo clippy -- -D warnings` before pushing.

## Testing Guidelines
Tests mirror module layout (e.g., `api/tests/handlers/`). Prefer deterministic handler tests for business logic and route tests for HTTP behavior. Frameworks in use include `rstest`, `axum-test`, and `testcontainers` for MySQL-backed scenarios. Run targeted suites during development, then confirm with `cargo test-all` prior to merge.

## Commit & Pull Request Guidelines
Write concise, imperative commit subjects (≈50 characters) similar to "fix Railway caching" or "observability". Pull requests should summarize the change, link relevant issues, note testing performed, and call out config or migration impacts; include Swagger screenshots when UI behavior changes.

## Security & Configuration Tips
Never commit secrets. Copy `.env.example` to `.env` locally and configure runtime with environment variables such as `DATABASE_URL`, `RUST_LOG`, and `SENTRY_DSN`. Override Figment settings by exporting keys like `APP_SERVER__HOST` or `APP_LOGGING__LEVEL` for service-specific tuning.
