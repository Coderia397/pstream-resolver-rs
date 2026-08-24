//! SubDL subtitle search — port of `subdlSearch` in `local-resolver/server.mjs`.
//!
//! Same reasoning as the YouTube endpoint: the frontend used to call SubDL
//! directly with the key in the bundle, where anyone could read it. Only the
//! *search* needs the key — the subtitle files SubDL returns are plain public
//! URLs the browser can still fetch itself — so just this one call is proxied
//! and the key never leaves the device.
//!
//! Set `SUBDL_API_KEY` (no `VITE_` prefix) in the environment to enable.

use crate::http::{get_text_with, GIGA};
use serde_json::{json, Value};
use std::time::Duration;

pub struct SearchArgs<'a> {
    pub tmdb_id: &'a str,
    pub is_tv: bool,
    pub season: u32,
    pub episode: u32,
    pub langs: &'a str,
}

/// Returns the response body to hand back verbatim: `{ subtitles: [...] }`,
/// or the same shape with an `error` when the lookup couldn't be made.
pub async fn search(args: SearchArgs<'_>) -> Value {
    let Ok(key) = std::env::var("SUBDL_API_KEY") else {
        return json!({ "subtitles": [], "error": "SUBDL_API_KEY not configured" });
    };
    if key.is_empty() {
        return json!({ "subtitles": [], "error": "SUBDL_API_KEY not configured" });
    }

    let media_type = if args.is_tv { "tv" } else { "movie" };
    let mut url = format!(
        "https://api.subdl.com/api/v1/subtitles?api_key={}&tmdb_id={}&type={}&subs_per_page=30&language={}",
        urlencoding::encode(&key),
        urlencoding::encode(args.tmdb_id),
        media_type,
        urlencoding::encode(args.langs),
    );
    if args.is_tv {
        url.push_str(&format!(
            "&season_number={}&episode_number={}",
            args.season, args.episode
        ));
    }

    let Some(body) = get_text_with(&GIGA, &url, Duration::from_secs(10), &[("Accept", "application/json")]).await
    else {
        // get_text_with collapses transport errors and non-2xx alike; the JS
        // distinguished them only to put the status in the message.
        return json!({ "subtitles": [], "error": "SubDL request failed" });
    };

    let parsed: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return json!({ "subtitles": [], "error": "SubDL returned invalid JSON" }),
    };

    match parsed.get("subtitles") {
        Some(Value::Array(a)) => json!({ "subtitles": a }),
        // Key absent or not an array — treat as no results, same as the JS.
        _ => json!({ "subtitles": [] }),
    }
}
