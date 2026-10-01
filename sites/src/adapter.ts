export interface Article {
  id: string;
  title: string;
  url: string;
  source: string;
  category: string;
  summary: string;
  published_at: string;
  fetched_at: string;
}
export interface Source {
  id: string;
  name: string;
  home: string;
  url?: string;
  method?: "GET" | "POST";
  request_body?: string | null;
  request_language?: string | null;
  max_bytes?: number;
  timeout_seconds?: number;
}
export interface Env {
  DB: D1Database;
  REFRESH_TOKEN_SHA256?: string;
}
export interface Core {
  sources: Source[];
  parse: (source: string, xml: string, now: string) => Article[];
}
export interface Dependencies {
  fetchFeed?: typeof fetch;
  now?: () => number;
  uuid?: () => string;
}
const json = (data: unknown, status = 200) =>
  Response.json(data, {
    status,
    headers: {
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
    },
  });
export async function tokenHash(token: string): Promise<string> {
  return [
    ...new Uint8Array(
      await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token)),
    ),
  ]
    .map((x) => x.toString(16).padStart(2, "0"))
    .join("");
}
export async function authorized(
  req: Request,
  expected?: string,
): Promise<boolean> {
  if (!expected || !/^[a-f0-9]{64}$/.test(expected)) return false;
  const value = req.headers.get("authorization");
  if (!value?.startsWith("Bearer ") || value.length > 4096) return false;
  const actual = await tokenHash(value.slice(7));
  let diff = 0;
  for (let i = 0; i < 64; i++)
    diff |= actual.charCodeAt(i) ^ expected.charCodeAt(i);
  return diff === 0;
}
function readFilters(
  params: URLSearchParams,
  sources: Source[],
  includeDate: boolean,
) {
  const keys = includeDate
    ? ["category", "q", "sources", "date"]
    : ["category", "q", "sources"];
  if (keys.some((key) => params.getAll(key).length > 1)) return null;
  const category = params.get("category") ?? "";
  const query = (params.get("q") ?? "").trim();
  if (
    (category &&
      ![
        "模型进展",
        "研究前沿",
        "开发工具",
        "产业动态",
        "安全治理",
        "具身智能",
      ].includes(category)) ||
    [...query].length > 200 ||
    query.includes("\0")
  )
    return null;
  const conditions: string[] = [];
  const values: string[] = [];
  if (category) {
    conditions.push("category = ?");
    values.push(category);
  }
  if (query) {
    conditions.push("(title LIKE ? ESCAPE '!' OR summary LIKE ? ESCAPE '!')");
    const pattern = `%${query.replace(/[!%_]/g, "!$&")}%`;
    values.push(pattern, pattern);
  }
  const selection = params.get("sources");
  if (selection !== null) {
    if (new TextEncoder().encode(selection).length > 2048) return null;
    const raw = selection.split(",");
    if (raw.length > 32) return null;
    const ids = [...new Set(raw.map((id) => id.trim()).filter(Boolean))];
    if (ids.some((id) => !sources.some((source) => source.id === id)))
      return null;
    conditions.push(
      ids.length ? `source IN (${ids.map(() => "?").join(",")})` : "0 = 1",
    );
    values.push(...ids);
  }
  const date = params.get("date");
  if (includeDate && date !== null) {
    const time = Date.parse(`${date}T00:00:00.000Z`);
    if (
      !/^\d{4}-\d{2}-\d{2}$/.test(date) ||
      !Number.isFinite(time) ||
      new Date(time).toISOString().slice(0, 10) !== date
    )
      return null;
    conditions.push("substr(published_at,1,10) = ?");
    values.push(date);
  }
  return { conditions, values };
}
export function createApi(core: Core, deps: Dependencies = {}) {
  const getTime = deps.now ?? Date.now,
    fetchFeed = deps.fetchFeed ?? fetch,
    uuid = deps.uuid ?? (() => crypto.randomUUID());
  async function ingest(env: Env) {
    const now = getTime(),
      started = new Date(now).toISOString(),
      id = uuid(),
      lease = `${now + 300000}:${id}`;
    const lock = await env.DB.prepare(
      "INSERT INTO settings(key,value) VALUES('ingest_lease',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE CAST(settings.value AS INTEGER) < ?",
    )
      .bind(lease, now)
      .run();
    if (!lock.meta.changes)
      return { status: "busy", processed: 0, inserted: 0 };
    try {
      await env.DB.prepare(
        "UPDATE runs SET status='failed',finished_at=? WHERE status='running'",
      )
        .bind(started)
        .run();
      const recent = await env.DB.prepare(
        "SELECT finished_at FROM runs WHERE status IN ('success','partial') ORDER BY started_at DESC LIMIT 1",
      ).first<{ finished_at: string }>();
      if (recent && now - Date.parse(recent.finished_at) < 3000000)
        return { status: "not_due", processed: 0, inserted: 0 };
      await env.DB.prepare(
        "INSERT INTO runs(id,started_at,status) VALUES(?,?,'running')",
      )
        .bind(id, started)
        .run();
      const details = await Promise.all(
        core.sources.map(async (source) => {
          try {
            const url = source.url;
            if (!url) throw Error("Unknown source");
            const res = await fetchFeed(url, {
              method: source.method ?? "GET",
              body: source.request_body ?? undefined,
              headers: {
                "User-Agent": "AI-Signal/2.0 (official AI news aggregation)",
                Accept:
                  "application/rss+xml,application/xml,application/json,text/html,text/markdown",
                ...(source.request_body
                  ? { "Content-Type": "application/json" }
                  : {}),
                ...(source.request_language
                  ? { "Accept-Language": source.request_language }
                  : {}),
              },
              signal: AbortSignal.timeout(
                Math.min(source.timeout_seconds ?? 18, 30) * 1000,
              ),
            });
            if (!res.ok) throw Error(`HTTP ${res.status}`);
            const reader = res.body?.getReader();
            if (!reader) throw Error("Empty feed");
            let size = 0;
            const chunks: Uint8Array[] = [];
            while (true) {
              const { done, value } = await reader.read();
              if (done) break;
              size += value.byteLength;
              if (size > Math.min(source.max_bytes ?? 3000000, 6000000)) {
                await reader.cancel();
                throw Error("Feed too large");
              }
              chunks.push(value);
            }
            const bytes = new Uint8Array(size);
            let pos = 0;
            for (const c of chunks) {
              bytes.set(c, pos);
              pos += c.length;
            }
            const articles = core.parse(
              source.id,
              new TextDecoder().decode(bytes),
              started,
            );
            let inserted = 0;
            for (const a of articles) {
              const result = await env.DB.prepare(
                "INSERT OR IGNORE INTO articles(id,title,url,source,category,summary,published_at,fetched_at) VALUES(?,?,?,?,?,?,?,?)",
              )
                .bind(
                  a.id,
                  a.title,
                  a.url,
                  a.source,
                  a.category,
                  a.summary,
                  a.published_at,
                  a.fetched_at,
                )
                .run();
              inserted += result.meta.changes;
              if (!result.meta.changes)
                await env.DB.prepare(
                  "UPDATE articles SET title=?,summary=?,category=?,fetched_at=? WHERE url=?",
                )
                  .bind(a.title, a.summary, a.category, a.fetched_at, a.url)
                  .run();
            }
            return {
              source: source.id,
              status: "ok",
              items: articles.length,
              inserted,
              checked_at: started,
            };
          } catch (error) {
            return {
              source: source.id,
              status: "error",
              items: 0,
              inserted: 0,
              error: error instanceof Error ? error.message : "Feed failed",
              checked_at: started,
            };
          }
        }),
      );
      const successes = details.filter((x) => x.status === "ok").length,
        status =
          successes === core.sources.length
            ? "success"
            : successes
              ? "partial"
              : "failed";
      const processed = details.reduce((n, x) => n + x.items, 0),
        inserted = details.reduce((n, x) => n + x.inserted, 0);
      await env.DB.prepare(
        "UPDATE runs SET finished_at=?,status=?,added=?,details=? WHERE id=?",
      )
        .bind(
          new Date(getTime()).toISOString(),
          status,
          inserted,
          JSON.stringify(details),
          id,
        )
        .run();
      return { status, processed, inserted, details };
    } finally {
      await env.DB.prepare(
        "UPDATE settings SET value='0' WHERE key='ingest_lease' AND value=?",
      )
        .bind(lease)
        .run();
    }
  }
  return async function handle(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url);
    try {
      if (url.pathname === "/healthz")
        return json({ status: "ok", runtime: "rust-wasm" });
      if (url.pathname === "/api/news") {
        if (req.method !== "GET")
          return json({ error: "Method not allowed" }, 405);
        const number = (key: string, fallback: number, max: number) => {
          const v = url.searchParams.get(key);
          return v !== null && /^\d+$/.test(v)
            ? Math.min(Number(v), max)
            : fallback;
        };
        const limit = Math.max(1, number("limit", 50, 350)),
          offset = number("offset", 0, 1000000);
        const filters = readFilters(url.searchParams, core.sources, true);
        if (!filters) return json({ error: "Invalid news filters" }, 400);
        const { conditions, values } = filters;
        const where = conditions.length
          ? ` WHERE ${conditions.join(" AND ")}`
          : "";
        const articles = (
          await env.DB.prepare(
            `SELECT * FROM articles${where} ORDER BY published_at DESC,id ASC LIMIT ? OFFSET ?`,
          )
            .bind(...values, limit, offset)
            .all()
        ).results;
        const matched = await env.DB.prepare(
          `SELECT COUNT(*) AS count FROM articles${where}`,
        )
          .bind(...values)
          .first<{ count: number }>();
        const total = await env.DB.prepare(
          "SELECT COUNT(*) AS count FROM articles",
        ).first<{ count: number }>();
        const recent = await env.DB.prepare(
          "SELECT COUNT(*) AS count FROM articles WHERE published_at >= ? AND published_at <= ?",
        )
          .bind(
            new Date(getTime() - 86400000).toISOString(),
            new Date(getTime()).toISOString(),
          )
          .first<{ count: number }>();
        const run = await env.DB.prepare(
          "SELECT * FROM runs ORDER BY started_at DESC LIMIT 1",
        ).first<Record<string, unknown>>();
        const schedule = await env.DB.prepare(
          "SELECT value FROM settings WHERE key='schedule'",
        ).first<{ value: string }>();
        return json({
          articles,
          sources: core.sources.map(({ id, name, home }) => ({
            id,
            name,
            home,
          })),
          run: run
            ? { ...run, details: JSON.parse(String(run.details)) }
            : null,
          schedule: schedule?.value ?? "尚未启用",
          pagination: {
            total: matched?.count ?? 0,
            limit,
            offset,
            has_more: offset + articles.length < (matched?.count ?? 0),
          },
          stats: {
            total_articles: total?.count ?? 0,
            recent_articles: recent?.count ?? 0,
            total_sources: core.sources.length,
          },
        });
      }
      if (url.pathname === "/api/timeline") {
        if (req.method !== "GET")
          return json({ error: "Method not allowed" }, 405);
        const daysParam = url.searchParams.get("days") ?? "30";
        if (
          url.searchParams.getAll("days").length > 1 ||
          !["7", "30", "90"].includes(daysParam)
        )
          return json({ error: "Invalid timeline range" }, 400);
        const days = Number(daysParam);
        const filters = readFilters(url.searchParams, core.sources, false);
        if (!filters) return json({ error: "Invalid news filters" }, 400);
        const now = getTime();
        const today = new Date(now).toISOString().slice(0, 10);
        const start =
          Date.parse(`${today}T00:00:00.000Z`) - (days - 1) * 86400000;
        filters.conditions.push("published_at >= ?", "published_at <= ?");
        filters.values.push(
          new Date(start).toISOString(),
          new Date(now).toISOString(),
        );
        const rows = (
          await env.DB.prepare(
            `SELECT substr(published_at,1,10) AS date,COUNT(*) AS count FROM articles WHERE ${filters.conditions.join(" AND ")} GROUP BY substr(published_at,1,10) ORDER BY date ASC`,
          )
            .bind(...filters.values)
            .all<{ date: string; count: number }>()
        ).results;
        const counts = new Map(rows.map((row) => [row.date, row.count]));
        const buckets = Array.from({ length: days }, (_, i) => {
          const date = new Date(start + i * 86400000)
            .toISOString()
            .slice(0, 10);
          return { date, count: counts.get(date) ?? 0 };
        });
        return json({
          buckets,
          total: buckets.reduce((sum, bucket) => sum + bucket.count, 0),
          timezone: "UTC",
          days,
        });
      }
      if (url.pathname === "/api/refresh") {
        if (req.method !== "POST")
          return json({ error: "Method not allowed" }, 405);
        if (!(await authorized(req, env.REFRESH_TOKEN_SHA256)))
          return json({ error: "Unauthorized" }, 401);
        const origin = req.headers.get("origin");
        if (origin && origin !== url.origin)
          return json({ error: "Forbidden origin" }, 403);
        return json(await ingest(env));
      }
      return json({ error: "Not found" }, 404);
    } catch {
      return json({ error: "Service temporarily unavailable" }, 503);
    }
  };
}
