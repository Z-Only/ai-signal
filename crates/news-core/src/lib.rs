//! Deterministic, network-free news normalization for native and WebAssembly runtimes.
//!
//! Callers supply a clock and perform HTTP/database work themselves. Test feeds are
//! synthetic fixtures: they do not claim that a publisher or the network is healthy.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use quick_xml::{events::Event, Reader};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt, sync::LazyLock};
use url::Url;

pub const MAX_FEED_BYTES: usize = 3_000_000;
pub const SOURCE_ITEM_LIMIT: usize = 35;
pub const MAX_SUMMARY_CHARS: usize = 420;
const MAX_XML_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Source {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub home: &'static str,
}

pub const SOURCES: [Source; 5] = [
    Source {
        id: "openai",
        name: "OpenAI",
        url: "https://openai.com/news/rss.xml",
        home: "https://openai.com/news/",
    },
    Source {
        id: "deepmind",
        name: "Google DeepMind",
        url: "https://deepmind.google/blog/rss.xml",
        home: "https://deepmind.google/blog/",
    },
    Source {
        id: "google",
        name: "Google AI",
        url: "https://blog.google/innovation-and-ai/technology/ai/rss/",
        home: "https://blog.google/innovation-and-ai/technology/ai/",
    },
    Source {
        id: "nvidia",
        name: "NVIDIA",
        url: "https://blogs.nvidia.com/feed/",
        home: "https://blogs.nvidia.com/",
    },
    Source {
        id: "huggingface",
        name: "Hugging Face",
        url: "https://huggingface.co/blog/feed.xml",
        home: "https://huggingface.co/blog",
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicSource {
    pub id: &'static str,
    pub name: &'static str,
    pub home: &'static str,
}

pub fn public_sources() -> Vec<PublicSource> {
    SOURCES
        .iter()
        .map(|s| PublicSource {
            id: s.id,
            name: s.name,
            home: s.home,
        })
        .collect()
}

pub fn source_by_id(id: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|source| source.id == id)
}

/// The JSON/database contract used by the existing Vue frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Article {
    pub id: String,
    pub title: String,
    pub url: String,
    pub source: String,
    pub category: String,
    pub summary: String,
    pub published_at: String,
    pub fetched_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    TooLarge,
    InvalidXml(String),
    InvalidFormat,
    UnsupportedDoctype,
    TooDeep,
}

impl fmt::Display for FeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("Feed exceeds 3 MB limit"),
            Self::InvalidXml(message) => write!(f, "Invalid XML: {message}"),
            Self::InvalidFormat => f.write_str("Invalid RSS format"),
            Self::UnsupportedDoctype => f.write_str("RSS document types are not allowed"),
            Self::TooDeep => f.write_str("RSS exceeds XML nesting limit"),
        }
    }
}

impl std::error::Error for FeedError {}

static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());
static CATEGORY_RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"(?i)robot|robotic|physical ai", "具身智能"),
        (
            r"(?i)research|paper|science|reasoning|benchmark|study",
            "研究前沿",
        ),
        (
            r"(?i)open.source|hugging|developer|api|agent|coding|code|tool",
            "开发工具",
        ),
        (r"(?i)policy|safety|responsib|security|govern", "安全治理"),
        (r"(?i)model|gpt|gemini|claude|llm|llama", "模型进展"),
    ]
    .into_iter()
    .map(|(pattern, category)| (Regex::new(pattern).unwrap(), category))
    .collect()
});
static AI_KEYWORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bai\b|artificial intelligence|machine learning|agent|model|robot|llm|inference|training").unwrap()
});

/// Decode HTML entities, remove markup, and collapse Unicode whitespace.
/// This returns plain text; consumers must still render it as text, never HTML.
pub fn clean_text(input: &str) -> String {
    let decoded = html_escape::decode_html_entities(input);
    HTML_TAG
        .replace_all(&decoded, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Limit by Unicode scalar values instead of slicing bytes or UTF-16 surrogates.
pub fn clean_excerpt(input: &str) -> String {
    clean_text(input).chars().take(MAX_SUMMARY_CHARS).collect()
}

/// Preserve the original category precedence when multiple keywords match.
pub fn classify_category(text: &str) -> &'static str {
    CATEGORY_RULES
        .iter()
        .find(|(pattern, _)| pattern.is_match(text))
        .map_or("产业动态", |(_, category)| *category)
}

/// HTTPS only. Strip fragments and all lowercase `utm_` parameters, preserving
/// other query segments byte-for-byte, including their order and duplicates.
/// Credential-bearing article links are rejected.
pub fn canonicalize_url(input: &str) -> Option<String> {
    let mut url = Url::parse(input.trim()).ok()?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);
    if let Some(query) = url.query() {
        let retained: Vec<&str> = query
            .split('&')
            .filter(|segment| {
                let key = segment.split_once('=').map_or(*segment, |(key, _)| key);
                !url::form_urlencoded::parse(key.as_bytes())
                    .next()
                    .is_some_and(|(decoded_key, _)| decoded_key.starts_with("utm_"))
            })
            .collect();
        // Re-encoding retained pairs would change %20 to +, escape /:~, and
        // append = to bare flags, changing article IDs only on tracked links.
        let query = (!retained.is_empty()).then(|| retained.join("&"));
        url.set_query(query.as_deref());
    }
    Some(url.into())
}

pub fn iso_timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn published_date(input: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let input = input.trim();
    let date = DateTime::parse_from_rfc2822(input)
        .or_else(|_| DateTime::parse_from_rfc3339(input))
        .ok()?
        .with_timezone(&Utc);
    // Avoid an overflowing `now + 24h` for extreme caller-supplied clocks.
    if date.signed_duration_since(now) > Duration::days(1) {
        None
    } else {
        Some(date)
    }
}

#[derive(Default)]
struct RawItem {
    title: String,
    link: String,
    pub_date: Option<String>,
    dc_date: Option<String>,
    description: Option<String>,
    content: Option<String>,
}

impl RawItem {
    fn field_mut(&mut self, name: &[u8]) -> Option<&mut String> {
        match name {
            b"title" => Some(&mut self.title),
            b"link" => Some(&mut self.link),
            b"pubDate" => Some(self.pub_date.get_or_insert_with(String::new)),
            b"dc:date" => Some(self.dc_date.get_or_insert_with(String::new)),
            b"description" => Some(self.description.get_or_insert_with(String::new)),
            b"content:encoded" => Some(self.content.get_or_insert_with(String::new)),
            _ => None,
        }
    }
}

fn append_field(item: &mut Option<RawItem>, path: &[Vec<u8>], value: &str) {
    if let (Some(item), Some(name)) = (item, path.get(3)) {
        if let Some(field) = item.field_mut(name) {
            field.push_str(value);
        }
    }
}

fn parse_rss(xml: &str) -> Result<Vec<RawItem>, FeedError> {
    if xml.len() > MAX_FEED_BYTES {
        return Err(FeedError::TooLarge);
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    let mut path: Vec<Vec<u8>> = Vec::new();
    let mut seen_root = false;
    let mut seen_channel = false;
    let mut item_count = 0;
    let mut item = None;
    let mut items = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|error| FeedError::InvalidXml(error.to_string()))?
        {
            Event::Start(element) => {
                // The event reader validates tags; attributes require iteration.
                for attribute in element.attributes() {
                    attribute.map_err(|error| FeedError::InvalidXml(error.to_string()))?;
                }
                let name = element.name().as_ref().to_vec();
                if path.is_empty() {
                    if seen_root || name != b"rss" {
                        return Err(FeedError::InvalidFormat);
                    }
                    seen_root = true;
                }
                path.push(name);
                if path.len() > MAX_XML_DEPTH {
                    return Err(FeedError::TooDeep);
                }
                if path.len() == 2 && path[1] == b"channel" {
                    seen_channel = true;
                }
                if path.len() == 3 && path[1] == b"channel" && path[2] == b"item" {
                    item_count += 1;
                    if item_count <= SOURCE_ITEM_LIMIT {
                        item = Some(RawItem::default());
                    }
                }
                // Creates explicit empty fields and separates nested HTML elements.
                append_field(&mut item, &path, if path.len() > 4 { " " } else { "" });
            }
            Event::End(_) => {
                if path.len() == 3 {
                    if let Some(raw) = item.take() {
                        items.push(raw);
                    }
                }
                if path.len() > 4 {
                    append_field(&mut item, &path, " ");
                }
                path.pop();
            }
            Event::Text(value) => {
                let value = String::from_utf8_lossy(value.as_ref());
                if path.is_empty() && !value.trim().is_empty() {
                    return Err(FeedError::InvalidFormat);
                }
                append_field(&mut item, &path, &value);
            }
            Event::CData(value) => {
                if path.is_empty() {
                    return Err(FeedError::InvalidFormat);
                }
                append_field(&mut item, &path, &String::from_utf8_lossy(value.as_ref()));
            }
            Event::GeneralRef(value) => {
                if path.is_empty() {
                    return Err(FeedError::InvalidFormat);
                }
                let reference = format!("&{};", String::from_utf8_lossy(value.as_ref()));
                append_field(
                    &mut item,
                    &path,
                    &html_escape::decode_html_entities(&reference),
                );
            }
            Event::DocType(_) => return Err(FeedError::UnsupportedDoctype),
            Event::Eof => break,
            _ => {}
        }
    }
    if !seen_root || !seen_channel || !path.is_empty() {
        return Err(FeedError::InvalidFormat);
    }
    Ok(items)
}

/// Parse at most the first 35 RSS items, validate and normalize each article,
/// then deduplicate canonical URLs within the feed (first valid item wins).
///
/// Invalid individual items are skipped. Invalid XML rejects the entire feed,
/// so a truncated response cannot silently cause a partially ingested feed.
pub fn normalize_feed(
    source: &Source,
    xml: &str,
    now: DateTime<Utc>,
) -> Result<Vec<Article>, FeedError> {
    let raw_items = parse_rss(xml)?;
    let mut seen = BTreeSet::new();
    let mut articles = Vec::new();
    for raw in raw_items {
        let title = clean_text(&raw.title);
        let Some(url) = canonicalize_url(&clean_text(&raw.link)) else {
            continue;
        };
        let Some(published) = raw
            .pub_date
            .or(raw.dc_date)
            .and_then(|date| published_date(&date, now))
        else {
            continue;
        };
        if title.is_empty() {
            continue;
        }
        let summary = clean_excerpt(&raw.description.or(raw.content).unwrap_or_default());
        let combined = format!("{title} {summary}");
        if source.id == "nvidia" && !AI_KEYWORDS.is_match(&combined) {
            continue;
        }
        if !seen.insert(url.clone()) {
            continue;
        }
        articles.push(Article {
            id: format!("{:x}", Sha256::digest(url.as_bytes())),
            title,
            url,
            source: source.id.to_string(),
            category: classify_category(&combined).to_string(),
            summary,
            published_at: iso_timestamp(published),
            fetched_at: iso_timestamp(now),
        });
    }
    Ok(articles)
}

/// String-only bridge for a thin JavaScript host. The same implementation is
/// callable from native Rust, allowing unit tests to exercise all validation.
/// Returns `{ "ok": true, "articles": [...] }` or `{ "ok": false, "error": "..." }`.
#[cfg_attr(feature = "wasm", wasm_bindgen::prelude::wasm_bindgen)]
pub fn normalize_feed_json(source_id: &str, xml: &str, now_iso: &str) -> String {
    let result = (|| {
        let source = source_by_id(source_id).ok_or_else(|| "Unknown news source".to_string())?;
        let now = DateTime::parse_from_rfc3339(now_iso)
            .map_err(|_| "Invalid current timestamp: expected RFC 3339".to_string())?
            .with_timezone(&Utc);
        normalize_feed(source, xml, now).map_err(|error| error.to_string())
    })();
    match result {
        Ok(articles) => serde_json::json!({ "ok": true, "articles": articles }).to_string(),
        Err(error) => serde_json::json!({ "ok": false, "error": error }).to_string(),
    }
}

/// Public source metadata as a JSON array; internal feed URLs are omitted.
#[cfg_attr(feature = "wasm", wasm_bindgen::prelude::wasm_bindgen)]
pub fn public_sources_json() -> String {
    serde_json::json!(public_sources()).to_string()
}

#[cfg(test)]
mod tests;
