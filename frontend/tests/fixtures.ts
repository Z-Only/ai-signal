import type { Article, NewsData } from "../src/core/types";
export const article = (
  id = "1",
  overrides: Partial<Article> = {},
): Article => ({
  id,
  title: `Official article ${id}`,
  url: `https://example.com/news/${id}`,
  source: "openai",
  category: "模型进展",
  summary: `Source summary ${id}`,
  published_at: "2026-10-01T05:00:00Z",
  ...overrides,
});
export const news = (overrides: Partial<NewsData> = {}): NewsData => ({
  articles: [
    article(),
    article("2", { source: "nvidia", category: "开发工具" }),
    article("3", { source: "unknown", summary: "", category: "研究前沿" }),
  ],
  sources: [
    { id: "openai", name: "OpenAI", home: "https://openai.com/news/" },
    { id: "nvidia", name: "NVIDIA", home: "https://blogs.nvidia.com/" },
    { id: "google", name: "Google AI", home: "https://blog.google/" },
  ],
  schedule: "每小时自动更新",
  run: {
    status: "partial",
    finished_at: "2026-10-01T05:10:00Z",
    details: [
      {
        source: "openai",
        status: "ok",
        items: 12,
        checked_at: "2026-10-01T05:00:00Z",
      },
      {
        source: "nvidia",
        status: "error",
        checked_at: "2026-10-01T05:00:00Z",
        error: "Upstream unavailable",
      },
    ],
  },
  ...overrides,
});
export const respond = (body: unknown, ok = true) =>
  ({ ok, status: ok ? 200 : 503, json: async () => body }) as Response;
