//! LookMovie — port of `resolveLookMovie` in `local-resolver/server.mjs`.
//!
//! A plain JSON API, so no scraping: search by title, resolve the episode id
//! for shows, then fetch the view payload which carries both streams and
//! subtitles. Needs a title, so it sits out when the caller didn't send one.
//!
//! Note the `no_proxy: false` on the returned source, unlike every other
//! provider here. LookMovie binds stream URLs to the IP that requested them,
//! which is this device — a visitor's browser fetching directly gets refused,
//! so playback has to route back through `/proxy/stream`.

use crate::http::{get_text_with, GIGA};
use crate::models::{MediaKind, ProviderResult, Source, Subtitle};
use serde_json::Value;
use std::time::Duration;

const BASE: &str = "https://lmscript.xyz";
const NAME: &str = "LookMovie 🎬";
pub const ID: &str = "lookmovie";

const TIMEOUT: Duration = Duration::from_secs(6);

/// Stream keys in descending preference, matching the JS list.
const QUALITY_ORDER: &[&str] = &["auto", "1080p", "1080", "720p", "720", "480p", "480"];

async fn get_json(url: &str) -> Option<Value> {
    let body = get_text_with(&GIGA, url, TIMEOUT, &[("Accept", "application/json")]).await?;
    serde_json::from_str(&body).ok()
}

pub async fn scrape(
    kind: MediaKind,
    season: u32,
    episode: u32,
    title: &str,
    year: Option<u32>,
) -> Option<ProviderResult> {
    if title.is_empty() {
        return None;
    }

    let is_show = !kind.is_movie();
    println!("[LookMovie] Searching \"{title}\"");

    let search_url = format!(
        "{BASE}{}?filters%5Bq%5D={}",
        if is_show { "/v1/shows" } else { "/v1/movies" },
        urlencoding::encode(title)
    );

    let search = get_json(&search_url).await?;
    let items = search.get("items")?.as_array()?;

    // Prefer an exact title match at the right year; otherwise take the first
    // result, same as the JS.
    let matched = items
        .iter()
        .find(|i| {
            let t_ok = i
                .get("title")
                .and_then(Value::as_str)
                .map(|t| t.eq_ignore_ascii_case(title))
                .unwrap_or(false);
            let y_ok = match year {
                None => true,
                Some(y) => i
                    .get("year")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                    .map(|iy| iy == y as u64)
                    .unwrap_or(false),
            };
            t_ok && y_ok
        })
        .or_else(|| items.first())?;

    // Movies carry their own id; shows need an episode lookup first.
    let media_id: u64 = if is_show {
        let show_id = matched.get("id_show")?.as_u64()?;
        let details = get_json(&format!("{BASE}/v1/shows?expand=episodes&id={show_id}")).await?;
        details
            .get("episodes")?
            .as_array()?
            .iter()
            .find(|e| {
                let n = |k: &str| e.get(k).and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
                n("season") == Some(season as u64) && n("episode") == Some(episode as u64)
            })?
            .get("id")?
            .as_u64()?
    } else {
        matched.get("id_movie")?.as_u64()?
    };

    let view = get_json(&format!(
        "{BASE}{}?expand=streams,subtitles&id={media_id}",
        if is_show { "/v1/episodes/view" } else { "/v1/movies/view" }
    ))
    .await?;

    let streams = view.get("streams")?;
    let url = QUALITY_ORDER
        .iter()
        .find_map(|q| streams.get(*q).and_then(Value::as_str))?;

    let subtitles: Vec<Subtitle> = view
        .get("subtitles")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    let raw = s.get("url").and_then(Value::as_str)?;
                    let lang = s.get("language").and_then(Value::as_str).unwrap_or("").to_string();
                    Some(Subtitle {
                        url: if raw.starts_with("http") {
                            raw.to_string()
                        } else {
                            format!("{BASE}{raw}")
                        },
                        label: lang.clone(),
                        lang,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    println!("[LookMovie] ✅ stream + {} subtitle track(s)", subtitles.len());

    // proxied(): the URL is bound to this device's IP, so visitors must come
    // back through /proxy/stream rather than fetching it directly.
    let source = Source::direct_m3u8(url, "auto").tagged("LookMovie", ID).proxied();

    let mut result = ProviderResult::new(NAME, ID, vec![source]);
    result.subtitles = subtitles;
    Some(result)
}
