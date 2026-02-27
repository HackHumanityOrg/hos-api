use anyhow::Result;
use entities::discourse::discourse_instances;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};

pub async fn register_instance(
    db: &sea_orm::DatabaseConnection,
    key: &str,
    base_url: &str,
) -> Result<()> {
    let am = discourse_instances::ActiveModel {
        key: Set(key.to_string()),
        base_url: Set(base_url.to_string()),
        enabled: Set(true),
        ..Default::default()
    };
    discourse_instances::Entity::insert(am.clone())
        .on_conflict(
            OrmOnConflict::column(discourse_instances::Column::Key)
                .update_columns([
                    discourse_instances::Column::BaseUrl,
                    discourse_instances::Column::Enabled,
                ])
                .to_owned(),
        )
        .exec(db)
        .await?;
    Ok(())
}
