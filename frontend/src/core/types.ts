export type Locale = "zh-CN" | "en";
export type Theme = "light" | "dark" | "system";
export type View = "feed" | "sources" | "timeline";
// null includes every current and future source; [] intentionally matches nothing.
export type SourceSelection = string[] | null;
export type TimelineDays = 7 | 30 | 90;
export interface TimelineData {
  buckets: { date: string; count: number }[];
  total: number;
  timezone: "UTC";
  days: TimelineDays;
}
export const categories = [
  "全部资讯",
  "模型进展",
  "研究前沿",
  "开发工具",
  "产业动态",
  "安全治理",
  "具身智能",
] as const;
export type Category = (typeof categories)[number];
export interface Article {
  id: string;
  title: string;
  url: string;
  source: string;
  category: string;
  summary: string;
  published_at: string;
}
export interface Source {
  id: string;
  name: string;
  home: string;
}
export interface SourceStatus {
  source: string;
  status: "ok" | "error";
  items?: number;
  checked_at: string;
  error?: string;
}
export interface IngestionRun {
  status: "success" | "partial" | "failed" | "running";
  finished_at?: string | null;
  details: SourceStatus[];
}
export interface Pagination {
  total: number;
  offset: number;
  limit: number;
  has_more: boolean;
}
export interface NewsStats {
  total_articles: number;
  recent_articles: number;
  total_sources: number;
}
export interface NewsData {
  articles: Article[];
  sources: Source[];
  run: IngestionRun | null;
  schedule: string;
  pagination?: Pagination;
  stats?: NewsStats;
}
