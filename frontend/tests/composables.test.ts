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
    expect(fetch).toHaveBeenLastCalledWith("/api/news", expect.any(Object));
    expect(wrapper.vm.data.articles).toHaveLength(3);
  });
});
