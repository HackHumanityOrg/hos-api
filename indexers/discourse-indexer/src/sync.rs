use crate::discourse_api;
use crate::domain;
use anyhow::Result;
use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{info, warn};

#[derive(Clone)]
pub struct Concurrency {
    pub post_tasks: Arc<Semaphore>,
    pub likes: Arc<Semaphore>,
    pub revisions: Arc<Semaphore>,
}

impl Concurrency {
    pub fn new(post_tasks: usize, likes: usize, revisions: usize) -> Self {
        Self {
            post_tasks: Arc::new(Semaphore::new(post_tasks.max(1))),
            likes: Arc::new(Semaphore::new(likes.max(1))),
            revisions: Arc::new(Semaphore::new(revisions.max(1))),
        }
    }
}

#[tracing::instrument(skip(db, client, conc))]
pub async fn sync_recent(
    db: Arc<DatabaseConnection>,
    client: &discourse_api::DiscourseClient,
    instance: &str,
    recent_topic_cap: u32,
    conc: &Concurrency,
    max_pages_per_run: u32,
    recent_lookback_hours: i64,
) -> Result<()> {
    services_helpers::sync_categories(db.as_ref(), client, instance).await?;
    let options = services_helpers::TopicSyncOptions {
        mode: services_helpers::TopicSyncMode::Recent {
            topic_cap: recent_topic_cap,
            lookback_hours: Some(recent_lookback_hours),
        },
        concurrency: conc,
        max_pages_per_run,
    };
    services_helpers::sync_topics(db, client, instance, options).await
}

#[tracing::instrument(skip(db, client, conc))]
pub async fn sync_full(
    db: Arc<DatabaseConnection>,
    client: &discourse_api::DiscourseClient,
    instance: &str,
    conc: &Concurrency,
    max_pages_per_run: u32,
) -> Result<()> {
    services_helpers::sync_categories(db.as_ref(), client, instance).await?;
    let options = services_helpers::TopicSyncOptions {
        mode: services_helpers::TopicSyncMode::Full,
        concurrency: conc,
        max_pages_per_run,
    };
    services_helpers::sync_topics(db.clone(), client, instance, options).await?;
    services_helpers::sync_directory_users(db.as_ref(), client, instance, false, u32::MAX, u32::MAX)
        .await
}

mod services_helpers {
    use super::*;
    use reqwest::StatusCode;

    pub struct TopicSyncOptions<'a> {
        pub mode: TopicSyncMode,
        pub concurrency: &'a Concurrency,
        pub max_pages_per_run: u32,
    }

    pub enum TopicSyncMode {
        Recent {
            topic_cap: u32,
            lookback_hours: Option<i64>,
        },
        Full,
    }

    fn extract_http_status(err: &anyhow::Error) -> Option<StatusCode> {
        for cause in err.chain() {
            if let Some(status_err) = cause.downcast_ref::<discourse_api::HttpStatusError>() {
                return Some(status_err.status);
            }
            if let Some(status_err) = cause.downcast_ref::<discourse_api::MaxRetriesReachedError>()
            {
                return Some(status_err.status);
            }
        }
        None
    }

    fn is_not_found_error(err: &anyhow::Error) -> bool {
        if matches!(extract_http_status(err), Some(StatusCode::NOT_FOUND)) {
            return true;
        }

        let msg = err.to_string();
        msg.contains("404") || msg.contains("Not Found")
    }

    fn is_skippable_item_error(err: &anyhow::Error) -> bool {
        if let Some(status) = extract_http_status(err) {
            return matches!(
                status,
                StatusCode::FORBIDDEN | StatusCode::NOT_FOUND | StatusCode::GONE
            );
        }

        false
    }

    pub async fn sync_categories(
        db: &DatabaseConnection,
        client: &discourse_api::DiscourseClient,
        instance: &str,
    ) -> Result<()> {
        let start = std::time::Instant::now();
        let cats = domain::categories::adapter::fetch_categories_all(client, 50).await?;

        for c in cats.iter() {
            domain::categories::repo::upsert_category(db, instance, c).await?;
        }

        info!(
            count = cats.len(),
            duration_ms = start.elapsed().as_millis(),
            "Synced categories"
        );
        Ok(())
    }

    pub async fn sync_directory_users(
        db: &DatabaseConnection,
        client: &discourse_api::DiscourseClient,
        instance: &str,
        recent: bool,
        page_limit: u32,
        max_pages: u32,
    ) -> Result<()> {
        let start = std::time::Instant::now();
        let users =
            domain::users::adapter::fetch_users_directory(client, recent, page_limit, max_pages)
                .await?;

        for item in users.iter() {
            domain::users::repo::upsert_user(db, instance, item).await?;
        }

        info!(
            count = users.len(),
            recent = recent,
            page_limit = page_limit,
            max_pages = max_pages,
            duration_ms = start.elapsed().as_millis(),
            "Synced directory users"
        );
        Ok(())
    }

    pub async fn sync_topics(
        db: Arc<DatabaseConnection>,
        client: &discourse_api::DiscourseClient,
        instance: &str,
        options: TopicSyncOptions<'_>,
    ) -> Result<()> {
        use domain::topics::model::TopicResponse;
        let start = std::time::Instant::now();
        let mut page: u32 = 0;
        let mut prev_ids: Option<Vec<i32>> = None;
        let mut pages_fetched: u32 = 0;
        let mut topics_processed: u32 = 0;
        let (is_recent, recent_topic_cap, lookback_cutoff) = match options.mode {
            TopicSyncMode::Recent {
                topic_cap,
                lookback_hours,
            } => (
                true,
                topic_cap,
                lookback_hours.map(|h| chrono::Utc::now() - chrono::Duration::hours(h)),
            ),
            TopicSyncMode::Full => (false, 0, None),
        };
        let conc = options.concurrency;
        let max_pages_per_run = options.max_pages_per_run;

        info!(
            sync_type = if is_recent { "recent" } else { "full" },
            recent_topic_cap = recent_topic_cap,
            max_pages_per_run = max_pages_per_run,
            lookback_hours = lookback_cutoff.map(|ts| ts.to_string()),
            "Starting topics sync"
        );

        loop {
            // Global hard cap on pages for safety
            if pages_fetched >= max_pages_per_run {
                warn!(
                    pages_fetched = pages_fetched,
                    max_pages_per_run = max_pages_per_run,
                    "Reached maximum pages limit, stopping topic sync"
                );
                break;
            }

            let resp: TopicResponse = if is_recent {
                match domain::topics::adapter::fetch_topics_recent_page(client, page).await {
                    Ok(r) => r,
                    Err(e) => {
                        if is_not_found_error(&e) {
                            break;
                        }
                        return Err(e);
                    }
                }
            } else {
                match domain::topics::adapter::fetch_topics_created_page(client, page).await {
                    Ok(r) => r,
                    Err(e) => {
                        if is_not_found_error(&e) {
                            break;
                        }
                        return Err(e);
                    }
                }
            };

            pages_fetched += 1;
            if resp.topic_list.topics.is_empty() {
                info!(page = page, "No topics found on page, ending sync");
                break;
            }

            topics_processed += resp.topic_list.topics.len() as u32;

            // Log progress every 10 pages
            if pages_fetched % 10 == 0 {
                info!(
                    pages_fetched = pages_fetched,
                    topics_processed = topics_processed,
                    current_page = page,
                    sync_type = if is_recent { "recent" } else { "full" },
                    "Topics sync progress"
                );
            }

            // For full sync: stop if page content repeats (same IDs as previous page)
            if !is_recent {
                let ids: Vec<i32> = resp.topic_list.topics.iter().map(|t| t.id).collect();
                // Preserve order for strict comparison
                if let Some(prev) = &prev_ids {
                    if prev == &ids {
                        info!(page = page, "Found duplicate topics, ending full sync");
                        break;
                    }
                }
                prev_ids = Some(ids);
            }

            // Early-break for recent mode if all topics are outside lookback window
            if is_recent {
                if let Some(cutoff) = lookback_cutoff {
                    let all_old = resp.topic_list.topics.iter().all(|t| t.bumped_at < cutoff);
                    if all_old {
                        info!(
                            page = page,
                            cutoff = %cutoff,
                            "All topics older than lookback window, ending recent sync"
                        );
                        break;
                    }
                }
            }

            domain::topics::repo::upsert_topics_bulk(
                db.as_ref(),
                instance,
                &resp.topic_list.topics,
            )
            .await?;

            for t in resp.topic_list.topics {
                if let Err(e) = sync_posts_for_topic(
                    db.clone(),
                    client,
                    instance,
                    t.id,
                    is_recent,
                    conc,
                    max_pages_per_run,
                )
                .await
                {
                    if is_skippable_item_error(&e) {
                        warn!(
                            topic_id = t.id,
                            error = %e,
                            "Skipping topic due skippable posts error"
                        );
                        continue;
                    }

                    return Err(e);
                }
            }

            // Recent: cap to a few pages; Full: continue until empty or repeat
            if is_recent {
                let cap = if recent_topic_cap == 0 {
                    1
                } else {
                    recent_topic_cap
                };
                // Fix off-by-one: pages are 0-indexed; cap is number of pages to fetch
                if (page + 1) >= cap {
                    info!(
                        page = page + 1,
                        cap = cap,
                        "Reached recent topic cap, ending sync"
                    );
                    break;
                }
            }

            page += 1;
        }

        info!(
            sync_type = if is_recent { "recent" } else { "full" },
            pages_fetched = pages_fetched,
            topics_processed = topics_processed,
            duration_ms = start.elapsed().as_millis(),
            topics_per_sec = if start.elapsed().as_secs() > 0 {
                topics_processed as u64 / start.elapsed().as_secs()
            } else {
                topics_processed as u64
            },
            "Completed topics sync"
        );
        Ok(())
    }

    #[tracing::instrument(skip(db, client, conc), fields(topic_id = topic_id, recent = recent))]
    pub async fn sync_posts_for_topic(
        db: Arc<DatabaseConnection>,
        client: &discourse_api::DiscourseClient,
        instance: &str,
        topic_id: i32,
        recent: bool,
        conc: &Concurrency,
        max_pages_per_run: u32,
    ) -> Result<()> {
        use domain::posts::model::PostResponse;
        let start = std::time::Instant::now();
        let mut page: u32 = 0;
        let mut processed = 0i32;
        let mut total: Option<i32> = None;
        let mut prev_post_ids: Option<Vec<i32>> = None;
        let mut pages_fetched: u32 = 0;
        let mut likes_processed = 0u32;
        let mut revisions_processed = 0u32;
        let mut likes_skipped = 0u32;
        let mut revisions_skipped = 0u32;

        loop {
            if pages_fetched >= max_pages_per_run {
                break;
            }

            let r: PostResponse =
                match domain::posts::adapter::fetch_posts_page(client, topic_id, page).await {
                    Ok(r) => r,
                    Err(e) => {
                        if is_skippable_item_error(&e) {
                            warn!(
                                topic_id = topic_id,
                                page = page,
                                error = %e,
                                "Skipping topic posts due skippable status"
                            );
                            return Ok(());
                        }
                        if is_not_found_error(&e) {
                            break;
                        }
                        return Err(e);
                    }
                };

            pages_fetched += 1;
            if total.is_none() {
                total = r.posts_count;
            }

            if r.post_stream.posts.is_empty() {
                break;
            }

            // Stop if page content repeats (same post IDs as previous page), for both recent and full
            let ids: Vec<i32> = r.post_stream.posts.iter().map(|p| p.id).collect();
            if let Some(prev) = &prev_post_ids {
                if prev == &ids {
                    break;
                }
            }
            prev_post_ids = Some(ids);

            // Ensure all users referenced in posts exist in the database
            for post in &r.post_stream.posts {
                domain::users::adapter::fetch_user_by_username(
                    db.as_ref(),
                    client,
                    instance,
                    &post.username,
                    post.user_id,
                )
                .await?;
            }

            domain::posts::repo::upsert_posts_bulk(db.as_ref(), instance, &r.post_stream.posts)
                .await?;

            let mut futs: FuturesUnordered<BoxFuture<'static, (u32, u32, u32, u32)>> =
                FuturesUnordered::new();
            for p in r.post_stream.posts.iter().cloned() {
                let db_all = db.clone();
                let client_all = client.clone();
                let instance_all = instance.to_string();
                let likes_sem = conc.likes.clone();
                let rev_sem = conc.revisions.clone();
                let post_sem = conc.post_tasks.clone();

                futs.push(Box::pin(async move {
                    let mut post_likes_count = 0u32;
                    let mut post_revisions_count = 0u32;
                    let mut post_likes_skipped = 0u32;
                    let mut post_revisions_skipped = 0u32;

                    // Gate per-post work
                    let _post_permit = post_sem.acquire().await.unwrap();

                    // Likes - only fetch if post has likes (check actions_summary)
                    let has_likes = p
                        .actions_summary
                        .iter()
                        .any(|action| action.id == 2 && action.count > 0); // action_id 2 = likes

                    if has_likes {
                        let _permit = likes_sem.acquire().await.unwrap();
                        match domain::likes::adapter::fetch_post_likes(&client_all, p.id).await {
                            Ok(likes) => {
                                for like_user in &likes {
                                    if let Err(e) = domain::users::adapter::fetch_user_by_username(
                                        db_all.as_ref(),
                                        &client_all,
                                        &instance_all,
                                        &like_user.username,
                                        like_user.id,
                                    )
                                    .await
                                    {
                                        tracing::warn!(
                                            post_id = p.id,
                                            like_user = %like_user.username,
                                            error = %e,
                                            "Failed to upsert like user"
                                        );
                                    }
                                }

                                post_likes_count = likes.len() as u32;
                                if let Err(e) = domain::likes::repo::upsert_post_likes_batch(
                                    db_all.as_ref(),
                                    &instance_all,
                                    p.id,
                                    &likes,
                                )
                                .await
                                {
                                    tracing::warn!(
                                        post_id = p.id,
                                        error = %e,
                                        "Failed to upsert post likes"
                                    );
                                }
                            }
                            Err(e) => {
                                post_likes_skipped = 1;
                                if is_skippable_item_error(&e) {
                                    tracing::warn!(
                                        post_id = p.id,
                                        error = %e,
                                        "Skipping likes due skippable status"
                                    );
                                } else {
                                    tracing::warn!(
                                        post_id = p.id,
                                        error = %e,
                                        "Failed to fetch likes"
                                    );
                                }
                            }
                        }
                    } else {
                        post_likes_skipped = 1;
                        tracing::debug!(
                            post_id = p.id,
                            "Skipping likes fetch - no likes in actions_summary"
                        );
                    }

                    // Revisions - only fetch if post has revisions (version > 1 and can view history)
                    if p.can_view_edit_history && p.version > 1 {
                        let _permit = rev_sem.acquire().await.unwrap();
                        for ver in 2..=p.version {
                            match domain::revisions::adapter::fetch_post_revision(
                                &client_all,
                                p.id,
                                ver,
                            )
                            .await
                            {
                                Ok(rev) => {
                                    post_revisions_count += 1;
                                    if let Err(e) = domain::revisions::repo::upsert_revision(
                                        db_all.as_ref(),
                                        &instance_all,
                                        p.id,
                                        &rev,
                                    )
                                    .await
                                    {
                                        tracing::warn!(
                                            post_id = p.id,
                                            version = ver,
                                            error = %e,
                                            "Failed to upsert post revision"
                                        );
                                    }
                                }
                                Err(e) => {
                                    post_revisions_skipped = 1;
                                    if is_skippable_item_error(&e) {
                                        tracing::warn!(
                                            post_id = p.id,
                                            version = ver,
                                            error = %e,
                                            "Skipping revision due skippable status"
                                        );
                                        break;
                                    }

                                    tracing::warn!(
                                        post_id = p.id,
                                        version = ver,
                                        error = %e,
                                        "Failed to fetch revision"
                                    );
                                }
                            }
                        }
                    } else {
                        post_revisions_skipped = 1;
                        tracing::debug!(
                            post_id = p.id,
                            version = p.version,
                            can_view_history = p.can_view_edit_history,
                            "Skipping revisions fetch - no revisions available"
                        );
                    }

                    (
                        post_likes_count,
                        post_revisions_count,
                        post_likes_skipped,
                        post_revisions_skipped,
                    )
                }));
            }

            while let Some((post_likes, post_revisions, likes_skip, revisions_skip)) =
                futs.next().await
            {
                likes_processed += post_likes;
                revisions_processed += post_revisions;
                likes_skipped += likes_skip;
                revisions_skipped += revisions_skip;
            }

            processed += r.post_stream.posts.len() as i32;
            if let Some(total) = total {
                if processed >= total {
                    break;
                }
            }
            page += 1;
        }

        info!(
            topic_id = topic_id,
            posts_processed = processed,
            likes_processed = likes_processed,
            likes_skipped = likes_skipped,
            revisions_processed = revisions_processed,
            revisions_skipped = revisions_skipped,
            pages_fetched = pages_fetched,
            duration_ms = start.elapsed().as_millis(),
            "Completed posts sync for topic"
        );
        Ok(())
    }
}
