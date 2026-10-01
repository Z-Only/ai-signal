import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import App from "../src/App.vue";
import NewsSearch from "../src/components/NewsSearch.vue";
import { translate } from "../src/core/i18n";
import { article, news, respond } from "./fixtures";
const t = translate.bind(null, "en");
const requested = () => new URL(vi.mocked(fetch).mock.lastCall![0] as string, "https://example.com").searchParams;

describe("accessible explicit search", () => {
  it("labels its input and submits trimmed literal queries only on submit", async () => {
    const wrapper = mount(NewsSearch, { props: { query: "", t } });
    const input = wrapper.find("input");
    expect(wrapper.find("label").attributes("for")).toBe(input.attributes("id"));
    expect(wrapper.attributes("role")).toBe("search");
    expect(input.attributes("aria-describedby")).toBe("search-help");
    expect(wrapper.findAll("button")[1]!.attributes("disabled")).toBeDefined();
    await input.setValue("  100%_ AI\\robot  ");
    expect(wrapper.emitted("search")).toBeUndefined();
    await wrapper.trigger("submit");
    expect(wrapper.emitted("search")).toEqual([["100%_ AI\\robot"]]);
    expect((input.element as HTMLInputElement).value).toBe("100%_ AI\\robot");
    await wrapper.findAll("button")[1]!.trigger("click");
    expect(wrapper.emitted("search")![1]).toEqual([""]);
    expect((input.element as HTMLInputElement).value).toBe("");
    await wrapper.setProps({ query: "External update" });
    expect((input.element as HTMLInputElement).value).toBe("External update");
    await input.setValue("");
    expect(wrapper.findAll("button")[1]!.attributes("disabled")).toBeUndefined();
  });
  it("counts Unicode code points, allows exactly 200, and blocks overlong searches", async () => {
    const wrapper = mount(NewsSearch, { props: { query: "", t } });
    const input = wrapper.find("input");
    await input.setValue(`  ${"🤖".repeat(200)}  `);
    expect(wrapper.find('[type="submit"]').attributes("disabled")).toBeUndefined();
    await wrapper.trigger("submit");
    expect(wrapper.emitted("search")![0]).toEqual(["🤖".repeat(200)]);
    await input.setValue("🤖".repeat(201));
    expect(input.attributes("aria-invalid")).toBe("true");
    expect(input.attributes("aria-describedby")).toContain("search-error");
    expect(wrapper.find('[role="alert"]').text()).toContain("200 characters");
    expect(wrapper.find('[type="submit"]').attributes("disabled")).toBeDefined();
    await wrapper.trigger("submit");
    expect(wrapper.emitted("search")).toHaveLength(1);
    await wrapper.findAll("button")[1]!.trigger("click");
    expect(input.attributes("aria-invalid")).toBe("false");
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
  });
  it("rejects null characters and allows correcting the query", async () => {
    const wrapper = mount(NewsSearch, { props: { query: "", t } });
    await wrapper.find("input").setValue("robot\0ignored");
    expect(wrapper.find("input").attributes("aria-invalid")).toBe("true");
    expect(wrapper.find('[role="alert"]').text()).toContain("Remove null characters");
    await wrapper.trigger("submit");
    expect(wrapper.emitted("search")).toBeUndefined();
    await wrapper.find("input").setValue("robot");
    await wrapper.trigger("submit");
    expect(wrapper.emitted("search")).toEqual([["robot"]]);
  });
  it("searches beyond loaded pages, combines categories, paginates and clears without changing preferences", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(news({
      pagination: { total: 139, offset: 0, limit: 50, has_more: true },
      stats: { total_articles: 139, recent_articles: 4, total_sources: 5 },
    }))));
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.findAll("select")[0]!.setValue("en");
    await wrapper.findAll("select")[1]!.setValue("dark");
    const form = wrapper.find("form");
    await form.find("input").setValue("  robotics%_\\  ");
    expect(fetch).toHaveBeenCalledTimes(1);
    const matches = news({
      articles: [article("from-archive", { title: "A literal robotics%_\\ match", category: "具身智能" })],
      pagination: { total: 2, offset: 0, limit: 1, has_more: true },
      stats: { total_articles: 139, recent_articles: 4, total_sources: 5 },
    });
    vi.mocked(fetch).mockResolvedValueOnce(respond(matches));
    await form.trigger("submit");
    await flushPromises();
    expect(requested().get("q")).toBe("robotics%_\\");
    expect(requested().has("category")).toBe(false);
    expect(requested().has("offset")).toBe(false);
    expect(wrapper.find(".result-summary").text()).toContain("2 results · 1 shown");
    expect(wrapper.find(".result-summary").text()).toContain("Results for “robotics%_\\”");
    expect(wrapper.find(".lead-card").text()).toContain("A literal robotics%_\\ match");
    expect(wrapper.find(".stats").text()).toContain("139");
    vi.mocked(fetch).mockResolvedValueOnce(respond(matches));
    await wrapper.findAll(".tabs button")[6]!.trigger("click");
    await flushPromises();
    expect(requested().get("category")).toBe("具身智能");
    expect(requested().get("q")).toBe("robotics%_\\");
    vi.mocked(fetch).mockResolvedValueOnce(respond(news({
      ...matches,
      articles: [article("next-archive", { category: "具身智能" })],
      pagination: { total: 2, offset: 1, limit: 1, has_more: false },
    })));
    await wrapper.find(".pagination button").trigger("click");
    await flushPromises();
    expect(Object.fromEntries(requested())).toEqual({ category: "具身智能", q: "robotics%_\\", offset: "1", limit: "1" });
    expect(wrapper.find(".result-summary").text()).toContain("2 results · 2 shown");
    expect(wrapper.findAll(".news-card")).toHaveLength(1);
    vi.mocked(fetch).mockResolvedValueOnce(respond(matches));
    await wrapper.find(".page-head .reload").trigger("click");
    await flushPromises();
    expect(requested().get("q")).toBe("robotics%_\\");
    expect(requested().has("offset")).toBe(false);
    vi.mocked(fetch).mockResolvedValueOnce(respond(news({ articles: [], pagination: { total: 0, offset: 0, limit: 50, has_more: false } })));
    await form.findAll("button")[1]!.trigger("click");
    await flushPromises();
    expect(Object.fromEntries(requested())).toEqual({ category: "具身智能" });
    expect(wrapper.find(".empty").text()).toContain("No matching articles");
    expect(wrapper.find(".result-summary").text()).toBe("0 results · 0 shown");
    expect(localStorage.getItem("ai-signal-language")).toBe("en");
    expect(localStorage.getItem("ai-signal-theme")).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
  it("does not turn a failed filter request into an empty-library claim and retries the current query", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(news())));
    const wrapper = mount(App);
    await flushPromises();
    vi.mocked(fetch).mockRejectedValueOnce(new Error("Offline"));
    await wrapper.find("input").setValue("missing");
    await wrapper.find("form").trigger("submit");
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(true);
    expect(wrapper.find(".lead-card").exists()).toBe(false);
    expect(wrapper.find(".empty").exists()).toBe(false);
    vi.mocked(fetch).mockResolvedValueOnce(respond(news({ articles: [] })));
    await wrapper.find('[role="alert"] button').trigger("click");
    await flushPromises();
    expect(requested().get("q")).toBe("missing");
    expect(wrapper.find(".empty").text()).toContain("没有找到匹配的资讯");
    await wrapper.find("input").setValue("  ");
    await wrapper.find("form").trigger("submit");
    await flushPromises();
    expect(requested().has("q")).toBe(false);
  });
});

describe("pending search presentation", () => {
  it("removes old-category cards immediately and only announces the current results", async () => {
    let finish!: (value: Response) => void;
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(news())).mockImplementationOnce(() => new Promise<Response>((resolve) => { finish = resolve; })));
    const wrapper = mount(App);
    await flushPromises();
    expect(wrapper.find(".lead-card").exists()).toBe(true);
    await wrapper.findAll(".tabs button")[6]!.trigger("click");
    expect(wrapper.find(".lead-card").exists()).toBe(false);
    expect(wrapper.findAll(".news-card")).toHaveLength(0);
    expect(wrapper.find(".result-summary").text()).toContain("正在加载匹配的资讯");
    expect(wrapper.find(".result-summary").text()).not.toContain("共 3 条结果");
    expect(wrapper.find(".feed-results").attributes("aria-busy")).toBe("true");
    expect(wrapper.findAll(".tabs button")[6]!.attributes("aria-pressed")).toBe("true");
    finish(respond(news({ articles: [article("robot", { category: "具身智能" })], pagination: { total: 7, offset: 0, limit: 1, has_more: true } })));
    await flushPromises();
    expect(wrapper.find(".lead-card").text()).toContain("Official article robot");
    expect(wrapper.find(".result-summary").text()).toBe("共 7 条结果 · 已显示 1 条");
  });
});
