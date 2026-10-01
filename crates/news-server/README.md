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
  latest run, schedule, pagination and database-backed stats. Limits clamp to
  1–350; offsets start at zero. `recent_articles` counts publications from the
  rolling preceding 24 hours. `total_sources` is the five configured publishers
- `POST /api/refresh`: requires `Authorization: Bearer <ADMIN_TOKEN>`. Returns
  `success`, `partial`, or `failed`, with `processed` and source details, or
  `busy` / `not_due` with `added: 0`. `processed` counts normalized/upserted
  articles, not exclusively newly inserted rows

The existing article/category/run JSON fields are retained. The schedule is
runtime-owned: there is no HTTP endpoint to enable or disable it. Readiness of
an individual publisher is represented by its latest refresh detail, not by
`/healthz`. Publisher failure never removes already saved articles.

## Reliability and security

Schema initialization creates the same `articles`, `runs`, and `settings` tables
and indexes as the original application. SQL values use bound parameters. Every
successful refresh writes articles, run details and lock release in one SQLite
transaction. Database operations run in Tokio's blocking pool.

The scheduler refreshes at startup, then hourly; missed ticks are skipped. A
five-minute SQLite lease coordinates concurrent requests and independent server
processes using the same database. The lease has an owner identifier, so a stale
worker cannot write results or release a newer worker's lease. After a successful
or partial refresh, requests inside 50 minutes are no-ops. All-source failures
can be retried immediately. Interrupted owners recover after lease expiry.

Publisher requests are concurrent, HTTPS-only, 18-second bounded, and capped at
3 MB both by declared size and streamed bytes. Redirects are refused rather than
following publisher-controlled destinations. If a publisher changes its feed
URL, update the allowlist in `news-core`; old data remains available meanwhile.
Only UTF-8 RSS is accepted by this adapter. The shared core validates XML,
normalizes text/URLs and applies the original Chinese category precedence.

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
