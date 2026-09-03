#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub() {
    }
}

use crate::models::{MediaKind, ProviderResult, Source};


pub const ID: &str = "bingeflix";


struct Provider {
    id: String,
    enabled: bool,
    movie_url: String,
    tv_url: String,
}

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
    _title: Option<&str>,
    _year: Option<u32>,
) -> Option<ProviderResult> {
    let providers: Vec<Provider> = vec![
        Provider {
            id: "vidlink".to_string(),
            enabled: true,
            movie_url: "https://vidlink.pro/movie/{tmdbId}".to_string(),
            tv_url: "https://vidlink.pro/tv/{tmdbId}/{season}/{episode}".to_string(),
        },
        Provider {
            id: "vidsrc".to_string(),
            enabled: true,
            movie_url: "https://vidsrc.me/embed/movie?tmdb={tmdbId}".to_string(),
            tv_url: "https://vidsrc.me/embed/tv?tmdb={tmdbId}&season={season}&episode={episode}".to_string(),
        }
    ];

    let mut sources = Vec::new();
    for p in providers {
        if !p.enabled {
            continue;
        }

        let url = if kind.is_movie() {
            p.movie_url
                .replace("{tmdbId}", tmdb_id)
                .replace("{extra}", "")
        } else {
            p.tv_url
                .replace("{tmdbId}", tmdb_id)
                .replace("{season}", &season.to_string())
                .replace("{episode}", &episode.to_string())
                .replace("{extra}", "")
        };

        let source = Source::embed(url, p.id.clone()).tagged(ID, &p.id);
        sources.push(source);
    }

    ProviderResult::some_if_any(ID, ID, sources)
}
