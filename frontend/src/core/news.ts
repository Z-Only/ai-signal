import type { Article, Locale, NewsData } from "./types";
import { translate } from "./i18n";
export function emptyNews(): NewsData {
  return { articles: [], sources: [], run: null, schedule: "" };
}
export function parseNews(value: unknown): NewsData {
  if (!value || typeof value !== "object")
    throw new Error("Invalid news response");
  const data = value as NewsData;
  if (
    !Array.isArray(data.articles) ||
    !Array.isArray(data.sources) ||
    typeof data.schedule !== "string"
  )
    throw new Error("Invalid news response");
  for (const article of data.articles) {
    if (
      !article ||
      [
        "id",
        "title",
        "url",
        "source",
        "category",
        "summary",
        "published_at",
      ].some((key) => typeof article[key as keyof Article] !== "string")
    )
      throw new Error("Invalid article");
  }
  for (const source of data.sources) {
    if (
      !source ||
      typeof source.id !== "string" ||
      typeof source.name !== "string" ||
      typeof source.home !== "string"
    )
      throw new Error("Invalid source");
  }
  if (
    data.run &&
    (!Array.isArray(data.run.details) ||
      !["success", "partial", "failed", "running"].includes(data.run.status))
  )
    throw new Error("Invalid run");
  if (
    data.pagination &&
    (![
      data.pagination.total,
      data.pagination.offset,
      data.pagination.limit,
    ].every((v) => Number.isSafeInteger(v) && v >= 0) ||
      typeof data.pagination.has_more !== "boolean" ||
      data.pagination.limit === 0)
  )
    throw new Error("Invalid pagination");
  if (
    data.stats &&
    ![
      data.stats.total_articles,
      data.stats.recent_articles,
      data.stats.total_sources,
    ].every((v) => Number.isSafeInteger(v) && v >= 0)
  )
    throw new Error("Invalid statistics");
  return { ...data, run: data.run ?? null };
}
export function safeUrl(value: string): string | undefined {
  try {
    const url = new URL(value);
    return url.protocol === "https:" ? url.href : undefined;
  } catch {
    return undefined;
  }
}
export function formatDate(value: string, locale: Locale): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? translate(locale, "unknownDate")
    : date.toLocaleString(locale, {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      });
}
export function recentCount(articles: Article[], now = Date.now()): number {
  return articles.filter((article) => {
    const age = now - Date.parse(article.published_at);
    return age >= 0 && age < 86_400_000;
  }).length;
}
export function sourceInitial(id: string): string {
  return id.slice(0, 1).toUpperCase();
}
export function mergeArticles(previous: Article[], next: Article[]): Article[] {
  return [
    ...new Map(
      [...previous, ...next].map((article) => [article.id, article]),
    ).values(),
  ];
}
