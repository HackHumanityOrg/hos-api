use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let drop_views = [
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposal_non_voters CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_registered_voters CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_proposals CASCADE",
            "DROP MATERIALIZED VIEW IF EXISTS near.hos_approved_proposals CASCADE",
        ];

        for statement in drop_views {
            db.execute_unprepared(statement).await?;
        }

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
                    CASE
                        WHEN COALESCE(ra.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
                            THEN (ra.args_json ->> 'proposal_id')::NUMERIC
                    END AS proposal_id,
                    ra.signer_account_id AS approver_id,
                    TO_TIMESTAMP(ra.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC' AS proposal_approved_at,
                    ra.block_height,
                    ra.block_timestamp,
                    ra.receipt_id,
                    ra.transaction_hash,
                    ra.execution_outcome_id,
                    eo.results_json AS approve_results_json
                FROM near.near_receipt_actions ra
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                WHERE ra.method_name = 'approve_proposal'
                  AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                  AND eo.status = 'SuccessReceiptId'
                ORDER BY proposal_id, ra.block_timestamp DESC, ra.receipt_id DESC
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
                ap.proposal_approved_at AS approved_at,
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
                ) AS voting_options,
                ap.receipt_id AS approval_receipt_id,
                ap.transaction_hash AS approval_transaction_hash,
                ap.execution_outcome_id AS approval_execution_outcome_id,
                ap.block_height AS approval_block_height,
                ap.block_timestamp AS approval_block_timestamp
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
                    block_timestamp AS latest_lockup_update_ts,
                    TRUE AS has_locked_unlocked_near
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
                    COALESCE(lu.latest_lockup_update_ts, fr.block_timestamp) AS balance_updated_ts,
                    COALESCE(lu.has_locked_unlocked_near, FALSE) AS has_locked_unlocked_near
                FROM first_registration fr
                LEFT JOIN first_storage fs ON fs.account_id = fr.registered_voter_id
                LEFT JOIN latest_lockup_update lu ON lu.account_id = fr.registered_voter_id
            ),
            venear_contract_growth_config AS (
                SELECT DISTINCT ON (ra.receiver_id)
                    ra.receiver_id AS hos_contract_address,
                    NULLIF(
                        ra.args_json -> 'venear_growth_config' -> 'annual_growth_rate_ns' ->> 'numerator',
                        ''
                    )::NUMERIC AS growth_rate_numerator_ns,
                    NULLIF(
                        ra.args_json -> 'venear_growth_config' -> 'annual_growth_rate_ns' ->> 'denominator',
                        ''
                    )::NUMERIC AS growth_rate_denominator_ns
                FROM near.near_receipt_actions ra
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND near.is_hos_contract(eo.executor_account_id) = TRUE
                WHERE ra.action_kind = 'FunctionCall'
                  AND ra.method_name = 'new'
                  AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                  AND eo.status IN ('SuccessReceiptId', 'SuccessValue')
                ORDER BY ra.receiver_id, ra.block_timestamp ASC, ra.receipt_id ASC
            ),
            voting_power_from_rewards AS (
                SELECT
                    bb.*,
                    CASE
                        WHEN gc.growth_rate_numerator_ns IS NULL
                          OR gc.growth_rate_denominator_ns IS NULL
                          OR gc.growth_rate_denominator_ns = 0
                            THEN 0::NUMERIC
                        ELSE
                            (FLOOR(bb.principal_balance / 1e21) * 1e21)
                            * (gc.growth_rate_numerator_ns / gc.growth_rate_denominator_ns)
                            * (
                                (FLOOR(((EXTRACT(EPOCH FROM NOW()) * 1e9)::NUMERIC) / 1e9) * 1e9)
                                - (
                                    FLOOR(
                                        (
                                            CASE
                                                WHEN bb.has_locked_unlocked_near
                                                    THEN bb.balance_updated_ts::NUMERIC
                                                ELSE bb.block_timestamp::NUMERIC
                                            END
                                        ) / 1e9
                                    ) * 1e9
                                )
                            )
                    END AS extra_venear_on_principal
                FROM base_balances bb
                LEFT JOIN venear_contract_growth_config gc
                    ON gc.hos_contract_address = bb.hos_contract_address
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
                    COALESCE(SUM(vpr.principal_balance), 0)::NUMERIC AS delegated_voting_power,
                    COALESCE(SUM(vpr.extra_venear_on_principal), 0)::NUMERIC AS delegated_extra_venear,
                    MAX(ad.block_timestamp)::BIGINT AS latest_inbound_ts
                FROM active_delegations ad
                LEFT JOIN voting_power_from_rewards vpr
                    ON vpr.registered_voter_id = ad.delegator_account_id
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
                FROM near.hos_proposals
                WHERE is_approved = TRUE
                ORDER BY approved_at DESC NULLS LAST
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
                vpr.id,
                vpr.receipt_id,
                DATE(TO_TIMESTAMP(vpr.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC') AS registered_date,
                TO_TIMESTAMP(vpr.block_timestamp::DOUBLE PRECISION / 1e9) AT TIME ZONE 'UTC' AS registered_at,
                vpr.registered_voter_id,
                vpr.hos_contract_address,
                vpr.has_locked_unlocked_near,
                ad.delegator_account_id IS NOT NULL AS is_actively_delegating,
                ad.delegatee_account_id AS delegatee_id,
                COALESCE(pt.proposal_participation_rate, 0) AS proposal_participation_rate,
                vpr.voting_power_from_locks_unlocks,
                vpr.initial_voting_power,
                vpr.principal_balance,
                COALESCE(vpr.extra_venear_on_principal, 0) AS extra_venear_on_principal,
                COALESCE(di.delegated_voting_power, 0) AS voting_power_from_delegations,
                COALESCE(di.delegated_extra_venear, 0) AS delegated_extra_venear,
                CASE
                    WHEN ad.delegator_account_id IS NOT NULL THEN 0::NUMERIC
                    ELSE
                        COALESCE(vpr.principal_balance, 0)
                        + COALESCE(vpr.extra_venear_on_principal, 0)
                        + COALESCE(di.delegated_voting_power, 0)
                        + COALESCE(di.delegated_extra_venear, 0)
                END AS current_voting_power,
                vpr.block_height,
                vpr.block_hash,
                vpr.block_timestamp,
                (
                    COALESCE(vpr.principal_balance, 0)
                    + COALESCE(vpr.extra_venear_on_principal, 0)
                )::TEXT AS own_voting_power,
                (
                    COALESCE(di.delegated_voting_power, 0)
                    + COALESCE(di.delegated_extra_venear, 0)
                )::TEXT AS delegated_voting_power,
                (
                    CASE
                        WHEN ad.delegator_account_id IS NOT NULL THEN 0::NUMERIC
                        ELSE
                            COALESCE(vpr.principal_balance, 0)
                            + COALESCE(vpr.extra_venear_on_principal, 0)
                            + COALESCE(di.delegated_voting_power, 0)
                            + COALESCE(di.delegated_extra_venear, 0)
                    END
                )::TEXT AS total_voting_power,
                COALESCE(di.delegators_count, 0)::BIGINT AS delegators_count,
                ad.delegatee_account_id AS delegated_to_account_id,
                di.account_id IS NOT NULL AS has_inbound_delegation,
                ad.delegator_account_id IS NOT NULL AS has_outbound_delegation,
                lv.last_vote_ts,
                GREATEST(
                    vpr.balance_updated_ts,
                    COALESCE(di.latest_inbound_ts, 0),
                    COALESCE(ad.block_timestamp, 0),
                    COALESCE(lv.last_vote_ts, 0)
                )::BIGINT AS updated_ts
            FROM voting_power_from_rewards vpr
            LEFT JOIN active_delegations ad ON ad.delegator_account_id = vpr.registered_voter_id
            LEFT JOIN delegated_in di ON di.account_id = vpr.registered_voter_id
            LEFT JOIN latest_votes lv ON lv.account_id = vpr.registered_voter_id
            LEFT JOIN participation pt ON pt.account_id = vpr.registered_voter_id
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

        let recreate_indexes = [
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_hos_proposals_proposal_id ON near.hos_proposals (proposal_id)",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_hos_registered_voters_account ON near.hos_registered_voters (registered_voter_id)",
            "CREATE INDEX IF NOT EXISTS ix_hos_proposal_non_voters_proposal ON near.hos_proposal_non_voters (proposal_id)",
        ];

        for statement in recreate_indexes {
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

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
