use anyhow::{anyhow, bail, Context, Result};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const GOLDEN_FILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HosMvGoldenFixture {
    pub version: u32,
    pub cases: Vec<HosMvGoldenCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HosMvGoldenCase {
    pub case_name: String,
    pub view: String,
    pub lookup: BTreeMap<String, JsonValue>,
    pub expected: BTreeMap<String, JsonValue>,
}

struct GoldenCaseSpec {
    case_name: &'static str,
    view: &'static str,
    selector_sql: &'static str,
    lookup_fields: &'static [&'static str],
    expected_fields: &'static [&'static str],
}

const GOLDEN_CASE_SPECS: &[GoldenCaseSpec] = &[
    GoldenCaseSpec {
        case_name: "hos_function_calls_create_proposal",
        view: "hos_function_calls",
        selector_sql: "FROM near.hos_function_calls t WHERE t.method_name = 'create_proposal' ORDER BY t.block_height ASC, t.block_timestamp ASC, t.action_id ASC LIMIT 1",
        lookup_fields: &["action_id"],
        expected_fields: &[
            "receipt_id",
            "method_name",
            "predecessor_id",
            "receiver_id",
            "signer_account_id",
            "gas",
            "deposit",
            "args_json",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_function_calls_vote",
        view: "hos_function_calls",
        selector_sql: "FROM near.hos_function_calls t WHERE t.method_name = 'vote' ORDER BY t.block_height ASC, t.block_timestamp ASC, t.action_id ASC LIMIT 1",
        lookup_fields: &["action_id"],
        expected_fields: &[
            "receipt_id",
            "method_name",
            "predecessor_id",
            "receiver_id",
            "signer_account_id",
            "args_json",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_event_logs_create_proposal",
        view: "hos_event_logs",
        selector_sql: "FROM near.hos_event_logs t WHERE t.event_type = 'create_proposal' ORDER BY t.block_height ASC, t.receipt_id ASC, t.log_index ASC LIMIT 1",
        lookup_fields: &["receipt_id", "log_index"],
        expected_fields: &[
            "contract_id",
            "event_type",
            "event_standard",
            "event_version",
            "event_data",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_proposal_voting_history_vote",
        view: "hos_proposal_voting_history",
        selector_sql: "FROM near.hos_proposal_voting_history t ORDER BY t.block_height ASC, t.block_timestamp ASC, t.receipt_id ASC LIMIT 1",
        lookup_fields: &["receipt_id"],
        expected_fields: &[
            "proposal_id",
            "voter_id",
            "vote_option",
            "voting_power",
            "near_balance",
            "extra_venear_balance",
            "delegator_account_id",
            "delegated_near_balance",
            "delegated_extra_venear_balance",
            "hos_contract_address",
            "event_type",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_approved_proposals_approval",
        view: "hos_approved_proposals",
        selector_sql: "FROM near.hos_approved_proposals t ORDER BY t.block_height ASC, t.block_timestamp ASC, t.receipt_id ASC LIMIT 1",
        lookup_fields: &["receipt_id"],
        expected_fields: &[
            "proposal_id",
            "proposal_approver_id",
            "hos_contract_address",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_delegation_events_delegate_all",
        view: "hos_delegation_events",
        selector_sql: "FROM near.hos_delegation_events t WHERE t.event_type = 'delegate_all' ORDER BY t.block_height ASC, t.block_timestamp ASC, t.receipt_id ASC LIMIT 1",
        lookup_fields: &["id"],
        expected_fields: &[
            "delegator_account_id",
            "delegatee_account_id",
            "event_type",
            "delegate_event",
            "owner_id",
            "near_amount",
            "success",
            "is_latest_delegator_event",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_user_activities_storage_deposit",
        view: "hos_user_activities",
        selector_sql: "FROM near.hos_user_activities t WHERE t.method_name = 'storage_deposit' ORDER BY t.block_height ASC, t.block_timestamp ASC, t.receipt_id ASC LIMIT 1",
        lookup_fields: &["id"],
        expected_fields: &[
            "account_id",
            "method_name",
            "event_type",
            "event_status",
            "near_amount",
            "voting_power",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_user_activities_on_lockup_update",
        view: "hos_user_activities",
        selector_sql: "FROM near.hos_user_activities t WHERE t.method_name = 'on_lockup_update' ORDER BY t.block_height ASC, t.block_timestamp ASC, t.receipt_id ASC LIMIT 1",
        lookup_fields: &["id"],
        expected_fields: &[
            "account_id",
            "method_name",
            "event_type",
            "event_status",
            "near_amount",
            "locked_near_balance",
            "voting_power",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_proposals_primary",
        view: "hos_proposals",
        selector_sql: "FROM near.hos_proposals t ORDER BY t.proposal_id ASC LIMIT 1",
        lookup_fields: &["proposal_id"],
        expected_fields: &[
            "proposal_title",
            "proposal_description",
            "proposal_url",
            "hos_contract_address",
            "is_approved",
            "is_rejected",
            "has_votes",
            "creator_id",
            "approver_id",
            "num_distinct_voters",
            "num_for_votes",
            "num_against_votes",
            "num_abstain_votes",
            "for_voting_power",
            "against_voting_power",
            "abstain_voting_power",
            "voting_options",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_registered_voters_primary",
        view: "hos_registered_voters",
        selector_sql: "FROM near.hos_registered_voters t ORDER BY t.registered_voter_id ASC LIMIT 1",
        lookup_fields: &["registered_voter_id"],
        expected_fields: &[
            "hos_contract_address",
            "has_locked_unlocked_near",
            "is_actively_delegating",
            "delegatee_id",
            "proposal_participation_rate",
            "voting_power_from_locks_unlocks",
            "initial_voting_power",
            "principal_balance",
            "voting_power_from_delegations",
            "current_voting_power",
            "delegators_count",
            "has_inbound_delegation",
            "has_outbound_delegation",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_config_history_guardians",
        view: "hos_config_history",
        selector_sql: "FROM near.hos_config_history t WHERE t.config_field = 'guardians' ORDER BY t.block_height ASC, t.receipt_id ASC LIMIT 1",
        lookup_fields: &["receipt_id", "config_field"],
        expected_fields: &[
            "contract_id",
            "method_name",
            "predecessor_account_id",
            "config_field",
            "config_value",
            "config_source",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_config_history_reviewer_ids",
        view: "hos_config_history",
        selector_sql: "FROM near.hos_config_history t WHERE t.config_field = 'reviewer_ids' ORDER BY t.block_height ASC, t.receipt_id ASC LIMIT 1",
        lookup_fields: &["receipt_id", "config_field"],
        expected_fields: &[
            "contract_id",
            "method_name",
            "predecessor_account_id",
            "config_field",
            "config_value",
            "config_source",
            "block_height",
            "block_timestamp",
        ],
    },
    GoldenCaseSpec {
        case_name: "hos_proposal_non_voters_first",
        view: "hos_proposal_non_voters",
        selector_sql: "FROM near.hos_proposal_non_voters t ORDER BY t.proposal_id ASC, t.registered_voter_id ASC LIMIT 1",
        lookup_fields: &["proposal_id", "registered_voter_id"],
        expected_fields: &["proposal_id", "registered_voter_id"],
    },
];

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn json_object_expr(alias: &str, fields: &[&str]) -> Result<String> {
    if fields.is_empty() {
        bail!("json object field list cannot be empty");
    }

    let mut pairs = Vec::with_capacity(fields.len());
    for field in fields {
        if !is_safe_identifier(field) {
            bail!("unsafe column identifier: {field}");
        }
        pairs.push(format!("'{field}', {alias}.\"{field}\""));
    }

    Ok(format!("jsonb_build_object({})", pairs.join(", ")))
}

fn json_value_to_map(value: JsonValue, label: &str) -> Result<BTreeMap<String, JsonValue>> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("expected JSON object for {label}"))?;

    let mut map = BTreeMap::new();
    for (key, val) in object {
        map.insert(key.clone(), val.clone());
    }

    Ok(map)
}

fn map_to_json_object(map: &BTreeMap<String, JsonValue>) -> JsonValue {
    let mut out = JsonMap::new();
    for (key, value) in map {
        out.insert(key.clone(), value.clone());
    }
    JsonValue::Object(out)
}

fn json_numbers_equivalent(lhs: &serde_json::Number, rhs: &serde_json::Number) -> bool {
    if lhs == rhs {
        return true;
    }

    if let (Some(left), Some(right)) = (lhs.as_f64(), rhs.as_f64()) {
        let epsilon = left.abs().max(right.abs()).max(1.0) * 1e-12;
        return (left - right).abs() <= epsilon;
    }

    false
}

fn json_values_equivalent(lhs: &JsonValue, rhs: &JsonValue) -> bool {
    match (lhs, rhs) {
        (JsonValue::Number(left), JsonValue::Number(right)) => json_numbers_equivalent(left, right),
        (JsonValue::Array(left), JsonValue::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left_value, right_value)| json_values_equivalent(left_value, right_value))
        }
        (JsonValue::Object(left), JsonValue::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left_value)| {
                    right
                        .get(key)
                        .is_some_and(|right_value| json_values_equivalent(left_value, right_value))
                })
        }
        _ => lhs == rhs,
    }
}

fn build_lookup_where_clause(
    lookup: &BTreeMap<String, JsonValue>,
    alias: &str,
) -> Result<(String, Vec<JsonValue>)> {
    if lookup.is_empty() {
        bail!("lookup cannot be empty");
    }

    let mut conditions = Vec::with_capacity(lookup.len());
    let mut values = Vec::with_capacity(lookup.len());

    for (field, value) in lookup {
        if !is_safe_identifier(field) {
            bail!("unsafe lookup column identifier: {field}");
        }

        let idx = values.len() + 1;
        conditions.push(format!(
            "COALESCE(to_jsonb({alias}.\"{field}\")::text, 'null') = ${idx}"
        ));
        values.push(value.clone());
    }

    Ok((conditions.join(" AND "), values))
}

async fn scalar_json(
    db: &impl ConnectionTrait,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<JsonValue> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            values,
        ))
        .await?
        .ok_or_else(|| anyhow!("expected one row, got none for SQL: {sql}"))?;

    row.try_get("", "value")
        .context("failed to decode JSON scalar from SQL")
}

pub fn expected_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/venear_governance/expectations/hos_mv_golden_rows.json")
}

pub async fn capture_hos_mv_goldens(db: &impl ConnectionTrait) -> Result<HosMvGoldenFixture> {
    let mut cases = Vec::with_capacity(GOLDEN_CASE_SPECS.len());

    for spec in GOLDEN_CASE_SPECS {
        if !is_safe_identifier(spec.view) {
            bail!("unsafe view identifier: {}", spec.view);
        }

        let lookup_expr = json_object_expr("t", spec.lookup_fields)?;
        let expected_expr = json_object_expr("t", spec.expected_fields)?;

        let sql = format!(
            "SELECT {lookup_expr} AS lookup, {expected_expr} AS expected {}",
            spec.selector_sql
        );

        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                vec![],
            ))
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "no deterministic row found for golden case '{}' on view '{}'",
                    spec.case_name,
                    spec.view
                )
            })?;

        let lookup: JsonValue = row
            .try_get("", "lookup")
            .context("failed to decode lookup JSON")?;
        let expected: JsonValue = row
            .try_get("", "expected")
            .context("failed to decode expected JSON")?;

        cases.push(HosMvGoldenCase {
            case_name: spec.case_name.to_string(),
            view: spec.view.to_string(),
            lookup: json_value_to_map(lookup, "lookup")?,
            expected: json_value_to_map(expected, "expected")?,
        });
    }

    Ok(HosMvGoldenFixture {
        version: GOLDEN_FILE_VERSION,
        cases,
    })
}

pub fn read_hos_mv_golden_fixture(path: &Path) -> Result<HosMvGoldenFixture> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read HOS MV fixture {}", path.display()))?;

    let fixture: HosMvGoldenFixture = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse HOS MV fixture {}", path.display()))?;

    if fixture.version != GOLDEN_FILE_VERSION {
        bail!(
            "unexpected HOS MV fixture version {} (expected {})",
            fixture.version,
            GOLDEN_FILE_VERSION
        );
    }

    Ok(fixture)
}

pub fn write_hos_mv_golden_fixture(path: &Path, fixture: &HosMvGoldenFixture) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create fixture dir {}", parent.display()))?;
    }

    let data = serde_json::to_vec_pretty(fixture).context("failed to serialize golden fixture")?;
    std::fs::write(path, data)
        .with_context(|| format!("failed to write HOS MV fixture {}", path.display()))?;

    Ok(())
}

pub async fn assert_hos_mv_golden_rows(db: &impl ConnectionTrait) -> Result<()> {
    let fixture_path = expected_fixture_path();
    let fixture = read_hos_mv_golden_fixture(&fixture_path)?;

    for case in &fixture.cases {
        if !is_safe_identifier(&case.view) {
            bail!("unsafe view identifier in fixture: {}", case.view);
        }

        let expected_fields = case.expected.keys().map(String::as_str).collect::<Vec<_>>();
        let expected_expr = json_object_expr("t", &expected_fields)?;
        let (where_clause, lookup_values) = build_lookup_where_clause(&case.lookup, "t")?;

        let sql = format!(
            "SELECT {expected_expr} AS value FROM near.{} t WHERE {} LIMIT 1",
            case.view, where_clause
        );

        let values = lookup_values
            .into_iter()
            .map(|value| {
                serde_json::to_string(&value)
                    .unwrap_or_else(|_| "null".to_string())
                    .into()
            })
            .collect::<Vec<sea_orm::Value>>();

        let actual_json = scalar_json(db, &sql, values)
            .await
            .with_context(|| format!("case '{}': failed to read actual row", case.case_name))?;

        let actual_map = json_value_to_map(actual_json, "actual")?;

        for (field, expected_value) in &case.expected {
            let actual_value = actual_map.get(field).cloned().unwrap_or(JsonValue::Null);
            if !json_values_equivalent(&actual_value, expected_value) {
                bail!(
                    "golden mismatch in case '{}' view '{}' field '{}': expected {}, got {} (lookup={})",
                    case.case_name,
                    case.view,
                    field,
                    expected_value,
                    actual_value,
                    map_to_json_object(&case.lookup)
                );
            }
        }
    }

    Ok(())
}

async fn scalar_i64(db: &impl ConnectionTrait, sql: &str) -> Result<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![],
        ))
        .await?
        .ok_or_else(|| anyhow!("expected one row, got none for SQL: {sql}"))?;

    row.try_get("", "value")
        .context("failed to decode i64 scalar from SQL")
}

pub async fn assert_hos_mv_normalized_semantics(db: &impl ConnectionTrait) -> Result<()> {
    let vote_option_mismatches = scalar_i64(
        db,
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_proposal_voting_history pvh
         INNER JOIN near.near_receipt_actions ra
           ON ra.receipt_id = pvh.receipt_id
         WHERE COALESCE(ra.args_json ->> 'vote', '') ~ '^[0-9]+$'
           AND pvh.vote_option IS DISTINCT FROM (ra.args_json ->> 'vote')::NUMERIC",
    )
    .await?;
    if vote_option_mismatches != 0 {
        bail!(
            "hos_proposal_voting_history vote_option normalization mismatch count: {}",
            vote_option_mismatches
        );
    }

    let vote_proposal_id_mismatches = scalar_i64(
        db,
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_proposal_voting_history pvh
         INNER JOIN near.near_receipt_actions ra
           ON ra.receipt_id = pvh.receipt_id
         WHERE COALESCE(ra.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
           AND pvh.proposal_id IS DISTINCT FROM (ra.args_json ->> 'proposal_id')::NUMERIC",
    )
    .await?;
    if vote_proposal_id_mismatches != 0 {
        bail!(
            "hos_proposal_voting_history proposal_id normalization mismatch count: {}",
            vote_proposal_id_mismatches
        );
    }

    let approval_proposal_id_mismatches = scalar_i64(
        db,
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_approved_proposals ap
         INNER JOIN near.near_receipt_actions ra
           ON ra.receipt_id = ap.receipt_id
         WHERE COALESCE(ra.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
           AND ap.proposal_id IS DISTINCT FROM (ra.args_json ->> 'proposal_id')::NUMERIC",
    )
    .await?;
    if approval_proposal_id_mismatches != 0 {
        bail!(
            "hos_approved_proposals proposal_id normalization mismatch count: {}",
            approval_proposal_id_mismatches
        );
    }

    let latest_delegator_rows_mismatches = scalar_i64(
        db,
        "SELECT COUNT(*)::bigint AS value
         FROM (
             SELECT delegator_account_id
             FROM near.hos_delegation_events
             WHERE delegator_account_id IS NOT NULL
               AND is_latest_delegator_event = TRUE
             GROUP BY delegator_account_id
             HAVING COUNT(*) <> 1
         ) mismatches",
    )
    .await?;
    if latest_delegator_rows_mismatches != 0 {
        bail!(
            "hos_delegation_events latest-row invariant mismatch count: {}",
            latest_delegator_rows_mismatches
        );
    }

    let proposal_aggregate_mismatches = scalar_i64(
        db,
        "WITH recomputed AS (
             SELECT
                 proposal_id,
                 COUNT(*)::BIGINT AS num_distinct_voters,
                 SUM(CASE WHEN vote_option = 0 THEN 1 ELSE 0 END)::BIGINT AS num_for_votes,
                 SUM(CASE WHEN vote_option = 1 THEN 1 ELSE 0 END)::BIGINT AS num_against_votes,
                 SUM(CASE WHEN vote_option = 2 THEN 1 ELSE 0 END)::BIGINT AS num_abstain_votes,
                 COALESCE(SUM(CASE WHEN vote_option = 0 THEN voting_power ELSE 0 END), 0)::NUMERIC AS for_voting_power,
                 COALESCE(SUM(CASE WHEN vote_option = 1 THEN voting_power ELSE 0 END), 0)::NUMERIC AS against_voting_power,
                 COALESCE(SUM(CASE WHEN vote_option = 2 THEN voting_power ELSE 0 END), 0)::NUMERIC AS abstain_voting_power
             FROM near.hos_proposal_voting_history
             GROUP BY proposal_id
         )
         SELECT COUNT(*)::BIGINT AS value
         FROM near.hos_proposals p
         LEFT JOIN recomputed r
           ON r.proposal_id = p.proposal_id
         WHERE p.num_distinct_voters IS DISTINCT FROM COALESCE(r.num_distinct_voters, 0)
            OR p.num_for_votes IS DISTINCT FROM COALESCE(r.num_for_votes, 0)
            OR p.num_against_votes IS DISTINCT FROM COALESCE(r.num_against_votes, 0)
            OR p.num_abstain_votes IS DISTINCT FROM COALESCE(r.num_abstain_votes, 0)
            OR p.for_voting_power IS DISTINCT FROM COALESCE(r.for_voting_power, 0)
            OR p.against_voting_power IS DISTINCT FROM COALESCE(r.against_voting_power, 0)
            OR p.abstain_voting_power IS DISTINCT FROM COALESCE(r.abstain_voting_power, 0)",
    )
    .await?;
    if proposal_aggregate_mismatches != 0 {
        bail!(
            "hos_proposals aggregate mismatch count: {}",
            proposal_aggregate_mismatches
        );
    }

    let voter_power_mismatches = scalar_i64(
        db,
        "SELECT COUNT(*)::BIGINT AS value
         FROM near.hos_registered_voters rv
         WHERE rv.current_voting_power IS DISTINCT FROM
             CASE
                 WHEN rv.has_outbound_delegation THEN COALESCE(rv.voting_power_from_delegations, 0)
                 ELSE COALESCE(rv.principal_balance, 0) + COALESCE(rv.voting_power_from_delegations, 0)
             END
            OR rv.own_voting_power IS DISTINCT FROM rv.principal_balance::TEXT
            OR rv.delegated_voting_power IS DISTINCT FROM COALESCE(rv.voting_power_from_delegations, 0)::TEXT
            OR rv.total_voting_power IS DISTINCT FROM rv.current_voting_power::TEXT",
    )
    .await?;
    if voter_power_mismatches != 0 {
        bail!(
            "hos_registered_voters voting power normalization mismatch count: {}",
            voter_power_mismatches
        );
    }

    let config_history_mismatches = scalar_i64(
        db,
        "SELECT COUNT(*)::BIGINT AS value
         FROM near.hos_config_history ch
         WHERE jsonb_typeof(ch.config_value) <> 'array'
            OR (ch.method_name = 'set_guardians' AND ch.config_field <> 'guardians')
            OR (ch.method_name = 'set_reviewer_ids' AND ch.config_field <> 'reviewer_ids')
            OR (ch.method_name = 'new' AND ch.config_source <> 'init_new')",
    )
    .await?;
    if config_history_mismatches != 0 {
        bail!(
            "hos_config_history normalization mismatch count: {}",
            config_history_mismatches
        );
    }

    Ok(())
}
