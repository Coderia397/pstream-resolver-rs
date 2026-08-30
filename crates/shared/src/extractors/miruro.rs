//! Miruro — bespoke extractor for Miruro Anime (`https://www.miruro.to`).
//!
//! Miruro is a modern anime streaming SPA that serves video streams and
//! subtitles via JSON endpoints:
//! - Episode lookup: `https://www.miruro.to/api/episodes?id={id_or_slug}`
//! - Stream playlist: `https://www.miruro.to/api/stream?id={anime_id}&ep={episode}&server={server}`
//! - Search fallback: `https://www.miruro.to/api/search?query={title}`

use crate::extractors::JSON_ACCEPT;
use crate::http::PROXY;
use crate::models::{MediaKind, ProviderResult, Source, Subtitle};
use crate::utils::sort_sources_by_quality;
use serde::Deserialize;
use std::time::Duration;

const NAME: &str = "Miruro Anime 🌸";
pub const ID: &str = "miruro";
const BASE: &str = "https://www.miruro.to";
const REFERER: &str = "https://www.miruro.to/";
const ORIGIN: &str = "https://www.miruro.to";

#[derive(Debug, Clone, Deserialize)]
pub struct MiruroStreamResponse {
    #[serde(default)]
    pub sources: Option<Vec<MiruroSourceItem>>,
    #[serde(default)]
    pub data: Option<MiruroStreamData>,
    #[serde(default)]
    pub subtitles: Option<Vec<MiruroSubtitleItem>>,
    #[serde(default)]
    pub tracks: Option<Vec<MiruroSubtitleItem>>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MiruroStreamData {
    #[serde(default)]
    pub sources: Option<Vec<MiruroSourceItem>>,
    #[serde(default)]
    pub subtitles: Option<Vec<MiruroSubtitleItem>>,
    #[serde(default)]
    pub tracks: Option<Vec<MiruroSubtitleItem>>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MiruroSourceItem {
    #[serde(default, alias = "file")]
    pub url: Option<String>,
    #[serde(default, alias = "label")]
    pub quality: Option<String>,
    #[serde(rename = "isM3U8", default)]
    pub is_m3u8: Option<bool>,
    #[serde(rename = "type", default)]
    pub source_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MiruroSubtitleItem {
    #[serde(default, alias = "file")]
    pub url: Option<String>,
    #[serde(default, alias = "language", alias = "lang")]
    pub lang: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MiruroSearchResponse {
    #[serde(default)]
    pub results: Option<Vec<MiruroSearchItem>>,
    #[serde(default)]
    pub data: Option<Vec<MiruroSearchItem>>,
    #[serde(default)]
    pub items: Option<Vec<MiruroSearchItem>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MiruroSearchItem {
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub title: Option<MiruroTitle>,
    #[serde(rename = "name", default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MiruroTitle {
    #[serde(default)]
    pub romaji: Option<String>,
    #[serde(default)]
    pub english: Option<String>,
    #[serde(default)]
    pub native: Option<String>,
    #[serde(rename = "userPreferred", default)]
    pub user_preferred: Option<String>,
}

impl MiruroSearchItem {
    pub fn get_identifier(&self) -> Option<String> {
        if let Some(ref s) = self.slug {
            if !s.trim().is_empty() {
                return Some(s.trim().to_string());
            }
        }
        if let Some(ref id) = self.id {
            if let Some(s) = id.as_str() {
                if !s.trim().is_empty() {
                    return Some(s.trim().to_string());
                }
            } else if let Some(n) = id.as_i64() {
                return Some(n.to_string());
            }
        }
        None
    }

    pub fn matches_title(&self, query: &str) -> bool {
        let q = query.to_ascii_lowercase();
        if let Some(ref name) = self.name {
            if name.to_ascii_lowercase().contains(&q) {
                return true;
            }
        }
        if let Some(ref t) = self.title {
            if t.english.as_deref().map(|s| s.to_ascii_lowercase().contains(&q)).unwrap_or(false)
                || t.romaji.as_deref().map(|s| s.to_ascii_lowercase().contains(&q)).unwrap_or(false)
                || t.user_preferred.as_deref().map(|s| s.to_ascii_lowercase().contains(&q)).unwrap_or(false)
                || t.native.as_deref().map(|s| s.contains(query)).unwrap_or(false)
            {
                return true;
            }
        }
        false
    }
}

/// Parse stream response payload and extract sources and subtitles.
pub fn parse_stream_response(resp: &MiruroStreamResponse) -> (Vec<Source>, Vec<Subtitle>) {
    let mut sources: Vec<Source> = Vec::new();
    let mut subtitles: Vec<Subtitle> = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();

    // 1. Extract sources from direct list or nested data
    let candidate_sources = resp
        .sources
        .as_deref()
        .or_else(|| resp.data.as_ref().and_then(|d| d.sources.as_deref()));

    if let Some(items) = candidate_sources {
        for item in items {
            if let Some(ref url) = item.url {
                let trimmed = url.trim();
                if trimmed.starts_with("http") && seen_urls.insert(trimmed.to_string()) {
                    let quality = item.quality.as_deref().unwrap_or("auto");
                    let is_m3u8 = item.is_m3u8.unwrap_or_else(|| {
                        trimmed.contains(".m3u8")
                            || item
                                .source_type
                                .as_deref()
                                .map(|t| t.eq_ignore_ascii_case("hls"))
                                .unwrap_or(false)
                    });

                    let mut s = if is_m3u8 {
                        Source::direct_m3u8(trimmed, quality)
                    } else {
                        Source::embed(trimmed, quality)
                    }
                    .tagged("Miruro Anime", ID)
                    .with_referer(REFERER);

                    s.is_m3u8 = is_m3u8;
                    sources.push(s);
                }
            }
        }
    }

    // Direct single url fallback
    if sources.is_empty() {
        let single_url = resp
            .url
            .as_deref()
            .or_else(|| resp.data.as_ref().and_then(|d| d.url.as_deref()));

        if let Some(u) = single_url {
            let trimmed = u.trim();
            if trimmed.starts_with("http") && seen_urls.insert(trimmed.to_string()) {
                let is_m3u8 = trimmed.contains(".m3u8");
                let mut s = if is_m3u8 {
                    Source::direct_m3u8(trimmed, "1080p")
                } else {
                    Source::embed(trimmed, "1080p")
                }
                .tagged("Miruro Anime", ID)
                .with_referer(REFERER);
                s.is_m3u8 = is_m3u8;
                sources.push(s);
            }
        }
    }

    // 2. Extract subtitles / tracks
    let candidate_subs = resp
        .subtitles
        .as_deref()
        .or_else(|| resp.tracks.as_deref())
        .or_else(|| resp.data.as_ref().and_then(|d| d.subtitles.as_deref()))
        .or_else(|| resp.data.as_ref().and_then(|d| d.tracks.as_deref()));

    if let Some(subs) = candidate_subs {
        for sub in subs {
            if let Some(ref url) = sub.url {
                let trimmed_url = url.trim();
                if trimmed_url.starts_with("http") {
                    let lang = sub
                        .lang
                        .as_deref()
                        .or(sub.label.as_deref())
                        .unwrap_or("English")
                        .to_string();
                    let label = sub.label.as_deref().unwrap_or(&lang).to_string();
                    subtitles.push(Subtitle {
                        url: trimmed_url.to_string(),
                        lang,
                        label,
                    });
                }
            }
        }
    }

    // Sort sources by quality descending (1080p > 720p > 480p > 360p > auto)
    sort_sources_by_quality(&mut sources);

    (sources, subtitles)
}

/// Search Miruro API for anime identifier (ID or slug).
pub async fn search_anime(query: &str) -> Option<String> {
    let encoded = urlencoding::encode(query);
    let search_url = format!("{BASE}/api/search?query={encoded}");

    let resp = PROXY
        .get(&search_url)
        .header("Referer", REFERER)
        .header("Origin", ORIGIN)
        .header("Accept", JSON_ACCEPT)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let search_res: MiruroSearchResponse = resp.json().await.ok()?;
    let items = search_res
        .results
        .or(search_res.data)
        .or(search_res.items)?;

    // Try finding exact title match first, or fall back to first item
    let matched = items
        .iter()
        .find(|item| item.matches_title(query))
        .or_else(|| items.first())?;

    matched.get_identifier()
}

/// Query Miruro stream API for an anime ID/slug and episode.
pub async fn fetch_stream(anime_id: &str, episode: u32) -> Option<MiruroStreamResponse> {
    let servers = ["vidstream", "megacloud", "streamwish", ""];

    for server in servers {
        let stream_url = if server.is_empty() {
            format!("{BASE}/api/stream?id={anime_id}&ep={episode}")
        } else {
            format!("{BASE}/api/stream?id={anime_id}&ep={episode}&server={server}")
        };

        let req = PROXY
            .get(&stream_url)
            .header("Referer", REFERER)
            .header("Origin", ORIGIN)
            .header("Accept", JSON_ACCEPT)
            .timeout(Duration::from_secs(8));

        if let Ok(resp) = req.send().await {
            if resp.status().is_success() {
                if let Ok(data) = resp.json::<MiruroStreamResponse>().await {
                    let has_sources = data
                        .sources
                        .as_ref()
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
                        || data
                            .data
                            .as_ref()
                            .and_then(|d| d.sources.as_ref())
                            .map(|s| !s.is_empty())
                            .unwrap_or(false)
                        || data.url.is_some()
                        || data.data.as_ref().and_then(|d| d.url.as_ref()).is_some();

                    if has_sources {
                        return Some(data);
                    }
                }
            }
        }
    }

    None
}

/// Scrape Miruro anime streams.
pub async fn scrape(
    tmdb_id: &str,
    _kind: MediaKind,
    _season: u32,
    episode: u32,
    title: Option<&str>,
) -> Option<ProviderResult> {
    let ep = if episode == 0 { 1 } else { episode };

    println!("[Miruro] Scraping TMDB: {tmdb_id}, Title: {:?}, Ep: {ep}", title);

    // 1. If title is provided, search anime ID / slug
    let mut anime_identifier: Option<String> = None;
    if let Some(t) = title {
        if !t.trim().is_empty() {
            anime_identifier = search_anime(t.trim()).await;
        }
    }

    // 2. If no title search match, try direct tmdb_id as anime ID
    let id_to_query = anime_identifier.as_deref().unwrap_or(tmdb_id);

    // 3. Fetch streaming playlist
    let stream_resp = fetch_stream(id_to_query, ep).await?;
    let (sources, subtitles) = parse_stream_response(&stream_resp);

    if sources.is_empty() {
        println!("[Miruro] ❌ No sources in stream response for id: {id_to_query}");
        return None;
    }

    println!("[Miruro] ✅ Found {} source(s), {} subtitle(s)", sources.len(), subtitles.len());

    let mut result = ProviderResult::new(NAME, ID, sources);
    result.subtitles = subtitles;
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stream_response_with_direct_sources() {
        let json_data = r#"{
            "sources": [
                {
                    "file": "https://cdn.miruro.to/hls/1080p/index.m3u8",
                    "label": "1080p",
                    "isM3U8": true,
                    "type": "hls"
                },
                {
                    "file": "https://cdn.miruro.to/hls/720p/index.m3u8",
                    "label": "720p",
                    "isM3U8": true,
                    "type": "hls"
                }
            ],
            "subtitles": [
                {
                    "file": "https://cdn.miruro.to/subs/en.vtt",
                    "lang": "English",
                    "label": "English"
                }
            ]
        }"#;

        let resp: MiruroStreamResponse = serde_json::from_str(json_data).unwrap();
        let (sources, subs) = parse_stream_response(&resp);

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].quality, "1080p");
        assert_eq!(sources[0].url, "https://cdn.miruro.to/hls/1080p/index.m3u8");
        assert!(sources[0].is_m3u8);
        assert_eq!(sources[0].provider.as_deref(), Some("Miruro Anime"));
        assert_eq!(sources[0].provider_id.as_deref(), Some("miruro"));

        assert_eq!(sources[1].quality, "720p");
        assert_eq!(sources[1].url, "https://cdn.miruro.to/hls/720p/index.m3u8");

        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].lang, "English");
        assert_eq!(subs[0].url, "https://cdn.miruro.to/subs/en.vtt");
    }

    #[test]
    fn parses_stream_response_with_nested_data() {
        let json_data = r#"{
            "data": {
                "sources": [
                    {
                        "file": "https://stream.server.org/video-720p.m3u8",
                        "label": "720p"
                    },
                    {
                        "file": "https://stream.server.org/video-1080p.m3u8",
                        "label": "1080p"
                    }
                ],
                "tracks": [
                    {
                        "file": "https://stream.server.org/sub-ja.vtt",
                        "language": "Japanese",
                        "label": "Japanese (CC)"
                    }
                ]
            }
        }"#;

        let resp: MiruroStreamResponse = serde_json::from_str(json_data).unwrap();
        let (sources, subs) = parse_stream_response(&resp);

        // Sorting check: 1080p should come before 720p
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].quality, "1080p");
        assert_eq!(sources[1].quality, "720p");

        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].lang, "Japanese");
        assert_eq!(subs[0].label, "Japanese (CC)");
    }

    #[test]
    fn parses_search_response_and_matches_title() {
        let json_data = r#"{
            "results": [
                {
                    "id": 1429,
                    "slug": "shingeki-no-kyojin",
                    "title": {
                        "romaji": "Shingeki no Kyojin",
                        "english": "Attack on Titan",
                        "native": "進撃の巨人"
                    }
                },
                {
                    "id": 95479,
                    "slug": "jujutsu-kaisen",
                    "title": {
                        "romaji": "Jujutsu Kaisen",
                        "english": "Jujutsu Kaisen",
                        "native": "呪術廻戦"
                    }
                }
            ]
        }"#;

        let resp: MiruroSearchResponse = serde_json::from_str(json_data).unwrap();
        let items = resp.results.unwrap();

        let aot = items.iter().find(|i| i.matches_title("Attack on Titan")).unwrap();
        assert_eq!(aot.get_identifier(), Some("shingeki-no-kyojin".to_string()));

        let jjk = items.iter().find(|i| i.matches_title("Jujutsu Kaisen")).unwrap();
        assert_eq!(jjk.get_identifier(), Some("jujutsu-kaisen".to_string()));
    }
}
