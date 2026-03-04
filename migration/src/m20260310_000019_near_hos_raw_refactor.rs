use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let drop_legacy_tables = [
            "DROP VIEW IF EXISTS near.hos_execution_outcomes CASCADE",
            "DROP VIEW IF EXISTS near.hos_receipt_actions CASCADE",
            "DROP TABLE IF EXISTS near.near_account_events CASCADE",
            "DROP TABLE IF EXISTS near.near_access_keys CASCADE",
            "DROP TABLE IF EXISTS near.near_account_balances CASCADE",
            "DROP TABLE IF EXISTS near.near_accounts CASCADE",
            "DROP TABLE IF EXISTS near.near_token_events_raw CASCADE",
            "DROP TABLE IF EXISTS near.near_token_events CASCADE",
            "DROP TABLE IF EXISTS near.near_token_contracts CASCADE",
            "DROP TABLE IF EXISTS near.near_token_balances CASCADE",
            "DROP TABLE IF EXISTS near.near_token_transfers CASCADE",
            "DROP TABLE IF EXISTS near.near_token_metadata CASCADE",
            "DROP TABLE IF EXISTS near.near_staking_pool_balances CASCADE",
            "DROP TABLE IF EXISTS near.near_staking_pool_rewards CASCADE",
            "DROP TABLE IF EXISTS near.near_staking_validator_proposals CASCADE",
            "DROP TABLE IF EXISTS near.near_staking_epochs CASCADE",
            "DROP TABLE IF EXISTS near.near_staking_delegators CASCADE",
            "DROP TABLE IF EXISTS near.near_staking_pool_events CASCADE",
            "DROP TABLE IF EXISTS near.near_staking_pool_actions CASCADE",
            "DROP TABLE IF EXISTS near.near_staking_pools CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_receipt_tx_origins CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_unhandled_events CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_governance_actions CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_admin_events CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_account_voting_power_events CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_delegation_events CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_lockup_events CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_lockup_actions CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_lockups CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_votes CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_proposal_events CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_account_balances CASCADE",
            "DROP TABLE IF EXISTS near.near_venear_events_raw CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposal_non_voters CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_config_history CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_registered_voters CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_user_activities CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposals CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_delegation_events CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_approved_proposals CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposal_voting_history CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_event_logs CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_function_calls CASCADE",
            "DROP FUNCTION IF EXISTS near.refresh_hos_materialized_views()",
            "DROP TABLE IF EXISTS near.hos_receipt_edges CASCADE",
            "DROP TABLE IF EXISTS near.hos_receipts CASCADE",
            "DROP TABLE IF EXISTS near.hos_relevant_receipts CASCADE",
            "DROP TABLE IF EXISTS near.hos_execution_outcomes CASCADE",
            "DROP TABLE IF EXISTS near.hos_receipt_actions CASCADE",
            "DROP TABLE IF EXISTS near.near_receipt_edges CASCADE",
            "DROP TABLE IF EXISTS near.near_receipt_actions CASCADE",
            "DROP TABLE IF EXISTS near.venear_governance_relevant_receipts CASCADE",
            "DROP TABLE IF EXISTS near.hos_config CASCADE",
            "DROP FUNCTION IF EXISTS near.is_hos_action(TEXT, TEXT, TEXT)",
            "DROP FUNCTION IF EXISTS near.is_hos_contract(TEXT)",
            "DROP FUNCTION IF EXISTS near.safe_json_parse(TEXT)",
        ];

        for statement in drop_legacy_tables {
            db.execute_unprepared(statement).await?;
        }

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS near.hos_config (
                id INTEGER PRIMARY KEY,
                hos_contract TEXT NOT NULL,
                venear_contract_prefix TEXT NOT NULL,
                voting_contract_prefix TEXT NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                CONSTRAINT ck_near_hos_config_singleton CHECK (id = 1)
            )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO near.hos_config (id, hos_contract, venear_contract_prefix, voting_contract_prefix)
             VALUES (1, 'dao', 'venear', 'vote')
             ON CONFLICT (id) DO NOTHING",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS near.venear_governance_relevant_receipts (
                receipt_id TEXT PRIMARY KEY,
                reason TEXT NOT NULL,
                discovered_by_receipt_id TEXT,
                transaction_hash TEXT,
                discovered_at_block_height BIGINT NOT NULL,
                executed BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_venear_gov_relevant_receipts_executed_height
               ON near.venear_governance_relevant_receipts (executed, discovered_at_block_height)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_venear_gov_relevant_receipts_transaction_hash
               ON near.venear_governance_relevant_receipts (transaction_hash)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS near.near_receipt_edges (
                parent_receipt_id TEXT NOT NULL,
                child_receipt_id TEXT NOT NULL,
                parent_execution_outcome_id TEXT NOT NULL,
                block_height BIGINT NOT NULL,
                block_hash TEXT NOT NULL,
                block_timestamp BIGINT NOT NULL,
                transaction_hash TEXT,
                PRIMARY KEY (parent_receipt_id, child_receipt_id)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_near_receipt_edges_child_receipt_id
               ON near.near_receipt_edges (child_receipt_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_near_receipt_edges_transaction_hash
               ON near.near_receipt_edges (transaction_hash)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS near.near_receipt_actions (
                id TEXT PRIMARY KEY,
                block_height BIGINT NOT NULL,
                receipt_id TEXT NOT NULL,
                execution_outcome_id TEXT NOT NULL,
                transaction_hash TEXT,
                shard_id BIGINT NOT NULL,
                signer_account_id TEXT,
                signer_public_key TEXT,
                gas_price TEXT,
                action_kind TEXT NOT NULL,
                action_json JSONB NOT NULL,
                predecessor_id TEXT NOT NULL,
                receiver_id TEXT NOT NULL,
                block_hash TEXT NOT NULL,
                chunk_hash TEXT NOT NULL,
                author TEXT NOT NULL,
                method_name TEXT,
                gas BIGINT,
                deposit TEXT,
                args_base64 TEXT,
                args_json JSONB,
                action_index INTEGER NOT NULL,
                block_timestamp BIGINT NOT NULL
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_receipt_id
               ON near.near_receipt_actions (receipt_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_receiver_method_height
               ON near.near_receipt_actions (receiver_id, method_name, block_height)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_signer_height
               ON near.near_receipt_actions (signer_account_id, block_height)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_kind_height
               ON near.near_receipt_actions (action_kind, block_height)",
        )
        .await?;

        let enrich_execution_outcomes = [
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS transaction_hash TEXT",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS block_hash TEXT",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS chunk_hash TEXT",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS gas_burnt BIGINT",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS gas_used DOUBLE PRECISION",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS tokens_burnt DOUBLE PRECISION",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS executor_account_id TEXT",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS status TEXT",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS outcome_receipt_ids JSONB",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS executed_in_block_hash TEXT",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS logs JSONB",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS results_json JSONB",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS block_timestamp BIGINT",
            "CREATE INDEX IF NOT EXISTS ix_near_execution_outcomes_status_height_enriched ON near.near_execution_outcomes (status, block_height)",
            "CREATE INDEX IF NOT EXISTS ix_near_execution_outcomes_transaction_hash_enriched ON near.near_execution_outcomes (transaction_hash)",
        ];

        for statement in enrich_execution_outcomes {
            db.execute_unprepared(statement).await?;
        }

        db.execute_unprepared(
            "CREATE OR REPLACE FUNCTION near.safe_json_parse(input_text TEXT)
             RETURNS JSONB
             LANGUAGE plpgsql
             STABLE
             AS $$
             BEGIN
               IF input_text IS NULL OR btrim(input_text) = '' THEN
                 RETURN NULL;
               END IF;
               RETURN input_text::jsonb;
             EXCEPTION WHEN OTHERS THEN
               RETURN jsonb_build_object('error', 'invalid_json');
             END;
             $$",
        )
        .await?;

        db.execute_unprepared(
            "CREATE OR REPLACE FUNCTION near.is_hos_contract(account_id TEXT)
             RETURNS BOOLEAN
             LANGUAGE SQL
             STABLE
             AS $$
               SELECT CASE
                 WHEN account_id IS NULL OR btrim(account_id) = '' THEN FALSE
                 ELSE EXISTS (
                   SELECT 1
                   FROM near.hos_config cfg
                   WHERE account_id = cfg.hos_contract
                      OR account_id = cfg.venear_contract_prefix || '.' || cfg.hos_contract
                      OR account_id = cfg.voting_contract_prefix || '.' || cfg.hos_contract
                      OR account_id LIKE 'lockup.%.' || cfg.venear_contract_prefix || '.' || cfg.hos_contract
                      OR account_id LIKE 'lockup.%.' || cfg.voting_contract_prefix || '.' || cfg.hos_contract
                 )
               END
             $$",
        )
        .await?;

        db.execute_unprepared(
            "CREATE OR REPLACE FUNCTION near.is_hos_action(receiver_id TEXT, predecessor_id TEXT, signer_account_id TEXT)
             RETURNS BOOLEAN
             LANGUAGE SQL
             STABLE
             AS $$
               SELECT
                 near.is_hos_contract(receiver_id)
                 OR near.is_hos_contract(predecessor_id)
                 OR near.is_hos_contract(signer_account_id)
             $$",
        )
        .await?;

        let drop_mviews = [
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposal_non_voters CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_config_history CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_registered_voters CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_user_activities CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposals CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_delegation_events CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_approved_proposals CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposal_voting_history CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_event_logs CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_function_calls CASCADE",
        ];

        for statement in drop_mviews {
            db.execute_unprepared(statement).await?;
        }

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_function_calls AS
            SELECT
                md5(ra.id || ':function_call') AS id,
                ra.id AS action_id,
                ra.receipt_id,
                ra.execution_outcome_id,
                ra.transaction_hash,
                ra.block_height,
                ra.block_hash,
                ra.chunk_hash,
                ra.shard_id,
                ra.predecessor_id,
                ra.receiver_id,
                ra.signer_account_id,
                ra.signer_public_key,
                ra.gas_price,
                ra.method_name,
                ra.gas,
                ra.deposit,
                ra.args_base64,
                ra.args_json,
                eo.status AS outcome_status,
                eo.logs AS outcome_logs,
                eo.results_json,
                eo.outcome_receipt_ids,
                ra.block_timestamp
            FROM near.near_receipt_actions ra
            LEFT JOIN near.near_execution_outcomes eo
                ON eo.receipt_id = ra.receipt_id
               AND near.is_hos_contract(eo.executor_account_id) = TRUE
            WHERE ra.action_kind = 'FunctionCall'
              AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_event_logs AS
            WITH log_rows AS (
                SELECT
                    eo.receipt_id,
                    eo.id AS execution_outcome_id,
                    eo.transaction_hash,
                    eo.block_height,
                    eo.block_hash,
                    eo.chunk_hash,
                    eo.shard_id,
                    eo.executor_account_id,
                    eo.block_timestamp,
                    (logs.ordinality - 1)::INTEGER AS log_index,
                    logs.log_line,
                    near.safe_json_parse(REPLACE(COALESCE(logs.log_line, ''), 'EVENT_JSON:', '')) AS event_json
                FROM near.near_execution_outcomes eo
                LEFT JOIN LATERAL jsonb_array_elements_text(COALESCE(eo.logs, '[]'::JSONB))
                    WITH ORDINALITY AS logs(log_line, ordinality)
                    ON TRUE
                WHERE near.is_hos_contract(eo.executor_account_id) = TRUE
                  AND COALESCE(logs.log_line, '') LIKE 'EVENT_JSON:%'
            )
            SELECT
                md5(receipt_id || ':' || log_index::TEXT) AS id,
                receipt_id,
                execution_outcome_id,
                transaction_hash,
                block_height,
                block_hash,
                chunk_hash,
                shard_id,
                executor_account_id AS contract_id,
                log_index,
                log_line,
                event_json,
                event_json ->> 'standard' AS event_standard,
                event_json ->> 'version' AS event_version,
                event_json ->> 'event' AS event_type,
                event_json -> 'data' AS event_data,
                block_timestamp
            FROM log_rows
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_proposal_voting_history AS
            WITH vote_actions AS (
                SELECT
                    ra.id,
                    ra.receipt_id,
                    ra.execution_outcome_id,
                    ra.transaction_hash,
                    ra.predecessor_id AS voter_id,
                    ra.receiver_id AS hos_contract_address,
                    ra.args_json,
                    ra.block_height,
                    ra.block_hash,
                    ra.block_timestamp,
                    eo.logs,
                    eo.status,
                    near.safe_json_parse(REPLACE(COALESCE(eo.logs ->> 0, ''), 'EVENT_JSON:', '')) AS event_log
                FROM near.near_receipt_actions ra
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                WHERE ra.action_kind = 'FunctionCall'
                  AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                  AND ra.method_name = 'vote'
                  AND eo.status IN ('SuccessReceiptId', 'SuccessValue')
            ),
            proposal_metadata AS (
                SELECT DISTINCT ON (proposal_id)
                    proposal_id,
                    proposal_title
                FROM (
                    SELECT
                        CASE
                            WHEN COALESCE(ra.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
                                THEN (ra.args_json ->> 'proposal_id')::NUMERIC
                        END AS proposal_id,
                        ra.args_json -> 'metadata' ->> 'title' AS proposal_title,
                        ra.block_timestamp,
                        ra.receipt_id
                    FROM near.near_receipt_actions ra
                    INNER JOIN near.near_execution_outcomes eo
                        ON eo.receipt_id = ra.receipt_id
                       AND near.is_hos_contract(eo.executor_account_id) = TRUE
                    WHERE ra.method_name = 'create_proposal'
                      AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                      AND eo.status IN ('SuccessReceiptId', 'SuccessValue')
                ) m
                WHERE proposal_id IS NOT NULL
                ORDER BY proposal_id, block_timestamp ASC, receipt_id ASC
            ),
            voting_history AS (
                SELECT
                    va.id,
                    va.receipt_id,
                    TO_TIMESTAMP(va.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC' AS voted_at,
                    DATE(TO_TIMESTAMP(va.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC') AS voted_date,
                    CASE
                        WHEN COALESCE(va.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
                            THEN (va.args_json ->> 'proposal_id')::NUMERIC
                    END AS proposal_id,
                    va.hos_contract_address,
                    va.voter_id,
                    CASE
                        WHEN COALESCE(va.args_json ->> 'vote', '') ~ '^[0-9]+$'
                            THEN (va.args_json ->> 'vote')::NUMERIC
                    END AS vote_option,
                    CASE
                        WHEN COALESCE(va.event_log ->> 'error', '') = ''
                            THEN NULLIF(va.event_log -> 'data' -> 0 ->> 'account_balance', '')::NUMERIC
                    END AS voting_power,
                    NULLIF(va.args_json -> 'v_account' -> 'V0' -> 'balance' ->> 'near_balance', '')::NUMERIC AS near_balance,
                    NULLIF(va.args_json -> 'v_account' -> 'V0' -> 'balance' ->> 'extra_venear_balance', '')::NUMERIC AS extra_venear_balance,
                    va.args_json -> 'v_account' -> 'V0' -> 'delegation' ->> 'account_id' AS delegator_account_id,
                    NULLIF(va.args_json -> 'v_account' -> 'V0' -> 'delegated_balance' ->> 'near_balance', '')::NUMERIC AS delegated_near_balance,
                    NULLIF(va.args_json -> 'v_account' -> 'V0' -> 'delegated_balance' ->> 'extra_venear_balance', '')::NUMERIC AS delegated_extra_venear_balance,
                    va.block_height,
                    va.block_hash,
                    va.block_timestamp,
                    va.transaction_hash,
                    va.execution_outcome_id,
                    COALESCE(va.event_log ->> 'event', 'add_vote') AS event_type
                FROM vote_actions va
                WHERE COALESCE(va.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
            ),
            latest_vote_per_voter AS (
                SELECT DISTINCT ON (proposal_id, voter_id)
                    *
                FROM voting_history
                ORDER BY proposal_id, voter_id, block_timestamp DESC, receipt_id DESC
            )
            SELECT
                lv.id,
                lv.receipt_id,
                lv.voted_date,
                lv.voted_at,
                lv.proposal_id,
                pm.proposal_title AS proposal_name,
                lv.hos_contract_address,
                lv.voter_id,
                lv.vote_option,
                lv.voting_power,
                lv.near_balance,
                lv.extra_venear_balance,
                lv.delegator_account_id,
                lv.delegated_near_balance,
                lv.delegated_extra_venear_balance,
                lv.block_height,
                lv.block_hash,
                lv.block_timestamp,
                lv.transaction_hash,
                lv.execution_outcome_id,
                lv.event_type
            FROM latest_vote_per_voter lv
            LEFT JOIN proposal_metadata pm
                ON pm.proposal_id = lv.proposal_id
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_approved_proposals AS
            SELECT
                ra.id,
                ra.receipt_id,
                DATE(TO_TIMESTAMP(ra.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC') AS proposal_approved_date,
                TO_TIMESTAMP(ra.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC' AS proposal_approved_at,
                ra.receiver_id AS hos_contract_address,
                CASE
                    WHEN COALESCE(ra.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
                        THEN (ra.args_json ->> 'proposal_id')::NUMERIC
                END AS proposal_id,
                ra.signer_account_id AS proposal_approver_id,
                ra.block_hash,
                ra.block_height,
                ra.block_timestamp,
                ra.transaction_hash,
                ra.execution_outcome_id
            FROM near.near_receipt_actions ra
            INNER JOIN near.near_execution_outcomes eo
                ON eo.receipt_id = ra.receipt_id
               AND near.is_hos_contract(eo.executor_account_id) = TRUE
            WHERE ra.action_kind = 'FunctionCall'
              AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
              AND ra.method_name = 'approve_proposal'
              AND eo.status = 'SuccessReceiptId'
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_delegation_events AS
            WITH action_base AS (
                SELECT
                    ra.id,
                    ra.receipt_id,
                    ra.execution_outcome_id,
                    ra.transaction_hash,
                    ra.predecessor_id AS delegator_account_id,
                    ra.receiver_id AS hos_contract_address,
                    ra.args_json,
                    ra.method_name,
                    ra.block_height,
                    ra.block_hash,
                    ra.block_timestamp,
                    eo.status,
                    eo.logs
                FROM near.near_receipt_actions ra
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                WHERE ra.action_kind = 'FunctionCall'
                  AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                  AND ra.method_name IN ('delegate_all', 'undelegate')
                  AND eo.status IN ('SuccessReceiptId', 'SuccessValue')
            ),
            parsed_logs AS (
                SELECT
                    ab.*,
                    log_line,
                    near.safe_json_parse(REPLACE(COALESCE(log_line, ''), 'EVENT_JSON:', '')) AS parsed_log
                FROM action_base ab
                LEFT JOIN LATERAL jsonb_array_elements_text(COALESCE(ab.logs, '[]'::jsonb)) logs(log_line)
                    ON TRUE
            ),
            normalized AS (
                SELECT
                    md5(ab.receipt_id || '_' || COALESCE(parsed_log -> 'data' -> 0 ->> 'owner_id', '')) AS id,
                    ab.receipt_id,
                    DATE(TO_TIMESTAMP(ab.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC') AS event_date,
                    TO_TIMESTAMP(ab.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC' AS event_timestamp,
                    ab.hos_contract_address,
                    ab.delegator_account_id,
                    CASE
                        WHEN ab.method_name = 'delegate_all'
                            THEN ab.args_json ->> 'receiver_id'
                    END AS delegatee_account_id,
                    ab.method_name AS event_type,
                    ab.method_name AS delegate_method,
                    parsed_log ->> 'event' AS delegate_event,
                    parsed_log -> 'data' -> 0 ->> 'owner_id' AS owner_id,
                    NULLIF(parsed_log -> 'data' -> 0 ->> 'amount', '')::NUMERIC AS near_amount,
                    TRUE AS success,
                    ab.block_height,
                    ab.block_hash,
                    ab.block_timestamp,
                    ab.transaction_hash,
                    ab.execution_outcome_id
                FROM parsed_logs ab
            ),
            latest AS (
                SELECT
                    n.*,
                    ROW_NUMBER() OVER (
                        PARTITION BY n.delegator_account_id
                        ORDER BY n.block_timestamp DESC, n.receipt_id DESC
                    ) AS row_num
                FROM normalized n
            )
            SELECT
                id,
                receipt_id,
                event_date,
                event_timestamp,
                hos_contract_address,
                delegator_account_id,
                delegatee_account_id,
                event_type,
                delegate_method,
                delegate_event,
                row_num = 1 AS is_latest_delegator_event,
                owner_id,
                near_amount,
                success,
                block_height,
                block_hash,
                block_timestamp,
                transaction_hash,
                execution_outcome_id
            FROM latest
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_user_activities AS
            WITH base AS (
                SELECT
                    ra.id,
                    ra.receipt_id,
                    ra.execution_outcome_id,
                    ra.transaction_hash,
                    ra.receiver_id AS hos_contract_address,
                    ra.signer_account_id AS account_id,
                    ra.method_name,
                    ra.args_json,
                    ra.block_height,
                    ra.block_hash,
                    ra.block_timestamp,
                    eo.status,
                    eo.logs,
                    near.safe_json_parse(REPLACE(COALESCE(eo.logs ->> 0, ''), 'EVENT_JSON:', '')) AS first_log
                FROM near.near_receipt_actions ra
                LEFT JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                WHERE ra.action_kind = 'FunctionCall'
                  AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                  AND ra.method_name IN (
                        'new',
                        'storage_deposit',
                        'on_lockup_deployed',
                        'on_lockup_update',
                        'lock_near',
                        'lock_pending_near',
                        'begin_unlock_near',
                        'delegate_all',
                        'undelegate',
                        'vote',
                        'create_proposal',
                        'approve_proposal',
                        'reject_proposal',
                        'on_get_snapshot',
                        'withdraw_from_staking_pool',
                        'withdraw_all_from_staking_pool',
                        'unstake',
                        'unstake_all'
                  )
            )
            SELECT
                md5(id || ':' || method_name) AS id,
                receipt_id,
                hos_contract_address,
                account_id,
                DATE(TO_TIMESTAMP(block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC') AS event_date,
                TO_TIMESTAMP(block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC' AS event_timestamp,
                method_name,
                COALESCE(
                    CASE
                        WHEN COALESCE(first_log ->> 'error', '') = '' THEN
                            CASE
                                WHEN method_name IN ('delegate_all', 'undelegate', 'on_lockup_update')
                                    THEN method_name || '_' || COALESCE(first_log ->> 'event', '')
                                ELSE first_log ->> 'event'
                            END
                    END,
                    method_name
                ) AS event_type,
                CASE
                    WHEN status IN ('SuccessReceiptId', 'SuccessValue') THEN 'succeeded'
                    WHEN status = 'Failure' THEN 'failed'
                    ELSE 'unknown'
                END AS event_status,
                COALESCE(
                    NULLIF(args_json ->> 'amount', '')::NUMERIC,
                    NULLIF(args_json ->> 'lockup_deposit', '')::NUMERIC,
                    NULLIF(first_log -> 'data' -> 0 ->> 'amount', '')::NUMERIC,
                    NULLIF(first_log -> 'data' -> 0 ->> 'account_balance', '')::NUMERIC
                ) AS near_amount,
                NULLIF(first_log -> 'data' -> 0 ->> 'locked_near_balance', '')::NUMERIC AS locked_near_balance,
                CASE
                    WHEN COALESCE(args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
                        THEN (args_json ->> 'proposal_id')::BIGINT
                END AS proposal_id,
                COALESCE(
                    NULLIF(first_log -> 'data' -> 0 ->> 'account_balance', '')::NUMERIC,
                    NULLIF(args_json ->> 'amount', '')::NUMERIC
                ) AS voting_power,
                block_height,
                block_hash,
                block_timestamp,
                transaction_hash,
                execution_outcome_id
            FROM base
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_proposals AS
            WITH create_proposal AS (
                SELECT DISTINCT ON (proposal_id)
                    ra.id,
                    ra.receipt_id,
                    ra.execution_outcome_id,
                    ra.transaction_hash,
                    ra.receiver_id AS hos_contract_address,
                    ra.signer_account_id AS creator_id,
                    ra.args_json -> 'metadata' AS metadata,
                    CASE
                        WHEN COALESCE(near.safe_json_parse(REPLACE(COALESCE(eo.logs ->> 0, ''), 'EVENT_JSON:', '')) ->> 'error', '') = ''
                            THEN NULLIF(near.safe_json_parse(REPLACE(COALESCE(eo.logs ->> 0, ''), 'EVENT_JSON:', '')) -> 'data' -> 0 ->> 'proposal_id', '')::NUMERIC
                    END AS proposal_id,
                    ra.block_height,
                    ra.block_hash,
                    ra.block_timestamp
                FROM near.near_receipt_actions ra
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                WHERE ra.method_name = 'create_proposal'
                  AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                  AND eo.status IN ('SuccessReceiptId', 'SuccessValue')
                ORDER BY proposal_id, ra.block_timestamp ASC, ra.receipt_id ASC
            ),
            reject_proposal AS (
                SELECT DISTINCT ON (proposal_id)
                    CASE
                        WHEN COALESCE(ra.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
                            THEN (ra.args_json ->> 'proposal_id')::NUMERIC
                    END AS proposal_id,
                    ra.signer_account_id AS rejecter_id,
                    ra.block_timestamp,
                    ra.transaction_hash,
                    ra.execution_outcome_id,
                    ra.receipt_id
                FROM near.near_receipt_actions ra
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                WHERE ra.method_name = 'reject_proposal'
                  AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                  AND eo.status IN ('SuccessReceiptId', 'SuccessValue')
                ORDER BY proposal_id, ra.block_timestamp DESC, ra.receipt_id DESC
            ),
            approve_proposal AS (
                SELECT DISTINCT ON (proposal_id)
                    ap.proposal_id,
                    ap.proposal_approver_id AS approver_id,
                    ap.proposal_approved_at,
                    ap.block_timestamp,
                    ap.receipt_id,
                    ap.transaction_hash,
                    ap.execution_outcome_id,
                    eo.results_json AS approve_results_json
                FROM near.hos_approved_proposals ap
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ap.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                ORDER BY proposal_id, ap.block_timestamp DESC, ap.receipt_id DESC
            ),
            snapshot_payload AS (
                SELECT
                    ap.proposal_id,
                    ra.receipt_id,
                    eo.results_json AS snapshot_results
                FROM approve_proposal ap
                INNER JOIN near.near_receipt_actions ra
                    ON ra.receipt_id = ap.approve_results_json ->> 'receipt_id'
                   AND ra.method_name = 'on_get_snapshot'
                   AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
            ),
            voting_aggregates AS (
                SELECT
                    proposal_id,
                    COUNT(*)::BIGINT AS num_distinct_voters,
                    STRING_AGG(DISTINCT voter_id, ', ' ORDER BY voter_id) AS listagg_distinct_voters,
                    SUM(CASE WHEN vote_option = 0 THEN 1 ELSE 0 END)::BIGINT AS num_for_votes,
                    SUM(CASE WHEN vote_option = 1 THEN 1 ELSE 0 END)::BIGINT AS num_against_votes,
                    SUM(CASE WHEN vote_option = 2 THEN 1 ELSE 0 END)::BIGINT AS num_abstain_votes,
                    COALESCE(SUM(CASE WHEN vote_option = 0 THEN voting_power ELSE 0 END), 0)::NUMERIC AS for_voting_power,
                    COALESCE(SUM(CASE WHEN vote_option = 1 THEN voting_power ELSE 0 END), 0)::NUMERIC AS against_voting_power,
                    COALESCE(SUM(CASE WHEN vote_option = 2 THEN voting_power ELSE 0 END), 0)::NUMERIC AS abstain_voting_power
                FROM near.hos_proposal_voting_history
                GROUP BY proposal_id
            )
            SELECT
                cp.id,
                cp.receipt_id,
                cp.proposal_id,
                cp.metadata ->> 'title' AS proposal_title,
                cp.metadata ->> 'description' AS proposal_description,
                cp.metadata ->> 'link' AS proposal_url,
                cp.hos_contract_address,
                ap.proposal_id IS NOT NULL AS is_approved,
                rp.proposal_id IS NOT NULL AS is_rejected,
                COALESCE(va.num_distinct_voters, 0) > 0 AS has_votes,
                TO_TIMESTAMP(COALESCE(
                    NULLIF(sp.snapshot_results ->> 'creation_time_ns', '')::NUMERIC,
                    cp.block_timestamp::NUMERIC
                ) / 1e9) AT TIME ZONE 'UTC' AS created_at,
                cp.creator_id,
                TO_TIMESTAMP(ap.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC' AS approved_at,
                TO_TIMESTAMP(NULLIF(sp.snapshot_results ->> 'voting_start_time_ns', '')::NUMERIC / 1e9) AT TIME ZONE 'UTC' AS voting_start_at,
                ap.approver_id,
                TO_TIMESTAMP(rp.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC' AS rejected_at,
                rp.rejecter_id,
                NULLIF(sp.snapshot_results ->> 'voting_duration_ns', '')::NUMERIC AS voting_duration_ns,
                NULLIF(sp.snapshot_results -> 'snapshot_and_state' ->> 'total_venear', '')::NUMERIC AS total_venear_at_approval,
                va.listagg_distinct_voters,
                COALESCE(va.num_distinct_voters, 0) AS num_distinct_voters,
                COALESCE(va.num_for_votes, 0) AS num_for_votes,
                COALESCE(va.num_against_votes, 0) AS num_against_votes,
                COALESCE(va.num_abstain_votes, 0) AS num_abstain_votes,
                COALESCE(va.for_voting_power, 0) AS for_voting_power,
                COALESCE(va.against_voting_power, 0) AS against_voting_power,
                COALESCE(va.abstain_voting_power, 0) AS abstain_voting_power,
                cp.block_height,
                cp.block_hash,
                cp.block_timestamp,
                cp.transaction_hash,
                cp.execution_outcome_id,
                cp.metadata AS proposal_metadata,
                jsonb_build_object(
                    'success_value',
                    jsonb_strip_nulls(jsonb_build_object(
                        'voting_start_time_ns', NULLIF(sp.snapshot_results ->> 'voting_start_time_ns', '')::NUMERIC,
                        'voting_duration_ns', NULLIF(sp.snapshot_results ->> 'voting_duration_ns', '')::NUMERIC,
                        'creation_time_ns', NULLIF(sp.snapshot_results ->> 'creation_time_ns', '')::NUMERIC,
                        'status', CASE
                            WHEN rp.proposal_id IS NOT NULL THEN 'rejected'
                            WHEN ap.proposal_id IS NOT NULL THEN 'approved'
                            ELSE 'created'
                        END
                    ))
                ) AS proposal_payload,
                COALESCE(
                    (
                        SELECT ARRAY_AGG(value)
                        FROM jsonb_array_elements_text(cp.metadata -> 'voting_options') AS options(value)
                    ),
                    ARRAY['For', 'Against', 'Abstain']
                ) AS voting_options
            FROM create_proposal cp
            LEFT JOIN approve_proposal ap ON ap.proposal_id = cp.proposal_id
            LEFT JOIN snapshot_payload sp ON sp.proposal_id = cp.proposal_id
            LEFT JOIN reject_proposal rp ON rp.proposal_id = cp.proposal_id
            LEFT JOIN voting_aggregates va ON va.proposal_id = cp.proposal_id
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_registered_voters AS
            WITH registration_events AS (
                SELECT
                    ua.receipt_id,
                    ua.hos_contract_address,
                    COALESCE(
                        ua.account_id,
                        ua.event_type,
                        ua.event_status
                    ) AS fallback_account,
                    COALESCE(
                        NULLIF(ua.event_type, ''),
                        NULLIF(ua.account_id, '')
                    ) AS fallback_event,
                    COALESCE(
                        NULLIF(ua.account_id, ''),
                        NULLIF(ua.hos_contract_address, '')
                    ) AS account_from_row,
                    ua.block_timestamp,
                    ua.block_height,
                    ua.block_hash,
                    near.safe_json_parse(REPLACE(COALESCE(eo.logs ->> 0, ''), 'EVENT_JSON:', '')) AS event_log,
                    ra.args_json
                FROM near.hos_user_activities ua
                INNER JOIN near.near_receipt_actions ra
                    ON ra.receipt_id = ua.receipt_id
                   AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                LEFT JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ua.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                WHERE ua.method_name = 'on_lockup_deployed'
            ),
            first_registration AS (
                SELECT DISTINCT ON (registered_voter_id)
                    md5(re.receipt_id) AS id,
                    re.receipt_id,
                    re.hos_contract_address,
                    registered_voter_id,
                    re.block_timestamp,
                    re.block_height,
                    re.block_hash
                FROM (
                    SELECT
                        receipt_id,
                        hos_contract_address,
                        COALESCE(
                            event_log -> 'data' -> 0 ->> 'account_id',
                            args_json ->> 'account_id',
                            account_from_row
                        ) AS registered_voter_id,
                        block_timestamp,
                        block_height,
                        block_hash
                    FROM registration_events
                ) re
                WHERE registered_voter_id IS NOT NULL
                ORDER BY registered_voter_id, block_timestamp ASC, receipt_id ASC
            ),
            first_storage AS (
                SELECT DISTINCT ON (account_id)
                    account_id,
                    COALESCE(near_amount, 0) AS initial_voting_power
                FROM near.hos_user_activities
                WHERE method_name = 'storage_deposit'
                  AND account_id IS NOT NULL
                ORDER BY account_id, block_timestamp ASC, receipt_id ASC
            ),
            latest_lockup_update AS (
                SELECT DISTINCT ON (account_id)
                    account_id,
                    COALESCE(locked_near_balance, 0) AS voting_power_from_locks_unlocks,
                    block_timestamp AS latest_lockup_update_ts
                FROM near.hos_user_activities
                WHERE method_name = 'on_lockup_update'
                  AND account_id IS NOT NULL
                ORDER BY account_id, block_timestamp DESC, receipt_id DESC
            ),
            base_balances AS (
                SELECT
                    fr.*,
                    COALESCE(fs.initial_voting_power, 0) AS initial_voting_power,
                    COALESCE(lu.voting_power_from_locks_unlocks, 0) AS voting_power_from_locks_unlocks,
                    COALESCE(fs.initial_voting_power, 0) + COALESCE(lu.voting_power_from_locks_unlocks, 0) AS principal_balance,
                    COALESCE(lu.latest_lockup_update_ts, fr.block_timestamp) AS balance_updated_ts
                FROM first_registration fr
                LEFT JOIN first_storage fs ON fs.account_id = fr.registered_voter_id
                LEFT JOIN latest_lockup_update lu ON lu.account_id = fr.registered_voter_id
            ),
            latest_delegations AS (
                SELECT DISTINCT ON (delegator_account_id)
                    delegator_account_id,
                    delegatee_account_id,
                    event_type,
                    block_timestamp
                FROM near.hos_delegation_events
                WHERE event_type IN ('delegate_all', 'undelegate')
                  AND success = TRUE
                ORDER BY delegator_account_id, block_timestamp DESC, receipt_id DESC
            ),
            active_delegations AS (
                SELECT
                    delegator_account_id,
                    delegatee_account_id,
                    block_timestamp
                FROM latest_delegations
                WHERE event_type = 'delegate_all'
                  AND delegatee_account_id IS NOT NULL
            ),
            delegated_in AS (
                SELECT
                    ad.delegatee_account_id AS account_id,
                    COUNT(*)::BIGINT AS delegators_count,
                    COALESCE(SUM(bb.principal_balance), 0)::NUMERIC AS delegated_voting_power,
                    MAX(ad.block_timestamp)::BIGINT AS latest_inbound_ts
                FROM active_delegations ad
                LEFT JOIN base_balances bb ON bb.registered_voter_id = ad.delegator_account_id
                GROUP BY ad.delegatee_account_id
            ),
            latest_votes AS (
                SELECT
                    voter_id AS account_id,
                    MAX(block_timestamp)::BIGINT AS last_vote_ts
                FROM near.hos_proposal_voting_history
                GROUP BY voter_id
            ),
            ten_recent_approved AS (
                SELECT proposal_id
                FROM near.hos_approved_proposals
                ORDER BY proposal_approved_at DESC
                LIMIT 10
            ),
            participation AS (
                SELECT
                    voter_id AS account_id,
                    COUNT(DISTINCT CASE WHEN p.proposal_id IS NOT NULL THEN pvh.proposal_id END)::NUMERIC / 10 AS proposal_participation_rate
                FROM near.hos_proposal_voting_history pvh
                LEFT JOIN ten_recent_approved p ON p.proposal_id = pvh.proposal_id
                GROUP BY voter_id
            )
            SELECT
                bb.id,
                bb.receipt_id,
                DATE(TO_TIMESTAMP(bb.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC') AS registered_date,
                TO_TIMESTAMP(bb.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC' AS registered_at,
                bb.registered_voter_id,
                bb.hos_contract_address,
                bb.voting_power_from_locks_unlocks <> 0 AS has_locked_unlocked_near,
                ad.delegator_account_id IS NOT NULL AS is_actively_delegating,
                ad.delegatee_account_id AS delegatee_id,
                COALESCE(pt.proposal_participation_rate, 0) AS proposal_participation_rate,
                bb.voting_power_from_locks_unlocks,
                bb.initial_voting_power,
                bb.principal_balance,
                0::NUMERIC AS extra_venear_on_principal,
                COALESCE(di.delegated_voting_power, 0) AS voting_power_from_delegations,
                0::NUMERIC AS delegated_extra_venear,
                CASE
                    WHEN ad.delegator_account_id IS NOT NULL THEN COALESCE(di.delegated_voting_power, 0)
                    ELSE bb.principal_balance + COALESCE(di.delegated_voting_power, 0)
                END AS current_voting_power,
                bb.block_height,
                bb.block_hash,
                bb.block_timestamp,
                bb.principal_balance::TEXT AS own_voting_power,
                COALESCE(di.delegated_voting_power, 0)::TEXT AS delegated_voting_power,
                CASE
                    WHEN ad.delegator_account_id IS NOT NULL THEN COALESCE(di.delegated_voting_power, 0)::TEXT
                    ELSE (bb.principal_balance + COALESCE(di.delegated_voting_power, 0))::TEXT
                END AS total_voting_power,
                COALESCE(di.delegators_count, 0)::BIGINT AS delegators_count,
                ad.delegatee_account_id AS delegated_to_account_id,
                di.account_id IS NOT NULL AS has_inbound_delegation,
                ad.delegator_account_id IS NOT NULL AS has_outbound_delegation,
                lv.last_vote_ts,
                GREATEST(
                    bb.balance_updated_ts,
                    COALESCE(di.latest_inbound_ts, 0),
                    COALESCE(ad.block_timestamp, 0),
                    COALESCE(lv.last_vote_ts, 0)
                )::BIGINT AS updated_ts
            FROM base_balances bb
            LEFT JOIN active_delegations ad ON ad.delegator_account_id = bb.registered_voter_id
            LEFT JOIN delegated_in di ON di.account_id = bb.registered_voter_id
            LEFT JOIN latest_votes lv ON lv.account_id = bb.registered_voter_id
            LEFT JOIN participation pt ON pt.account_id = bb.registered_voter_id
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_config_history AS
            WITH config_actions AS (
                SELECT
                    ra.id,
                    ra.receipt_id,
                    ra.execution_outcome_id,
                    ra.transaction_hash,
                    ra.predecessor_id AS predecessor_account_id,
                    ra.receiver_id AS contract_id,
                    ra.method_name,
                    ra.args_json,
                    ra.block_height,
                    ra.block_timestamp
                FROM near.near_receipt_actions ra
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                WHERE ra.method_name IN ('new', 'set_guardians', 'set_reviewer_ids')
                  AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                  AND eo.status IN ('SuccessReceiptId', 'SuccessValue')
            ),
            expanded AS (
                SELECT
                    id,
                    receipt_id,
                    execution_outcome_id,
                    transaction_hash,
                    predecessor_account_id,
                    contract_id,
                    method_name,
                    block_height,
                    block_timestamp,
                    'guardians'::TEXT AS config_field,
                    CASE
                        WHEN method_name = 'new' THEN args_json -> 'config' -> 'guardians'
                        ELSE args_json -> 'guardians'
                    END AS config_value,
                    CASE WHEN method_name = 'new' THEN 'init_new' ELSE 'set_method' END AS config_source,
                    0::INTEGER AS field_order
                FROM config_actions
                WHERE method_name IN ('new', 'set_guardians')

                UNION ALL

                SELECT
                    id,
                    receipt_id,
                    execution_outcome_id,
                    transaction_hash,
                    predecessor_account_id,
                    contract_id,
                    method_name,
                    block_height,
                    block_timestamp,
                    'reviewer_ids'::TEXT AS config_field,
                    CASE
                        WHEN method_name = 'new' THEN args_json -> 'config' -> 'reviewer_ids'
                        ELSE args_json -> 'reviewer_ids'
                    END AS config_value,
                    CASE WHEN method_name = 'new' THEN 'init_new' ELSE 'set_method' END AS config_source,
                    1::INTEGER AS field_order
                FROM config_actions
                WHERE method_name IN ('new', 'set_reviewer_ids')
            )
            SELECT
                ROW_NUMBER() OVER (ORDER BY block_height ASC, id ASC, field_order ASC)::BIGINT AS id,
                contract_id,
                method_name,
                predecessor_account_id,
                transaction_hash,
                receipt_id,
                execution_outcome_id,
                block_height,
                block_timestamp,
                config_field,
                config_value::JSONB,
                config_source,
                field_order
            FROM expanded
            WHERE jsonb_typeof(config_value::JSONB) = 'array'
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE MATERIALIZED VIEW near.hos_proposal_non_voters AS
            SELECT
                md5(p.proposal_id::TEXT || '_' || rv.registered_voter_id) AS id,
                p.proposal_id,
                rv.registered_voter_id
            FROM near.hos_proposals p
            CROSS JOIN near.hos_registered_voters rv
            WHERE NOT EXISTS (
                SELECT 1
                FROM near.hos_proposal_voting_history v
                WHERE v.proposal_id = p.proposal_id
                  AND v.voter_id = rv.registered_voter_id
            )
            "#,
        )
        .await?;

        let mview_indexes = [
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_hos_function_calls_action_id ON near.hos_function_calls (action_id)",
            "CREATE INDEX IF NOT EXISTS ix_hos_function_calls_receiver_method_height ON near.hos_function_calls (receiver_id, method_name, block_height)",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_hos_event_logs_receipt_log ON near.hos_event_logs (receipt_id, log_index)",
            "CREATE INDEX IF NOT EXISTS ix_hos_event_logs_contract_event_height ON near.hos_event_logs (contract_id, event_type, block_height)",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_hos_proposals_proposal_id ON near.hos_proposals (proposal_id)",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_hos_approved_proposals_receipt_id ON near.hos_approved_proposals (receipt_id)",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_hos_proposal_voting_history_receipt_id ON near.hos_proposal_voting_history (receipt_id)",
            "CREATE INDEX IF NOT EXISTS ix_hos_proposal_voting_history_proposal_voter ON near.hos_proposal_voting_history (proposal_id, voter_id)",
            "CREATE INDEX IF NOT EXISTS ix_hos_delegation_events_delegator_height ON near.hos_delegation_events (delegator_account_id, block_height)",
            "CREATE INDEX IF NOT EXISTS ix_hos_delegation_events_delegatee_height ON near.hos_delegation_events (delegatee_account_id, block_height)",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_hos_registered_voters_account ON near.hos_registered_voters (registered_voter_id)",
            "CREATE INDEX IF NOT EXISTS ix_hos_config_history_contract_field_height ON near.hos_config_history (contract_id, config_field, block_height)",
            "CREATE INDEX IF NOT EXISTS ix_hos_user_activities_account_height ON near.hos_user_activities (account_id, block_height)",
            "CREATE INDEX IF NOT EXISTS ix_hos_proposal_non_voters_proposal ON near.hos_proposal_non_voters (proposal_id)",
        ];

        for statement in mview_indexes {
            db.execute_unprepared(statement).await?;
        }

        db.execute_unprepared(
            "CREATE OR REPLACE FUNCTION near.refresh_hos_materialized_views()
             RETURNS void
             LANGUAGE plpgsql
             AS $$
             BEGIN
               REFRESH MATERIALIZED VIEW near.hos_function_calls;
               REFRESH MATERIALIZED VIEW near.hos_event_logs;
               REFRESH MATERIALIZED VIEW near.hos_proposal_voting_history;
               REFRESH MATERIALIZED VIEW near.hos_approved_proposals;
               REFRESH MATERIALIZED VIEW near.hos_delegation_events;
               REFRESH MATERIALIZED VIEW near.hos_user_activities;
               REFRESH MATERIALIZED VIEW near.hos_proposals;
               REFRESH MATERIALIZED VIEW near.hos_registered_voters;
               REFRESH MATERIALIZED VIEW near.hos_config_history;
               REFRESH MATERIALIZED VIEW near.hos_proposal_non_voters;
             END;
             $$",
        )
        .await?;

        db.execute_unprepared("SELECT near.refresh_hos_materialized_views()")
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let statements = [
            "DROP FUNCTION IF EXISTS near.refresh_hos_materialized_views()",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_event_logs CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_function_calls CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposal_non_voters CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_config_history CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_registered_voters CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_user_activities CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposals CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_delegation_events CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_approved_proposals CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposal_voting_history CASCADE",
            "DROP VIEW IF EXISTS near.hos_execution_outcomes CASCADE",
            "DROP VIEW IF EXISTS near.hos_receipt_actions CASCADE",
            "DROP TABLE IF EXISTS near.hos_receipt_edges CASCADE",
            "DROP TABLE IF EXISTS near.hos_receipts CASCADE",
            "DROP TABLE IF EXISTS near.hos_relevant_receipts CASCADE",
            "DROP TABLE IF EXISTS near.hos_execution_outcomes CASCADE",
            "DROP TABLE IF EXISTS near.hos_receipt_actions CASCADE",
            "DROP TABLE IF EXISTS near.near_receipt_edges CASCADE",
            "DROP TABLE IF EXISTS near.near_receipt_actions CASCADE",
            "DROP TABLE IF EXISTS near.venear_governance_relevant_receipts CASCADE",
            "DROP INDEX IF EXISTS near.ix_near_execution_outcomes_transaction_hash_enriched",
            "DROP INDEX IF EXISTS near.ix_near_execution_outcomes_status_height_enriched",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS block_timestamp",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS results_json",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS logs",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS executed_in_block_hash",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS outcome_receipt_ids",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS status",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS executor_account_id",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS tokens_burnt",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS gas_used",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS gas_burnt",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS chunk_hash",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS block_hash",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS transaction_hash",
            "DROP TABLE IF EXISTS near.hos_config CASCADE",
            "DROP FUNCTION IF EXISTS near.is_hos_action(TEXT, TEXT, TEXT)",
            "DROP FUNCTION IF EXISTS near.is_hos_contract(TEXT)",
            "DROP FUNCTION IF EXISTS near.safe_json_parse(TEXT)",
        ];

        for statement in statements {
            db.execute_unprepared(statement).await?;
        }

        Ok(())
    }
}
