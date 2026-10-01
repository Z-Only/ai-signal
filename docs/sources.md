# Official source catalog

The registry in `crates/news-core/src/lib.rs` is the source of truth for native and Sites ingestion. Public responses expose only the source ID, name and homepage. The server-only WASM manifest supplies fixed endpoints, methods, request bodies, language, byte limits and timeouts; visitors cannot submit URLs or modify these settings.

Verified on 2026-10-01. This is a qualitative selection of influential model, product and developer ecosystems, not a vendor ranking. Public APIs and page structures can change; an individual source failure remains visible without deleting saved articles.

| Source | Official channel | Format / publication date |
| --- | --- | --- |
| OpenAI | https://openai.com/news/ | Official RSS, publisher timestamp |
| Google DeepMind | https://deepmind.google/blog/ | Official RSS, publisher timestamp |
| Google AI | https://blog.google/innovation-and-ai/technology/ai/ | Official RSS, publisher timestamp |
| NVIDIA | https://blogs.nvidia.com/ | Official RSS with AI-topic filtering |
| Hugging Face | https://huggingface.co/blog | Official RSS, publisher timestamp |
| Meta AI | https://about.fb.com/news/tag/ai/ | AI-tag RSS advertised by the official archive |
| Microsoft AI | https://blogs.microsoft.com/blog/tag/ai/ | AI-tag RSS advertised by the official archive |
| Mistral AI | https://mistral.ai/news | Official RSS; valid XML despite text/plain MIME |
| Anthropic | https://www.anthropic.com/news | Server-rendered dated announcement cards |
| DeepSeek | https://api-docs.deepseek.com/updates/ | Dated official changelog sections with distinct news links |
| Moonshot AI / Kimi | https://www.kimi.com/en/blog/ | Server-rendered dated blog cards |
| ByteDance Seed | https://seed.bytedance.com/en/blog/ | Public first-party announcement API; PublishDate milliseconds |
| Tencent Hunyuan | https://hunyuan.tencent.com/research | Public first-party read-only POST; displayPublishTime then publishedAt; creation-only rows are skipped |
| Alibaba Qwen | https://qwen.ai/blog | Public first-party article retrieval API; extra.date timestamp |
| Z.ai Release Notes | https://docs.z.ai/release-notes/new-released | Official dated MDX updates; documentation announcement date |

## Interpretation and bounds

- Date-only announcements use the publisher's calendar date at UTC midnight, rather than pretending an exact publication time was supplied. Article timestamps are normalized to UTC and displayed in the browser's local zone; timeline buckets explicitly use UTC days.
- Z.ai release-note dates are not necessarily the original model-launch/blog dates. Only its verified `#YYYY-MM-DD` section fragments are preserved for distinct entry identities; general URL fragment removal remains unchanged.
- No date is replaced with the ingestion time to make an old or undated story appear new.
- Each source contributes at most 35 valid articles per run. Listing/API adapters collect a bounded current index, not an exhaustive archive. ByteDance's configured first page requests 20 records; pinned/hidden-language entries can reduce the visible count. Qwen entries are sorted by official dates before capping.
- Qwen's current response is approximately 4.5 MB with full article HTML, so its explicit limit is 6 MB and 30 seconds. Other sources retain 3 MB and 18 seconds. Request/stream/parser limits all remain bounded.
- Tencent uses a fixed read-only list request and Chinese locale for broader coverage. Internal usernames and other non-publicly displayed metadata are not retained.
- Timeline counts describe this site's retained official-source corpus. They are not comprehensive measures of AI industry activity or vendor influence.
- xAI and Cohere were investigated but are not enabled in this release: server access or a bounded index with stable publication dates was not sufficiently established. No third-party substitute feed is presented as an official feed.

## Acceptance

Tests use small synthetic structural fixtures, including malformed data and transport limits. Compiled Rust/WASM parsers were also checked against official public response bodies in the development environment. Deployment acceptance must still verify the actual Sites runtime: network access at the edge can differ from development access. Keep source errors visible and prior records intact if a provider changes its interface.
