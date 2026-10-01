//! Portable native runtime. HTTP and clocks are injectable; tests never contact publishers.

use ai_news_core::{
    iso_timestamp, normalize_feed, public_sources, Article, PublicSource, Source, MAX_FEED_BYTES,
    SOURCES,
};
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use futures_util::{
    future::{join_all, BoxFuture},
    StreamExt,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::sync::watch;
use tower_http::services::ServeDir;

const LEASE_MS: i64 = 5 * 60 * 1000;
const NOT_DUE_MS: i64 = 50 * 60 * 1000;
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(18);
const UNAVAILABLE: &str = "资讯服务暂时不可用，请稍后重试。已保存的数据不会丢失。";

type Result<T> = std::result::Result<T, String>;

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Implementations must only fetch the supplied allowlisted publisher source.
pub trait FeedFetcher: Send + Sync {
    fn fetch<'a>(&'a self, source: &'a Source) -> BoxFuture<'a, Result<String>>;
}

pub struct HttpFetcher {
    client: reqwest::Client,
}
impl HttpFetcher {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("AI-Signal/1.0 (official-feed aggregator)")
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { client })
    }
}
impl FeedFetcher for HttpFetcher {
    fn fetch<'a>(&'a self, source: &'a Source) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let response = self
                .client
                .get(source.url)
                .header(
                    header::ACCEPT,
                    "application/rss+xml, application/xml, text/xml",
                )
                .send()
                .await
                .map_err(|_| "Feed request failed".to_string())?;
            if !response.status().is_success() {
                return Err(format!("HTTP {}", response.status().as_u16()));
            }
            if response
                .content_length()
                .is_some_and(|n| n > MAX_FEED_BYTES as u64)
            {
                return Err("Feed exceeds 3 MB limit".into());
            }
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| "Feed response interrupted".to_string())?;
                if bytes.len() + chunk.len() > MAX_FEED_BYTES {
                    return Err("Feed exceeds 3 MB limit".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            String::from_utf8(bytes).map_err(|_| "Feed is not UTF-8".into())
        })
    }
}

/// All SQLite work runs on Tokio's blocking pool, never on an async worker.
#[derive(Clone)]
pub struct Database(Arc<Mutex<Connection>>);
impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path).map_err(|e| e.to_string())?)
    }
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory().map_err(|e| e.to_string())?)
    }
    fn from_connection(connection: Connection) -> Result<Self> {
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        connection
            .execute_batch(include_str!("schema.sql"))
            .map_err(|e| e.to_string())?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }
    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let connection = self.0.clone();
        tokio::task::spawn_blocking(move || {
            f(&mut *connection
                .lock()
                .map_err(|_| "Database lock poisoned".to_string())?)
        })
        .await
        .map_err(|e| e.to_string())?
    }
    pub async fn snapshot(&self) -> Result<News> {
        self.snapshot_page(350, 0, Utc::now()).await
    }
    pub async fn snapshot_page(
        &self,
        limit: usize,
        offset: usize,
        now: DateTime<Utc>,
    ) -> Result<News> {
        let limit = limit.clamp(1, 350);
        let offset = offset.min(i64::MAX as usize);
        self.run(move |connection| {
            // One read transaction keeps rows, totals, and run metadata coherent across processes.
            let db = connection.transaction().map_err(|e| e.to_string())?;
            let mut statement = db.prepare("SELECT id,title,url,source,category,summary,published_at,fetched_at FROM articles ORDER BY published_at DESC,id ASC LIMIT ? OFFSET ?").map_err(|e| e.to_string())?;
            let articles = statement.query_map(params![limit, offset], |row| Ok(Article { id: row.get(0)?, title: row.get(1)?, url: row.get(2)?, source: row.get(3)?, category: row.get(4)?, summary: row.get(5)?, published_at: row.get(6)?, fetched_at: row.get(7)? }))
                .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
            let run = db.query_row("SELECT id,started_at,finished_at,status,added,details FROM runs ORDER BY started_at DESC,rowid DESC LIMIT 1", [], |row| {
                let details: String = row.get(5)?;
                Ok(Run { id: row.get(0)?, started_at: row.get(1)?, finished_at: row.get(2)?, status: row.get(3)?, added: row.get(4)?, details: serde_json::from_str(&details).unwrap_or_default() })
            }).optional().map_err(|e| e.to_string())?;
            let schedule = db.query_row("SELECT value FROM settings WHERE key='schedule'", [], |row| row.get(0)).optional().map_err(|e| e.to_string())?.unwrap_or_else(|| "尚未启用".into());
            let (total, recent): (usize, usize) = db.query_row("SELECT COUNT(*),COALESCE(SUM(published_at >= ? AND published_at <= ?),0) FROM articles", [iso_timestamp(now - chrono::Duration::days(1)), iso_timestamp(now)], |row| Ok((row.get(0)?, row.get(1)?))).map_err(|e| e.to_string())?;
            Ok(News { articles, sources: public_sources(), run, schedule, pagination: Pagination { total, limit, offset, has_more: offset.saturating_add(limit) < total }, stats: Stats { total_articles: total, recent_articles: recent, total_sources: SOURCES.len() } })
        }).await
    }
    pub async fn set_schedule(&self, enabled: bool) -> Result<()> {
        self.run(move |db| {
            db.execute("INSERT INTO settings(key,value) VALUES('schedule',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [if enabled { "每小时自动更新" } else { "尚未启用" }]).map_err(|e| e.to_string())?;
            Ok(())
        }).await
    }
    async fn acquire(&self, now: DateTime<Utc>, id: String) -> Result<Option<Refresh>> {
        self.run(move |db| {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
            let lease: Option<String> = tx.query_row("SELECT value FROM settings WHERE key='ingest_lease'", [], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
            if lease.and_then(|v| v.parse::<i64>().ok()).is_some_and(|v| v > now.timestamp_millis()) { return Ok(Some(Refresh::skipped("busy"))); }
            let recent: Option<String> = tx.query_row("SELECT finished_at FROM runs WHERE status IN ('success','partial') ORDER BY finished_at DESC LIMIT 1", [], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
            if recent.and_then(|v| DateTime::parse_from_rfc3339(&v).ok()).is_some_and(|v| now.signed_duration_since(v).num_milliseconds() < NOT_DUE_MS) { return Ok(Some(Refresh::skipped("not_due"))); }
            // A crashed/cancelled previous owner cannot remain 'running' forever.
            tx.execute("UPDATE runs SET status='failed',finished_at=? WHERE status='running'", [iso_timestamp(now)]).map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO settings(key,value) VALUES('ingest_lease',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [(now.timestamp_millis() + LEASE_MS).to_string()]).map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO settings(key,value) VALUES('ingest_owner',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [&id]).map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO runs(id,started_at,status) VALUES(?,?,'running')", params![id, iso_timestamp(now)]).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(None)
        }).await
    }
    async fn finish(
        &self,
        id: String,
        now: DateTime<Utc>,
        articles: Vec<Article>,
        details: Vec<SourceDetail>,
    ) -> Result<Refresh> {
        self.run(move |db| {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
            let owner: Option<String> = tx.query_row("SELECT value FROM settings WHERE key='ingest_owner'", [], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
            if owner.as_deref() != Some(&id) { return Ok(Refresh::skipped("busy")); }
            for article in &articles {
                tx.execute("INSERT INTO articles(id,title,url,source,category,summary,published_at,fetched_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(url) DO UPDATE SET title=excluded.title,summary=excluded.summary,category=excluded.category,fetched_at=excluded.fetched_at", params![article.id, article.title, article.url, article.source, article.category, article.summary, article.published_at, article.fetched_at]).map_err(|e| e.to_string())?;
            }
            let good = details.iter().filter(|d| d.status == "ok").count();
            let status = if good == SOURCES.len() { "success" } else if good > 0 { "partial" } else { "failed" };
            tx.execute("UPDATE runs SET finished_at=?,status=?,added=?,details=? WHERE id=?", params![iso_timestamp(now), status, articles.len(), serde_json::to_string(&details).map_err(|e| e.to_string())?, id]).map_err(|e| e.to_string())?;
            tx.execute("UPDATE settings SET value='0' WHERE key='ingest_lease'", []).map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM settings WHERE key='ingest_owner'", []).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(Refresh { status: status.into(), processed: Some(articles.len()), added: None, details: Some(details) })
        }).await
    }
}

#[derive(Debug, Serialize)]
pub struct News {
    pub articles: Vec<Article>,
    pub sources: Vec<PublicSource>,
    pub run: Option<Run>,
    pub schedule: String,
    pub pagination: Pagination,
    pub stats: Stats,
}
#[derive(Debug, Serialize)]
pub struct Pagination {
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
}
#[derive(Debug, Serialize)]
pub struct Stats {
    pub total_articles: usize,
    pub recent_articles: usize,
    pub total_sources: usize,
}
#[derive(Default, serde::Deserialize)]
struct NewsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct Run {
    pub id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub added: usize,
    pub details: Vec<SourceDetail>,
}
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SourceDetail {
    pub source: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub checked_at: String,
}
#[derive(Debug, Serialize)]
pub struct Refresh {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<SourceDetail>>,
}
impl Refresh {
    fn skipped(status: &str) -> Self {
        Self {
            status: status.into(),
            processed: None,
            added: Some(0),
            details: None,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    fetcher: Arc<dyn FeedFetcher>,
    clock: Arc<dyn Clock>,
    admin_digest: Option<[u8; 32]>,
}
impl AppState {
    pub fn new(
        db: Database,
        fetcher: Arc<dyn FeedFetcher>,
        clock: Arc<dyn Clock>,
        admin_token: Option<&str>,
    ) -> Self {
        Self {
            db,
            fetcher,
            clock,
            admin_digest: admin_token
                .filter(|s| !s.is_empty())
                .map(|s| Sha256::digest(s.as_bytes()).into()),
        }
    }
    fn authorized(&self, headers: &HeaderMap) -> bool {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        match (self.admin_digest, token) {
            (Some(expected), Some(token)) => {
                bool::from(expected.ct_eq(&Sha256::digest(token.as_bytes())[..]))
            }
            _ => false,
        }
    }
    pub async fn refresh(&self) -> Result<Refresh> {
        let start = self.clock.now();
        let id = uuid::Uuid::new_v4().to_string();
        if let Some(skipped) = self.db.acquire(start, id.clone()).await? {
            return Ok(skipped);
        }
        let feeds = join_all(SOURCES.iter().map(|source| async move {
            let fetched = tokio::time::timeout(FETCH_TIMEOUT, self.fetcher.fetch(source))
                .await
                .unwrap_or_else(|_| Err("Feed request timed out".into()));
            let parsed = fetched
                .and_then(|xml| normalize_feed(source, &xml, start).map_err(|e| e.to_string()));
            let mut detail = SourceDetail {
                source: source.id.into(),
                status: "ok".into(),
                items: None,
                error: None,
                checked_at: iso_timestamp(start),
            };
            let articles = match parsed {
                Ok(articles) => {
                    detail.items = Some(articles.len());
                    articles
                }
                Err(error) => {
                    detail.status = "error".into();
                    detail.error = Some(error.chars().take(180).collect());
                    Vec::new()
                }
            };
            (articles, detail)
        }))
        .await;
        let (articles, details): (Vec<_>, Vec<_>) = feeds.into_iter().unzip();
        self.db
            .finish(
                id,
                self.clock.now(),
                articles.into_iter().flatten().collect(),
                details,
            )
            .await
    }
}

async fn news(State(state): State<AppState>, Query(query): Query<NewsQuery>) -> Response {
    match state
        .db
        .snapshot_page(
            query.limit.unwrap_or(350),
            query.offset.unwrap_or(0),
            state.clock.now(),
        )
        .await
    {
        Ok(news) => Json(news).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, UNAVAILABLE),
    }
}
async fn refresh(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !state.authorized(&headers) {
        return error(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    // Detach from the request: a disconnected browser must not interrupt an ingest.
    match tokio::spawn(async move { state.refresh().await }).await {
        Ok(Ok(result)) => Json(result).into_response(),
        _ => error(StatusCode::SERVICE_UNAVAILABLE, UNAVAILABLE),
    }
}
fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({"error": message}))).into_response()
}
async fn not_found() -> Response {
    error(StatusCode::NOT_FOUND, "Not found")
}
async fn headers(request: axum::extract::Request, next: Next) -> Response {
    let api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    for (key, value) in [("x-content-type-options", "nosniff"), ("referrer-policy", "strict-origin-when-cross-origin"), ("content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; base-uri 'self'; object-src 'none'"), ("cache-control", if api { "no-store" } else { "no-cache" })] {
        response.headers_mut().insert(key, HeaderValue::from_static(value));
    }
    response
}
pub fn router(state: AppState, static_dir: Option<PathBuf>) -> Router {
    let app = Router::new()
        .route("/api/news", get(news))
        .route("/api/refresh", post(refresh))
        .route("/api", any(not_found))
        .route("/api/{*path}", any(not_found))
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({"status":"ok"})) }),
        )
        .with_state(state);
    let app = match static_dir {
        Some(path) => app.fallback_service(ServeDir::new(path)),
        None => app.fallback(not_found),
    };
    app.layer(middleware::from_fn(headers))
}

/// Refresh immediately at startup, then hourly. SQLite provides multi-process safety.
/// The scheduler needs no ADMIN_TOKEN: that token authorizes external write requests only.
pub async fn scheduler(state: AppState, mut shutdown: watch::Receiver<bool>) -> Result<()> {
    state.db.set_schedule(true).await?;
    let mut timer = tokio::time::interval(REFRESH_INTERVAL);
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = timer.tick() => { if let Err(error) = state.refresh().await { eprintln!("Background refresh failed: {error}"); } }
        }
    }
    state.db.set_schedule(false).await
}

#[cfg(test)]
mod tests;
