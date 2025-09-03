use crate::domain::util::with_deadlock_retry;
use anyhow::Result;
use entities::discourse_indexer_instances;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};

pub async fn register_instance(
    db: &sea_orm::DatabaseConnection,
    key: &str,
    base_url: &str,
) -> Result<()> {
    let am = discourse_indexer_instances::ActiveModel {
        key: Set(key.to_string()),
        base_url: Set(base_url.to_string()),
        enabled: Set(1),
        ..Default::default()
    };
    with_deadlock_retry(|| async {
        discourse_indexer_instances::Entity::insert(am.clone())
            .on_conflict(
                OrmOnConflict::column(discourse_indexer_instances::Column::Key)
                    .update_columns([
                        discourse_indexer_instances::Column::BaseUrl,
                        discourse_indexer_instances::Column::Enabled,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
    })
    .await?;
    Ok(())
}
