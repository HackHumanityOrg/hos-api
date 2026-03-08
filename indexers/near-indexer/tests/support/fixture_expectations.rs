use anyhow::{anyhow, bail, Context, Result};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::fixture_catalog::{expectation_root, FixtureCase, FixtureKind, ModuleExpectationKind};

const FIXTURE_EXPECTATION_VERSION: u32 = 1;
const JSON_DIGEST_THRESHOLD_BYTES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableExpectation {
    pub rows: Vec<BTreeMap<String, JsonValue>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FixtureExpectationFile {
    pub version: u32,
    pub fixture_id: String,
    pub fixture_kind: String,
    pub module_kind: String,
    pub canonical: BTreeMap<String, TableExpectation>,
    pub materialized_views: BTreeMap<String, TableExpectation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureSnapshot {
    pub canonical: BTreeMap<String, TableExpectation>,
    pub materialized_views: BTreeMap<String, TableExpectation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationCategory {
    Canonical,
    MaterializedView,
}

#[derive(Debug, Clone, Copy)]
pub struct RelationSpec {
    pub name: &'static str,
    category: RelationCategory,
    pub order_by: &'static str,
}

const CANONICAL_RELATION_SPECS: &[RelationSpec] = &[
    RelationSpec {
        name: "near_blocks",
        category: RelationCategory::Canonical,
        order_by: "height",
    },
    RelationSpec {
        name: "near_chunks",
        category: RelationCategory::Canonical,
        order_by: "block_height, shard_id, chunk_hash",
    },
    RelationSpec {
        name: "near_state_changes",
        category: RelationCategory::Canonical,
        order_by: "block_height, shard_id",
    },
    RelationSpec {
        name: "near_transactions",
        category: RelationCategory::Canonical,
        order_by: "block_height, shard_id, hash",
    },
    RelationSpec {
        name: "near_receipts",
        category: RelationCategory::Canonical,
        order_by: "block_height, shard_id, receipt_id",
    },
    RelationSpec {
        name: "near_execution_outcomes",
        category: RelationCategory::Canonical,
        order_by: "block_height, shard_id, id",
    },
    RelationSpec {
        name: "near_receipt_actions",
        category: RelationCategory::Canonical,
        order_by: "block_height, receipt_id, action_index, id",
    },
    RelationSpec {
        name: "near_receipt_edges",
        category: RelationCategory::Canonical,
        order_by: "parent_receipt_id, child_receipt_id",
    },
    RelationSpec {
        name: "near_receipt_tx_hash_map",
        category: RelationCategory::Canonical,
        order_by: "receipt_id",
    },
];

const MATERIALIZED_VIEW_SPECS: &[RelationSpec] = &[
    RelationSpec {
        name: "hos_function_calls",
        category: RelationCategory::MaterializedView,
        order_by: "block_height, block_timestamp, action_id",
    },
    RelationSpec {
        name: "hos_event_logs",
        category: RelationCategory::MaterializedView,
        order_by: "block_height, receipt_id, log_index",
    },
    RelationSpec {
        name: "hos_proposal_voting_history",
        category: RelationCategory::MaterializedView,
        order_by: "proposal_id, voter_id, block_height, receipt_id",
    },
    RelationSpec {
        name: "hos_delegation_events",
        category: RelationCategory::MaterializedView,
        order_by: "block_height, receipt_id, id",
    },
    RelationSpec {
        name: "hos_user_activities",
        category: RelationCategory::MaterializedView,
        order_by: "block_height, receipt_id, id",
    },
    RelationSpec {
        name: "hos_proposals",
        category: RelationCategory::MaterializedView,
        order_by: "proposal_id",
    },
    RelationSpec {
        name: "hos_registered_voters",
        category: RelationCategory::MaterializedView,
        order_by: "registered_voter_id",
    },
    RelationSpec {
        name: "hos_config_history",
        category: RelationCategory::MaterializedView,
        order_by: "block_height, receipt_id, config_field",
    },
    RelationSpec {
        name: "hos_proposal_non_voters",
        category: RelationCategory::MaterializedView,
        order_by: "proposal_id, registered_voter_id",
    },
];

const RESET_TABLES: &[&str] = &[
    "near.near_blocks",
    "near.near_chunks",
    "near.near_state_changes",
    "near.near_transactions",
    "near.near_receipts",
    "near.near_execution_outcomes",
    "near.near_receipt_actions",
    "near.near_receipt_edges",
    "near.near_receipt_tx_hash_map",
    "near.near_sync_cursors",
];

fn relation_specs() -> impl Iterator<Item = &'static RelationSpec> {
    CANONICAL_RELATION_SPECS
        .iter()
        .chain(MATERIALIZED_VIEW_SPECS.iter())
}

fn digest_value(value: &JsonValue) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

fn should_digest_json_value(value: &JsonValue) -> bool {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() > JSON_DIGEST_THRESHOLD_BYTES)
        .unwrap_or(true)
}

fn replace_value_with_digest(
    row: &mut BTreeMap<String, JsonValue>,
    field_name: &str,
    digest_field_name: &str,
) {
    let value = row.remove(field_name).unwrap_or(JsonValue::Null);
    let digest_value = if value.is_null() {
        JsonValue::Null
    } else {
        JsonValue::String(digest_value(&value))
    };
    row.insert(digest_field_name.to_string(), digest_value);
}

fn maybe_digest_json_value(
    row: &mut BTreeMap<String, JsonValue>,
    field_name: &str,
    digest_field_name: &str,
) {
    let Some(value) = row.get(field_name).cloned() else {
        return;
    };
    if value.is_null() || !should_digest_json_value(&value) {
        return;
    }

    row.remove(field_name);
    row.insert(
        digest_field_name.to_string(),
        JsonValue::String(digest_value(&value)),
    );
}

fn normalize_relation_row(
    relation_name: &str,
    row: BTreeMap<String, JsonValue>,
) -> BTreeMap<String, JsonValue> {
    let mut row = row;

    row.remove("created_at");
    row.remove("updated_at");

    match relation_name {
        "near_blocks" => replace_value_with_digest(&mut row, "raw_data", "raw_data_sha256"),
        "near_chunks" => replace_value_with_digest(&mut row, "raw_data", "raw_data_sha256"),
        "near_transactions" => {
            replace_value_with_digest(&mut row, "actions", "actions_sha256");
            replace_value_with_digest(&mut row, "raw_data", "raw_data_sha256");
        }
        "near_receipts" => {
            replace_value_with_digest(&mut row, "receipt", "receipt_sha256");
            replace_value_with_digest(&mut row, "raw_data", "raw_data_sha256");
        }
        "near_execution_outcomes" => {
            replace_value_with_digest(&mut row, "outcome", "outcome_sha256");
            replace_value_with_digest(&mut row, "raw_data", "raw_data_sha256");
            maybe_digest_json_value(&mut row, "logs", "logs_sha256");
            maybe_digest_json_value(&mut row, "results_json", "results_json_sha256");
            maybe_digest_json_value(
                &mut row,
                "outcome_receipt_ids",
                "outcome_receipt_ids_sha256",
            );
        }
        "near_state_changes" => {
            row.remove("id");
            replace_value_with_digest(&mut row, "state_changes", "state_changes_sha256");
        }
        "near_receipt_actions" => {
            replace_value_with_digest(&mut row, "action_json", "action_json_sha256");
            maybe_digest_json_value(&mut row, "args_json", "args_json_sha256");
        }
        _ => {}
    }

    row
}

fn value_to_btreemap(value: JsonValue, relation_name: &str) -> Result<BTreeMap<String, JsonValue>> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("expected JSON object row for relation {}", relation_name))?;

    let mut out = BTreeMap::new();
    for (key, value) in object {
        out.insert(key.clone(), value.clone());
    }

    Ok(out)
}

fn json_values_equivalent(lhs: &JsonValue, rhs: &JsonValue) -> bool {
    match (lhs, rhs) {
        (JsonValue::Number(left), JsonValue::Number(right)) => {
            if left == right {
                return true;
            }

            match (left.as_i64(), right.as_i64()) {
                (Some(left), Some(right)) => return left == right,
                _ => {}
            }

            match (left.as_u64(), right.as_u64()) {
                (Some(left), Some(right)) => return left == right,
                _ => {}
            }

            match (left.as_f64(), right.as_f64()) {
                (Some(left), Some(right)) if left.is_finite() && right.is_finite() => {
                    let scale = left.abs().max(right.abs()).max(1.0);
                    return (left - right).abs() <= scale * 1e-12;
                }
                _ => {}
            }

            false
        }
        (JsonValue::Array(left), JsonValue::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left_value, right_value)| {
                        json_values_equivalent(left_value, right_value)
                    })
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

async fn relation_rows(
    db: &impl ConnectionTrait,
    relation_spec: &RelationSpec,
) -> Result<Vec<BTreeMap<String, JsonValue>>> {
    let sql = format!(
        "SELECT to_jsonb(t) AS value FROM near.{} t ORDER BY {}",
        relation_spec.name, relation_spec.order_by
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            vec![],
        ))
        .await
        .with_context(|| {
            format!(
                "failed reading relation snapshot for {}",
                relation_spec.name
            )
        })?;

    rows.into_iter()
        .map(|row| {
            let value: JsonValue = row.try_get("", "value").with_context(|| {
                format!("failed decoding row for relation {}", relation_spec.name)
            })?;
            let row_map = value_to_btreemap(value, relation_spec.name)?;
            Ok(normalize_relation_row(relation_spec.name, row_map))
        })
        .collect()
}

pub async fn capture_relation_snapshot(
    db: &impl ConnectionTrait,
    relation_spec: &RelationSpec,
) -> Result<TableExpectation> {
    Ok(TableExpectation {
        rows: relation_rows(db, relation_spec).await?,
    })
}

pub async fn capture_fixture_snapshot(db: &impl ConnectionTrait) -> Result<FixtureSnapshot> {
    let mut canonical = BTreeMap::new();
    for relation_spec in CANONICAL_RELATION_SPECS {
        canonical.insert(
            relation_spec.name.to_string(),
            capture_relation_snapshot(db, relation_spec).await?,
        );
    }

    let mut materialized_views = BTreeMap::new();
    for relation_spec in MATERIALIZED_VIEW_SPECS {
        materialized_views.insert(
            relation_spec.name.to_string(),
            capture_relation_snapshot(db, relation_spec).await?,
        );
    }

    Ok(FixtureSnapshot {
        canonical,
        materialized_views,
    })
}

pub async fn capture_fixture_expectation(
    db: &impl ConnectionTrait,
    fixture_case: &FixtureCase,
    module_kind: ModuleExpectationKind,
) -> Result<FixtureExpectationFile> {
    let snapshot = capture_fixture_snapshot(db).await?;

    Ok(FixtureExpectationFile {
        version: FIXTURE_EXPECTATION_VERSION,
        fixture_id: fixture_case.id.clone(),
        fixture_kind: fixture_case.kind.as_str().to_string(),
        module_kind: module_kind.as_str().to_string(),
        canonical: snapshot.canonical,
        materialized_views: snapshot.materialized_views,
    })
}

pub fn expectation_path(fixture_case: &FixtureCase, module_kind: ModuleExpectationKind) -> PathBuf {
    expectation_root()
        .join(module_kind.as_str())
        .join(format!("{}.json", fixture_case.id))
}

pub fn read_fixture_expectation(path: &Path) -> Result<FixtureExpectationFile> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed reading fixture expectation {}", path.display()))?;
    let expectation: FixtureExpectationFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed parsing fixture expectation {}", path.display()))?;

    if expectation.version != FIXTURE_EXPECTATION_VERSION {
        bail!(
            "unexpected fixture expectation version {} in {} (expected {})",
            expectation.version,
            path.display(),
            FIXTURE_EXPECTATION_VERSION
        );
    }

    Ok(expectation)
}

pub fn write_fixture_expectation(path: &Path, expectation: &FixtureExpectationFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed creating fixture expectation directory {}",
                parent.display()
            )
        })?;
    }

    let bytes =
        serde_json::to_vec_pretty(expectation).context("failed serializing fixture expectation")?;
    std::fs::write(path, bytes)
        .with_context(|| format!("failed writing fixture expectation {}", path.display()))?;
    Ok(())
}

fn assert_table_expectation(
    relation_name: &str,
    actual: &TableExpectation,
    expected: &TableExpectation,
) -> Result<()> {
    if actual.rows.len() != expected.rows.len() {
        bail!(
            "relation {} row count mismatch: expected {}, got {}",
            relation_name,
            expected.rows.len(),
            actual.rows.len()
        );
    }

    for (row_index, (actual_row, expected_row)) in
        actual.rows.iter().zip(expected.rows.iter()).enumerate()
    {
        let keys = actual_row
            .keys()
            .chain(expected_row.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        for key in keys {
            let actual_value = actual_row.get(&key).cloned().unwrap_or(JsonValue::Null);
            let expected_value = expected_row.get(&key).cloned().unwrap_or(JsonValue::Null);
            if !json_values_equivalent(&actual_value, &expected_value) {
                bail!(
                    "relation {} row {} field '{}' mismatch: expected {}, got {}",
                    relation_name,
                    row_index,
                    key,
                    expected_value,
                    actual_value
                );
            }
        }
    }

    Ok(())
}

pub fn assert_snapshot_matches(
    actual: &FixtureSnapshot,
    expected: &FixtureExpectationFile,
) -> Result<()> {
    for relation_spec in relation_specs() {
        let relation_name = relation_spec.name;
        let (actual_table, expected_table) = match relation_spec.category {
            RelationCategory::Canonical => (
                actual.canonical.get(relation_name).ok_or_else(|| {
                    anyhow!("missing actual canonical relation {}", relation_name)
                })?,
                expected.canonical.get(relation_name).ok_or_else(|| {
                    anyhow!("missing expected canonical relation {}", relation_name)
                })?,
            ),
            RelationCategory::MaterializedView => (
                actual
                    .materialized_views
                    .get(relation_name)
                    .ok_or_else(|| anyhow!("missing actual materialized view {}", relation_name))?,
                expected
                    .materialized_views
                    .get(relation_name)
                    .ok_or_else(|| {
                        anyhow!("missing expected materialized view {}", relation_name)
                    })?,
            ),
        };

        assert_table_expectation(relation_name, actual_table, expected_table)?;
    }

    Ok(())
}

pub async fn assert_fixture_expectation(
    db: &impl ConnectionTrait,
    fixture_case: &FixtureCase,
    module_kind: ModuleExpectationKind,
) -> Result<()> {
    let expectation = read_fixture_expectation(&expectation_path(fixture_case, module_kind))?;
    let actual = capture_fixture_snapshot(db).await?;

    assert_snapshot_matches(&actual, &expectation).with_context(|| {
        format!(
            "fixture {} ({}) did not match checked-in expectations",
            fixture_case.id,
            module_kind.as_str()
        )
    })?;

    Ok(())
}

pub fn assert_snapshot_equal(
    left: &FixtureSnapshot,
    right: &FixtureSnapshot,
    label: &str,
) -> Result<()> {
    let expected = FixtureExpectationFile {
        version: FIXTURE_EXPECTATION_VERSION,
        fixture_id: label.to_string(),
        fixture_kind: FixtureKind::Range.as_str().to_string(),
        module_kind: "snapshot".to_string(),
        canonical: right.canonical.clone(),
        materialized_views: right.materialized_views.clone(),
    };

    assert_snapshot_matches(left, &expected)
}

pub async fn refresh_hos_materialized_views(db: &impl ConnectionTrait) -> Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT near.refresh_hos_materialized_views()",
        vec![],
    ))
    .await
    .context("failed refreshing HOS materialized views")?;
    Ok(())
}

pub async fn reset_near_indexer_tables(db: &impl ConnectionTrait) -> Result<()> {
    let sql = format!(
        "TRUNCATE TABLE {} RESTART IDENTITY CASCADE",
        RESET_TABLES.join(", ")
    );
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &sql,
        vec![],
    ))
    .await
    .context("failed resetting near-indexer test tables")?;
    Ok(())
}
