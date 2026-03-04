//! Fixture fetcher for veNEAR governance integration tests.
//!
//! Streams fixed scenario ranges from mainnet FastNEAR and stores each range as a
//! JSON array (`Vec<StreamerMessage>`). Scenarios are intentionally bounded to
//! provide meaningful multi-block integration fixtures without unbounded growth.
//!
//! Output default:
//! `tests/fixtures/venear_governance/ranges/<scenario>.json`
//!
//! Optional environment variables:
//! - `FASTNEAR_API_KEY`: FastNEAR bearer token.
//! - `SCENARIOS`: comma-separated scenario names to fetch.
//! - `OVERWRITE_EXISTING`: `true`/`false`, default `false`.
//! - `FIXTURES_DIR`: custom output directory.

use anyhow::{anyhow, bail, Context, Result};
use near_indexer::core::lake_stream::{create_fastnear_stream, LakeStreamConfig};
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tracing::info;

const EVENT_JSON_PREFIX: &str = "EVENT_JSON:";
const LOCKUP_SUFFIX: &str = ".venear.dao";
const VENEAR_CONTRACT: &str = "venear.dao";
const VOTING_CONTRACT: &str = "vote.dao";

#[derive(Debug, Clone, Copy)]
struct ScenarioSpec {
    name: &'static str,
    start_height: u64,
    block_count: u64,
    required_event_types: &'static [&'static str],
}

const DEFAULT_SCENARIOS: &[ScenarioSpec] = &[
    ScenarioSpec {
        name: "hos_lockup_core_167957",
        start_height: 167_957_448,
        block_count: 40,
        required_event_types: &["ft_mint", "lockup_deployed", "lockup_update"],
    },
    ScenarioSpec {
        name: "hos_create_proposal_167957",
        start_height: 167_957_500,
        block_count: 40,
        required_event_types: &["create_proposal"],
    },
    ScenarioSpec {
        name: "hos_approval_snapshot_168368",
        start_height: 168_368_820,
        block_count: 40,
        required_event_types: &["proposal_approve", "on_get_snapshot"],
    },
    ScenarioSpec {
        name: "hos_vote_168369",
        start_height: 168_369_140,
        block_count: 40,
        required_event_types: &["add_vote"],
    },
    ScenarioSpec {
        name: "hos_delegation_187483",
        start_height: 187_483_260,
        block_count: 40,
        required_event_types: &["delegate_all"],
    },
    ScenarioSpec {
        name: "hos_config_185721",
        start_height: 185_721_110,
        block_count: 40,
        required_event_types: &["admin_set_guardians", "admin_set_reviewer_ids"],
    },
];

fn parse_csv_env(name: &str) -> Option<Vec<String>> {
    std::env::var(name).ok().map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect()
    })
}

fn resolve_scenarios(selected_names: Option<Vec<String>>) -> Result<Vec<ScenarioSpec>> {
    let Some(selected_names) = selected_names else {
        return Ok(DEFAULT_SCENARIOS.to_vec());
    };

    if selected_names.is_empty() {
        bail!("SCENARIOS resolved to an empty list");
    }

    let mut selected = Vec::with_capacity(selected_names.len());
    for scenario_name in selected_names {
        let Some(spec) = DEFAULT_SCENARIOS
            .iter()
            .find(|spec| spec.name == scenario_name)
        else {
            let known = DEFAULT_SCENARIOS
                .iter()
                .map(|spec| spec.name)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(anyhow!(
                "unknown scenario '{scenario_name}'. Known scenarios: {known}"
            ));
        };
        selected.push(*spec);
    }

    Ok(selected)
}

fn parse_bool_env(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

async fn capture_scenario(
    scenario: ScenarioSpec,
    fixtures_dir: &Path,
    api_key: Option<String>,
) -> Result<()> {
    let scenario_path = fixtures_dir.join(format!("{}.json", scenario.name));

    info!(
        scenario = scenario.name,
        start_height = scenario.start_height,
        block_count = scenario.block_count,
        required_event_types = ?scenario.required_event_types,
        "Capturing fixture range scenario"
    );

    let (_handle, stream) = create_fastnear_stream(
        LakeStreamConfig {
            chain: "mainnet".to_string(),
            start_height: scenario.start_height,
            concurrency: 1,
            preload_pool_size: 1,
        },
        api_key,
        None,
    )?;

    let target_event_set: HashSet<String> = scenario
        .required_event_types
        .iter()
        .map(|value| value.to_string())
        .collect();
    let mut found_event_types: HashSet<String> = HashSet::new();
    let mut messages = Vec::with_capacity(scenario.block_count as usize);
    let mut stream = ReceiverStream::new(stream);

    while messages.len() < scenario.block_count as usize {
        let Some(message) = stream.next().await else {
            break;
        };

        for event in classify_block_with_targets(&message, &found_event_types, &target_event_set) {
            found_event_types.insert(event);
        }
        messages.push(message);
    }

    if messages.len() != scenario.block_count as usize {
        bail!(
            "scenario '{}' expected {} blocks but captured {}",
            scenario.name,
            scenario.block_count,
            messages.len()
        );
    }

    let missing_required = scenario
        .required_event_types
        .iter()
        .filter(|event| !found_event_types.contains(**event))
        .copied()
        .collect::<Vec<_>>();
    if !missing_required.is_empty() {
        bail!(
            "scenario '{}' missing required events: {:?}",
            scenario.name,
            missing_required
        );
    }

    let first_height = messages
        .first()
        .map(|msg| msg.block.header.height)
        .unwrap_or_default();
    let last_height = messages
        .last()
        .map(|msg| msg.block.header.height)
        .unwrap_or_default();

    let json = serde_json::to_string_pretty(&messages)
        .with_context(|| format!("failed to serialize scenario '{}'", scenario.name))?;
    tokio::fs::write(&scenario_path, json)
        .await
        .with_context(|| format!("failed to write fixture {}", scenario_path.display()))?;

    info!(
        scenario = scenario.name,
        path = %scenario_path.display(),
        first_height,
        last_height,
        captured_blocks = messages.len(),
        found_event_types = ?found_event_types,
        "Scenario fixture range saved"
    );

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,near_lake_framework=warn".into()),
        )
        .json()
        .init();

    let api_key = std::env::var("FASTNEAR_API_KEY").ok();
    let selected_scenarios = resolve_scenarios(parse_csv_env("SCENARIOS"))?;
    let overwrite_existing = parse_bool_env("OVERWRITE_EXISTING", false);

    let fixtures_dir = std::env::var("FIXTURES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/venear_governance/ranges")
        });

    tokio::fs::create_dir_all(&fixtures_dir)
        .await
        .with_context(|| format!("failed to create fixtures dir: {}", fixtures_dir.display()))?;

    info!(
        scenarios = ?selected_scenarios.iter().map(|spec| spec.name).collect::<Vec<_>>(),
        overwrite_existing = overwrite_existing,
        fixtures_dir = %fixtures_dir.display(),
        "Starting veNEAR fixture range fetcher"
    );

    for scenario in selected_scenarios {
        let scenario_path = fixtures_dir.join(format!("{}.json", scenario.name));
        if scenario_path.exists() && !overwrite_existing {
            info!(
                scenario = scenario.name,
                path = %scenario_path.display(),
                "Fixture scenario already exists, skipping"
            );
            continue;
        }
        capture_scenario(scenario, &fixtures_dir, api_key.clone()).await?;
    }

    info!(
        fixtures_dir = %fixtures_dir.display(),
        "All requested veNEAR fixture ranges captured successfully"
    );

    Ok(())
}

/// Inspect a `StreamerMessage` and return the subset of `target_event_set`
/// discovered in this block but not present in `already_found`.
///
/// In NEAR Lake each `IndexerShard` contains `receipt_execution_outcomes` where
/// every entry bundles the executed receipt with its outcome.
fn classify_block_with_targets(
    message: &StreamerMessage,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) -> Vec<String> {
    let mut new_types = Vec::new();

    for shard in &message.shards {
        // Transaction-level outcomes
        if let Some(chunk) = &shard.chunk {
            for tx_with_outcome in &chunk.transactions {
                let receiver = tx_with_outcome.transaction.receiver_id.as_str();
                let logs = &tx_with_outcome.outcome.execution_outcome.outcome.logs;
                check_logs_for_events(
                    logs,
                    receiver,
                    &mut new_types,
                    already_found,
                    target_event_set,
                );
                check_plain_logs(
                    logs,
                    receiver,
                    &mut new_types,
                    already_found,
                    target_event_set,
                );

                if is_venear_contract(receiver) || is_voting_contract(receiver) {
                    check_admin_actions(
                        receiver,
                        &tx_with_outcome.transaction.actions,
                        &mut new_types,
                        already_found,
                        target_event_set,
                    );
                }

                if is_venear_contract(receiver) {
                    let success = is_status_success(
                        &tx_with_outcome.outcome.execution_outcome.outcome.status,
                    );
                    check_delegation_actions(
                        &tx_with_outcome.transaction.actions,
                        success,
                        &mut new_types,
                        already_found,
                        target_event_set,
                    );
                }
            }
        }

        // Receipt execution outcomes — each entry has BOTH the receipt and its outcome.
        for receipt_with_outcome in &shard.receipt_execution_outcomes {
            let receipt = &receipt_with_outcome.receipt;
            let outcome = &receipt_with_outcome.execution_outcome;
            let receiver = receipt.receiver_id.as_str();
            let logs = &outcome.outcome.logs;

            // Scan EVENT_JSON logs for known veNEAR / nep141 events.
            check_logs_for_events(
                logs,
                receiver,
                &mut new_types,
                already_found,
                target_event_set,
            );
            check_plain_logs(
                logs,
                receiver,
                &mut new_types,
                already_found,
                target_event_set,
            );

            // Callback method names (on_get_snapshot, on_lockup_update, on_lockup_deployed) are
            // recognized by the *method* being called, not by emitted events.
            if is_voting_contract(receiver) || is_venear_contract(receiver) {
                check_callback_receipts(
                    receipt,
                    receipt_with_outcome,
                    &mut new_types,
                    already_found,
                    target_event_set,
                );
                check_admin_receipt_actions(
                    receiver,
                    receipt,
                    &mut new_types,
                    already_found,
                    target_event_set,
                );
            }

            if is_venear_contract(receiver) {
                let success = is_status_success(&outcome.outcome.status);
                check_delegation_receipt_actions(
                    receipt,
                    success,
                    &mut new_types,
                    already_found,
                    target_event_set,
                );
            }

            // Any function call to a .venear.dao lockup account is a lockup_action.
            if receiver.ends_with(LOCKUP_SUFFIX) {
                if !already_found.contains("lockup_action")
                    && target_event_set.contains("lockup_action")
                    && has_function_call_action(receipt)
                {
                    new_types.push("lockup_action".to_string());
                }
                // Detect lockup lifecycle methods that don't emit events.
                check_lockup_lifecycle_receipt_actions(
                    receipt,
                    &mut new_types,
                    already_found,
                    target_event_set,
                );
            }
        }
    }

    new_types.sort();
    new_types.dedup();
    new_types
}

fn is_status_success(status: &impl serde::Serialize) -> bool {
    let Ok(value) = serde_json::to_value(status) else {
        return false;
    };

    let Some(map) = value.as_object() else {
        return false;
    };

    map.contains_key("SuccessValue") || map.contains_key("SuccessReceiptId")
}

fn push_key_if_target(
    key: &str,
    new_types: &mut Vec<String>,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) {
    if !already_found.contains(key) && target_event_set.contains(key) {
        new_types.push(key.to_string());
    }
}

fn check_delegation_actions(
    actions: &[near_lake_framework::near_indexer_primitives::views::ActionView],
    success: bool,
    new_types: &mut Vec<String>,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) {
    use near_lake_framework::near_indexer_primitives::views::ActionView;

    for action in actions {
        let ActionView::FunctionCall { method_name, .. } = action else {
            continue;
        };

        let key = match (method_name.as_str(), success) {
            ("delegate_all", true) => Some("delegate_all"),
            ("delegate_all", false) => Some("delegate_all_failed"),
            ("undelegate", true) => Some("undelegate"),
            ("undelegate", false) => Some("undelegate_failed"),
            _ => None,
        };

        if let Some(key) = key {
            push_key_if_target(key, new_types, already_found, target_event_set);
        }
    }
}

fn check_delegation_receipt_actions(
    receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    success: bool,
    new_types: &mut Vec<String>,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) {
    use near_lake_framework::near_indexer_primitives::views::ReceiptEnumView;

    if let ReceiptEnumView::Action { actions, .. } = &receipt.receipt {
        check_delegation_actions(actions, success, new_types, already_found, target_event_set);
    }
}

fn admin_key_for_method(receiver: &str, method_name: &str) -> Option<&'static str> {
    if receiver == VENEAR_CONTRACT {
        return match method_name {
            "set_lockup_contract" => Some("admin_set_lockup_contract"),
            "set_lockup_code_deployers" => Some("admin_set_lockup_code_deployers"),
            "prepare_lockup_code" => Some("admin_prepare_lockup_code"),
            _ => None,
        };
    }

    if receiver == VOTING_CONTRACT {
        return match method_name {
            "set_guardians" => Some("admin_set_guardians"),
            "set_reviewer_ids" => Some("admin_set_reviewer_ids"),
            "upgrade" => Some("admin_upgrade"),
            "migrate_state" => Some("admin_migrate_state"),
            _ => None,
        };
    }

    None
}

fn check_admin_actions(
    receiver: &str,
    actions: &[near_lake_framework::near_indexer_primitives::views::ActionView],
    new_types: &mut Vec<String>,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) {
    use near_lake_framework::near_indexer_primitives::views::ActionView;

    for action in actions {
        let ActionView::FunctionCall { method_name, .. } = action else {
            continue;
        };

        if let Some(key) = admin_key_for_method(receiver, method_name.as_str()) {
            push_key_if_target(key, new_types, already_found, target_event_set);
        }
    }
}

fn check_admin_receipt_actions(
    receiver: &str,
    receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    new_types: &mut Vec<String>,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) {
    use near_lake_framework::near_indexer_primitives::views::ReceiptEnumView;

    if let ReceiptEnumView::Action { actions, .. } = &receipt.receipt {
        check_admin_actions(
            receiver,
            actions,
            new_types,
            already_found,
            target_event_set,
        );
    }
}

fn check_logs_for_events(
    logs: &[String],
    receiver: &str,
    new_types: &mut Vec<String>,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) {
    for log in logs {
        if !log.starts_with(EVENT_JSON_PREFIX) {
            continue;
        }
        let json_part = &log[EVENT_JSON_PREFIX.len()..];
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_part) else {
            continue;
        };
        let Some(event) = parsed.get("event").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(key) = resolve_event_key(event, receiver) {
            if !already_found.contains(key) && target_event_set.contains(key) {
                new_types.push(key.to_string());
            }
        }
    }
}

fn check_plain_logs(
    logs: &[String],
    receiver: &str,
    new_types: &mut Vec<String>,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) {
    if !target_event_set.contains("plain_log") || already_found.contains("plain_log") {
        return;
    }

    if !is_venear_contract(receiver)
        && !is_voting_contract(receiver)
        && !receiver.ends_with(LOCKUP_SUFFIX)
    {
        return;
    }

    if logs
        .iter()
        .any(|log| !log.starts_with(EVENT_JSON_PREFIX) && !log.trim().is_empty())
    {
        new_types.push("plain_log".to_string());
    }
}

fn check_lockup_lifecycle_receipt_actions(
    receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    new_types: &mut Vec<String>,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) {
    use near_lake_framework::near_indexer_primitives::views::{ActionView, ReceiptEnumView};
    if let ReceiptEnumView::Action { actions, .. } = &receipt.receipt {
        for action in actions {
            if let ActionView::FunctionCall { method_name, .. } = action {
                let key = match method_name.as_str() {
                    "begin_unlock_near" => Some("begin_unlock_near"),
                    "end_unlock_near" => Some("end_unlock_near"),
                    "lock_pending_near" => Some("lock_pending_near"),
                    _ => None,
                };
                if let Some(key) = key {
                    push_key_if_target(key, new_types, already_found, target_event_set);
                }
            }
        }
    }
}

fn check_callback_receipts(
    receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    outcome: &near_lake_framework::near_indexer_primitives::IndexerExecutionOutcomeWithReceipt,
    new_types: &mut Vec<String>,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) {
    use near_lake_framework::near_indexer_primitives::views::{ActionView, ReceiptEnumView};
    if let ReceiptEnumView::Action { actions, .. } = &receipt.receipt {
        for action in actions {
            if let ActionView::FunctionCall { method_name, .. } = action {
                let key = match method_name.as_str() {
                    "on_get_snapshot" => Some("on_get_snapshot"),
                    "on_lockup_update" => Some("on_lockup_update"),
                    "on_lockup_deployed" => {
                        // Distinguish success vs failure by whether lockup_deployed event fired.
                        // If there is a lockup_deployed EVENT_JSON log in this receipt, it's success.
                        let has_lockup_deployed_event =
                            outcome.execution_outcome.outcome.logs.iter().any(|log| {
                                log.starts_with(EVENT_JSON_PREFIX)
                                    && serde_json::from_str::<serde_json::Value>(
                                        &log[EVENT_JSON_PREFIX.len()..],
                                    )
                                    .ok()
                                    .and_then(|v| {
                                        v.get("event")
                                            .and_then(|e| e.as_str())
                                            .map(|e| e == "lockup_deployed")
                                    })
                                    .unwrap_or(false)
                            });
                        if has_lockup_deployed_event {
                            Some("on_lockup_deployed")
                        } else {
                            Some("on_lockup_deployed_failed")
                        }
                    }
                    _ => None,
                };
                if let Some(key) = key {
                    if !already_found.contains(key) && target_event_set.contains(key) {
                        new_types.push(key.to_string());
                    }
                }
            }
        }
    }
}

fn has_function_call_action(
    receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
) -> bool {
    use near_lake_framework::near_indexer_primitives::views::{ActionView, ReceiptEnumView};
    if let ReceiptEnumView::Action { actions, .. } = &receipt.receipt {
        actions
            .iter()
            .any(|a| matches!(a, ActionView::FunctionCall { .. }))
    } else {
        false
    }
}

fn is_venear_contract(id: &str) -> bool {
    id == VENEAR_CONTRACT
}

fn is_voting_contract(id: &str) -> bool {
    id == VOTING_CONTRACT
}

/// Map a raw event name (from EVENT_JSON log) to a fixture file key.
/// `receiver` is the contract that emitted the log — used to filter ft events
/// to only `venear.dao` (the veNEAR token contract).
fn resolve_event_key<'a>(event: &str, receiver: &str) -> Option<&'a str> {
    match event {
        // veNEAR governance events — always on venear.dao or vote.dao
        "lockup_deployed" => Some("lockup_deployed"),
        "lockup_update" => Some("lockup_update"),
        "lockup_delete" => Some("lockup_delete"),
        "lockup_lock_near" => Some("lockup_lock_near"),
        "create_proposal" => Some("create_proposal"),
        "proposal_approve" => Some("proposal_approve"),
        "proposal_reject" => Some("proposal_reject"),
        "add_vote" => Some("add_vote"),
        "remove_vote" => Some("remove_vote"),
        // NEP-141 token events — only capture from venear.dao (the veNEAR token)
        "ft_mint" if receiver == VENEAR_CONTRACT => Some("ft_mint"),
        "ft_burn" if receiver == VENEAR_CONTRACT => Some("ft_burn"),
        _ => None,
    }
}
