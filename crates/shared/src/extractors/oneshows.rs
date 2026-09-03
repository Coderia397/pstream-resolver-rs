#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MediaKind;

    #[tokio::test]
    async fn constructs_correct_movie_urls() {
        let res = scrape("27205", MediaKind::Movie, 1, 1, Some("Inception"), Some(2010)).await.unwrap();
        assert!(res.sources.len() >= 1);
        assert!(res.sources[0].is_direct());
    }

    #[tokio::test]
    async fn constructs_correct_tv_urls() {
        let res = scrape("1396", MediaKind::Tv, 2, 3, Some("Breaking Bad"), Some(2008)).await.unwrap();
        assert!(res.sources.len() >= 1);
        assert!(res.sources[0].is_direct());
    }
}

use crate::models::{MediaKind, ProviderResult};
use crate::http::PROXY;
use crate::utils::sort_sources_by_quality;

pub const ID: &str = "oneshows";
const NAME: &str = "1shows 📺";

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
    _title: Option<&str>,
    _year: Option<u32>,
) -> Option<ProviderResult> {
    let src_url = if kind.is_movie() {
        format!("https://vsembed.ru/vs_src.php?type=movie&id={}", tmdb_id)
    } else {
        format!("https://vsembed.ru/vs_src.php?type=tv&id={}&season={}&episode={}", tmdb_id, season, episode)
    };
    
    match crate::extractors::vsembed::extract_m3u8(&PROXY, &src_url).await {
        Ok(mut sources) => {
            for s in &mut sources {
                *s = s.clone().tagged(NAME, ID);
            }
            sort_sources_by_quality(&mut sources);
            ProviderResult::some_if_any(NAME, ID, sources)
        }
        Err(e) => {
            eprintln!("[{}] vsembed extract error: {}", ID, e);
            None
        }
    }
}
