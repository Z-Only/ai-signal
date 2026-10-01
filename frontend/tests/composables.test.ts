import { defineComponent, h } from "vue";
import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { usePreferences } from "../src/composables/usePreferences";
import { useNews } from "../src/composables/useNews";
import { news, respond } from "./fixtures";
const preferencesHarness = defineComponent({
  setup: () => ({ ...usePreferences() }),
  render: () => h("div"),
});
const newsHarness = defineComponent({
  setup: () => ({ ...useNews() }),
  render: () => h("div"),
});
describe("preferences", () => {
  it("uses system appearance and tracks changes, with explicit overrides", async () => {
    const wrapper = mount(preferencesHarness);
    const media = matchMedia("(prefers-color-scheme: dark)");
    expect(document.documentElement.dataset.theme).toBe("light");
    const change = new Event("change");
    Object.assign(change, { matches: true });
    media.dispatchEvent(change);
    await wrapper.vm.$nextTick();
    expect(document.documentElement.dataset.theme).toBe("dark");
    wrapper.vm.theme = "light";
    await wrapper.vm.$nextTick();
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(localStorage.getItem("ai-signal-theme")).toBe("light");
    wrapper.vm.theme = "dark";
    await wrapper.vm.$nextTick();
    expect(document.documentElement.dataset.theme).toBe("dark");
    const remove = vi.spyOn(media, "removeEventListener");
    wrapper.unmount();
    expect(remove).toHaveBeenCalledWith("change", expect.any(Function));
  });
  it.each(["light", "dark", "system", "invalid"])(
    "restores saved %s appearance and English",
    (theme) => {
      localStorage.setItem("ai-signal-language", "en");
      localStorage.setItem("ai-signal-theme", theme);
      const wrapper = mount(preferencesHarness);
      expect(wrapper.vm.locale).toBe("en");
      expect(wrapper.vm.theme).toBe(theme === "invalid" ? "system" : theme);
      expect(wrapper.vm.t("feed")).toBe("News overview");
    },
  );
  it("works when storage access fails", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("Denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("Full");
    });
    const wrapper = mount(preferencesHarness);
    expect(wrapper.vm.locale).toBe("zh-CN");
    wrapper.vm.theme = "dark";
    await wrapper.vm.$nextTick();
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});
describe("news request lifecycle", () => {
  it("ignores repeat clicks while a request is pending and aborts on unmount", async () => {
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
    const wrapper = mount(newsHarness);
    await wrapper.vm.load();
    expect(fetch).toHaveBeenCalledTimes(1);
    const signal = vi.mocked(fetch).mock.calls[0]![1]!.signal!;
    const load = wrapper.vm.load;
    wrapper.unmount();
    expect(signal.aborted).toBe(true);
    resolve(respond(news()));
    await flushPromises();
    expect(wrapper.vm.data.articles).toHaveLength(0);
    await load();
    expect(fetch).toHaveBeenCalledTimes(1);
  });
  it("does not surface errors after unmount", async () => {
    let reject!: (error: Error) => void;
    vi.stubGlobal(
      "fetch",
      vi.fn(
        () =>
          new Promise<Response>((_, fail) => {
            reject = fail;
          }),
      ),
    );
    const wrapper = mount(newsHarness);
    wrapper.unmount();
    reject(new Error("Aborted"));
    await flushPromises();
    expect(wrapper.vm.error).toBe(false);
  });
  it("times out hanging requests and permits a retry", async () => {
    vi.useFakeTimers();
    vi.stubGlobal(
      "fetch",
      vi.fn(
        (_: string, options: RequestInit) =>
          new Promise<Response>((_, reject) => {
            options.signal!.addEventListener("abort", () =>
              reject(new Error("Aborted")),
            );
          }),
      ),
    );
    const wrapper = mount(newsHarness);
    await vi.advanceTimersByTimeAsync(15_000);
    expect(wrapper.vm.error).toBe(true);
    expect(wrapper.vm.loading).toBe(false);
    vi.mocked(fetch).mockResolvedValueOnce(respond(news()));
    await wrapper.vm.load(true);
    expect(fetch).toHaveBeenLastCalledWith("/api/news?limit=50", expect.any(Object));
    expect(wrapper.vm.data.articles).toHaveLength(3);
  });
});

describe("filtered request isolation", () => {
  function deferred() {
    let resolve!: (value: Response) => void;
    let reject!: (error: Error) => void;
    const promise = new Promise<Response>((done, fail) => { resolve = done; reject = fail; });
    return { promise, resolve, reject };
  }
  const params = () => new URL(vi.mocked(fetch).mock.lastCall![0] as string, "https://example.com").searchParams;

  it("aborts and discards obsolete successes without finishing a newer request", async () => {
    const older = deferred();
    const current = deferred();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(news())).mockReturnValueOnce(older.promise).mockReturnValueOnce(current.promise));
    const wrapper = mount(newsHarness);
    await flushPromises();
    const oldLoad = wrapper.vm.setFilters("开发工具", "old");
    expect(wrapper.vm.data.articles).toHaveLength(0);
    expect(wrapper.vm.ready).toBe(false);
    const oldSignal = vi.mocked(fetch).mock.lastCall![1]!.signal!;
    const newLoad = wrapper.vm.setFilters("具身智能", " new ");
    expect(oldSignal.aborted).toBe(true);
    expect(wrapper.vm.query).toBe("new");
    older.resolve(respond(news()));
    await oldLoad;
    expect(wrapper.vm.data.articles).toHaveLength(0);
    expect(wrapper.vm.loading).toBe(true);
    expect(wrapper.vm.error).toBe(false);
    current.resolve(respond(news({ articles: [], pagination: { total: 0, offset: 0, limit: 50, has_more: false } })));
    await newLoad;
    expect(wrapper.vm.loading).toBe(false);
    expect(wrapper.vm.ready).toBe(true);
    expect(wrapper.vm.resultCount).toBe(0);
    expect(Object.fromEntries(params())).toEqual({ category: "具身智能", q: "new", limit: "50" });
    await wrapper.vm.load(true);
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("ignores obsolete rejection, preserves global metadata and resets append pagination", async () => {
    const append = deferred();
    const filtered = deferred();
    const base = news({ pagination: { total: 139, offset: 0, limit: 50, has_more: true }, stats: { total_articles: 139, recent_articles: 6, total_sources: 5 } });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(base)).mockReturnValueOnce(append.promise).mockReturnValueOnce(filtered.promise));
    const wrapper = mount(newsHarness);
    await flushPromises();
    const appendLoad = wrapper.vm.load(true);
    await wrapper.vm.load(true);
    await wrapper.vm.load();
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(params().get("offset")).toBe("50");
    const newLoad = wrapper.vm.setFilters("模型进展", "");
    expect(wrapper.vm.data.articles).toHaveLength(0);
    expect(wrapper.vm.data.stats).toEqual(base.stats);
    expect(wrapper.vm.data.sources).toEqual(base.sources);
    expect(wrapper.vm.hasMore).toBe(false);
    expect(params().has("offset")).toBe(false);
    append.reject(new Error("Old append aborted"));
    await appendLoad;
    expect(wrapper.vm.error).toBe(false);
    expect(wrapper.vm.loading).toBe(true);
    filtered.resolve(respond(news({ articles: [], pagination: { total: 0, offset: 0, limit: 50, has_more: false } })));
    await newLoad;
    expect(wrapper.vm.data.articles).toHaveLength(0);
    expect(wrapper.vm.loading).toBe(false);
  });

  it("also rejects an obsolete response whose body finishes after the newer response", async () => {
    const body = deferred();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce({ ok: true, json: () => body.promise }).mockResolvedValueOnce(respond(news({ articles: [] }))));
    const wrapper = mount(newsHarness);
    await flushPromises();
    await wrapper.vm.setFilters("研究前沿", "new");
    body.resolve(news() as unknown as Response);
    await flushPromises();
    expect(wrapper.vm.data.articles).toHaveLength(0);
    expect(wrapper.vm.query).toBe("new");
    expect(wrapper.vm.loading).toBe(false);
  });

  it("retries a failed next page with the same filters and retains existing articles", async () => {
    const firstPage = news({ pagination: { total: 6, offset: 0, limit: 3, has_more: true } });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(firstPage)));
    const wrapper = mount(newsHarness);
    await flushPromises();
    await wrapper.vm.setFilters("安全治理", " safety ");
    vi.mocked(fetch).mockRejectedValueOnce(new Error("Offline"));
    await wrapper.vm.load(true);
    expect(wrapper.vm.error).toBe(true);
    expect(wrapper.vm.data.articles).toHaveLength(3);
    vi.mocked(fetch).mockResolvedValueOnce(respond(news({ articles: [], pagination: { total: 6, offset: 3, limit: 3, has_more: false } })));
    await wrapper.vm.retry();
    expect(wrapper.vm.error).toBe(false);
    expect(wrapper.vm.data.articles).toHaveLength(3);
    expect(Object.fromEntries(params())).toEqual({ category: "安全治理", q: "safety", offset: "3", limit: "3" });
  });

  it("refreshes identical submitted filters, ignores pending duplicate submissions, and refuses changes after unmount", async () => {
    const pending = deferred();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(news())).mockReturnValueOnce(pending.promise));
    const wrapper = mount(newsHarness);
    await flushPromises();
    const refresh = wrapper.vm.setFilters("全部资讯", "  ");
    await wrapper.vm.setFilters("全部资讯", "");
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(wrapper.vm.data.articles).toHaveLength(3);
    expect(Object.fromEntries(params())).toEqual({ limit: "50" });
    const changeFilters = wrapper.vm.setFilters;
    wrapper.unmount();
    await changeFilters("具身智能", "robots");
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(wrapper.vm.active).toBe("全部资讯");
    pending.reject(new Error("Aborted"));
    await refresh;
    expect(wrapper.vm.error).toBe(false);
  });

  it("an obsolete timer never aborts the active query, and a query timeout remains retryable", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("fetch", vi.fn((_url: string, options: RequestInit) => new Promise<Response>((_, reject) => {
      options.signal!.addEventListener("abort", () => reject(new Error("Aborted")));
    })));
    const wrapper = mount(newsHarness);
    await vi.advanceTimersByTimeAsync(10_000);
    const filtered = wrapper.vm.setFilters("具身智能", "robots");
    const signal = vi.mocked(fetch).mock.lastCall![1]!.signal!;
    await vi.advanceTimersByTimeAsync(5_000);
    expect(signal.aborted).toBe(false);
    expect(wrapper.vm.loading).toBe(true);
    await vi.advanceTimersByTimeAsync(10_000);
    await filtered;
    expect(signal.aborted).toBe(true);
    expect(wrapper.vm.error).toBe(true);
    vi.mocked(fetch).mockResolvedValueOnce(respond(news()));
    await wrapper.vm.retry();
    expect(wrapper.vm.error).toBe(false);
    expect(params().get("category")).toBe("具身智能");
    expect(params().get("q")).toBe("robots");
  });
});
