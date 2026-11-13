//! RSS 2.0 feed generation.
//!
//! The output is intentionally very close to the feed produced by the original
//! original pipeline so that existing subscribers see no change.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

use crate::config::Config;
use crate::model::ArticleSummary;

/// Escape text for XML content and attribute values.
pub fn escape_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Format a timestamp the way JavaScript's `Date#toUTCString` does.
pub fn format_utc(date: DateTime<Utc>) -> String {
    date.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Parse an article date (`YYYY.MM.DD`) into midnight UTC.
pub fn parse_article_date(value: &str) -> Option<DateTime<Utc>> {
    NaiveDate::parse_from_str(value.trim(), "%Y.%m.%d")
        .ok()
        .and_then(|date| {
            Utc.from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
                .single()
        })
}

/// Render the RSS feed.
///
/// `articles` must already be filtered (visible posts only) and sorted newest
/// first. `now` is used both for `lastBuildDate` and as a fallback `pubDate`.
pub fn render(config: &Config, articles: &[ArticleSummary], now: DateTime<Utc>) -> String {
    let limit = config.rss.limit;
    let items: Vec<String> = articles
        .iter()
        .take(limit)
        .map(|article| render_item(config, article, now))
        .collect();
    let language = config.site_language();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel>
    <title>{title}</title>
    <link>{url}</link>
    <description>{description}</description>
    <language>{language}</language>
    <lastBuildDate>{last_build_date}</lastBuildDate>
    <atom:link href="{self_url}" rel="self" type="application/rss+xml" />
{items}
  </channel>
</rss>"#,
        title = escape_xml(&config.site.name),
        url = escape_xml(config.site.url.trim_end_matches('/')),
        description = escape_xml(config.site_description()),
        last_build_date = format_utc(now),
        self_url = escape_xml(&format!(
            "{}/{}",
            config.site.url.trim_end_matches('/'),
            config.rss.file
        )),
        items = items.join("\n"),
    )
}

fn render_item(config: &Config, article: &ArticleSummary, now: DateTime<Utc>) -> String {
    let link = format!(
        "{}{}",
        config.site.url.trim_end_matches('/'),
        article.summary_url_path()
    );
    let pub_date = article
        .date
        .as_deref()
        .and_then(parse_article_date)
        .map_or_else(|| format_utc(now), format_utc);
    let author = if article.author.is_empty() {
        config.default_author()
    } else {
        &article.author
    };
    let categories: Vec<String> = article
        .tags
        .iter()
        .map(|tag| format!("      <category>{}</category>", escape_xml(tag)))
        .collect();
    let mut item = format!(
        r#"    <item>
      <title>{title}</title>
      <link>{link}</link>
      <guid isPermaLink="true">{guid}</guid>
      <pubDate>{pub_date}</pubDate>
      <dc:creator>{author}</dc:creator>"#,
        title = escape_xml(&article.title),
        link = escape_xml(&link),
        guid = escape_xml(&link),
        author = escape_xml(author),
    );
    for line in &categories {
        item.push('\n');
        item.push_str(line);
    }
    item.push_str("\n    </item>");
    item
}

impl ArticleSummary {
    /// URL path used by the feed.
    fn summary_url_path(&self) -> &str {
        &self.url_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, SiteConfig};

    fn config() -> Config {
        let mut config = Config {
            site: SiteConfig {
                name: "SF-Zhou's Blog".into(),
                url: "https://sf-zhou.github.io".into(),
                description: Some("SF-Zhou's Blog".into()),
                language: Some("zh-CN".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        config.rss.limit = 20;
        config
    }

    fn article(title: &str, date: &str) -> ArticleSummary {
        ArticleSummary {
            title: title.into(),
            date: Some(date.into()),
            author: "SF-Zhou".into(),
            tags: vec!["Rust".into(), "RDMA".into()],
            filename: "post".into(),
            author_url: "https://github.com/SF-Zhou".into(),
            url_path: "/rdma/post.html".into(),
        }
    }

    #[test]
    fn parses_dates_to_utc_midnight() {
        let parsed = parse_article_date("2026.11.12").unwrap();
        assert_eq!(format_utc(parsed), "Thu, 12 Nov 2026 00:00:00 GMT");
        assert!(parse_article_date("not-a-date").is_none());
    }

    #[test]
    fn renders_channel_and_items() {
        let now = Utc.with_ymd_and_hms(2026, 11, 13, 1, 2, 3).unwrap();
        let feed = render(&config(), &[article("Title & <Test>", "2026.11.12")], now);
        assert!(feed.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(feed.contains("<title>SF-Zhou&apos;s Blog</title>"));
        assert!(feed.contains("<language>zh-CN</language>"));
        assert!(feed.contains("<lastBuildDate>Fri, 13 Nov 2026 01:02:03 GMT</lastBuildDate>"));
        assert!(feed.contains("<title>Title &amp; &lt;Test&gt;</title>"));
        assert!(feed.contains("<pubDate>Thu, 12 Nov 2026 00:00:00 GMT</pubDate>"));
        assert!(feed.contains("<dc:creator>SF-Zhou</dc:creator>"));
        assert!(feed.contains("<category>Rust</category>"));
        assert!(feed.contains("<category>RDMA</category>"));
    }

    #[test]
    fn falls_back_to_now_for_invalid_dates() {
        let now = Utc.with_ymd_and_hms(2026, 11, 13, 1, 2, 3).unwrap();
        let mut broken = article("Broken", "nope");
        broken.date = Some("nope".into());
        let feed = render(&config(), &[broken], now);
        assert!(feed.contains("<pubDate>Fri, 13 Nov 2026 01:02:03 GMT</pubDate>"));
    }

    #[test]
    fn respects_limit() {
        let now = Utc.with_ymd_and_hms(2026, 11, 13, 1, 2, 3).unwrap();
        let articles: Vec<_> = (0..25)
            .map(|index| article(&format!("post {index}"), "2026.01.01"))
            .collect();
        let feed = render(&config(), &articles, now);
        assert_eq!(feed.matches("<item>").count(), 20);
    }
}
