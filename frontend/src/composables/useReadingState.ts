import { onUnmounted, ref } from "vue";
import { categories, type Category, type View } from "../core/types";

interface ReadingState {
  view: View;
  category: Category;
  query: string;
  limit: number;
}
const defaultLimit = 50;
const maxRestoreLimit = 200;
const historyKey = "aiSignalReader";

export function useReadingState() {
  function read(): ReadingState {
    const params = new URLSearchParams(window.location.search);
    const candidate = params.get("category");
    const category = categories.includes(candidate as Category)
      ? (candidate as Category)
      : "全部资讯";
    const rawQuery = (params.get("q") ?? "").trim();
    const query =
      Array.from(rawQuery).length <= 200 && !rawQuery.includes("\0")
        ? rawQuery
        : "";
    const saved = window.history.state?.[historyKey];
    const limit =
      saved?.version === 1 && saved.category === category &&
      saved.query === query && Number.isSafeInteger(saved.limit) &&
      saved.limit >= defaultLimit && saved.limit <= maxRestoreLimit
        ? (saved.limit as number)
        : defaultLimit;
    return {
      view: params.get("view") === "sources" ? "sources" : "feed",
      category,
      query,
      limit,
    };
  }

  function write(state: ReadingState, mode: "push" | "replace") {
    const url = new URL(window.location.href);
    for (const key of ["view", "category", "q"]) url.searchParams.delete(key);
    if (state.view === "sources") url.searchParams.set("view", state.view);
    if (state.category !== "全部资讯")
      url.searchParams.set("category", state.category);
    if (state.query) url.searchParams.set("q", state.query);
    try {
      window.history[mode === "push" ? "pushState" : "replaceState"]({
        ...window.history.state,
        [historyKey]: {
          version: 1,
          category: state.category,
          query: state.query,
          limit: state.limit,
        },
      }, "", `${url.pathname}${url.search}${url.hash}`);
    } catch {
      // Restricted history access must not prevent reading or changing filters.
    }
  }

  const route = ref(read());
  const restoration = ref(0);
  write(route.value, "replace");
  function navigate(
    change: Partial<Pick<ReadingState, "view" | "category" | "query">>,
  ) {
    const next = { ...route.value, ...change };
    next.query = next.query.trim();
    const filtersChanged = next.category !== route.value.category ||
      next.query !== route.value.query;
    if (!filtersChanged && next.view === route.value.view) return;
    if (filtersChanged) next.limit = defaultLimit;
    write(next, "push");
    route.value = next;
  }
  function restore() {
    const next = read();
    // Signal traversal before the synchronous route watcher sees the entry.
    restoration.value += 1;
    route.value = next;
    write(route.value, "replace");
  }
  function rememberRange(count: number) {
    const limit = Math.min(maxRestoreLimit, Math.max(defaultLimit, count));
    if (route.value.limit === limit) return;
    route.value.limit = limit;
    write(route.value, "replace");
  }
  window.addEventListener("popstate", restore);
  onUnmounted(() => window.removeEventListener("popstate", restore));
  return { route, restoration, navigate, rememberRange };
}
