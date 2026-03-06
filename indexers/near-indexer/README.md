# NEAR Indexer

This service ingests NEAR blocks through NEAR Lake or FastNEAR and writes
chain data into the `near` PostgreSQL schema.

It has two modules:

- `raw`: full-chain canonical raw storage
- `venear_governance`: House of Stake (HoS) graph tracking, HoS-scoped raw
  projection, and HoS materialized-view refresh

The important design point is that the House of Stake materialized views are
always derived from the canonical `near_*` raw tables. The HoS module exists to
decide which transaction graphs are relevant to HoS, preserve enough raw data
to rebuild those views when `raw` is disabled, and refresh the views on a
controlled cadence.

## Runtime Model

The service starts in [src/main.rs](src/main.rs).

At startup it:

1. loads `indexers/near-indexer/config.toml` plus environment overrides through
   `common::config`
2. validates runtime constraints
3. opens the database
4. registers both modules
5. initializes cursor state from `near.near_sync_cursors`
6. plans stream assignments
7. starts one worker per stream assignment

Important runtime rules:

- `venear_governance` requires `near.concurrency = 1`
- if `raw` and `venear_governance` are both enabled, both must use
  `stream_mode = "shared"`
- a shared stream starts from the minimum effective start height across its
  assigned modules
- an independent stream starts from its own module-specific height/cursor

Relevant files:

- [src/main.rs](src/main.rs)
- [src/lib.rs](src/lib.rs)
- [src/modules/mod.rs](src/modules/mod.rs)
- [src/core/lake_stream.rs](src/core/lake_stream.rs)
- [src/core/cursor.rs](src/core/cursor.rs)
- [src/core/processor.rs](src/core/processor.rs)

## Providers

The service can read from:

- NEAR Lake via S3-backed `near-lake-framework`
- FastNEAR via HTTP-backed `near-lake-framework`

FastNEAR start heights are clamped to the first block FastNEAR can serve:

- mainnet: `9_820_210`
- testnet: `42_376_888`

`preload_pool_size` controls upstream buffering. `fastnear_num_threads` controls
FastNEAR fetch parallelism only.

## Cursor Semantics

Each module has its own persisted cursor row in `near.near_sync_cursors` under
the key `near-indexer:<module_name>`.

The indexer also keeps an in-memory cursor cache and a pending-height map so
cursor advancement remains chain-linked even if blocks complete out of order.
The database cursor is rate-limited to roughly one write per second per module.

Operationally this means:

- module processing is independent
- a block only advances a module cursor when its `prev_height` links to the
  module's current committed height
- rewinding a module means resetting its cursor row plus any module-owned state

## Canonical Raw Schema

The canonical raw tables used by both modules and all HoS materialized views
are:

- `near.near_blocks`
- `near.near_chunks`
- `near.near_state_changes`
- `near.near_transactions`
- `near.near_receipts`
- `near.near_execution_outcomes`
- `near.near_receipt_actions`
- `near.near_receipt_edges`
- `near.near_receipt_tx_hash_map`
- `near.near_sync_cursors`

These tables are the long-term source of truth. HoS materialized views do not
read from staging tables.

## Raw Module

`raw` is the full-chain canonical writer in
[src/modules/raw/processor.rs](src/modules/raw/processor.rs).

For every streamed block it writes:

- block header plus full block payload into `near_blocks`
- chunk headers plus raw chunk payloads into `near_chunks`
- transactions plus full transaction payloads into `near_transactions`
- receipts from transaction outcomes, chunk receipts, local receipts, and shard
  receipt execution outcomes into `near_receipts`
- execution outcomes into `near_execution_outcomes`
- receipt action rows into `near_receipt_actions`
- parent/child receipt edges into `near_receipt_edges`
- shard `state_changes` payloads into `near_state_changes`
- receipt-to-transaction lineage into `near_receipt_tx_hash_map`

The raw module is not HoS-aware. It stores the chain payload as broadly as
possible and writes canonical tables in block-scoped batches. It uses
`ON CONFLICT` semantics to fill in lineage and raw JSON as more context becomes
available.

Receipt lineage is resolved in layers:

1. derive what can be resolved from the current `StreamerMessage`
2. load known mappings from `near_receipt_tx_hash_map`
3. fall back to canonical `near_execution_outcomes`
4. fall back to canonical `near_receipt_actions`
5. fall back to canonical `near_receipt_edges`
6. fall back to parent execution outcomes through receipt edges

That lineage map is shared with the HoS module.

## HoS Matcher

The HoS module currently treats these accounts as in-scope:

- `venear.dao`
- `vote.dao`
- `hos-root.sputnik-dao.near`
- `rewards.hos-dao.near`
- any hashed veNEAR lockup matching `^[0-9a-f]{40}\\.venear\\.dao$`

Both the Rust matcher and the SQL helper function `near.is_hos_contract()` are
kept aligned. HoS relevance is based on any of:

- `receiver_id`
- `predecessor_id`
- `signer_account_id`

The SQL-side helpers are:

- `near.is_hos_contract(account_id text)`
- `near.is_hos_action(receiver_id text, predecessor_id text, signer_account_id text)`

## HoS Module

`venear_governance` lives in
[src/modules/venear_governance/processor.rs](src/modules/venear_governance/processor.rs).

Its job is to track transaction graphs, decide whether they touch HoS, and
refresh HoS materialized views off the canonical `near_*` tables. It no longer
uses HoS staging tables.

### Current in-memory model

The HoS module keeps runtime graph state in memory:

- one `GraphState` per open transaction graph
- a shared source-block cache keyed by block height
- a full in-memory HoS bundle only after a graph first touches HoS

For each open graph it tracks:

- `first_seen_height` and `last_seen_height`
- whether any node in the graph touched HoS
- whether the root transaction row has been seen
- pending child receipt ids
- open yielded-promise receipts with timeout heights
- per-block source references used for pre-HoS hydration
- an optional HoS bundle once the graph becomes HoS-relevant
- an optional incomplete reason when restart replay cannot recover earlier history

There is no `near_hos_tx_state` table and no `near_hos_staging_*` schema in the
current design.

### HoS-only mode

When `raw` is disabled:

- `venear_governance` keeps open graphs in memory
- non-HoS graphs are pruned immediately when they close
- HoS graphs are written into the canonical `near_*` tables immediately when
  they close
- source blocks are retained while any open pre-HoS graph still references them
- if a restart rewinds too little to rebuild the full pre-HoS history, the
  graph is marked incomplete and not partially projected

This means canonical HoS raw rows are the long-term store, but only HoS-touching
graphs are written there.

### Raw+HoS mode

When `raw` is enabled:

- `raw` writes the canonical `near_*` tables for the full chain
- `venear_governance` still tracks HoS graph closure in memory
- `venear_governance` does not duplicate canonical raw writes
- `venear_governance` remains responsible for HoS MV refresh cadence and HoS
  graph observability

So in dual-module mode, `raw` is the canonical writer and `venear_governance`
is the HoS relevance tracker plus MV refresher.

### Closure and restart semantics

Graph closure is not purely `pending_receipt_count = 0`.

A graph is considered closed only when:

- there are no pending child receipts, and
- there are no unresolved yielded promises still inside their timeout window

The current implementation uses a default yielded-promise timeout of `200`
blocks, matching the protocol-level default used by NEAR yield/resume docs.

Restart recovery for the HoS module is bounded, not durable:

- on restart, the HoS module rewinds `restart_rewind_blocks`
- it rebuilds in-memory graph state by replaying the rewound block range
- while the process stays up, source retention follows open graphs rather than a
  fixed cache window
- if a relevant graph spans beyond the rewind window while `raw=false`, HoS-only
  recovery can miss earlier history after restart

## HoS Materialized Views

The HoS SQL model is created in
[migration/src/m20260310_000019_near_hos_raw_refactor.rs](../../migration/src/m20260310_000019_near_hos_raw_refactor.rs)
and updated by later HoS migrations.

Current HoS materialized views:

- `near.hos_function_calls`
- `near.hos_event_logs`
- `near.hos_proposal_voting_history`
- `near.hos_approved_proposals`
- `near.hos_delegation_events`
- `near.hos_user_activities`
- `near.hos_proposals`
- `near.hos_registered_voters`
- `near.hos_config_history`
- `near.hos_proposal_non_voters`

High-level dependency shape:

- `hos_function_calls` comes primarily from `near_receipt_actions`, joined with
  `near_execution_outcomes`
- `hos_event_logs` comes from HoS execution outcome logs
- proposal, voting, delegation, config, and voter views are built on top of
  those canonical action/outcome/log relations

The refresh entrypoint is:

- `SELECT near.refresh_hos_materialized_views()`

The HoS module checks refresh every `mview_refresh_interval_blocks` blocks, but
only refreshes when at least one HoS graph closed since the last refresh. The
repo default is `100`.

## How The Modules Relate To The Views

If you only care about HoS views and do not want full-chain raw storage:

- enable `venear_governance`
- disable `raw`
- the HoS module will project only closed HoS graphs into canonical raw tables
- restart recovery will be bounded by `restart_rewind_blocks`

If you want full-chain raw plus HoS views:

- enable both `raw` and `venear_governance`
- keep both on `stream_mode = "shared"`
- `raw` becomes the full-chain storage layer
- `venear_governance` becomes the HoS tracker and MV refresher

## Start Heights

There are three start-height inputs to keep in mind:

- global `near.start_height`
- module-specific `near.modules.raw.start_height`
- module-specific `near.modules.venear_governance.start_height`

If a module already has a cursor, the worker resumes from the cursor instead of
the configured height. If there is no cursor, the effective start comes from the
module-specific height if set, otherwise the global height.

For the current HoS matcher scope, the earliest known matcher-covered on-chain
activity is `hos-root.sputnik-dao.near` at block `158320040`, so a HoS-complete
backfill must start at or before that height.

## Observability

Both modules emit one structured success log per block with:

- total `elapsed_ms`
- per-stage `phase_ms`
- selected `counts`
- selected `flags`

The HoS module exposes timings for:

- `lineage_lookup`
- `receipt_tx_map_write`
- `memory_merge`
- `canonical_close_write`
- `receipt_tx_map_cleanup`
- `mview_refresh`

The HoS module also records graph-pressure metrics such as:

- `active_graph_count`
- `active_hos_graph_count`
- `closed_hos_graph_count`
- `pruned_non_hos_graph_count`
- `yield_open_graph_count`
- `late_first_touch_cache_miss_count`
- `max_open_graph_age_blocks`

These are the primary metrics to watch before assuming Postgres is the
bottleneck.

## Tests And Fixtures

The most important tests are:

- [tests/raw_fixtures_e2e.rs](tests/raw_fixtures_e2e.rs): full-chain raw fixture
  projection plus HoS MV golden-row checks
- [tests/venear_raw_projection_e2e.rs](tests/venear_raw_projection_e2e.rs):
  HoS-only projection and graph-finalization behavior
- [tests/fastnear_live_stream_e2e.rs](tests/fastnear_live_stream_e2e.rs): live
  streaming behavior against FastNEAR
- [tests/venear_live_lake_db_e2e.rs](tests/venear_live_lake_db_e2e.rs): live
  HoS ingestion into Postgres

The fixture ranges under `tests/fixtures/venear_governance/ranges/` and the
golden MV rows in
[tests/fixtures/venear_governance/expectations/hos_mv_golden_rows.json](tests/fixtures/venear_governance/expectations/hos_mv_golden_rows.json)
are the best references for expected HoS semantics.
