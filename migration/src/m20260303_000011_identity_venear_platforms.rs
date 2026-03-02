use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            INSERT INTO identity.platforms (id, label)
            VALUES
                ('venear_governance', 'veNEAR Governance'),
                ('venear_lockup', 'veNEAR Lockup')
            ON CONFLICT (id) DO UPDATE
            SET label = EXCLUDED.label
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            UPDATE identity.accounts
            SET
                platform_id = 'venear_lockup',
                updated_at = CURRENT_TIMESTAMP
            WHERE platform_id = 'near'
              AND scope = 'mainnet'
              AND external_id LIKE '%.venear.dao'
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            UPDATE identity.accounts
            SET
                platform_id = 'venear_governance',
                updated_at = CURRENT_TIMESTAMP
            WHERE platform_id = 'near'
              AND scope = 'mainnet'
              AND external_id NOT LIKE '%.venear.dao'
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            UPDATE identity.accounts
            SET
                platform_id = 'near',
                updated_at = CURRENT_TIMESTAMP
            WHERE platform_id IN ('venear_governance', 'venear_lockup')
              AND scope = 'mainnet'
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            DELETE FROM identity.platforms p
            WHERE p.id IN ('venear_governance', 'venear_lockup')
              AND NOT EXISTS (
                    SELECT 1
                    FROM identity.accounts a
                    WHERE a.platform_id = p.id
              )
            "#,
        )
        .await?;

        Ok(())
    }
}
