# AI news core

Pure Rust normalization shared by the native HTTP API and the Cloudflare Worker.
This crate does not access the network, storage, environment, random number
sources, or the system clock. The runtime supplies `DateTime<Utc>` explicitly.

```rust
use ai_news_core::{normalize_feed, source_by_id};
use chrono::DateTime;

let now = DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")?
    .with_timezone(&chrono::Utc);
let source = source_by_id("openai").expect("known source");
let articles = normalize_feed(source, "<rss><channel/></rss>", now)?;
assert!(articles.is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Compatibility and validation

- Keeps the existing five source IDs, names, feed URLs, and home URLs
- Keeps the article JSON keys and Chinese category precedence
- Considers the first 35 RSS items per source, before validation/filtering
- Accepts RFC 2822 RSS dates and RFC 3339 timestamps, including UTC offsets;
  intentionally rejects ambiguous/free-form JavaScript date strings
- Rejects dates more than 24 hours ahead of the supplied clock
- Keeps only absolute HTTPS links; removes fragments and lowercase `utm_`
  parameters. Other parameter order, duplicates, and case remain significant
- Uses the canonical URL's lowercase hexadecimal SHA-256 as the article ID
- Applies the original NVIDIA AI-keyword filter to title and bounded summary
- Uses description before `content:encoded`, and `pubDate` before `dc:date`,
  including when the preferred element is explicitly empty
- Formats dates in UTC with three fractional-second digits

Intentional hardening: reject URL credentials, malformed XML/attributes, DTDs,
inputs larger than 3,000,000 bytes, and nesting deeper than 64 elements. Parse
errors reject the whole feed; individually invalid articles are skipped. Empty
RSS channels are valid empty results. DTDs are never loaded or expanded.

Intentional text improvements: decode XML references and HTML entities into
plain text, preserve word spacing around nested markup, and truncate summaries
to 420 Unicode scalar values without splitting UTF-8 or surrogate pairs. A
multi-scalar grapheme (such as a combined emoji) can still be split at the cap.
The frontend must render returned strings as text, never as trusted HTML.

Canonical duplicate URLs within one feed keep the first valid item. The runtime
is responsible for deduplication/upsert across sources and ingestion runs.

## Verification

From the repository root:

```sh
cargo test -p ai-news-core
cargo clippy -p ai-news-core --all-targets -- -D warnings
cargo check -p ai-news-core --target wasm32-unknown-unknown
cargo llvm-cov -p ai-news-core --lib --fail-under-lines 90
```

The unit fixtures are synthetic and deterministic. They test parser behavior;
they are not evidence that any live feed, network, database, or deployed Worker
is working. WebAssembly compilation is likewise distinct from deployment.

## JavaScript / WASM bridge

The `wasm` feature enables `wasm-bindgen` exports. Both exports are ordinary Rust
functions even when that feature is off, and all input validation is unit-tested
natively. There is no target-specific parser or business-logic implementation.

- `normalize_feed_json(source_id, xml, now_iso)` returns a JSON string containing
  `{ "ok": true, "articles": [...] }` or `{ "ok": false, "error": "..." }`.
  The clock must be RFC 3339. Only the five known source IDs are accepted.
- `public_sources_json()` returns the public metadata JSON array directly,
  containing `id`, `name`, and `home`, without the internal feed URLs.

Build the WASM binary from the workspace root:

```sh
cargo build -p ai-news-core --release --target wasm32-unknown-unknown --features wasm
wasm-bindgen target/wasm32-unknown-unknown/release/ai_news_core.wasm \
  --target web --out-dir target/news-core-web --out-name ai_news_core
```

Install a `wasm-bindgen-cli` version matching the `wasm-bindgen` version locked in
Cargo.lock. The CLI emits JavaScript bindings, TypeScript declarations, and the
processed `ai_news_core_bg.wasm`. A Workers adapter should statically import the
WASM module and initialize the generated bindings from that compiled module;
do not fetch WASM bytes and try to compile them inside the Worker runtime.
Generated files and runtime-specific initialization belong to the host build.

To exercise the bridge with the export feature enabled:

```sh
cargo test -p ai-news-core --lib --features wasm
cargo clippy -p ai-news-core --all-targets --all-features -- -D warnings
cargo llvm-cov -p ai-news-core --lib --all-features \
  --ignore-filename-regex '/tests\.rs$' --fail-under-lines 90
```

The generated binary/glue can also be exercised locally without a browser or
network. This verifies actual WASM execution, separately from native unit tests:

```sh
node crates/news-core/tests/wasm-smoke.mjs target/news-core-web
```
