import { describe, expect, it } from "vitest";
import { categoryLabel, scheduleLabel, translate } from "../src/core/i18n";
import {
  emptyNews,
  formatDate,
  mergeArticles,
  parseNews,
  recentCount,
  safeUrl,
  sourceInitial,
} from "../src/core/news";
import { article, news } from "./fixtures";
describe("news model and utilities", () => {
  it("accepts the legacy API and pagination without inventing missing data", () => {
    expect(parseNews(news())).toEqual(news());
    expect(parseNews({ ...news(), run: undefined }).run).toBeNull();
    expect(
      parseNews(
        news({
          pagination: { total: 9, offset: 0, limit: 3, has_more: true },
          stats: { total_articles: 9, recent_articles: 4, total_sources: 3 },
        }),
      ).stats?.total_articles,
    ).toBe(9);
    expect(emptyNews()).toEqual({
      articles: [],
      sources: [],
      run: null,
      schedule: "",
    });
  });
  it.each([
    null,
    0,
    {},
    { articles: [], sources: [], schedule: 3 },
    news({ articles: [null] as never }),
    news({ articles: [{ id: "x" }] as never }),
    news({ sources: [null] as never }),
    news({ sources: [{ id: 1 }] as never }),
    news({ sources: [{ id: "x", name: 1 }] as never }),
    news({ sources: [{ id: "x", name: "X", home: 1 }] as never }),
    news({ run: { status: "unknown", details: [] } as never }),
    news({ run: { status: "success", details: null } as never }),
    news({ pagination: { total: -1, offset: 0, limit: 3, has_more: true } }),
    news({ pagination: { total: 1, offset: 0, limit: 0, has_more: false } }),
    news({
      pagination: { total: 1, offset: 0, limit: 1, has_more: "false" } as never,
    }),
    news({
      stats: { total_articles: 1, recent_articles: NaN, total_sources: 1 },
    }),
  ])("rejects malformed API data %#", (value) => {
    expect(() => parseNews(value)).toThrow();
  });
  it("only links HTTPS originals", () => {
    expect(safeUrl("https://example.com")).toBe("https://example.com/");
    for (const url of [
      "javascript:alert(1)",
      "http://example.com",
      "not a url",
      "/relative",
    ])
      expect(safeUrl(url)).toBeUndefined();
  });
  it("counts only articles in the rolling past 24 hours", () => {
    const now = Date.parse("2026-10-01T06:00:00Z");
    expect(
      recentCount(
        [
          article(),
          article("2", { published_at: "2026-09-30T06:00:00Z" }),
          article("3", { published_at: "2026-10-01T07:00:00Z" }),
          article("4", { published_at: "invalid" }),
        ],
        now,
      ),
    ).toBe(1);
    expect(recentCount([])).toBe(0);
  });
  it("merges overlapping pages while updating duplicates", () => {
    expect(
      mergeArticles(
        [article()],
        [article("1", { title: "Updated" }), article("2")],
      ),
    ).toEqual([article("1", { title: "Updated" }), article("2")]);
    expect(sourceInitial("huggingface")).toBe("H");
    expect(sourceInitial("")).toBe("");
  });
  it("formats source timestamps and translates interface labels only", () => {
    expect(formatDate("2026-10-01T05:00:00Z", "en")).toBe(
      new Date("2026-10-01T05:00:00Z").toLocaleString("en", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      }),
    );
    expect(formatDate("invalid", "en")).toBe("Time unavailable");
    expect(translate("en", "syncedCount", { count: 4 })).toBe(
      "4 articles synced",
    );
    expect(translate("zh-CN", "syncedCount", { count: 4 })).toBe(
      "已同步 4 条资讯",
    );
    expect(translate("en", "syncedCount")).toBe("{count} articles synced");
    expect(categoryLabel("en", "模型进展")).toBe("Models");
    expect(categoryLabel("zh-CN", "模型进展")).toBe("模型进展");
    expect(categoryLabel("en", "New category")).toBe("New category");
    expect(scheduleLabel("en", "每小时自动更新")).toBe(
      "Updated automatically every hour",
    );
    expect(scheduleLabel("zh-CN", "每小时自动更新")).toBe("每小时自动更新");
    expect(scheduleLabel("en", "尚未启用")).toBe(
      "Automatic updates not enabled",
    );
    expect(scheduleLabel("en", "")).toBe("Connecting");
    expect(scheduleLabel("en", "Custom schedule")).toBe("Custom schedule");
  });
});

describe("English singular counts", () => {
  it.each([
    ["articlesUnit", "article", "articles"],
    ["updatesUnit", "new update", "new updates"],
    ["channels", "official channel", "official channels"],
    ["resultCount", "1 result · 1 shown", "2 results · 2 shown"],
    ["updateCount", "1 update", "2 updates"],
    ["syncedCount", "1 article synced", "2 articles synced"],
    ["loaded", "1 loaded · 1 matching article", "2 loaded · 2 matching articles"],
  ] as const)("inflects %s", (key, singular, plural) => {
    expect(translate("en", key, { count: 1, total: 1, loaded: 1 })).toBe(singular);
    expect(translate("en", key, { count: 2, total: 2, loaded: 2 })).toBe(plural);
  });
  it("keeps zero plural, Chinese unchanged, and unrelated messages intact", () => {
    expect(translate("en", "resultCount", { count: 0, loaded: 0 })).toBe("0 results · 0 shown");
    expect(translate("en", "loaded", { count: 1, total: 8 })).toBe("1 loaded · 8 matching articles");
    expect(translate("zh-CN", "resultCount", { count: 1, loaded: 1 })).toBe("共 1 条结果 · 已显示 1 条");
    expect(translate("en", "brand", { count: 1 })).toBe("AI Signal");
  });
});
