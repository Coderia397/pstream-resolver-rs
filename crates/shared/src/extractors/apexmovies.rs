#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub() {
    }
}

use crate::models::{MediaKind, ProviderResult, Source};

pub const ID: &str = "apexmovies";

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
    _title: Option<&str>,
    _year: Option<u32>,
) -> Option<ProviderResult> {
    let type_str = if kind.is_movie() { "movie" } else { "tv" };
    
    let mut url = format!(
        "https://apexmovies.net/wp-content/themes/fmovie/player/stream-player.php?id={}&type={}",
        tmdb_id, type_str
    );

    if !kind.is_movie() {
        url = format!("{}&s={}&e={}", url, season, episode);
    }

    let Ok(resp) = reqwest::get(&url).await else {
        return None;
    };
    let Ok(body) = resp.text().await else {
        return None;
    };

    let mut sources = Vec::new();
    let m3u8_urls = crate::extractors::find_m3u8_urls(&body);
    
    if !m3u8_urls.is_empty() {
        for u in m3u8_urls.into_iter().take(2) {
            sources.push(Source::direct_m3u8(u, "auto").tagged("Apexmovies", ID));
        }
    } else {
        // Fallback to embed if no direct streams are found
        sources.push(Source::embed(url, "auto").tagged("Apexmovies", ID));
    }

    ProviderResult::some_if_any("Apexmovies", ID, sources)
}
