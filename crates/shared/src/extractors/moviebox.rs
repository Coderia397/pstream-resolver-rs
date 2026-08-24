//! MovieBox — port of `extractors/moviebox.js`.
//!
//! The odd one out: it has no TMDB lookup, so it searches by title and takes
//! whatever the results page exposes. Three strategies, in the JS's order —
//! first an HLS manifest, then a progressive mp4, then the Nuxt hydration
//! payload, which sometimes carries a CDN URL the rendered HTML doesn't.
//!
//! `year` is accepted to match the JS signature but is genuinely unused there
//! too — it only ever reached a log line, never the query.

use crate::extractors::find_m3u8_urls;
use crate::http::{get_text_with, INSECURE};
use crate::models::{ProviderResult, Source};
use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Duration;

const NAME: &str = "MovieBox 🍿";
pub const ID: &str = "moviebox";

static MP4_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)https?://[^\s"'<>]+\.mp4[^\s"'<>]*"#).expect("mp4 regex"));

/// The Nuxt payload is a JS literal, not JSON — grab it as raw text and
/// regex inside it rather than trying to parse it.
static NUXT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"window\.__NUXT__=([\s\S]*?);</script>"#).expect("nuxt regex"));

/// MovieBox fronts its media on this CDN; prefer it over a generic match.
static CDN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)https?://pbcdn\.aoneroom\.com[^\s"'`)]+"#).expect("cdn regex")
});

static ANY_MEDIA_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)https?://[^\s"'`)]+\.(?:m3u8|mp4)[^\s"'`)]*"#).expect("media regex")
});

pub async fn scrape(title: &str, year: Option<u32>) -> Option<ProviderResult> {
    if title.is_empty() {
        return None;
    }

    println!(
        "[MovieBox] Searching \"{title}\" ({})",
        year.map(|y| y.to_string()).unwrap_or_default()
    );

    let url = format!(
        "https://movieboxonline.net/search-result?keyword={}",
        urlencoding::encode(title)
    );

    let html = get_text_with(
        &INSECURE,
        &url,
        Duration::from_secs(8),
        &[
            ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
            ("Accept-Language", "en-US,en;q=0.9"),
        ],
    )
    .await?;

    // 1. A manifest sitting in the page.
    if let Some(m3u8) = find_m3u8_urls(&html).into_iter().next() {
        println!("[MovieBox] ✅ Found M3U8: {m3u8}");
        return Some(ProviderResult::new(
            NAME,
            ID,
            vec![Source::direct_m3u8(m3u8, "1080p").tagged("MovieBox", ID)],
        ));
    }

    // 2. A progressive file instead.
    if let Some(mp4) = MP4_RE.find(&html) {
        println!("[MovieBox] ✅ Found MP4: {}", mp4.as_str());
        let mut src = Source::direct_m3u8(mp4.as_str(), "1080p").tagged("MovieBox", ID);
        src.is_m3u8 = false;
        return Some(ProviderResult::new(NAME, ID, vec![src]));
    }

    // 3. Nothing rendered — look inside the hydration payload.
    if let Some(nuxt) = NUXT_RE.captures(&html).and_then(|c| c.get(1)) {
        let state = nuxt.as_str();
        let found = CDN_RE
            .find(state)
            .or_else(|| ANY_MEDIA_RE.find(state))
            .map(|m| m.as_str().to_string());

        if let Some(media) = found {
            println!("[MovieBox] ✅ Found CDN Media URL in Nuxt State: {media}");
            let is_m3u8 = media.contains(".m3u8");
            let mut src = Source::direct_m3u8(media, "1080p").tagged("MovieBox", ID);
            src.is_m3u8 = is_m3u8;
            return Some(ProviderResult::new(NAME, ID, vec![src]));
        }
    }

    println!("[MovieBox] ❌ No direct stream URL in search payload");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulls_media_url_out_of_nuxt_state() {
        let html = r#"<script>window.__NUXT__={data:[{u:"https://pbcdn.aoneroom.com/v/abc.mp4"}]};</script>"#;
        let state = NUXT_RE.captures(html).unwrap().get(1).unwrap().as_str();
        let hit = CDN_RE.find(state).unwrap().as_str();
        assert_eq!(hit, "https://pbcdn.aoneroom.com/v/abc.mp4");
    }

    #[test]
    fn falls_back_to_any_media_url_when_cdn_is_absent() {
        let state = r#"{"x":"https://other.example/v/clip.m3u8?t=1"}"#;
        assert!(CDN_RE.find(state).is_none());
        assert_eq!(
            ANY_MEDIA_RE.find(state).unwrap().as_str(),
            "https://other.example/v/clip.m3u8?t=1"
        );
    }
}
