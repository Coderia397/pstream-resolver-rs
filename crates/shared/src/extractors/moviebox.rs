//! MovieBox — two-stage extractor querying backend search BFF API,
//! fetching detail page HTML, and extracting Nuxt 3 hydration streams.
//!
//! MovieBox is a Nuxt 3 SPA that searches via a backend BFF API and renders
//! video streams on individual movie detail pages.
//!
//! Resolution workflow:
//! 1. POST title query to backend search API (`https://h5-api.aoneroom.com/wefeed-h5api-bff/subject/search`).
//! 2. Parse `data.items` JSON array to extract `detailPath`.
//! 3. Fetch detail page HTML (`https://movieboxonline.net/movies/{detailPath}`).
//! 4. Extract `.m3u8` or `.mp4` stream URLs from Nuxt 3 hydration data or HTML.

use crate::extractors::{find_m3u8_urls, JSON_ACCEPT};
use crate::http::{get_text_with, GIGA};
use crate::models::{ProviderResult, Source};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::time::Duration;

const NAME: &str = "MovieBox 🍿";
pub const ID: &str = "moviebox";
const SEARCH_API: &str = "https://h5-api.aoneroom.com/wefeed-h5api-bff/subject/search";
const SEARCH_ORIGIN: &str = "https://h5.aoneroom.com";
const SEARCH_REFERER: &str = "https://h5.aoneroom.com/";
const DETAIL_BASE: &str = "https://movieboxonline.net/movies";
const REFERER: &str = "https://movieboxonline.net/";

/// Matches Nuxt 3 `<script id="__NUXT_DATA__">` JSON payloads or legacy Nuxt 2 `window.__NUXT__=` state.
static NUXT_DATA_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)(?:<script[^>]*id="__NUXT_DATA__"[^>]*>|window\.__NUXT__\s*=\s*)(.*?)(?:</script>|;\s*</script>)"#)
        .expect("nuxt data regex")
});

/// MovieBox media files on Aoneroom CDNs (pbcdn, pbcdnw, macdn, etc.) ending in .m3u8 or .mp4.
/// Strictly excludes image files (.jpg, .jpeg, .png, .webp).
static AONEROOM_MEDIA_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)https?://(?:[a-zA-Z0-9-]+\.)?aoneroom\.com[^\s"'`<>\\]*?\.(?:m3u8|mp4)(?:[?#][^\s"'`<>\\]*)?"#)
        .expect("aoneroom media regex")
});

static MP4_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)https?://[^\s"'`<>\\]+?\.mp4(?:[?#][^\s"'`<>\\]*)?"#).expect("mp4 regex")
});

static ANY_MEDIA_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)https?://[^\s"'`<>\\]+?\.(?:m3u8|mp4)(?:[?#][^\s"'`<>\\]*)?"#)
        .expect("any media regex")
});

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub data: Option<SearchData>,
}

#[derive(Debug, Deserialize)]
pub struct SearchData {
    #[serde(default)]
    pub items: Option<Vec<SearchItem>>,
}

#[derive(Debug, Deserialize)]
pub struct SearchItem {
    #[serde(rename = "subjectId", default)]
    pub subject_id: Option<serde_json::Value>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "detailPath", default)]
    pub detail_path: Option<String>,
    #[serde(rename = "releaseDate", default)]
    pub release_date: Option<String>,
    #[serde(rename = "subjectType", default)]
    pub subject_type: Option<i32>,
}

/// Extract a valid video stream URL (.m3u8 or .mp4) from the MovieBox detail page HTML,
/// inspecting Nuxt 3 hydration data (__NUXT_DATA__), legacy Nuxt 2 state, direct M3U8s,
/// and fallback regexes while ignoring static image assets.
pub fn extract_stream_from_html(html: &str) -> Option<String> {
    // Helper to find all Aoneroom media
    let mut all_media = Vec::new();
    
    // 1. Nuxt 3 hydration payload or Nuxt 2 window.__NUXT__
    if let Some(nuxt) = NUXT_DATA_RE.captures(html).and_then(|c| c.get(1)) {
        let unescaped = nuxt.as_str().replace(r"\/", "/").replace(r"\\", "\\");
        for m in AONEROOM_MEDIA_RE.find_iter(&unescaped) {
            all_media.push(m.as_str().to_string());
        }
        for m in find_m3u8_urls(&unescaped) {
            all_media.push(m);
        }
    }

    for m in find_m3u8_urls(html) {
        all_media.push(m);
    }
    for m in AONEROOM_MEDIA_RE.find_iter(html) {
        all_media.push(m.as_str().to_string());
    }
    for m in MP4_RE.find_iter(html) {
        all_media.push(m.as_str().to_string());
    }

    // PRIORITY: Return the first .m3u8 found (main feature). 
    // If none exists, fallback to .mp4 (which is likely just a trailer).
    if let Some(m3u8) = all_media.iter().find(|url| url.contains(".m3u8")) {
        return Some(m3u8.clone());
    }
    if let Some(mp4) = all_media.iter().find(|url| url.contains(".mp4")) {
        return Some(mp4.clone());
    }

    None
}

pub async fn scrape(title: &str, year: Option<u32>) -> Option<ProviderResult> {
    if title.is_empty() {
        return None;
    }

    println!(
        "[MovieBox] Searching \"{title}\" ({})",
        year.map(|y| y.to_string()).unwrap_or_default()
    );

    // Step 1: Query Search BFF API
    let search_body = serde_json::json!({
        "keyword": title,
        "page": 1,
        "perPage": 10,
        "subjectType": 0
    });

    let resp = GIGA
        .post(SEARCH_API)
        .header("Content-Type", "application/json")
        .header("Accept", JSON_ACCEPT)
        .header("Origin", SEARCH_ORIGIN)
        .header("Referer", SEARCH_REFERER)
        .timeout(Duration::from_secs(8))
        .json(&search_body)
        .send()
        .await
        .ok()?;

    let search_res: SearchResponse = resp.json().await.ok()?;
    let items = search_res.data?.items?;
    if items.is_empty() {
        println!("[MovieBox] ❌ No search results found for \"{title}\"");
        return None;
    }

    // Step 2: Match item by title/year or pick first valid item
    let valid_items: Vec<&SearchItem> = items
        .iter()
        .filter(|item| {
            item.detail_path
                .as_ref()
                .map(|p| !p.trim().is_empty())
                .unwrap_or(false)
        })
        .collect();

    if valid_items.is_empty() {
        println!("[MovieBox] ❌ No search items with valid detailPath found for \"{title}\"");
        return None;
    }

    let chosen_item = if let Some(y) = year {
        let year_str = y.to_string();
        valid_items
            .iter()
            .find(|item| {
                item.title
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case(title))
                    .unwrap_or(false)
                    && item
                        .release_date
                        .as_deref()
                        .map(|d| d.starts_with(&year_str))
                        .unwrap_or(false)
            })
            .or_else(|| {
                valid_items
                    .iter()
                    .find(|item| {
                        item.title
                            .as_deref()
                            .map(|t| t.eq_ignore_ascii_case(title))
                            .unwrap_or(false)
                    })
            })
    } else {
        valid_items
            .iter()
            .find(|item| {
                item.title
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case(title))
                    .unwrap_or(false)
            })
    }
    .copied()
    .or_else(|| valid_items.first().copied())?;

    let detail_path = chosen_item.detail_path.as_ref()?;
    let detail_url = format!("{DETAIL_BASE}/{}", detail_path.trim_start_matches('/'));

    println!("[MovieBox] Fetching detail page: {detail_url}");

    // Step 3: Fetch Detail Page HTML
    let html = get_text_with(
        &GIGA,
        &detail_url,
        Duration::from_secs(10),
        &[
            ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
            ("Accept-Language", "en-US,en;q=0.9"),
            ("Referer", REFERER),
        ],
    )
    .await?;

    // Step 4: Stream Extraction
    let stream_url = extract_stream_from_html(&html)?;
    let is_m3u8 = stream_url.contains(".m3u8");

    println!("[MovieBox] ✅ Found Stream URL: {stream_url} (is_m3u8: {is_m3u8})");

    let mut source = Source::direct_m3u8(&stream_url, "1080p")
        .tagged("MovieBox", ID)
        .with_referer(REFERER);
    source.is_m3u8 = is_m3u8;

    Some(ProviderResult::new(NAME, ID, vec![source]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_m3u8_from_nuxt3_hydration_payload() {
        let html = r#"
        <!DOCTYPE html>
        <html>
        <head><title>Movie Detail</title></head>
        <body>
        <div id="__nuxt"></div>
        <script type="application/json" data-nuxt-data="nuxt-app" data-ssr="true" id="__NUXT_DATA__">
        [["ShallowReactive",1],{"data":2},"https:\/\/pbcdn.aoneroom.com\/media\/hls\/master.m3u8?token=abc"]
        </script>
        </body>
        </html>
        "#;

        let stream = extract_stream_from_html(html);
        assert_eq!(
            stream,
            Some("https://pbcdn.aoneroom.com/media/hls/master.m3u8?token=abc".to_string())
        );
    }

    #[test]
    fn extracts_mp4_from_nuxt3_with_macdn() {
        let html = r#"
        <div id="teleports"></div>
        <script type="application/json" data-nuxt-data="nuxt-app" data-ssr="true" id="__NUXT_DATA__">
        ["ShallowReactive",{"state":1},"https:\/\/macdn.aoneroom.com\/media\/vone\/2025\/06\/14\/311ec6fd9f018d78ee89a590f15541b1-sd.mp4"]
        </script>
        "#;

        let stream = extract_stream_from_html(html);
        assert_eq!(
            stream,
            Some("https://macdn.aoneroom.com/media/vone/2025/06/14/311ec6fd9f018d78ee89a590f15541b1-sd.mp4".to_string())
        );
    }

    #[test]
    fn ignores_image_urls_on_pbcdnw() {
        let html = r#"
        <script type="application/json" id="__NUXT_DATA__">
        [
            "https:\/\/pbcdnw.aoneroom.com\/image\/2026\/01\/27\/ca253305a555591c771c8122e2cfd051.jpg",
            "https:\/\/pbcdnw.aoneroom.com\/image\/2023\/11\/20\/75470511da6841fcc06445636e99283c.jpeg",
            "https:\/\/pbcdnw.aoneroom.com\/covers\/hero.webp",
            "https:\/\/macdn.aoneroom.com\/media\/vone\/2025\/06\/14\/video-sd.mp4"
        ]
        </script>
        "#;

        let stream = extract_stream_from_html(html);
        assert_eq!(
            stream,
            Some("https://macdn.aoneroom.com/media/vone/2025/06/14/video-sd.mp4".to_string())
        );
    }

    #[test]
    fn extracts_legacy_nuxt2_state() {
        let html = r#"<script>window.__NUXT__={data:[{u:"https://pbcdn.aoneroom.com/v/abc.mp4"}]};</script>"#;
        let stream = extract_stream_from_html(html);
        assert_eq!(stream, Some("https://pbcdn.aoneroom.com/v/abc.mp4".to_string()));
    }

    #[test]
    fn falls_back_to_raw_html_m3u8() {
        let html = r#"
        <div class="player">
            <video src="https://cdn.example.org/live/playlist.m3u8"></video>
        </div>
        "#;
        let stream = extract_stream_from_html(html);
        assert_eq!(stream, Some("https://cdn.example.org/live/playlist.m3u8".to_string()));
    }

    #[test]
    fn falls_back_to_raw_html_mp4() {
        let html = r#"
        <div class="player">
            <source src="https://cdn.example.org/videos/movie_1080p.mp4" type="video/mp4">
        </div>
        "#;
        let stream = extract_stream_from_html(html);
        assert_eq!(stream, Some("https://cdn.example.org/videos/movie_1080p.mp4".to_string()));
    }

    #[test]
    fn parses_search_response_and_finds_detail_path() {
        let json_data = r#"{
            "code": 0,
            "message": "ok",
            "data": {
                "items": [
                    {
                        "subjectId": "6047437085185823776",
                        "title": "Inception",
                        "releaseDate": "2010-07-16",
                        "detailPath": "inception-e1BOR6f19C7"
                    },
                    {
                        "subjectId": "1234567890",
                        "title": "Inception: The Cobol Job",
                        "releaseDate": "2010-12-07",
                        "detailPath": "inception-the-cobol-job-abc1234"
                    }
                ]
            }
        }"#;

        let resp: SearchResponse = serde_json::from_str(json_data).unwrap();
        let items = resp.data.unwrap().items.unwrap();
        let item = items
            .iter()
            .find(|i| i.title.as_deref() == Some("Inception"))
            .unwrap();
        assert_eq!(item.detail_path.as_deref(), Some("inception-e1BOR6f19C7"));
    }

    #[test]
    fn matches_best_item_by_title_and_year() {
        let items = vec![
            SearchItem {
                subject_id: None,
                title: Some("Avatar".to_string()),
                detail_path: Some("avatar-2009-slug".to_string()),
                release_date: Some("2009-12-18".to_string()),
                subject_type: Some(0),
            },
            SearchItem {
                subject_id: None,
                title: Some("Avatar: The Way of Water".to_string()),
                detail_path: Some("avatar-way-of-water-slug".to_string()),
                release_date: Some("2022-12-16".to_string()),
                subject_type: Some(0),
            },
            SearchItem {
                subject_id: None,
                title: Some("Avatar".to_string()),
                detail_path: Some("avatar-2024-rerelease-slug".to_string()),
                release_date: Some("2024-10-01".to_string()),
                subject_type: Some(0),
            },
        ];

        let target_title = "Avatar";
        let target_year = Some(2024);

        let year_str = target_year.unwrap().to_string();
        let chosen = items
            .iter()
            .find(|item| {
                item.title
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case(target_title))
                    .unwrap_or(false)
                    && item
                        .release_date
                        .as_deref()
                        .map(|d| d.starts_with(&year_str))
                        .unwrap_or(false)
            })
            .unwrap();

        assert_eq!(chosen.detail_path.as_deref(), Some("avatar-2024-rerelease-slug"));
    }

    #[test]
    fn validates_source_headers_and_metadata() {
        let url = "https://pbcdn.aoneroom.com/media/hls/master.m3u8";
        let is_m3u8 = url.contains(".m3u8");
        let mut source = Source::direct_m3u8(url, "1080p")
            .tagged("MovieBox", ID)
            .with_referer(REFERER);
        source.is_m3u8 = is_m3u8;

        let result = ProviderResult::new(NAME, ID, vec![source]);
        assert_eq!(result.provider, "MovieBox 🍿");
        assert_eq!(result.provider_id, "moviebox");
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].url, url);
        assert_eq!(result.sources[0].quality, "1080p");
        assert!(result.sources[0].is_m3u8);
        assert!(result.sources[0].no_proxy);
        assert_eq!(result.sources[0].provider.as_deref(), Some("MovieBox"));
        assert_eq!(result.sources[0].provider_id.as_deref(), Some("moviebox"));
        assert_eq!(
            result.sources[0].referer.as_deref(),
            Some("https://movieboxonline.net/")
        );
    }
}
