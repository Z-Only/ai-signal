import { computed, onMounted, onUnmounted, ref } from "vue";
import { emptyNews, mergeArticles, parseNews } from "../core/news";
import type { Category } from "../core/types";

export function useNews() {
  const data = ref(emptyNews());
  const active = ref<Category>("全部资讯");
  const query = ref("");
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

  async function request(append: boolean): Promise<void> {
    controller?.abort();
    const current = ++requestId;
    const requestController = new AbortController();
    controller = requestController;
    const isCurrent = () => !disposed && current === requestId;
    loading.value = true;
    error.value = false;
    const timeout = window.setTimeout(() => requestController.abort(), 15_000);
    try {
      const params = new URLSearchParams();
      if (active.value !== "全部资讯") params.set("category", active.value);
      if (query.value) params.set("q", query.value);
      const page = data.value.pagination;
      if (append && page) {
        params.set("offset", String(page.offset + page.limit));
        params.set("limit", String(page.limit));
      }
      const suffix = params.size ? `?${params}` : "";
      const response = await fetch(`/api/news${suffix}`, {
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
  }

  async function load(append = false): Promise<void> {
    if (disposed || loading.value || (append && ready.value && !hasMore.value))
      return;
    await request(append);
  }

  async function setFilters(category: Category, search: string): Promise<void> {
    if (disposed) return;
    const trimmed = search.trim();
    if (active.value === category && query.value === trimmed) {
      await load();
      return;
    }
    active.value = category;
    query.value = trimmed;
    // Preserve library metadata, but never label old results with new filters.
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
    loading,
    error,
    ready,
    hasMore,
    resultCount,
    load,
    setFilters,
    retry,
  };
}
