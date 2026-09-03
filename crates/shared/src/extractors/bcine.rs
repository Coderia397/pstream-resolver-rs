#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub() {
    }
}

use crate::models::{MediaKind, ProviderResult, Source};

pub const ID: &str = "bcine";

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
    _title: Option<&str>,
    _year: Option<u32>,
) -> Option<ProviderResult> {
    // bcine.ru wrappers 1embed.cc. Since extracting .m3u8 from 1embed requires browser execution,
    // we fallback to the 1embed iframe URL directly.
    let url = if kind.is_movie() {
        format!("https://1embed.cc/embed/movie/{}", tmdb_id)
    } else {
        format!("https://1embed.cc/embed/tv/{}/{}/{}", tmdb_id, season, episode)
    };

    let sources = vec![Source::embed(url, "auto").tagged("BCine", ID)];
    ProviderResult::some_if_any("BCine", ID, sources)
}
