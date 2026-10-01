use super::*;
use serde_json::json;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn feed(items: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:content="http://purl.org/rss/1.0/modules/content/"><channel><title>Fixture</title>{items}</channel></rss>"#
    )
}

fn item(title: &str, url: &str, date: &str, summary: &str) -> String {
    format!("<item><title>{title}</title><link>{url}</link><pubDate>{date}</pubDate><description>{summary}</description></item>")
}

fn valid_item() -> String {
    item(
        "A model arrives",
        "https://example.com/story",
        "Wed, 30 Sep 2026 12:30:00 +0000",
        "A useful announcement",
    )
}

fn normalize(xml: &str) -> Result<Vec<Article>, FeedError> {
    normalize_feed(&SOURCES[0], xml, now())
}

#[test]
fn official_sources_match_existing_site() {
    let expected = [
        (
            "openai",
            "OpenAI",
            "https://openai.com/news/rss.xml",
            "https://openai.com/news/",
        ),
        (
            "deepmind",
            "Google DeepMind",
            "https://deepmind.google/blog/rss.xml",
            "https://deepmind.google/blog/",
        ),
        (
            "google",
            "Google AI",
            "https://blog.google/innovation-and-ai/technology/ai/rss/",
            "https://blog.google/innovation-and-ai/technology/ai/",
        ),
        (
            "nvidia",
            "NVIDIA",
            "https://blogs.nvidia.com/feed/",
            "https://blogs.nvidia.com/",
        ),
        (
            "huggingface",
            "Hugging Face",
            "https://huggingface.co/blog/feed.xml",
            "https://huggingface.co/blog",
        ),
    ];
    for (source, (id, name, url, home)) in SOURCES.iter().zip(expected) {
        assert_eq!(
            (source.id, source.name, source.url, source.home),
            (id, name, url, home)
        );
        assert_eq!(source_by_id(id), Some(source));
    }
    assert!(source_by_id("unknown").is_none());
    assert!(source_by_id("OpenAI").is_none());
}

#[test]
fn public_sources_omit_feed_urls() {
    let sources = public_sources();
    assert_eq!(sources.len(), SOURCES.len());
    assert_eq!(
        serde_json::to_value(&sources[0]).unwrap(),
        json!({"id":"openai", "name":"OpenAI", "home":"https://openai.com/news/"})
    );
    assert_eq!(
        serde_json::to_value(SOURCES[0]).unwrap()["url"],
        SOURCES[0].url
    );
}

#[test]
fn clean_text_decodes_entities_removes_markup_and_collapses_whitespace() {
    assert_eq!(
        clean_text(" \n<p>Hello&nbsp; world</p>\t &amp; &#20013; &#x1F916; "),
        "Hello world & 中 🤖"
    );
    assert_eq!(clean_text("&lt;b&gt;bold&lt;/b&gt;"), "bold");
    assert_eq!(
        clean_text("one<br/>two <img src='x'> three"),
        "one two three"
    );
    assert_eq!(clean_text("<b></b> \u{2003}\n\t"), "");
    assert_eq!(clean_text("2 < 3 &unknown;"), "2 < 3 &unknown;");
}

#[test]
fn excerpts_are_unicode_safe_and_limited() {
    for ch in ["a", "中", "🤖"] {
        assert_eq!(clean_excerpt(&ch.repeat(421)), ch.repeat(420));
    }
    assert_eq!(clean_excerpt("hello"), "hello");
    assert_eq!(clean_excerpt(""), "");
    assert_eq!(
        clean_excerpt(&format!("<p>{}</p>", "🤖".repeat(420)))
            .chars()
            .count(),
        420
    );
}

#[test]
fn every_category_rule_matches_case_insensitively() {
    let groups: &[(&str, &[&str])] = &[
        ("具身智能", &["robot", "ROBOTIC", "physical AI"]),
        (
            "研究前沿",
            &[
                "research",
                "Paper",
                "SCIENCE",
                "reasoning",
                "benchmark",
                "study",
            ],
        ),
        (
            "开发工具",
            &[
                "open-source",
                "open source",
                "hugging",
                "developer",
                "API",
                "agent",
                "coding",
                "code",
                "tool",
            ],
        ),
        (
            "安全治理",
            &["policy", "SAFETY", "responsible", "security", "governance"],
        ),
        (
            "模型进展",
            &["model", "GPT", "Gemini", "claude", "llm", "Llama"],
        ),
        ("产业动态", &["business update", "", "新产品发布"]),
    ];
    for (expected, keywords) in groups {
        for keyword in *keywords {
            assert_eq!(classify_category(keyword), *expected, "{keyword}");
        }
    }
}

#[test]
fn category_precedence_is_stable() {
    assert_eq!(
        classify_category("robot research agent safety GPT"),
        "具身智能"
    );
    assert_eq!(classify_category("research agent safety GPT"), "研究前沿");
    assert_eq!(classify_category("agent safety GPT"), "开发工具");
    assert_eq!(classify_category("safety GPT"), "安全治理");
}

#[test]
fn canonicalizes_https_host_ports_fragments_and_tracking() {
    let cases = [
        (" https://EXAMPLE.com:443/a#part ", "https://example.com/a"),
        ("https://example.com", "https://example.com/"),
        (
            "https://example.com/p?utm_source=x&utm_source=y&keep=1#x",
            "https://example.com/p?keep=1",
        ),
        (
            "https://example.com/p?a=1&utm_campaign=x&a=2",
            "https://example.com/p?a=1&a=2",
        ),
        (
            "https://example.com/p?utm_source=x",
            "https://example.com/p",
        ),
        (
            "https://example.com/p?UTM_SOURCE=x&utm_=y&x=",
            "https://example.com/p?UTM_SOURCE=x&x=",
        ),
        (
            "https://example.com/p?q=a%20b",
            "https://example.com/p?q=a%20b",
        ),
        (
            "https://example.com/p?%75tm_source=x&keep=yes",
            "https://example.com/p?keep=yes",
        ),
        ("https://example.com:8443/p#x", "https://example.com:8443/p"),
    ];
    for (input, expected) in cases {
        assert_eq!(
            canonicalize_url(input).as_deref(),
            Some(expected),
            "{input}"
        );
        assert_eq!(
            canonicalize_url(expected).as_deref(),
            Some(expected),
            "idempotence"
        );
    }
}

#[test]
fn rejects_unsafe_or_invalid_links() {
    for input in [
        "http://example.com",
        "ftp://example.com",
        "javascript:alert(1)",
        "data:text/plain,news",
        "/relative",
        "not a url",
        "",
        "https://",
        "https://user@example.com",
        "https://:password@example.com",
        "https://user:password@example.com",
    ] {
        assert_eq!(canonicalize_url(input), None, "{input}");
    }
}

#[test]
fn timestamps_use_utc_milliseconds() {
    let date = DateTime::parse_from_rfc3339("2026-09-30T20:00:00.123456-04:00")
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(iso_timestamp(date), "2026-10-01T00:00:00.123Z");
    assert_eq!(iso_timestamp(now()), "2026-10-01T00:00:00.000Z");
}

#[test]
fn parses_rss_and_iso_dates_with_offsets() {
    for input in [
        "Wed, 30 Sep 2026 12:30:00 +0000",
        "2026-09-30T12:30:00Z",
        "2026-09-30T14:30:00+02:00",
        " Wed, 30 Sep 2026 12:30:00 GMT ",
    ] {
        assert_eq!(
            iso_timestamp(published_date(input, now()).unwrap()),
            "2026-09-30T12:30:00.000Z"
        );
    }
}

#[test]
fn rejects_invalid_and_too_future_dates_without_panicking() {
    for input in [
        "",
        "not a date",
        "2026-02-30T10:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-10-02T00:00:00.001Z",
        "999999999999",
        "<script>",
    ] {
        assert!(published_date(input, now()).is_none(), "{input}");
    }
    assert!(published_date("2026-10-02T00:00:00Z", now()).is_some());
    assert!(published_date("1970-01-01T00:00:00Z", now()).is_some());
    assert!(published_date("2026-10-01T00:00:00Z", DateTime::<Utc>::MAX_UTC).is_some());
    assert!(published_date("2026-10-01T00:00:00Z", DateTime::<Utc>::MIN_UTC).is_none());
}

#[test]
fn normalizes_article_contract_and_sha256_identity() {
    let articles = normalize(&feed(&valid_item())).unwrap();
    assert_eq!(articles.len(), 1);
    let article = &articles[0];
    assert_eq!(article.title, "A model arrives");
    assert_eq!(article.url, "https://example.com/story");
    assert_eq!(
        article.id,
        "ab116c15d8ac8b5285e0a8c70be0bf03ad55f8ca63cbfe0903142381dec8268d"
    );
    assert_eq!(article.id.len(), 64);
    assert_eq!(article.source, "openai");
    assert_eq!(article.category, "模型进展");
    assert_eq!(article.summary, "A useful announcement");
    assert_eq!(article.published_at, "2026-09-30T12:30:00.000Z");
    assert_eq!(article.fetched_at, "2026-10-01T00:00:00.000Z");
    let json = serde_json::to_string(article).unwrap();
    assert_eq!(serde_json::from_str::<Article>(&json).unwrap(), *article);
}

#[test]
fn handles_cdata_escaped_html_and_xml_entities() {
    let xml = feed(&item(
        "<![CDATA[<b>AI</b> &amp; Science]]>",
        "https://example.com/p?a=1&amp;utm_source=rss#x",
        "2026-09-30T12:30:00Z",
        "&lt;p&gt;One&amp;nbsp; two &amp;amp; three &#x1F916;&lt;/p&gt;",
    ));
    let article = normalize(&xml).unwrap().remove(0);
    assert_eq!(article.title, "AI & Science");
    assert_eq!(article.url, "https://example.com/p?a=1");
    // XML escaping is decoded before the HTML-to-plain-text cleaning pass.
    assert_eq!(article.summary, "One two & three 🤖");
}

#[test]
fn nested_markup_and_empty_elements_become_plain_text() {
    let xml = feed(&item(
        "AI <b>research</b>",
        "https://example.com",
        "2026-09-30T00:00:00Z",
        "one<p>two<br/>three</p>four",
    ));
    let article = normalize(&xml).unwrap().remove(0);
    assert_eq!(article.title, "AI research");
    assert_eq!(article.summary, "one two three four");
}

#[test]
fn missing_description_falls_back_to_content_and_dc_date() {
    let xml = feed("<item><title>AI research</title><link>https://example.com/x</link><dc:date>2026-09-30T00:00:00Z</dc:date><content:encoded><![CDATA[<p>Useful content</p>]]></content:encoded></item>");
    let article = normalize(&xml).unwrap().remove(0);
    assert_eq!(article.summary, "Useful content");
    assert_eq!(article.published_at, "2026-09-30T00:00:00.000Z");
}

#[test]
fn explicit_empty_description_takes_precedence_over_content() {
    let xml = feed("<item><title>AI news</title><link>https://example.com/x</link><pubDate>2026-09-30T00:00:00Z</pubDate><description/><content:encoded>Fallback</content:encoded></item>");
    assert_eq!(normalize(&xml).unwrap()[0].summary, "");
}

#[test]
fn missing_both_summary_fields_is_valid() {
    let xml = feed("<item><title>AI news</title><link>https://example.com/x</link><pubDate>2026-09-30T00:00:00Z</pubDate></item>");
    assert_eq!(normalize(&xml).unwrap()[0].summary, "");
}

#[test]
fn pub_date_takes_precedence_over_dc_date_even_when_invalid() {
    let xml = feed("<item><title>AI news</title><link>https://example.com/x</link><pubDate>bad</pubDate><dc:date>2026-09-30T00:00:00Z</dc:date></item>");
    assert!(normalize(&xml).unwrap().is_empty());
}

#[test]
fn skips_missing_required_fields() {
    for fields in [
        "<title>AI</title><link>https://example.com/x</link>",
        "<title>AI</title><pubDate>2026-09-30T00:00:00Z</pubDate>",
        "<link>https://example.com/x</link><pubDate>2026-09-30T00:00:00Z</pubDate>",
        "<title/><link>https://example.com/x</link><pubDate>2026-09-30T00:00:00Z</pubDate>",
        "<title>AI</title><link/><pubDate>2026-09-30T00:00:00Z</pubDate>",
        "<title>AI</title><link>https://example.com/x</link><pubDate/>",
        "",
    ] {
        assert!(
            normalize(&feed(&format!("<item>{fields}</item>")))
                .unwrap()
                .is_empty(),
            "{fields}"
        );
    }
}

#[test]
fn invalid_entries_do_not_discard_valid_neighbors() {
    let xml = feed(
        &[
            item(
                "Invalid link",
                "http://example.com",
                "2026-09-30T00:00:00Z",
                "",
            ),
            item("Invalid date", "https://example.com/invalid", "bad", ""),
            item("", "https://example.com/empty", "2026-09-30T00:00:00Z", ""),
            item(
                "Future",
                "https://example.com/future",
                "2026-10-02T00:00:01Z",
                "",
            ),
            valid_item(),
        ]
        .join(""),
    );
    assert_eq!(normalize(&xml).unwrap().len(), 1);
}

#[test]
fn source_id_is_preserved_for_every_publisher() {
    for source in SOURCES
        .iter()
        .filter(|source| !ingestion::is_indexed_source(source.id))
    {
        assert_eq!(
            normalize_feed(source, &feed(&valid_item()), now()).unwrap()[0].source,
            source.id
        );
    }
}

#[test]
fn nvidia_requires_ai_keywords_in_title_or_summary() {
    let source = source_by_id("nvidia").unwrap();
    for keyword in [
        "AI",
        "artificial intelligence",
        "machine learning",
        "agent",
        "model",
        "robot",
        "LLM",
        "inference",
        "training",
    ] {
        for (title, summary) in [(keyword, ""), ("Announcement", keyword)] {
            let xml = feed(&item(
                title,
                "https://example.com/x",
                "2026-09-30T00:00:00Z",
                summary,
            ));
            assert_eq!(
                normalize_feed(source, &xml, now()).unwrap().len(),
                1,
                "{keyword}"
            );
        }
    }
    for title in [
        "Graphics launch",
        "A daily update",
        "Retail expansion",
        "chair",
    ] {
        let xml = feed(&item(
            title,
            "https://example.com/x",
            "2026-09-30T00:00:00Z",
            "Nonmatching news",
        ));
        assert!(
            normalize_feed(source, &xml, now()).unwrap().is_empty(),
            "{title}"
        );
        assert_eq!(normalize(&xml).unwrap().len(), 1);
    }
}

#[test]
fn nvidia_filter_and_category_use_only_the_bounded_excerpt() {
    let summary = format!("{} AI robot", "x".repeat(420));
    let xml = feed(&item(
        "Launch",
        "https://example.com/x",
        "2026-09-30T00:00:00Z",
        &summary,
    ));
    assert!(normalize_feed(&SOURCES[3], &xml, now()).unwrap().is_empty());
    assert_eq!(normalize(&xml).unwrap()[0].category, "产业动态");
}

#[test]
fn canonical_url_duplicates_keep_first_valid_article() {
    let xml = feed(
        &[
            item(
                "First",
                "https://example.com/x?utm_source=one#top",
                "2026-09-30T00:00:00Z",
                "One",
            ),
            item(
                "Duplicate",
                "https://example.com/x?utm_source=two",
                "2026-09-30T00:00:00Z",
                "Two",
            ),
            item(
                "Distinct",
                "https://example.com/x?keep=yes",
                "2026-09-30T00:00:00Z",
                "Three",
            ),
        ]
        .join(""),
    );
    let articles = normalize(&xml).unwrap();
    assert_eq!(articles.len(), 2);
    assert_eq!(articles[0].title, "First");
    assert_eq!(articles[0].summary, "One");
    assert_ne!(articles[0].id, articles[1].id);
}

#[test]
fn rejected_duplicate_does_not_hide_a_later_valid_article() {
    let xml = feed(
        &[
            item("Bad", "https://example.com/story", "invalid", ""),
            valid_item(),
        ]
        .join(""),
    );
    assert_eq!(normalize(&xml).unwrap()[0].title, "A model arrives");
}

#[test]
fn limits_first_35_items_before_validation() {
    let items = (0..40)
        .map(|i| {
            item(
                &format!("AI {i}"),
                &format!("https://example.com/{i}"),
                "2026-09-30T00:00:00Z",
                "",
            )
        })
        .collect::<String>();
    let articles = normalize(&feed(&items)).unwrap();
    assert_eq!(articles.len(), SOURCE_ITEM_LIMIT);
    assert_eq!(articles.last().unwrap().title, "AI 34");
    let invalid_then_valid = format!("{}{}", "<item/>".repeat(35), valid_item());
    assert!(normalize(&feed(&invalid_then_valid)).unwrap().is_empty());
}

#[test]
fn ignores_channel_metadata_extensions_and_nested_fake_items() {
    let xml = feed(&format!("<link>https://example.com/channel</link><extension><item><title>Ignore</title></item></extension><item><ignored><title>Wrong</title></ignored><title>Right</title><link>https://example.com/x</link><pubDate>2026-09-30T00:00:00Z</pubDate><guid>ignored</guid></item><item><unknown/></item>{}", valid_item()));
    let articles = normalize(&xml).unwrap();
    assert_eq!(articles.len(), 2);
    assert_eq!(articles[0].title, "Right");
}

#[test]
fn accepts_empty_rss_channels_comments_and_processing_instructions() {
    for xml in [
        "<rss><channel/></rss>",
        " \n<rss><channel></channel></rss> \n",
        "<?xml version='1.0'?><!-- fixture --><rss><?fixture testing?><channel/></rss>",
    ] {
        assert!(normalize(xml).unwrap().is_empty());
    }
}

#[test]
fn rejects_non_rss_incomplete_and_multiple_root_documents() {
    for xml in [
        "",
        "plain text",
        "<html/>",
        "<feed/>",
        "<rss/>",
        "<rss><other/></rss>",
        "<rss><channel>",
        "<rss><channel/></rss><rss><channel/></rss>",
        "<rss><channel/></rss>unexpected",
        "<![CDATA[text]]><rss><channel/></rss>",
        "&amp;<rss><channel/></rss>",
    ] {
        assert!(normalize(xml).is_err(), "{xml}");
    }
}

#[test]
fn malformed_xml_is_not_partially_ingested() {
    for xml in [
        "<rss><channel></rss>",
        "<rss><channel><item></channel></rss>",
        "<rss><channel><!-- unclosed</channel></rss>",
        "<rss><channel><!-- invalid -- comment --></channel></rss>",
    ] {
        assert!(
            matches!(normalize(xml), Err(FeedError::InvalidXml(_))),
            "{xml}"
        );
    }
    let xml = format!("<rss><channel>{}<item><title>truncated", valid_item());
    assert!(normalize(&xml).is_err());
}

#[test]
fn rejects_document_types_without_expanding_external_or_recursive_entities() {
    for declaration in [
        "<!DOCTYPE rss>",
        "<!DOCTYPE rss SYSTEM 'file:///etc/passwd'>",
        "<!DOCTYPE rss [<!ENTITY xxe SYSTEM 'https://example.com/private'>]>",
        "<!DOCTYPE rss [<!ENTITY a '&a;&a;'>]>",
    ] {
        assert_eq!(
            normalize(&format!("{declaration}<rss><channel/></rss>")),
            Err(FeedError::UnsupportedDoctype)
        );
    }
}

#[test]
fn enforces_feed_size_in_bytes_with_inclusive_boundary() {
    let prefix = "<rss><channel><!--";
    let suffix = "--></channel></rss>";
    let exact = format!(
        "{prefix}{}{suffix}",
        "x".repeat(MAX_FEED_BYTES - prefix.len() - suffix.len())
    );
    assert_eq!(exact.len(), MAX_FEED_BYTES);
    assert!(normalize(&exact).unwrap().is_empty());
    assert_eq!(normalize(&(exact + " ")), Err(FeedError::TooLarge));
    assert_eq!(
        normalize(&"🤖".repeat(MAX_FEED_BYTES / 4 + 1)),
        Err(FeedError::TooLarge)
    );
}

#[test]
fn enforces_xml_depth_with_inclusive_boundary() {
    let nested = |count: usize| {
        format!(
            "<rss><channel>{}{}</channel></rss>",
            "<x>".repeat(count),
            "</x>".repeat(count)
        )
    };
    assert!(normalize(&nested(MAX_XML_DEPTH - 2)).unwrap().is_empty());
    assert_eq!(
        normalize(&nested(MAX_XML_DEPTH - 1)),
        Err(FeedError::TooDeep)
    );
}

#[test]
fn error_messages_are_useful_and_implement_standard_error() {
    for (error, expected) in [
        (FeedError::TooLarge, "Feed exceeds 3 MB limit"),
        (
            FeedError::InvalidXml("mismatched tag".into()),
            "Invalid XML: mismatched tag",
        ),
        (FeedError::InvalidFormat, "Invalid RSS format"),
        (
            FeedError::UnsupportedDoctype,
            "RSS document types are not allowed",
        ),
        (FeedError::TooDeep, "RSS exceeds XML nesting limit"),
    ] {
        assert_eq!(error.to_string(), expected);
        let error: &dyn std::error::Error = &error;
        assert!(error.source().is_none());
    }
}

#[test]
fn article_summary_is_bounded_after_parsing_multibyte_text() {
    let xml = feed(&item(
        "研究",
        "https://example.com/中文",
        "2026-09-30T00:00:00Z",
        &format!("<![CDATA[<p>{}</p>]]>", "🤖中".repeat(500)),
    ));
    let article = normalize(&xml).unwrap().remove(0);
    assert_eq!(article.summary, "🤖中".repeat(210));
    assert_eq!(article.url, "https://example.com/%E4%B8%AD%E6%96%87");
}

#[test]
fn rejects_malformed_and_duplicate_attributes() {
    for xml in [
        "<rss version='2.0' version='2.0'><channel/></rss>",
        "<rss version=2.0><channel/></rss>",
        "<rss broken><channel/></rss>",
    ] {
        assert!(
            matches!(normalize(xml), Err(FeedError::InvalidXml(_))),
            "{xml}"
        );
    }
}

#[test]
fn decodes_entities_in_required_fields_before_validation() {
    let xml = feed(&item(
        "AI &#78;ews",
        "https://example.com/&#120;",
        "2026-09-30T00:00:00&#90;",
        "",
    ));
    let article = normalize(&xml).unwrap().remove(0);
    assert_eq!(article.title, "AI News");
    assert_eq!(article.url, "https://example.com/x");
    assert_eq!(article.published_at, "2026-09-30T00:00:00.000Z");
}

#[test]
fn json_bridge_returns_native_equivalent_articles() {
    let xml = feed(&valid_item());
    let value: serde_json::Value =
        serde_json::from_str(&normalize_feed_json("openai", &xml, "2026-10-01T00:00:00Z")).unwrap();
    assert_eq!(
        value,
        json!({"ok": true, "articles": normalize(&xml).unwrap()})
    );
    assert!(value.get("error").is_none());
}

#[test]
fn json_bridge_preserves_unicode_and_escapes_quotes_and_backslashes() {
    let xml = feed(&item(
        "人工智能 &quot;news&quot;",
        "https://example.com/unicode",
        "2026-09-30T00:00:00Z",
        "<![CDATA[<p>机器人 🤖 \\ test</p>]]>",
    ));
    let serialized = normalize_feed_json("openai", &xml, "2026-10-01T00:00:00Z");
    let value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(value["articles"][0]["title"], "人工智能 \"news\"");
    assert_eq!(value["articles"][0]["summary"], "机器人 🤖 \\ test");
    assert_eq!(value["ok"], true);
}

#[test]
fn json_bridge_accepts_every_known_source_and_current_time_offset() {
    let xml = feed(&valid_item());
    for source in SOURCES {
        let value: serde_json::Value = serde_json::from_str(&normalize_feed_json(
            source.id,
            if ingestion::is_indexed_source(source.id) {
                ingestion::tests::fixture(source.id)
            } else {
                &xml
            },
            "2026-09-30T20:00:00-04:00",
        ))
        .unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["articles"][0]["source"], source.id);
        assert_eq!(
            value["articles"][0]["fetched_at"],
            "2026-10-01T00:00:00.000Z"
        );
    }
}

#[test]
fn json_bridge_unknown_sources_return_error_envelopes() {
    for source in [
        "",
        "unknown",
        "OpenAI",
        " openai ",
        "https://example.com",
        "🦀",
        "openai\\\"\n",
    ] {
        let value: serde_json::Value = serde_json::from_str(&normalize_feed_json(
            source,
            &feed(&valid_item()),
            "2026-10-01T00:00:00Z",
        ))
        .unwrap();
        assert_eq!(value, json!({"ok": false, "error": "Unknown news source"}));
        assert!(value.get("articles").is_none());
    }
}

#[test]
fn json_bridge_rejects_missing_malformed_and_ambiguous_clocks() {
    for now in [
        "",
        "yesterday",
        "2026-10-01",
        "Wed, 30 Sep 2026 12:30:00 GMT",
        "2026-02-30T00:00:00Z",
        "2026-10-01T00:00:00",
        "<script>",
        "999999999999999999999999",
    ] {
        let value: serde_json::Value =
            serde_json::from_str(&normalize_feed_json("openai", &feed(&valid_item()), now))
                .unwrap();
        assert_eq!(
            value,
            json!({"ok": false, "error": "Invalid current timestamp: expected RFC 3339"}),
            "{now}"
        );
    }
}

#[test]
fn json_bridge_validation_priority_is_deterministic() {
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&normalize_feed_json(
            "unknown",
            "invalid xml",
            "invalid clock"
        ))
        .unwrap(),
        json!({"ok": false, "error": "Unknown news source"})
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&normalize_feed_json(
            "openai",
            "invalid xml",
            "invalid clock"
        ))
        .unwrap(),
        json!({"ok": false, "error": "Invalid current timestamp: expected RFC 3339"})
    );
}

#[test]
fn json_bridge_empty_and_filtered_feeds_succeed_with_empty_arrays() {
    for xml in [
        feed(""),
        feed("<item/>"),
        feed(&item(
            "Old article",
            "http://example.com",
            "2026-09-30T00:00:00Z",
            "",
        )),
    ] {
        let value: serde_json::Value =
            serde_json::from_str(&normalize_feed_json("openai", &xml, "2026-10-01T00:00:00Z"))
                .unwrap();
        assert_eq!(value, json!({"ok": true, "articles": []}));
    }
}

#[test]
fn json_bridge_reports_all_feed_error_variants_as_strings() {
    let cases = [
        ("not rss".to_string(), "Invalid RSS format"),
        ("<rss><channel></rss>".to_string(), "Invalid XML:"),
        (
            "<!DOCTYPE rss><rss><channel/></rss>".to_string(),
            "RSS document types are not allowed",
        ),
        ("x".repeat(MAX_FEED_BYTES + 1), "Feed exceeds 3 MB limit"),
        (
            format!(
                "<rss><channel>{}{}</channel></rss>",
                "<x>".repeat(MAX_XML_DEPTH),
                "</x>".repeat(MAX_XML_DEPTH)
            ),
            "RSS exceeds XML nesting limit",
        ),
    ];
    for (xml, expected) in cases {
        let value: serde_json::Value =
            serde_json::from_str(&normalize_feed_json("openai", &xml, "2026-10-01T00:00:00Z"))
                .unwrap();
        assert_eq!(value["ok"], false);
        assert!(
            value["error"].as_str().unwrap().starts_with(expected),
            "{value}"
        );
        assert!(value.get("articles").is_none());
    }
}

#[test]
fn public_sources_json_bridge_is_an_array_without_internal_urls() {
    let value: serde_json::Value = serde_json::from_str(&public_sources_json()).unwrap();
    assert_eq!(value, serde_json::to_value(public_sources()).unwrap());
    let sources = value.as_array().unwrap();
    assert_eq!(sources.len(), SOURCES.len());
    for (source, expected) in sources.iter().zip(SOURCES) {
        assert_eq!(source["id"], expected.id);
        assert_eq!(source["name"], expected.name);
        assert_eq!(source["home"], expected.home);
        assert!(source.get("url").is_none());
    }
}

#[test]
fn tracking_removal_preserves_retained_query_bytes() {
    for query in [
        "q=a%20b",
        "next=/a:b~c",
        "flag",
        "q=a%2fb&q=a%2Fb",
        "flag&flag=&x=1&x=2",
        "a+b=some+value",
        "%70age=%2F",
    ] {
        let untracked = format!("https://example.com/p?{query}");
        let tracked = format!("https://example.com/p?utm_source=rss&{query}&utm_medium=email");
        assert_eq!(canonicalize_url(&untracked), Some(untracked.clone()));
        assert_eq!(canonicalize_url(&tracked), Some(untracked), "{query}");
    }
}

#[test]
fn tracking_removal_decodes_only_keys_and_preserves_empty_segments() {
    for (query, expected) in [
        ("%75tm_source=rss&q=a%20b&u%74m_medium=email", "?q=a%20b"),
        ("utm%5Fcampaign=rss&next=/a:b~c", "?next=/a:b~c"),
        (
            "keep=utm_source%3Dx&%55TM_SOURCE=rss",
            "?keep=utm_source%3Dx&%55TM_SOURCE=rss",
        ),
        ("%75tm_source", ""),
        ("utm_source=x&utm_medium=y", ""),
        ("utm_source=&utm_medium", ""),
        ("", "?"),
        ("&", "?&"),
        ("utm_source=x&", "?"),
        ("&utm_source=x&&flag&", "?&&flag&"),
        ("=keep&utm_source=x", "?=keep"),
    ] {
        assert_eq!(
            canonicalize_url(&format!("https://example.com/p?{query}")),
            Some(format!("https://example.com/p{expected}")),
            "{query}",
        );
    }
}

#[test]
fn tracked_and_untracked_urls_have_identical_article_ids() {
    for query in ["q=a%20b", "next=/a:b~c", "flag"] {
        let untracked = format!("https://example.com/p?{query}");
        let tracked = format!("{untracked}&amp;utm_source=rss");
        let first = normalize(&feed(&item(
            "AI news",
            &untracked,
            "2026-09-30T00:00:00Z",
            "",
        )))
        .unwrap();
        let second = normalize(&feed(&item(
            "AI news",
            &tracked,
            "2026-09-30T00:00:00Z",
            "",
        )))
        .unwrap();
        assert_eq!(first[0].id, second[0].id, "{query}");
        assert_eq!(first[0].url, second[0].url, "{query}");
    }
}

#[test]
fn sha256_identity_keeps_leading_zero_bytes_and_canonical_equivalence() {
    for url in [
        "https://example.com/story/223",
        "https://EXAMPLE.com:443/story/223?utm_source=rss#section",
    ] {
        let article = normalize(&feed(&item(
            "AI news",
            url,
            "2026-09-30T00:00:00Z",
            "An update",
        )))
        .unwrap()
        .remove(0);
        // Fixed vector independently computed from the canonical URL's UTF-8 bytes.
        assert_eq!(
            article.id,
            "00c7cec5d9121cd11558453e3f03224db6136b17a6764e1dc265d84f44dde40b"
        );
        assert_eq!(article.url, "https://example.com/story/223");
    }
}

#[test]
fn xml_string_events_preserve_unicode_cdata_and_references() {
    let xml = feed(&item(
        "中文 &#x1F916; &amp; AI",
        "https://example.com/unicode?a=1&amp;b=2",
        "2026-09-30T00:00:00Z",
        "<![CDATA[中文 <b>café</b>]]> &#20013; &amp; 🤖",
    ));
    let article = normalize(&xml).unwrap().remove(0);
    assert_eq!(article.title, "中文 🤖 & AI");
    assert_eq!(article.summary, "中文 café 中 & 🤖");
    assert_eq!(article.url, "https://example.com/unicode?a=1&b=2");
}
