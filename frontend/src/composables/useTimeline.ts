import { onUnmounted, ref, watch } from "vue";
import { filterParams, sourcesKey } from "../core/filters";
import { parseTimeline } from "../core/timeline";
import type { Category, SourceSelection, TimelineData, TimelineDays } from "../core/types";

interface TimelineFilters {
  category: Category;
  query: string;
  sources: SourceSelection;
  days: TimelineDays;
}
export function useTimeline(filters: () => TimelineFilters, enabled: () => boolean) {
  const data = ref<TimelineData | null>(null);
  const loading = ref(false);
  const error = ref(false);
  let controller: AbortController | undefined;
  let requestId = 0;
  let disposed = false;
  const key = () => {
    const value = filters();
    return JSON.stringify([value.category, value.query, sourcesKey(value.sources), value.days]);
  };
  async function request() {
    controller?.abort();
    const current = ++requestId;
    const currentFilters = filters();
    const requestController = new AbortController();
    controller = requestController;
    const isCurrent = () => !disposed && current === requestId;
    loading.value = true;
    error.value = false;
    const timeout = window.setTimeout(() => requestController.abort(), 15_000);
    try {
      const params = filterParams(currentFilters.category, currentFilters.query, currentFilters.sources);
      params.set("days", String(currentFilters.days));
      const response = await fetch(`/api/timeline?${params}`, {
        signal: requestController.signal,
        headers: { Accept: "application/json" },
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const result = parseTimeline(await response.json(), currentFilters.days);
      if (isCurrent()) data.value = result;
    } catch {
      if (isCurrent()) error.value = true;
    } finally {
      window.clearTimeout(timeout);
      if (isCurrent()) loading.value = false;
    }
  }
  async function load() {
    if (disposed || !enabled() || loading.value) return;
    await request();
  }
  watch([enabled, key], ([active], previous) => {
    // Only the visible timeline fetches. A date change never changes this key.
    controller?.abort();
    requestId += 1;
    loading.value = false;
    error.value = false;
    if (previous[1] !== key()) data.value = null;
    if (active) void request();
  }, { immediate: true, flush: "sync" });
  onUnmounted(() => {
    disposed = true;
    controller?.abort();
  });
  return { data, loading, error, load };
}
