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
                .unwrap_or_else(|| Ok(source_fixture(source.id)))
        })
    }
}
fn feed(source: &str, title: &str) -> String {
    format!("<rss><channel><item><title>{title}</title><link>https://example.test/{source}</link><pubDate>2025-10-01T00:00:00Z</pubDate><description>Official AI update</description></item></channel></rss>")
}
fn source_fixture(source: &str) -> String {
    match source {
        "anthropic" => r#"<main><a href="/news/native-fixture"><h2>AI model launch</h2><time>Oct 1, 2025</time><p>Official AI update</p></a></main>"#.into(),
        "deepseek" => r#"<div class="theme-doc-markdown"><div class="col--12"><h2 id="date-2025-10-01">2025/10/01</h2><h3>AI model launch</h3><p>Official AI update <a href="/news/news20251001">Read</a></p></div></div>"#.into(),
        "kimi" => r#"<div class="menu-card"><a href="/en/blog/native-fixture"><h4 class="card-title">AI model launch</h4><p class="card-date">2025-10-01</p><p class="card-description">Official AI update</p></a></div>"#.into(),
        "bytedance" => serde_json::json!({"BaseResp":{"StatusCode":0},"sub_article_list":[{"ArticleMeta":{"PublishDate":1759276800000_i64},"ArticleSubContentEn":{"Title":"AI model launch","TitleKey":"native-fixture","Abstract":"Official AI update"}}]}).to_string(),
        "tencent" => serde_json::json!({"code":0,"data":{"list":[{"id":42,"title":"AI model launch","customUrl":"native-fixture","desc":"Official AI update","displayPublishTime":1759276800_i64}]}}).to_string(),
        "qwen" => serde_json::json!({"success":true,"data":{"articles":[{"title":"AI model launch","path":"native-fixture","extra":{"date":"2025-10-01T00:00:00Z","description":"Official AI update"}}]}}).to_string(),
        "glm" => r#"<Update label="2025-10-01" description="AI model launch">Official AI update</Update>"#.into(),
        _ => feed(source, "AI model launch"),
    }
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
    assert_eq!(body["sources"].as_array().unwrap().len(), SOURCES.len());
    assert!(body["sources"][0].get("url").is_none());
    assert_eq!(body["pagination"]["total"], 0);
    assert_eq!(body["stats"]["total_sources"], SOURCES.len());
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
    assert_eq!(refresh["processed"], SOURCES.len());
    assert_eq!(refresh["inserted"], SOURCES.len());
    assert_eq!(refresh["details"][0]["items"], 1);
    assert!(refresh["details"][0].get("error").is_none());
    let (_, _, news) = request(app.clone(), "GET", "/api/news", None).await;
    assert_eq!(news["articles"].as_array().unwrap().len(), SOURCES.len());
    assert_eq!(news["articles"][0]["category"], "模型进展");
    assert_eq!(news["run"]["status"], "success");
    assert_eq!(news["run"]["added"], SOURCES.len());
    assert!(news["run"]["finished_at"].is_string());
    let (_, _, result) = request(app, "POST", "/api/refresh", Some("Bearer test-secret")).await;
    assert_eq!(result, serde_json::json!({"status":"not_due","added":0}));
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), SOURCES.len());
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
        serde_json::json!({"total_articles":355,"recent_articles":354,"total_sources":SOURCES.len()})
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
    assert_eq!(partial.processed, Some(SOURCES.len() - 2));
    let news = state.db.snapshot().await.unwrap();
    assert_eq!(news.articles.len(), SOURCES.len());
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
    assert_eq!(
        state.db.snapshot().await.unwrap().articles.len(),
        SOURCES.len()
    );
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
    while fetcher.calls.load(Ordering::SeqCst) != SOURCES.len() {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        second.db.snapshot().await.unwrap().run.unwrap().status,
        "running"
    );
    assert_eq!(second.refresh().await.unwrap().status, "busy");
    gate.add_permits(SOURCES.len());
    assert_eq!(running.await.unwrap().status, "success");
    assert_eq!(second.refresh().await.unwrap().status, "not_due");
    assert_eq!(
        second.db.snapshot().await.unwrap().articles.len(),
        SOURCES.len()
    );
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
    while fetcher.calls.load(Ordering::SeqCst) < SOURCES.len() {
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
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), SOURCES.len());
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
        ..SOURCES[0]
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
            ..SOURCES[0]
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
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), SOURCES.len());
    clock.advance(REFRESH_INTERVAL.as_millis() as i64);
    tokio::time::advance(Duration::from_secs(1)).await;
    while fetcher.calls.load(Ordering::SeqCst) < 2 * SOURCES.len() {
        tokio::task::yield_now().await;
    }
    tx.send(true).unwrap();
    task.await.unwrap();
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 2 * SOURCES.len());
}

#[tokio::test]
async fn repeated_and_mixed_refreshes_count_new_urls_separately_from_processed() {
    let (state, fetcher, clock) = fixture();
    let first = state.refresh().await.unwrap();
    assert_eq!(first.inserted, Some(SOURCES.len()));
    assert_eq!(first.processed, Some(SOURCES.len()));
    clock.advance(NOT_DUE_MS);
    fetcher.set("openai", Ok(feed("openai", "Updated robot title")));
    let repeated = state.refresh().await.unwrap();
    assert_eq!(repeated.inserted, Some(0));
    assert_eq!(repeated.processed, Some(SOURCES.len()));
    let news = state.db.snapshot().await.unwrap();
    assert_eq!(news.run.unwrap().added, 0);
    assert_eq!(news.articles.len(), SOURCES.len());
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
    assert_eq!(mixed.processed, Some(SOURCES.len()));
    let news = state.db.snapshot().await.unwrap();
    assert_eq!(news.run.unwrap().added, 1);
    assert_eq!(news.articles.len(), SOURCES.len() + 1);
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
        assert_eq!(news.pagination.total, SOURCES.len());
        assert!(!news.pagination.has_more);
        assert_eq!(
            news.articles.len(),
            if offset == 0 { SOURCES.len() } else { 0 }
        );
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

fn timeline_path(parameters: &[(&str, &str)]) -> String {
    news_path(parameters).replacen("/api/news", "/api/timeline", 1)
}

#[tokio::test]
async fn sources_filter_all_none_multi_select_duplicates_and_pagination() {
    let app = router(search_fixture().await, None);
    let (_, _, all) = request(app.clone(), "GET", "/api/news", None).await;
    for (sources, total) in [
        ("", 0),
        (" \t", 0),
        (",,,", 0),
        ("openai,", 120),
        (",openai", 120),
        ("openai,,google", 127),
        ("openai", 120),
        ("google", 7),
        ("openai,google", 127),
        ("google, openai,google", 127),
        ("deepmind", 0),
    ] {
        let path = news_path(&[("sources", sources), ("limit", "2"), ("offset", "1")]);
        let (status, _, result) = request(app.clone(), "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK, "sources {sources:?}");
        assert_eq!(result["pagination"]["total"], total);
        assert_eq!(result["pagination"]["has_more"], total > 3);
        assert_eq!(
            result["articles"].as_array().unwrap().len(),
            if total > 0 { 2 } else { 0 }
        );
        assert_eq!(result["stats"], all["stats"]);
        assert_eq!(result["sources"], all["sources"]);
        assert_eq!(result["run"], all["run"]);
    }
    for (sources, category, query, expected) in [
        ("openai", "研究前沿", "needle", 20),
        ("google", "研究前沿", "needle", 0),
        ("google,openai", "研究前沿", "汉字", 1),
        ("google", "安全治理", "' OR 1=1 --", 1),
        ("openai", "安全治理", "' OR 1=1 --", 0),
        ("google,openai", "模型进展", r"%_\", 1),
    ] {
        let path = news_path(&[
            ("sources", sources),
            ("category", category),
            ("q", query),
            ("date", "2025-10-01"),
        ]);
        let (status, _, result) = request(app.clone(), "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["pagination"]["total"], expected);
        assert_eq!(result["stats"], all["stats"]);
    }
}

#[tokio::test]
async fn source_validation_is_bounded_and_identical_for_news_and_timeline() {
    let (state, fetcher, _) = fixture();
    let app = router(state, None);
    for (input, expected) in [
        ("unknown".to_string(), "Unknown source"),
        ("openai,unknown".to_string(), "Unknown source"),
        ("OPENAI".to_string(), "Unknown source"),
        ("openai\0".to_string(), "Unknown source"),
        ("openai') OR 1=1 --".to_string(), "Unknown source"),
        (" ".repeat(2049), "Sources query must be at most 2048 bytes"),
        ("界".repeat(683), "Sources query must be at most 2048 bytes"),
        (
            vec!["openai"; 33].join(","),
            "Sources query must contain at most 32 entries",
        ),
    ] {
        for path in [
            news_path(&[("sources", &input)]),
            timeline_path(&[("sources", &input)]),
        ] {
            let (status, headers, result) = request(app.clone(), "GET", &path, None).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
            assert_eq!(headers["cache-control"], "no-store");
            assert_eq!(result["error"], expected);
        }
    }
    for input in [
        " ".repeat(2048),
        vec!["openai"; 32].join(","),
        format!("{}openai", " ".repeat(2042)),
    ] {
        for path in [
            news_path(&[("sources", &input)]),
            timeline_path(&[("sources", &input)]),
        ] {
            assert_eq!(
                request(app.clone(), "GET", &path, None).await.0,
                StatusCode::OK
            );
        }
    }
    // Every current registry ID is accepted without another adapter allowlist.
    for source in SOURCES {
        for path in [
            news_path(&[("sources", source.id)]),
            timeline_path(&[("sources", source.id)]),
        ] {
            assert_eq!(
                request(app.clone(), "GET", &path, None).await.0,
                StatusCode::OK
            );
        }
    }
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
}

async fn seed_dates(state: &AppState, dates: &[(&str, &str, &str)]) {
    let now = state.clock.now();
    let articles = dates
        .iter()
        .enumerate()
        .map(|(index, (source, published, fetched))| Article {
            id: index.to_string(),
            title: format!("UTC article {index}"),
            url: format!("https://example.test/dates/{index}"),
            source: source.to_string(),
            category: "研究前沿".into(),
            summary: "Literal 10%_\\ title".into(),
            published_at: iso_timestamp(
                DateTime::parse_from_rfc3339(published)
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            fetched_at: iso_timestamp(
                DateTime::parse_from_rfc3339(fetched)
                    .unwrap()
                    .with_timezone(&Utc),
            ),
        })
        .collect();
    state.db.acquire(now, "date-seed".into()).await.unwrap();
    state
        .db
        .finish("date-seed".into(), now, articles, details())
        .await
        .unwrap();
}

#[tokio::test]
async fn date_filters_use_exact_utc_calendar_day_and_published_time() {
    let (state, _, _) = fixture();
    seed_dates(
        &state,
        &[
            ("openai", "2024-02-28T23:59:59.999Z", "2024-02-29T12:00:00Z"),
            ("openai", "2024-02-29T00:00:00.000Z", "2024-03-01T00:00:00Z"),
            ("google", "2024-02-29T23:59:59.999Z", "2024-03-01T00:00:00Z"),
            ("openai", "2024-03-01T00:00:00.000Z", "2024-02-29T00:00:00Z"),
            (
                "google",
                "2024-03-01T01:00:00+02:00",
                "2024-03-01T00:00:00Z",
            ),
            (
                "google",
                "2024-02-28T23:00:00-02:00",
                "2024-03-01T00:00:00Z",
            ),
        ],
    )
    .await;
    let app = router(state, None);
    let path = news_path(&[("date", "2024-02-29"), ("limit", "2"), ("offset", "1")]);
    let (status, _, result) = request(app.clone(), "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        result["pagination"],
        serde_json::json!({"total":4,"limit":2,"offset":1,"has_more":true})
    );
    assert_eq!(result["articles"][0]["id"], "4");
    assert_eq!(result["articles"][1]["id"], "5");
    assert_eq!(result["stats"]["total_articles"], 6);
    let path = news_path(&[
        ("date", "2024-02-29"),
        ("sources", "google"),
        ("category", "研究前沿"),
        ("q", r"10%_\"),
    ]);
    let (_, _, result) = request(app.clone(), "GET", &path, None).await;
    assert_eq!(result["pagination"]["total"], 3);
    for (date, expected) in [
        ("2024-02-28", 1),
        ("2024-03-01", 1),
        ("2025-02-28", 0),
        ("9999-12-31", 0),
    ] {
        let (_, _, result) = request(app.clone(), "GET", &news_path(&[("date", date)]), None).await;
        assert_eq!(result["pagination"]["total"], expected);
        assert_eq!(result["stats"]["total_articles"], 6);
    }
}

#[tokio::test]
async fn date_validation_rejects_noncanonical_and_impossible_dates() {
    let (state, _, _) = fixture();
    let app = router(state, None);
    for date in [
        "",
        "2025-02-29",
        "1900-02-29",
        "2024-04-31",
        "2024-00-10",
        "2024-13-01",
        "2024-01-00",
        "2024-01-32",
        "2024-1-01",
        "24-01-01",
        "2024/01/01",
        "2024-01-01T00:00:00Z",
        " 2024-01-01",
        "2024-01-01 ",
        "2024-01-0界",
        "２０２４-01-01",
        "' OR 1=1--",
        "2024-01-0\0",
    ] {
        let (status, _, result) =
            request(app.clone(), "GET", &news_path(&[("date", date)]), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{date:?}");
        assert_eq!(
            result["error"],
            "Date must be a valid UTC calendar date in YYYY-MM-DD format"
        );
    }
    for date in [
        "0000-01-01",
        "0001-01-01",
        "2000-02-29",
        "2024-02-29",
        "9999-12-31",
    ] {
        assert_eq!(
            request(app.clone(), "GET", &news_path(&[("date", date)]), None)
                .await
                .0,
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn timeline_zero_fills_all_supported_ranges_and_does_not_fetch() {
    let (state, fetcher, _) = fixture();
    let app = router(state, None);
    for (suffix, days, start) in [
        ("", 30, "2025-09-02"),
        ("?days=7", 7, "2025-09-25"),
        ("?days=30", 30, "2025-09-02"),
        ("?days=90", 90, "2025-07-04"),
    ] {
        let (status, headers, result) =
            request(app.clone(), "GET", &format!("/api/timeline{suffix}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["cache-control"], "no-store");
        assert_eq!(result["timezone"], "UTC");
        assert_eq!(result["days"], days);
        assert_eq!(result["total"], 0);
        let buckets = result["buckets"].as_array().unwrap();
        assert_eq!(buckets.len(), days as usize);
        assert_eq!(buckets.first().unwrap()["date"], start);
        assert_eq!(buckets.last().unwrap()["date"], "2025-10-01");
        assert!(buckets.iter().all(|bucket| bucket["count"] == 0));
        assert!(buckets
            .windows(2)
            .all(|pair| pair[0]["date"].as_str() < pair[1]["date"].as_str()));
    }
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn timeline_uses_publication_range_boundaries_and_excludes_future_rows() {
    let (state, _, clock) = fixture();
    let now = iso_timestamp(clock.now());
    seed_dates(
        &state,
        &[
            ("openai", "2025-07-03T23:59:59.999Z", &now),
            ("openai", "2025-07-04T00:00:00.000Z", &now),
            ("openai", "2025-09-01T23:59:59.999Z", &now),
            ("google", "2025-09-02T00:00:00.000Z", &now),
            ("openai", "2025-09-24T23:59:59.999Z", &now),
            ("openai", "2025-09-25T00:00:00.000Z", &now),
            ("google", "2025-09-25T23:59:59.999Z", &now),
            ("openai", "2025-10-01T00:00:00.000Z", "2020-01-01T00:00:00Z"),
            ("openai", &now, "2020-01-01T00:00:00Z"),
            ("google", "2025-10-01T12:00:00.001Z", &now),
            ("openai", "2025-10-02T00:00:00.000Z", &now),
        ],
    )
    .await;
    let app = router(state, None);
    for (days, expected) in [("7", 4), ("30", 6), ("90", 8)] {
        let (_, _, result) =
            request(app.clone(), "GET", &timeline_path(&[("days", days)]), None).await;
        assert_eq!(result["total"], expected);
        let buckets = result["buckets"].as_array().unwrap();
        assert_eq!(
            buckets
                .iter()
                .map(|bucket| bucket["count"].as_u64().unwrap())
                .sum::<u64>(),
            expected
        );
        assert_eq!(buckets.last().unwrap()["count"], 2);
        assert_eq!(
            buckets
                .iter()
                .find(|bucket| bucket["date"] == "2025-09-25")
                .unwrap()["count"],
            2
        );
        assert_eq!(
            buckets
                .iter()
                .find(|bucket| bucket["date"] == "2025-09-26")
                .unwrap()["count"],
            0
        );
    }
    for (sources, expected) in [
        ("openai", 3),
        ("google", 1),
        ("openai,google", 4),
        ("openai,openai", 3),
        ("", 0),
    ] {
        let path = timeline_path(&[
            ("days", "7"),
            ("sources", sources),
            ("category", "研究前沿"),
            ("q", r"10%_\"),
            ("date", "invalid-but-ignored"),
        ]);
        let (status, _, result) = request(app.clone(), "GET", &path, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["total"], expected);
        assert_eq!(result["buckets"].as_array().unwrap().len(), 7);
    }
}

#[tokio::test]
async fn timeline_filters_match_news_and_keep_search_literal() {
    let app = router(search_fixture().await, None);
    for (sources, category, query) in [
        ("openai", "研究前沿", "needle"),
        ("google", "研究前沿", "needle"),
        ("google,openai", "研究前沿", "汉字"),
        ("google", "安全治理", "' OR 1=1 --"),
        ("google,openai", "模型进展", r"%_\"),
        ("google", "", "%"),
        ("", "", ""),
    ] {
        let parameters = [("sources", sources), ("category", category), ("q", query)];
        let (_, _, news) = request(app.clone(), "GET", &news_path(&parameters), None).await;
        let (status, _, timeline) =
            request(app.clone(), "GET", &timeline_path(&parameters), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(timeline["total"], news["pagination"]["total"]);
        assert_eq!(timeline["buckets"][29]["count"], timeline["total"]);
    }
}

#[tokio::test]
async fn timeline_handles_leap_day_and_midnight_rollover() {
    let (state, _, clock) = fixture();
    let now = DateTime::parse_from_rfc3339("2024-03-01T00:00:00Z")
        .unwrap()
        .timestamp_millis();
    clock.0.store(now, Ordering::SeqCst);
    seed_dates(
        &state,
        &[
            ("openai", "2024-02-29T23:59:59.999Z", "2024-03-01T00:00:00Z"),
            ("openai", "2024-03-01T00:00:00.000Z", "2024-03-01T00:00:00Z"),
            ("openai", "2024-03-01T00:00:00.001Z", "2024-03-01T00:00:00Z"),
        ],
    )
    .await;
    let app = router(state, None);
    let (_, _, result) = request(app.clone(), "GET", "/api/timeline?days=7", None).await;
    assert_eq!(result["buckets"][0]["date"], "2024-02-24");
    assert_eq!(
        result["buckets"][5],
        serde_json::json!({"date":"2024-02-29","count":1})
    );
    assert_eq!(
        result["buckets"][6],
        serde_json::json!({"date":"2024-03-01","count":1})
    );
    assert_eq!(result["total"], 2);
    clock.advance(-1);
    let (_, _, result) = request(app, "GET", "/api/timeline?days=7", None).await;
    assert_eq!(result["buckets"][0]["date"], "2024-02-23");
    assert_eq!(
        result["buckets"][6],
        serde_json::json!({"date":"2024-02-29","count":1})
    );
    assert_eq!(result["total"], 1);
}

#[tokio::test]
async fn timeline_validation_and_database_errors_are_sanitized_json() {
    let (state, fetcher, _) = fixture();
    let app = router(state.clone(), None);
    for days in [
        "0",
        "1",
        "8",
        "29",
        "31",
        "91",
        "999999",
        "-7",
        "7.0",
        "07",
        "+7",
        " 7",
        "7 ",
        "seven",
        "",
        "184467440737095516160",
    ] {
        let (status, headers, result) =
            request(app.clone(), "GET", &timeline_path(&[("days", days)]), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "days={days:?}");
        assert_eq!(headers["cache-control"], "no-store");
        assert!(result["error"].is_string());
    }
    for parameters in [
        vec![("category", "unknown".to_string())],
        vec![("q", "x".repeat(201))],
        vec![("q", "abc\0def".to_string())],
    ] {
        let parameters: Vec<_> = parameters
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        let (status, _, result) =
            request(app.clone(), "GET", &timeline_path(&parameters), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(result["error"].is_string());
    }
    for path in [
        "/api/news?limit=no",
        "/api/news?sources=openai&sources=google",
        "/api/timeline?days=7&days=30",
    ] {
        let (status, _, result) = request(app.clone(), "GET", path, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(result["error"], "Invalid query parameters");
    }
    assert_eq!(
        request(app.clone(), "POST", "/api/timeline", None).await.0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        request(app.clone(), "HEAD", "/api/timeline", None).await.0,
        StatusCode::OK
    );
    state
        .db
        .run(|db| {
            db.execute_batch("DROP TABLE articles")
                .map_err(|error| error.to_string())
        })
        .await
        .unwrap();
    let (status, headers, result) = request(app, "GET", "/api/timeline", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(result, serde_json::json!({"error":UNAVAILABLE}));
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn read_apis_fail_closed_for_corrupt_schema_rows_and_transaction_state() {
    for corruption in [
        "DROP TABLE articles",
        "DROP TABLE settings",
        "DROP TABLE articles; CREATE TABLE articles(category TEXT,title TEXT,summary TEXT,published_at TEXT,source TEXT)",
        "INSERT INTO articles(id,title,url,source,category,summary,published_at,fetched_at) VALUES(x'01','title','https://example.test/corrupt','openai','研究前沿','summary','2025-10-01T00:00:00.000Z','2025-10-01T00:00:00.000Z')",
        "BEGIN",
    ] {
        let (state, _, clock) = fixture();
        state.db.0.lock().unwrap().execute_batch(corruption).unwrap();
        let (status, _, result) = request(router(state.clone(), None), "GET", "/api/news", None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{corruption}");
        assert_eq!(result, serde_json::json!({"error": UNAVAILABLE}));
        if corruption == "DROP TABLE settings" {
            assert!(state.db.set_schedule(true).await.is_err());
            assert!(state.refresh().await.is_err());
            assert!(state.db.finish("missing".into(), clock.now(), Vec::new(), details()).await.is_err());
        }
        if corruption == "BEGIN" {
            assert!(state.refresh().await.is_err());
            assert!(state.db.finish("missing".into(), clock.now(), Vec::new(), details()).await.is_err());
        }
    }
    let invalid = Connection::open_in_memory().unwrap();
    invalid
        .execute_batch("CREATE TABLE articles(id TEXT)")
        .unwrap();
    assert!(Database::from_connection(invalid).is_err());
}

#[tokio::test]
async fn database_lock_poisoning_and_worker_panics_return_errors() {
    let (state, _, clock) = fixture();
    let database = state.db.clone();
    assert!(std::thread::spawn(move || {
        let _lock = database.0.lock().unwrap();
        panic!("synthetic private lock failure");
    })
    .join()
    .is_err());
    for path in ["/api/news", "/api/timeline"] {
        let (status, _, result) = request(router(state.clone(), None), "GET", path, None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(result, serde_json::json!({"error": UNAVAILABLE}));
    }
    assert!(state.db.set_schedule(false).await.is_err());
    assert!(state.refresh().await.is_err());
    assert!(state
        .db
        .finish("poison".into(), clock.now(), Vec::new(), details())
        .await
        .is_err());
    let (state, _, _) = fixture();
    assert!(state
        .db
        .run::<()>(|_| panic!("synthetic private worker failure"))
        .await
        .is_err());
}

#[tokio::test]
async fn http_fetcher_uses_only_registry_methods_headers_bodies_and_size_limits() {
    let application = Router::new()
        .route(
            "/post",
            post(|headers: HeaderMap, body: String| async move {
                assert_eq!(headers[header::CONTENT_TYPE], "application/json");
                assert_eq!(headers[header::ACCEPT_LANGUAGE], "zh");
                assert_eq!(body, r#"{"pageNum":1,"pageSize":20,"renderType":0}"#);
                assert!(headers[header::ACCEPT]
                    .to_str()
                    .unwrap()
                    .contains("application/json"));
                "{\"data\":[]}"
            }),
        )
        .route(
            "/extended",
            get(|| async { "x".repeat(MAX_FEED_BYTES + 1) }),
        )
        .route("/bounded", get(|| async { "x".repeat(65) }))
        .route(
            "/stream-bounded",
            get(|| async {
                Body::from_stream(futures_util::stream::iter([
                    Ok::<_, std::io::Error>(vec![b'x'; 32]),
                    Ok(vec![b'x'; 33]),
                ]))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
    let fetcher = HttpFetcher {
        client: reqwest::Client::new(),
    };
    let source = Source {
        url: Box::leak(format!("http://{address}/post").into_boxed_str()),
        method: "POST",
        request_body: Some(r#"{"pageNum":1,"pageSize":20,"renderType":0}"#),
        request_language: Some("zh"),
        ..SOURCES[0]
    };
    assert_eq!(fetcher.fetch(&source).await.unwrap(), "{\"data\":[]}");
    let source = Source {
        method: "DELETE",
        ..source
    };
    assert_eq!(
        fetcher.fetch(&source).await.unwrap_err(),
        "Unsupported source method"
    );
    let source = Source {
        url: Box::leak(format!("http://{address}/extended").into_boxed_str()),
        max_bytes: 6_000_000,
        ..SOURCES[0]
    };
    assert_eq!(
        fetcher.fetch(&source).await.unwrap().len(),
        MAX_FEED_BYTES + 1
    );
    for path in ["bounded", "stream-bounded"] {
        let source = Source {
            url: Box::leak(format!("http://{address}/{path}").into_boxed_str()),
            max_bytes: 64,
            ..SOURCES[0]
        };
        assert_eq!(
            fetcher.fetch(&source).await.unwrap_err(),
            "Feed exceeds 64 byte limit"
        );
    }
    server.abort();
}

#[tokio::test]
async fn per_source_timeout_overrides_default_http_timeout() {
    let application = Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            "source payload"
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
    let fetcher = HttpFetcher {
        client: reqwest::Client::builder()
            .timeout(Duration::from_millis(1))
            .build()
            .unwrap(),
    };
    let source = Source {
        url: Box::leak(format!("http://{address}/slow").into_boxed_str()),
        timeout_seconds: 1,
        ..SOURCES[0]
    };
    assert_eq!(fetcher.fetch(&source).await.unwrap(), "source payload");
    server.abort();
}

#[tokio::test]
async fn refresh_allows_qwens_longer_timeout_before_reporting_source_failure() {
    struct SlowQwen(AtomicUsize);
    impl FeedFetcher for SlowQwen {
        fn fetch<'a>(&'a self, source: &'a Source) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                if source.id == "qwen" {
                    std::future::pending::<Result<String>>().await
                } else {
                    Ok(source_fixture(source.id))
                }
            })
        }
    }
    let (mut state, _, _) = fixture();
    let fetcher = Arc::new(SlowQwen(AtomicUsize::new(0)));
    state.fetcher = fetcher.clone();
    let task = tokio::spawn(async move { state.refresh().await.unwrap() });
    while fetcher.0.load(Ordering::SeqCst) < SOURCES.len() {
        tokio::task::yield_now().await;
    }
    tokio::time::pause();
    tokio::time::advance(FETCH_TIMEOUT + Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert!(!task.is_finished());
    tokio::time::advance(Duration::from_secs(12)).await;
    let result = task.await.unwrap();
    assert_eq!(result.status, "partial");
    assert_eq!(result.processed, Some(SOURCES.len() - 1));
    let details = result.details.unwrap();
    assert_eq!(
        details
            .iter()
            .filter(|detail| detail.status == "error")
            .count(),
        1
    );
    assert_eq!(
        details
            .iter()
            .find(|detail| detail.source == "qwen")
            .unwrap()
            .error
            .as_deref(),
        Some("Feed request timed out")
    );
}
