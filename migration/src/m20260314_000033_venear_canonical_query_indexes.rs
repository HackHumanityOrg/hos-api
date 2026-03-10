use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP_STATEMENTS: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_fc_method_height_receipt
       ON near.near_receipt_actions (method_name, block_height DESC, receipt_id DESC)
       WHERE action_kind = 'FunctionCall'",
    "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_fc_receiver_method_height_receipt
       ON near.near_receipt_actions (receiver_id, method_name, block_height DESC, receipt_id DESC)
       WHERE action_kind = 'FunctionCall'",
    "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_fc_predecessor_method_height_receipt
       ON near.near_receipt_actions (predecessor_id, method_name, block_height DESC, receipt_id DESC)
       WHERE action_kind = 'FunctionCall'",
    "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_fc_proposal_method_height_receipt
       ON near.near_receipt_actions (
           near.safe_bigint(args_json ->> 'proposal_id'),
           method_name,
           block_height DESC,
           receipt_id DESC
       )
       WHERE action_kind = 'FunctionCall'
         AND method_name IN ('approve_proposal', 'reject_proposal', 'vote')",
    "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_fc_lockup_owner_method_height_receipt
       ON near.near_receipt_actions (
           (args_json ->> 'owner_account_id'),
           method_name,
           block_height DESC,
           receipt_id DESC
       )
       WHERE action_kind = 'FunctionCall'
         AND method_name = 'on_lockup_update'",
    "CREATE INDEX IF NOT EXISTS ix_near_receipt_actions_fc_delegate_receiver_method_height_receipt
       ON near.near_receipt_actions (
           (args_json ->> 'receiver_id'),
           method_name,
           block_height DESC,
           receipt_id DESC
       )
       WHERE action_kind = 'FunctionCall'
         AND method_name = 'delegate_all'",
    "CREATE INDEX IF NOT EXISTS ix_near_execution_outcomes_receipt_executor_status
       ON near.near_execution_outcomes (receipt_id, executor_account_id, status)",
    "CREATE INDEX IF NOT EXISTS ix_near_receipt_edges_parent_receipt_id
       ON near.near_receipt_edges (parent_receipt_id)",
    "CREATE INDEX IF NOT EXISTS ix_near_receipt_edges_child_receipt_id
       ON near.near_receipt_edges (child_receipt_id)",
];

const DOWN_STATEMENTS: &[&str] = &[
    "DROP INDEX IF EXISTS near.ix_near_receipt_edges_child_receipt_id",
    "DROP INDEX IF EXISTS near.ix_near_receipt_edges_parent_receipt_id",
    "DROP INDEX IF EXISTS near.ix_near_execution_outcomes_receipt_executor_status",
    "DROP INDEX IF EXISTS near.ix_near_receipt_actions_fc_delegate_receiver_method_height_receipt",
    "DROP INDEX IF EXISTS near.ix_near_receipt_actions_fc_lockup_owner_method_height_receipt",
    "DROP INDEX IF EXISTS near.ix_near_receipt_actions_fc_proposal_method_height_receipt",
    "DROP INDEX IF EXISTS near.ix_near_receipt_actions_fc_predecessor_method_height_receipt",
    "DROP INDEX IF EXISTS near.ix_near_receipt_actions_fc_receiver_method_height_receipt",
    "DROP INDEX IF EXISTS near.ix_near_receipt_actions_fc_method_height_receipt",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for sql in UP_STATEMENTS {
            db.execute_unprepared(sql).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for sql in DOWN_STATEMENTS {
            db.execute_unprepared(sql).await?;
        }
        Ok(())
    }
}
