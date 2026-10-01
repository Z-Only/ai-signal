//! Single-response adapters for verified publisher indexes. No network or clock access.

use super::{
    canonicalize_url, clean_text, iso_timestamp, FeedError, RawItem, Source, SOURCE_ITEM_LIMIT,
};
use chrono::{DateTime, NaiveDate};
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;
use url::Url;

pub(super) fn is_indexed_source(id: &str) -> bool {
    matches!(
        id,
        "anthropic" | "deepseek" | "kimi" | "bytedance" | "tencent" | "qwen" | "glm"
    )
}

pub(super) fn parse(source: &Source, body: &str) -> Result<Vec<RawItem>, FeedError> {
    match source.id {
        "bytedance" => seed(body),
        "tencent" => hunyuan(body),
        "qwen" => qwen(body),
        _ => {
            let document = Html::parse_document(body);
            // Bound DOM traversal as well as input bytes. HTML is intentionally parsed
            // as HTML5, not coerced into XML or parsed with regular expressions.
            if document
                .tree
                .nodes()
                .any(|node| node.ancestors().take(66).count() > 64)
            {
                return Err(FeedError::InvalidSourceData);
            }
            match source.id {
                "anthropic" => Ok(anthropic(&document, source)),
                "deepseek" => Ok(deepseek(&document, source)),
                "kimi" => Ok(kimi(&document, source)),
                "glm" => glm(&document, source),
                _ => Err(FeedError::InvalidSourceData),
            }
        }
    }
}

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("static publisher selector is valid")
}

fn text(element: ElementRef<'_>) -> String {
    clean_text(&element.text().collect::<Vec<_>>().join(" "))
}

fn child_text(element: ElementRef<'_>, css: &str) -> String {
    element
        .select(&selector(css))
        .next()
        .map(text)
        .unwrap_or_default()
}

/// Dates published without a time retain their day at 00:00 UTC. This is a
/// representation of the publisher's calendar date, never the fetch timestamp.
fn calendar_date(value: &str) -> Option<String> {
    ["%Y-%m-%d", "%b %d, %Y", "%B %d, %Y"]
        .iter()
        .find_map(|format| NaiveDate::parse_from_str(value.trim(), format).ok())
        .map(|date| iso_timestamp(date.and_hms_opt(0, 0, 0).unwrap().and_utc()))
}

fn official_link(source: &Source, href: &str) -> Option<String> {
    let base = Url::parse(source.home).ok()?;
    let target = base.join(href).ok()?;
    if target.origin() != base.origin() || target.path() == "/" {
        return None;
    }
    canonicalize_url(target.as_str())
}

fn raw(title: String, link: String, date: Option<String>, summary: String) -> RawItem {
    RawItem {
        title,
        link,
        pub_date: date,
        description: Some(summary),
        ..RawItem::default()
    }
}

fn kimi(document: &Html, source: &Source) -> Vec<RawItem> {
    document
        .select(&selector(".menu-card"))
        .take(SOURCE_ITEM_LIMIT)
        .map(|card| {
            let href = card
                .select(&selector("a[href]"))
                .next()
                .and_then(|a| a.value().attr("href"));
            raw(
                child_text(card, "h4.card-title"),
                href.and_then(|h| official_link(source, h))
                    .unwrap_or_default(),
                calendar_date(&child_text(card, ".card-date")),
                child_text(card, ".card-description"),
            )
        })
        .collect()
}

fn anthropic(document: &Html, source: &Source) -> Vec<RawItem> {
    document
        .select(&selector("main a[href]"))
        .filter(|link| link.select(&selector("time")).next().is_some())
        .take(SOURCE_ITEM_LIMIT)
        .map(|link| {
            let title = child_text(link, "h2,h3,h4,h5,h6,span[class*=__title]");
            let href = link.value().attr("href").unwrap_or_default();
            raw(
                title,
                official_link(source, href).unwrap_or_default(),
                calendar_date(&child_text(link, "time")),
                child_text(link, "p"),
            )
        })
        .collect()
}

fn deepseek(document: &Html, source: &Source) -> Vec<RawItem> {
    let mut items = Vec::new();
    let mut current: Option<RawItem> = None;
    let Some(container) = document
        .select(&selector(".theme-doc-markdown .col--12"))
        .next()
    else {
        return items;
    };
    for element in container.children().filter_map(ElementRef::wrap) {
        if element.value().name() == "h2" {
            if let Some(item) = current.take() {
                items.push(item);
                if items.len() == SOURCE_ITEM_LIMIT {
                    break;
                }
            }
            current = element
                .value()
                .attr("id")
                .and_then(|id| id.strip_prefix("date-"))
                .map(|date| {
                    raw(
                        String::new(),
                        String::new(),
                        calendar_date(date),
                        String::new(),
                    )
                });
        } else if let Some(item) = current.as_mut() {
            if element.value().name() == "h3" {
                // Docusaurus hash-link text is an accessibility marker, not title.
                let title = element
                    .text()
                    .filter(|part| part.trim() != "\u{200b}" && part.trim() != "#")
                    .collect::<Vec<_>>()
                    .join(" ");
                if !item.title.is_empty() {
                    item.title.push_str(" / ");
                }
                item.title.push_str(&title);
            }
            if item.link.is_empty() {
                item.link = element
                    .select(&selector("a[href]"))
                    .filter_map(|link| link.value().attr("href"))
                    .filter_map(|href| official_link(source, href))
                    .find(|link| {
                        Url::parse(link).is_ok_and(|url| url.path().starts_with("/news/news"))
                    })
                    .unwrap_or_default();
            }
            if element.value().name() == "p"
                && item.description.as_ref().is_some_and(String::is_empty)
            {
                item.description = Some(text(element));
            }
        }
    }
    if items.len() < SOURCE_ITEM_LIMIT {
        if let Some(item) = current {
            items.push(item);
        }
    }
    items
}

fn json(body: &str) -> Result<Value, FeedError> {
    serde_json::from_str(body).map_err(|_| FeedError::InvalidSourceData)
}

fn string(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn seed(body: &str) -> Result<Vec<RawItem>, FeedError> {
    let data = json(body)?;
    if data.pointer("/BaseResp/StatusCode").and_then(Value::as_i64) != Some(0) {
        return Err(FeedError::InvalidSourceData);
    }
    let items = data
        .get("sub_article_list")
        .and_then(Value::as_array)
        .ok_or(FeedError::InvalidSourceData)?;
    let mut newest: Vec<_> = items.iter().collect();
    newest.sort_by_key(|item| {
        std::cmp::Reverse(
            item.pointer("/ArticleMeta/PublishDate")
                .and_then(Value::as_i64),
        )
    });
    Ok(newest
        .into_iter()
        .take(SOURCE_ITEM_LIMIT)
        .map(|item| {
            let content = &item["ArticleSubContentEn"];
            let date = item
                .pointer("/ArticleMeta/PublishDate")
                .and_then(Value::as_i64)
                .and_then(DateTime::from_timestamp_millis)
                .map(iso_timestamp);
            let link = slug_link(
                "https://seed.bytedance.com/en/blog/",
                &string(content, "TitleKey"),
            );
            raw(
                string(content, "Title"),
                link.unwrap_or_default(),
                date,
                string(content, "Abstract"),
            )
        })
        .collect())
}

fn slug_link(base: &str, slug: &str) -> Option<String> {
    if slug.is_empty()
        || slug == "."
        || slug == ".."
        || !slug.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.'
        })
    {
        return None;
    }
    Some(format!("{base}{slug}"))
}

fn hunyuan(body: &str) -> Result<Vec<RawItem>, FeedError> {
    let data = json(body)?;
    if data.get("code").and_then(Value::as_i64) != Some(0) {
        return Err(FeedError::InvalidSourceData);
    }
    let items = data
        .pointer("/data/list")
        .and_then(Value::as_array)
        .ok_or(FeedError::InvalidSourceData)?;
    Ok(items
        .iter()
        .take(SOURCE_ITEM_LIMIT)
        .map(|item| {
            let date = ["displayPublishTime", "publishedAt"]
                .iter()
                .filter_map(|field| item.get(field).and_then(Value::as_i64))
                .find(|timestamp| *timestamp > 0)
                .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
                .map(iso_timestamp);
            let mut slug = string(item, "customUrl");
            if slug.is_empty() {
                slug = item
                    .get("id")
                    .and_then(Value::as_u64)
                    .map(|id| id.to_string())
                    .unwrap_or_default();
            }
            let link = slug_link("https://hunyuan.tencent.com/research/", &slug)
                .map(|url| format!("{url}?langVersion=zh"));
            raw(
                string(item, "title"),
                link.unwrap_or_default(),
                date,
                string(item, "desc"),
            )
        })
        .collect())
}

fn qwen(body: &str) -> Result<Vec<RawItem>, FeedError> {
    let data = json(body)?;
    if data.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(FeedError::InvalidSourceData);
    }
    let items = data
        .pointer("/data/articles")
        .and_then(Value::as_array)
        .ok_or(FeedError::InvalidSourceData)?;
    let mut newest: Vec<_> = items.iter().collect();
    newest.sort_by_key(|item| {
        std::cmp::Reverse(
            item.pointer("/extra/date")
                .and_then(Value::as_str)
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok()),
        )
    });
    Ok(newest
        .into_iter()
        .take(SOURCE_ITEM_LIMIT)
        .map(|item| {
            let slug = string(item, "path");
            let link =
                slug_link("https://qwen.ai/blog?id=", &slug).map(|url| format!("{url}&locale=en"));
            let extra = &item["extra"];
            let mut summary = string(extra, "description");
            if summary.trim().is_empty() {
                summary = string(extra, "introduction");
            }
            raw(
                string(item, "title"),
                link.unwrap_or_default(),
                extra.get("date").and_then(Value::as_str).map(str::to_owned),
                summary,
            )
        })
        .collect())
}

fn glm(document: &Html, source: &Source) -> Result<Vec<RawItem>, FeedError> {
    let mut dates = std::collections::BTreeSet::new();
    let mut items = Vec::new();
    for update in document.select(&selector("update")).take(SOURCE_ITEM_LIMIT) {
        let label = update.value().attr("label").unwrap_or_default();
        let date = calendar_date(label);
        // The official HTML uses this exact date as its anchor ID. Duplicate
        // labels would make permalink identity ambiguous; fail closed.
        if date.is_none() || !dates.insert(label.to_owned()) {
            return Err(FeedError::InvalidSourceData);
        }
        items.push(raw(
            update
                .value()
                .attr("description")
                .unwrap_or_default()
                .to_owned(),
            format!("{}#{label}", source.home),
            date,
            String::new(),
        ));
    }
    Ok(items)
}

pub(super) fn article_url(source: &Source, input: &str) -> Option<String> {
    let canonical = canonicalize_url(input)?;
    if source.id != "glm" {
        return Some(canonical);
    }
    let original = Url::parse(input.trim()).ok()?;
    let fragment = original.fragment()?;
    if canonical != "https://docs.z.ai/release-notes/new-released"
        || fragment.len() != 10
        || NaiveDate::parse_from_str(fragment, "%Y-%m-%d").is_err()
    {
        return None;
    }
    Some(format!("{canonical}#{fragment}"))
}

#[cfg(test)]
#[path = "tests/ingestion.rs"]
pub(crate) mod tests;
