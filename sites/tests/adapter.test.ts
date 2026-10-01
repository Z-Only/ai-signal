import { describe, it, expect, vi } from "vitest";
import { DatabaseSync } from "node:sqlite";
import { readFileSync } from "node:fs";
import {
  createApi,
  authorized,
  tokenHash,
  type Core,
  type Env,
  type Article,
  type Source,
} from "../src/adapter";
import { createSite, createRustCore } from "../src/runtime";
const now = Date.parse("2026-10-01T05:00:00Z");
const article: Article = {
  id: "one",
  title: "New model",
  url: "https://openai.com/news/model",
  source: "openai",
  category: "模型进展",
  summary: "Original excerpt",
  published_at: new Date(now).toISOString(),
  fetched_at: new Date(now).toISOString(),
};
const source: Source = {
  id: "openai",
  name: "OpenAI",
  home: "https://openai.com/news/",
  url: "https://openai.com/news/rss.xml",
};
function database() {
  const db = new DatabaseSync(":memory:");
  db.exec(readFileSync("../drizzle/0000_dear_carlie_cooper.sql", "utf8"));
  function prepare(sql: string) {
    let values: unknown[] = [];
    return {
      bind(...v: unknown[]) {
        values = v;
        return this;
      },
      async run() {
        const r = db.prepare(sql).run(...(values as any[]));
        return { meta: { changes: Number(r.changes) } };
      },
      async all() {
        return { results: db.prepare(sql).all(...(values as any[])) };
      },
      async first() {
        return db.prepare(sql).get(...(values as any[])) ?? null;
      },
    };
  }
  return { db, DB: { prepare } as unknown as D1Database };
}
async function setup(
  options: {
    sources?: Source[];
    parse?: Core["parse"];
    fetchFeed?: any;
  } = {},
) {
  const d = database();
  const env: Env = {
    DB: d.DB,
    REFRESH_TOKEN_SHA256: await tokenHash("test-token"),
  };
  let time = now;
  let seq = 0;
  const core: Core = {
    sources: options.sources ?? [source],
    parse: options.parse ?? (() => [article]),
  };
  const fetchFeed =
    options.fetchFeed ?? vi.fn(async () => new Response("<rss/>"));
  const api = createApi(core, {
    now: () => time,
    uuid: () => `run-${++seq}`,
    fetchFeed,
  });
  return {
    api,
    env,
    db: d.db,
    core,
    fetchFeed,
    advance: () => {
      time += 3600000;
    },
    async refresh(
      headers: Record<string, string> = { Authorization: "Bearer test-token" },
    ) {
      return api(
        new Request("https://site.test/api/refresh", {
          method: "POST",
          headers,
        }),
        env,
      );
    },
    async news(query = "") {
      return (
        await api(new Request("https://site.test/api/news" + query), env)
      ).json();
    },
  };
}
describe("authorization", () => {
  it("fails closed for missing invalid and wrong credentials", async () => {
    for (const hash of [undefined, "abc", "F".repeat(64)])
      expect(await authorized(new Request("https://site.test"), hash)).toBe(
        false,
      );
    const hash = await tokenHash("good");
    for (const auth of [
      "",
      "Basic good",
      "Bearer wrong",
      "Bearer " + "a".repeat(4096),
    ])
      expect(
        await authorized(
          new Request("https://site.test", {
            headers: { Authorization: auth },
          }),
          hash,
        ),
      ).toBe(false);
    expect(
      await authorized(
        new Request("https://site.test", {
          headers: { Authorization: "Bearer good" },
        }),
        hash,
      ),
    ).toBe(true);
  });
});
describe("HTTP and database", () => {
  it("public reads return persisted records true counts and bounded pagination", async () => {
    const s = await setup();
    const empty: any = await s.news();
    expect(empty.articles).toEqual([]);
    expect(empty.run).toBeNull();
    expect(empty.schedule).toBe("尚未启用");
    expect(empty.stats.total_articles).toBe(0);
    expect((await s.refresh()).status).toBe(200);
    const news: any = await s.news("?limit=1&offset=0");
    expect(news.articles[0].id).toBe("one");
    expect(news.stats).toEqual({
      total_articles: 1,
      recent_articles: 1,
      total_sources: 1,
    });
    expect(news.pagination).toEqual({
      total: 1,
      offset: 0,
      limit: 1,
      has_more: false,
    });
    expect(news.run.status).toBe("success");
    expect(
      ((await s.news("?limit=0&offset=999999999")) as any).pagination,
    ).toEqual({ total: 1, offset: 1000000, limit: 1, has_more: false });
    expect(((await s.news("?limit=-1&offset=x")) as any).pagination.limit).toBe(
      50,
    );
    expect(((await s.news("?limit=999")) as any).pagination.limit).toBe(350);
    s.db.exec("INSERT INTO settings VALUES('schedule','hourly')");
    expect(((await s.news()) as any).schedule).toBe("hourly");
  });
  it("filters the full corpus before pagination and keeps global statistics", async () => {
    const articles = [
      {
        ...article,
        id: "a",
        title: "ROBOT breakthrough",
        category: "具身智能",
      },
      {
        ...article,
        id: "b",
        title: "Other",
        summary: "A robot learns",
        category: "具身智能",
      },
      { ...article, id: "c", title: "Robot model", category: "模型进展" },
      {
        ...article,
        id: "d",
        title: "Exact 100%_! claim",
        category: "产业动态",
      },
      {
        ...article,
        id: "e",
        title: "Exact 100abc claim",
        category: "产业动态",
      },
    ].map((a) => ({ ...a, url: `https://example.com/${a.id}` }));
    const s = await setup({ parse: () => articles });
    await s.refresh();
    const filters = new URLSearchParams({
      category: "具身智能",
      q: "  robot  ",
      limit: "1",
    });
    const first: any = await s.news(`?${filters}`);
    expect(first.articles.map((a: Article) => a.id)).toEqual(["a"]);
    expect(first.pagination).toEqual({
      total: 2,
      offset: 0,
      limit: 1,
      has_more: true,
    });
    expect(first.stats.total_articles).toBe(5);
    filters.set("offset", "1");
    const next: any = await s.news(`?${filters}`);
    expect(next.articles.map((a: Article) => a.id)).toEqual(["b"]);
    expect(next.pagination.has_more).toBe(false);
    expect(((await s.news("?q=robot")) as any).pagination.total).toBe(3);
    expect(
      ((await s.news("?category=" + encodeURIComponent("具身智能"))) as any)
        .pagination.total,
    ).toBe(2);
    expect(
      ((await s.news("?q=" + encodeURIComponent("%_!"))) as any).articles.map(
        (a: Article) => a.id,
      ),
    ).toEqual(["d"]);
    expect(
      ((await s.news("?q=" + encodeURIComponent("' OR 1=1 --"))) as any)
        .pagination.total,
    ).toBe(0);
    expect(
      ((await s.news("?q=%20%20&category=")) as any).pagination.total,
    ).toBe(5);
    expect(
      ((await s.news("?q=" + encodeURIComponent("🤖".repeat(200)))) as any)
        .pagination.total,
    ).toBe(0);
  });
  it("rejects invalid category and oversized search without changing stored data", async () => {
    const s = await setup();
    await s.refresh();
    for (const query of [
      "?category=unknown",
      "?q=%00",
      "?q=" + encodeURIComponent("🤖".repeat(201)),
    ]) {
      const res = await s.api(
        new Request("https://site.test/api/news" + query),
        s.env,
      );
      expect(res.status).toBe(400);
      expect(await res.json()).toEqual({ error: "Invalid news filters" });
    }
    expect(((await s.news()) as any).stats.total_articles).toBe(1);
  });
  it("supports all, none, multiple source selection and exact UTC dates", async () => {
    const sources = [
      source,
      {
        id: "google",
        name: "Google",
        home: "https://blog.google/",
        url: "https://blog.google/feed",
      },
    ];
    const records = [
      {
        ...article,
        id: "a",
        source: "openai",
        published_at: "2026-09-30T23:59:59.000Z",
      },
      {
        ...article,
        id: "b",
        source: "google",
        published_at: "2026-10-01T00:00:00.000Z",
      },
      {
        ...article,
        id: "c",
        source: "google",
        published_at: "2024-02-29T12:00:00.000Z",
      },
    ].map((a) => ({ ...a, url: `https://example.com/${a.id}` }));
    const s = await setup({
      sources,
      parse: (id) => records.filter((a) => a.source === id),
    });
    await s.refresh();
    const all: any = await s.news();
    expect(all.pagination.total).toBe(3);
    const both: any = await s.news(
      "?sources=openai,%20google,openai&date=2026-10-01",
    );
    expect(both.articles.map((a: Article) => a.id)).toEqual(["b"]);
    expect(both.stats.total_articles).toBe(3);
    expect(((await s.news("?sources=openai")) as any).pagination.total).toBe(1);
    expect(((await s.news("?sources=")) as any).pagination.total).toBe(0);
    expect(((await s.news("?sources=,%20,")) as any).pagination.total).toBe(0);
    expect(((await s.news("?date=2024-02-29")) as any).pagination.total).toBe(
      1,
    );
    for (const query of [
      "?sources=unknown",
      "?sources=openai&sources=google",
      "?date=2026-10-01&date=2026-10-02",
      "?category=&category=",
      "?q=one&q=two",
      "?sources=" + "x".repeat(2049),
      "?sources=" + "openai,".repeat(32),
      "?date=2026-02-29",
      "?date=2026-13-01",
      "?date=2026-2-01",
      "?date=",
    ]) {
      const response = await s.api(
        new Request("https://site.test/api/news" + query),
        s.env,
      );
      expect(response.status).toBe(400);
    }
  });
  it("returns filtered zero-filled daily UTC timelines and excludes future data", async () => {
    const records = [
      {
        ...article,
        id: "a",
        title: "Robot launch",
        published_at: "2026-10-01T00:00:00.000Z",
      },
      {
        ...article,
        id: "b",
        title: "Older release",
        published_at: "2026-09-25T00:00:00.000Z",
      },
      { ...article, id: "c", published_at: "2026-09-24T23:59:59.000Z" },
      { ...article, id: "d", published_at: "2026-10-01T05:00:00.001Z" },
    ].map((a) => ({ ...a, url: `https://example.com/${a.id}` }));
    const s = await setup({ parse: () => records });
    await s.refresh();
    const timeline = async (query = "") =>
      s.api(new Request("https://site.test/api/timeline" + query), s.env);
    const result: any = await (await timeline("?days=7")).json();
    expect(result).toMatchObject({ days: 7, timezone: "UTC", total: 2 });
    expect(result.buckets).toHaveLength(7);
    expect(result.buckets[0]).toEqual({ date: "2026-09-25", count: 1 });
    expect(result.buckets[1]).toEqual({ date: "2026-09-26", count: 0 });
    expect(result.buckets[6]).toEqual({ date: "2026-10-01", count: 1 });
    const filtered: any = await (
      await timeline(
        "?days=7&sources=openai&q=robot&category=" +
          encodeURIComponent("模型进展") +
          "&date=invalid-ignored",
      )
    ).json();
    expect(filtered.total).toBe(1);
    expect(
      ((await (await timeline("?days=7&sources=")).json()) as any).total,
    ).toBe(0);
    expect(((await (await timeline()).json()) as any).buckets).toHaveLength(30);
    expect(
      ((await (await timeline("?days=90")).json()) as any).buckets,
    ).toHaveLength(90);
    for (const query of [
      "?days=0",
      "?days=999",
      "?days=07",
      "?days=7&days=30",
      "?sources=openai&sources=google",
      "?sources=unknown",
      "?q=%00",
    ])
      expect((await timeline(query)).status).toBe(400);
    expect(
      (
        await s.api(
          new Request("https://site.test/api/timeline", { method: "POST" }),
          s.env,
        )
      ).status,
    ).toBe(405);
    s.db.close();
    expect((await timeline()).status).toBe(503);
  });
  it("excludes future records from the rolling day count", async () => {
    const future = {
      ...article,
      published_at: new Date(now + 3600000).toISOString(),
    };
    const s = await setup({ parse: () => [future] });
    await s.refresh();
    expect(((await s.news()) as any).stats.recent_articles).toBe(0);
  });
  it("rejects unsafe requests and reports health/missing routes", async () => {
    const s = await setup();
    expect((await s.refresh({})).status).toBe(401);
    expect(
      (
        await s.refresh({
          Authorization: "Bearer test-token",
          Origin: "https://other.test",
        })
      ).status,
    ).toBe(403);
    expect(
      (await s.api(new Request("https://site.test/api/refresh"), s.env)).status,
    ).toBe(405);
    expect(
      (
        await s.api(
          new Request("https://site.test/api/news", { method: "POST" }),
          s.env,
        )
      ).status,
    ).toBe(405);
    expect(
      (await s.api(new Request("https://site.test/api/unknown"), s.env)).status,
    ).toBe(404);
    expect(
      await (
        await s.api(new Request("https://site.test/healthz"), s.env)
      ).json(),
    ).toEqual({ status: "ok", runtime: "rust-wasm" });
  });
  it("throttles repeat success and upserts without counting updates as inserts", async () => {
    const s = await setup();
    expect(await (await s.refresh()).json()).toMatchObject({
      inserted: 1,
      processed: 1,
      status: "success",
    });
    expect(await (await s.refresh()).json()).toMatchObject({
      status: "not_due",
    });
    s.advance();
    expect(
      await (
        await s.refresh({
          Authorization: "Bearer test-token",
          Origin: "https://site.test",
        })
      ).json(),
    ).toMatchObject({ inserted: 0, processed: 1 });
    expect(((await s.news()) as any).articles).toHaveLength(1);
    expect(((await s.news()) as any).run.added).toBe(0);
  });
  it("prevents overlapping refresh without releasing another lease", async () => {
    const s = await setup();
    s.db
      .prepare("INSERT INTO settings VALUES('ingest_lease',?)")
      .run(`${now + 9999}:other`);
    expect(await (await s.refresh()).json()).toMatchObject({ status: "busy" });
    expect(s.db.prepare("SELECT value FROM settings").get()?.value).toBe(
      `${now + 9999}:other`,
    );
  });
  it("closes abandoned running rows even when a recent success throttles refresh", async () => {
    const s = await setup();
    await s.refresh();
    s.db
      .prepare(
        "INSERT INTO runs(id,started_at,status) VALUES('abandoned',?,'running')",
      )
      .run(new Date(now + 1).toISOString());
    expect(await (await s.refresh()).json()).toMatchObject({
      status: "not_due",
    });
    expect(
      s.db
        .prepare("SELECT status,finished_at FROM runs WHERE id='abandoned'")
        .get(),
    ).toEqual({ status: "failed", finished_at: new Date(now).toISOString() });
  });
  it("retains old records on all-source failure and reports partial successes", async () => {
    let fail = false;
    const s = await setup({
      fetchFeed: vi.fn(
        async () => new Response("feed", { status: fail ? 503 : 200 }),
      ),
    });
    await s.refresh();
    fail = true;
    s.advance();
    expect(await (await s.refresh()).json()).toMatchObject({
      status: "failed",
      inserted: 0,
    });
    expect(((await s.news()) as any).articles).toHaveLength(1);
    const partial = await setup({
      sources: [
        source,
        {
          id: "google",
          name: "Google",
          home: "https://blog.google/",
          url: "https://blog.google/feed",
        },
      ],
      fetchFeed: vi.fn(async (url: string) => {
        if (url.includes("blog.google")) throw Error("Unavailable");
        return new Response("ok");
      }),
    });
    expect(await (await partial.refresh()).json()).toMatchObject({
      status: "partial",
      inserted: 1,
    });
  });
  it.each(["empty", "oversize", "parse", "unknown", "nonerror"])(
    "records %s feed failures",
    async (kind) => {
      const s = await setup({
        sources:
          kind === "unknown"
            ? [{ ...source, id: "missing", url: undefined }]
            : undefined,
        parse:
          kind === "parse"
            ? () => {
                throw Error("Malformed RSS");
              }
            : undefined,
        fetchFeed: vi.fn(async () => {
          if (kind === "nonerror") throw "failure";
          if (kind === "empty") return new Response(null);
          return new Response(kind === "oversize" ? "x".repeat(3000001) : "ok");
        }),
      });
      const result: any = await (await s.refresh()).json();
      expect(result.status).toBe("failed");
      expect(result.details[0].error).toBeTruthy();
      expect(
        s.db
          .prepare("SELECT value FROM settings WHERE key='ingest_lease'")
          .get()?.value,
      ).toBe("0");
    },
  );
  it("uses trusted registry transport metadata and source-specific bounded bodies", async () => {
    const fetchFeed = vi.fn(
      async () =>
        new Response("x".repeat(3000001), {
          headers: { "Content-Type": "text/plain" },
        }),
    );
    const configured: Source = {
      ...source,
      method: "POST",
      request_body: '{"pageNum":1}',
      request_language: "zh",
      max_bytes: 6000000,
      timeout_seconds: 30,
    };
    const s = await setup({ sources: [configured], fetchFeed });
    expect(await (await s.refresh()).json()).toMatchObject({
      status: "success",
      processed: 1,
    });
    expect(fetchFeed).toHaveBeenCalledWith(
      source.url,
      expect.objectContaining({
        method: "POST",
        body: '{"pageNum":1}',
        headers: expect.objectContaining({
          "Content-Type": "application/json",
          "Accept-Language": "zh",
          "User-Agent": "AI-Signal/2.0 (official AI news aggregation)",
        }),
      }),
    );
    const body = (await s.news()) as any;
    expect(body.sources[0]).toEqual({
      id: source.id,
      name: source.name,
      home: source.home,
    });
    const tooLarge = await setup({
      sources: [{ ...source, max_bytes: 6000001 }],
      fetchFeed: vi.fn(async () => new Response("x".repeat(6000001))),
    });
    expect(await (await tooLarge.refresh()).json()).toMatchObject({
      status: "failed",
      details: [{ error: "Feed too large" }],
    });
  });
  it("reports storage unavailable without leaking internals", async () => {
    const s = await setup();
    s.db.close();
    const r = await s.api(new Request("https://site.test/api/news"), s.env);
    expect(r.status).toBe(503);
    expect(await r.json()).toEqual({
      error: "Service temporarily unavailable",
    });
  });
  it("uses defaults with an empty source list", async () => {
    const s = await setup();
    const api = createApi({ sources: [], parse: () => [] });
    const r = await api(
      new Request("https://site.test/api/refresh", {
        method: "POST",
        headers: { Authorization: "Bearer test-token" },
      }),
      s.env,
    );
    expect(r.status).toBe(200);
  });
});
describe("Rust and static runtime adapter", () => {
  it("calls Rust bindings and rejects malformed envelopes", () => {
    const bindings = {
      ingestion_sources_json: () => JSON.stringify([source]),
      normalize_feed_json: () =>
        JSON.stringify({ ok: true, articles: [article] }),
    };
    const core = createRustCore(bindings);
    expect(core.sources).toEqual([source]);
    expect(core.parse("openai", "xml", "time")).toEqual([article]);
    for (const envelope of [
      { ok: false, error: "Bad feed" },
      { ok: true },
      { ok: false },
    ])
      expect(() =>
        createRustCore({
          ...bindings,
          normalize_feed_json: () => JSON.stringify(envelope),
        }).parse("x", "x", "x"),
      ).toThrow();
  });
  it("serves static assets SPA fallback headers and API", async () => {
    const s = await setup();
    const site = createSite(s.core, {
      "/index.html": { body: "<html>AI</html>", type: "text/html" },
      "/assets/app.js": { body: "code", type: "application/javascript" },
    });
    const get = (p: string, method = "GET") =>
      site.fetch(new Request("https://site.test" + p, { method }), s.env);
    expect(await (await get("/")).text()).toBe("<html>AI</html>");
    expect(await (await get("/sources")).text()).toContain("AI");
    expect(await (await get("/", "HEAD")).text()).toBe("");
    expect((await get("/", "POST")).status).toBe(405);
    expect((await get("/missing.js")).status).toBe(404);
    expect(
      (await get("/assets/app.js")).headers.get("Cache-Control"),
    ).toContain("immutable");
    expect((await get("/")).headers.get("Content-Security-Policy")).toContain(
      "object-src 'none'",
    );
    expect((await get("/api/news")).status).toBe(200);
    expect((await get("/healthz")).status).toBe(200);
  });
});
