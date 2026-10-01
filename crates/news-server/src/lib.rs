//! Portable native runtime. HTTP and clocks are injectable; tests never contact publishers.

use ai_news_core::{
    iso_timestamp, normalize_feed, public_sources, Article, PublicSource, Source, MAX_FEED_BYTES,
    SOURCES,
};
use axum::{
    extract::{rejection::QueryRejection, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use chrono::{DateTime, Days, NaiveDate, Utc};
use futures_util::{
    future::{join_all, BoxFuture},
    StreamExt,
};
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension,
    TransactionBehavior,
};
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
const CATEGORIES: [&str; 6] = [
    "模型进展",
    "研究前沿",
    "开发工具",
    "安全治理",
    "具身智能",
    "产业动态",
];
const FILTERED_ARTICLES: &str = r"FROM articles WHERE (?1 IS NULL OR category = ?1) AND (?2 IS NULL OR title LIKE ?2 ESCAPE '\' OR summary LIKE ?2 ESCAPE '\')";
const MAX_SOURCE_QUERY_BYTES: usize = 2048;
const MAX_SOURCE_QUERY_ENTRIES: usize = 32;
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
            let mut request = match source.method {
                "GET" => self.client.get(source.url),
                "POST" => self.client.post(source.url),
                _ => return Err("Unsupported source method".into()),
            };
            if let Some(body) = source.request_body {
                request = request
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body);
            }
            if let Some(language) = source.request_language {
                request = request.header(header::ACCEPT_LANGUAGE, language);
            }
            let limit_error = || {
                if source.max_bytes == MAX_FEED_BYTES {
                    "Feed exceeds 3 MB limit".to_string()
                } else {
                    format!("Feed exceeds {} byte limit", source.max_bytes)
                }
            };
            let response = request
                .timeout(Duration::from_secs(source.timeout_seconds))
                .header(header::ACCEPT, "application/rss+xml, application/atom+xml, application/json, application/xml, text/xml, text/html")
                .send()
                .await
                .map_err(|_| "Feed request failed".to_string())?;
            if !response.status().is_success() {
                return Err(format!("HTTP {}", response.status().as_u16()));
            }
            if response
                .content_length()
                .is_some_and(|n| n > source.max_bytes as u64)
            {
                return Err(limit_error());
            }
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| "Feed response interrupted".to_string())?;
                if bytes.len().saturating_add(chunk.len()) > source.max_bytes {
                    return Err(limit_error());
                }
                bytes.extend_from_slice(&chunk);
            }
            String::from_utf8(bytes).map_err(|_| "Feed is not UTF-8".into())
        })
    }
}

// SQLite integers are signed 64-bit values; public counts stay platform-sized.
// Keep conversions checked now that rusqlite no longer implements usize codecs.
fn sqlite_integer(value: usize) -> Result<i64> {
    i64::try_from(value).map_err(|_| "Value exceeds SQLite integer range".to_string())
}

fn row_count(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<usize> {
    let value: i64 = row.get(column)?;
    usize::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
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
        self.snapshot_filtered(limit, offset, now, NewsFilter::default())
            .await
    }
    async fn snapshot_filtered(
        &self,
        limit: usize,
        offset: usize,
        now: DateTime<Utc>,
        filter: NewsFilter,
    ) -> Result<News> {
        let limit = limit.clamp(1, 350);
        let offset = offset.min(usize::try_from(i64::MAX).unwrap_or(usize::MAX));
        let sql_limit = sqlite_integer(limit)?;
        let sql_offset = sqlite_integer(offset)?;
        let (filtered_sql, mut values) = filter.sql();
        self.run(move |connection| {
            // One read transaction keeps rows, totals, and run metadata coherent across processes.
            let db = connection.transaction().map_err(|e| e.to_string())?;
            let filtered_total: usize = db.query_row(&format!("SELECT COUNT(*) {filtered_sql}"), params_from_iter(&values), |row| row_count(row, 0)).map_err(|e| e.to_string())?;
            values.extend([SqlValue::Integer(sql_limit), SqlValue::Integer(sql_offset)]);
            let mut statement = db.prepare(&format!("SELECT id,title,url,source,category,summary,published_at,fetched_at {filtered_sql} ORDER BY published_at DESC,id ASC LIMIT ? OFFSET ?")).map_err(|e| e.to_string())?;
            let articles = statement.query_map(params_from_iter(&values), |row| Ok(Article { id: row.get(0)?, title: row.get(1)?, url: row.get(2)?, source: row.get(3)?, category: row.get(4)?, summary: row.get(5)?, published_at: row.get(6)?, fetched_at: row.get(7)? }))
                .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
            let run = db.query_row("SELECT id,started_at,finished_at,status,added,details FROM runs ORDER BY started_at DESC,rowid DESC LIMIT 1", [], |row| {
                let details: String = row.get(5)?;
                Ok(Run { id: row.get(0)?, started_at: row.get(1)?, finished_at: row.get(2)?, status: row.get(3)?, added: row_count(row, 4)?, details: serde_json::from_str(&details).unwrap_or_default() })
            }).optional().map_err(|e| e.to_string())?;
            let schedule = db.query_row("SELECT value FROM settings WHERE key='schedule'", [], |row| row.get(0)).optional().map_err(|e| e.to_string())?.unwrap_or_else(|| "尚未启用".into());
            let (total, recent): (usize, usize) = db.query_row("SELECT COUNT(*),COALESCE(SUM(published_at >= ? AND published_at <= ?),0) FROM articles", [iso_timestamp(now - chrono::Duration::days(1)), iso_timestamp(now)], |row| Ok((row_count(row, 0)?, row_count(row, 1)?))).map_err(|e| e.to_string())?;
            Ok(News { articles, sources: public_sources(), run, schedule, pagination: Pagination { total: filtered_total, limit, offset, has_more: offset.saturating_add(limit) < filtered_total }, stats: Stats { total_articles: total, recent_articles: recent, total_sources: SOURCES.len() } })
        }).await
    }
    async fn timeline(
        &self,
        days: usize,
        now: DateTime<Utc>,
        filter: NewsFilter,
    ) -> Result<Timeline> {
        let start = now
            .date_naive()
            .checked_sub_days(Days::new((days - 1) as u64))
            .ok_or("Timeline range exceeds calendar bounds")?;
        let (mut sql, mut values) = filter.sql();
        // Ingest normalizes every timestamp to millisecond-precision UTC. These
        // lexicographic bounds keep the published_at index usable and exclude futures.
        sql.push_str(" AND published_at >= ? AND published_at <= ?");
        values.push(SqlValue::Text(format!("{start}T00:00:00.000Z")));
        values.push(SqlValue::Text(iso_timestamp(now)));
        self.run(move |db| {
            let mut statement = db
                .prepare(&format!("SELECT substr(published_at,1,10),COUNT(*) {sql} GROUP BY substr(published_at,1,10) ORDER BY substr(published_at,1,10)"))
                .map_err(|e| e.to_string())?;
            let counts = statement
                .query_map(params_from_iter(&values), |row| Ok((row.get::<_, String>(0)?, row_count(row, 1)?)))
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()
                .map_err(|e| e.to_string())?;
            let buckets: Vec<_> = start.iter_days().take(days).map(|date| {
                let date = date.to_string();
                let count = counts.get(&date).copied().unwrap_or(0);
                TimelineBucket { date, count }
            }).collect();
            let total = buckets.iter().try_fold(0usize, |total, bucket| {
                total.checked_add(bucket.count).ok_or("Timeline count exceeds integer range")
            })?;
            Ok(Timeline { buckets, total, timezone: "UTC", days })
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
            let mut inserted = 0;
            for article in &articles {
                // Ignore only an existing URL; unrelated constraint failures still abort the transaction.
                let added = tx.execute("INSERT INTO articles(id,title,url,source,category,summary,published_at,fetched_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(url) DO NOTHING", params![article.id, article.title, article.url, article.source, article.category, article.summary, article.published_at, article.fetched_at]).map_err(|e| e.to_string())?;
                inserted += added;
                if added == 0 {
                    tx.execute("UPDATE articles SET title=?,summary=?,category=?,fetched_at=? WHERE url=?", params![article.title, article.summary, article.category, article.fetched_at, article.url]).map_err(|e| e.to_string())?;
                }
            }
            let good = details.iter().filter(|d| d.status == "ok").count();
            let status = if good == SOURCES.len() { "success" } else if good > 0 { "partial" } else { "failed" };
            tx.execute("UPDATE runs SET finished_at=?,status=?,added=?,details=? WHERE id=?", params![iso_timestamp(now), status, sqlite_integer(inserted)?, serde_json::to_string(&details).map_err(|e| e.to_string())?, id]).map_err(|e| e.to_string())?;
            tx.execute("UPDATE settings SET value='0' WHERE key='ingest_lease'", []).map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM settings WHERE key='ingest_owner'", []).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(Refresh { status: status.into(), processed: Some(articles.len()), inserted: Some(inserted), added: None, details: Some(details) })
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
#[derive(Debug, Serialize)]
pub struct Timeline {
    pub buckets: Vec<TimelineBucket>,
    pub total: usize,
    pub timezone: &'static str,
    pub days: usize,
}
#[derive(Debug, Serialize)]
pub struct TimelineBucket {
    pub date: String,
    pub count: usize,
}
#[derive(Default, serde::Deserialize)]
struct NewsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    category: Option<String>,
    q: Option<String>,
    sources: Option<String>,
    date: Option<String>,
}
#[derive(Default, serde::Deserialize)]
struct TimelineQuery {
    category: Option<String>,
    q: Option<String>,
    sources: Option<String>,
    days: Option<String>,
}
#[derive(Default)]
struct NewsFilter {
    category: Option<String>,
    pattern: Option<String>,
    sources: Option<Vec<String>>,
    date: Option<String>,
}
impl NewsFilter {
    fn new(
        category: Option<String>,
        query: Option<String>,
        sources: Option<String>,
        date: Option<String>,
    ) -> Result<Self> {
        let category = category.filter(|value| !value.is_empty());
        if category
            .as_deref()
            .is_some_and(|value| !CATEGORIES.contains(&value))
        {
            return Err("Unknown category".into());
        }
        let query = query.unwrap_or_default();
        let query = query.trim();
        if query.chars().count() > 200 {
            return Err("Search query must be at most 200 characters".into());
        }
        if query.contains('\0') {
            return Err("Search query must not contain null characters".into());
        }
        // Escape the escape character first, then the two LIKE wildcards.
        let pattern = (!query.is_empty()).then(|| {
            format!(
                "%{}%",
                query
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            )
        });
        let sources = sources
            .map(|input| {
                if input.len() > MAX_SOURCE_QUERY_BYTES {
                    return Err("Sources query must be at most 2048 bytes".into());
                }
                let mut selected = Vec::new();
                if !input.trim().is_empty() {
                    for (index, id) in input.split(',').enumerate() {
                        if index >= MAX_SOURCE_QUERY_ENTRIES {
                            return Err("Sources query must contain at most 32 entries".into());
                        }
                        let id = id.trim();
                        if id.is_empty() {
                            continue;
                        }
                        if !SOURCES.iter().any(|source| source.id == id) {
                            return Err("Unknown source".into());
                        }
                        if !selected.iter().any(|source| source == id) {
                            selected.push(id.to_string());
                        }
                    }
                }
                Ok::<_, String>(selected)
            })
            .transpose()?;
        if let Some(date) = &date {
            let canonical = date.len() == 10
                && date.bytes().enumerate().all(|(index, byte)| {
                    if index == 4 || index == 7 {
                        byte == b'-'
                    } else {
                        byte.is_ascii_digit()
                    }
                });
            if !canonical || NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
                return Err("Date must be a valid UTC calendar date in YYYY-MM-DD format".into());
            }
        }
        Ok(Self {
            category,
            pattern,
            sources,
            date,
        })
    }

    fn sql(&self) -> (String, Vec<SqlValue>) {
        let mut sql = FILTERED_ARTICLES.to_string();
        let mut values = vec![self.category.clone().into(), self.pattern.clone().into()];
        if let Some(sources) = &self.sources {
            if sources.is_empty() {
                sql.push_str(" AND 0");
            } else {
                sql.push_str(&format!(
                    " AND source IN ({})",
                    vec!["?"; sources.len()].join(",")
                ));
                values.extend(sources.iter().cloned().map(SqlValue::Text));
            }
        }
        if let Some(date) = &self.date {
            sql.push_str(" AND published_at >= ? AND published_at <= ?");
            values.push(SqlValue::Text(format!("{date}T00:00:00.000Z")));
            values.push(SqlValue::Text(format!("{date}T23:59:59.999Z")));
        }
        (sql, values)
    }
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
    pub inserted: Option<usize>,
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
            inserted: None,
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
            let fetched = tokio::time::timeout(
                Duration::from_secs(source.timeout_seconds),
                self.fetcher.fetch(source),
            )
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

async fn news(
    State(state): State<AppState>,
    query: std::result::Result<Query<NewsQuery>, QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid query parameters"),
    };
    let filter = match NewsFilter::new(query.category, query.q, query.sources, query.date) {
        Ok(filter) => filter,
        Err(message) => return error(StatusCode::BAD_REQUEST, &message),
    };
    match state
        .db
        .snapshot_filtered(
            query.limit.unwrap_or(350),
            query.offset.unwrap_or(0),
            state.clock.now(),
            filter,
        )
        .await
    {
        Ok(news) => Json(news).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, UNAVAILABLE),
    }
}
async fn timeline(
    State(state): State<AppState>,
    query: std::result::Result<Query<TimelineQuery>, QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return error(StatusCode::BAD_REQUEST, "Invalid query parameters"),
    };
    let days = match query.days.as_deref().unwrap_or("30") {
        "7" => 7,
        "30" => 30,
        "90" => 90,
        _ => return error(StatusCode::BAD_REQUEST, "Days must be 7, 30, or 90"),
    };
    let filter = match NewsFilter::new(query.category, query.q, query.sources, None) {
        Ok(filter) => filter,
        Err(message) => return error(StatusCode::BAD_REQUEST, &message),
    };
    match state.db.timeline(days, state.clock.now(), filter).await {
        Ok(timeline) => Json(timeline).into_response(),
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
        .route("/api/timeline", get(timeline))
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
