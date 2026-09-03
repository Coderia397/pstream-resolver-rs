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


pub mod bstsrs;
pub mod cinemaos;
pub mod dramacool;
pub mod miruro;
pub mod moviebox;
pub mod nontongo;
pub mod oneshows;
pub mod cinemaarmyext;
pub mod bingeflix;
pub mod bcine;
pub mod apexmovies;
pub mod hydrahd;
pub mod sixtysevenmovies;
pub mod cineby;
pub mod flickystream;
pub mod spencerdevs;
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

pub const HTML_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
pub const JSON_ACCEPT: &str = "application/json, text/plain, */*";

/// Every provider `/api/stream` queries, in the order the JS lists them.
///
/// Order is not a priority ranking — all are queried concurrently and every
/// one that answers contributes its sources. It only decides which provider
/// gets named as the headline one in the response.
pub static PROVIDERS: &[Provider] = &[];

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

    let _oneshows = timed(oneshows::ID, oneshows::scrape(tmdb_id, kind, season, episode, title, year));
        let _cinemaarmyext = timed(cinemaarmyext::ID, cinemaarmyext::scrape(tmdb_id, kind, season, episode, title, year));
            let _bingeflix = timed(bingeflix::ID, bingeflix::scrape(tmdb_id, kind, season, episode, title, year));
    let _bcine = timed(bcine::ID, bcine::scrape(tmdb_id, kind, season, episode, title, year));
    let _apexmovies = timed(apexmovies::ID, apexmovies::scrape(tmdb_id, kind, season, episode, title, year));
                    let _hydrahd = timed(hydrahd::ID, hydrahd::scrape(tmdb_id, kind, season, episode, title, year));
    let _sixtysevenmovies = timed(sixtysevenmovies::ID, sixtysevenmovies::scrape(tmdb_id, kind, season, episode, title, year));
    let _cineby = timed(cineby::ID, cineby::scrape(tmdb_id, kind, season, episode, title, year));
    let _flickystream = timed(flickystream::ID, flickystream::scrape(tmdb_id, kind, season, episode, title, year));
    let _spencerdevs = timed(spencerdevs::ID, spencerdevs::scrape(tmdb_id, kind, season, episode, title, year));
    let vixsrc = timed(vixsrc::ID, vixsrc::scrape(tmdb_id, kind, season, episode));
    let cinemaos = timed(cinemaos::ID, cinemaos::scrape(tmdb_id, kind, season, episode));
    let bstsrs = timed(bstsrs::ID, bstsrs::scrape(tmdb_id, kind, season, episode, title, year));
    let dramacool = timed(dramacool::ID, dramacool::scrape(tmdb_id, kind, season, episode, title, year));
    let miruro = async { None }; // Disabled: Cloudflare UAM

    // MovieBox searches by name, so it sits out entirely
    // when the caller didn't send a title.
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
            Some(t) if !t.is_empty() && kind.is_movie() => timed(moviebox::ID, moviebox::scrape(t, year)).await,
            _ => None,
        }
    };

    let nontongo = async { None }; // Disabled 2026-08-26: returning 504 Gateway Timeout

    let (vixsrc, _oneshows, _cinemaarmyext, _bingeflix, _bcine, _apexmovies, _hydrahd, _sixtysevenmovies, _cineby, _flickystream, _spencerdevs, cinemaos, bstsrs, dramacool, miruro, table, moviebox, nontongo) =
        futures::join!(vixsrc, _oneshows, _cinemaarmyext, _bingeflix, _bcine, _apexmovies, _hydrahd, _sixtysevenmovies, _cineby, _flickystream, _spencerdevs, cinemaos, bstsrs, dramacool, miruro, table, moviebox, nontongo);

    // Order matches the JS list: vixsrc, cinemaos, bstsrs, dramacool, miruro,
    // then table providers, moviebox, and nontongo.
    let mut results: Vec<ProviderResult> = std::iter::once(vixsrc)
        .chain(std::iter::once(_oneshows))
                .chain(std::iter::once(_cinemaarmyext))
                        .chain(std::iter::once(_bingeflix))
        .chain(std::iter::once(_bcine))
        .chain(std::iter::once(_apexmovies))
                                        .chain(std::iter::once(_hydrahd))
        .chain(std::iter::once(_sixtysevenmovies))
        .chain(std::iter::once(_cineby))
        .chain(std::iter::once(_flickystream))
        .chain(std::iter::once(_spencerdevs))
        .chain(std::iter::once(cinemaos))
        .chain(std::iter::once(bstsrs))
        .chain(std::iter::once(dramacool))
        .chain(std::iter::once(miruro))
        .chain(table)
        .chain(std::iter::once(moviebox))
        .chain(std::iter::once(nontongo))
        .flatten()
        .collect();

    for res in &mut results {
        res.sources.sort_by(crate::utils::compare_sources_adaptive);
    }
    results.retain(|res| !res.sources.is_empty());

    // Strict two-tier provider ranking:
    // Tier 1: Providers with direct streams (!is_embed).
    // Tier 2: Providers with only embeds (is_embed).
    // Within Tier 1: ranked descending by peak direct quality.
    // Within Tier 2: ranked descending by peak embed quality.
    results.sort_by(|a, b| {
        let a_has_direct = a.sources.iter().any(|s| s.is_direct());
        let b_has_direct = b.sources.iter().any(|s| s.is_direct());

        if a_has_direct != b_has_direct {
            return b_has_direct.cmp(&a_has_direct);
        }

        let a_peak = if a_has_direct {
            a.sources
                .iter()
                .filter(|s| s.is_direct())
                .map(|s| crate::utils::quality_rank(&s.quality))
                .max()
                .unwrap_or(0)
        } else {
            a.sources
                .iter()
                .map(|s| crate::utils::quality_rank(&s.quality))
                .max()
                .unwrap_or(0)
        };

        let b_peak = if b_has_direct {
            b.sources
                .iter()
                .filter(|s| s.is_direct())
                .map(|s| crate::utils::quality_rank(&s.quality))
                .max()
                .unwrap_or(0)
        } else {
            b.sources
                .iter()
                .map(|s| crate::utils::quality_rank(&s.quality))
                .max()
                .unwrap_or(0)
        };

        b_peak.cmp(&a_peak)
    });

    results
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
        let p = Provider {
            id: "dummy",
            enabled: false,
            name: "Miruro Anime 🌸",
            base: "",
            movie_path: "",
            tv_path: "",
            timeout_secs: 0,
            max_sources: 0,
            qualities: &[],
            client: ClientKind::Proxy,
            accept: "",
            send_referer: false,
            tag_source_referer: false,
            mark_not_embed: false,
        };
        assert_eq!(p.short_name(), "Miruro Anime");
    }

    #[test]
    fn test_two_tier_provider_results_sorting_and_uncontaminated_quality() {
        let mut results = vec![
            // Prov 1: Only Embed 1080p
            ProviderResult::new(
                "EmbedProv",
                "embed_prov",
                vec![Source::embed("https://embed.example/1080", "1080p")],
            ),
            // Prov 2: Direct 720p AND Embed 1080p (must NOT contaminate peak direct)
            ProviderResult::new(
                "MixedProv",
                "mixed_prov",
                vec![
                    Source::embed("https://embed.example/1080_contam", "1080p"),
                    Source::direct_m3u8("https://direct.example/720.m3u8", "720p"),
                ],
            ),
            // Prov 3: Direct 1080p
            ProviderResult::new(
                "DirectHDProv",
                "direct_hd_prov",
                vec![Source::direct_m3u8("https://direct.example/1080.m3u8", "1080p")],
            ),
            // Prov 4: Direct 480p (SD fallback)
            ProviderResult::new(
                "DirectSDProv",
                "direct_sd_prov",
                vec![Source::direct_m3u8("https://direct.example/480.mp4", "480p")],
            ),
            // Prov 5: Only Embed 720p
            ProviderResult::new(
                "EmbedSDProv",
                "embed_sd_prov",
                vec![Source::embed("https://embed.example/720", "720p")],
            ),
        ];

        for res in &mut results {
            res.sources.sort_by(crate::utils::compare_sources_adaptive);
        }
        results.retain(|res| !res.sources.is_empty());

        results.sort_by(|a, b| {
            let a_has_direct = a.sources.iter().any(|s| s.is_direct());
            let b_has_direct = b.sources.iter().any(|s| s.is_direct());

            if a_has_direct != b_has_direct {
                return b_has_direct.cmp(&a_has_direct);
            }

            let a_peak = if a_has_direct {
                a.sources
                    .iter()
                    .filter(|s| s.is_direct())
                    .map(|s| crate::utils::quality_rank(&s.quality))
                    .max()
                    .unwrap_or(0)
            } else {
                a.sources
                    .iter()
                    .map(|s| crate::utils::quality_rank(&s.quality))
                    .max()
                    .unwrap_or(0)
            };

            let b_peak = if b_has_direct {
                b.sources
                    .iter()
                    .filter(|s| s.is_direct())
                    .map(|s| crate::utils::quality_rank(&s.quality))
                    .max()
                    .unwrap_or(0)
            } else {
                b.sources
                    .iter()
                    .map(|s| crate::utils::quality_rank(&s.quality))
                    .max()
                    .unwrap_or(0)
            };

            b_peak.cmp(&a_peak)
        });

        // 1. First provider MUST be Direct 1080p (DirectHDProv)
        assert_eq!(results[0].provider_id, "direct_hd_prov");

        // 2. Second provider MUST be Direct 720p (MixedProv), beating direct 480p and embed 1080p
        assert_eq!(results[1].provider_id, "mixed_prov");
        // Inside MixedProv, direct 720p must precede embed 1080p
        assert!(results[1].sources[0].is_direct());
        assert_eq!(results[1].sources[0].quality, "720p");
        assert!(results[1].sources[1].is_embed());
        assert_eq!(results[1].sources[1].quality, "1080p");

        // 3. Third provider MUST be Direct 480p (DirectSDProv), beating all embeds
        assert_eq!(results[2].provider_id, "direct_sd_prov");
        assert_eq!(results[2].sources[0].quality, "480p");

        // 4. Fourth provider is Embed 1080p (EmbedProv)
        assert_eq!(results[3].provider_id, "embed_prov");

        // 5. Fifth provider is Embed 720p (EmbedSDProv)
        assert_eq!(results[4].provider_id, "embed_sd_prov");
    }
}
pub mod vsembed;
