use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("identity"), Alias::new("persons")))
                    .add_column(ColumnDef::new(IdentityPersons::Name).string_len(255).null())
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                r#"
                WITH account_candidates AS (
                    SELECT
                        pa.person_id,
                        a.platform_id,
                        a.external_id,
                        aa.alias,
                        CASE
                            WHEN a.platform_id = 'discourse'
                                AND aa.alias IS NOT NULL
                                AND btrim(aa.alias) <> '' THEN aa.alias
                            WHEN a.platform_id = 'telegram'
                                AND aa.alias IS NOT NULL
                                AND btrim(aa.alias) <> '' THEN aa.alias
                            WHEN a.platform_id IN ('near', 'venear_governance')
                                AND a.external_id NOT LIKE '%.venear.dao' THEN
                                NULLIF(
                                    btrim(
                                        regexp_replace(
                                            CASE
                                                WHEN a.external_id LIKE '%.near' THEN regexp_replace(a.external_id, '\\.near$', '')
                                                WHEN a.external_id LIKE '%.tg' THEN regexp_replace(a.external_id, '\\.tg$', '')
                                                ELSE a.external_id
                                            END,
                                            '([_.-]?gov)$',
                                            ''
                                        )
                                    ),
                                    ''
                                )
                            ELSE NULL
                        END AS candidate_name,
                        CASE
                            WHEN a.platform_id = 'discourse'
                                AND aa.alias IS NOT NULL
                                AND btrim(aa.alias) <> '' THEN 10
                            WHEN a.platform_id = 'telegram'
                                AND aa.alias IS NOT NULL
                                AND btrim(aa.alias) <> '' THEN 20
                            WHEN a.platform_id IN ('near', 'venear_governance')
                                AND a.external_id NOT LIKE '%.venear.dao' THEN 30
                            ELSE 100
                        END AS priority
                    FROM identity.person_accounts pa
                    JOIN identity.accounts a
                      ON a.id = pa.account_id
                    LEFT JOIN LATERAL (
                        SELECT aa.alias
                        FROM identity.account_aliases aa
                        WHERE aa.account_id = a.id
                          AND aa.is_current = TRUE
                        ORDER BY aa.last_seen_at DESC, aa.id DESC
                        LIMIT 1
                    ) aa ON TRUE
                ),
                ranked_candidates AS (
                    SELECT
                        person_id,
                        candidate_name,
                        ROW_NUMBER() OVER (
                            PARTITION BY person_id
                            ORDER BY priority, length(candidate_name), candidate_name
                        ) AS rn
                    FROM account_candidates
                    WHERE candidate_name IS NOT NULL
                )
                UPDATE identity.persons p
                SET
                    name = COALESCE(
                        rc.candidate_name,
                        CONCAT('person-', p.id::text)
                    ),
                    updated_at = CURRENT_TIMESTAMP
                FROM ranked_candidates rc
                WHERE rc.person_id = p.id
                  AND rc.rn = 1;

                UPDATE identity.persons
                SET
                    name = CONCAT('person-', id::text),
                    updated_at = CURRENT_TIMESTAMP
                WHERE name IS NULL;
                "#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("identity"), Alias::new("persons")))
                    .drop_column(IdentityPersons::Name)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum IdentityPersons {
    Name,
}
