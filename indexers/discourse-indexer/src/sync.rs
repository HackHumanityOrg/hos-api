use crate::discourse_api;
use crate::domain;
use anyhow::Result;

use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct Concurrency {
    pub http: Arc<Semaphore>,
    pub post_tasks: Arc<Semaphore>,
    pub likes: Arc<Semaphore>,
    pub revisions: Arc<Semaphore>,
}

impl Concurrency {
    pub fn new(http_limit: usize, post_tasks: usize, likes: usize, revisions: usize) -> Self {
        Self {
            http: Arc::new(Semaphore::new(http_limit.max(1))),
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
    max_topic_pages: u32,
    max_post_pages: u32,
    conc: &Concurrency,
) -> Result<()> {
    // categories and recent users
    services_helpers::sync_categories(db, client, instance).await?;
    services_helpers::sync_directory_users(db, client, instance, true, 5, max_topic_pages).await?;
    services_helpers::sync_topics(
        db,
        client,
        instance,
        max_topic_pages,
        max_post_pages,
        true,
        conc,
    )
    .await
}

#[tracing::instrument(skip(db, client, conc))]
pub async fn sync_full(
    db: &sea_orm::DatabaseConnection,
    client: &discourse_api::DiscourseClient,
    instance: &str,
    max_topic_pages: u32,
    max_post_pages: u32,
    conc: &Concurrency,
) -> Result<()> {
    services_helpers::sync_categories(db, client, instance).await?;
    services_helpers::sync_directory_users(db, client, instance, false, u32::MAX, max_topic_pages)
        .await?;
    services_helpers::sync_topics(
        db,
        client,
        instance,
        max_topic_pages,
        max_post_pages,
        false,
        conc,
    )
    .await
}

mod services_helpers {
    use super::*;
    use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};

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
        max_topic_pages: u32,
        max_post_pages: u32,
        recent: bool,
        conc: &Concurrency,
    ) -> Result<()> {
        use crate::domain::topics::model::TopicResponse;
        let mut page: u32 = 0;
        loop {
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
            if resp.topic_list.topics.is_empty() {
                break;
            }
            domain::topics::repo::upsert_topics_bulk(db, instance, &resp.topic_list.topics).await?;
            for t in resp.topic_list.topics {
                sync_posts_for_topic(db, client, instance, t.id, max_post_pages, conc).await?;
            }
            if page >= max_topic_pages {
                break;
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
        max_pages: u32,
        conc: &Concurrency,
    ) -> Result<()> {
        use crate::domain::posts::model::PostResponse;
        let mut page: u32 = 0;
        let mut processed = 0i32;
        let mut total: Option<i32> = None;
        loop {
            if page > max_pages {
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
            if total.is_none() {
                total = r.posts_count;
            }
            if r.post_stream.posts.is_empty() {
                break;
            }
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
