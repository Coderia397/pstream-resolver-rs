//! Provider extractors.
//!
//! Every extractor has the same contract: given a TMDB id and (for tv) a
//! season/episode, return `Some(ProviderResult)` when it found something
//! playable, or `None` for anything else — miss, timeout, parse failure,
//! upstream 500. Nothing here returns Err, because the caller's response to
//! every failure mode is identical: try the next provider.
//!
//! Nine of the eleven providers in the JS differ only in data — base URL,
//! path shape, timeout, how many sources to keep, what to label the quality.
//! They are a table here rather than nine near-identical modules. Providers
//! that need real logic get their own file.

use crate::http::{GIGA, PROXY};
use crate::models::{MediaKind, ProviderResult, Source};
use std::time::Duration;

pub mod lookmovie;
pub mod moviebox;
pub mod nontongo;
pub mod vixsrc;

/// Which shared client a provider goes out through. Most need the residential
/// path because their CDNs block datacenter ranges; the ones on Cloudflare
/// Pages don't care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    Giga,
    Proxy,
}

pub struct Provider {
    pub id: &'static str,
    /// Whether this provider is queried at all.
    ///
    /// A disabled one is kept in the table rather than deleted, with a note
    /// saying what was observed and when. Every entry here worked once, and a
    /// provider that has gone dark often comes back — deleting it loses the
    /// path shape, the headers it wants and the quality labels, all of which
    /// then have to be rediscovered.
    ///
    /// Disabled providers cost nothing: `run_all` skips them, so they are not
    /// one of the outbound requests every resolve pays for.
    pub enabled: bool,
    /// Display name including the emoji the frontend renders.
    pub name: &'static str,
    pub base: &'static str,
    /// Path templates. `{id}`, `{season}` and `{episode}` are substituted.
    pub movie_path: &'static str,
    pub tv_path: &'static str,
    pub timeout_secs: u64,
    pub max_sources: usize,
    /// Quality label per source index; the last entry repeats for the rest.
    pub qualities: &'static [&'static str],
    pub client: ClientKind,
    pub accept: &'static str,
    /// Send `Referer: {base}/` on the scrape request.
    pub send_referer: bool,
    /// Also record the base as `referer` on each returned source, for hosts
    /// that check it again when the player fetches segments.
    pub tag_source_referer: bool,
    /// Emit `isEmbed: false` explicitly.
    pub mark_not_embed: bool,
}

const HTML_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
const JSON_ACCEPT: &str = "application/json, text/plain, */*";

/// Every provider `/api/stream` queries, in the order the JS lists them.
///
/// Order is not a priority ranking — all are queried concurrently and every
/// one that answers contributes its sources. It only decides which provider
/// gets named as the headline one in the response.
pub static PROVIDERS: &[Provider] = &[
    Provider {
        id: "watchflix",
        // 2026-08-25: watchflix.st does not resolve at all (curl 000). Domain gone.
        enabled: false,
        name: "WatchFlix 🎬",
        base: "https://watchflix.st",
        movie_path: "/movie/{id}",
        tv_path: "/tv/{id}/{season}/{episode}",
        timeout_secs: 7,
        max_sources: 3,
        qualities: &["1080p", "auto"],
        client: ClientKind::Proxy,
        accept: HTML_ACCEPT,
        send_referer: true,
        tag_source_referer: true,
        mark_not_embed: true,
    },
    Provider {
        id: "bingr",
        // 2026-08-25: alive, but /watch/movie/{id} lands on the homepage
        //             (title "Bingr — Stream Movies…", no title text). Path shape changed.
        enabled: false,
        name: "Bingr 🚀",
        base: "https://bingr.one",
        movie_path: "/watch/movie/{id}",
        tv_path: "/watch/tv/{id}/{season}/{episode}",
        timeout_secs: 7,
        max_sources: 3,
        qualities: &["1080p", "720p"],
        client: ClientKind::Proxy,
        accept: HTML_ACCEPT,
        send_referer: true,
        tag_source_referer: true,
        mark_not_embed: false,
    },
    Provider {
        id: "fireflix",
        // 2026-08-25: fireflix.pages.dev 302s to fireflix2.pages.dev, which 404s
        //             on /api/movie?id=. Moved and changed its API shape.
        enabled: false,
        name: "FireFlix 🔥",
        base: "https://fireflix.pages.dev",
        movie_path: "/api/movie?id={id}",
        tv_path: "/api/tv?id={id}&season={season}&episode={episode}",
        timeout_secs: 6,
        max_sources: 2,
        qualities: &["1080p"],
        client: ClientKind::Giga,
        accept: JSON_ACCEPT,
        send_referer: false,
        tag_source_referer: false,
        mark_not_embed: false,
    },
    Provider {
        id: "oneshows",
        // 2026-08-25: alive, but the page we fetch is an error page, not the title.
        enabled: false,
        name: "1Shows 📺",
        base: "https://www.1shows.org",
        movie_path: "/movie/{id}",
        tv_path: "/tv/{id}/{season}/{episode}",
        timeout_secs: 7,
        max_sources: 3,
        qualities: &["1080p", "720p"],
        client: ClientKind::Proxy,
        accept: HTML_ACCEPT,
        send_referer: true,
        tag_source_referer: false,
        mark_not_embed: false,
    },
    Provider {
        id: "cinemaos",
        // 2026-08-25: URL is CORRECT — page title reads "Inception (2010) - Cinemaos".
        //             The manifest is no longer in the HTML; the site loads it
        //             client-side, so a plain regex sweep finds nothing.
        enabled: false,
        name: "CinemaOS 🎥",
        base: "https://cinemaos.live",
        movie_path: "/movie/{id}",
        tv_path: "/tv/{id}/{season}/{episode}",
        timeout_secs: 7,
        max_sources: 2,
        qualities: &["1080p"],
        client: ClientKind::Proxy,
        accept: HTML_ACCEPT,
        send_referer: true,
        tag_source_referer: false,
        mark_not_embed: false,
    },
    Provider {
        id: "aurorascreen",
        // 2026-08-25: www.aurorascreen.org 301s to aurorascreen.org, which 404s on
        //             /movie/{id}. Moved and changed its path shape.
        enabled: false,
        name: "AuroraScreen 🌌",
        base: "https://www.aurorascreen.org",
        movie_path: "/movie/{id}",
        tv_path: "/tv/{id}/{season}/{episode}",
        timeout_secs: 7,
        max_sources: 2,
        qualities: &["1080p"],
        client: ClientKind::Proxy,
        accept: HTML_ACCEPT,
        send_referer: true,
        tag_source_referer: false,
        mark_not_embed: false,
    },
    Provider {
        // Anime; one URL shape regardless of movie/tv.
        id: "miruro",
        // 2026-08-25: alive; tested fairly with an anime id (1429) rather than a film.
        //             Still no manifest in the HTML — loaded client-side.
        enabled: false,
        name: "Miruro Anime 🌸",
        base: "https://www.miruro.com",
        movie_path: "/watch?id={id}",
        tv_path: "/watch?id={id}",
        timeout_secs: 7,
        max_sources: 2,
        qualities: &["1080p"],
        client: ClientKind::Proxy,
        accept: HTML_ACCEPT,
        send_referer: true,
        tag_source_referer: false,
        mark_not_embed: false,
    },
    Provider {
        // Series only — the JS uses the episode URL for both cases.
        id: "bstsrs",
        // 2026-08-25: no sources across the sample. Not investigated individually.
        enabled: false,
        name: "BSTSrs Series 📺",
        base: "https://bstsrs.in",
        movie_path: "/show/{id}/season/{season}/episode/{episode}",
        tv_path: "/show/{id}/season/{season}/episode/{episode}",
        timeout_secs: 7,
        max_sources: 2,
        qualities: &["720p"],
        client: ClientKind::Proxy,
        accept: HTML_ACCEPT,
        send_referer: true,
        tag_source_referer: false,
        mark_not_embed: false,
    },
    Provider {
        // Asian drama; episode-addressed, no season in the path.
        id: "dramacool",
        // 2026-08-25: no sources across the sample. Not investigated individually.
        enabled: false,
        name: "DramaCool 🎭",
        base: "https://dramacoolv.buzz",
        movie_path: "/drama/{id}-episode-{episode}.html",
        tv_path: "/drama/{id}-episode-{episode}.html",
        timeout_secs: 8,
        max_sources: 2,
        qualities: &["720p"],
        client: ClientKind::Proxy,
        accept: HTML_ACCEPT,
        send_referer: true,
        tag_source_referer: false,
        mark_not_embed: false,
    },
];

/// Query every provider concurrently and return each one that produced
/// sources, in table order.
///
/// **Difference from the JS, deliberate:** `moviebox.js` and `nontongo.js`
/// return `{ success: false, error }` when they fail. That object is truthy,
/// so it survives the caller's `.filter(Boolean)` and is counted as a working
/// provider — which means `results.length` can be non-zero with no playable
/// source anywhere, and the response goes out as `success: true` with an empty
/// `sources` array. Here a failure is `None` and simply isn't collected.
pub async fn run_all(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
    title: Option<&str>,
    year: Option<u32>,
) -> Vec<ProviderResult> {
    async fn timed<F>(id: &'static str, fut: F) -> Option<ProviderResult>
    where
        F: std::future::Future<Output = Option<ProviderResult>>,
    {
        let t0 = std::time::Instant::now();
        let out = fut.await;
        crate::health::record(id, out.is_some(), t0.elapsed());
        out
    }

    let vixsrc = timed(vixsrc::ID, vixsrc::scrape(tmdb_id, kind, season, episode));

    // LookMovie and MovieBox both search by name, so they sit out entirely
    // when the caller didn't send a title.
    let lookmovie = async {
        match title {
            Some(t) if !t.is_empty() => {
                timed(lookmovie::ID, lookmovie::scrape(kind, season, episode, t, year)).await
            }
            _ => None,
        }
    };

    // Disabled providers are skipped entirely rather than queried and ignored.
    // Every resolve pays for each outbound request in latency and, on a phone,
    // in mobile data — nine dead ones were costing both on every single call.
    let table = futures::future::join_all(
        PROVIDERS
            .iter()
            .filter(|p| p.enabled)
            .map(|p| timed(p.id, p.scrape(tmdb_id, kind, season, episode))),
    );

    let moviebox = async {
        match title {
            Some(t) if !t.is_empty() => timed(moviebox::ID, moviebox::scrape(t, year)).await,
            _ => None,
        }
    };

    let nontongo = timed(
        nontongo::ID,
        nontongo::scrape(tmdb_id, kind, season, episode),
    );

    let (vixsrc, lookmovie, table, moviebox, nontongo) =
        futures::join!(vixsrc, lookmovie, table, moviebox, nontongo);

    // Order matches the JS list exactly: vixsrc, lookmovie, the nine table
    // providers, then moviebox and nontongo.
    std::iter::once(vixsrc)
        .chain(std::iter::once(lookmovie))
        .chain(table)
        .chain(std::iter::once(moviebox))
        .chain(std::iter::once(nontongo))
        .flatten()
        .collect()
}

fn fill(template: &str, id: &str, season: u32, episode: u32) -> String {
    template
        .replace("{id}", id)
        .replace("{season}", &season.to_string())
        .replace("{episode}", &episode.to_string())
}

impl Provider {
    pub async fn scrape(
        &self,
        tmdb_id: &str,
        kind: MediaKind,
        season: u32,
        episode: u32,
    ) -> Option<ProviderResult> {
        let path = fill(
            if kind.is_movie() { self.movie_path } else { self.tv_path },
            tmdb_id,
            season,
            episode,
        );
        let url = format!("{}{}", self.base, path);

        println!("[{}] Probing {url}", self.id);

        let client = match self.client {
            ClientKind::Giga => &*GIGA,
            ClientKind::Proxy => &*PROXY,
        };

        let referer = format!("{}/", self.base);
        let mut extra: Vec<(&str, &str)> = vec![("Accept", self.accept)];
        if self.send_referer {
            extra.push(("Referer", referer.as_str()));
        }

        let body = crate::http::get_text_with(
            client,
            &url,
            Duration::from_secs(self.timeout_secs),
            &extra,
        )
        .await?;

        let sources: Vec<Source> = find_m3u8_urls(&body)
            .into_iter()
            .take(self.max_sources)
            .enumerate()
            .map(|(i, u)| {
                // Past the end of the list, reuse the last label — matches the
                // JS ternaries, which only ever distinguish the first source.
                let quality = self
                    .qualities
                    .get(i)
                    .or_else(|| self.qualities.last())
                    .copied()
                    .unwrap_or("auto");

                let mut s = Source::direct_m3u8(u, quality).tagged(self.short_name(), self.id);
                if self.tag_source_referer {
                    s = s.with_referer(self.base);
                }
                if self.mark_not_embed {
                    s = s.not_embed();
                }
                s
            })
            .collect();

        if sources.is_empty() {
            println!("[{}] no manifests in response", self.id);
        } else {
            println!("[{}] ✅ {} source(s)", self.id, sources.len());
        }

        ProviderResult::some_if_any(self.name, self.id, sources)
    }

    /// Display name without the trailing emoji.
    ///
    /// The JS tags individual sources with the plain name and only the result
    /// with the decorated one — "WatchFlix" on each source, "WatchFlix 🎬" on
    /// the result. Cutting at the first non-ASCII char gets that for every
    /// entry in the table, including the two-word ones.
    fn short_name(&self) -> &'static str {
        match self.name.find(|c: char| !c.is_ascii()) {
            Some(i) => self.name[..i].trim_end(),
            None => self.name,
        }
    }
}

/// Pull every m3u8 URL out of an arbitrary blob of HTML or JSON.
///
/// Deliberately dumb: several providers bury the manifest in inline script,
/// escaped JSON, or an attribute, and a regex sweep beats writing a parser
/// for each one. Deduplicated, order preserved.
pub fn find_m3u8_urls(haystack: &str) -> Vec<String> {
    use once_cell::sync::Lazy;
    use regex::Regex;

    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"https?://[^\s"'<>\\]+\.m3u8[^\s"'<>\\]*"#).expect("m3u8 regex")
    });

    let mut seen = std::collections::HashSet::new();
    RE.find_iter(haystack)
        .map(|m| m.as_str().to_string())
        .filter(|u| seen.insert(u.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_path_templates() {
        assert_eq!(fill("/tv/{id}/{season}/{episode}", "550", 2, 7), "/tv/550/2/7");
        assert_eq!(fill("/movie/{id}", "550", 1, 1), "/movie/550");
        assert_eq!(
            fill("/drama/{id}-episode-{episode}.html", "abc", 1, 12),
            "/drama/abc-episode-12.html"
        );
    }

    #[test]
    fn finds_and_dedupes_manifests() {
        let html = r#"<script>var a="https://x.com/a.m3u8?t=1";
                      var b='https://x.com/a.m3u8?t=1';
                      var c="https://x.com/b.m3u8";</script>"#;
        let found = find_m3u8_urls(html);
        assert_eq!(found.len(), 2, "duplicates should collapse: {found:?}");
        assert_eq!(found[0], "https://x.com/a.m3u8?t=1");
    }

    #[test]
    fn every_provider_has_a_quality_label() {
        for p in PROVIDERS {
            assert!(!p.qualities.is_empty(), "{} has no qualities", p.id);
            assert!(p.max_sources > 0, "{} keeps no sources", p.id);
        }
    }

    #[test]
    fn a_disabled_provider_still_carries_a_usable_config() {
        // Disabled entries are kept so they can be switched back on when a
        // provider returns. That is only worth doing if the config survives
        // intact — otherwise re-enabling means rediscovering it anyway.
        for p in PROVIDERS.iter().filter(|p| !p.enabled) {
            assert!(p.base.starts_with("http"), "{} lost its base url", p.id);
            assert!(p.movie_path.contains("{id}"), "{} lost its movie path", p.id);
            assert!(!p.name.is_empty(), "{} lost its name", p.id);
        }
    }

    #[test]
    fn short_name_drops_the_emoji() {
        let by_id = |id: &str| PROVIDERS.iter().find(|p| p.id == id).unwrap();
        assert_eq!(by_id("watchflix").short_name(), "WatchFlix");
        assert_eq!(by_id("miruro").short_name(), "Miruro Anime");
        assert_eq!(by_id("bstsrs").short_name(), "BSTSrs Series");
        assert_eq!(by_id("oneshows").short_name(), "1Shows");
    }
}
