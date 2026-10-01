import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import App from "../src/App.vue";
import ArticleFeed from "../src/components/ArticleFeed.vue";
import NewsStats from "../src/components/NewsStats.vue";
import SourceDirectory from "../src/components/SourceDirectory.vue";
import { translate } from "../src/core/i18n";
import { article, news, respond } from "./fixtures";
const t = translate.bind(null, "en");
const start = async (body = news()) => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(body)));
  const wrapper = mount(App);
  await flushPromises();
  return wrapper;
};
describe("reader application", () => {
  it("keeps the baseline feed, real source names, timestamps and source health", async () => {
    const wrapper = await start();
    expect(fetch).toHaveBeenCalledWith(
      "/api/news",
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
    expect(wrapper.find("h1").text()).toBe("AI 的下一步，从这里看见");
    expect(wrapper.find(".lead-card").text()).toContain("Official article 1");
    expect(wrapper.findAll(".news-card")).toHaveLength(2);
    expect(wrapper.find(".no-summary").text()).toContain("官方未提供摘要");
    expect(wrapper.find(".news-card:last-child").text()).toContain("unknown");
    expect(wrapper.find("time").attributes("datetime")).toBe(
      "2026-10-01T05:00:00Z",
    );
    expect(wrapper.find(".lead-card").attributes("rel")).toBe(
      "noopener noreferrer",
    );
    expect(wrapper.find(".health.bad").exists()).toBe(true);
    expect(wrapper.find(".health.pending").exists()).toBe(true);
    expect(wrapper.find(".notice").text()).toContain("部分信源");
    expect(wrapper.find("nav button").attributes("aria-current")).toBe("page");
    expect(wrapper.find('[role="tablist"]').exists()).toBe(false);
    expect(wrapper.findAll(".tabs button")).toHaveLength(7);
    expect(wrapper.find(".skip-link").attributes("href")).toBe("#main-content");
  });
  it("requests whole-library categories and explains empty server results", async () => {
    const wrapper = await start();
    vi.mocked(fetch).mockResolvedValueOnce(respond(news({
      articles: [article("beyond-page", { category: "开发工具" })],
      pagination: { total: 1, offset: 0, limit: 50, has_more: false },
      stats: { total_articles: 139, recent_articles: 4, total_sources: 5 },
    })));
    await wrapper.findAll(".tabs button")[3]!.trigger("click");
    await flushPromises();
    expect(new URL(vi.mocked(fetch).mock.lastCall![0] as string, "https://example.com").searchParams.get("category")).toBe("开发工具");
    expect(wrapper.find(".lead-card").text()).toContain("Official article beyond-page");
    expect(wrapper.findAll(".tabs button")[3]!.attributes("aria-pressed")).toBe("true");
    expect(wrapper.find(".result-summary").text()).toContain("共 1 条结果");
    expect(wrapper.find(".tabs button span").text()).toBe("139");
    vi.mocked(fetch).mockResolvedValueOnce(respond(news({ articles: [] })));
    await wrapper.findAll(".tabs button")[6]!.trigger("click");
    await flushPromises();
    expect(wrapper.find(".empty").text()).toContain("没有找到匹配的资讯");
    expect(wrapper.find(".empty").text()).toContain("试试其他关键词或分类");
    await wrapper.findAll(".tabs button")[0]!.trigger("click");
    await flushPromises();
    expect(fetch).toHaveBeenLastCalledWith("/api/news", expect.any(Object));
    expect(wrapper.findAll(".news-card")).toHaveLength(2);
  });
  it("opens the source directory from both navigation and the radar", async () => {
    const wrapper = await start();
    await wrapper.find(".sources-card button").trigger("click");
    expect(wrapper.find("h1").text()).toBe("信息有出处，判断有依据");
    const sources = wrapper.findAll(".sources-page article");
    expect(sources).toHaveLength(3);
    expect(sources[0]!.text()).toContain("已同步 12 条资讯");
    expect(sources[1]!.text()).toContain("本轮采集失败");
    expect(sources[2]!.text()).toContain("等待首次采集");
    expect(sources[0]!.find("a").attributes("href")).toBe(
      "https://openai.com/news/",
    );
    await wrapper.findAll("nav button")[0]!.trigger("click");
    expect(wrapper.find(".lead-card").exists()).toBe(true);
    await wrapper.findAll("nav button")[1]!.trigger("click");
    expect(wrapper.find(".sources-page").exists()).toBe(true);
    expect(wrapper.findAll("nav button")[1]!.attributes("aria-current")).toBe(
      "page",
    );
  });
  it("switches zh/en UI and leaves source content intact", async () => {
    const wrapper = await start();
    await wrapper.findAll("select")[0]!.setValue("en");
    expect(wrapper.find("h1").text()).toBe("See what comes next in AI");
    expect(wrapper.find(".tabs").text()).toContain("Developer tools");
    expect(wrapper.find(".lead-card").text()).toContain("Official article 1");
    expect(document.documentElement.lang).toBe("en");
    expect(document.title).toBe("AI Signal · See what comes next");
    expect(localStorage.getItem("ai-signal-language")).toBe("en");
    await wrapper.findAll("select")[0]!.setValue("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");
    await wrapper.findAll("select")[1]!.setValue("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
  it("renders truthful totals supplied by the backend", async () => {
    const wrapper = await start(
      news({
        pagination: { total: 1234, offset: 0, limit: 3, has_more: true },
        stats: { total_articles: 1234, recent_articles: 23, total_sources: 5 },
      }),
    );
    const stats = wrapper.findAll(".stats strong");
    expect(stats[0]!.text()).toContain("1234");
    expect(stats[1]!.text()).toContain("23");
    expect(stats[2]!.text()).toContain("05");
    expect(wrapper.find(".tabs button span").text()).toBe("1234");
    expect(wrapper.find(".pagination").text()).toContain("已加载 3 条资讯");
    vi.mocked(fetch).mockResolvedValueOnce(
      respond(
        news({
          articles: [article("3"), article("4")],
          pagination: { total: 4, offset: 3, limit: 3, has_more: false },
        }),
      ),
    );
    await wrapper.find(".pagination button").trigger("click");
    await flushPromises();
    expect(fetch).toHaveBeenLastCalledWith(
      "/api/news?offset=3&limit=3",
      expect.any(Object),
    );
    expect(wrapper.findAll(".news-card")).toHaveLength(3);
    expect(wrapper.find(".pagination").exists()).toBe(false);
  });
  it("loads and refreshes, preserving the last successful articles on error", async () => {
    let resolve!: (value: Response) => void;
    vi.stubGlobal(
      "fetch",
      vi.fn(
        () =>
          new Promise<Response>((done) => {
            resolve = done;
          }),
      ),
    );
    const wrapper = mount(App);
    await wrapper.vm.$nextTick();
    expect(wrapper.find(".empty").text()).toContain("正在连接");
    expect(wrapper.find(".reload").attributes("disabled")).toBeDefined();
    expect(wrapper.find(".feed-results").attributes("aria-busy")).toBe("true");
    resolve(respond(news()));
    await flushPromises();
    vi.mocked(fetch).mockResolvedValueOnce(respond(null, false));
    await wrapper.find(".reload").trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').text()).toContain("暂时无法加载");
    expect(wrapper.find(".lead-card").text()).toContain("Official article 1");
    vi.mocked(fetch).mockResolvedValueOnce(
      respond(news({ articles: [article("new")], run: null })),
    );
    await wrapper.find('[role="alert"] button').trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(wrapper.find(".lead-card").text()).toContain("Official article new");
  });
  it("shows API/network and malformed response errors without inventing articles", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("Offline")));
    const wrapper = mount(App);
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(true);
    expect(wrapper.find(".empty").exists()).toBe(false);
    expect(wrapper.find(".result-summary").text()).not.toContain("共 0 条结果");
    vi.mocked(fetch).mockResolvedValueOnce(respond({ articles: "bad" }));
    await wrapper.find('[role="alert"] button').trigger("click");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(true);
    expect(wrapper.find(".lead-card").exists()).toBe(false);
  });
  it("handles an empty library and failed ingestion separately from fetch errors", async () => {
    const wrapper = await start(
      news({ articles: [], run: { status: "failed", details: [] } }),
    );
    expect(wrapper.find(".empty").text()).toContain("首次采集尚未完成");
    expect(wrapper.find(".notice").text()).toContain("本轮采集失败");
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
  });
  it("renders missing summaries and blocks unsafe original links", () => {
    const wrapper = mount(ArticleFeed, {
      props: {
        articles: [article("1", { summary: "", url: "javascript:alert(1)" })],
        sources: [],
        locale: "en",
        t,
      },
    });
    expect(wrapper.find(".lead-card").text()).toContain(
      "This source did not provide a summary",
    );
    expect(wrapper.find(".lead-card").attributes("href")).toBeUndefined();
    expect(wrapper.findAll(".news-card")).toHaveLength(0);
    const empty = mount(ArticleFeed, {
      props: { articles: [], sources: [], locale: "en", t },
    });
    expect(empty.find(".lead-card").exists()).toBe(false);
  });
  it("uses pagination totals without statistics and supports zero-item source status", () => {
    const wrapper = mount(NewsStats, {
      props: {
        data: news({
          pagination: { total: 88, limit: 3, offset: 0, has_more: true },
          run: null,
        }),
        locale: "en",
        t,
      },
    });
    expect(wrapper.find("strong").text()).toContain("88");
    expect(wrapper.text()).toContain("Awaiting first collection");
    const directory = mount(SourceDirectory, {
      props: {
        data: news({
          run: {
            status: "success",
            details: [
              { source: "openai", status: "ok", checked_at: "invalid" },
            ],
          },
        }),
        locale: "en",
        t,
      },
    });
    expect(directory.text()).toContain("0 articles synced");
    expect(directory.text()).toContain("Time unavailable");
  });
});
