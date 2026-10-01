import { nextTick } from "vue";
import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import App from "../src/App.vue";
import { article, news, respond } from "./fixtures";

async function click(button: HTMLButtonElement, detail: number) {
  button.dispatchEvent(new MouseEvent("click", { bubbles: true, detail }));
  await nextTick();
}

async function setup() {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(news({
    pagination: { total: 6, offset: 0, limit: 3, has_more: true },
  }))));
  const wrapper = mount(App, { attachTo: document.body });
  await flushPromises();
  let resolve!: (response: Response) => void;
  vi.mocked(fetch).mockImplementationOnce(() => new Promise<Response>((done) => { resolve = done; }));
  return { wrapper, resolve: (response: Response) => resolve(response) };
}
const nextPage = (hasMore: boolean) => respond(news({
  articles: [article("4"), article("5"), article("6")],
  pagination: { total: hasMore ? 9 : 6, offset: 3, limit: 3, has_more: hasMore },
}));

describe("keyboard pagination", () => {
  it.each([true, false])("keeps the pending control focusable and moves to the first new article; more=%s", async (hasMore) => {
    const { wrapper, resolve } = await setup();
    const button = wrapper.find<HTMLButtonElement>(".pagination button");
    button.element.focus();
    await click(button.element, 0);
    expect(button.attributes("disabled")).toBeUndefined();
    expect(button.attributes("aria-disabled")).toBe("true");
    expect(document.activeElement).toBe(button.element);
    await click(button.element, 0);
    expect(fetch).toHaveBeenCalledTimes(2);
    resolve(nextPage(hasMore));
    await flushPromises();
    expect(document.activeElement).toBe(wrapper.find('[data-article-id="4"]').element);
    expect(wrapper.find(".pagination button").exists()).toBe(hasMore);
  });
  it("does not move focus for a pointer click", async () => {
    const { wrapper, resolve } = await setup();
    const button = wrapper.find<HTMLButtonElement>(".pagination button");
    button.element.focus();
    await click(button.element, 1);
    resolve(nextPage(true));
    await flushPromises();
    expect(document.activeElement).toBe(button.element);
  });
  it("does not steal focus when the reader moves elsewhere during loading", async () => {
    const { wrapper, resolve } = await setup();
    const button = wrapper.find<HTMLButtonElement>(".pagination button");
    button.element.focus();
    await click(button.element, 0);
    const search = wrapper.find<HTMLInputElement>("input");
    search.element.focus();
    resolve(nextPage(false));
    await flushPromises();
    expect(document.activeElement).toBe(search.element);
  });
  it("does not move focus when there are no new unique articles", async () => {
    const { wrapper, resolve } = await setup();
    const button = wrapper.find<HTMLButtonElement>(".pagination button");
    button.element.focus();
    await click(button.element, 0);
    resolve(respond(news({ pagination: { total: 9, offset: 3, limit: 3, has_more: true } })));
    await flushPromises();
    expect(document.activeElement).toBe(button.element);
  });
  it("handles navigation to Sources during the pending append", async () => {
    const { wrapper, resolve } = await setup();
    const button = wrapper.find<HTMLButtonElement>(".pagination button");
    button.element.focus();
    await click(button.element, 0);
    await wrapper.findAll("nav button")[1]!.trigger("click");
    resolve(nextPage(false));
    await flushPromises();
    expect(wrapper.find(".sources-page").exists()).toBe(true);
    expect(wrapper.find(".feed-results").exists()).toBe(false);
  });
});

describe("keyboard retry focus", () => {
  async function failedAppend() {
    const { wrapper, resolve } = await setup();
    const button = wrapper.find<HTMLButtonElement>(".pagination button");
    button.element.focus();
    await click(button.element, 0);
    resolve(respond(null, false));
    await flushPromises();
    expect(document.activeElement).toBe(button.element);
    const retry = wrapper.find<HTMLButtonElement>('[role="alert"] button');
    retry.element.focus();
    return { wrapper, retry };
  }
  it("moves keyboard Retry to the first new article on a successful final page", async () => {
    const { wrapper, retry } = await failedAppend();
    vi.mocked(fetch).mockResolvedValueOnce(nextPage(false));
    await click(retry.element, 0);
    await flushPromises();
    expect(document.activeElement).toBe(wrapper.find('[data-article-id="4"]').element);
    expect(wrapper.find(".pagination button").exists()).toBe(false);
    expect(new URL(vi.mocked(fetch).mock.lastCall![0] as string, window.location.origin).searchParams.get("offset")).toBe("3");
  });
  it("returns focus to Retry if the retry also fails", async () => {
    const { wrapper, retry } = await failedAppend();
    vi.mocked(fetch).mockResolvedValueOnce(respond(null, false));
    await click(retry.element, 0);
    await flushPromises();
    expect(document.activeElement).toBe(wrapper.find('[role="alert"] button').element);
  });
  it("uses the first existing article if a retried refresh returns the same articles", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(news())));
    const wrapper = mount(App, { attachTo: document.body });
    await flushPromises();
    vi.mocked(fetch).mockResolvedValueOnce(respond(null, false));
    await wrapper.find(".page-head .reload").trigger("click");
    await flushPromises();
    const retry = wrapper.find<HTMLButtonElement>('[role="alert"] button');
    retry.element.focus();
    vi.mocked(fetch).mockResolvedValueOnce(respond(news()));
    await click(retry.element, 0);
    await flushPromises();
    expect(document.activeElement).toBe(wrapper.find(".lead-card").element);
  });
  it("focuses the main result region after an empty retry result", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(null, false)));
    const wrapper = mount(App, { attachTo: document.body });
    await flushPromises();
    const retry = wrapper.find<HTMLButtonElement>('[role="alert"] button');
    retry.element.focus();
    vi.mocked(fetch).mockResolvedValueOnce(respond(news({ articles: [] })));
    await click(retry.element, 0);
    await flushPromises();
    expect(document.activeElement).toBe(wrapper.find("main").element);
  });
  it("focuses the last existing article if the final page has only duplicates", async () => {
    const { wrapper, resolve } = await setup();
    const button = wrapper.find<HTMLButtonElement>(".pagination button");
    button.element.focus();
    await click(button.element, 0);
    resolve(respond(news({ pagination: { total: 3, offset: 3, limit: 3, has_more: false } })));
    await flushPromises();
    expect(document.activeElement).toBe(wrapper.find('[data-article-id="3"]').element);
  });
});

it("keeps a Sources retry in its main region without requiring a feed target", async () => {
  window.history.replaceState(null, "", "?view=sources");
  vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond(null, false)));
  const wrapper = mount(App, { attachTo: document.body });
  await flushPromises();
  const retry = wrapper.find<HTMLButtonElement>('[role="alert"] button');
  retry.element.focus();
  vi.mocked(fetch).mockResolvedValueOnce(respond(news()));
  await click(retry.element, 0);
  await flushPromises();
  expect(document.activeElement).toBe(wrapper.find("main").element);
  expect(wrapper.find(".sources-page").exists()).toBe(true);
});
