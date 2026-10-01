# Native Rust server

Portable Axum + Tokio + SQLite backend sharing all feed normalization with
`ai-news-core`. It does not require Cloudflare, Sites, a hosted database, or an AI
API key. SQLite is compiled into the binary.

From the repository root:

```sh
cargo run -p ai-news-server
# Optional: serve the built Vue application from the same origin.
STATIC_DIR=frontend/dist cargo run -p ai-news-server
```

| Environment variable | Default | Meaning |
| --- | --- | --- |
| `BIND_ADDR` | `127.0.0.1:3000` | HTTP listen address; use `0.0.0.0:3000` in containers |
| `DATABASE_PATH` | `news.sqlite3` | Persistent SQLite path; its parent directory must already exist |
| `ADMIN_TOKEN` | unset | Bearer token permitting manual refresh; unset/empty denies every HTTP write |
| `STATIC_DIR` | unset | Optional built frontend directory |
| `ENABLE_SCHEDULER` | `true` | Set exactly `false` to disable startup/hourly refresh |

Use a long random `ADMIN_TOKEN` supplied through your deployment secret manager.
Never put it in Vue build-time variables, source code, URLs, or committed files.
Public read-only clients do not need a token. Use HTTPS at a reverse proxy when
exposing the server beyond localhost; forwarded headers do not grant permissions.
There are no permissive CORS headers or cookie-based writer credentials.

## API

- `GET /healthz`: process liveness, `{"status":"ok"}`
- `GET /api/news?limit=350&offset=0`: public articles sorted newest first, sources,
  latest run, schedule, pagination and database-backed stats. Optional `category`
  exactly matches one of 模型进展、研究前沿、开发工具、安全治理、具身智能、产业动态;
  omit it or use an empty value for all categories. Optional `q` searches literal
  substrings in titles or summaries, case-insensitively for ASCII letters (SQLite
  LIKE semantics). `%`, `_` and backslash are literal characters. Queries are
  trimmed and limited to 200 Unicode characters; invalid categories and longer
  trimmed queries return JSON HTTP 400. NUL characters are rejected as invalid
  rather than allowing SQLite to truncate the search pattern. Optional `sources`
  is a comma-separated list of configured source IDs; omitting it selects every
  source, including newly configured sources, while `sources=` selects none.
  IDs are trimmed and deduplicated, and empty comma segments are ignored. The raw
  value is capped at 2,048 UTF-8 bytes and 32 comma-separated entries before
  deduplication. Unknown IDs return JSON HTTP 400. Optional `date=YYYY-MM-DD`
  restricts publication to that UTC calendar day; malformed or impossible dates
  return JSON HTTP 400. All filters apply across the complete
  database before pagination. `pagination.total` is the filtered count; `stats`
  always describes the entire database. Limits clamp to
  1–350; offsets start at zero. `recent_articles` counts publications from the
  rolling preceding 24 hours. `total_sources` is the current configured registry
- `GET /api/timeline?days=30`: public database-backed daily publication counts.
  `days` accepts 7, 30, or 90 and defaults to 30; other values return JSON HTTP 400.
  The response is `{buckets:[{date:"YYYY-MM-DD",count:0}],total,timezone:"UTC",days}`.
  Buckets are ascending and include every day, even days with no articles,
  ending today in UTC. Counts use stored `published_at`, never `fetched_at`, and
  exclude records before the first day's UTC midnight or after the current time.
  `total` is the sum of the daily counts. The same `category`, `q`, and `sources`
  filters apply; `date`, `limit`, and `offset` do not restrict the timeline.
  Reading the timeline never triggers publisher requests. Both read APIs return
  sanitized JSON HTTP 503 when the database is unavailable
- `POST /api/refresh`: requires `Authorization: Bearer <ADMIN_TOKEN>`. Returns
  `success`, `partial`, or `failed`, with `processed`, `inserted` and source details, or
  `busy` / `not_due` with `added: 0`. `processed` counts normalized/upserted
  articles; `inserted` and `run.added` count only newly inserted URLs

The existing article/category/run JSON fields are retained. The schedule is
runtime-owned: there is no HTTP endpoint to enable or disable it. Readiness of
an individual publisher is represented by its latest refresh detail, not by
`/healthz`. Publisher failure never removes already saved articles.

## Reliability and security

Schema initialization creates the same `articles`, `runs`, and `settings` tables
and indexes as the original application. SQL values use bound parameters. Every
successful refresh writes articles, run details and lock release in one SQLite
transaction. Database operations run in Tokio's blocking pool.

The scheduler refreshes at startup, then hourly; missed ticks are skipped.
Ctrl+C and Unix SIGTERM stop HTTP serving gracefully, wait for an active refresh
to finish, and clear the scheduler status. Signal setup errors are propagated. A
five-minute SQLite lease coordinates concurrent requests and independent server
processes using the same database. The lease has an owner identifier, so a stale
worker cannot write results or release a newer worker's lease. After a successful
or partial refresh, requests inside 50 minutes are no-ops. All-source failures
can be retried immediately. Interrupted owners recover after lease expiry.

Publisher requests are concurrent and HTTPS-only, with an 18-second timeout
except for Qwen's 30-second timeout. These limits apply to both the HTTP request
and the outer refresh task. Source-specific byte limits
apply both to declared response size and streamed bytes: 3 MB normally and 6 MB
for Qwen's verified larger index. Redirects are refused rather than following
publisher-controlled destinations. The core registry controls each endpoint,
GET/POST method, optional JSON request body, language header, and response bound;
read API parameters cannot override these values. If a publisher changes its
endpoint, update the allowlist in `news-core`; old data remains available meanwhile.
Responses must be UTF-8. The shared core parses RSS 2.0, official
HTML/MDX indexes, and JSON indexes, normalizes text/URLs, and applies the original
Chinese category precedence.

Token comparison hashes both tokens then constant-time compares fixed-size
SHA-256 digests. Missing configuration fails closed. API error responses are
sanitized; internal background failures are logged without the token. Security
headers and restrictive CSP also cover static assets. A browser disconnect does
not cancel an already-authorized refresh.

## Verification

```sh
cargo test -p ai-news-server
cargo clippy -p ai-news-server --all-targets -- -D warnings
cargo llvm-cov -p ai-news-server --all-targets --summary-only
```

Tests inject fake clocks and synthetic feeds; HTTP adapter tests use only a
loopback mock server. No test calls live publishers. Coverage values must be
measured, not inferred from test counts.
