//! NontonGo — port of `extractors/nontongo.js`.
//!
//! Unlike the table-driven providers this one doesn't publish manifests in the
//! page body. It renders a player bootstrap containing a JS `sources` array,
//! so the array is lifted out and parsed as JSON.
//!
//! Two quirks carried over from the JS:
//!   * TLS verification is off — the host serves a chain that doesn't validate,
//!     and it's unreachable otherwise.
//!   * Failure returns None here. The JS returns `{ success: false, error }`,
//!     which is truthy, so it survives the caller's `.filter(Boolean)` and is
//!     counted as a working provider. See the note in `mod.rs`.

use crate::http::{get_text_with, INSECURE};
use crate::models::{MediaKind, ProviderResult, Source};
use crate::utils::sort_sources_by_quality;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::time::Duration;

const NAME: &str = "NontonGo 🍿";
pub const ID: &str = "nontongo";
const REFERER: &str = "https://nontongo.win/";

/// One entry of the player's `sources` array.
#[derive(Debug, Deserialize)]
struct RawSource {
    file: String,
    #[serde(default)]
    label: Option<String>,
}

/// Matches either `const sources = [...]` or a `sources: [...]` property.
///
/// Stops at the first `]`, same as the JS. That would truncate a nested
/// array, but the player never emits one — entries are flat objects.
static SOURCES_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)(?:const\s+sources\s*=|sources\s*:)\s*(\[[^\]]+\])"#)
        .expect("nontongo sources regex")
});

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
) -> Option<ProviderResult> {
    if tmdb_id.is_empty() {
        return None;
    }

    println!("[NontonGo] Scraping {tmdb_id}");

    let url = if kind.is_movie() {
        format!("https://nontongo.win/stream/movie_upcloud/view1.php?id={tmdb_id}&type=movie")
    } else {
        format!("https://nontongo.win/stream/tv_upcloud/view1.php?id={tmdb_id}&s={season}&e={episode}")
    };

    let html = get_text_with(
        &INSECURE,
        &url,
        Duration::from_secs(8),
        &[
            ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
            ("Accept-Language", "en-US,en;q=0.9"),
            ("Referer", REFERER),
        ],
    )
    .await?;

    let Some(caps) = SOURCES_RE.captures(&html) else {
        println!("[NontonGo] ❌ No sources array found in view page");
        return None;
    };

    let raw: Vec<RawSource> = match serde_json::from_str(&caps[1]) {
        Ok(v) => v,
        Err(e) => {
            println!("[NontonGo] ❌ sources array did not parse: {e}");
            return None;
        }
    };

    let mut sources: Vec<Source> = raw
        .into_iter()
        .filter(|s| !s.file.is_empty())
        .map(|s| {
            let is_m3u8 = s.file.contains(".m3u8");
            let quality = s.label.unwrap_or_else(|| "auto".to_string());
            let mut src = Source::direct_m3u8(s.file, quality).tagged("NontonGo", ID);
            // The array carries progressive mp4s as well as HLS.
            src.is_m3u8 = is_m3u8;
            src
        })
        .collect();

    // The upstream JSON sources array is ordered ascending (360p -> 1080p).
    // Sort descending so highest quality (1080p) appears at index 0.
    sort_sources_by_quality(&mut sources);

    if sources.is_empty() {
        println!("[NontonGo] ❌ sources array was empty");
    } else {
        let best = sources.first().map(|s| s.quality.as_str()).unwrap_or("auto");
        println!("[NontonGo] ✅ Found {} direct sources! Best: {best}", sources.len());
    }

    ProviderResult::some_if_any(NAME, ID, sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifts_a_const_sources_array() {
        let html = r#"<script>const sources = [{"file":"https://a/x.m3u8","label":"1080p"},
                                               {"file":"https://a/y.mp4","label":"720p"}];</script>"#;
        let caps = SOURCES_RE.captures(html).expect("should match");
        let raw: Vec<RawSource> = serde_json::from_str(&caps[1]).expect("should parse");
        assert_eq!(raw.len(), 2);
        assert_eq!(raw[0].label.as_deref(), Some("1080p"));
    }

    #[test]
    fn also_matches_the_property_form() {
        let html = r#"jwplayer().setup({ sources: [{"file":"https://a/x.m3u8"}] });"#;
        let caps = SOURCES_RE.captures(html).expect("should match property form");
        let raw: Vec<RawSource> = serde_json::from_str(&caps[1]).expect("should parse");
        assert_eq!(raw[0].file, "https://a/x.m3u8");
        assert!(raw[0].label.is_none());
    }
}
