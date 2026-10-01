# AI Signal frontend

The Vue 3/TypeScript reader preserves the original AI Signal layout while separating data access, source models, preferences, and presentation components. Production requests use `/api/news`; test fixtures exist only in `tests/`.

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

Vitest, Vue Test Utils, and jsdom test every runtime source module, including the real entrypoint. V8 coverage includes **all** `src/**/*.ts` and `src/**/*.vue` without app-source exclusions. The local aggregate minimum is 80% for lines, statements, functions, and branches. CI separately checks changed frontend lines at 90%. Coverage output is available in `coverage/coverage-summary.json`, `coverage/coverage-final.json`, and `coverage/lcov.info`. Type-only declarations have no executable lines; CSS/layout verification requires the browser visitor audit.

## Reader behavior

- The original response model (`articles`, `sources`, `run`, `schedule`) remains supported
- Optional `stats` provides truthful whole-library totals; pagination totals are the next fallback
- Optional `pagination` supports `offset`/`limit` and a Load more control; overlapping pages are deduplicated by article ID
- Categories filter loaded articles, with an explicit loaded-count notice while additional pages remain
- Refresh failures retain the last successful response; requests time out after 15 seconds and are canceled when the reader unmounts
- Interface language is Chinese or English; titles and summaries remain in their source language
- Light, dark, and system appearance preferences are stored locally, with a session-only fallback when storage is unavailable
- Category filters are native toggle buttons; navigation reports its active page, timestamps carry machine-readable dates, and external links identify new-tab behavior
- Reduced-motion preferences disable animation and transitions

Unit tests do not claim browser layout or visual-regression coverage. The public visitor audit remains a separate deployment-stage check.
