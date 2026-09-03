#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub() {
    }
}

use crate::models::{MediaKind, ProviderResult, Source};

pub const ID: &str = "cinemaarmyext";

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
    _title: Option<&str>,
    _year: Option<u32>,
) -> Option<ProviderResult> {
    let vidzee_url = match kind {
        MediaKind::Movie => format!("https://player.vidzee.wtf/embed/movie/{}", tmdb_id),
        MediaKind::Tv => format!("https://player.vidzee.wtf/embed/tv/{}/{}/{}", tmdb_id, season, episode),
    };

    let mut vidzee_source = Source::embed(vidzee_url, "auto");
    vidzee_source.provider = Some("VidZee".to_string());
    vidzee_source.provider_id = Some(ID.to_string());

    Some(ProviderResult {
        success: true,
        provider: "CinemaArmy".to_string(),
        provider_id: ID.to_string(),
        sources: vec![vidzee_source],
        subtitles: vec![],
    })
}
