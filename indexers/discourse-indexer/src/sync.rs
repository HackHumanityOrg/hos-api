use crate::discourse_api;
use crate::domain;
use anyhow::Result;
use chrono::Utc;
use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};
use sea_orm::DatabaseConnection;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{info, warn};

#[derive(Clone, Copy, Debug)]
pub struct MetricsRefreshConfig {
    pub cycle_interval_sec: u64,
    pub batch_size: u32,
    pub active_window_days: i64,
    pub active_refresh_hours: u64,
}

impl MetricsRefreshConfig {
    fn normalized(self) -> Self {
        Self {
            cycle_interval_sec: self.cycle_interval_sec.max(1),
            batch_size: self.batch_size.max(1),
            active_window_days: self.active_window_days.max(1),
            active_refresh_hours: self.active_refresh_hours.max(1),
        }
    }
}

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
    services_helpers::sync_topics(db, client, instance, options).await
}

#[tracing::instrument(skip(db, client))]
pub async fn sync_directory_full(
    db: Arc<DatabaseConnection>,
    client: &discourse_api::DiscourseClient,
    instance: &str,
    refresh: MetricsRefreshConfig,
) -> Result<()> {
    services_helpers::sync_directory_metrics_cycle(db.as_ref(), client, instance, refresh).await
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

    fn is_skippable_posts_page_error(err: &anyhow::Error) -> bool {
        if is_skippable_item_error(err) {
            return true;
        }

        matches!(extract_http_status(err), Some(StatusCode::BAD_REQUEST))
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

    fn slots_for_period(period_sec: u64, cycle_interval_sec: u64) -> u32 {
        let normalized_cycle = cycle_interval_sec.max(1);
        let slots = (period_sec / normalized_cycle).max(1);
        u32::try_from(slots).unwrap_or(u32::MAX)
    }

    fn cycle_tick(cycle_interval_sec: u64) -> i64 {
        let cycle = i64::try_from(cycle_interval_sec.max(1)).unwrap_or(i64::MAX);
        Utc::now().timestamp().div_euclid(cycle)
    }

    fn slot_for_tick(total_slots: u32, tick: i64) -> u32 {
        let slots = total_slots.max(1);
        tick.rem_euclid(i64::from(slots)) as u32
    }

    fn slot_for_external_id(external_id: i32, total_slots: u32) -> u32 {
        let slots_i32 = i32::try_from(total_slots.max(1)).unwrap_or(i32::MAX);
        external_id.rem_euclid(slots_i32) as u32
    }

    fn mix_seed(mut value: u64) -> u64 {
        value ^= value >> 33;
        value = value.wrapping_mul(0xff51afd7ed558ccd);
        value ^= value >> 33;
        value = value.wrapping_mul(0xc4ceb9fe1a85ec53);
        value ^= value >> 33;
        value
    }

    fn rank_for_cycle(external_id: i32, cycle_seed: u64) -> u64 {
        let id = external_id as u64;
        mix_seed(cycle_seed ^ id.wrapping_mul(0x9e3779b185ebca87))
    }

    fn select_by_cycle(
        mut candidates: Vec<domain::users::repo::MetricsRefreshUser>,
        take: usize,
        cycle_seed: u64,
    ) -> Vec<domain::users::repo::MetricsRefreshUser> {
        if candidates.is_empty() || take == 0 {
            return Vec::new();
        }

        candidates.sort_by_key(|u| rank_for_cycle(u.external_id, cycle_seed));
        candidates.into_iter().take(take).collect()
    }

    pub async fn sync_directory_metrics_cycle(
        db: &DatabaseConnection,
        client: &discourse_api::DiscourseClient,
        instance: &str,
        refresh: MetricsRefreshConfig,
    ) -> Result<()> {
        let refresh = refresh.normalized();
        let start = std::time::Instant::now();
        let all_users = domain::users::repo::list_metric_refresh_users(db, instance).await?;

        if all_users.is_empty() {
            info!(
                batch_size = refresh.batch_size,
                duration_ms = start.elapsed().as_millis(),
                "No users available for directory metrics refresh"
            );
            return Ok(());
        }

        let active_ids = domain::users::repo::list_active_user_ids_since(
            db,
            instance,
            refresh.active_window_days,
        )
        .await?;

        let current_tick = cycle_tick(refresh.cycle_interval_sec);
        let current_seed = u64::try_from(current_tick).unwrap_or(0);

        let daily_slots = slots_for_period(24 * 60 * 60, refresh.cycle_interval_sec);
        let daily_slot = slot_for_tick(daily_slots, current_tick);

        let active_slots = slots_for_period(
            refresh.active_refresh_hours.saturating_mul(60 * 60),
            refresh.cycle_interval_sec,
        );
        let active_slot = slot_for_tick(active_slots, current_tick);

        let mut daily_candidates = Vec::new();
        for user in &all_users {
            if slot_for_external_id(user.external_id, daily_slots) == daily_slot {
                daily_candidates.push(user.clone());
            }
        }

        let daily_ids: HashSet<i32> = daily_candidates.iter().map(|u| u.external_id).collect();
        let mut active_candidates = Vec::new();
        for user in &all_users {
            if !active_ids.contains(&user.external_id) || daily_ids.contains(&user.external_id) {
                continue;
            }
            if slot_for_external_id(user.external_id, active_slots) == active_slot {
                active_candidates.push(user.clone());
            }
        }

        let daily_pool_count = daily_candidates.len() as u64;
        let active_pool_count = active_candidates.len() as u64;

        let batch_limit = refresh.batch_size as usize;
        let mut selected = select_by_cycle(
            active_candidates,
            batch_limit,
            current_seed ^ 0xa5a5a5a5a5a5a5a5,
        );
        if selected.len() < batch_limit {
            selected.extend(select_by_cycle(
                daily_candidates,
                batch_limit - selected.len(),
                current_seed ^ 0x5a5a5a5a5a5a5a5a,
            ));
        }

        if selected.is_empty() {
            info!(
                batch_size = refresh.batch_size,
                total_users = all_users.len(),
                active_users = active_ids.len(),
                daily_slot = daily_slot,
                daily_slots = daily_slots,
                active_slot = active_slot,
                active_slots = active_slots,
                duration_ms = start.elapsed().as_millis(),
                "No users selected for directory metrics refresh this cycle"
            );
            return Ok(());
        }

        let selected_active = selected
            .iter()
            .filter(|u| active_ids.contains(&u.external_id))
            .count() as u64;
        let selected_daily = selected.len() as u64 - selected_active;

        let attempted = selected.len() as u64;
        let mut updated = 0u64;
        let mut not_found = 0u64;
        let mut failed = 0u64;

        for user in selected {
            match domain::users::adapter::fetch_directory_user_by_username(client, &user.username)
                .await
            {
                Ok(Some(item)) => {
                    if item.user.id != user.external_id {
                        not_found += 1;
                        warn!(
                            username = %user.username,
                            expected_external_id = user.external_id,
                            returned_external_id = item.user.id,
                            "Directory username lookup returned mismatched user id"
                        );
                        continue;
                    }

                    match domain::users::repo::upsert_user(db, instance, &item).await {
                        Ok(()) => {
                            updated += 1;
                        }
                        Err(e) => {
                            failed += 1;
                            warn!(
                                username = %user.username,
                                external_user_id = user.external_id,
                                error = %e,
                                "Failed to upsert user from directory username lookup"
                            );
                        }
                    }
                }
                Ok(None) => {
                    not_found += 1;
                    warn!(
                        username = %user.username,
                        external_user_id = user.external_id,
                        "No directory metrics found for username"
                    );
                }
                Err(e) => {
                    failed += 1;
                    warn!(
                        username = %user.username,
                        external_user_id = user.external_id,
                        error = %e,
                        "Directory username lookup failed"
                    );
                }
            }
        }

        info!(
            attempted = attempted,
            updated = updated,
            not_found = not_found,
            failed = failed,
            selected_active = selected_active,
            selected_daily = selected_daily,
            active_pool = active_pool_count,
            daily_pool = daily_pool_count,
            active_users = active_ids.len(),
            total_users = all_users.len(),
            daily_slot = daily_slot,
            daily_slots = daily_slots,
            active_slot = active_slot,
            active_slots = active_slots,
            batch_size = refresh.batch_size,
            duration_ms = start.elapsed().as_millis(),
            "Synced directory user metrics"
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
        let known_users_cache = domain::users::adapter::KnownUsersCache::new();

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
                    &known_users_cache,
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

    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(skip(db, client, known_users_cache, conc), fields(topic_id = topic_id, recent = recent))]
    pub async fn sync_posts_for_topic(
        db: Arc<DatabaseConnection>,
        client: &discourse_api::DiscourseClient,
        instance: &str,
        topic_id: i32,
        recent: bool,
        known_users_cache: &domain::users::adapter::KnownUsersCache,
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
                        if is_skippable_posts_page_error(&e) {
                            warn!(
                                topic_id = topic_id,
                                page = page,
                                error = %e,
                                "Skipping topic posts due skippable status"
                            );
                            if page == 0 {
                                return Ok(());
                            }
                            break;
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
                    known_users_cache,
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
                let known_users_cache_all = known_users_cache.clone();
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
                                        &known_users_cache_all,
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
