//! Fixture fetcher for veNEAR governance integration tests.
//!
//! Streams mainnet blocks and saves one `StreamerMessage` per requested
//! veNEAR fixture type to `tests/fixtures/venear_governance/`.
//!
//! By default it uses FastNEAR streaming (no AWS credentials required).
//!
//! Usage:
//!   cd indexers/near-indexer
//!   START_BLOCK_HEIGHT=<height> cargo run --example fetch_venear_fixtures
//!
//! Required env vars:
//!   START_BLOCK_HEIGHT   - Block height to start scanning from
//!
//! Optional env vars:
//!   FASTNEAR_API_KEY     - FastNEAR authorization token
//!   TARGET_EVENT_TYPES   - Comma-separated fixture keys to capture
//!                          (default: full set in DEFAULT_TARGET_EVENT_TYPES)
//!   OPTIONAL_EVENT_TYPES - Comma-separated subset allowed to be missing
//!                          (default: proposal_reject,remove_vote)
//!   OVERWRITE_EXISTING   - true/false (default: false)
//!   MAX_SCAN_BLOCKS      - Maximum blocks to scan (default: 500_000)
//!   FIXTURES_DIR         - Output dir (default: tests/fixtures/venear_governance)

use anyhow::{bail, Context, Result};
use near_indexer::core::lake_stream::{create_fastnear_stream, LakeStreamConfig};
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use std::collections::HashSet;
use std::path::PathBuf;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tracing::{info, warn};

/// All event types we want to capture as fixtures.
/// The file name for each is the event type name.
const DEFAULT_TARGET_EVENT_TYPES: &[&str] = &[
    "lockup_deployed",
    "lockup_update",
    "lockup_delete",
    "create_proposal",
    "proposal_approve",
    "proposal_reject",
    "on_get_snapshot",
    "add_vote",
    "remove_vote",
    "ft_mint",
    "ft_burn",
    "on_lockup_update",
    "lockup_action",
    "delegate_all",
    "undelegate",
    "delegate_all_failed",
    "undelegate_failed",
];

const DEFAULT_OPTIONAL_EVENT_TYPES: &[&str] = &["proposal_reject", "remove_vote"];

const EVENT_JSON_PREFIX: &str = "EVENT_JSON:";
const LOCKUP_SUFFIX: &str = ".venear.dao";
const VENEAR_CONTRACT: &str = "venear.dao";
const VOTING_CONTRACT: &str = "vote.dao";

fn parse_event_list_env(name: &str) -> Option<Vec<String>> {
    std::env::var(name).ok().map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect()
    })
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,near_lake_framework=warn".into()),
        )
        .json()
        .init();

    let start_height: u64 = std::env::var("START_BLOCK_HEIGHT")
        .context("START_BLOCK_HEIGHT is required")?
        .parse()
        .context("START_BLOCK_HEIGHT must be a valid u64")?;

    let api_key = std::env::var("FASTNEAR_API_KEY").ok();
    let overwrite_existing = parse_bool_env("OVERWRITE_EXISTING", false);

    let target_event_types: Vec<String> = parse_event_list_env("TARGET_EVENT_TYPES")
        .unwrap_or_else(|| {
            DEFAULT_TARGET_EVENT_TYPES
                .iter()
                .map(|s| s.to_string())
                .collect()
        });

    if target_event_types.is_empty() {
        bail!("TARGET_EVENT_TYPES resolved to an empty list");
    }

    let optional_event_types: HashSet<String> = parse_event_list_env("OPTIONAL_EVENT_TYPES")
        .unwrap_or_else(|| {
            DEFAULT_OPTIONAL_EVENT_TYPES
                .iter()
                .map(|s| s.to_string())
                .collect()
        })
        .into_iter()
        .collect();

    let target_event_set: HashSet<String> = target_event_types.iter().cloned().collect();

    for optional in &optional_event_types {
        if !target_event_set.contains(optional) {
            warn!(
                optional_event = optional,
                "OPTIONAL_EVENT_TYPES entry is not part of TARGET_EVENT_TYPES"
            );
        }
    }

    let max_scan_blocks: u64 = std::env::var("MAX_SCAN_BLOCKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500_000);

    let fixtures_dir = std::env::var("FIXTURES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/venear_governance")
        });

    tokio::fs::create_dir_all(&fixtures_dir)
        .await
        .with_context(|| format!("Failed to create fixtures dir: {}", fixtures_dir.display()))?;

    info!(
        start_height = start_height,
        max_scan_blocks = max_scan_blocks,
        overwrite_existing = overwrite_existing,
        target_event_types = ?target_event_types,
        optional_event_types = ?optional_event_types,
        fixtures_dir = %fixtures_dir.display(),
        "Starting veNEAR fixture fetcher"
    );

    // Check which fixtures already exist so we can skip those event types.
    let mut found_event_types: HashSet<String> = HashSet::new();
    for event_type in &target_event_types {
        let path = fixtures_dir.join(format!("{event_type}.json"));
        if path.exists() && !overwrite_existing {
            info!(event_type, "Fixture already exists, skipping");
            found_event_types.insert(event_type.clone());
        }
    }

    let remaining: Vec<&str> = target_event_types
        .iter()
        .map(String::as_str)
        .filter(|t| !found_event_types.contains(*t))
        .collect();

    if remaining.is_empty() {
        info!("All fixtures already exist. Nothing to fetch.");
        return Ok(());
    }

    info!(
        remaining = ?remaining,
        "Scanning for {} missing event types",
        remaining.len()
    );

    let (_handle, stream) = create_fastnear_stream(
        LakeStreamConfig {
            chain: "mainnet".to_string(),
            start_height,
            concurrency: 1,
            preload_pool_size: 1,
        },
        api_key,
        None,
    )?;

    let mut stream = ReceiverStream::new(stream);
    let mut scanned: u64 = 0;

    while scanned < max_scan_blocks {
        let Some(message) = stream.next().await else {
            info!("Stream ended after {} blocks", scanned);
            break;
        };

        let block_height = message.block.header.height;

        // Check all shards for matching event logs and function calls.
        let new_types = classify_block(&message, &found_event_types, &target_event_set);
        for event_type in &new_types {
            let path = fixtures_dir.join(format!("{event_type}.json"));
            info!(
                block_height,
                event_type,
                path = %path.display(),
                "Saving fixture"
            );
            let json = serde_json::to_string_pretty(&message)
                .with_context(|| format!("Failed to serialize block {block_height}"))?;
            tokio::fs::write(&path, json)
                .await
                .with_context(|| format!("Failed to write fixture {}", path.display()))?;
            found_event_types.insert(event_type.clone());
        }

        scanned += 1;

        if scanned % 1000 == 0 {
            let still_missing: Vec<_> = target_event_types
                .iter()
                .map(String::as_str)
                .filter(|t| !found_event_types.contains(*t))
                .collect();
            info!(
                block_height,
                scanned,
                found = found_event_types.len(),
                missing = ?still_missing,
                "Progress"
            );
        }

        // Stop once all required event types have been captured.
        if target_event_types
            .iter()
            .all(|t| optional_event_types.contains(t) || found_event_types.contains(t))
        {
            info!(
                block_height,
                scanned, "All required event types captured — stopping scan"
            );
            break;
        }
    }

    // Final summary
    let missing: Vec<_> = target_event_types
        .iter()
        .map(String::as_str)
        .filter(|t| !found_event_types.contains(*t))
        .collect();

    let required_missing: Vec<_> = missing
        .iter()
        .copied()
        .filter(|t| !optional_event_types.contains(*t))
        .collect();

    let optional_missing: Vec<_> = missing
        .iter()
        .copied()
        .filter(|t| optional_event_types.contains(*t))
        .collect();

    if required_missing.is_empty() {
        info!(
            scanned,
            optional_missing = ?optional_missing,
            "SUCCESS: Required veNEAR fixture types captured in {}",
            fixtures_dir.display()
        );
    } else {
        warn!(
            scanned,
            required_missing = ?required_missing,
            optional_missing = ?optional_missing,
            "Scan complete but {} required event types were NOT found. \
             Try a wider START_BLOCK_HEIGHT range or increase MAX_SCAN_BLOCKS.",
            required_missing.len()
        );
    }

    Ok(())
}

/// Inspect a `StreamerMessage` and return the set of target event types found
/// that haven't been captured yet.
///
/// In NEAR Lake each `IndexerShard` contains `receipt_execution_outcomes` where
/// every entry bundles the *executed* receipt together with its outcome — this
/// is the correct place to look for event logs and callback method names.
fn classify_block(
    message: &StreamerMessage,
    already_found: &HashSet<String>,
    target_event_set: &HashSet<String>,
) -> Vec<String> {
    classify_block_with_targets(message, already_found, target_event_set)
}

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

            // Callback method names (on_get_snapshot, on_lockup_update) are
            // recognized by the *method* being called, not by emitted events.
            if is_voting_contract(receiver) || is_venear_contract(receiver) {
                check_callback_receipts(receipt, &mut new_types, already_found, target_event_set);
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
            if receiver.ends_with(LOCKUP_SUFFIX)
                && !already_found.contains("lockup_action")
                && target_event_set.contains("lockup_action")
                && has_function_call_action(receipt)
            {
                new_types.push("lockup_action".to_string());
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

fn check_callback_receipts(
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
                    "on_get_snapshot" => Some("on_get_snapshot"),
                    "on_lockup_update" => Some("on_lockup_update"),
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
