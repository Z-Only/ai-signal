// Exercises the compiled artifact and generated JS glue; no network or services.
// Build instructions: see ../README.md. Optional argument: bindings directory.
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const directory = path.resolve(process.argv[2] ?? 'target/news-core-web')
const { initSync, normalize_feed_json, public_sources_json } = await import(
  pathToFileURL(path.join(directory, 'ai_news_core.js')).href
)
// Local Node can compile bytes. In Workers, the host must supply an already
// compiled module from a static .wasm import instead.
const module = new WebAssembly.Module(
  fs.readFileSync(path.join(directory, 'ai_news_core_bg.wasm')),
)
initSync({ module })
const now = '2026-10-01T00:00:00Z'
const normalize = (source, xml, date = now) => JSON.parse(normalize_feed_json(source, xml, date))
const xml = '<rss><channel><item><title>AI robot &amp; news 🤖</title><link>https://example.com/x?utm_source=rss#part</link><pubDate>2026-09-30T00:00:00Z</pubDate><description>&lt;p&gt;New model&lt;/p&gt;</description></item></channel></rss>'
const sources = JSON.parse(public_sources_json())
assert.equal(sources.length, 5)
assert.equal(sources[0].id, 'openai')
assert.equal('url' in sources[0], false)

const result = normalize('openai', xml)
assert.equal(result.ok, true)
assert.equal(result.articles.length, 1)
assert.equal(result.articles[0].url, 'https://example.com/x')
assert.equal(result.articles[0].title, 'AI robot & news 🤖')
assert.equal(result.articles[0].category, '具身智能')
assert.equal(result.articles[0].summary, 'New model')
assert.equal(result.articles[0].id.length, 64)
assert.equal(result.articles[0].fetched_at, '2026-10-01T00:00:00.000Z')
assert.deepEqual(normalize('openai', '<rss><channel/></rss>'), { ok: true, articles: [] })
assert.deepEqual(normalize('missing', xml), { ok: false, error: 'Unknown news source' })
assert.deepEqual(normalize('openai', xml, 'bad date'), {
  ok: false,
  error: 'Invalid current timestamp: expected RFC 3339',
})
assert.deepEqual(normalize('openai', 'bad xml'), { ok: false, error: 'Invalid RSS format' })

// Force larger allocations and verify subsequent calls still read WASM memory.
const longXml = xml.replace('New model', '🤖'.repeat(100_000))
assert.equal(normalize('openai', longXml).articles[0].summary, '🤖'.repeat(420))
assert.deepEqual(JSON.parse(public_sources_json()), sources)
assert.deepEqual(normalize('openai', xml), result)
console.log('PASS: generated WASM exports, JSON validation, Unicode normalization, and memory growth')
