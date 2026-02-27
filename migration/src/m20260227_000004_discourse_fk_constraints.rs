use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UNIQUE_CONSTRAINTS: [(&str, &str); 4] = [
    ("discourse_categories", "uq_categories_instance_external_id"),
    ("discourse_users", "uq_users_instance_external_id"),
    ("discourse_topics", "uq_topics_instance_external_id"),
    ("discourse_posts", "uq_posts_instance_external_id"),
];

const LEGACY_UNIQUE_INDEXES: [&str; 4] = [
    "ux_categories_instance_ext",
    "ux_users_instance_ext",
    "ux_topics_instance_ext",
    "ux_posts_instance_ext",
];

const COMPOSITE_FK_DROPS: [(&str, &str); 6] = [
    ("discourse_topics", "fk_topics_category"),
    ("discourse_posts", "fk_posts_topic"),
    ("discourse_posts", "fk_posts_user"),
    ("discourse_post_revisions", "fk_revisions_post"),
    ("discourse_post_likes", "fk_likes_post"),
    ("discourse_post_likes", "fk_likes_user"),
];

const COMPOSITE_FK_CREATES: [&str; 6] = [
    r#"
ALTER TABLE discourse.discourse_topics
    ADD CONSTRAINT fk_topics_category
    FOREIGN KEY (instance, external_category_id)
    REFERENCES discourse.discourse_categories (instance, external_id)
    ON DELETE RESTRICT
    ON UPDATE CASCADE;
"#,
    r#"
ALTER TABLE discourse.discourse_posts
    ADD CONSTRAINT fk_posts_topic
    FOREIGN KEY (instance, external_topic_id)
    REFERENCES discourse.discourse_topics (instance, external_id)
    ON DELETE CASCADE
    ON UPDATE NO ACTION;
"#,
    r#"
ALTER TABLE discourse.discourse_posts
    ADD CONSTRAINT fk_posts_user
    FOREIGN KEY (instance, external_user_id)
    REFERENCES discourse.discourse_users (instance, external_id)
    ON DELETE RESTRICT
    ON UPDATE NO ACTION;
"#,
    r#"
ALTER TABLE discourse.discourse_post_revisions
    ADD CONSTRAINT fk_revisions_post
    FOREIGN KEY (instance, external_post_id)
    REFERENCES discourse.discourse_posts (instance, external_id)
    ON DELETE CASCADE
    ON UPDATE CASCADE;
"#,
    r#"
ALTER TABLE discourse.discourse_post_likes
    ADD CONSTRAINT fk_likes_post
    FOREIGN KEY (instance, external_post_id)
    REFERENCES discourse.discourse_posts (instance, external_id)
    ON DELETE CASCADE
    ON UPDATE CASCADE;
"#,
    r#"
ALTER TABLE discourse.discourse_post_likes
    ADD CONSTRAINT fk_likes_user
    FOREIGN KEY (instance, external_user_id)
    REFERENCES discourse.discourse_users (instance, external_id)
    ON DELETE NO ACTION
    ON UPDATE CASCADE;
"#,
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for (table, constraint) in COMPOSITE_FK_DROPS {
            db.execute_unprepared(&format!(
                "ALTER TABLE discourse.{table} DROP CONSTRAINT IF EXISTS {constraint};"
            ))
            .await?;
        }

        for (table, constraint) in UNIQUE_CONSTRAINTS {
            ensure_unique_constraint(manager, table, constraint).await?;
        }

        for index in LEGACY_UNIQUE_INDEXES {
            db.execute_unprepared(&format!("DROP INDEX IF EXISTS discourse.{index};"))
                .await?;
        }

        for statement in COMPOSITE_FK_CREATES {
            db.execute_unprepared(statement).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for (table, constraint) in COMPOSITE_FK_DROPS {
            db.execute_unprepared(&format!(
                "ALTER TABLE discourse.{table} DROP CONSTRAINT IF EXISTS {constraint};"
            ))
            .await?;
        }

        for (table, constraint) in UNIQUE_CONSTRAINTS {
            db.execute_unprepared(&format!(
                "ALTER TABLE discourse.{table} DROP CONSTRAINT IF EXISTS {constraint};"
            ))
            .await?;
        }

        db.execute_unprepared(
            r#"
CREATE UNIQUE INDEX IF NOT EXISTS ux_categories_instance_ext
    ON discourse.discourse_categories (instance, external_id);
CREATE UNIQUE INDEX IF NOT EXISTS ux_users_instance_ext
    ON discourse.discourse_users (instance, external_id);
CREATE UNIQUE INDEX IF NOT EXISTS ux_topics_instance_ext
    ON discourse.discourse_topics (instance, external_id);
CREATE UNIQUE INDEX IF NOT EXISTS ux_posts_instance_ext
    ON discourse.discourse_posts (instance, external_id);
"#,
        )
        .await?;

        for statement in COMPOSITE_FK_CREATES {
            db.execute_unprepared(statement).await?;
        }

        Ok(())
    }
}

async fn ensure_unique_constraint(
    manager: &SchemaManager<'_>,
    table_name: &str,
    constraint_name: &str,
) -> Result<(), DbErr> {
    let sql = format!(
        r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint c
        JOIN pg_class t ON t.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = t.relnamespace
        WHERE n.nspname = 'discourse'
          AND t.relname = '{table_name}'
          AND c.conname = '{constraint_name}'
          AND c.contype = 'u'
    ) THEN
        ALTER TABLE discourse.{table_name}
            ADD CONSTRAINT {constraint_name}
            UNIQUE (instance, external_id);
    END IF;
END $$;
"#
    );

    manager.get_connection().execute_unprepared(&sql).await?;
    Ok(())
}
