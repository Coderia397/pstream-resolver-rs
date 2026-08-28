//! DramaCool — bespoke extractor for DramaCool (`https://ww1.dramacool.cx`).
//!
//! DramaCool hosts Asian dramas, movies, and shows with slug-based routing:
//! - Episode streams: `/{slug}-episode-{episode}.html`
//! - Search fallback: `/search?type=drama&keyword={title}`
//!
//! Video player servers (Asianload, Vidhide, StreamWish, Mp4Upload, direct .m3u8)
//! are embedded in server tab elements (`<li class="linkserver" data-video="...">`)
//! and player iframes (`<iframe src="...">`).

use crate::extractors::find_m3u8_urls;
use crate::http::{get_text_with, PROXY};
use crate::models::{MediaKind, ProviderResult, Source};
use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Duration;

const NAME: &str = "DramaCool 🎭";
pub const ID: &str = "dramacool";
const BASE: &str = "https://ww1.dramacool.cx";

static LINKSERVER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)<li[^>]*\bclass\s*=\s*["'][^"']*(?:linkserver|server)[^"']*["'][^>]*\bdata-video\s*=\s*["']([^"']+)["']"#)
        .expect("linkserver regex")
});

static DATA_VIDEO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\bdata-video\s*=\s*["']([^"']+)["']"#).expect("data-video regex")
});

static IFRAME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<iframe[^>]*\s+src\s*=\s*["']([^"']+)["']"#).expect("iframe regex")
});

static SEARCH_RESULT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"href\s*=\s*["'](?:https?://[^"'/]+)?/([^"'/]+-episode-\d+\.html|drama-detail/[^"'/]+|[^"'/]+\.html)["'][^>]*>(?:<h3[^>]*>)?([^<]+)"#)
        .expect("search result regex")
});

static SEARCH_DRAMA_DETAIL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"href\s*=\s*["'](?:https?://[^"'/]+)?/drama-detail/([a-zA-Z0-9-]+)["']"#)
        .expect("drama detail regex")
});

/// Non-video host patterns to ignore (ads, tracking, comments, widgets).
static AD_DOMAINS: &[&str] = &[
    "google",
    "facebook",
    "histats",
    "disqus",
    "doubleclick",
    "twitter",
    "cloudflare",
    "recaptcha",
    "analytics",
    "widget",
];

/// Convert title into DramaCool slug format.
/// E.g. "Squid Game" -> "squid-game", "Crash Landing on You" -> "crash-landing-on-you".
pub fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    let mut prev_hyphen = true;

    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_hyphen = false;
        } else if c == '\'' || c == '\"' || c == '`' {
            continue;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }

    if slug.ends_with('-') {
        slug.pop();
    }

    slug
}

/// Construct primary episode path for DramaCool.
pub fn build_episode_path(slug: &str, kind: MediaKind, episode: u32) -> String {
    if kind.is_movie() {
        format!("/{slug}-episode-1.html")
    } else {
        format!("/{slug}-episode-{episode}.html")
    }
}

/// Check if a candidate URL points to a legitimate video host rather than an ad or tracker.
pub fn is_video_source(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if AD_DOMAINS.iter().any(|ad| lower.contains(ad)) {
        return false;
    }

    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("//")
}

/// Normalize protocol-relative URLs (`//example.com/...` -> `https://example.com/...`).
pub fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else {
        trimmed.to_string()
    }
}

/// Extract all playable video embed iframes, server links, and direct .m3u8 playlists from DramaCool HTML.
pub fn extract_sources_from_html(html: &str) -> Vec<Source> {
    let mut sources: Vec<Source> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Linkserver list items: <li class="linkserver" data-video="...">
    for caps in LINKSERVER_RE.captures_iter(html) {
        if let Some(m) = caps.get(1) {
            let normalized = normalize_url(m.as_str());
            if is_video_source(&normalized) && seen.insert(normalized.clone()) {
                let is_m3u8 = normalized.contains(".m3u8");
                let quality = if normalized.contains("1080") {
                    "1080p"
                } else if normalized.contains("480") || normalized.contains("360") {
                    "480p"
                } else {
                    "720p"
                };

                let mut s = if is_m3u8 {
                    Source::direct_m3u8(&normalized, quality)
                } else {
                    Source::embed(&normalized, quality)
                }
                .tagged("DramaCool", ID)
                .with_referer(BASE);

                s.is_m3u8 = is_m3u8;
                sources.push(s);
            }
        }
    }

    // 2. Generic data-video attributes
    for caps in DATA_VIDEO_RE.captures_iter(html) {
        if let Some(m) = caps.get(1) {
            let normalized = normalize_url(m.as_str());
            if is_video_source(&normalized) && seen.insert(normalized.clone()) {
                let is_m3u8 = normalized.contains(".m3u8");
                let mut s = if is_m3u8 {
                    Source::direct_m3u8(&normalized, "720p")
                } else {
                    Source::embed(&normalized, "720p")
                }
                .tagged("DramaCool", ID)
                .with_referer(BASE);

                s.is_m3u8 = is_m3u8;
                sources.push(s);
            }
        }
    }

    // 3. Iframe embed tags: <iframe src="...">
    for caps in IFRAME_RE.captures_iter(html) {
        if let Some(m) = caps.get(1) {
            let normalized = normalize_url(m.as_str());
            if is_video_source(&normalized) && seen.insert(normalized.clone()) {
                let is_m3u8 = normalized.contains(".m3u8");
                let mut s = if is_m3u8 {
                    Source::direct_m3u8(&normalized, "720p")
                } else {
                    Source::embed(&normalized, "720p")
                }
                .tagged("DramaCool", ID)
                .with_referer(BASE);

                s.is_m3u8 = is_m3u8;
                sources.push(s);
            }
        }
    }

    // 4. Direct .m3u8 streams
    for m3u8_url in find_m3u8_urls(html) {
        if seen.insert(m3u8_url.clone()) {
            let s = Source::direct_m3u8(&m3u8_url, "720p")
                .tagged("DramaCool", ID)
                .with_referer(BASE);
            sources.push(s);
        }
    }

    sources
}

/// Parse search response HTML to find best drama slug matching title.
pub fn parse_search_slug(html: &str, title: &str) -> Option<String> {
    let target_slug = slugify(title);
    let mut candidates: Vec<String> = Vec::new();

    // 1. Collect direct drama-detail links
    for caps in SEARCH_DRAMA_DETAIL_RE.captures_iter(html) {
        if let Some(slug_match) = caps.get(1) {
            let slug = slug_match.as_str().trim();
            if !slug.is_empty() && !candidates.iter().any(|c| c.as_str() == slug) {
                candidates.push(slug.to_string());
            }
        }
    }

    // 2. Check generic search results with title matching
    for caps in SEARCH_RESULT_RE.captures_iter(html) {
        if let (Some(path_match), Some(title_match)) = (caps.get(1), caps.get(2)) {
            let path = path_match.as_str();
            let result_title = title_match.as_str().trim();
            let result_slug = slugify(result_title);

            if result_slug == target_slug
                || result_slug.contains(&target_slug)
                || target_slug.contains(&result_slug)
            {
                if let Some(detail_slug) = path.strip_prefix("drama-detail/") {
                    let s = detail_slug.trim_matches('/').to_string();
                    if !candidates.iter().any(|c| c.as_str() == s.as_str()) {
                        candidates.push(s);
                    }
                } else if let Some(ep_slug) = path.split("-episode-").next() {
                    let s = ep_slug.trim_start_matches('/').to_string();
                    if !candidates.iter().any(|c| c.as_str() == s.as_str()) {
                        candidates.push(s);
                    }
                }
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    // Exact match first
    if let Some(exact) = candidates.iter().find(|s| s.as_str() == target_slug.as_str()) {
        return Some(exact.clone());
    }

    // If target doesn't specify a season, prefer a candidate that also doesn't specify a season
    if !target_slug.contains("season") {
        if let Some(no_season) = candidates.iter().find(|s| {
            (s.contains(&target_slug)
                || target_slug.contains(s.as_str())
                || s.replace("the-", "").starts_with(&target_slug))
                && !s.contains("season")
        }) {
            return Some(no_season.clone());
        }
    }

    // Fallback: candidate containing target_slug or target_slug containing candidate
    if let Some(matched) = candidates.iter().find(|s| {
        s.contains(&target_slug) || target_slug.contains(s.as_str())
    }) {
        return Some(matched.clone());
    }

    // Otherwise first candidate
    candidates.into_iter().next()
}

/// Fallback search on DramaCool to discover drama slug.
pub async fn search_drama_slug(title: &str) -> Option<String> {
    let encoded = urlencoding::encode(title);
    let search_url = format!("{BASE}/search?type=drama&keyword={encoded}");

    let html = get_text_with(
        &PROXY,
        &search_url,
        Duration::from_secs(8),
        &[
            ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
            ("Referer", format!("{BASE}/").as_str()),
        ],
    )
    .await?;

    parse_search_slug(&html, title)
}

/// Scrape DramaCool for drama episode streams.
pub async fn scrape(
    _tmdb_id: &str,
    kind: MediaKind,
    _season: u32,
    episode: u32,
    title: Option<&str>,
) -> Option<ProviderResult> {
    let title_str = title?.trim();
    if title_str.is_empty() {
        return None;
    }

    let slug = slugify(title_str);
    if slug.is_empty() {
        return None;
    }

    println!("[DramaCool] Scraping \"{title_str}\" (slug: {slug}) Ep {episode}");

    let primary_path = build_episode_path(&slug, kind, episode);
    let primary_url = format!("{BASE}{primary_path}");

    println!("[DramaCool] Fetching {primary_url}");

    let html = get_text_with(
        &PROXY,
        &primary_url,
        Duration::from_secs(8),
        &[
            ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
            ("Referer", format!("{BASE}/").as_str()),
        ],
    )
    .await;

    let sources = match html {
        Some(ref body) => {
            let src = extract_sources_from_html(body);
            if !src.is_empty() {
                src
            } else {
                try_fallback_search(title_str, kind, episode).await?
            }
        }
        None => {
            try_fallback_search(title_str, kind, episode).await?
        }
    };

    println!("[DramaCool] ✅ Found {} source(s)", sources.len());
    ProviderResult::some_if_any(NAME, ID, sources)
}

async fn try_fallback_search(
    title: &str,
    kind: MediaKind,
    episode: u32,
) -> Option<Vec<Source>> {
    println!("[DramaCool] Attempting search fallback for \"{title}\"");
    let searched_slug = search_drama_slug(title).await?;
    let path = build_episode_path(&searched_slug, kind, episode);
    let url = format!("{BASE}{path}");

    println!("[DramaCool] Fallback fetching {url}");
    let html = get_text_with(
        &PROXY,
        &url,
        Duration::from_secs(8),
        &[
            ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
            ("Referer", format!("{BASE}/").as_str()),
        ],
    )
    .await?;

    let src = extract_sources_from_html(&html);
    if src.is_empty() {
        None
    } else {
        Some(src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_drama_titles() {
        assert_eq!(slugify("Squid Game"), "squid-game");
        assert_eq!(slugify("Crash Landing on You"), "crash-landing-on-you");
        assert_eq!(slugify("The Glory: Part 2"), "the-glory-part-2");
        assert_eq!(slugify("Alchemy of Souls"), "alchemy-of-souls");
        assert_eq!(slugify("It's Okay to Not Be Okay"), "its-okay-to-not-be-okay");
    }

    #[test]
    fn builds_drama_episode_paths() {
        assert_eq!(
            build_episode_path("squid-game", MediaKind::Tv, 1),
            "/squid-game-episode-1.html"
        );
        assert_eq!(
            build_episode_path("squid-game", MediaKind::Tv, 6),
            "/squid-game-episode-6.html"
        );
        assert_eq!(
            build_episode_path("parasite", MediaKind::Movie, 1),
            "/parasite-episode-1.html"
        );
    }

    #[test]
    fn filters_out_ads_and_identifies_video_sources() {
        assert!(is_video_source("https://asianload.io/streaming.php?id=MTE2NjU="));
        assert!(is_video_source("https://streamwish.to/e/abc1234"));
        assert!(is_video_source("https://vidhide.com/v/xyz987"));
        assert!(is_video_source("//standardload.com/embed/12345"));

        assert!(!is_video_source("https://googleads.g.doubleclick.net/pagead/ads"));
        assert!(!is_video_source("https://histats.com/js15.js"));
        assert!(!is_video_source("https://disqus.com/embed.js"));
    }

    #[test]
    fn extracts_server_tabs_and_iframes_from_dramacool_html() {
        let html = r#"
        <div class="content-left">
            <div class="block-watch">
                <div class="watch-iframe">
                    <iframe src="https://asianload.io/streaming.php?id=MTE2NjU=" frameborder="0" allowfullscreen="true"></iframe>
                </div>
                <div class="block-tab">
                    <ul class="list-server-items">
                        <li class="linkserver active" data-status="1" data-video="https://asianload.io/streaming.php?id=MTE2NjU=" data-provider="asianload">
                            AsianLoad (FHD 1080)
                        </li>
                        <li class="linkserver" data-status="1" data-video="https://vidhide.com/v/vhide1234" data-provider="vidhide">
                            VidHide
                        </li>
                        <li class="linkserver" data-status="1" data-video="//streamwish.to/e/swish5678" data-provider="streamwish">
                            StreamWish
                        </li>
                        <li class="linkserver" data-status="1" data-video="https://mp4upload.com/embed-mp4xyz.html" data-provider="mp4upload">
                            Mp4Upload
                        </li>
                    </ul>
                </div>
            </div>
        </div>
        "#;

        let sources = extract_sources_from_html(html);
        assert_eq!(sources.len(), 4);

        assert_eq!(sources[0].url, "https://asianload.io/streaming.php?id=MTE2NjU=");
        assert_eq!(sources[0].is_embed, Some(true));
        assert_eq!(sources[0].provider.as_deref(), Some("DramaCool"));
        assert_eq!(sources[0].provider_id.as_deref(), Some("dramacool"));

        assert_eq!(sources[1].url, "https://vidhide.com/v/vhide1234");
        assert_eq!(sources[2].url, "https://streamwish.to/e/swish5678");
        assert_eq!(sources[3].url, "https://mp4upload.com/embed-mp4xyz.html");
    }

    #[test]
    fn parses_search_slug_from_html() {
        let html = r#"
        <ul class="list-episode-item">
            <li>
                <a href="/drama-detail/squid-game" title="Squid Game">
                    <h3>Squid Game</h3>
                </a>
            </li>
            <li>
                <a href="/drama-detail/squid-game-the-challenge" title="Squid Game: The Challenge">
                    <h3>Squid Game: The Challenge</h3>
                </a>
            </li>
        </ul>
        "#;

        let slug = parse_search_slug(html, "Squid Game");
        assert_eq!(slug, Some("squid-game".to_string()));
    }

    #[test]
    fn extracts_server_tabs_with_standard_server_class() {
        let html = r#"
        <div class="block watch-drama">
            <div class="watch_video watch-iframe">
                <iframe allowfullscreen src="//vidbasic.top/embed/4j8bf92mw"></iframe>
            </div>
            <div class="muti_link">
                <ul>
                    <li class="Standard Server selected" data-video="https://vidbasic.top/embed/4j8bf92mw">Standard Server</li>
                </ul>
            </div>
        </div>
        "#;

        let sources = extract_sources_from_html(html);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].url, "https://vidbasic.top/embed/4j8bf92mw");
        assert_eq!(sources[0].is_embed, Some(true));
        assert_eq!(sources[0].provider.as_deref(), Some("DramaCool"));
        assert_eq!(sources[0].provider_id.as_deref(), Some("dramacool"));
    }

    #[test]
    fn parses_domain_agnostic_search_slug_and_links() {
        let html = r#"
        <ul class="switch-block list-episode-item">
            <li>
                <a href="https://ww1.dramacool.cx/drama-detail/squid-game-season-3" class="img" title="Squid Game Season 3 (2025)">
                    <h3 class="title">Squid Game Season 3 (2025)</h3>
                </a>
            </li>
            <li>
                <a href="https://ww1.dramacool.cx/drama-detail/squid-game-season-2" class="img" title="Squid Game Season 2 (2024)">
                    <h3 class="title">Squid Game Season 2 (2024)</h3>
                </a>
            </li>
            <li>
                <a href="https://ww1.dramacool.cx/drama-detail/the-squid-games" class="img" title="Squid Games (2021)">
                    <h3 class="title">Squid Games (2021)</h3>
                </a>
            </li>
        </ul>
        "#;

        let slug = parse_search_slug(html, "Squid Game");
        assert_eq!(slug, Some("the-squid-games".to_string()));

        let s2_slug = parse_search_slug(html, "Squid Game Season 2");
        assert_eq!(s2_slug, Some("squid-game-season-2".to_string()));
    }
}
