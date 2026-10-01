import { defineComponent, h, ref } from "vue";
import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import App from "../src/App.vue";
import SourceFilters from "../src/components/SourceFilters.vue";
import TimelineView from "../src/components/TimelineView.vue";
import { useNews } from "../src/composables/useNews";
import { useReadingState } from "../src/composables/useReadingState";
import { useTimeline } from "../src/composables/useTimeline";
import { filterParams, normalizeDays, normalizeSources, sourcesKey, validDate } from "../src/core/filters";
import { translate } from "../src/core/i18n";
import { parseTimeline } from "../src/core/timeline";
import type { SourceSelection, TimelineData, TimelineDays } from "../src/core/types";
import { article, news, respond } from "./fixtures";

const t = translate.bind(null, "en");
const timeline = (days: TimelineDays = 30, count = 2): TimelineData => ({
  days, timezone: "UTC", total: days * count,
  buckets: Array.from({ length: days }, (_, index) => ({
    date: new Date(Date.parse("2026-10-01T00:00:00Z") - (days - index - 1) * 86_400_000).toISOString().slice(0, 10), count,
  })),
});
const params = (url: string) => new URL(url, window.location.origin).searchParams;
const last = () => params(vi.mocked(fetch).mock.lastCall![0] as string);
const timelineCalls = () => vi.mocked(fetch).mock.calls.filter(([url]) => String(url).startsWith("/api/timeline"));
function mockApi() {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => String(url).startsWith("/api/timeline")
    ? respond(timeline(Number(params(url).get("days")) as TimelineDays))
    : respond(news())));
}
async function traverse(direction: "back" | "forward") {
  const done = new Promise<void>((resolve) => window.addEventListener("popstate", () => resolve(), { once: true }));
  window.history[direction]();
  await done;
  await flushPromises();
}
const reader = defineComponent({ setup: () => useReadingState(), render: () => h("div") });
const timelineHarness = defineComponent({
  setup() {
    const enabled = ref(false);
    const filters = ref({ category: "全部资讯" as const, query: "", sources: null as SourceSelection, days: 30 as TimelineDays, date: "" });
    return { enabled, filters, ...useTimeline(() => filters.value, () => enabled.value) };
  }, render: () => h("div"),
});

describe("bounded source and UTC date filters", () => {
  it("distinguishes omitted all from explicit none, sorts and deduplicates IDs", () => {
    expect(normalizeSources(null)).toBeNull();
    expect(normalizeSources("")).toEqual([]);
    expect(normalizeSources("  openai,google,openai, ")).toEqual(["google", "openai"]);
    expect(normalizeSources([" nvidia", "google"])).toEqual(["google", "nvidia"]);
    expect(sourcesKey(null)).toBeNull();
    expect(sourcesKey([])).toBe("");
    expect(sourcesKey(["google", "openai"])).toBe("google,openai");
    expect(normalizeSources(Array.from({ length: 33 }, () => "openai"))).toBeNull();
    expect(normalizeSources("x".repeat(2049))).toBeNull();
    expect(normalizeSources("x".repeat(65))).toBeNull();
    expect(normalizeSources("openai,<script>")).toBeNull();
    expect(normalizeSources("信源")).toBeNull();
    expect(Object.fromEntries(filterParams("模型进展", "robots", []))).toEqual({ category: "模型进展", q: "robots", sources: "" });
    expect(filterParams("全部资讯", "", null).toString()).toBe("");
  });
  it.each(["2026-02-29", "2026-04-31", "2026-1-01", "2026-13-01", "2026-01-01Z", "invalid"])("rejects non-calendar date %s", (value) => {
    expect(validDate(value)).toBe(false);
  });
  it("accepts calendar leap days and only the supported day ranges", () => {
    expect(validDate("2024-02-29")).toBe(true);
    expect(validDate("0001-01-01")).toBe(true);
    expect(validDate("0000-01-01")).toBe(true);
    for (const value of [7, "7"]) expect(normalizeDays(value)).toBe(7);
    for (const value of [90, "90"]) expect(normalizeDays(value)).toBe(90);
    for (const value of [30, "30", 14, "7x", "999999", null]) expect(normalizeDays(value)).toBe(30);
  });
  it("normalizes sources/date/days URLs and preserves unrelated fields", () => {
    window.history.replaceState(null, "", "/?view=timeline&sources=openai,google,openai&date=2024-02-29&days=7&keep=yes#feed");
    const wrapper = mount(reader);
    expect(wrapper.vm.route).toMatchObject({ view: "timeline", sources: ["google", "openai"], date: "2024-02-29", days: 7 });
    expect(params(location.href).get("sources")).toBe("google,openai");
    expect(location.hash).toBe("#feed");
    expect(params(location.href).get("keep")).toBe("yes");
    wrapper.vm.navigate({ sources: [], date: "2026-02-29", days: 90 });
    expect(params(location.href).get("sources")).toBe("");
    expect(params(location.href).has("date")).toBe(false);
    expect(params(location.href).get("days")).toBe("90");
    wrapper.vm.navigate({ sources: null, query: "x".repeat(201), days: 30 });
    expect(params(location.href).has("sources")).toBe(false);
    expect(params(location.href).has("days")).toBe(false);
    expect(wrapper.vm.route.query).toBe("");
  });
  it("rejects invalid URL filters while preserving bounded unknown IDs for an explicit server error", () => {
    window.history.replaceState(null, "", "?sources=" + "a,".repeat(33) + "&date=2026-02-30&days=91");
    const wrapper = mount(reader);
    expect(wrapper.vm.route).toMatchObject({ sources: null, date: "", days: 30 });
    wrapper.vm.navigate({ sources: ["future-source"] });
    expect(wrapper.vm.route.sources).toEqual(["future-source"]);
  });
  it("binds range history to sources and date; Back/Forward keeps empty selections and ranges", async () => {
    const wrapper = mount(reader);
    wrapper.vm.navigate({ sources: ["openai"], date: "2026-10-01" });
    wrapper.vm.rememberRange(100);
    const saved = window.history.state;
    wrapper.vm.navigate({ sources: [], view: "timeline", days: 7 });
    expect(wrapper.vm.route.limit).toBe(50);
    await traverse("back");
    expect(wrapper.vm.route).toMatchObject({ sources: ["openai"], date: "2026-10-01", limit: 100, days: 30 });
    await traverse("forward");
    expect(wrapper.vm.route).toMatchObject({ sources: [], view: "timeline", days: 7, limit: 50 });
    window.history.replaceState(saved, "", "?sources=google&date=2026-10-01");
    window.dispatchEvent(new PopStateEvent("popstate"));
    expect(wrapper.vm.route.limit).toBe(50);
    window.history.replaceState(saved, "", "?sources=openai&date=2026-09-30");
    window.dispatchEvent(new PopStateEvent("popstate"));
    expect(wrapper.vm.route.limit).toBe(50);
  });
});

describe("validated UTC timeline responses", () => {
  it("accepts zero-filled ascending calendar days with exact totals", () => {
    expect(parseTimeline(timeline(7, 0), 7)).toEqual(timeline(7, 0));
    expect(parseTimeline(timeline(90, 1), 90).total).toBe(90);
  });
  it.each([
    null, 7, {}, { ...timeline(7), timezone: "local" }, { ...timeline(7), days: 30 },
    { ...timeline(7), buckets: [] }, { ...timeline(7), total: -1 },
    { ...timeline(7), total: 1.5 }, { ...timeline(7), total: 999 },
    { ...timeline(7), buckets: [null, ...timeline(7).buckets.slice(1)] },
    { ...timeline(7), buckets: [{ date: "invalid", count: 2 }, ...timeline(7).buckets.slice(1)] },
    { ...timeline(7), buckets: [{ date: 7, count: 2 }, ...timeline(7).buckets.slice(1)] },
    { ...timeline(7), buckets: [{ date: "2026-09-25", count: -1 }, ...timeline(7).buckets.slice(1)] },
    { ...timeline(7), buckets: [{ date: "2026-09-25", count: 1.5 }, ...timeline(7).buckets.slice(1)] },
    { ...timeline(7), buckets: [{ date: "2026-09-24", count: 2 }, ...timeline(7).buckets.slice(1)] },
  ])("rejects misleading or malformed statistics %#", (value) => {
    expect(() => parseTimeline(value, 7)).toThrow();
  });
});

describe("accessible dynamic source selection", () => {
  it("starts with every API source selected and offers native labeled checkboxes", async () => {
    const sources = [...news().sources, { id: "new-vendor", name: "New Official Vendor", home: "https://example.com" }];
    const wrapper = mount(SourceFilters, { props: { sources, selected: null, t } });
    expect(wrapper.element.tagName).toBe("FIELDSET");
    expect(wrapper.find("legend").text()).toBe("Filter official sources");
    expect(wrapper.text()).toContain("including future additions");
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(4);
    expect(wrapper.find("summary").text()).toBe("Choose individual sources (4 available)");
    expect(wrapper.find("details").attributes("open")).toBeUndefined();
    await wrapper.find("summary").trigger("click");
    expect((wrapper.find("details").element as HTMLDetailsElement).open).toBe(true);
    expect(wrapper.findAll("input").every((input) => (input.element as HTMLInputElement).checked)).toBe(true);
    await wrapper.find('input[value="new-vendor"]').setValue(false);
    expect(wrapper.emitted("select")![0]).toEqual([["google", "nvidia", "openai"]]);
    await wrapper.setProps({ selected: ["openai"] });
    await wrapper.find('input[value="new-vendor"]').setValue(true);
    expect(wrapper.emitted("select")![1]).toEqual([["new-vendor", "openai"]]);
    await wrapper.findAll("button")[1]!.trigger("click");
    expect(wrapper.emitted("select")![2]).toEqual([[]]);
    await wrapper.setProps({ selected: [] });
    expect(wrapper.find('[role="status"]').text()).toContain("Select at least one source");
    await wrapper.findAll("button")[0]!.trigger("click");
    expect(wrapper.emitted("select")![3]).toEqual([null]);
  });
  it("keeps reset controls available when the catalog cannot load", async () => {
    const wrapper = mount(SourceFilters, { props: { sources: [], selected: ["unknown"], t } });
    expect(wrapper.text()).toContain("Source list is unavailable");
    await wrapper.find("button").trigger("click");
    expect(wrapper.emitted("select")).toEqual([[null]]);
  });
  it("combines sources with query/category/date, resets pages and carries filters through append", async () => {
    window.history.replaceState(null, "", "?sources=openai&date=2026-10-01&q=model&category=" + encodeURIComponent("模型进展"));
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(news({ pagination: { total: 200, limit: 50, offset: 0, has_more: true } }))));
    const wrapper = mount(App);
    await flushPromises();
    expect(Object.fromEntries(last())).toEqual({ category: "模型进展", q: "model", sources: "openai", date: "2026-10-01", limit: "50" });
    expect((wrapper.find('input[value="openai"]').element as HTMLInputElement).checked).toBe(true);
    expect((wrapper.find('input[value="nvidia"]').element as HTMLInputElement).checked).toBe(false);
    await wrapper.find(".pagination button").trigger("click");
    await flushPromises();
    expect(Object.fromEntries(last())).toMatchObject({ sources: "openai", date: "2026-10-01", offset: "50" });
    await wrapper.find('input[value="nvidia"]').setValue(true);
    await flushPromises();
    expect(last().get("sources")).toBe("nvidia,openai");
    expect(last().has("offset")).toBe(false);
    expect(last().get("q")).toBe("model");
    await wrapper.find(".date-filter button").trigger("click");
    await flushPromises();
    expect(last().has("date")).toBe(false);
    expect(last().get("sources")).toBe("nvidia,openai");
  });
  it("renders an explicit none-selected state, preserves global stats, and recovers by selecting all", async () => {
    mockApi();
    const wrapper = mount(App);
    await flushPromises();
    vi.mocked(fetch).mockResolvedValueOnce(respond(news({ articles: [], pagination: { total: 0, offset: 0, limit: 50, has_more: false }, stats: { total_articles: 200, recent_articles: 10, total_sources: 3 } })));
    await wrapper.findAll(".source-filter-actions button")[1]!.trigger("click");
    await flushPromises();
    expect(last().get("sources")).toBe("");
    expect(wrapper.find(".empty").text()).toContain("尚未选择信源");
    expect(wrapper.find(".stats").text()).toContain("200");
    expect(wrapper.findAll('input[type="checkbox"]').every((input) => !(input.element as HTMLInputElement).checked)).toBe(true);
    await wrapper.find(".source-filter-actions button").trigger("click");
    await flushPromises();
    expect(last().has("sources")).toBe(false);
    expect(wrapper.find(".lead-card").exists()).toBe(true);
  });
  it.each(["feed", "sources"])("recovers an unknown-source initial error from the %s view without a catalog", async (view) => {
    window.history.replaceState(null, "", `?view=${view}&sources=unknown`);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(respond({ error: "Unknown source" }, false)).mockResolvedValueOnce(respond(news())));
    const wrapper = mount(App);
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(true);
    expect(wrapper.find(".source-catalog-status").exists()).toBe(true);
    await wrapper.find(".source-filter-actions button").trigger("click");
    await flushPromises();
    expect(last().has("sources")).toBe(false);
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(3);
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
  });
  it("aborts obsolete source loads and keeps catalog metadata during filter failures", async () => {
    mockApi();
    const wrapper = mount(App);
    await flushPromises();
    let finish!: (value: Response) => void;
    vi.mocked(fetch).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    await wrapper.find('input[value="openai"]').setValue(false);
    const signal = vi.mocked(fetch).mock.lastCall![1]!.signal!;
    vi.mocked(fetch).mockRejectedValueOnce(new Error("Offline"));
    await wrapper.find('input[value="google"]').setValue(false);
    await flushPromises();
    expect(signal.aborted).toBe(true);
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(3);
    expect(wrapper.find(".lead-card").exists()).toBe(false);
    finish(respond(news({ articles: [article("obsolete")] })));
    await flushPromises();
    expect(wrapper.find(".lead-card").exists()).toBe(false);
    await wrapper.find('[role="alert"] button').trigger("click");
    await flushPromises();
    expect(last().get("sources")).toBe("nvidia");
  });
  it("uses default sources/date on direct same-filter reload and ignores calls after disposal", async () => {
    mockApi();
    const wrapper = mount(defineComponent({ setup: () => useNews({ sources: ["openai"], date: "2026-10-01" }), render: () => h("div") }));
    await flushPromises();
    await wrapper.vm.setFilters("全部资讯", "");
    expect(last().get("sources")).toBe("openai");
    const restore = wrapper.vm.restoreFilters;
    wrapper.unmount();
    await restore("全部资讯", "", 50, [], "");
    expect(fetch).toHaveBeenCalledTimes(2);
  });
});

describe("timeline fetch lifecycle", () => {
  it("fetches only while visible and only for relevant filters, not date or result bookkeeping", async () => {
    mockApi();
    const wrapper = mount(timelineHarness);
    expect(fetch).not.toHaveBeenCalled();
    await wrapper.vm.load();
    expect(fetch).not.toHaveBeenCalled();
    wrapper.vm.enabled = true;
    await flushPromises();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(last().get("days")).toBe("30");
    wrapper.vm.filters.date = "2026-09-30";
    await flushPromises();
    expect(fetch).toHaveBeenCalledTimes(1);
    wrapper.vm.filters.sources = [];
    await flushPromises();
    expect(last().get("sources")).toBe("");
    wrapper.vm.filters.days = 7;
    await flushPromises();
    expect(wrapper.vm.data!.buckets).toHaveLength(7);
    wrapper.vm.enabled = false;
    await flushPromises();
    wrapper.vm.filters.query = "new search";
    await flushPromises();
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(wrapper.vm.data).toBeNull();
    wrapper.vm.enabled = true;
    await flushPromises();
    expect(last().get("q")).toBe("new search");
    expect(last().has("date")).toBe(false);
  });
  it.each(["success", "error"])("aborts stale requests and ignores their late %s", async (outcome) => {
    let finish!: (value: Response) => void;
    let fail!: (error: Error) => void;
    vi.stubGlobal("fetch", vi.fn().mockImplementationOnce(() => new Promise((resolve, reject) => { finish = resolve; fail = reject; })).mockResolvedValue(respond(timeline(7))));
    const wrapper = mount(timelineHarness);
    wrapper.vm.enabled = true;
    await flushPromises();
    await wrapper.vm.load();
    expect(fetch).toHaveBeenCalledTimes(1);
    const signal = vi.mocked(fetch).mock.lastCall![1]!.signal!;
    wrapper.vm.filters.days = 7;
    await flushPromises();
    expect(signal.aborted).toBe(true);
    if (outcome === "success") finish(respond(timeline()));
    else fail(new Error("Old failure"));
    await flushPromises();
    expect(wrapper.vm.data!.days).toBe(7);
    expect(wrapper.vm.error).toBe(false);
    expect(wrapper.vm.loading).toBe(false);
  });
  it("clears pending loading on navigation away and aborts on unmount without late state writes", async () => {
    let finish!: (value: Response) => void;
    vi.stubGlobal("fetch", vi.fn().mockImplementation(() => new Promise((resolve) => { finish = resolve; })));
    const wrapper = mount(timelineHarness);
    wrapper.vm.enabled = true;
    await flushPromises();
    const first = vi.mocked(fetch).mock.lastCall![1]!.signal!;
    wrapper.vm.enabled = false;
    await flushPromises();
    expect(first.aborted).toBe(true);
    expect(wrapper.vm.loading).toBe(false);
    finish(respond(timeline()));
    await flushPromises();
    expect(wrapper.vm.data).toBeNull();
    wrapper.vm.enabled = true;
    await flushPromises();
    const lastSignal = vi.mocked(fetch).mock.lastCall![1]!.signal!;
    const load = wrapper.vm.load;
    wrapper.unmount();
    expect(lastSignal.aborted).toBe(true);
    finish(respond(timeline()));
    await flushPromises();
    await load();
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(wrapper.vm.data).toBeNull();
  });
  it("times out, reports failure, and retries valid data", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("fetch", vi.fn((_url, init) => new Promise((_resolve, reject) => init!.signal!.addEventListener("abort", () => reject(new Error("Timeout"))))));
    const wrapper = mount(timelineHarness);
    wrapper.vm.enabled = true;
    await wrapper.vm.$nextTick();
    await vi.advanceTimersByTimeAsync(15_000);
    expect(wrapper.vm.error).toBe(true);
    expect(wrapper.vm.loading).toBe(false);
    expect(wrapper.vm.data).toBeNull();
    vi.mocked(fetch).mockResolvedValueOnce(respond({}));
    await wrapper.vm.load();
    expect(wrapper.vm.error).toBe(true);
    vi.mocked(fetch).mockResolvedValueOnce(respond(null, false));
    await wrapper.vm.load();
    expect(wrapper.vm.error).toBe(true);
    vi.mocked(fetch).mockResolvedValueOnce(respond(timeline()));
    await wrapper.vm.load();
    expect(wrapper.vm.data!.total).toBe(60);
    expect(wrapper.vm.error).toBe(false);
  });
});

describe("timeline view and reader integration", () => {
  it("exposes every day as a labeled keyboard/touch button with real zero counts", async () => {
    const wrapper = mount(TimelineView, { props: { data: timeline(7, 0), loading: false, error: false, days: 7, selectedDate: "2026-10-01", noSources: false, t } });
    expect(wrapper.findAll(".timeline-days button")).toHaveLength(7);
    expect(wrapper.find(".timeline-days button").attributes("aria-label")).toBe("2026-09-25 UTC: 0 articles. Open this day");
    expect(wrapper.find(".timeline-days button:last-child").exists()).toBe(true);
    expect(wrapper.find('button[aria-label^="2026-10-01"]').attributes("aria-pressed")).toBe("true");
    expect(wrapper.find(".timeline-status").text()).toContain("No matching publications");
    expect(wrapper.find(".timeline-chart-caption").text()).toContain("Peak: 0 / day");
    expect(wrapper.find("svg").attributes("aria-hidden")).toBe("true");
    await wrapper.find(".timeline-days button").trigger("click");
    expect(wrapper.emitted("select")).toEqual([["2026-09-25"]]);
    await wrapper.findAll(".timeline-ranges button")[2]!.trigger("click");
    expect(wrapper.emitted("days")).toEqual([[90]]);
    await wrapper.setProps({ noSources: true });
    expect(wrapper.find(".notice").text()).toContain("Select at least one source");
    expect(wrapper.find(".timeline-status").exists()).toBe(false);
  });
  it("never substitutes fake zeros during pending or failed counts and can retry", async () => {
    const wrapper = mount(TimelineView, { props: { data: null, loading: true, error: false, days: 30, selectedDate: "", noSources: false, t } });
    expect(wrapper.find(".timeline-status").text()).toBe("Loading daily counts…");
    expect(wrapper.find(".timeline-total").exists()).toBe(false);
    await wrapper.setProps({ loading: false, error: true, data: timeline() });
    expect(wrapper.find('[role="alert"]').text()).toContain("Unable to load daily counts");
    expect(wrapper.find(".timeline-total").exists()).toBe(false);
    expect(wrapper.find(".timeline-days").exists()).toBe(false);
    await wrapper.find('[role="alert"] button').trigger("click");
    expect(wrapper.emitted("retry")).toEqual([[]]);
  });
  it("opens timeline, changes range, opens a UTC day in the same filtered feed, clears date, and restores Back", async () => {
    window.history.replaceState(null, "", "?sources=openai&q=robot&category=" + encodeURIComponent("具身智能"));
    mockApi();
    const wrapper = mount(App, { attachTo: document.body });
    await flushPromises();
    expect(timelineCalls()).toHaveLength(0);
    await wrapper.findAll("nav button")[2]!.trigger("click");
    await flushPromises();
    expect(timelineCalls()).toHaveLength(1);
    expect(wrapper.findAll("nav button")[2]!.attributes("aria-current")).toBe("page");
    expect(Object.fromEntries(params(timelineCalls()[0]![0] as string))).toEqual({ category: "具身智能", q: "robot", sources: "openai", days: "30" });
    expect(wrapper.find(".timeline-total").text()).toContain("60");
    await wrapper.findAll(".timeline-ranges button")[0]!.trigger("click");
    await flushPromises();
    expect(timelineCalls()).toHaveLength(2);
    expect(wrapper.findAll(".timeline-days button")).toHaveLength(7);
    await wrapper.findAll(".timeline-ranges button")[0]!.trigger("click");
    await flushPromises();
    expect(timelineCalls()).toHaveLength(2);
    await wrapper.find(".timeline-days button").trigger("click");
    await flushPromises();
    expect(Object.fromEntries(last())).toEqual({ category: "具身智能", q: "robot", sources: "openai", date: "2026-09-25", limit: "50" });
    expect(wrapper.find(".date-filter").text()).toContain("2026-09-25（UTC）");
    expect(document.activeElement?.id).toBe("main-content");
    expect(wrapper.find(".timeline-panel").exists()).toBe(false);
    expect(params(location.href).get("date")).toBe("2026-09-25");
    await wrapper.find(".date-filter button").trigger("click");
    await flushPromises();
    expect(last().has("date")).toBe(false);
    expect(last().get("sources")).toBe("openai");
    await traverse("back");
    expect(last().get("date")).toBe("2026-09-25");
    await traverse("back");
    expect(wrapper.find(".timeline-panel").exists()).toBe(true);
    expect(wrapper.findAll(".timeline-days button")).toHaveLength(7);
    expect(timelineCalls()).toHaveLength(3);
  });
  it("ignores feed date for aggregation and reacts exactly once to category/search/source/range changes", async () => {
    window.history.replaceState(null, "", "?view=timeline&date=2026-09-29");
    mockApi();
    const wrapper = mount(App);
    await flushPromises();
    expect(timelineCalls()).toHaveLength(1);
    expect(params(timelineCalls()[0]![0] as string).has("date")).toBe(false);
    await wrapper.find(".date-filter button").trigger("click");
    await flushPromises();
    expect(timelineCalls()).toHaveLength(1);
    await wrapper.findAll(".tabs button")[1]!.trigger("click");
    await flushPromises();
    expect(timelineCalls()).toHaveLength(2);
    await wrapper.find("form input").setValue("new");
    await wrapper.find("form").trigger("submit");
    await flushPromises();
    expect(timelineCalls()).toHaveLength(3);
    await wrapper.find('input[value="openai"]').setValue(false);
    await flushPromises();
    expect(timelineCalls()).toHaveLength(4);
    expect(params(timelineCalls().at(-1)![0] as string).get("sources")).toBe("google,nvidia");
    await wrapper.findAll("select")[0]!.setValue("en");
    await wrapper.findAll("select")[1]!.setValue("dark");
    expect(wrapper.find("h1").text()).toBe("Follow the pace of AI");
    expect(wrapper.find(".timeline-timezone").text()).toContain("Calendar-day counts use UTC");
    expect(wrapper.find(".timeline-heading").text()).toContain("does not narrow this chart");
    expect(timelineCalls()).toHaveLength(4);
    await wrapper.find(".page-head .reload").trigger("click");
    await flushPromises();
    expect(timelineCalls()).toHaveLength(5);
    await wrapper.findAll("nav button")[1]!.trigger("click");
    await wrapper.find(".page-head .reload").trigger("click");
    await flushPromises();
    expect(timelineCalls()).toHaveLength(5);
  });
  it("recovers timeline failures independently from catalog/feed refresh", async () => {
    window.history.replaceState(null, "", "?view=timeline");
    vi.stubGlobal("fetch", vi.fn(async (url: string) => String(url).startsWith("/api/timeline") ? respond(null, false) : respond(news())));
    const wrapper = mount(App);
    await flushPromises();
    expect(wrapper.find(".timeline-panel [role='alert']").exists()).toBe(true);
    expect(wrapper.find(".timeline-total").exists()).toBe(false);
    vi.mocked(fetch).mockResolvedValueOnce(respond(timeline()));
    await wrapper.find(".timeline-panel [role='alert'] button").trigger("click");
    await flushPromises();
    expect(wrapper.find(".timeline-total").text()).toContain("60");
    expect(fetch).toHaveBeenCalledTimes(3);
  });
});
