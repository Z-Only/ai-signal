import { defineComponent, h } from "vue";
import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import App from "../src/App.vue";
import { useReadingState } from "../src/composables/useReadingState";
import { article, news, respond } from "./fixtures";

const harness = defineComponent({
  setup: () => ({ ...useReadingState() }),
  render: () => h("div"),
});
const queryParams = () => new URLSearchParams(window.location.search);
const apiParams = () => new URL(vi.mocked(fetch).mock.lastCall![0] as string, window.location.origin).searchParams;
const saved = (limit = 100) => ({ version: 1, category: "具身智能", query: "robots", limit });
const deepLink = "?category=" + encodeURIComponent("具身智能") + "&q=robots";
async function traverse(direction: "back" | "forward") {
  const change = new Promise<void>((resolve) => window.addEventListener("popstate", () => resolve(), { once: true }));
  window.history[direction]();
  await change;
  await flushPromises();
}

describe("reader URL state", () => {
  it("restores a deep link, normalizes known parameters and preserves unrelated parameters, fragment and state", () => {
    window.history.replaceState({ otherApplication: { id: 8 } }, "", "/reader?tracking=one&tracking=two&view=sources&category=%E5%85%B7%E8%BA%AB%E6%99%BA%E8%83%BD&q=%20robots%20&q=ignored#main-content");
    const wrapper = mount(harness);
    expect(wrapper.vm.route).toEqual({ sources: null, date: "", days: 30, view: "sources", category: "具身智能", query: "robots", limit: 50 });
    expect(queryParams().getAll("q")).toEqual(["robots"]);
    expect(queryParams().getAll("tracking")).toEqual(["one", "two"]);
    expect(window.location.pathname).toBe("/reader");
    expect(window.location.hash).toBe("#main-content");
    expect(window.history.state.otherApplication).toEqual({ id: 8 });
  });
  it.each([
    ["?view=bad&category=unknown&q=%00", ""],
    ["?view=feed&category=" + encodeURIComponent("全部资讯") + "&q=%20", ""],
    ["?q=" + encodeURIComponent("🤖".repeat(201)), ""],
    ["?q=" + encodeURIComponent("  " + "🤖".repeat(200) + "  "), "🤖".repeat(200)],
  ])("sanitizes invalid/default URL state: %s", (url, query) => {
    window.history.replaceState(null, "", url);
    const wrapper = mount(harness);
    expect(wrapper.vm.route).toEqual({ sources: null, date: "", days: 30, view: "feed", category: "全部资讯", query, limit: 50 });
    expect(queryParams().has("view")).toBe(false);
    expect(queryParams().has("category")).toBe(false);
    expect(queryParams().get("q")).toBe(query || null);
  });
  it.each([
    saved(100),
    { ...saved(), version: 2 },
    { ...saved(), category: "开发工具" },
    { ...saved(), query: "other" },
    saved(49), saved(201), saved(75.5), { ...saved(), limit: "100" },
  ])("only restores bounded, matching history metadata %#", (metadata) => {
    window.history.replaceState({ aiSignalReader: metadata }, "", deepLink);
    const wrapper = mount(harness);
    expect(wrapper.vm.route.limit).toBe(JSON.stringify(metadata) === JSON.stringify(saved(100)) ? 100 : 50);
  });
  it("pushes only meaningful navigation, replaces loaded range and restores Back/Forward", async () => {
    const wrapper = mount(harness);
    const push = vi.spyOn(window.history, "pushState");
    wrapper.vm.navigate({ query: " robots ", category: "具身智能" });
    expect(queryParams().get("q")).toBe("robots");
    expect(push).toHaveBeenCalledTimes(1);
    wrapper.vm.rememberRange(139);
    expect(window.history.state.aiSignalReader.limit).toBe(139);
    wrapper.vm.rememberRange(139);
    wrapper.vm.navigate({ query: "robots", category: "具身智能", view: "feed" });
    expect(push).toHaveBeenCalledTimes(1);
    wrapper.vm.navigate({ view: "sources" });
    expect(window.history.state.aiSignalReader.limit).toBe(139);
    expect(queryParams().get("view")).toBe("sources");
    wrapper.vm.navigate({ query: "new" });
    expect(wrapper.vm.route.limit).toBe(50);
    await traverse("back");
    expect(wrapper.vm.route).toEqual({ sources: null, date: "", days: 30, view: "sources", category: "具身智能", query: "robots", limit: 139 });
    await traverse("forward");
    expect(wrapper.vm.route.query).toBe("new");
    expect(push).toHaveBeenCalledTimes(3);
    wrapper.vm.rememberRange(999_999);
    expect(window.history.state.aiSignalReader.limit).toBe(200);
  });
  it("removes the history listener on unmount and tolerates unavailable History APIs", () => {
    vi.spyOn(window.history, "replaceState").mockImplementation(() => { throw new DOMException("Denied", "SecurityError"); });
    const wrapper = mount(harness);
    vi.spyOn(window.history, "pushState").mockImplementation(() => { throw new DOMException("Denied", "SecurityError"); });
    wrapper.vm.navigate({ query: "still works" });
    expect(wrapper.vm.route.query).toBe("still works");
    const remove = vi.spyOn(window, "removeEventListener");
    wrapper.unmount();
    expect(remove).toHaveBeenCalledWith("popstate", expect.any(Function));
    window.dispatchEvent(new PopStateEvent("popstate"));
    expect(wrapper.vm.route.query).toBe("still works");
  });
});

describe("restored reader integration", () => {
  it("loads exactly the deep-linked filters and view without an initial unfiltered request", async () => {
    window.history.replaceState({ aiSignalReader: saved(139) }, "", deepLink + "&view=sources");
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(news({ articles: [article("match")], pagination: { total: 1, limit: 139, offset: 0, has_more: false } }))));
    const wrapper = mount(App);
    await flushPromises();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(Object.fromEntries(apiParams())).toEqual({ category: "具身智能", q: "robots", limit: "139" });
    expect(wrapper.find(".sources-page").exists()).toBe(true);
    const push = vi.spyOn(window.history, "pushState");
    await wrapper.findAll("nav button")[0]!.trigger("click");
    expect(wrapper.find("input").element.value).toBe("robots");
    expect(wrapper.findAll(".tabs button")[6]!.attributes("aria-pressed")).toBe("true");
    expect(fetch).toHaveBeenCalledTimes(1);
    await wrapper.find("form").trigger("submit");
    await wrapper.findAll(".tabs button")[6]!.trigger("click");
    expect(push).toHaveBeenCalledTimes(1);
    expect(fetch).toHaveBeenCalledTimes(1);
    await wrapper.find(".page-head .reload").trigger("click");
    await flushPromises();
    expect(push).toHaveBeenCalledTimes(1);
  });
  it("reconstructs a bounded loaded range after reload and filter Back navigation", async () => {
    const articles = Array.from({ length: 50 }, (_, index) => article(String(index)));
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(news({ articles, pagination: { total: 139, offset: 0, limit: 50, has_more: true } }))));
    let wrapper = mount(App);
    await flushPromises();
    const push = vi.spyOn(window.history, "pushState");
    vi.mocked(fetch).mockResolvedValueOnce(respond(news({ articles: Array.from({ length: 50 }, (_, index) => article(String(index + 50))), pagination: { total: 139, offset: 50, limit: 50, has_more: true } })));
    await wrapper.find(".pagination button").trigger("click");
    await flushPromises();
    expect(window.history.state.aiSignalReader.limit).toBe(100);
    expect(push).not.toHaveBeenCalled();
    const firstHundred = news({ articles: Array.from({ length: 100 }, (_, index) => article(String(index))), pagination: { total: 139, offset: 0, limit: 100, has_more: true } });
    vi.mocked(fetch).mockResolvedValueOnce(respond(firstHundred));
    await wrapper.find(".page-head .reload").trigger("click");
    await flushPromises();
    expect(apiParams().get("limit")).toBe("100");
    wrapper.unmount();
    vi.mocked(fetch).mockResolvedValueOnce(respond(firstHundred));
    wrapper = mount(App);
    await flushPromises();
    expect(apiParams().get("limit")).toBe("100");
    expect(wrapper.findAll("[data-article-id]")).toHaveLength(100);
    await wrapper.findAll(".tabs button")[6]!.trigger("click");
    await flushPromises();
    expect(apiParams().get("limit")).toBe("50");
    vi.mocked(fetch).mockResolvedValueOnce(respond(firstHundred));
    await traverse("back");
    expect(apiParams().get("limit")).toBe("100");
    expect(apiParams().has("category")).toBe(false);
    expect(wrapper.findAll("[data-article-id]")).toHaveLength(100);
  });
  it("aborts pending search on Back and ignores its later response", async () => {
    let finish!: (response: Response) => void;
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(news())));
    const wrapper = mount(App);
    await flushPromises();
    vi.mocked(fetch).mockImplementationOnce(() => new Promise<Response>((resolve) => { finish = resolve; }));
    await wrapper.find("input").setValue("newer query");
    await wrapper.find("form").trigger("submit");
    const obsolete = vi.mocked(fetch).mock.lastCall![1]!.signal!;
    await traverse("back");
    expect(obsolete.aborted).toBe(true);
    expect(wrapper.find("input").element.value).toBe("");
    expect(wrapper.find(".lead-card").text()).toContain("Official article 1");
    finish(respond(news({ articles: [article("wrong")] })));
    await flushPromises();
    expect(wrapper.find(".lead-card").text()).not.toContain("Official article wrong");
    await traverse("forward");
    expect(apiParams().get("q")).toBe("newer query");
    expect(wrapper.find("input").element.value).toBe("newer query");
  });
  it("normalizes invalid popstate URLs and keeps errors/retries out of history", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(news())));
    const wrapper = mount(App);
    await flushPromises();
    await wrapper.find("input").setValue("valid");
    vi.mocked(fetch).mockRejectedValueOnce(new Error("Offline"));
    await wrapper.find("form").trigger("submit");
    await flushPromises();
    const push = vi.spyOn(window.history, "pushState");
    await wrapper.find('[role="alert"] button').trigger("click");
    await flushPromises();
    expect(push).not.toHaveBeenCalled();
    window.history.replaceState(null, "", "?view=invalid&category=unknown&q=%00&keep=yes");
    window.dispatchEvent(new PopStateEvent("popstate"));
    await flushPromises();
    expect(window.location.search).toBe("?keep=yes");
    expect(wrapper.find("input").element.value).toBe("");
    expect(Object.fromEntries(apiParams())).toEqual({ limit: "50" });
  });
});

describe("history-owned result ranges", () => {
  const range = (count: number) => news({
    articles: Array.from({ length: count }, (_, index) => article(String(index))),
    pagination: { total: 300, offset: 0, limit: count, has_more: true },
  });
  function seedHistory() {
    window.history.replaceState({ aiSignalReader: saved(100) }, "", deepLink);
    window.history.pushState({ aiSignalReader: saved(150) }, "", deepLink + "&view=sources");
  }

  it("restores 100 instead of retaining 150 for same-filter Back, then restores 150 on Forward", async () => {
    seedHistory();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(range(150))));
    const wrapper = mount(App);
    await flushPromises();
    expect(wrapper.find(".sources-page").exists()).toBe(true);
    expect(window.history.state.aiSignalReader.limit).toBe(150);
    const push = vi.spyOn(window.history, "pushState");
    vi.mocked(fetch).mockResolvedValueOnce(respond(range(100)));
    await traverse("back");
    expect(Object.fromEntries(apiParams())).toEqual({ category: "具身智能", q: "robots", limit: "100" });
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(wrapper.findAll("[data-article-id]")).toHaveLength(100);
    expect(window.history.state.aiSignalReader.limit).toBe(100);
    vi.mocked(fetch).mockResolvedValueOnce(respond(range(150)));
    await traverse("forward");
    expect(apiParams().get("limit")).toBe("150");
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(wrapper.find(".sources-page").exists()).toBe(true);
    expect(window.history.state.aiSignalReader.limit).toBe(150);
    expect(push).not.toHaveBeenCalled();
  });

  it.each(["success", "error"])("interrupts same-filter pending append and ignores its late %s and history writes", async (outcome) => {
    window.history.replaceState({ aiSignalReader: saved(100) }, "", deepLink + "&view=sources");
    window.history.pushState({ aiSignalReader: saved(150) }, "", deepLink);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(range(150))));
    const wrapper = mount(App);
    await flushPromises();
    let finishAppend!: (response: Response) => void;
    let failAppend!: (error: Error) => void;
    let finishRestore!: (response: Response) => void;
    vi.mocked(fetch)
      .mockImplementationOnce(() => new Promise<Response>((resolve, reject) => { finishAppend = resolve; failAppend = reject; }))
      .mockImplementationOnce(() => new Promise<Response>((resolve) => { finishRestore = resolve; }));
    await wrapper.find(".pagination button").trigger("click");
    const oldSignal = vi.mocked(fetch).mock.lastCall![1]!.signal!;
    expect(apiParams().get("offset")).toBe("150");
    await traverse("back");
    expect(oldSignal.aborted).toBe(true);
    expect(Object.fromEntries(apiParams())).toEqual({ category: "具身智能", q: "robots", limit: "100" });
    expect(fetch).toHaveBeenCalledTimes(3);
    if (outcome === "success") finishAppend(respond({ ...range(300), pagination: { total: 300, offset: 150, limit: 150, has_more: false } }));
    else failAppend(new Error("Obsolete append failed"));
    await flushPromises();
    expect(wrapper.find(".page-head .reload").attributes("disabled")).toBeDefined();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
    expect(window.history.state.aiSignalReader.limit).toBe(100);
    finishRestore(respond(range(100)));
    await flushPromises();
    expect(window.history.state.aiSignalReader.limit).toBe(100);
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(wrapper.find(".page-head .reload").attributes("disabled")).toBeUndefined();
    // Ordinary navigation and rememberRange updates do not become extra fetches.
    await wrapper.findAll("nav button")[0]!.trigger("click");
    expect(wrapper.findAll("[data-article-id]")).toHaveLength(100);
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("sends exactly one forced request when a popstate changes both filters and saved range", async () => {
    window.history.replaceState({ aiSignalReader: { ...saved(100), category: "开发工具", query: "tools" } }, "", "?category=" + encodeURIComponent("开发工具") + "&q=tools");
    window.history.pushState({ aiSignalReader: saved(150) }, "", deepLink);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(range(150))).mockResolvedValueOnce(respond(range(100))));
    const wrapper = mount(App);
    await flushPromises();
    await traverse("back");
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(Object.fromEntries(apiParams())).toEqual({ category: "开发工具", q: "tools", limit: "100" });
    expect(wrapper.findAll("[data-article-id]")).toHaveLength(100);
    expect(window.history.state.aiSignalReader.limit).toBe(100);
  });

  it("distinguishes popstate from range bookkeeping and ordinary navigation", async () => {
    const wrapper = mount(harness);
    expect(wrapper.vm.restoration).toBe(0);
    wrapper.vm.rememberRange(100);
    wrapper.vm.navigate({ view: "sources" });
    wrapper.vm.rememberRange(150);
    expect(wrapper.vm.restoration).toBe(0);
    await traverse("back");
    expect(wrapper.vm.restoration).toBe(1);
    expect(wrapper.vm.route.limit).toBe(100);
    wrapper.vm.rememberRange(100);
    expect(wrapper.vm.restoration).toBe(1);
  });
});
