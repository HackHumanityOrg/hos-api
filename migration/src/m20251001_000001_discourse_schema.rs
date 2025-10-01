use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create discourse schema
        let db = manager.get_connection();
        db.execute_unprepared("CREATE SCHEMA IF NOT EXISTS discourse")
            .await?;

        // discourse_instances
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("discourse"), DiscourseInstances::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscourseInstances::Key)
                            .string_len(100)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscourseInstances::BaseUrl)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseInstances::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(DiscourseInstances::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(DiscourseInstances::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // discourse_categories
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("discourse"), DiscourseCategories::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscourseCategories::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscourseCategories::Instance)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseCategories::ExternalId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseCategories::Name).text().not_null())
                    .col(ColumnDef::new(DiscourseCategories::Color).text().not_null())
                    .col(
                        ColumnDef::new(DiscourseCategories::TextColor)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseCategories::Slug).text().not_null())
                    .col(
                        ColumnDef::new(DiscourseCategories::TopicCount)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseCategories::PostCount)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseCategories::Description).text())
                    .col(ColumnDef::new(DiscourseCategories::DescriptionText).text())
                    .col(ColumnDef::new(DiscourseCategories::TopicsDay).integer())
                    .col(ColumnDef::new(DiscourseCategories::TopicsWeek).integer())
                    .col(ColumnDef::new(DiscourseCategories::TopicsMonth).integer())
                    .col(ColumnDef::new(DiscourseCategories::TopicsYear).integer())
                    .col(ColumnDef::new(DiscourseCategories::TopicsAllTime).integer())
                    .col(ColumnDef::new(DiscourseCategories::ParentCategoryId).integer())
                    .col(ColumnDef::new(DiscourseCategories::Position).integer().default(0))
                    .col(ColumnDef::new(DiscourseCategories::ReadRestricted).boolean().default(false))
                    .to_owned(),
            )
            .await?;

        // FK: categories.instance -> instances.key
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_categories_instance_instances")
                    .from(
                        (Alias::new("discourse"), DiscourseCategories::Table),
                        DiscourseCategories::Instance,
                    )
                    .to(
                        (Alias::new("discourse"), DiscourseInstances::Table),
                        DiscourseInstances::Key,
                    )
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;

        let _ = manager
            .create_index(
                Index::create()
                    .name("ux_categories_instance_ext")
                    .table((Alias::new("discourse"), DiscourseCategories::Table))
                    .col(DiscourseCategories::Instance)
                    .col(DiscourseCategories::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ix_categories_instance_slug")
                    .table((Alias::new("discourse"), DiscourseCategories::Table))
                    .col(DiscourseCategories::Instance)
                    .col(DiscourseCategories::Slug)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ix_categories_parent_category")
                    .table((Alias::new("discourse"), DiscourseCategories::Table))
                    .col(DiscourseCategories::ParentCategoryId)
                    .to_owned(),
            )
            .await;
        // Note: Self-referential FK removed because parent_category_id references external_id,
        // but there's only a composite unique constraint on (instance, external_id).
        // Parent categories are always within the same instance, so this is enforced at the application level.

        // discourse_users
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("discourse"), DiscourseUsers::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscourseUsers::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscourseUsers::Instance)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseUsers::ExternalId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseUsers::Username).text().not_null())
                    .col(ColumnDef::new(DiscourseUsers::Name).text())
                    .col(
                        ColumnDef::new(DiscourseUsers::AvatarTemplate)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseUsers::Title).text())
                    .col(ColumnDef::new(DiscourseUsers::LikesReceived).big_integer())
                    .col(ColumnDef::new(DiscourseUsers::LikesGiven).big_integer())
                    .col(ColumnDef::new(DiscourseUsers::TopicsEntered).big_integer())
                    .col(ColumnDef::new(DiscourseUsers::TopicCount).big_integer())
                    .col(ColumnDef::new(DiscourseUsers::PostCount).big_integer())
                    .col(ColumnDef::new(DiscourseUsers::PostsRead).big_integer())
                    .col(ColumnDef::new(DiscourseUsers::DaysVisited).big_integer())
                    .col(ColumnDef::new(DiscourseUsers::TrustLevel).integer().default(0))
                    .to_owned(),
            )
            .await?;

        // FK: users.instance -> instances.key
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_users_instance_instances")
                    .from(
                        (Alias::new("discourse"), DiscourseUsers::Table),
                        DiscourseUsers::Instance,
                    )
                    .to(
                        (Alias::new("discourse"), DiscourseInstances::Table),
                        DiscourseInstances::Key,
                    )
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ux_users_instance_ext")
                    .table((Alias::new("discourse"), DiscourseUsers::Table))
                    .col(DiscourseUsers::Instance)
                    .col(DiscourseUsers::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ux_users_instance_username")
                    .table((Alias::new("discourse"), DiscourseUsers::Table))
                    .col(DiscourseUsers::Instance)
                    .col(DiscourseUsers::Username)
                    .unique()
                    .to_owned(),
            )
            .await;

        // discourse_topics
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("discourse"), DiscourseTopics::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscourseTopics::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscourseTopics::Instance)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseTopics::ExternalId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseTopics::Title).text().not_null())
                    .col(
                        ColumnDef::new(DiscourseTopics::FancyTitle)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseTopics::Slug).text().not_null())
                    .col(
                        ColumnDef::new(DiscourseTopics::PostsCount)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseTopics::ReplyCount)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseTopics::CreatedAt)
                            .timestamp()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseTopics::LastPostedAt)
                            .timestamp()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseTopics::BumpedAt)
                            .timestamp()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseTopics::Pinned).boolean().not_null())
                    .col(
                        ColumnDef::new(DiscourseTopics::PinnedGlobally)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseTopics::Visible)
                            .boolean()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseTopics::Closed).boolean().not_null())
                    .col(
                        ColumnDef::new(DiscourseTopics::Archived)
                            .boolean()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseTopics::Views).integer().not_null())
                    .col(
                        ColumnDef::new(DiscourseTopics::LikeCount)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscourseTopics::ExternalCategoryId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscourseTopics::HighestPostNumber).integer().not_null())
                    .col(ColumnDef::new(DiscourseTopics::Archetype).string_len(50).not_null())
                    .col(ColumnDef::new(DiscourseTopics::ImageUrl).text())
                    .to_owned(),
            )
            .await?;

        // FK: topics.instance -> instances.key
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_topics_instance_instances")
                    .from(
                        (Alias::new("discourse"), DiscourseTopics::Table),
                        DiscourseTopics::Instance,
                    )
                    .to(
                        (Alias::new("discourse"), DiscourseInstances::Table),
                        DiscourseInstances::Key,
                    )
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_topics_category")
                    .from(
                        (Alias::new("discourse"), DiscourseTopics::Table),
                        DiscourseTopics::Instance,
                    )
                    .from_col(DiscourseTopics::ExternalCategoryId)
                    .to(
                        (Alias::new("discourse"), DiscourseCategories::Table),
                        DiscourseCategories::Instance,
                    )
                    .to_col(DiscourseCategories::ExternalId)
                    .on_delete(ForeignKeyAction::Restrict)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ux_topics_instance_ext")
                    .table((Alias::new("discourse"), DiscourseTopics::Table))
                    .col(DiscourseTopics::Instance)
                    .col(DiscourseTopics::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ix_topics_instance_category")
                    .table((Alias::new("discourse"), DiscourseTopics::Table))
                    .col(DiscourseTopics::Instance)
                    .col(DiscourseTopics::ExternalCategoryId)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ix_topics_instance_slug")
                    .table((Alias::new("discourse"), DiscourseTopics::Table))
                    .col(DiscourseTopics::Instance)
                    .col(DiscourseTopics::Slug)
                    .to_owned(),
            )
            .await;

        // discourse_posts
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("discourse"), DiscoursePosts::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscoursePosts::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePosts::Instance)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePosts::ExternalId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscoursePosts::Name).text())
                    .col(ColumnDef::new(DiscoursePosts::Username).text().not_null())
                    .col(
                        ColumnDef::new(DiscoursePosts::CreatedAt)
                            .timestamp()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePosts::PostNumber)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePosts::PostType)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePosts::UpdatedAt)
                            .timestamp()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePosts::ReplyCount)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscoursePosts::ReplyToPostNumber).integer())
                    .col(
                        ColumnDef::new(DiscoursePosts::QuoteCount)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePosts::IncomingLinkCount)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscoursePosts::Reads).integer().not_null())
                    .col(
                        ColumnDef::new(DiscoursePosts::ReadersCount)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscoursePosts::Score).double().not_null())
                    .col(
                        ColumnDef::new(DiscoursePosts::ExternalTopicId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscoursePosts::TopicSlug).text().not_null())
                    .col(ColumnDef::new(DiscoursePosts::DisplayUsername).text())
                    .col(ColumnDef::new(DiscoursePosts::PrimaryGroupName).text())
                    .col(ColumnDef::new(DiscoursePosts::FlairName).text())
                    .col(ColumnDef::new(DiscoursePosts::FlairUrl).text())
                    .col(ColumnDef::new(DiscoursePosts::FlairBgColor).text())
                    .col(ColumnDef::new(DiscoursePosts::FlairColor).text())
                    .col(ColumnDef::new(DiscoursePosts::Version).integer().not_null())
                    .col(
                        ColumnDef::new(DiscoursePosts::ExternalUserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePosts::CanViewEditHistory)
                            .boolean()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscoursePosts::Deleted).boolean().not_null())
                    .col(ColumnDef::new(DiscoursePosts::Raw).text())
                    .to_owned(),
            )
            .await?;

        // FK: posts.instance -> instances.key
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_posts_instance_instances")
                    .from(
                        (Alias::new("discourse"), DiscoursePosts::Table),
                        DiscoursePosts::Instance,
                    )
                    .to(
                        (Alias::new("discourse"), DiscourseInstances::Table),
                        DiscourseInstances::Key,
                    )
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_posts_topic")
                    .from(
                        (Alias::new("discourse"), DiscoursePosts::Table),
                        DiscoursePosts::Instance,
                    )
                    .from_col(DiscoursePosts::ExternalTopicId)
                    .to(
                        (Alias::new("discourse"), DiscourseTopics::Table),
                        DiscourseTopics::Instance,
                    )
                    .to_col(DiscourseTopics::ExternalId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::NoAction)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_posts_user")
                    .from(
                        (Alias::new("discourse"), DiscoursePosts::Table),
                        DiscoursePosts::Instance,
                    )
                    .from_col(DiscoursePosts::ExternalUserId)
                    .to(
                        (Alias::new("discourse"), DiscourseUsers::Table),
                        DiscourseUsers::Instance,
                    )
                    .to_col(DiscourseUsers::ExternalId)
                    .on_delete(ForeignKeyAction::Restrict)
                    .on_update(ForeignKeyAction::NoAction)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ux_posts_instance_ext")
                    .table((Alias::new("discourse"), DiscoursePosts::Table))
                    .col(DiscoursePosts::Instance)
                    .col(DiscoursePosts::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ix_posts_instance_topic_created_at")
                    .table((Alias::new("discourse"), DiscoursePosts::Table))
                    .col(DiscoursePosts::Instance)
                    .col(DiscoursePosts::ExternalTopicId)
                    .col(DiscoursePosts::CreatedAt)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ix_posts_instance_user")
                    .table((Alias::new("discourse"), DiscoursePosts::Table))
                    .col(DiscoursePosts::Instance)
                    .col(DiscoursePosts::ExternalUserId)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ix_posts_instance_created_at")
                    .table((Alias::new("discourse"), DiscoursePosts::Table))
                    .col(DiscoursePosts::Instance)
                    .col(DiscoursePosts::CreatedAt)
                    .to_owned(),
            )
            .await;

        // discourse_post_revisions
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("discourse"), DiscoursePostRevisions::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscoursePostRevisions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePostRevisions::Instance)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePostRevisions::ExternalPostId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePostRevisions::Version)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePostRevisions::CreatedAt)
                            .timestamp()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePostRevisions::Username)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(DiscoursePostRevisions::EditReason).text())
                    .col(ColumnDef::new(DiscoursePostRevisions::RawBodyBefore).text())
                    .col(ColumnDef::new(DiscoursePostRevisions::RawBodyAfter).text())
                    .col(ColumnDef::new(DiscoursePostRevisions::RawTitleBefore).text())
                    .col(ColumnDef::new(DiscoursePostRevisions::RawTitleAfter).text())
                    .to_owned(),
            )
            .await?;

        // FK: revisions.instance -> instances.key
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_revisions_instance_instances")
                    .from(
                        (Alias::new("discourse"), DiscoursePostRevisions::Table),
                        DiscoursePostRevisions::Instance,
                    )
                    .to(
                        (Alias::new("discourse"), DiscourseInstances::Table),
                        DiscourseInstances::Key,
                    )
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_revisions_post")
                    .from(
                        (Alias::new("discourse"), DiscoursePostRevisions::Table),
                        DiscoursePostRevisions::Instance,
                    )
                    .from_col(DiscoursePostRevisions::ExternalPostId)
                    .to(
                        (Alias::new("discourse"), DiscoursePosts::Table),
                        DiscoursePosts::Instance,
                    )
                    .to_col(DiscoursePosts::ExternalId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ux_revisions_instance_post_revision")
                    .table((Alias::new("discourse"), DiscoursePostRevisions::Table))
                    .col(DiscoursePostRevisions::Instance)
                    .col(DiscoursePostRevisions::ExternalPostId)
                    .col(DiscoursePostRevisions::Version)
                    .unique()
                    .to_owned(),
            )
            .await;

        // discourse_post_likes
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("discourse"), DiscoursePostLikes::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DiscoursePostLikes::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePostLikes::Instance)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePostLikes::ExternalPostId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePostLikes::ExternalUserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DiscoursePostLikes::CreatedAt)
                            .timestamp()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // FK: likes.instance -> instances.key
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_likes_instance_instances")
                    .from(
                        (Alias::new("discourse"), DiscoursePostLikes::Table),
                        DiscoursePostLikes::Instance,
                    )
                    .to(
                        (Alias::new("discourse"), DiscourseInstances::Table),
                        DiscourseInstances::Key,
                    )
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_likes_post")
                    .from(
                        (Alias::new("discourse"), DiscoursePostLikes::Table),
                        DiscoursePostLikes::Instance,
                    )
                    .from_col(DiscoursePostLikes::ExternalPostId)
                    .to(
                        (Alias::new("discourse"), DiscoursePosts::Table),
                        DiscoursePosts::Instance,
                    )
                    .to_col(DiscoursePosts::ExternalId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_foreign_key(
                sea_query::ForeignKeyCreateStatement::new()
                    .name("fk_likes_user")
                    .from(
                        (Alias::new("discourse"), DiscoursePostLikes::Table),
                        DiscoursePostLikes::Instance,
                    )
                    .from_col(DiscoursePostLikes::ExternalUserId)
                    .to(
                        (Alias::new("discourse"), DiscourseUsers::Table),
                        DiscourseUsers::Instance,
                    )
                    .to_col(DiscourseUsers::ExternalId)
                    .on_delete(ForeignKeyAction::NoAction)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ux_likes_instance_post_user")
                    .table((Alias::new("discourse"), DiscoursePostLikes::Table))
                    .col(DiscoursePostLikes::Instance)
                    .col(DiscoursePostLikes::ExternalPostId)
                    .col(DiscoursePostLikes::ExternalUserId)
                    .unique()
                    .to_owned(),
            )
            .await;
        let _ = manager
            .create_index(
                Index::create()
                    .name("ix_likes_instance_user")
                    .table((Alias::new("discourse"), DiscoursePostLikes::Table))
                    .col(DiscoursePostLikes::Instance)
                    .col(DiscoursePostLikes::ExternalUserId)
                    .to_owned(),
            )
            .await;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("discourse"), DiscoursePostLikes::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("discourse"), DiscoursePostRevisions::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("discourse"), DiscoursePosts::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("discourse"), DiscourseTopics::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("discourse"), DiscourseUsers::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("discourse"), DiscourseCategories::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("discourse"), DiscourseInstances::Table))
                    .to_owned(),
            )
            .await?;

        // Drop schema
        let db = manager.get_connection();
        db.execute_unprepared("DROP SCHEMA IF EXISTS discourse CASCADE")
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum DiscourseInstances {
    Table,
    Key,
    BaseUrl,
    Enabled,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum DiscourseCategories {
    Table,
    Id,
    Instance,
    ExternalId,
    Name,
    Color,
    TextColor,
    Slug,
    TopicCount,
    PostCount,
    Description,
    DescriptionText,
    TopicsDay,
    TopicsWeek,
    TopicsMonth,
    TopicsYear,
    TopicsAllTime,
    ParentCategoryId,
    Position,
    ReadRestricted,
}

#[derive(DeriveIden)]
enum DiscourseUsers {
    Table,
    Id,
    Instance,
    ExternalId,
    Username,
    Name,
    AvatarTemplate,
    Title,
    LikesReceived,
    LikesGiven,
    TopicsEntered,
    TopicCount,
    PostCount,
    PostsRead,
    DaysVisited,
    TrustLevel,
}

#[derive(DeriveIden)]
enum DiscourseTopics {
    Table,
    Id,
    Instance,
    ExternalId,
    Title,
    FancyTitle,
    Slug,
    PostsCount,
    ReplyCount,
    CreatedAt,
    LastPostedAt,
    BumpedAt,
    Pinned,
    PinnedGlobally,
    Visible,
    Closed,
    Archived,
    Views,
    LikeCount,
    ExternalCategoryId,
    HighestPostNumber,
    Archetype,
    ImageUrl,
}

#[derive(DeriveIden)]
enum DiscoursePosts {
    Table,
    Id,
    Instance,
    ExternalId,
    Name,
    Username,
    CreatedAt,
    PostNumber,
    PostType,
    UpdatedAt,
    ReplyCount,
    ReplyToPostNumber,
    QuoteCount,
    IncomingLinkCount,
    Reads,
    ReadersCount,
    Score,
    ExternalTopicId,
    TopicSlug,
    DisplayUsername,
    PrimaryGroupName,
    FlairName,
    FlairUrl,
    FlairBgColor,
    FlairColor,
    Version,
    ExternalUserId,
    CanViewEditHistory,
    Deleted,
    Raw,
}

#[derive(DeriveIden)]
enum DiscoursePostRevisions {
    Table,
    Id,
    Instance,
    ExternalPostId,
    Version,
    CreatedAt,
    Username,
    EditReason,
    RawBodyBefore,
    RawBodyAfter,
    RawTitleBefore,
    RawTitleAfter,
}

#[derive(DeriveIden)]
enum DiscoursePostLikes {
    Table,
    Id,
    Instance,
    ExternalPostId,
    ExternalUserId,
    CreatedAt,
}
