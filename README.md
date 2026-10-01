# AI Signal / AI 信号

A responsive official-source AI news reader with Vue 3 and a Rust backend. Headlines and excerpts retain the source language; interface text supports Chinese and English.

## Architecture

- `frontend/`: Vue 3, Vite, TypeScript, Bun; modular components, accessible categories, light/dark/system themes, resilient pagination.
- `crates/news-core/`: shared pure Rust RSS normalization, validation, topic classification, canonical URLs and deduplication. Compiles natively and to WebAssembly.
- `crates/news-server/`: native Rust Axum/Tokio/reqwest/SQLite backend, hourly scheduler and static-file serving.
- `sites/`: thin TypeScript HTTP/D1 adapter for ChatGPT Sites. Content parsing and normalization execute the same compiled Rust core; this deployment is not a native Axum process.
- `drizzle/`: append-only Sites D1 schema migrations preserving the original database structure.
- `scripts/`: fail-closed coverage checks and regression tests.

## Local development

Install the Rust toolchain from `rust-toolchain.toml`, Node.js 24 and Bun 1.4.2.

```sh
bun install --cwd frontend --frozen-lockfile
bun run --cwd frontend build
# Set ADMIN_TOKEN through your preferred local secrets manager.
STATIC_DIR=frontend/dist cargo run -p ai-news-server
```

See `crates/news-server/README.md` for backend environment variables. Never commit real credentials. `GET /api/news` is public. Administrative refresh requests require a bearer credential; unset credentials deny writes. Native startup/hourly refresh runs independently of visitors.

Run the frontend development server with `bun run --cwd frontend dev`; its `/api` proxy targets the backend. Configure matching ports as documented by each component.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
bun run --cwd frontend typecheck
bun run --cwd frontend test:coverage
bun run --cwd sites typecheck
bun run --cwd sites test:coverage
python3 -m unittest discover -s scripts -p 'test_*.py'
```

CI measures all handwritten application Rust, Vue and TypeScript source. Combined executable-line coverage must be at least 80%, and changed executable-line coverage at least 90%. Missing reports fail the gate. Generated WASM/bindings are built and smoke-tested, rather than treated as authored source. Tests use deterministic mock feeds and SQLite, not external news availability.

## Sites deployment

Build the frontend and generate the Rust bindings:

```sh
cargo build --locked --release -p ai-news-core --target wasm32-unknown-unknown --features wasm
cargo install wasm-bindgen-cli --version 0.2.129 --locked
wasm-bindgen target/wasm32-unknown-unknown/release/ai_news_core.wasm --target web --out-dir sites/generated --out-name ai_news_core
node crates/news-core/tests/wasm-smoke.mjs sites/generated
bun install --cwd sites --frozen-lockfile
bun run --cwd sites build
```

The output is `dist/server/index.js` plus a statically imported WASM module and migration metadata. The deployment environment must support WASM Worker modules and the `DB` D1 binding. Production Sites runtime verification is required; successful local packaging alone is not proof of hosting compatibility.

The Sites adapter uses `REFRESH_TOKEN_SHA256`, a SHA-256 digest of a separately authorized service bearer token. Never put the token in browser code, public repository files or logs. Public reading is separate from authenticated collection. Do not publish a deployment whose writer authentication is unconfigured. The previous owner-private release must remain in place until the new release passes its gates.

Sites does not acquire a Cron trigger merely from this source tree. Its unattended updater must be configured and verified separately; content updates write D1 without republishing. The native Rust deployment has its own hourly Tokio scheduler.

## Contribution and release rules

All changes go through pull requests, with self-review before publication. `main` must require `ci-gate`, up-to-date branches, and no bypass of failed checks or direct pushes. Enable built-in auto-merge only after review. `.github/REPOSITORY_GATES.md` describes settings that must be configured in GitHub; a file in this repository does not enable protection by itself.

Deploy only a verified merged commit. Preserve database migration history and current data. Never publish from an unmerged working directory. Post-deployment visitor tests should exercise keyboard access, filters, language, themes, pagination and responsive layouts before the next improvement PR.

## Sources and limitations

Official feeds: OpenAI, Google DeepMind, Google AI, NVIDIA (AI-topic filter), and Hugging Face's blog (including partner posts). Excerpts are cleaned and truncated originals, not AI-generated translations or fact-checks. Content rights remain with the original publishers. URL deduplication is not semantic event clustering.

## License

MIT. Third-party libraries retain their respective licenses.
