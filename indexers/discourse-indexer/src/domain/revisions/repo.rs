use crate::domain::revisions::model::Revision;
use anyhow::Result;
use entities::discourse::discourse_post_revisions;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};
use tracing::{debug, instrument};

#[instrument(skip(db, revision), fields(instance = instance, external_post_id = external_post_id, version = revision.current_version))]
pub async fn upsert_revision(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    external_post_id: i32,
    revision: &Revision,
) -> Result<()> {
    let raw_body_before = revision.get_raw_markdown_before();
    let raw_body_after = revision.get_raw_markdown_after();
    let raw_title_before = revision
        .title_changes
        .as_ref()
        .map(|tc| tc.get_raw_title_before());
    let raw_title_after = revision
        .title_changes
        .as_ref()
        .map(|tc| tc.get_raw_title_after());

    let am = discourse_post_revisions::ActiveModel {
        instance: Set(instance.to_string()),
        external_post_id: Set(external_post_id),
        version: Set(revision.current_version),
        created_at: Set(revision.created_at.naive_utc()),
        username: Set(revision.username.clone()),
        edit_reason: Set(revision.edit_reason.clone()),
        raw_body_before: Set(Some(raw_body_before)),
        raw_body_after: Set(Some(raw_body_after)),
        raw_title_before: Set(raw_title_before),
        raw_title_after: Set(raw_title_after),
        ..Default::default()
    };
    discourse_post_revisions::Entity::insert(am.clone())
        .on_conflict(
            OrmOnConflict::columns([
                discourse_post_revisions::Column::Instance,
                discourse_post_revisions::Column::ExternalPostId,
                discourse_post_revisions::Column::Version,
            ])
            .update_columns([
                discourse_post_revisions::Column::CreatedAt,
                discourse_post_revisions::Column::Username,
                discourse_post_revisions::Column::EditReason,
                discourse_post_revisions::Column::RawBodyBefore,
                discourse_post_revisions::Column::RawBodyAfter,
                discourse_post_revisions::Column::RawTitleBefore,
                discourse_post_revisions::Column::RawTitleAfter,
            ])
            .to_owned(),
        )
        .exec(db)
        .await?;
    debug!("upserted revision");
    Ok(())
}
