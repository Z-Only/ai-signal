import { computed, onMounted, onUnmounted, ref } from "vue";
import { emptyNews, mergeArticles, parseNews } from "../core/news";
import { filterParams, sourcesKey } from "../core/filters";
import type { Category, SourceSelection } from "../core/types";

export function useNews(
  initial: { category?: Category; query?: string; limit?: number; sources?: SourceSelection; date?: string } = {},
) {
  const data = ref(emptyNews());
  const active = ref<Category>(initial.category ?? "全部资讯");
  const query = ref(initial.query ?? "");
  const sources = ref<SourceSelection>(initial.sources ?? null);
  const date = ref(initial.date ?? "");
  let requestedLimit = initial.limit ?? 50;
  const loading = ref(false);
  const error = ref(false);
  const ready = ref(false);
  let controller: AbortController | undefined;
  let requestId = 0;
  let disposed = false;
  let failedAppend = false;
  const hasMore = computed(() => data.value.pagination?.has_more ?? false);
  const resultCount = computed(
    () => data.value.pagination?.total ?? data.value.articles.length,
  );

  async function request(append: boolean): Promise<boolean> {
    controller?.abort();
    const current = ++requestId;
    const requestController = new AbortController();
    controller = requestController;
    const isCurrent = () => !disposed && current === requestId;
    loading.value = true;
    error.value = false;
    const timeout = window.setTimeout(() => requestController.abort(), 15_000);
    try {
      const params = filterParams(active.value, query.value, sources.value);
      if (date.value) params.set("date", date.value);
      const page = data.value.pagination;
      if (append && page) {
        params.set("offset", String(page.offset + page.limit));
        params.set("limit", String(page.limit));
      } else {
        const limit = Math.min(
          200,
          Math.max(requestedLimit, data.value.articles.length),
        );
        params.set("limit", String(limit));
      }
      const response = await fetch(`/api/news?${params}`, {
        signal: requestController.signal,
        headers: { Accept: "application/json" },
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const result = parseNews(await response.json());
      if (isCurrent()) {
        data.value = append
          ? {
              ...result,
              articles: mergeArticles(data.value.articles, result.articles),
            }
          : result;
        ready.value = true;
        return true;
      }
    } catch {
      if (isCurrent()) {
        error.value = true;
        failedAppend = append;
      }
    } finally {
      window.clearTimeout(timeout);
      if (isCurrent()) loading.value = false;
    }
    return false;
  }

  async function load(append = false): Promise<boolean> {
    if (disposed || loading.value || (append && ready.value && !hasMore.value))
      return false;
    return request(append);
  }

  async function setFilters(
    category: Category,
    search: string,
    limit = 50,
    selection: SourceSelection = sources.value,
    day = date.value,
  ): Promise<void> {
    if (disposed) return;
    const trimmed = search.trim();
    if (active.value === category && query.value === trimmed &&
      sourcesKey(sources.value) === sourcesKey(selection) && date.value === day) {
      await load();
      return;
    }
    await restoreFilters(category, trimmed, limit, selection, day);
  }

  async function restoreFilters(
    category: Category,
    search: string,
    limit: number,
    selection: SourceSelection = sources.value,
    day = date.value,
  ): Promise<void> {
    if (disposed) return;
    requestedLimit = limit;
    active.value = category;
    query.value = search.trim();
    sources.value = selection;
    date.value = day;
    // A history entry owns its saved range, including when filters are unchanged.
    // Clear the old page before requesting so refresh cannot retain a larger range.
    data.value = { ...data.value, articles: [], pagination: undefined };
    ready.value = false;
    await request(false);
  }

  const retry = () => load(failedAppend);
  onMounted(() => void load());
  onUnmounted(() => {
    disposed = true;
    controller?.abort();
  });
  return {
    data,
    active,
    query,
    sources,
    date,
    loading,
    error,
    ready,
    hasMore,
    resultCount,
    load,
    setFilters,
    restoreFilters,
    retry,
  };
}
