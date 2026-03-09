use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let statements: &[&str] = &[
            "ALTER TABLE near.near_receipt_tx_hash_map
             ADD COLUMN IF NOT EXISTS is_persisted_graph BOOLEAN NOT NULL DEFAULT FALSE",
            "CREATE INDEX IF NOT EXISTS ix_near_receipt_tx_hash_map_prunable_last_seen_height
               ON near.near_receipt_tx_hash_map (last_seen_height)
               WHERE is_persisted_graph = FALSE",
            "UPDATE near.near_receipt_tx_hash_map map
             SET is_persisted_graph = TRUE,
                 updated_at = NOW()
             WHERE EXISTS (
                 SELECT 1
                 FROM near.near_receipts r
                 WHERE r.receipt_id = map.receipt_id
             )",
            "WITH candidate_tx_hashes AS (
                 SELECT
                     r.receipt_id,
                     eo.transaction_hash,
                     eo.block_height,
                     1 AS priority
                 FROM near.near_receipts r
                 JOIN near.near_execution_outcomes eo
                   ON eo.receipt_id = r.receipt_id
                 WHERE eo.transaction_hash IS NOT NULL

                 UNION ALL

                 SELECT
                     r.receipt_id,
                     ra.transaction_hash,
                     ra.block_height,
                     2 AS priority
                 FROM near.near_receipts r
                 JOIN near.near_receipt_actions ra
                   ON ra.receipt_id = r.receipt_id
                 WHERE ra.transaction_hash IS NOT NULL

                 UNION ALL

                 SELECT
                     r.receipt_id,
                     re.transaction_hash,
                     re.block_height,
                     3 AS priority
                 FROM near.near_receipts r
                 JOIN near.near_receipt_edges re
                   ON re.child_receipt_id = r.receipt_id
                 WHERE re.transaction_hash IS NOT NULL

                 UNION ALL

                 SELECT
                     r.receipt_id,
                     re.transaction_hash,
                     re.block_height,
                     4 AS priority
                 FROM near.near_receipts r
                 JOIN near.near_receipt_edges re
                   ON re.parent_receipt_id = r.receipt_id
                 WHERE re.transaction_hash IS NOT NULL

                 UNION ALL

                 SELECT
                     r.receipt_id,
                     eo.transaction_hash,
                     GREATEST(re.block_height, eo.block_height) AS block_height,
                     5 AS priority
                 FROM near.near_receipts r
                 JOIN near.near_receipt_edges re
                   ON re.child_receipt_id = r.receipt_id
                 JOIN near.near_execution_outcomes eo
                   ON eo.id = re.parent_execution_outcome_id
                 WHERE eo.transaction_hash IS NOT NULL
             ),
             ranked_tx_hashes AS (
                 SELECT
                     receipt_id,
                     transaction_hash,
                     block_height,
                     ROW_NUMBER() OVER (
                         PARTITION BY receipt_id
                         ORDER BY priority ASC, block_height ASC, transaction_hash ASC
                     ) AS rn
                 FROM candidate_tx_hashes
             )
             INSERT INTO near.near_receipt_tx_hash_map (
                 receipt_id,
                 transaction_hash,
                 is_persisted_graph,
                 first_seen_height,
                 last_seen_height,
                 created_at,
                 updated_at
             )
             SELECT
                 ranked.receipt_id,
                 ranked.transaction_hash,
                 TRUE,
                 ranked.block_height,
                 ranked.block_height,
                 NOW(),
                 NOW()
             FROM ranked_tx_hashes ranked
             WHERE ranked.rn = 1
             ON CONFLICT (receipt_id) DO UPDATE
             SET is_persisted_graph = TRUE,
                 first_seen_height = LEAST(
                     near.near_receipt_tx_hash_map.first_seen_height,
                     EXCLUDED.first_seen_height
                 ),
                 last_seen_height = GREATEST(
                     near.near_receipt_tx_hash_map.last_seen_height,
                     EXCLUDED.last_seen_height
                 ),
                 updated_at = NOW()",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let statements: &[&str] = &[
            "DROP INDEX IF EXISTS near.ix_near_receipt_tx_hash_map_prunable_last_seen_height",
            "ALTER TABLE near.near_receipt_tx_hash_map
             DROP COLUMN IF EXISTS is_persisted_graph",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }
}
