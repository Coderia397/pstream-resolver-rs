//! VixSrc — port of `resolveVixSrc` in `local-resolver/server.mjs`.
//!
//! Two-step mint. The token on the `/embed/` URL is a ~10 second credential
//! for the embed page only; the token that actually signs the playlist (~60
//! day life) is published as `window.masterPlaylist.params` *inside* that
//! page. So: fetch the API for the embed URL, fetch the embed page, lift the
//! real token out of it, and build the playlist URL from that.
//!
//! The result is verified before being returned. VixSrc lists titles it can't
//! actually serve, and those 403 at playlist fetch — returning a dead URL
//! would starve the frontend's fallback, which only advances when a source
//! fails at playback time.

use crate::http::{get_text_with, GIGA};
use crate::models::{MediaKind, ProviderResult, Source};
use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Duration;

const BASE: &str = "https://vixsrc.to";
const NAME: &str = "VixSrc ⚡";
pub const ID: &str = "vixsrc";

/// How much of the page to search after the `masterPlaylist` anchor. The
/// params block sits immediately after it; a wider window risks matching a
/// later, unrelated `token:`.
const BLOCK_LEN: usize = 1500;

static TOKEN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"['"]token['"]\s*:\s*['"]([^'"]+)['"]"#).expect("token regex"));
static EXPIRES_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"['"]expires['"]\s*:\s*['"]?(\d{9,})['"]?"#).expect("expires regex"));
static PLAYLIST_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"url:\s*['"]([^'"]*/playlist/[^'"]*)['"]"#).expect("playlist regex")
});

fn headers() -> Vec<(&'static str, &'static str)> {
    vec![("Referer", "https://vixsrc.to/"), ("Origin", BASE)]
}

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
) -> Option<ProviderResult> {
    let api_path = if kind.is_movie() {
        format!("/api/movie/{tmdb_id}")
    } else {
        format!("/api/tv/{tmdb_id}/{season}/{episode}")
    };

    println!("[VixSrc] Probing {BASE}{api_path}");

    // 1. Ask the API where the embed lives.
    let mut h = headers();
    h.push(("Accept", "application/json"));
    let api_body = get_text_with(&GIGA, &format!("{BASE}{api_path}"), Duration::from_secs(10), &h).await?;

    let api: serde_json::Value = serde_json::from_str(&api_body).ok()?;
    let src = api.get("src")?.as_str()?;
    let embed_url = if src.starts_with("http") {
        src.to_string()
    } else {
        format!("{BASE}{src}")
    };

    // 2. The real playlist token only exists inside the embed page.
    let mut h = headers();
    h.push(("Accept", "text/html"));
    let page = get_text_with(&GIGA, &embed_url, Duration::from_secs(12), &h).await?;

    let anchor = page.find("masterPlaylist")?;
    let block = &page[anchor..page.len().min(anchor + BLOCK_LEN)];

    let token = TOKEN_RE.captures(block)?.get(1)?.as_str();
    let expires = EXPIRES_RE.captures(block)?.get(1)?.as_str();
    // The page escapes forward slashes inside the JS string literal.
    let playlist = PLAYLIST_RE.captures(block)?.get(1)?.as_str().replace("\\/", "/");

    // FHD is only offered when the embed URL says the title supports it.
    let fhd = embed_url.contains("canPlayFHD=1");
    let url = format!(
        "{playlist}?token={token}&expires={expires}{}",
        if fhd { "&h=1" } else { "" }
    );

    // 3. Verify before handing it out.
    let mut h = headers();
    h.push(("Accept", "*/*"));
    let check = get_text_with(&GIGA, &url, Duration::from_secs(10), &h).await;
    match check {
        Some(body) if body.trim_start().starts_with("#EXTM3U") => {}
        _ => {
            println!("[VixSrc] ❌ playlist did not verify");
            return None;
        }
    }

    println!("[VixSrc] ✅ verified playlist");

    let source = Source::direct_m3u8(url, if fhd { "1080p" } else { "720p" })
        .tagged("VixSrc", ID)
        .with_referer("https://vixsrc.to/");

    Some(ProviderResult::new(NAME, ID, vec![source]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = r#"window.masterPlaylist = { params: { 'token': 'abc123',
        'expires': '1774348379' }, url: 'https://vix.cdn/playlist/9981\/' }"#;

    #[test]
    fn lifts_token_expires_and_playlist() {
        assert_eq!(TOKEN_RE.captures(BLOCK).unwrap().get(1).unwrap().as_str(), "abc123");
        assert_eq!(
            EXPIRES_RE.captures(BLOCK).unwrap().get(1).unwrap().as_str(),
            "1774348379"
        );
        let raw = PLAYLIST_RE.captures(BLOCK).unwrap().get(1).unwrap().as_str();
        assert_eq!(raw.replace("\\/", "/"), "https://vix.cdn/playlist/9981/");
    }

    #[test]
    fn rejects_a_short_expires_value() {
        // Guards the {9,} bound — a 4-digit year must not be mistaken for one.
        let b = r#"{'expires': '2026'}"#;
        assert!(EXPIRES_RE.captures(b).is_none());
    }
}
