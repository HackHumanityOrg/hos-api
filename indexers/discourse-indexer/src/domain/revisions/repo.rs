use crate::domain::revisions::model::Revision;
use crate::domain::util::bool_i8;
use anyhow::Result;
use entities::discourse_post_revisions;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};
use tracing::{debug, instrument};

#[instrument(skip(db, revision), fields(instance = instance, external_post_id = external_post_id, current_version = revision.current_version))]
pub async fn upsert_revision(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    external_post_id: i32,
    revision: &Revision,
) -> Result<()> {
    let am = discourse_post_revisions::ActiveModel {
        instance: Set(instance.to_string()),
        external_post_id: Set(external_post_id),
        created_at: Set(revision.created_at),
        username: Set(revision.username.clone()),
        acting_user_name: Set(revision.acting_user_name.clone()),
        display_username: Set(revision.display_username.clone()),
        avatar_template: Set(revision.avatar_template.clone()),
        edit_reason: Set(revision.edit_reason.clone()),
        body_changes: Set(revision.body_changes.as_ref().map(|b| serde_json::json!(b))),
        title_changes: Set(revision
            .title_changes
            .as_ref()
            .map(|t| serde_json::json!(t))),
        previous_category_id: Set(revision.previous_category_id),
        current_category_id: Set(revision.current_category_id),
        previous_user_id: Set(revision.previous_user_id),
        current_user_id: Set(revision.current_user_id),
        previous_hidden: Set(bool_i8(revision.previous_hidden)),
        current_hidden: Set(bool_i8(revision.current_hidden)),
        first_revision: Set(revision.first_revision),
        previous_revision: Set(revision.previous_revision),
        current_revision: Set(revision.current_revision),
        next_revision: Set(revision.next_revision),
        last_revision: Set(revision.last_revision),
        current_version: Set(revision.current_version),
        version_count: Set(revision.version_count),
        ..Default::default()
    };
    discourse_post_revisions::Entity::insert(am)
        .on_conflict(
            OrmOnConflict::columns([
                discourse_post_revisions::Column::Instance,
                discourse_post_revisions::Column::ExternalPostId,
                discourse_post_revisions::Column::CurrentVersion,
            ])
            .update_columns([
                discourse_post_revisions::Column::CreatedAt,
                discourse_post_revisions::Column::Username,
                discourse_post_revisions::Column::ActingUserName,
                discourse_post_revisions::Column::DisplayUsername,
                discourse_post_revisions::Column::AvatarTemplate,
                discourse_post_revisions::Column::EditReason,
                discourse_post_revisions::Column::BodyChanges,
                discourse_post_revisions::Column::TitleChanges,
                discourse_post_revisions::Column::PreviousCategoryId,
                discourse_post_revisions::Column::CurrentCategoryId,
                discourse_post_revisions::Column::PreviousUserId,
                discourse_post_revisions::Column::CurrentUserId,
                discourse_post_revisions::Column::PreviousHidden,
                discourse_post_revisions::Column::CurrentHidden,
                discourse_post_revisions::Column::FirstRevision,
                discourse_post_revisions::Column::PreviousRevision,
                discourse_post_revisions::Column::CurrentRevision,
                discourse_post_revisions::Column::NextRevision,
                discourse_post_revisions::Column::LastRevision,
                discourse_post_revisions::Column::VersionCount,
            ])
            .to_owned(),
        )
        .exec(db)
        .await?;
    debug!("upserted revision");
    Ok(())
}
