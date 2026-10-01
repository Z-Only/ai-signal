# AI Signal frontend

The Vue 3/TypeScript reader preserves the original AI Signal layout while separating data access, source models, preferences, and presentation components. Production requests use `/api/news` and `/api/timeline`; test fixtures exist only in `tests/`.

## Development

Use Bun 1.4.2 and Node 24 (Node runs Vitest/V8 coverage).

```sh
bun install --frozen-lockfile
bun run dev
```

Vite proxies `/api` to the Rust server on `127.0.0.1:8787`. `bun run build` emits the static application in `dist/` for the native server.

## Checks

```sh
bun run typecheck
bun run test:coverage
bun run build
```

Vitest, Vue Test Utils, and jsdom test every runtime source module, including the real entrypoint. V8 coverage includes **all** `src/**/*.ts` and `src/**/*.vue` without app-source exclusions. The local line minimum is 95%; statements, functions, and branches retain their additional 80% minimum. CI separately requires 95% combined production lines and 95% changed executable lines. Coverage output is available in `coverage/coverage-summary.json`, `coverage/coverage-final.json`, and `coverage/lcov.info`. Type-only declarations have no executable lines; CSS/layout verification requires the browser visitor audit.

## Reader behavior

- The original response model (`articles`, `sources`, `run`, `schedule`) remains supported
- Optional `stats` provides truthful whole-library totals; pagination totals are the next fallback
- Optional `pagination` supports `offset`/`limit` and a Load more control; overlapping pages are deduplicated by article ID
- Categories and submitted searches request the complete corpus from the backend using `category` and `q`; filtered pagination totals drive the result count, while collection statistics remain global
- Search matches literal title/summary substrings, trims whitespace, and accepts up to 200 Unicode characters; a clear control removes only the search
- Changing category or submitting a new search resets pagination, clears stale articles immediately, and aborts the prior request; late responses cannot replace current results
- Repeated refresh/load-more clicks are ignored while pending; failed load-more retries preserve and extend the current results
- Refresh failures retain the last successful response; requests time out after 15 seconds and are canceled when the reader unmounts
- View, category, submitted search, source selection, UTC day and timeline range are reflected in the URL and restored on reload or browser Back/Forward; unrelated parameters and fragments are retained, and invalid reader parameters are normalized
- History remembers up to 200 loaded articles for each entry; Back/Forward restores that entry’s bounded range even when its filters match the current page, aborting obsolete appends. Restoration uses one bounded request, without claiming scroll-position restoration or replaying unlimited pages
- Sources are native labeled checkboxes populated exclusively from the API catalog, with Select all / Clear selection controls. A labeled native disclosure keeps an expanded catalog compact on phones without hiding the selection summary or reset actions. The default (omitted `sources`) includes all present and future sources; `sources=` explicitly selects none. An explicit source subset is sorted and deduplicated, with 32-entry/2,048-character bounds and bounded ASCII IDs
- The feed sends `sources` and a validated `date=YYYY-MM-DD` to `/api/news` alongside category/search. Source or date changes reset pagination and abort obsolete requests; history ranges are bound to all feed filters
- The Daily timeline view lazily requests `/api/timeline` only while visible, using the current category/search/source selection and `days=7|30|90`. The selected feed date never narrows timeline aggregation. Range changes and navigation abort stale requests; counts are not fabricated during pending requests or errors
- The timeline validates ascending, consecutive zero-filled UTC day buckets and exact totals. Its compact decorative chart is paired with native day buttons exposing full dates and counts for keyboard, touch and screen-reader use. Opening a day restores the feed with the same filters; Show all dates clears only that date
- Clear source-selection controls remain available when the catalog or an unknown-source deep link fails, so Select all can recover without a loaded vendor list. Previously loaded catalog metadata survives filter errors
- Daily calendar counts are explicitly labeled UTC; article timestamps still use the browser time zone. The existing Last 24 hours collection statistic remains a rolling count
- Source status labels say official announcements because the collection catalog can include RSS/Atom, JSON, official HTML and release-note feeds
- Interface language is Chinese or English, with singular/plural English count labels; titles and summaries remain in their source language
- Light, dark, and system appearance preferences are stored locally, with a session-only fallback when storage is unavailable
- Category filters are native toggle buttons; navigation reports its active page, timestamps carry machine-readable dates, and external links identify new-tab behavior
- Reduced-motion preferences disable animation and transitions

Unit tests do not claim browser layout or visual-regression coverage. The public visitor audit remains a separate deployment-stage check.
