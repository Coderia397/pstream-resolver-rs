//! Trailer search — port of `youtubeSearch` in `local-resolver/server.mjs`.
//!
//! The frontend used to call the YouTube Data API directly, which meant
//! shipping an API key in the browser bundle where anyone could read it (Vite
//! inlines every `VITE_*` value). Doing the search here instead means no key
//! ever reaches a visitor — and this scrapes YouTube's own results page rather
//! than using the Data API, so there is no key to leak in the first place and
//! no quota to exhaust.
//!
//! Title and channel come back alongside the id because the frontend scores
//! candidates on those to pick the best-matching trailer.

use crate::http::{get_text_with, GIGA};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::time::Duration;

#[derive(Debug, Serialize)]
pub struct Video {
    #[serde(rename = "videoId")]
    pub video_id: String,
    pub title: String,
    #[serde(rename = "channelTitle")]
    pub channel_title: String,
}

/// Results are embedded as `ytInitialData` JSON inside the page. Each hit is a
/// `videoRenderer` whose id, title and owner sit in a known order.
static RENDERER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#""videoRenderer":\{"videoId":"([A-Za-z0-9_-]{11})"(.*?)"ownerText":\{"runs":\[\{"text":"(.*?)""#,
    )
    .expect("videoRenderer regex")
});

static TITLE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#""title":\{"runs":\[\{"text":"(.*?)""#).expect("title regex"));

/// Unescape a JSON string body (`&`, `\"`, …) by parsing it as one.
fn decode(s: &str) -> String {
    serde_json::from_str::<String>(&format!("\"{s}\"")).unwrap_or_else(|_| s.to_string())
}

pub async fn search(query: &str, max_results: usize) -> Vec<Video> {
    // sp=EgIQAQ%3D%3D filters to videos only, excluding channels and playlists.
    let url = format!(
        "https://www.youtube.com/results?search_query={}&sp=EgIQAQ%3D%3D",
        urlencoding::encode(query)
    );

    let Some(text) = get_text_with(
        &GIGA,
        &url,
        Duration::from_secs(12),
        &[
            ("Accept", "text/html,application/xhtml+xml"),
            ("Accept-Language", "en-US,en;q=0.9"),
        ],
    )
    .await
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for caps in RENDERER_RE.captures_iter(&text) {
        if out.len() >= max_results {
            break;
        }
        let video_id = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        if video_id.is_empty() || !seen.insert(video_id.to_string()) {
            continue;
        }

        // The title lives in the chunk between the id and ownerText.
        let middle = caps.get(2).map(|m| m.as_str()).unwrap_or_default();
        let title = TITLE_RE
            .captures(middle)
            .and_then(|c| c.get(1))
            .map(|m| decode(m.as_str()))
            .unwrap_or_default();

        out.push(Video {
            video_id: video_id.to_string(),
            title,
            channel_title: caps.get(3).map(|m| decode(m.as_str())).unwrap_or_default(),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_json_escapes_in_titles() {
        assert_eq!(decode(r"Fight Club & Friends"), "Fight Club & Friends");
        assert_eq!(decode(r#"He said \"hi\""#), "He said \"hi\"");
        // Invalid escapes fall back to the raw text rather than panicking.
        assert_eq!(decode(r"bad \q escape"), r"bad \q escape");
    }

    #[test]
    fn extracts_id_title_and_channel() {
        let page = r#"{"videoRenderer":{"videoId":"dQw4w9WgXcQ","title":{"runs":[{"text":"Some Trailer"}]},"ownerText":{"runs":[{"text":"A Channel"}]}}}"#;
        let caps = RENDERER_RE.captures(page).expect("should match");
        assert_eq!(caps.get(1).unwrap().as_str(), "dQw4w9WgXcQ");
        assert_eq!(caps.get(3).unwrap().as_str(), "A Channel");
        let title = TITLE_RE.captures(caps.get(2).unwrap().as_str()).unwrap();
        assert_eq!(title.get(1).unwrap().as_str(), "Some Trailer");
    }
}
