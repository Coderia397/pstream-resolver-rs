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

    /// Log which step gave up and return None.
    ///
    /// Worth the noise: this is a five-step chain over two requests, and a
    /// bare `?` at each one means a failure is indistinguishable from any
    /// other. Chasing a "no sources" report without this meant reproducing
    /// the whole chain by hand — and the embed token expires in ~10 seconds,
    /// so by the time you re-request it the evidence is gone.
    macro_rules! step {
        ($opt:expr, $what:literal) => {
            match $opt {
                Some(v) => v,
                None => {
                    println!("[VixSrc] ❌ {}", $what);
                    return None;
                }
            }
        };
    }

    // 1. Ask the API where the embed lives.
    let mut h = headers();
    h.push(("Accept", "application/json"));
    let api_body = step!(
        get_text_with(&GIGA, &format!("{BASE}{api_path}"), Duration::from_secs(10), &h).await,
        "api request failed"
    );

    let api: serde_json::Value = step!(
        serde_json::from_str(&api_body).ok(),
        "api response was not JSON"
    );
    let src = step!(
        api.get("src").and_then(|v| v.as_str()),
        "api response had no src"
    );
    let embed_url = if src.starts_with("http") {
        src.to_string()
    } else {
        format!("{BASE}{src}")
    };

    // 2. The real playlist token only exists inside the embed page. This must
    //    follow immediately — the token on the embed URL lives ~10 seconds,
    //    and a stale one answers 410.
    let mut h = headers();
    h.push(("Accept", "text/html"));
    let page = step!(
        get_text_with(&GIGA, &embed_url, Duration::from_secs(12), &h).await,
        "embed page fetch failed (410 means the embed token already expired)"
    );

    let anchor = step!(page.find("masterPlaylist"), "no masterPlaylist in embed page");
    let block = &page[anchor..page.len().min(anchor + BLOCK_LEN)];

static ASN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"['"]asn['"]\s*:\s*['"]([^'"]+)['"]"#).expect("asn regex"));

    let token = step!(
        TOKEN_RE.captures(block).and_then(|c| c.get(1)),
        "no token in masterPlaylist block"
    )
    .as_str();
    let expires = step!(
        EXPIRES_RE.captures(block).and_then(|c| c.get(1)),
        "no expires in masterPlaylist block"
    )
    .as_str();
    
    let asn = ASN_RE.captures(block).and_then(|c| c.get(1)).map(|m| m.as_str()).unwrap_or("");
    
    // The page escapes forward slashes inside the JS string literal.
    let playlist = step!(
        PLAYLIST_RE.captures(block).and_then(|c| c.get(1)),
        "no playlist url in masterPlaylist block"
    )
    .as_str()
    .replace("\\/", "/");

    // FHD is only offered when the embed URL says the title supports it.
    let fhd = embed_url.contains("canPlayFHD=1");
    let mut url = format!(
        "{playlist}{}token={token}&expires={expires}{}",
        if playlist.contains('?') { "&" } else { "?" },
        if fhd { "&h=1" } else { "" }
    );
    if !asn.is_empty() {
        url.push_str(&format!("&asn={asn}"));
    }

    // 3. Verify before handing it out.
    let mut h = headers();
    h.push(("Accept", "*/*"));
    let check = get_text_with(&GIGA, &url, Duration::from_secs(10), &h).await;
    match check {
        Some(body) if body.trim_start().starts_with("#EXTM3U") => {}
        _ => {
            println!("[VixSrc] ❌ playlist did not verify: {:#?}", check);
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

    #[tokio::test]
    #[ignore]
    async fn test_live_scrape() {
        let res = scrape("27205", crate::models::MediaKind::Movie, 1, 1).await;
        println!("{:#?}", res);
    }
