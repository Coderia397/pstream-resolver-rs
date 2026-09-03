#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MediaKind;

    #[tokio::test]
    #[ignore]
    async fn constructs_correct_movie_urls() {
        let res = scrape("27205", MediaKind::Movie, 1, 1, Some("Inception"), Some(2010)).await.unwrap();
        assert_eq!(res.sources.len(), 1);
    }
}

use crate::models::{MediaKind, ProviderResult, Source};
use crate::utils::sort_sources_by_quality;

pub const ID: &str = "spencerdevs";
const NAME: &str = "SpencerDevs 🚀";

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
    _title: Option<&str>,
    _year: Option<u32>,
) -> Option<ProviderResult> {
    let mut sources = Vec::new();

    if kind.is_movie() {
        sources.push(Source::embed(format!("https://watch.spencerdevs.xyz/movie/{tmdb_id}"), "1080p").tagged(NAME, ID).with_referer("https://watch.spencerdevs.xyz/"));
    } else {
        sources.push(Source::embed(format!("https://watch.spencerdevs.xyz/tv/{tmdb_id}/{season}/{episode}"), "1080p").tagged(NAME, ID).with_referer("https://watch.spencerdevs.xyz/"));
    }

    sort_sources_by_quality(&mut sources);
    ProviderResult::some_if_any(NAME, ID, sources)
}
