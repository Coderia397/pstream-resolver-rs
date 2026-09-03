//! BSTSrs — bespoke extractor for BSTSrs (`https://bstsrs.in`).
//!
//! BSTSrs hosts TV series and shows with slug-based routing:
//! - TV Episodes: `/show/{slug}-s{season:02d}e{episode:02d}/season/{season}/episode/{episode}`
//! - Movies / Specials: `/show/{slug}-movie/season/1/episode/1` or `/show/{slug}/season/1/episode/1`
//!
//! Embed sources are protected with an inline JavaScript cipher:
//! `onclick="window.open(dbneg('19e0c8906-19e0c8912-...'), '_blank');return false;"`
//!
//! The cipher splits the hex string by `-`, parses each part in base 16,
//! subtracts a base offset (default `0x19e0c889e`), and converts the resulting
//! integer to a character.

use crate::extractors::find_m3u8_urls;
use crate::http::get_text_with;
use crate::models::{MediaKind, ProviderResult, Source};
pub use crate::utils::slugify;
use crate::utils::{matches_year_tolerance, sort_sources_by_quality};
use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Duration;

const NAME: &str = "BSTSrs Series 📺";
pub const ID: &str = "bstsrs";
const BASE: &str = "https://bstsrs.in";
const DEFAULT_OFFSET: u64 = 0x19e0c889e;

static DBNEG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"dbneg\s*\(\s*['"]([0-9a-fA-F-]+)['"]\s*\)"#).expect("dbneg regex")
});

static OFFSET_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"var\s+a1\s*=\s*0x([0-9a-fA-F]+);"#).expect("offset regex")
});

static SEARCH_ANCHOR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<a[^>]*\bhref\s*=\s*["'](?:https?://bstsrs\.in)?/show/([a-zA-Z0-9-]+)["'][^>]*>(.*?)</a>"#)
        .expect("search anchor regex")
});

static STRIP_TAGS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<[^>]+>"#).expect("strip tags regex")
});

static SLUG_YEAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"-(19\d{2}|20\d{2})(?:-|$)"#).expect("slug year regex")
});

static TEXT_YEAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\b(19\d{2}|20\d{2})\b"#).expect("text year regex")
});

static SEARCH_LINK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"href\s*=\s*["'](?:https?://bstsrs\.in)?/show/([a-zA-Z0-9-]+)["']"#)
        .expect("search link regex")
});

static IFRAME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<iframe[^>]*\s+src\s*=\s*["']([^"']+)["']"#).expect("iframe regex")
});

#[derive(Debug, Clone)]
struct BstsrsSearchCandidate {
    slug: String,
    text: String,
    year: Option<u32>,
}

fn extract_year_from_slug(slug: &str) -> Option<u32> {
    SLUG_YEAR_RE
        .captures(slug)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
}

fn extract_year_from_text(text: &str) -> Option<u32> {
    TEXT_YEAR_RE
        .captures(text)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
}

/// Construct the BSTSrs episode URL path given a slug, media kind, season, and episode.
pub fn build_episode_path(slug: &str, kind: MediaKind, season: u32, episode: u32) -> String {
    if kind.is_movie() {
        format!("/show/{slug}-movie/season/1/episode/1")
    } else {
        format!("/show/{slug}-s{:02}e{:02}/season/{}/episode/{}", season, episode, season, episode)
    }
}

/// Decode a BSTSrs `dbneg` obfuscated string into a plaintext URL.
///
/// Algorithm:
/// 1. Split encoded string on `-`.
/// 2. Parse each chunk as hexadecimal integer.
/// 3. Subtract `offset` (default `0x19e0c889e`).
/// 4. Convert result to Unicode character.
pub fn decode_dbneg(encoded: &str, offset: u64) -> Option<String> {
    if encoded.trim().is_empty() {
        return None;
    }

    let mut decoded = String::new();
    for part in encoded.split('-') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let val = u64::from_str_radix(trimmed, 16).ok()?;
        let char_code = val.checked_sub(offset)? as u32;
        let c = char::from_u32(char_code)?;
        decoded.push(c);
    }

    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

/// Extract the dynamic offset integer from JavaScript in the page, falling back to `0x19e0c889e`.
pub fn extract_offset_from_html(html: &str) -> u64 {
    if let Some(caps) = OFFSET_RE.captures(html) {
        if let Some(hex_str) = caps.get(1) {
            if let Ok(offset) = u64::from_str_radix(hex_str.as_str(), 16) {
                return offset;
            }
        }
    }
    DEFAULT_OFFSET
}

/// Extract all decoded embed links, iframe URLs, and direct .m3u8 manifests from BSTSrs HTML.
pub fn extract_sources_from_html(html: &str) -> Vec<Source> {
    let mut sources: Vec<Source> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let offset = extract_offset_from_html(html);

    // 1. Parse dbneg encoded links from onclick handlers
    for caps in DBNEG_RE.captures_iter(html) {
        if let Some(encoded_match) = caps.get(1) {
            if let Some(decoded_url) = decode_dbneg(encoded_match.as_str(), offset) {
                let trimmed = decoded_url.trim().to_string();
                if trimmed.starts_with("http") && seen.insert(trimmed.clone()) {
                    let is_m3u8 = trimmed.contains(".m3u8");
                    let quality = if trimmed.contains("1080") {
                        "1080p"
                    } else if trimmed.contains("480") || trimmed.contains("360") {
                        "480p"
                    } else {
                        "720p"
                    };

                    let mut s = if is_m3u8 {
                        Source::direct_m3u8(&trimmed, quality)
                    } else {
                        Source::embed(&trimmed, quality)
                    }
                    .tagged("BSTSrs", ID)
                    .with_referer(BASE);

                    s.is_m3u8 = is_m3u8;
                    sources.push(s);
                }
            }
        }
    }

    // 2. Parse iframe embed tags in the page
    for caps in IFRAME_RE.captures_iter(html) {
        if let Some(src_match) = caps.get(1) {
            let mut src = src_match.as_str().trim().to_string();
            if src.starts_with("//") {
                src = format!("https:{src}");
            }
            if src.starts_with("http") && seen.insert(src.clone()) {
                let is_m3u8 = src.contains(".m3u8");
                let quality = if src.contains("1080") {
                    "1080p"
                } else if src.contains("480") || src.contains("360") {
                    "480p"
                } else {
                    "720p"
                };
                let mut s = if is_m3u8 {
                    Source::direct_m3u8(&src, quality)
                } else {
                    Source::embed(&src, quality)
                }
                .tagged("BSTSrs", ID)
                .with_referer(BASE);

                s.is_m3u8 = is_m3u8;
                sources.push(s);
            }
        }
    }

    // 3. Sweep for any direct .m3u8 manifests in page
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
                .tagged("BSTSrs", ID)
                .with_referer(BASE);
            sources.push(s);
        }
    }

    sort_sources_by_quality(&mut sources);
    sources
}

/// Fallback search on BSTSrs to discover show slug if direct slug fails.
pub async fn search_show_slug(title: &str, year: Option<u32>) -> Option<String> {
    let search_url = format!("{BASE}/index.php");
    let form_body = format!("menu=search&query={}", urlencoding::encode(title));

    let resp = crate::http::GIGA
        .post(&search_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Referer", format!("{BASE}/"))
        .header("Origin", BASE)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .timeout(Duration::from_secs(8))
        .body(form_body)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body = resp.text().await.ok()?;
    parse_search_show_slug(&body, title, year)
}

/// Parse search response HTML to find best show slug matching title and year (+/- 1 year tolerance).
pub fn parse_search_show_slug(
    html: &str,
    title: &str,
    target_year: Option<u32>,
) -> Option<String> {
    let target_slug = slugify(title);
    let mut candidates: Vec<BstsrsSearchCandidate> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Structured anchor tags with inner text
    for caps in SEARCH_ANCHOR_RE.captures_iter(html) {
        if let (Some(slug_m), Some(text_m)) = (caps.get(1), caps.get(2)) {
            let slug = slug_m.as_str().trim();
            if slug.is_empty() || slug.contains("index") || slug.contains("search") || slug == "home" {
                continue;
            }
            if seen.insert(slug.to_string()) {
                let clean_text = STRIP_TAGS_RE.replace_all(text_m.as_str(), " ").trim().to_string();
                let year = extract_year_from_text(&clean_text).or_else(|| extract_year_from_slug(slug));
                candidates.push(BstsrsSearchCandidate {
                    slug: slug.to_string(),
                    text: clean_text,
                    year,
                });
            }
        }
    }

    // 2. Fallback to bare hrefs if no structured anchors matched
    if candidates.is_empty() {
        for caps in SEARCH_LINK_RE.captures_iter(html) {
            if let Some(slug_m) = caps.get(1) {
                let slug = slug_m.as_str().trim();
                if slug.is_empty() || slug.contains("index") || slug.contains("search") || slug == "home" {
                    continue;
                }
                if seen.insert(slug.to_string()) {
                    let year = extract_year_from_slug(slug);
                    candidates.push(BstsrsSearchCandidate {
                        slug: slug.to_string(),
                        text: String::new(),
                        year,
                    });
                }
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    // Filter by title relevance if title is provided
    let relevant: Vec<&BstsrsSearchCandidate> = candidates
        .iter()
        .filter(|c| {
            if target_slug.is_empty() {
                return true;
            }
            let c_slug = &c.slug;
            let c_text_slug = slugify(&c.text);
            c_slug == &target_slug
                || c_slug.starts_with(&format!("{target_slug}-"))
                || c_slug.contains(&target_slug)
                || target_slug.contains(c_slug.as_str())
                || (!c_text_slug.is_empty()
                    && (c_text_slug == target_slug
                        || c_text_slug.contains(&target_slug)
                        || target_slug.contains(&c_text_slug)))
        })
        .collect();

    let pool = if relevant.is_empty() {
        candidates.iter().collect::<Vec<_>>()
    } else {
        relevant
    };

    if let Some(target_y) = target_year {
        // Exact year match (diff == 0)
        if let Some(exact) = pool.iter().find(|c| c.year == Some(target_y)) {
            return Some(exact.slug.clone());
        }

        // Tolerance match (+/- 1 year)
        if let Some(tol) = pool.iter().find(|c| {
            matches_year_tolerance(c.year, Some(target_y), 1)
        }) {
            return Some(tol.slug.clone());
        }

        // Slug ends with target year
        let year_str = target_y.to_string();
        if let Some(slug_m) = pool.iter().find(|c| c.slug.ends_with(&year_str)) {
            return Some(slug_m.slug.clone());
        }

        // Candidate with no year specified (un-versioned base show) matching title
        if let Some(no_year) = pool.iter().find(|c| {
            c.year.is_none()
                && (c.slug == target_slug
                    || c.slug.replace("the-", "") == target_slug.replace("the-", ""))
        }) {
            return Some(no_year.slug.clone());
        }

        if let Some(no_year) = pool.iter().find(|c| c.year.is_none()) {
            return Some(no_year.slug.clone());
        }
    } else {
        // No year requested: exact title first
        if let Some(exact) = pool.iter().find(|c| c.slug == target_slug) {
            return Some(exact.slug.clone());
        }
        if let Some(no_year) = pool.iter().find(|c| c.year.is_none()) {
            return Some(no_year.slug.clone());
        }
    }

    pool.first().map(|c| c.slug.clone())
}

/// Scrape BSTSrs for episode streams.
pub async fn scrape(
    _tmdb_id: &str,
    kind: MediaKind,
    season: u32,
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
        "[BSTSrs] Scraping \"{title_str}\" (slug: {slug}) S{season:02}E{episode:02} (year: {:?})",
        year
    );

    // Build candidate slugs: probe {slug}-{year} before un-versioned {slug}
    let mut candidate_slugs = Vec::new();
    if let Some(y) = year {
        let year_suffix = format!("-{y}");
        if slug.ends_with(&year_suffix) {
            candidate_slugs.push(slug.clone());
            let unversioned = slug[..slug.len() - year_suffix.len()].trim_end_matches('-').to_string();
            if !unversioned.is_empty() {
                candidate_slugs.push(unversioned);
            }
        } else {
            candidate_slugs.push(format!("{slug}-{y}"));
            candidate_slugs.push(slug.clone());
        }
    } else {
        candidate_slugs.push(slug.clone());
    }

    // Probe candidate slugs sequentially
    for candidate in &candidate_slugs {
        let primary_path = build_episode_path(candidate, kind, season, episode);
        let primary_url = format!("{BASE}{primary_path}");

        println!("[BSTSrs] Fetching {primary_url}");

        if let Some(html) = get_text_with(
            &crate::http::GIGA,
            &primary_url,
            Duration::from_secs(8),
            &[
                ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
                ("Referer", format!("{BASE}/").as_str()),
            ],
        )
        .await
        {
            let src = extract_sources_from_html(&html);
            if !src.is_empty() {
                println!(
                    "[BSTSrs] ✅ Found {} source(s) via direct probe of slug '{candidate}'",
                    src.len()
                );
                return ProviderResult::some_if_any(NAME, ID, src);
            }
        }
    }

    // Direct fetch failed or yielded no sources: fallback search
    println!("[BSTSrs] Direct probes failed; trying search fallback");
    let sources = try_fallback_search(title_str, kind, season, episode, year).await?;
    println!("[BSTSrs] ✅ Found {} source(s) via search fallback", sources.len());
    ProviderResult::some_if_any(NAME, ID, sources)
}

async fn try_fallback_search(
    title: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
    year: Option<u32>,
) -> Option<Vec<Source>> {
    println!("[BSTSrs] Attempting search fallback for \"{title}\"");
    let searched_slug = search_show_slug(title, year).await?;
    let path = build_episode_path(&searched_slug, kind, season, episode);
    let url = format!("{BASE}{path}");

    println!("[BSTSrs] Fallback fetching {url}");
    let html = get_text_with(
        &crate::http::GIGA,
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
    fn slugify_handles_various_titles() {
        assert_eq!(slugify("Breaking Bad"), "breaking-bad");
        assert_eq!(
            slugify("Death of the Pastor's Wife"),
            "death-of-the-pastors-wife"
        );
        assert_eq!(
            slugify("The Lord of the Rings: The Rings of Power"),
            "the-lord-of-the-rings-the-rings-of-power"
        );
        assert_eq!(slugify("Mr. Robot"), "mr-robot");
        assert_eq!(slugify("9-1-1: Lone Star"), "9-1-1-lone-star");
        assert_eq!(slugify("  Stranger Things  "), "stranger-things");
    }

    #[test]
    fn builds_correct_episode_paths() {
        assert_eq!(
            build_episode_path("breaking-bad", MediaKind::Tv, 1, 1),
            "/show/breaking-bad-s01e01/season/1/episode/1"
        );
        assert_eq!(
            build_episode_path("breaking-bad", MediaKind::Tv, 5, 14),
            "/show/breaking-bad-s05e14/season/5/episode/14"
        );
        assert_eq!(
            build_episode_path("inception", MediaKind::Movie, 1, 1),
            "/show/inception-movie/season/1/episode/1"
        );
    }

    #[test]
    fn decodes_dbneg_known_vector() {
        // Encode "https://voe.sx/e/v12345" with default offset 0x19e0c889e
        let plain = "https://voe.sx/e/v12345";
        let offset = DEFAULT_OFFSET;
        let encoded = plain
            .chars()
            .map(|c| format!("{:x}", (c as u64) + offset))
            .collect::<Vec<_>>()
            .join("-");

        let decoded = decode_dbneg(&encoded, offset);
        assert_eq!(decoded, Some(plain.to_string()));
    }

    #[test]
    fn decodes_dbneg_custom_offset() {
        let plain = "https://streamplay.to/embed-xyz987.html";
        let custom_offset: u64 = 0x1a2b3c4d5;
        let encoded = plain
            .chars()
            .map(|c| format!("{:x}", (c as u64) + custom_offset))
            .collect::<Vec<_>>()
            .join("-");

        let decoded = decode_dbneg(&encoded, custom_offset);
        assert_eq!(decoded, Some(plain.to_string()));
    }

    #[test]
    fn decodes_dbneg_handles_invalid_input() {
        assert_eq!(decode_dbneg("", DEFAULT_OFFSET), None);
        assert_eq!(decode_dbneg("   ", DEFAULT_OFFSET), None);
        assert_eq!(decode_dbneg("not-a-hex-value", DEFAULT_OFFSET), None);
        // Underflow: value smaller than offset
        assert_eq!(decode_dbneg("100-200", DEFAULT_OFFSET), None);
    }

    #[test]
    fn extracts_offset_from_html() {
        let html_custom = r#"<script>var a1 = 0x1234abcd; function dbneg(X) { ... }</script>"#;
        assert_eq!(extract_offset_from_html(html_custom), 0x1234abcd);

        let html_default = r#"<div>No offset here</div>"#;
        assert_eq!(extract_offset_from_html(html_default), DEFAULT_OFFSET);
    }

    #[test]
    fn extracts_sources_from_bstsrs_html() {
        let target_url1 = "https://voe.sx/e/1080pstream";
        let target_url2 = "https://streamplay.to/embed-abc";
        let enc1 = target_url1
            .chars()
            .map(|c| format!("{:x}", (c as u64) + DEFAULT_OFFSET))
            .collect::<Vec<_>>()
            .join("-");
        let enc2 = target_url2
            .chars()
            .map(|c| format!("{:x}", (c as u64) + DEFAULT_OFFSET))
            .collect::<Vec<_>>()
            .join("-");

        let html = format!(
            r#"
            <div class="ep-servers">
                <a class="embed-selector" onclick="window.open(dbneg('{enc1}'), '_blank');return false;">
                    <strong>voe.sx</strong> <span class="vris1080">FHD 1080</span>
                </a>
                <a class="embed-selector" onclick="window.open(dbneg('{enc2}'), '_blank');return false;">
                    <strong>streamplay.to</strong>
                </a>
                <iframe src="https://vidmoly.me/embed-direct123.html"></iframe>
            </div>
            "#
        );

        let sources = extract_sources_from_html(&html);
        assert_eq!(sources.len(), 3);

        // Sources sorted descending by quality: 1080p first!
        assert_eq!(sources[0].url, target_url1);
        assert_eq!(sources[0].quality, "1080p");
        assert_eq!(sources[0].is_embed, Some(true));
        assert_eq!(sources[0].provider.as_deref(), Some("BSTSrs"));
        assert_eq!(sources[0].provider_id.as_deref(), Some("bstsrs"));

        assert_eq!(sources[1].url, target_url2);
        assert_eq!(sources[1].quality, "720p");
        assert_eq!(sources[1].is_embed, Some(true));

        assert_eq!(sources[2].url, "https://vidmoly.me/embed-direct123.html");
        assert_eq!(sources[2].quality, "720p");
        assert_eq!(sources[2].is_embed, Some(true));
    }

    #[test]
    fn parses_search_show_slug_from_html() {
        let html = r#"
        <div class="search-results">
            <div class="result-item">
                <a href="https://bstsrs.in/show/breaking-bad">Breaking Bad</a>
            </div>
            <div class="result-item">
                <a href="/show/better-call-saul">Better Call Saul</a>
            </div>
        </div>
        "#;

        let slug = parse_search_show_slug(html, "Breaking Bad", None);
        assert_eq!(slug, Some("breaking-bad".to_string()));
    }

    #[test]
    fn parses_search_show_slug_with_year_disambiguation() {
        let html = r#"
        <div class="search-results">
            <div class="result-item">
                <a href="/show/avatar-the-last-airbender">Avatar: The Last Airbender (2005)</a>
            </div>
            <div class="result-item">
                <a href="/show/avatar-the-last-airbender-2024">Avatar: The Last Airbender (2024)</a>
            </div>
        </div>
        "#;
        assert_eq!(
            parse_search_show_slug(html, "Avatar: The Last Airbender", Some(2024)),
            Some("avatar-the-last-airbender-2024".to_string())
        );
        assert_eq!(
            parse_search_show_slug(html, "Avatar: The Last Airbender", Some(2005)),
            Some("avatar-the-last-airbender".to_string())
        );
    }
}
