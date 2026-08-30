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
pub use crate::utils::slugify;
use crate::utils::{matches_year_tolerance, sort_sources_by_quality};
use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Duration;

const NAME: &str = "DramaCool 🎭";
pub const ID: &str = "dramacool";
const BASE: &str = "https://ww1.dramacool.cx";

static LINKSERVER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<li[^>]*\b(?:class\s*=\s*["'][^"']*(?:linkserver|server)[^"']*["'][^>]*\bdata-video\s*=\s*["']([^"']+)["']|\bdata-video\s*=\s*["']([^"']+)["'][^>]*\bclass\s*=\s*["'][^"']*(?:linkserver|server)[^"']*["'])[^>]*>(.*?)</li>"#)
        .expect("linkserver regex")
});

static DATA_VIDEO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<[^>]*\bdata-video\s*=\s*["']([^"']+)["'][^>]*>(.*?)(?:</[^>]+>|$)"#)
        .expect("data-video regex")
});

static IFRAME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<iframe[^>]*\s+src\s*=\s*["']([^"']+)["']"#).expect("iframe regex")
});

static SEARCH_ANCHOR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<a\b([^>]*\bhref\s*=\s*["'][^"']+["'][^>]*)>(.*?)</a>"#)
        .expect("search anchor regex")
});

static HREF_ATTR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\bhref\s*=\s*["'](?:https?://[^"'/]+)?/([^"']+)["']"#)
        .expect("href attr regex")
});

static TITLE_ATTR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\btitle\s*=\s*["']([^"']+)["']"#).expect("title attr regex")
});

static STRIP_TAGS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<[^>]+>"#).expect("strip tags regex")
});

static HEADER_YEAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\((19\d{2}|20\d{2})\)"#).expect("dramacool header year regex")
});

static BARE_YEAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\b(19\d{2}|20\d{2})\b"#).expect("dramacool bare year regex")
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

#[derive(Debug, Clone)]
struct DramaSearchCandidate {
    slug: String,
    title: String,
    year: Option<u32>,
}

fn extract_year_from_drama_text(text: &str) -> Option<u32> {
    if let Some(caps) = HEADER_YEAR_RE.captures(text) {
        if let Some(m) = caps.get(1) {
            if let Ok(y) = m.as_str().parse::<u32>() {
                return Some(y);
            }
        }
    }
    if let Some(caps) = BARE_YEAR_RE.captures(text) {
        if let Some(m) = caps.get(1) {
            if let Ok(y) = m.as_str().parse::<u32>() {
                return Some(y);
            }
        }
    }
    None
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

    // 1. Linkserver list items: <li class="linkserver" data-video="...">Text</li>
    for caps in LINKSERVER_RE.captures_iter(html) {
        let raw_url = caps.get(1).or_else(|| caps.get(2));
        if let Some(m) = raw_url {
            let normalized = normalize_url(m.as_str());
            let tag_text = caps.get(3).map(|t| t.as_str()).unwrap_or("");
            if is_video_source(&normalized) && seen.insert(normalized.clone()) {
                let is_m3u8 = normalized.contains(".m3u8");
                let combined = format!("{normalized} {tag_text}").to_ascii_lowercase();
                let quality = if combined.contains("1080") || combined.contains("fhd") {
                    "1080p"
                } else if combined.contains("480") || combined.contains("360") || combined.contains("sd") {
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
            let tag_text = caps.get(2).map(|t| t.as_str()).unwrap_or("");
            if is_video_source(&normalized) && seen.insert(normalized.clone()) {
                let is_m3u8 = normalized.contains(".m3u8");
                let combined = format!("{normalized} {tag_text}").to_ascii_lowercase();
                let quality = if combined.contains("1080") || combined.contains("fhd") {
                    "1080p"
                } else if combined.contains("480") || combined.contains("360") || combined.contains("sd") {
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

    // 3. Iframe embed tags: <iframe src="...">
    for caps in IFRAME_RE.captures_iter(html) {
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

    // 4. Direct .m3u8 streams
    for m3u8_url in find_m3u8_urls(html) {
        if seen.insert(m3u8_url.clone()) {
            let quality = if m3u8_url.contains("1080") {
                "1080p"
            } else if m3u8_url.contains("480") || m3u8_url.contains("360") {
                "480p"
            } else {
                "720p"
            };

            let s = Source::direct_m3u8(&m3u8_url, quality)
                .tagged("DramaCool", ID)
                .with_referer(BASE);
            sources.push(s);
        }
    }

    sort_sources_by_quality(&mut sources);
    sources
}

/// Parse search response HTML to find best drama slug matching title and year (+/- 1 year tolerance).
pub fn parse_search_slug(html: &str, title: &str, target_year: Option<u32>) -> Option<String> {
    let target_slug = slugify(title);
    let mut candidates: Vec<DramaSearchCandidate> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Scan all anchor tags for drama links
    for caps in SEARCH_ANCHOR_RE.captures_iter(html) {
        let tag_attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let inner_html = caps.get(2).map(|m| m.as_str()).unwrap_or("");

        let Some(href_cap) = HREF_ATTR_RE.captures(tag_attrs) else {
            continue;
        };
        let path = href_cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if path.is_empty() || path.contains("index") || path.contains("search") || path == "home" {
            continue;
        }

        let slug_opt = if let Some(detail_slug) = path.strip_prefix("drama-detail/") {
            Some(detail_slug.trim_matches('/').to_string())
        } else if let Some(ep_slug) = path.split("-episode-").next() {
            Some(ep_slug.trim_start_matches('/').trim_end_matches(".html").to_string())
        } else if path.ends_with(".html") {
            Some(path.trim_end_matches(".html").trim_start_matches('/').to_string())
        } else {
            Some(path.trim_matches('/').to_string())
        };

        if let Some(slug) = slug_opt {
            if !slug.is_empty() && seen.insert(slug.clone()) {
                let title_attr = TITLE_ATTR_RE
                    .captures(tag_attrs)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str())
                    .unwrap_or("");
                let inner_text = STRIP_TAGS_RE.replace_all(inner_html, " ");
                let combined_text = format!("{title_attr} {inner_text}").trim().to_string();
                let year = extract_year_from_drama_text(&combined_text)
                    .or_else(|| extract_year_from_drama_text(&slug));

                candidates.push(DramaSearchCandidate {
                    slug,
                    title: combined_text,
                    year,
                });
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    // Filter by title relevance
    let relevant: Vec<&DramaSearchCandidate> = candidates
        .iter()
        .filter(|c| {
            if target_slug.is_empty() {
                return true;
            }
            let c_slug = &c.slug;
            let c_title_slug = slugify(&c.title);
            c_slug == &target_slug
                || c_slug.starts_with(&format!("{target_slug}-"))
                || c_slug.contains(&target_slug)
                || target_slug.contains(c_slug.as_str())
                || (!c_title_slug.is_empty()
                    && (c_title_slug == target_slug
                        || c_title_slug.starts_with(&format!("{target_slug}-"))
                        || c_title_slug.contains(&target_slug)
                        || target_slug.contains(&c_title_slug)))
        })
        .collect();

    let pool = if relevant.is_empty() {
        candidates.iter().collect::<Vec<_>>()
    } else {
        relevant
    };

    if let Some(target_y) = target_year {
        // Priority 1: Exact title match AND exact year
        if let Some(exact) = pool.iter().find(|c| {
            (c.slug == target_slug || slugify(&c.title) == target_slug) && c.year == Some(target_y)
        }) {
            return Some(exact.slug.clone());
        }

        // Priority 2: Any relevant candidate with exact year
        if let Some(exact_year) = pool.iter().find(|c| c.year == Some(target_y)) {
            return Some(exact_year.slug.clone());
        }

        // Priority 3: Tolerance year match (+/- 1 year)
        if let Some(tol_year) = pool.iter().find(|c| {
            matches_year_tolerance(c.year, Some(target_y), 1)
        }) {
            return Some(tol_year.slug.clone());
        }

        // Priority 4: Exact title match without year
        if let Some(exact) = pool.iter().find(|c| c.slug == target_slug) {
            return Some(exact.slug.clone());
        }

        // Priority 5: Candidate with no year specified
        if let Some(no_year) = pool.iter().find(|c| c.year.is_none()) {
            return Some(no_year.slug.clone());
        }
    } else {
        // No year provided
        // Exact match first
        if let Some(exact) = pool.iter().find(|c| c.slug == target_slug) {
            return Some(exact.slug.clone());
        }

        // If target doesn't specify a season, prefer a candidate that also doesn't specify a season
        if !target_slug.contains("season") {
            if let Some(no_season) = pool.iter().find(|c| {
                (c.slug.contains(&target_slug)
                    || target_slug.contains(c.slug.as_str())
                    || c.slug.replace("the-", "").starts_with(&target_slug))
                    && !c.slug.contains("season")
            }) {
                return Some(no_season.slug.clone());
            }
        }
    }

    // Fallback: candidate containing target_slug or target_slug containing candidate
    if let Some(matched) = pool.iter().find(|c| {
        c.slug.contains(&target_slug) || target_slug.contains(c.slug.as_str())
    }) {
        return Some(matched.slug.clone());
    }

    // Otherwise first candidate
    pool.first().map(|c| c.slug.clone())
}

/// Fallback search on DramaCool to discover drama slug.
pub async fn search_drama_slug(title: &str, year: Option<u32>) -> Option<String> {
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

    parse_search_slug(&html, title, year)
}

/// Scrape DramaCool for drama episode streams.
pub async fn scrape(
    _tmdb_id: &str,
    kind: MediaKind,
    _season: u32,
    episode: u32,
    title: Option<&str>,
    year: Option<u32>,
) -> Option<ProviderResult> {
    let title_str = title?.trim();
    if title_str.is_empty() {
        return None;
    }

    let slug = slugify(title_str);
    if slug.is_empty() {
        return None;
    }

    println!(
        "[DramaCool] Scraping \"{title_str}\" (slug: {slug}) Ep {episode} (year: {:?})",
        year
    );

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
                try_fallback_search(title_str, kind, episode, year).await?
            }
        }
        None => {
            try_fallback_search(title_str, kind, episode, year).await?
        }
    };

    println!("[DramaCool] ✅ Found {} source(s)", sources.len());
    ProviderResult::some_if_any(NAME, ID, sources)
}

async fn try_fallback_search(
    title: &str,
    kind: MediaKind,
    episode: u32,
    year: Option<u32>,
) -> Option<Vec<Source>> {
    println!("[DramaCool] Attempting search fallback for \"{title}\"");
    let searched_slug = search_drama_slug(title, year).await?;
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

        // Sources sorted descending by quality: 1080p first!
        assert_eq!(sources[0].url, "https://asianload.io/streaming.php?id=MTE2NjU=");
        assert_eq!(sources[0].quality, "1080p");
        assert_eq!(sources[0].is_embed, Some(true));
        assert_eq!(sources[0].provider.as_deref(), Some("DramaCool"));
        assert_eq!(sources[0].provider_id.as_deref(), Some("dramacool"));

        assert_eq!(sources[1].url, "https://vidhide.com/v/vhide1234");
        assert_eq!(sources[1].quality, "720p");

        assert_eq!(sources[2].url, "https://streamwish.to/e/swish5678");
        assert_eq!(sources[2].quality, "720p");

        assert_eq!(sources[3].url, "https://mp4upload.com/embed-mp4xyz.html");
        assert_eq!(sources[3].quality, "720p");
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

        let slug = parse_search_slug(html, "Squid Game", None);
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

        let slug = parse_search_slug(html, "Squid Game", Some(2021));
        assert_eq!(slug, Some("the-squid-games".to_string()));

        let s2_slug = parse_search_slug(html, "Squid Game Season 2", Some(2024));
        assert_eq!(s2_slug, Some("squid-game-season-2".to_string()));
    }
}
