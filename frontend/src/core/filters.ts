import type { Category, SourceSelection, TimelineDays } from "./types";

export function normalizeSources(value: string | SourceSelection): SourceSelection {
  if (value === null) return null;
  const ids = typeof value === "string" ? value.split(",") : value;
  if (ids.length > 32 || ids.join(",").length > 2048) return null;
  const trimmed = ids.map((id) => id.trim()).filter(Boolean);
  if (trimmed.some((id) => !/^[a-zA-Z0-9_-]{1,64}$/.test(id))) return null;
  return [...new Set(trimmed)].sort();
}
export const sourcesKey = (sources: SourceSelection) => sources?.join(",") ?? null;
export function validDate(value: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const date = new Date(`${value}T00:00:00Z`);
  return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === value;
}
export function normalizeDays(value: unknown): TimelineDays {
  return value === 7 || value === "7" ? 7 : value === 90 || value === "90" ? 90 : 30;
}
export function filterParams(category: Category, query: string, sources: SourceSelection): URLSearchParams {
  const params = new URLSearchParams();
  if (category !== "全部资讯") params.set("category", category);
  if (query) params.set("q", query);
  if (sources !== null) params.set("sources", sources.join(","));
  return params;
}
