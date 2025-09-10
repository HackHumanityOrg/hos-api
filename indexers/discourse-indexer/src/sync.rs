use crate::discourse_api;
use crate::domain;
use anyhow::Result;

use std::sync::Arc;
use tokio::sync::Semaphore;

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
    db: &sea_orm::DatabaseConnection,
    client: &discourse_api::DiscourseClient,
    instance: &str,
    recent_topic_cap: u32,
    conc: &Concurrency,
    max_pages_per_run: u32,
    recent_lookback_hours: i64,
) -> Result<()> {
    // categories and recent users
    services_helpers::sync_categories(db, client, instance).await?;
    services_helpers::sync_topics(
        db,
        client,
        instance,
        true,
        recent_topic_cap,
        conc,
        max_pages_per_run,
        Some(recent_lookback_hours),
    )
    .await
}

#[tracing::instrument(skip(db, client, conc))]
pub async fn sync_full(
    db: &sea_orm::DatabaseConnection,
    client: &discourse_api::DiscourseClient,
    instance: &str,
    conc: &Concurrency,
    max_pages_per_run: u32,
) -> Result<()> {
    services_helpers::sync_categories(db, client, instance).await?;
    services_helpers::sync_directory_users(db, client, instance, false, u32::MAX, u32::MAX).await?;
    services_helpers::sync_topics(
        db,
        client,
        instance,
        false,
        0,
        conc,
        max_pages_per_run,
        None,
    )
    .await
}

mod services_helpers {
    use super::*;
    use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};

    // Caps are provided via parameters from config

    pub async fn sync_categories(
        db: &sea_orm::DatabaseConnection,
        client: &discourse_api::DiscourseClient,
        instance: &str,
    ) -> Result<()> {
        let cats = crate::domain::categories::adapter::fetch_categories_all(client, 50).await?;
        for c in cats.iter() {
            crate::domain::categories::repo::upsert_category(db, instance, c).await?;
        }
        Ok(())
    }

    pub async fn sync_directory_users(
        db: &sea_orm::DatabaseConnection,
        client: &discourse_api::DiscourseClient,
        instance: &str,
        recent: bool,
        page_limit: u32,
        max_pages: u32,
    ) -> Result<()> {
        let users = crate::domain::users::adapter::fetch_users_directory(
            client, recent, page_limit, max_pages,
        )
        .await?;
        for item in users.iter() {
            crate::domain::users::repo::upsert_user(db, instance, item).await?;
        }
        Ok(())
    }

    pub async fn sync_topics(
        db: &sea_orm::DatabaseConnection,
        client: &discourse_api::DiscourseClient,
        instance: &str,
        recent: bool,
        recent_topic_cap: u32,
        conc: &Concurrency,
        max_pages_per_run: u32,
        recent_lookback_hours: Option<i64>,
    ) -> Result<()> {
        use crate::domain::topics::model::TopicResponse;
        let mut page: u32 = 0;
        let mut prev_ids: Option<Vec<i32>> = None;
        let mut pages_fetched: u32 = 0;
        let lookback_cutoff = if recent {
            recent_lookback_hours.map(|h| chrono::Utc::now() - chrono::Duration::hours(h))
        } else {
            None
        };
        loop {
            // Global hard cap on pages for safety
            if pages_fetched >= max_pages_per_run {
                break;
            }
            let resp: TopicResponse = if recent {
                match domain::topics::adapter::fetch_topics_recent_page(client, page).await {
                    Ok(r) => r,
                    Err(e) => {
                        if let Some(st) = e.downcast_ref::<crate::discourse_api::HttpStatusError>()
                        {
                            if st.status == reqwest::StatusCode::NOT_FOUND {
                                break;
                            }
                        }
                        return Err(e);
                    }
                }
            } else {
                match domain::topics::adapter::fetch_topics_created_page(client, page).await {
                    Ok(r) => r,
                    Err(e) => {
                        if let Some(st) = e.downcast_ref::<crate::discourse_api::HttpStatusError>()
                        {
                            if st.status == reqwest::StatusCode::NOT_FOUND {
                                break;
                            }
                        }
                        return Err(e);
                    }
                }
            };
            pages_fetched += 1;
            if resp.topic_list.topics.is_empty() {
                break;
            }
            // For full sync: stop if page content repeats (same IDs as previous page)
            if !recent {
                let ids: Vec<i32> = resp.topic_list.topics.iter().map(|t| t.id).collect();
                // Preserve order for strict comparison
                if let Some(prev) = &prev_ids {
                    if prev == &ids {
                        break;
                    }
                }
                prev_ids = Some(ids);
            }
            // Early-break for recent mode if all topics are outside lookback window
            if recent {
                if let Some(cutoff) = lookback_cutoff {
                    let all_old = resp.topic_list.topics.iter().all(|t| t.bumped_at < cutoff);
                    if all_old {
                        break;
                    }
                }
            }
            domain::topics::repo::upsert_topics_bulk(db, instance, &resp.topic_list.topics).await?;
            for t in resp.topic_list.topics {
                sync_posts_for_topic(db, client, instance, t.id, recent, conc, max_pages_per_run)
                    .await?;
            }
            // Recent: cap to a few pages; Full: continue until empty or repeat
            if recent {
                let cap = if recent_topic_cap == 0 {
                    1
                } else {
                    recent_topic_cap
                };
                // Fix off-by-one: pages are 0-indexed; cap is number of pages to fetch
                if (page + 1) >= cap {
                    break;
                }
            }
            page += 1;
        }
        Ok(())
    }

    #[tracing::instrument(skip(db, client, conc))]
    pub async fn sync_posts_for_topic(
        db: &sea_orm::DatabaseConnection,
        client: &discourse_api::DiscourseClient,
        instance: &str,
        topic_id: i32,
        recent: bool,
        conc: &Concurrency,
        max_pages_per_run: u32,
    ) -> Result<()> {
        use crate::domain::posts::model::PostResponse;
        let mut page: u32 = 0;
        let mut processed = 0i32;
        let mut total: Option<i32> = None;
        let mut prev_post_ids: Option<Vec<i32>> = None;
        let mut pages_fetched: u32 = 0;
        loop {
            if pages_fetched >= max_pages_per_run {
                break;
            }
            let r: PostResponse =
                match domain::posts::adapter::fetch_posts_page(client, topic_id, page).await {
                    Ok(r) => r,
                    Err(e) => {
                        if let Some(st) = e.downcast_ref::<crate::discourse_api::HttpStatusError>()
                        {
                            if st.status == reqwest::StatusCode::NOT_FOUND {
                                break;
                            }
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
            domain::posts::repo::upsert_posts_bulk(db, instance, &r.post_stream.posts).await?;

            let mut futs: FuturesUnordered<BoxFuture<'static, ()>> = FuturesUnordered::new();
            for p in r.post_stream.posts.iter().cloned() {
                let db_all = db.clone();
                let client_all = client.clone();
                let instance_key = instance.to_string();
                let likes_sem = conc.likes.clone();
                let rev_sem = conc.revisions.clone();
                let post_sem = conc.post_tasks.clone();
                futs.push(Box::pin(async move {
                    // Gate per-post work
                    let _post_permit = post_sem.acquire().await.unwrap();

                    // Likes
                    {
                        let _permit = likes_sem.acquire().await.unwrap();
                        if let Ok(likes) =
                            domain::likes::adapter::fetch_post_likes(&client_all, p.id).await
                        {
                            let _ = domain::likes::repo::upsert_post_likes_batch(
                                &db_all,
                                &instance_key,
                                p.id,
                                &likes,
                            )
                            .await;
                        }
                    }

                    // Revisions
                    if p.can_view_edit_history && p.version > 1 {
                        let _permit = rev_sem.acquire().await.unwrap();
                        for ver in 2..=p.version {
                            if let Ok(rev) = domain::revisions::adapter::fetch_post_revision(
                                &client_all,
                                p.id,
                                ver,
                            )
                            .await
                            {
                                let _ = domain::revisions::repo::upsert_revision(
                                    &db_all,
                                    &instance_key,
                                    p.id,
                                    &rev,
                                )
                                .await;
                            }
                        }
                    }
                }));
            }
            while (futs.next().await).is_some() {}

            processed += r.post_stream.posts.len() as i32;
            if let Some(total) = total {
                if processed >= total {
                    break;
                }
            }
            page += 1;
        }
        Ok(())
    }
}
