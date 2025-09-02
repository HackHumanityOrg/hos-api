use discourse_indexer::sync::Concurrency;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Semaphores cap concurrent tasks at the configured limits.
#[tokio::test]
async fn test_concurrency_permits_limit_parallelism() {
    // Limits for http and post_tasks we will verify
    let http_limit = 3usize;
    let post_limit = 2usize;
    let conc = Concurrency::new(http_limit, post_limit, 4, 5);

    // Track current and max concurrency observed
    async fn exercise(semaphore: Arc<tokio::sync::Semaphore>, limit: usize) {
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..10 {
            let sem = semaphore.clone();
            let cur = current.clone();
            let max = max_seen.clone();
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let now = cur.fetch_add(1, Ordering::SeqCst) + 1;
                loop {
                    let prev = max.load(Ordering::SeqCst);
                    if now > prev {
                        if max
                            .compare_exchange(prev, now, Ordering::SeqCst, Ordering::SeqCst)
                            .is_ok()
                        {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                // Simulate work while holding the permit
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                cur.fetch_sub(1, Ordering::SeqCst);
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        // Assert we actually reached the limit and never exceeded it
        let observed = max_seen.load(Ordering::SeqCst);
        assert_eq!(observed, limit, "observed concurrency should equal limit");
    }

    // Verify http semaphore
    exercise(conc.http.clone(), http_limit).await;
    // Verify post_tasks semaphore
    exercise(conc.post_tasks.clone(), post_limit).await;
}
