#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MediaKind;

    #[tokio::test]
    #[ignore]
    #[ignore]
    #[ignore]
    #[ignore]
    async fn test_scrape_movie() {
        let res = scrape("278", MediaKind::Movie, 0, 0, Some("The Shawshank Redemption"), Some(1994)).await;
        assert!(res.is_some());
        let res = res.unwrap();
        assert!(!res.sources.is_empty());
        println!("{:#?}", res.sources);
    }

    #[tokio::test]
    #[ignore]
    #[ignore]
    #[ignore]
    #[ignore]
    async fn test_scrape_tv() {
        let res = scrape("1396", MediaKind::Tv, 1, 2, Some("Breaking Bad"), Some(2008)).await;
        assert!(res.is_some());
        let res = res.unwrap();
        assert!(!res.sources.is_empty());
        println!("{:#?}", res.sources);
    }
}

use crate::http::{get_text_with, GIGA};
use crate::models::{MediaKind, ProviderResult, Source};
use regex::Regex;
use std::time::Duration;

pub const ID: &str = "hydrahd";
const NAME: &str = "HydraHD 💧";
const BASE: &str = "https://yuppow.app"; // hydrahd.org now embeds yuppow.app

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
    _title: Option<&str>,
    _year: Option<u32>,
) -> Option<ProviderResult> {
    // yuppow.app uses direct tmdb_id routing
    let url = match kind {
        MediaKind::Movie => format!("{}/movies/{}/", BASE, tmdb_id),
        MediaKind::Tv => format!("{}/tv/{}/?s={}&e={}", BASE, tmdb_id, season, episode),
    };

    let headers = vec![
        ("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/115.0.0.0 Safari/537.36"),
        ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"),
    ];

    let body = get_text_with(&GIGA, &url, Duration::from_secs(10), &headers).await?;

    let re = Regex::new(r#"data-provider-url="([^"]+)""#).unwrap();
    let mut sources = Vec::new();

    for cap in re.captures_iter(&body) {
        if let Some(url_match) = cap.get(1) {
            let embed_url = url_match.as_str().to_string();
            // Try to extract provider name from URL host (e.g. vidlink.pro -> vidlink)
            // Just return as embed
            sources.push(Source::embed(embed_url.clone(), "auto").tagged(NAME, ID).with_referer(BASE));
        }
    }

    if sources.is_empty() {
        return None;
    }

    ProviderResult::some_if_any(NAME, ID, sources)
}
