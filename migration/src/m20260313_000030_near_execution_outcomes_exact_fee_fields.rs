use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let statements: &[&str] = &[
            "ALTER TABLE near.near_execution_outcomes
             ADD COLUMN IF NOT EXISTS tokens_burnt_yocto TEXT",
            "UPDATE near.near_execution_outcomes
             SET tokens_burnt_yocto = COALESCE(
                 tokens_burnt_yocto,
                 outcome ->> 'tokens_burnt',
                 raw_data #>> '{outcome,tokens_burnt}',
                 '0'
             )
             WHERE tokens_burnt_yocto IS NULL",
            "ALTER TABLE near.near_execution_outcomes
             ALTER COLUMN tokens_burnt_yocto SET NOT NULL",
            "ALTER TABLE near.near_execution_outcomes
             DROP COLUMN IF EXISTS gas_used",
            "ALTER TABLE near.near_execution_outcomes
             DROP COLUMN IF EXISTS tokens_burnt",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let statements: &[&str] = &[
            "ALTER TABLE near.near_execution_outcomes
             ADD COLUMN IF NOT EXISTS gas_used DOUBLE PRECISION",
            "ALTER TABLE near.near_execution_outcomes
             ADD COLUMN IF NOT EXISTS tokens_burnt DOUBLE PRECISION",
            "UPDATE near.near_execution_outcomes
             SET tokens_burnt = COALESCE(
                 tokens_burnt,
                 ((NULLIF(tokens_burnt_yocto, '')::numeric) / 1e24)::double precision
             )
             WHERE tokens_burnt_yocto IS NOT NULL",
            "ALTER TABLE near.near_execution_outcomes
             DROP COLUMN IF EXISTS tokens_burnt_yocto",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }
}
