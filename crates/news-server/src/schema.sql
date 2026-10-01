CREATE TABLE IF NOT EXISTS articles (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    url TEXT NOT NULL,
    source TEXT NOT NULL,
    category TEXT NOT NULL,
    summary TEXT NOT NULL,
    published_at TEXT NOT NULL,
    fetched_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS articles_url_unique ON articles(url);
CREATE INDEX IF NOT EXISTS articles_published_idx ON articles(published_at);
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL,
    added INTEGER DEFAULT 0 NOT NULL,
    details TEXT DEFAULT '[]' NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
