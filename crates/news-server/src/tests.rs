use super::*;
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicI64, AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;
use tower::ServiceExt;

struct TestClock(AtomicI64);
impl TestClock {
    fn new() -> Self {
        Self(AtomicI64::new(1_759_320_000_000))
    }
    fn advance(&self, milliseconds: i64) {
        self.0.fetch_add(milliseconds, Ordering::SeqCst);
    }
}
impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(self.0.load(Ordering::SeqCst)).unwrap()
    }
}
struct MockFetcher {
    feeds: Mutex<HashMap<String, Result<String>>>,
    calls: AtomicUsize,
    gate: Option<Arc<Semaphore>>,
}
impl MockFetcher {
    fn new() -> Self {
        Self {
            feeds: Mutex::new(HashMap::new()),
            calls: AtomicUsize::new(0),
            gate: None,
        }
    }
    fn set(&self, source: &str, feed: Result<String>) {
        self.feeds.lock().unwrap().insert(source.into(), feed);
    }
}
impl FeedFetcher for MockFetcher {
    fn fetch<'a>(&'a self, source: &'a Source) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                gate.acquire().await.unwrap().forget();
            }
            self.feeds
                .lock()
                .unwrap()
                .get(source.id)
                .cloned()
                .unwrap_or_else(|| Ok(feed(source.id, "AI model launch")))
        })
    }
}
fn feed(source: &str, title: &str) -> String {
    format!("<rss><channel><item><title>{title}</title><link>https://example.test/{source}</link><pubDate>2025-10-01T00:00:00Z</pubDate><description>Official AI update</description></item></channel></rss>")
}
fn fixture() -> (AppState, Arc<MockFetcher>, Arc<TestClock>) {
    let fetcher = Arc::new(MockFetcher::new());
    let clock = Arc::new(TestClock::new());
    (
        AppState::new(
            Database::in_memory().unwrap(),
            fetcher.clone(),
            clock.clone(),
            Some("test-secret"),
        ),
        fetcher,
        clock,
    )
}
async fn request(
    app: Router,
    method: &str,
    path: &str,
    token: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, token);
    }
    let response = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn details() -> Vec<SourceDetail> {
    SOURCES
        .iter()
        .map(|s| SourceDetail {
            source: s.id.into(),
            status: "ok".into(),
            items: Some(1),
            error: None,
            checked_at: "2025-10-01T00:00:00.000Z".into(),
        })
        .collect()
}

#[tokio::test]
async fn public_news_and_health_are_safe_without_credentials() {
    let (mut state, _, _) = fixture();
    state.admin_digest = None;
    let app = router(state, None);
    let (status, headers, body) = request(app.clone(), "GET", "/api/news", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert!(headers["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("object-src 'none'"));
    assert_eq!(body["articles"], serde_json::json!([]));
    assert_eq!(body["run"], Value::Null);
    assert_eq!(body["schedule"], "尚未启用");
    assert_eq!(body["sources"].as_array().unwrap().len(), 5);
    assert!(body["sources"][0].get("url").is_none());
    assert_eq!(body["pagination"]["total"], 0);
    assert_eq!(body["stats"]["total_sources"], 5);
    let (status, headers, body) = request(app, "GET", "/healthz", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-cache");
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn all_unauthorized_writes_fail_closed_without_fetching() {
    let (state, fetcher, _) = fixture();
    for token in [
        None,
        Some("Bearer wrong"),
        Some("Basic test-secret"),
        Some("Bearer "),
        Some("Bearer test-secret-extra"),
    ] {
        let (status, _, body) =
            request(router(state.clone(), None), "POST", "/api/refresh", token).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "Unauthorized");
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_bytes(&[255]).unwrap(),
    );
    assert!(!state.authorized(&headers));
    for configured in [None, Some("")] {
        let denied = AppState::new(
            state.db.clone(),
            fetcher.clone(),
            Arc::new(SystemClock),
            configured,
        );
        assert_eq!(
            request(
                router(denied, None),
                "POST",
                "/api/refresh",
                Some("Bearer test-secret")
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
    assert!(state.db.snapshot().await.unwrap().run.is_none());
}

#[tokio::test]
async fn refresh_populates_public_contract_and_is_idempotent() {
    let (state, fetcher, _) = fixture();
    let app = router(state.clone(), None);
    let (status, _, refresh) = request(
        app.clone(),
        "POST",
        "/api/refresh",
        Some("Bearer test-secret"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refresh["status"], "success");
    assert_eq!(refresh["processed"], 5);
    assert_eq!(refresh["inserted"], 5);
    assert_eq!(refresh["details"][0]["items"], 1);
    assert!(refresh["details"][0].get("error").is_none());
    let (_, _, news) = request(app.clone(), "GET", "/api/news", None).await;
    assert_eq!(news["articles"].as_array().unwrap().len(), 5);
    assert_eq!(news["articles"][0]["category"], "模型进展");
    assert_eq!(news["run"]["status"], "success");
    assert_eq!(news["run"]["added"], 5);
    assert!(news["run"]["finished_at"].is_string());
    let (_, _, result) = request(app, "POST", "/api/refresh", Some("Bearer test-secret")).await;
    assert_eq!(result, serde_json::json!({"status":"not_due","added":0}));
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn routing_rejects_unsupported_methods_and_unknown_api() {
    let (state, _, _) = fixture();
    let app = router(state, None);
    for (method, path, expected) in [
        ("GET", "/api/refresh", 405),
        ("POST", "/api/news", 405),
        ("GET", "/missing", 404),
        ("GET", "/api/schedule", 404),
        ("POST", "/api/schedule", 404),
        ("GET", "/api", 404),
        ("GET", "/api/news?limit=no", 400),
    ] {
        assert_eq!(
            request(app.clone(), method, path, None).await.0.as_u16(),
            expected
        );
    }
    assert_eq!(
        request(app, "HEAD", "/api/news", None).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn pagination_and_stats_use_database_totals() {
    let (state, _, clock) = fixture();
    let now = clock.now();
    let articles = (0..355)
        .map(|i| Article {
            id: format!("{i:04}"),
            title: format!("article {i}"),
            url: format!("https://example.test/{i}"),
            source: "openai".into(),
            category: "研究前沿".into(),
            summary: "safe text".into(),
            published_at: iso_timestamp(if i == 0 {
                now - chrono::Duration::days(2)
            } else {
                now
            }),
            fetched_at: iso_timestamp(now),
        })
        .collect();
    state.db.acquire(now, "seed".into()).await.unwrap();
    state
        .db
        .finish("seed".into(), now, articles, details())
        .await
        .unwrap();
    let app = router(state, None);
    let (_, _, body) = request(app.clone(), "GET", "/api/news?limit=2&offset=1", None).await;
    assert_eq!(body["articles"].as_array().unwrap().len(), 2);
    assert_eq!(body["articles"][0]["id"], "0002");
    assert_eq!(
        body["pagination"],
        serde_json::json!({"total":355,"limit":2,"offset":1,"has_more":true})
    );
    assert_eq!(
        body["stats"],
        serde_json::json!({"total_articles":355,"recent_articles":354,"total_sources":5})
    );
    let (_, _, body) = request(app.clone(), "GET", "/api/news?limit=9999&offset=354", None).await;
    assert_eq!(body["pagination"]["limit"], 350);
    assert_eq!(body["pagination"]["has_more"], false);
    assert_eq!(body["articles"].as_array().unwrap().len(), 1);
    let (_, _, body) = request(app, "GET", "/api/news?limit=0&offset=9999", None).await;
    assert_eq!(body["pagination"]["limit"], 1);
    assert_eq!(body["articles"], serde_json::json!([]));
}

#[tokio::test]
async fn partial_and_failed_feeds_retain_saved_articles_and_retry() {
    let (state, fetcher, clock) = fixture();
    state.refresh().await.unwrap();
    clock.advance(NOT_DUE_MS);
    fetcher.set("openai", Err("upstream unavailable".into()));
    fetcher.set("deepmind", Ok("<broken".into()));
    fetcher.set("google", Ok(feed("google", "New robot research")));
    let partial = state.refresh().await.unwrap();
    assert_eq!(partial.status, "partial");
    assert_eq!(partial.processed, Some(3));
    let news = state.db.snapshot().await.unwrap();
    assert_eq!(news.articles.len(), 5);
    assert_eq!(
        news.articles
            .iter()
            .find(|a| a.source == "openai")
            .unwrap()
            .title,
        "AI model launch"
    );
    assert_eq!(
        news.articles
            .iter()
            .find(|a| a.source == "google")
            .unwrap()
            .category,
        "具身智能"
    );
    clock.advance(NOT_DUE_MS);
    for source in SOURCES {
        fetcher.set(source.id, Err("测".repeat(250)));
    }
    let failed = state.refresh().await.unwrap();
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.processed, Some(0));
    for detail in failed.details.unwrap() {
        assert_eq!(detail.error.unwrap().chars().count(), 180);
        assert!(detail.items.is_none());
    }
    assert_eq!(state.db.snapshot().await.unwrap().articles.len(), 5);
    assert_eq!(state.refresh().await.unwrap().status, "failed");
}

#[tokio::test]
async fn separate_database_connections_coordinate_overlapping_refreshes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("news.db");
    let gate = Arc::new(Semaphore::new(0));
    let fetcher = Arc::new(MockFetcher {
        gate: Some(gate.clone()),
        ..MockFetcher::new()
    });
    let clock = Arc::new(TestClock::new());
    let first = AppState::new(
        Database::open(&path).unwrap(),
        fetcher.clone(),
        clock.clone(),
        None,
    );
    let second = AppState::new(
        Database::open(&path).unwrap(),
        Arc::new(MockFetcher::new()),
        clock,
        None,
    );
    let running = tokio::spawn(async move { first.refresh().await.unwrap() });
    while fetcher.calls.load(Ordering::SeqCst) != 5 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        second.db.snapshot().await.unwrap().run.unwrap().status,
        "running"
    );
    assert_eq!(second.refresh().await.unwrap().status, "busy");
    gate.add_permits(5);
    assert_eq!(running.await.unwrap().status, "success");
    assert_eq!(second.refresh().await.unwrap().status, "not_due");
    assert_eq!(second.db.snapshot().await.unwrap().articles.len(), 5);
}

#[tokio::test]
async fn expired_owner_cannot_write_or_unlock_new_owner() {
    let (state, _, clock) = fixture();
    assert!(state
        .db
        .acquire(clock.now(), "old".into())
        .await
        .unwrap()
        .is_none());
    clock.advance(LEASE_MS + 1);
    assert!(state
        .db
        .acquire(clock.now(), "new".into())
        .await
        .unwrap()
        .is_none());
    let article = normalize_feed(&SOURCES[0], &feed("old", "AI model"), clock.now()).unwrap();
    assert_eq!(
        state
            .db
            .finish("old".into(), clock.now(), article, details())
            .await
            .unwrap()
            .status,
        "busy"
    );
    assert_eq!(
        state
            .db
            .acquire(clock.now(), "third".into())
            .await
            .unwrap()
            .unwrap()
            .status,
        "busy"
    );
    assert!(state.db.snapshot().await.unwrap().articles.is_empty());
    assert_eq!(
        state
            .db
            .finish("new".into(), clock.now(), Vec::new(), details())
            .await
            .unwrap()
            .status,
        "success"
    );
    let old: String = state
        .db
        .run(|db| {
            db.query_row("SELECT status FROM runs WHERE id='old'", [], |r| r.get(0))
                .map_err(|e| e.to_string())
        })
        .await
        .unwrap();
    assert_eq!(old, "failed");
}

#[tokio::test]
async fn invalid_lease_and_run_details_are_recoverable() {
    let (state, _, _) = fixture();
    state
        .db
        .run(|db| {
            db.execute(
                "INSERT INTO settings(key,value) VALUES('ingest_lease','invalid')",
                [],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(state.refresh().await.unwrap().status, "success");
    state
        .db
        .run(|db| {
            db.execute("UPDATE runs SET details='broken-json'", [])
                .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(state
        .db
        .snapshot()
        .await
        .unwrap()
        .run
        .unwrap()
        .details
        .is_empty());
}

#[tokio::test]
async fn sqlite_failure_is_sanitized_and_transaction_is_atomic() {
    let (state, _, clock) = fixture();
    state
        .db
        .acquire(clock.now(), "atomic".into())
        .await
        .unwrap();
    state.db.run(|db| { db.execute_batch("CREATE TRIGGER prevent_articles BEFORE INSERT ON articles WHEN NEW.url='https://example.test/reject' BEGIN SELECT RAISE(ABORT, 'private database error'); END;").map_err(|e| e.to_string()) }).await.unwrap();
    let mut articles =
        normalize_feed(&SOURCES[0], &feed("atomic", "AI model"), clock.now()).unwrap();
    let mut rejected = articles[0].clone();
    rejected.id = "reject".into();
    rejected.url = "https://example.test/reject".into();
    articles.push(rejected);
    assert!(state
        .db
        .finish("atomic".into(), clock.now(), articles, details())
        .await
        .is_err());
    assert!(state.db.snapshot().await.unwrap().articles.is_empty());
    state
        .db
        .run(|db| {
            db.execute_batch("DROP TABLE runs;")
                .map_err(|e| e.to_string())
        })
        .await
        .unwrap();
    for (method, path, token) in [
        ("GET", "/api/news", None),
        ("POST", "/api/refresh", Some("Bearer test-secret")),
    ] {
        // Expire the existing lease so refresh reaches the failing query.
        clock.advance(LEASE_MS + 1);
        let (status, _, body) = request(router(state.clone(), None), method, path, token).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], UNAVAILABLE);
        assert!(!body.to_string().contains("database error"));
    }
}

#[tokio::test]
async fn optional_static_files_work_without_overriding_api_routes() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("index.html"),
        "<!doctype html><title>AI Signal</title>",
    )
    .unwrap();
    let (state, _, _) = fixture();
    let app = router(state, Some(directory.path().into()));
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .contains("text/html"));
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&bytes).contains("AI Signal"));
    assert_eq!(
        request(app.clone(), "GET", "/api/unknown", None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(app, "GET", "/missing.js", None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn scheduler_records_status_refreshes_and_stops() {
    let (state, fetcher, _) = fixture();
    let (tx, rx) = watch::channel(false);
    let worker_state = state.clone();
    let task = tokio::spawn(async move { scheduler(worker_state, rx).await.unwrap() });
    while fetcher.calls.load(Ordering::SeqCst) < 5 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        state.db.snapshot().await.unwrap().schedule,
        "每小时自动更新"
    );
    tx.send(true).unwrap();
    task.await.unwrap();
    assert_eq!(state.db.snapshot().await.unwrap().schedule, "尚未启用");
    assert_eq!(
        state.db.snapshot().await.unwrap().run.unwrap().status,
        "success"
    );
    let (_, rx) = watch::channel(true);
    scheduler(state, rx).await.unwrap();
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn timeout_is_reported_as_failed_source_without_live_network() {
    let (mut state, _, _) = fixture();
    struct Hanging;
    impl FeedFetcher for Hanging {
        fn fetch<'a>(&'a self, _: &'a Source) -> BoxFuture<'a, Result<String>> {
            Box::pin(std::future::pending())
        }
    }
    state.fetcher = Arc::new(Hanging);
    let state_for_task = state.clone();
    let task = tokio::spawn(async move { state_for_task.refresh().await.unwrap() });
    while state.db.snapshot().await.unwrap().run.is_none() {
        tokio::task::yield_now().await;
    }
    // Paused time only starts after SQLite acquisition, avoiding blocking-pool races.
    tokio::time::pause();
    tokio::time::advance(FETCH_TIMEOUT + Duration::from_secs(1)).await;
    let result = task.await.unwrap();
    assert_eq!(result.status, "failed");
    assert!(result
        .details
        .unwrap()
        .iter()
        .all(|d| d.error.as_deref() == Some("Feed request timed out")));
}

#[tokio::test]
async fn database_open_rejects_invalid_path_and_system_clock_is_current() {
    let directory = tempfile::tempdir().unwrap();
    assert!(Database::open(directory.path()).is_err());
    assert!(
        SystemClock
            .now()
            .signed_duration_since(Utc::now())
            .num_seconds()
            .abs()
            < 2
    );
    let fetcher = HttpFetcher::new().unwrap();
    let source = Source {
        id: "test",
        name: "test",
        url: "http://127.0.0.1:1/",
        home: "https://example.test/",
    };
    assert_eq!(
        fetcher.fetch(&source).await.unwrap_err(),
        "Feed request failed"
    );
}

#[tokio::test]
async fn http_fetcher_bounds_mock_responses_and_rejects_redirects() {
    // Local mock transport only; production HttpFetcher always enforces HTTPS.
    let application = Router::new()
        .route("/ok", get(|| async { "<rss><channel/></rss>" }))
        .route("/error", get(|| async { StatusCode::SERVICE_UNAVAILABLE }))
        .route(
            "/redirect",
            get(|| async { (StatusCode::FOUND, [(header::LOCATION, "/ok")]) }),
        )
        .route("/large", get(|| async { "x".repeat(MAX_FEED_BYTES + 1) }))
        .route("/utf8", get(|| async { vec![255_u8] }))
        .route(
            "/stream-large",
            get(|| async {
                Body::from_stream(futures_util::stream::iter([
                    Ok::<_, std::io::Error>(vec![b'x'; MAX_FEED_BYTES / 2]),
                    Ok(vec![b'x'; MAX_FEED_BYTES / 2 + 1]),
                ]))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
    let fetcher = HttpFetcher {
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
    };
    for (path, expected) in [
        ("ok", Ok("<rss><channel/></rss>")),
        ("error", Err("HTTP 503")),
        ("redirect", Err("HTTP 302")),
        ("large", Err("Feed exceeds 3 MB limit")),
        ("utf8", Err("Feed is not UTF-8")),
        ("stream-large", Err("Feed exceeds 3 MB limit")),
    ] {
        let url = format!("http://{address}/{path}");
        // Source owns static publisher URLs in production; test values last this process.
        let source = Source {
            id: "test",
            name: "test",
            url: Box::leak(url.into_boxed_str()),
            home: "https://example.test/",
        };
        assert_eq!(
            fetcher
                .fetch(&source)
                .await
                .as_deref()
                .map_err(String::as_str),
            expected
        );
    }
    server.abort();
}

#[tokio::test]
async fn scheduler_waits_a_full_hour_before_next_refresh() {
    let (state, fetcher, clock) = fixture();
    let (tx, rx) = watch::channel(false);
    let worker_state = state.clone();
    let task = tokio::spawn(async move { scheduler(worker_state, rx).await.unwrap() });
    loop {
        if state
            .db
            .snapshot()
            .await
            .unwrap()
            .run
            .is_some_and(|run| run.status == "success")
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    tokio::time::pause();
    tokio::time::advance(REFRESH_INTERVAL - Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 5);
    clock.advance(REFRESH_INTERVAL.as_millis() as i64);
    tokio::time::advance(Duration::from_secs(1)).await;
    while fetcher.calls.load(Ordering::SeqCst) < 10 {
        tokio::task::yield_now().await;
    }
    tx.send(true).unwrap();
    task.await.unwrap();
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn repeated_and_mixed_refreshes_count_new_urls_separately_from_processed() {
    let (state, fetcher, clock) = fixture();
    let first = state.refresh().await.unwrap();
    assert_eq!(first.inserted, Some(5));
    assert_eq!(first.processed, Some(5));
    clock.advance(NOT_DUE_MS);
    fetcher.set("openai", Ok(feed("openai", "Updated robot title")));
    let repeated = state.refresh().await.unwrap();
    assert_eq!(repeated.inserted, Some(0));
    assert_eq!(repeated.processed, Some(5));
    let news = state.db.snapshot().await.unwrap();
    assert_eq!(news.run.unwrap().added, 0);
    assert_eq!(news.articles.len(), 5);
    let updated = news
        .articles
        .iter()
        .find(|article| article.source == "openai")
        .unwrap();
    assert_eq!(updated.title, "Updated robot title");
    assert_eq!(updated.category, "具身智能");
    assert_eq!(updated.fetched_at, iso_timestamp(clock.now()));
    clock.advance(NOT_DUE_MS);
    fetcher.set("google", Ok(feed("new-google-url", "New AI model")));
    let mixed = state.refresh().await.unwrap();
    assert_eq!(mixed.inserted, Some(1));
    assert_eq!(mixed.processed, Some(5));
    let news = state.db.snapshot().await.unwrap();
    assert_eq!(news.run.unwrap().added, 1);
    assert_eq!(news.articles.len(), 6);
}

#[tokio::test]
async fn insert_counting_does_not_hide_unrelated_constraint_failures() {
    let (state, _, clock) = fixture();
    state.refresh().await.unwrap();
    let before = state.db.snapshot().await.unwrap();
    clock.advance(NOT_DUE_MS);
    state
        .db
        .acquire(clock.now(), "constraint-check".into())
        .await
        .unwrap();
    let mut articles =
        normalize_feed(&SOURCES[0], &feed("new-valid", "New AI model"), clock.now()).unwrap();
    let mut conflicting = before.articles[0].clone();
    // Same primary key but a different URL must fail, not be silently ignored.
    conflicting.url = "https://example.test/primary-key-conflict".into();
    articles.push(conflicting);
    assert!(state
        .db
        .finish("constraint-check".into(), clock.now(), articles, details())
        .await
        .is_err());
    let after = state.db.snapshot().await.unwrap();
    assert_eq!(after.articles, before.articles);
    let run = after.run.unwrap();
    assert_eq!(run.status, "running");
    assert_eq!(run.added, 0);
}

fn news_path(parameters: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse("https://example.test/api/news").unwrap();
    url.query_pairs_mut()
        .extend_pairs(parameters.iter().copied());
    format!("/api/news?{}", url.query().unwrap())
}

async fn search_fixture() -> AppState {
    let (state, _, clock) = fixture();
    let now = iso_timestamp(clock.now());
    let mut articles: Vec<_> = (0..120)
        .map(|index| Article {
            id: format!("{index:04}"),
            title: if index >= 100 {
                format!("RESEARCH Needle {index}")
            } else {
                format!("AI update {index}")
            },
            url: format!("https://example.test/search/{index}"),
            source: "openai".into(),
            category: if index >= 100 {
                "研究前沿"
            } else {
                "模型进展"
            }
            .into(),
            summary: "Official update".into(),
            published_at: now.clone(),
            fetched_at: now.clone(),
        })
        .collect();
    for (id, title, summary, category) in [
        (
            "percent",
            "Efficiency improves 10% today",
            "Literal punctuation",
            "安全治理",
        ),
        (
            "underscore",
            "Model foo_bar",
            "Literal punctuation",
            "具身智能",
        ),
        (
            "backslash",
            r"Path C:\Models",
            "Literal punctuation",
            "产业动态",
        ),
        (
            "combined",
            r"Percent%_\Pair",
            "Literal punctuation",
            "模型进展",
        ),
        (
            "quoted",
            "Quoted ' OR 1=1 -- text",
            "Literal SQL-like text",
            "安全治理",
        ),
        (
            "summary",
            "Ordinary title",
            "SummaryOnlyTerm appears here",
            "开发工具",
        ),
        ("unicode", "普通标题", "汉字摘要", "研究前沿"),
    ] {
        articles.push(Article {
            id: id.into(),
            title: title.into(),
            summary: summary.into(),
            category: category.into(),
            url: format!("https://example.test/search/{id}"),
            source: "google".into(),
            published_at: now.clone(),
            fetched_at: now.clone(),
        });
    }
    state
        .db
        .acquire(clock.now(), "search-seed".into())
        .await
        .unwrap();
    state
        .db
        .finish("search-seed".into(), clock.now(), articles, details())
        .await
        .unwrap();
    state
}

#[tokio::test]
async fn search_and_category_filter_full_corpus_before_pagination() {
    let app = router(search_fixture().await, None);
    let (_, _, initial) = request(app.clone(), "GET", "/api/news?limit=50", None).await;
    assert_eq!(initial["articles"].as_array().unwrap().len(), 50);
    assert!(initial["articles"]
        .as_array()
        .unwrap()
        .iter()
        .all(|article| article["category"] == "模型进展"));
    let path = news_path(&[
        ("category", "研究前沿"),
        ("q", "  nEeDlE  "),
        ("limit", "2"),
        ("offset", "1"),
    ]);
    let (status, _, filtered) = request(app.clone(), "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        filtered["pagination"],
        serde_json::json!({"total":20,"limit":2,"offset":1,"has_more":true})
    );
    assert_eq!(filtered["articles"][0]["id"], "0101");
    assert_eq!(filtered["articles"][1]["id"], "0102");
    assert_eq!(filtered["stats"], initial["stats"]);
    assert_eq!(filtered["stats"]["total_articles"], 127);
    let path = news_path(&[("q", "needle"), ("limit", "2"), ("offset", "19")]);
    let (_, _, last) = request(app.clone(), "GET", &path, None).await;
    assert_eq!(last["articles"].as_array().unwrap().len(), 1);
    assert_eq!(last["articles"][0]["id"], "0119");
    assert_eq!(last["pagination"]["total"], 20);
    assert_eq!(last["pagination"]["has_more"], false);
    let path = news_path(&[("category", "研究前沿"), ("q", "needle"), ("offset", "999")]);
    let (_, _, past_end) = request(app, "GET", &path, None).await;
    assert_eq!(past_end["articles"], serde_json::json!([]));
    assert_eq!(past_end["pagination"]["total"], 20);
    assert_eq!(past_end["pagination"]["has_more"], false);
}

#[tokio::test]
async fn category_filter_is_exact_and_query_matches_title_or_summary() {
    let app = router(search_fixture().await, None);
    for category in CATEGORIES {
        let path = news_path(&[("category", category)]);
        let (status, _, body) = request(app.clone(), "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK);
        let articles = body["articles"].as_array().unwrap();
        assert!(!articles.is_empty());
        assert!(articles
            .iter()
            .all(|article| article["category"] == category));
        assert_eq!(
            body["pagination"]["total"].as_u64().unwrap() as usize,
            articles.len()
        );
        assert_eq!(body["stats"]["total_articles"], 127);
    }
    for (category, query, expected) in [
        ("开发工具", "SUMMARYONLYTERM", 1),
        ("模型进展", "SUMMARYONLYTERM", 0),
        ("研究前沿", "汉字", 1),
        ("", "unmatched query", 0),
    ] {
        let path = news_path(&[("category", category), ("q", query)]);
        let (status, _, body) = request(app.clone(), "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["pagination"]["total"], expected);
        assert_eq!(
            body["articles"].as_array().unwrap().len(),
            expected as usize
        );
        assert_eq!(body["stats"]["total_articles"], 127);
    }
    let path = news_path(&[("category", ""), ("q", " \t\u{2003}"), ("limit", "50")]);
    let (_, _, empty) = request(app, "GET", &path, None).await;
    assert_eq!(empty["pagination"]["total"], 127);
    assert_eq!(empty["articles"].as_array().unwrap().len(), 50);
    assert_eq!(empty["pagination"]["has_more"], true);
}

#[tokio::test]
async fn search_escapes_like_wildcards_backslashes_and_sql_like_input() {
    let app = router(search_fixture().await, None);
    for (query, expected) in [
        ("%", 2),
        ("_", 2),
        (r"\", 2),
        (r"%_\", 1),
        ("' OR 1=1 --", 1),
        ("%' OR 1=1 --", 0),
        ("10% TODAY", 1),
        (r"c:\models", 1),
    ] {
        let path = news_path(&[("q", query)]);
        let (status, _, body) = request(app.clone(), "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK, "query {query:?}");
        assert_eq!(body["pagination"]["total"], expected, "query {query:?}");
        assert_eq!(
            body["articles"].as_array().unwrap().len(),
            expected as usize
        );
    }
    let (_, _, all) = request(app, "GET", "/api/news", None).await;
    assert_eq!(all["pagination"]["total"], 127);
}

#[tokio::test]
async fn filter_validation_rejects_unknown_categories_and_overlong_trimmed_queries() {
    let (state, fetcher, _) = fixture();
    let app = router(state, None);
    for category in ["unknown", "研究", "研究前沿 ", "全部资讯", "' OR 1=1 --"] {
        let path = news_path(&[("category", category)]);
        let (status, headers, body) = request(app.clone(), "GET", &path, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(headers["cache-control"], "no-store");
        assert_eq!(body["error"], "Unknown category");
    }
    for query in ["\0", "needle\0ignored"] {
        let path = news_path(&[("q", query)]);
        let (status, _, body) = request(app.clone(), "GET", &path, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["error"],
            "Search query must not contain null characters"
        );
    }
    for query in ["x".repeat(201), "界".repeat(201)] {
        let path = news_path(&[("q", &query)]);
        let (status, _, body) = request(app.clone(), "GET", &path, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "Search query must be at most 200 characters");
    }
    for query in [
        "x".repeat(200),
        "界".repeat(200),
        format!("  {}\t", "界".repeat(200)),
        " ".repeat(201),
    ] {
        let path = news_path(&[("q", &query)]);
        assert_eq!(
            request(app.clone(), "GET", &path, None).await.0,
            StatusCode::OK
        );
    }
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn sqlite_integer_boundaries_are_checked_in_both_directions() {
    assert_eq!(sqlite_integer(0).unwrap(), 0);
    assert_eq!(sqlite_integer(350).unwrap(), 350);
    let largest = usize::try_from(i64::MAX).unwrap_or(usize::MAX);
    assert_eq!(
        sqlite_integer(largest).unwrap(),
        i64::try_from(largest).unwrap()
    );
    if let Some(overflow) = largest.checked_add(1) {
        assert_eq!(
            sqlite_integer(overflow).unwrap_err(),
            "Value exceeds SQLite integer range"
        );
    }
    let db = Connection::open_in_memory().unwrap();
    for value in [0_i64, 350, i64::MAX] {
        let result = db.query_row("SELECT ?", [value], |row| row_count(row, 0));
        match usize::try_from(value) {
            Ok(expected) => assert_eq!(result.unwrap(), expected),
            Err(_) => assert!(
                matches!(result, Err(rusqlite::Error::IntegralValueOutOfRange(0, v)) if v == value)
            ),
        }
    }
    for value in [-1_i64, i64::MIN] {
        let result = db.query_row("SELECT 0, ?", [value], |row| row_count(row, 1));
        assert!(
            matches!(result, Err(rusqlite::Error::IntegralValueOutOfRange(1, v)) if v == value)
        );
    }
    for sql in ["SELECT 'corrupt'", "SELECT NULL", "SELECT 1.5"] {
        assert!(matches!(
            db.query_row(sql, [], |row| row_count(row, 0)),
            Err(rusqlite::Error::InvalidColumnType(..))
        ));
    }
}

#[tokio::test]
async fn pagination_clamps_extreme_values_without_sqlite_overflow() {
    let (state, _, clock) = fixture();
    state.refresh().await.unwrap();
    for offset in [0, usize::MAX] {
        let news = state
            .db
            .snapshot_page(usize::MAX, offset, clock.now())
            .await
            .unwrap();
        assert_eq!(news.pagination.limit, 350);
        assert_eq!(
            news.pagination.offset,
            offset.min(usize::try_from(i64::MAX).unwrap_or(usize::MAX))
        );
        assert_eq!(news.pagination.total, 5);
        assert!(!news.pagination.has_more);
        assert_eq!(news.articles.len(), if offset == 0 { 5 } else { 0 });
    }
}

#[tokio::test]
async fn invalid_persisted_run_counts_fail_closed() {
    let (state, _, _) = fixture();
    state.refresh().await.unwrap();
    for value in [
        rusqlite::types::Value::Integer(-1),
        rusqlite::types::Value::Text("corrupt private value".into()),
    ] {
        state
            .db
            .run(move |db| {
                db.execute("UPDATE runs SET added=?", [value])
                    .map_err(|e| e.to_string())?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(state.db.snapshot().await.is_err());
        let (status, headers, body) =
            request(router(state.clone(), None), "GET", "/api/news", None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(headers["cache-control"], "no-store");
        assert_eq!(body, serde_json::json!({"error": UNAVAILABLE}));
    }
}

#[tokio::test]
async fn static_directory_redirect_head_and_range_contracts_are_preserved() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("docs")).unwrap();
    std::fs::write(directory.path().join("docs/index.html"), "0123456789").unwrap();
    let (state, _, _) = fixture();
    let app = router(state, Some(directory.path().into()));
    let redirect = app
        .clone()
        .oneshot(Request::builder().uri("/docs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(redirect.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(redirect.headers()[header::LOCATION], "/docs/");
    let head = app
        .clone()
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/docs/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()[header::CONTENT_LENGTH], "10");
    assert!(head
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .is_empty());
    let partial = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/docs/")
                .header(header::RANGE, "bytes=2-5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(partial.headers()[header::CONTENT_RANGE], "bytes 2-5/10");
    assert_eq!(partial.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        &partial.into_body().collect().await.unwrap().to_bytes()[..],
        b"2345"
    );
    let invalid = app
        .oneshot(
            Request::builder()
                .uri("/docs/")
                .header(header::RANGE, "bytes=99-100")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(invalid.headers()[header::CONTENT_RANGE], "bytes */10");
}
