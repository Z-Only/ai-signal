use super::*;
use crate::{normalize_feed, normalize_feed_json, source_by_id, MAX_FEED_BYTES, SOURCES};
use chrono::{DateTime, Utc};
use serde_json::json;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn normalize(id: &str, body: &str) -> Result<Vec<crate::Article>, FeedError> {
    normalize_feed(source_by_id(id).unwrap(), body, now())
}

pub(crate) fn fixture(id: &str) -> &'static str {
    match id {
        "anthropic" => include_str!("../../tests/fixtures/anthropic.html"),
        "deepseek" => include_str!("../../tests/fixtures/deepseek.html"),
        "kimi" => include_str!("../../tests/fixtures/kimi.html"),
        "bytedance" => include_str!("../../tests/fixtures/bytedance.json"),
        "tencent" => include_str!("../../tests/fixtures/tencent.json"),
        "qwen" => include_str!("../../tests/fixtures/qwen.json"),
        "glm" => include_str!("../../tests/fixtures/glm.md"),
        _ => "",
    }
}

#[test]
fn indexed_sources_keep_the_shared_native_and_json_contract() {
    for source in SOURCES.iter().filter(|s| is_indexed_source(s.id)) {
        let articles = normalize(source.id, fixture(source.id)).unwrap();
        assert!(!articles.is_empty(), "{}", source.id);
        assert!(articles.iter().all(|a| a.source == source.id
            && a.id.len() == 64
            && a.fetched_at == "2026-10-01T00:00:00.000Z"));
        let result: Value = serde_json::from_str(&normalize_feed_json(
            source.id,
            fixture(source.id),
            "2026-10-01T00:00:00Z",
        ))
        .unwrap();
        assert_eq!(result, json!({"ok":true,"articles":articles}));
    }
}

#[test]
fn registry_is_unique_bounded_and_exported_without_public_transport_fields() {
    let ids: std::collections::BTreeSet<_> = SOURCES.iter().map(|s| s.id).collect();
    assert_eq!(ids.len(), 15);
    let manifest: Value = serde_json::from_str(&crate::ingestion_sources_json()).unwrap();
    assert_eq!(manifest, serde_json::to_value(SOURCES).unwrap());
    for source in SOURCES {
        assert!(source.url.starts_with("https://"));
        assert!(source.home.starts_with("https://"));
        if source.id == "tencent" {
            assert_eq!(source.method, "POST");
            assert_eq!(source.request_language, Some("zh"));
            assert_eq!(
                serde_json::from_str::<Value>(source.request_body.unwrap()).unwrap(),
                json!({"pageNum":1,"pageSize":20,"renderType":0})
            );
        } else {
            assert_eq!(source.method, "GET");
            assert_eq!(source.request_body, None);
            assert_eq!(source.request_language, None);
        }
        assert_eq!(
            source.timeout_seconds,
            if source.id == "qwen" { 30 } else { 18 }
        );
        assert_eq!(
            source.max_bytes,
            if source.id == "qwen" {
                6_000_000
            } else {
                MAX_FEED_BYTES
            }
        );
    }
    let public: Value = serde_json::from_str(&crate::public_sources_json()).unwrap();
    for source in public.as_array().unwrap() {
        assert_eq!(source.as_object().unwrap().len(), 3);
    }
}

#[test]
fn date_only_sources_retain_the_published_day_and_never_fetch_time() {
    assert_eq!(
        calendar_date("  September 30, 2026 ").as_deref(),
        Some("2026-09-30T00:00:00.000Z")
    );
    assert_eq!(
        calendar_date("Sep 3, 2026").as_deref(),
        Some("2026-09-03T00:00:00.000Z")
    );
    assert!(calendar_date("2026-02-30").is_none());
    for id in ["anthropic", "kimi", "deepseek", "glm"] {
        assert_eq!(
            normalize(id, fixture(id)).unwrap()[0].published_at,
            "2026-09-30T00:00:00.000Z"
        );
    }
}

#[test]
fn anthropic_supports_featured_root_paths_and_list_title_spans() {
    let articles = normalize("anthropic", fixture("anthropic")).unwrap();
    assert_eq!(articles.len(), 2);
    assert_eq!(articles[0].url, "https://www.anthropic.com/example-release");
    assert_eq!(articles[0].summary, "A publisher excerpt");
    assert_eq!(articles[1].title, "Example AI update");
    assert_eq!(articles[1].summary, "");
    let body = format!("<main><a href='/no-date'><h2>No date</h2></a>{}<a href='https://evil.test/news'><time>Sep 30, 2026</time><h4>Fake</h4></a><a href='/empty'><time>Sep 30, 2026</time></a><a href='/future'><time>Sep 30, 2030</time><h4>Future</h4></a></main>", fixture("anthropic"));
    assert_eq!(normalize("anthropic", &body).unwrap().len(), 2);
}

#[test]
fn kimi_requires_a_dated_title_and_official_link_and_deduplicates_cards() {
    let body = fixture("kimi").repeat(2);
    let articles = normalize("kimi", &body).unwrap();
    assert_eq!(articles.len(), 1);
    assert_eq!(articles[0].url, "https://www.kimi.com/en/blog/example-kimi");
    for body in [
        "<div class='menu-card'></div>".to_owned(),
        fixture("kimi").replace("2026-09-30", "bad"),
        fixture("kimi").replace("/en/blog/example-kimi", "https://evil.test/fake"),
        fixture("kimi").replace("Example Kimi release", ""),
    ] {
        assert_eq!(normalize("kimi", &body), Err(FeedError::InvalidSourceData));
    }
}

#[test]
fn deepseek_requires_a_real_news_link_not_a_shared_index_or_guide() {
    let articles = normalize("deepseek", fixture("deepseek")).unwrap();
    assert_eq!(articles.len(), 1);
    assert_eq!(articles[0].title, "Example DeepSeek release");
    assert_eq!(
        articles[0].url,
        "https://api-docs.deepseek.com/news/news260930"
    );
    let body = fixture("deepseek").replace("</h3><p>Read", "</h3><h3>Second model</h3><p>Read");
    assert_eq!(
        normalize("deepseek", &body).unwrap()[0].title,
        "Example DeepSeek release / Second model"
    );
    for body in [
        "<main></main>".to_owned(),
        fixture("deepseek").replace("date-2026-09-30", "unrelated"),
        fixture("deepseek").replace("/news/news260930", "/guides/example"),
    ] {
        assert_eq!(
            normalize("deepseek", &body),
            Err(FeedError::InvalidSourceData)
        );
    }
    let body = "<div class='theme-doc-markdown'><div class='col--12'><h1>News</h1><p>Intro</p><h2 id='unrelated'>Other heading</h2><p>Ignored</p></div></div>";
    assert_eq!(
        normalize("deepseek", body),
        Err(FeedError::InvalidSourceData)
    );
}

#[test]
fn every_new_source_fails_closed_on_empty_mismatched_or_broken_data() {
    for source in SOURCES.iter().filter(|s| is_indexed_source(s.id)) {
        for body in [
            "",
            "Site Unavailable",
            "{}",
            "[]",
            "<rss><channel/></rss>",
            "<html><body>Login required</body></html>",
        ] {
            let error: Value = serde_json::from_str(&normalize_feed_json(
                source.id,
                body,
                "2026-10-01T00:00:00Z",
            ))
            .unwrap();
            assert_eq!(
                error,
                json!({"ok":false,"error":"Official source data is missing or invalid"})
            );
            assert_eq!(
                normalize(source.id, body),
                Err(FeedError::InvalidSourceData),
                "{} {body}",
                source.id
            );
        }
    }
    for (id, body) in [
        (
            "bytedance",
            r#"{"BaseResp":{"StatusCode":0},"sub_article_list":[]}"#,
        ),
        ("tencent", r#"{"code":0,"data":{"list":[]}}"#),
        ("qwen", r#"{"success":true,"data":{"articles":[]}}"#),
        ("bytedance", r#"{"BaseResp":{"StatusCode":1}}"#),
        ("tencent", r#"{"code":1}"#),
        ("qwen", r#"{"success":false}"#),
        ("bytedance", r#"{"BaseResp":{"StatusCode":0}}"#),
        ("tencent", r#"{"code":0}"#),
        ("qwen", r#"{"success":true}"#),
    ] {
        assert_eq!(normalize(id, body), Err(FeedError::InvalidSourceData));
    }
    assert!(parse(source_by_id("openai").unwrap(), "").is_err());
}

#[test]
fn structured_sources_use_publisher_dates_not_update_times() {
    assert_eq!(
        normalize("bytedance", fixture("bytedance")).unwrap()[0].published_at,
        "2026-09-29T16:00:00.000Z"
    );
    assert_eq!(
        normalize("tencent", fixture("tencent")).unwrap()[0].published_at,
        "2026-09-29T16:00:00.000Z"
    );
    let body = fixture("tencent")
        .replace(
            "\"displayPublishTime\":1790697600",
            "\"displayPublishTime\":0",
        )
        .replace("\"customUrl\":\"example-hunyuan\"", "\"customUrl\":\"\"");
    let article = normalize("tencent", &body).unwrap().remove(0);
    assert_eq!(article.published_at, "2026-09-28T16:00:00.000Z");
    assert_eq!(
        article.url,
        "https://hunyuan.tencent.com/research/123?langVersion=zh"
    );
    assert_eq!(
        normalize("qwen", fixture("qwen")).unwrap()[0].published_at,
        "2026-09-30T02:00:00.000Z"
    );
    assert_eq!(
        normalize("qwen", fixture("qwen")).unwrap()[0].summary,
        "A publisher excerpt"
    );
    let body = fixture("qwen").replace(
        "\"description\":\"\"",
        "\"description\":\"Preferred excerpt\"",
    );
    assert_eq!(
        normalize("qwen", &body).unwrap()[0].summary,
        "Preferred excerpt"
    );
}

#[test]
fn structured_sources_skip_invalid_neighbor_rows_without_using_mutation_dates() {
    for (id, pointer, date_pointer) in [
        ("bytedance", "/sub_article_list", "/ArticleMeta/PublishDate"),
        ("tencent", "/data/list", "/displayPublishTime"),
        ("qwen", "/data/articles", "/extra/date"),
    ] {
        let mut value: Value = serde_json::from_str(fixture(id)).unwrap();
        let row = value.pointer(pointer).unwrap()[0].clone();
        let mut invalid = row.clone();
        *invalid.pointer_mut(date_pointer).unwrap() = Value::Null;
        if id == "tencent" {
            invalid["publishedAt"] = Value::Null;
            invalid["createdAt"] = json!(1790697600);
        }
        *value.pointer_mut(pointer).unwrap() = json!([{}, invalid, row]);
        assert_eq!(normalize(id, &value.to_string()).unwrap().len(), 1);
    }
    assert_eq!(
        slug_link("https://example.test/", "some-model.3_8").as_deref(),
        Some("https://example.test/some-model.3_8")
    );
    for slug in [
        "",
        ".",
        "..",
        "../other",
        "a?x=1",
        "x#y",
        "https://evil.test",
    ] {
        assert_eq!(slug_link("https://example.test/", slug), None);
    }
}

#[test]
fn qwen_sorts_by_original_date_before_the_bounded_item_limit() {
    let rows: Vec<_> = (1..=40).map(|day| json!({"path":format!("day-{day}"),"title":"Example Qwen","extra":{"date":format!("2026-08-{:02}T00:00:00Z", day % 28 + 1)}})).collect();
    let body = json!({"success":true,"data":{"articles":rows}}).to_string();
    let articles = normalize("qwen", &body).unwrap();
    assert_eq!(articles.len(), SOURCE_ITEM_LIMIT);
    assert!(articles
        .windows(2)
        .all(|pair| pair[0].published_at >= pair[1].published_at));
    assert_eq!(articles[0].published_at, "2026-08-28T00:00:00.000Z");
}

#[test]
fn per_source_size_limits_and_html_depth_are_bounded() {
    let oversized = "x".repeat(MAX_FEED_BYTES + 1);
    assert_eq!(normalize("kimi", &oversized), Err(FeedError::TooLarge));
    let qwen_limit = source_by_id("qwen").unwrap().max_bytes;
    let oversized = "x".repeat(qwen_limit + 1);
    let error = normalize("qwen", &oversized).unwrap_err();
    assert_eq!(error, FeedError::SourceTooLarge(qwen_limit));
    assert_eq!(error.to_string(), "Source exceeds 6000000 byte limit");
    let large = fixture("qwen").replace("A publisher excerpt", &"x".repeat(MAX_FEED_BYTES));
    assert_eq!(normalize("qwen", &large).unwrap()[0].summary.len(), 420);
    assert_eq!(
        normalize(
            "kimi",
            &format!("{}{}", "<div>".repeat(70), "</div>".repeat(70))
        ),
        Err(FeedError::InvalidSourceData)
    );
}

#[test]
fn calendar_permalink_exception_is_exact_and_does_not_change_other_sources() {
    let articles = normalize("glm", fixture("glm")).unwrap();
    assert_eq!(articles.len(), 2);
    assert_eq!(
        articles[0].url,
        "https://docs.z.ai/release-notes/new-released#2026-09-30"
    );
    assert_ne!(articles[0].id, articles[1].id);
    assert_eq!(normalize("glm", fixture("glm")).unwrap(), articles);
    let source = source_by_id("glm").unwrap();
    for input in [
        "https://docs.z.ai/release-notes/new-released",
        "https://evil.test/release-notes/new-released#2026-09-30",
        "https://docs.z.ai/other#2026-09-30",
        "https://docs.z.ai/release-notes/new-released#2026-02-30",
        "https://docs.z.ai/release-notes/new-released#2026-9-30",
        "https://docs.z.ai/release-notes/new-released#evil",
        "http://docs.z.ai/release-notes/new-released#2026-09-30",
    ] {
        assert!(article_url(source, input).is_none(), "{input}");
    }
    assert_eq!(
        article_url(source_by_id("openai").unwrap(), &articles[0].url).as_deref(),
        Some("https://docs.z.ai/release-notes/new-released")
    );
    assert_eq!(
        normalize("glm", &fixture("glm").replace("2026-09-29", "2026-09-30")),
        Err(FeedError::InvalidSourceData)
    );
    assert_eq!(
        normalize("glm", "<Update label='bad' description='Unknown'/>"),
        Err(FeedError::InvalidSourceData)
    );
    assert_eq!(
        normalize("glm", "<Update label='2026-09-30'></Update>"),
        Err(FeedError::InvalidSourceData)
    );
    assert_eq!(
        normalize("glm", "<Update description='Unknown'></Update>"),
        Err(FeedError::InvalidSourceData)
    );
}

#[test]
fn official_links_reject_other_origins_credentials_and_invalid_base_urls() {
    let source = source_by_id("anthropic").unwrap();
    for href in [
        "/",
        "http://www.anthropic.com/news/old",
        "//evil.test/news",
        "https://user@www.anthropic.com/news/secret",
        "https://[bad/",
    ] {
        assert_eq!(official_link(source, href), None);
    }
    let invalid = Source {
        home: "bad url",
        ..*source
    };
    assert_eq!(official_link(&invalid, "/news"), None);
}

#[test]
fn seed_sorts_pinned_entries_by_original_publication_date() {
    let mut data: Value = serde_json::from_str(fixture("bytedance")).unwrap();
    let older = data["sub_article_list"][0].clone();
    let mut newer = older.clone();
    newer["ArticleMeta"]["PublishDate"] = json!(1790784000000i64);
    newer["ArticleSubContentEn"]["TitleKey"] = json!("newer-story");
    data["sub_article_list"] = json!([older, newer]);
    let articles = normalize("bytedance", &data.to_string()).unwrap();
    assert!(articles[0].url.ends_with("/newer-story"));
    assert!(articles[0].published_at > articles[1].published_at);
}

#[test]
fn html_adapters_limit_entries_before_invalid_item_filtering() {
    let cards = (0..40).map(|id| format!("<div class='menu-card'><a href='/en/blog/{id}'><h4 class='card-title'>Model {id}</h4><p class='card-date'>2026-09-30</p></a></div>")).collect::<String>();
    assert_eq!(normalize("kimi", &cards).unwrap().len(), SOURCE_ITEM_LIMIT);
    let sections = (0..40).map(|id| format!("<h2 id='date-2026-09-30'>Date</h2><h3>Model {id}</h3><p><a href='/news/news{id}'>Details</a></p><p>Additional information</p>")).collect::<String>();
    let body =
        format!("<div class='theme-doc-markdown'><div class='col--12'>{sections}</div></div>");
    assert_eq!(
        normalize("deepseek", &body).unwrap().len(),
        SOURCE_ITEM_LIMIT
    );
}
