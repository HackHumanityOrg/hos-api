# Repository Guidelines

## Project Structure & Module Organization
- Root workspace managed by Cargo with multiple crates:
  - `api/`: Axum REST server with Swagger UI and OpenAPI.
  - `indexers/`: `indexer-1`, `indexer-2`, `indexer-3` services.
  - `shared/`: common libraries (`common`, `db-core`, `entities`).
  - `migration/`: SeaORM migrations. Config files live in `config.toml`, plus `api/config.toml` and `indexers/<name>/config.toml`.

## Build, Test, and Development Commands
- `cargo check-all`: Compile all crates.
- `cargo test-all`: Run all tests across the workspace.
- `cargo run-api`: Start the API at `http://127.0.0.1:3000`.
- `cargo run-indexer-1` (etc.): Run an indexer locally.
- `cargo migrate-up`: Apply DB migrations; `cargo migrate-generate <name>` to create one.
- `cargo generate-entities`: Regenerate SeaORM entities after schema changes.

## Coding Style & Naming Conventions
- Rust 2021 edition; use standard Rust idioms: 4‑space indent, `snake_case` for functions/modules, `CamelCase` for types.
- Run `cargo fmt` before committing; optional linting via `cargo clippy -- -D warnings`.
- Libraries use `thiserror` for typed errors; binaries use `anyhow` for context-rich errors.

## Testing Guidelines
- Frameworks: `rstest`, `axum-test`, `testcontainers` for real MySQL.
- Mirror test structure to modules (see `api/tests/{handlers,routes}/...`).
- Quick examples: `cargo test -p api`, `cargo test -p api handlers::health`.
- Keep tests deterministic; prefer handler tests for logic, route tests for HTTP behavior.

## Commit & Pull Request Guidelines
- Commit style (based on history): short, imperative subjects (e.g., "fix Railway caching", "observability"). Keep subjects ~50 chars; scope optional and lowercase is fine.
- PRs must include: concise description, linked issues, testing notes, and any config/migration impacts. Add screenshots for API/Swagger changes when relevant.

## Security & Configuration Tips
- Never commit secrets. Use `.env` (copy from `.env.example`) and environment variables: `DATABASE_URL`, `RUST_LOG`, `DASH0_OTLP_ENDPOINT`, `DASH0_AUTH_TOKEN`.
- Config is hierarchical: root `config.toml` → service config → `APP_*` env overrides. API binds to `APP_SERVER_HOST`/`PORT` if set.

## Quick Start
```bash
cargo install cargo-run-bin && cargo bin --install
cp .env.example .env && cargo migrate-up && cargo generate-entities
cargo run-api  # Swagger UI at /swagger-ui
```

