import { computed, onMounted, onUnmounted, ref } from "vue";
import { emptyNews, mergeArticles, parseNews } from "../core/news";
export function useNews() {
  const data = ref(emptyNews());
  const loading = ref(false);
  const error = ref(false);
  let controller: AbortController | undefined;
  let disposed = false;
  const hasMore = computed(() => data.value.pagination?.has_more ?? false);
  async function load(append = false): Promise<void> {
    if (loading.value || disposed) return;
    loading.value = true;
    error.value = false;
    controller = new AbortController();
    const timeout = window.setTimeout(() => controller?.abort(), 15_000);
    try {
      const page = data.value.pagination;
      const query =
        append && page
          ? `?offset=${page.offset + page.limit}&limit=${page.limit}`
          : "";
      const response = await fetch(`/api/news${query}`, {
        signal: controller.signal,
        headers: { Accept: "application/json" },
      });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const result = parseNews(await response.json());
      if (!disposed)
        data.value = append
          ? {
              ...result,
              articles: mergeArticles(data.value.articles, result.articles),
            }
          : result;
    } catch {
      if (!disposed) error.value = true;
    } finally {
      window.clearTimeout(timeout);
      loading.value = false;
    }
  }
  onMounted(() => void load());
  onUnmounted(() => {
    disposed = true;
    controller?.abort();
  });
  return { data, loading, error, hasMore, load };
}
