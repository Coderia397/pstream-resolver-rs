//! Empirical Adversarial Test Suite for DramaCool Extractor.
//!
//! Stress-tests title slugification, special characters, seasons, movie vs TV handling,
//! HTML search parsing, malformed DOM robustness, concurrency, aggregator integration,
//! and live multi-drama stream extraction.

use pstream_shared::extractors::dramacool::{
    build_episode_path, extract_sources_from_html, is_video_source, normalize_url,
    parse_search_slug, scrape, search_drama_slug, slugify, ID,
};
use pstream_shared::extractors::run_all;
use pstream_shared::http::{get_text_with, PROXY};
use pstream_shared::models::MediaKind;
use std::time::{Duration, Instant};

// =========================================================================
// 1. Title Slugification & Special Characters
// =========================================================================

#[test]
fn test_slugify_empty_and_whitespace() {
    assert_eq!(slugify(""), "");
    assert_eq!(slugify("   "), "");
    assert_eq!(slugify("\t\n\r"), "");
}

#[test]
fn test_slugify_special_characters_and_quotes() {
    // Quotes and apostrophes should be stripped without inserting hyphens
    assert_eq!(slugify("It's Okay to Not Be Okay"), "its-okay-to-not-be-okay");
    assert_eq!(slugify("Don't Look Up"), "dont-look-up");
    assert_eq!(slugify("\"The Glory\""), "the-glory");
    assert_eq!(slugify("`Taxi Driver`"), "taxi-driver");
    assert_eq!(slugify("What's Wrong with Secretary Kim"), "whats-wrong-with-secretary-kim");

    // Punctuation and symbols become single hyphens
    assert_eq!(slugify("The Glory: Part 2"), "the-glory-part-2");
    assert_eq!(slugify("F/X2"), "f-x2");
    assert_eq!(slugify("Fast & Furious"), "fast-furious");
    assert_eq!(slugify("Spider-Man: Across the Spider-Verse"), "spider-man-across-the-spider-verse");
    assert_eq!(slugify("Squid --- Game !!! (2021)???"), "squid-game-2021");
    assert_eq!(slugify("Love (ft. Marriage and Divorce)"), "love-ft-marriage-and-divorce");
    assert_eq!(slugify("100 Days My Prince [Special]"), "100-days-my-prince-special");
}

#[test]
fn test_slugify_numeric_and_short_titles() {
    assert_eq!(slugify("1987"), "1987");
    assert_eq!(slugify("2037"), "2037");
    assert_eq!(slugify("365: Repeat the Year"), "365-repeat-the-year");
    assert_eq!(slugify("V"), "v");
    assert_eq!(slugify("W"), "w");
    assert_eq!(slugify("D.P."), "d-p");
}

#[test]
fn test_slugify_asian_and_diacritic_characters() {
    // Non-ASCII characters are skipped in ASCII alphanumeric slugifier
    assert_eq!(slugify("Café Minamdang"), "caf-minamdang");
    assert_eq!(slugify("Amélie"), "am-lie");
    // Korean Hangul only produces empty ASCII slug (triggering fallback search in extractor)
    assert_eq!(slugify("오징어 게임"), "");
}

// =========================================================================
// 2. Episode & Movie Path Building
// =========================================================================

#[test]
fn test_build_episode_paths_matrix() {
    // Movies always map to episode-1
    assert_eq!(
        build_episode_path("parasite", MediaKind::Movie, 1),
        "/parasite-episode-1.html"
    );
    assert_eq!(
        build_episode_path("parasite", MediaKind::Movie, 2),
        "/parasite-episode-1.html"
    );
    assert_eq!(
        build_episode_path("parasite", MediaKind::Movie, 0),
        "/parasite-episode-1.html"
    );

    // TV dramas map to their specific episode
    assert_eq!(
        build_episode_path("squid-game", MediaKind::Tv, 1),
        "/squid-game-episode-1.html"
    );
    assert_eq!(
        build_episode_path("squid-game", MediaKind::Tv, 16),
        "/squid-game-episode-16.html"
    );
    assert_eq!(
        build_episode_path("squid-game", MediaKind::Tv, 100),
        "/squid-game-episode-100.html"
    );
    assert_eq!(
        build_episode_path("squid-game", MediaKind::Tv, 0),
        "/squid-game-episode-0.html"
    );
}

// =========================================================================
// 3. URL Normalization & Video Source Identification
// =========================================================================

#[test]
fn test_url_normalization_variations() {
    assert_eq!(
        normalize_url("//vidbasic.top/embed/4j8bf92mw"),
        "https://vidbasic.top/embed/4j8bf92mw"
    );
    assert_eq!(
        normalize_url("  //vidbasic.top/embed/4j8bf92mw  "),
        "https://vidbasic.top/embed/4j8bf92mw"
    );
    assert_eq!(
        normalize_url("https://asianload.io/streaming.php?id=123"),
        "https://asianload.io/streaming.php?id=123"
    );
    assert_eq!(
        normalize_url("http://streamwish.to/e/abc"),
        "http://streamwish.to/e/abc"
    );
}

#[test]
fn test_is_video_source_and_ad_filtering() {
    // Valid video hosts
    assert!(is_video_source("https://asianload.io/streaming.php?id=123"));
    assert!(is_video_source("https://vidbasic.top/embed/abc"));
    assert!(is_video_source("https://streamwish.to/e/xyz"));
    assert!(is_video_source("https://vidhide.com/v/123"));
    assert!(is_video_source("https://mp4upload.com/embed-123.html"));
    assert!(is_video_source("//standardload.com/embed/123"));

    // Ad & tracking domains must be rejected
    assert!(!is_video_source("https://googleads.g.doubleclick.net/pagead/ads"));
    assert!(!is_video_source("https://histats.com/js15.js"));
    assert!(!is_video_source("https://disqus.com/embed.js"));
    assert!(!is_video_source("https://static.cloudflare.com/beacon.min.js"));
    assert!(!is_video_source("https://connect.facebook.net/en_US/sdk.js"));
    assert!(!is_video_source("https://platform.twitter.com/widgets.js"));
    assert!(!is_video_source("https://www.google-analytics.com/analytics.js"));
    assert!(!is_video_source("https://recaptcha.net/api.js"));
    assert!(!is_video_source("https://adwidget.com/serve?id=999"));

    // Non-HTTP / malformed URLs
    assert!(!is_video_source("javascript:alert(1)"));
    assert!(!is_video_source("data:text/html,<html></html>"));
    assert!(!is_video_source("about:blank"));
    assert!(!is_video_source(""));
}

// =========================================================================
// 4. HTML Source Extraction & Parsing Robustness
// =========================================================================

#[test]
fn test_extract_sources_attribute_variations() {
    let html = r#"
    <div class="block watch-drama">
        <!-- Standard iframe -->
        <iframe src="//vidbasic.top/embed/iframe1" allowfullscreen></iframe>
        
        <!-- Standard server selected -->
        <li class="Standard Server selected" data-video="https://vidbasic.top/embed/server1">Server 1</li>
        
        <!-- Case-insensitive LinkServer class with 1080 token in url -->
        <li class="LINKSERVER active" data-video="https://asianload.io/streaming.php?id=server2_1080p">AsianLoad 1080p</li>
        
        <!-- Generic server tab with 480 token in url -->
        <li class="server-tab" data-video="//streamwish.to/e/server3_480p">StreamWish 480p</li>
        
        <!-- Single quoted data-video -->
        <div data-video='https://vidhide.com/v/server4'></div>
    </div>
    "#;

    let sources = extract_sources_from_html(html);
    assert_eq!(sources.len(), 5);

    // Verify properties
    assert_eq!(sources[0].url, "https://vidbasic.top/embed/server1");
    assert_eq!(sources[0].provider_id.as_deref(), Some(ID));
    assert_eq!(sources[0].is_embed, Some(true));

    assert_eq!(sources[1].url, "https://asianload.io/streaming.php?id=server2_1080p");
    assert_eq!(sources[1].quality, "1080p");

    assert_eq!(sources[2].url, "https://streamwish.to/e/server3_480p");
    assert_eq!(sources[2].quality, "480p");

    assert_eq!(sources[3].url, "https://vidhide.com/v/server4");
    assert_eq!(sources[4].url, "https://vidbasic.top/embed/iframe1");
}

#[test]
fn test_extract_sources_deduplication() {
    let html = r#"
    <div>
        <iframe src="https://vidbasic.top/embed/same_url"></iframe>
        <li class="linkserver" data-video="https://vidbasic.top/embed/same_url">Server</li>
        <div data-video="https://vidbasic.top/embed/same_url"></div>
    </div>
    "#;

    let sources = extract_sources_from_html(html);
    assert_eq!(sources.len(), 1, "Duplicate URLs across tags must be deduplicated");
    assert_eq!(sources[0].url, "https://vidbasic.top/embed/same_url");
}

#[test]
fn test_extract_sources_direct_m3u8() {
    let html = r#"
    <script>
        var playerConfig = {
            file: "https://hls.asianload.io/stream/manifest.m3u8?token=xyz",
            file1080: "https://hls.asianload.io/stream/1080p.m3u8"
        };
    </script>
    <li class="linkserver" data-video="https://cdn.example.com/hls/live.m3u8">Direct Stream</li>
    "#;

    let sources = extract_sources_from_html(html);
    assert_eq!(sources.len(), 3);

    assert_eq!(sources[0].url, "https://cdn.example.com/hls/live.m3u8");
    assert_eq!(sources[0].is_m3u8, true);

    let m3u8_urls: Vec<&str> = sources.iter().map(|s| s.url.as_str()).collect();
    assert!(m3u8_urls.contains(&"https://hls.asianload.io/stream/manifest.m3u8?token=xyz"));
    assert!(m3u8_urls.contains(&"https://hls.asianload.io/stream/1080p.m3u8"));
}

#[test]
fn test_fuzzing_extract_sources_from_html_zero_panic() {
    let huge_str = "x".repeat(100_000);
    let fuzz_inputs = vec![
        "",
        "<!DOCTYPE html>",
        "<li class=\"linkserver\"",
        "<li data-video=\"",
        "<iframe src=\"",
        "<li class=\"linkserver\" data-video=\"https://vidbasic.top/embed/foo\">",
        "\0\0\0\x01\x02\x03<iframe src=\"//test.com/embed\"></iframe>",
        "<html><head><title>Unclosed</title><body><li class='linkserver' data-video='",
        &huge_str,
    ];

    for (i, input) in fuzz_inputs.into_iter().enumerate() {
        let result = std::panic::catch_unwind(|| {
            extract_sources_from_html(input);
        });
        assert!(
            result.is_ok(),
            "extract_sources_from_html panicked on fuzz case #{i}"
        );
    }
}

// =========================================================================
// 5. Search HTML Parsing & Disambiguation
// =========================================================================

#[test]
fn test_parse_search_slug_domain_variations() {
    let html = r#"
    <ul class="list-episode-item">
        <li>
            <a href="https://ww1.dramacool.cx/drama-detail/crash-landing-on-you">
                <h3>Crash Landing on You</h3>
            </a>
        </li>
        <li>
            <a href="https://dramacool.bg/drama-detail/crash-course-in-romance">
                <h3>Crash Course in Romance</h3>
            </a>
        </li>
        <li>
            <a href="/drama-detail/crash">
                <h3>Crash</h3>
            </a>
        </li>
    </ul>
    "#;

    assert_eq!(
        parse_search_slug(html, "Crash Landing on You"),
        Some("crash-landing-on-you".to_string())
    );
    assert_eq!(
        parse_search_slug(html, "Crash Course in Romance"),
        Some("crash-course-in-romance".to_string())
    );
    assert_eq!(
        parse_search_slug(html, "Crash"),
        Some("crash".to_string())
    );
}

#[test]
fn test_parse_search_slug_multi_season_prioritization() {
    let html = r#"
    <ul class="switch-block list-episode-item">
        <li>
            <a href="/drama-detail/taxi-driver-season-2">
                <h3>Taxi Driver Season 2</h3>
            </a>
        </li>
        <li>
            <a href="/drama-detail/taxi-driver">
                <h3>Taxi Driver</h3>
            </a>
        </li>
        <li>
            <a href="/drama-detail/taxi-driver-season-3">
                <h3>Taxi Driver Season 3</h3>
            </a>
        </li>
    </ul>
    "#;

    // Searching Season 1 / base title prefers base slug without 'season'
    assert_eq!(
        parse_search_slug(html, "Taxi Driver"),
        Some("taxi-driver".to_string())
    );

    // Searching Season 2 explicitly matches season-2
    assert_eq!(
        parse_search_slug(html, "Taxi Driver Season 2"),
        Some("taxi-driver-season-2".to_string())
    );
}

#[test]
fn test_parse_search_slug_empty_and_no_match() {
    assert_eq!(parse_search_slug("", "Squid Game"), None);
    assert_eq!(
        parse_search_slug("<div>No results found</div>", "NonExistentShow"),
        None
    );
}

// =========================================================================
// 6. Extractor Input Edge Cases & Live Search
// =========================================================================

#[tokio::test]
async fn test_scrape_empty_or_whitespace_title_returns_none() {
    assert!(scrape("93405", MediaKind::Tv, 1, 1, None).await.is_none());
    assert!(scrape("93405", MediaKind::Tv, 1, 1, Some("")).await.is_none());
    assert!(scrape("93405", MediaKind::Tv, 1, 1, Some("   \t\n")).await.is_none());
}

#[tokio::test]
async fn test_live_search_drama_slug() {
    let slug = search_drama_slug("Squid Game").await;
    assert_eq!(slug, Some("the-squid-games".to_string()));
}

// =========================================================================
// 7. Aggregator Integration & Concurrency Stress
// =========================================================================

#[tokio::test]
async fn test_run_all_aggregator_includes_dramacool() {
    let results = run_all("94796", MediaKind::Tv, 1, 1, Some("Crash Landing on You"), Some(2019)).await;
    let dramacool_res = results.iter().find(|r| r.provider_id == ID);

    assert!(
        dramacool_res.is_some(),
        "Expected run_all to include dramacool in results for Crash Landing on You"
    );
    let dc = dramacool_res.unwrap();
    assert!(!dc.sources.is_empty(), "DramaCool sources in run_all must not be empty");
    println!(
        "[Aggregator Test] ✅ run_all successfully included DramaCool with {} sources",
        dc.sources.len()
    );
}

#[tokio::test]
async fn test_dramacool_concurrency_stress() {
    let titles = vec![
        ("93405", MediaKind::Tv, 1, 1, "Squid Game"),
        ("94796", MediaKind::Tv, 1, 1, "Crash Landing on You"),
        ("136283", MediaKind::Tv, 1, 1, "The Glory"),
        ("496243", MediaKind::Movie, 1, 1, "Parasite"),
        ("96162", MediaKind::Tv, 1, 1, "It's Okay to Not Be Okay"),
    ];

    let t0 = Instant::now();
    let tasks: Vec<_> = titles
        .into_iter()
        .map(|(tmdb_id, kind, season, ep, title)| async move {
            scrape(tmdb_id, kind, season, ep, Some(title)).await
        })
        .collect();

    let results = futures::future::join_all(tasks).await;
    let elapsed = t0.elapsed();

    println!(
        "[Concurrency Stress] Completed 5 parallel scrapes in {elapsed:?}"
    );

    for (idx, r) in results.into_iter().enumerate() {
        assert!(
            r.is_some(),
            "Parallel scrape task #{idx} failed unexpectedly"
        );
        let res = r.unwrap();
        assert!(!res.sources.is_empty());
    }
}

// =========================================================================
// 8. Live Multi-Drama Empirical Resolution Tests against ww1.dramacool.cx
// =========================================================================

#[tokio::test]
async fn test_live_multidrama_e2e_matrix() {
    struct TestCase {
        name: &'static str,
        tmdb_id: &'static str,
        kind: MediaKind,
        season: u32,
        episode: u32,
        title: &'static str,
        expect_success: bool,
    }

    let test_matrix = vec![
        TestCase {
            name: "Squid Game (K-Drama Tv S1E1)",
            tmdb_id: "93405",
            kind: MediaKind::Tv,
            season: 1,
            episode: 1,
            title: "Squid Game",
            expect_success: true,
        },
        TestCase {
            name: "Crash Landing on You (K-Drama Romance S1E1)",
            tmdb_id: "94796",
            kind: MediaKind::Tv,
            season: 1,
            episode: 1,
            title: "Crash Landing on You",
            expect_success: true,
        },
        TestCase {
            name: "The Glory (K-Drama Revenge S1E1)",
            tmdb_id: "136283",
            kind: MediaKind::Tv,
            season: 1,
            episode: 1,
            title: "The Glory",
            expect_success: true,
        },
        TestCase {
            name: "Parasite (Korean Oscar Movie)",
            tmdb_id: "496243",
            kind: MediaKind::Movie,
            season: 1,
            episode: 1,
            title: "Parasite",
            expect_success: true,
        },
        TestCase {
            name: "It's Okay to Not Be Okay (Special characters in title)",
            tmdb_id: "96162",
            kind: MediaKind::Tv,
            season: 1,
            episode: 1,
            title: "It's Okay to Not Be Okay",
            expect_success: true,
        },
        TestCase {
            name: "Weak Hero Class 1 (Action Webtoon Drama S1E1)",
            tmdb_id: "196417",
            kind: MediaKind::Tv,
            season: 1,
            episode: 1,
            title: "Weak Hero Class 1",
            expect_success: true,
        },
        TestCase {
            name: "Non-Existent Drama (Resilience check)",
            tmdb_id: "99999999",
            kind: MediaKind::Tv,
            season: 1,
            episode: 1,
            title: "TotallyFakeDramaNonExistent99999",
            expect_success: false,
        },
    ];

    let mut successful_resolutions = 0;
    let total_live_tests = test_matrix.len();

    for tc in test_matrix {
        println!("\n========================================================");
        println!("[Empirical Test] Testing Drama: {}", tc.name);
        println!("========================================================");

        let t0 = Instant::now();
        let mut res = scrape(tc.tmdb_id, tc.kind, tc.season, tc.episode, Some(tc.title)).await;
        
        // Single retry if network packet was throttled
        if tc.expect_success && res.is_none() {
            tokio::time::sleep(Duration::from_millis(150)).await;
            res = scrape(tc.tmdb_id, tc.kind, tc.season, tc.episode, Some(tc.title)).await;
        }
        
        let elapsed = t0.elapsed();

        println!("[Empirical Test] Completed in {elapsed:?}");

        if tc.expect_success {
            assert!(
                res.is_some(),
                "Expected successful scrape for '{}', but got None (took {:?})",
                tc.name,
                elapsed
            );

            let provider_res = res.unwrap();
            assert_eq!(provider_res.provider_id, ID);
            assert!(
                !provider_res.sources.is_empty(),
                "Expected non-empty sources for '{}'",
                tc.name
            );

            println!(
                "[Empirical Test] ✅ Successfully resolved {} source(s) for '{}':",
                provider_res.sources.len(),
                tc.name
            );

            for (idx, source) in provider_res.sources.iter().enumerate() {
                println!(
                    "  [Source #{idx}] URL: {} | Quality: {:?} | is_m3u8: {:?} | is_embed: {:?} | Referer: {:?}",
                    source.url, source.quality, source.is_m3u8, source.is_embed, source.referer
                );

                assert!(
                    source.url.starts_with("http://") || source.url.starts_with("https://"),
                    "Source URL '{}' must start with http:// or https://",
                    source.url
                );
                assert_eq!(
                    source.provider_id.as_deref(),
                    Some(ID),
                    "Provider ID must match 'dramacool'"
                );
            }

            successful_resolutions += 1;
        } else {
            assert!(
                res.is_none(),
                "Expected None for non-existent drama '{}', but got {res:?}",
                tc.name
            );
            println!("[Empirical Test] ✅ Correctly returned None for non-existent title");
            successful_resolutions += 1;
        }
    }

    println!(
        "\n[Empirical Test Summary] Passed {}/{} live drama resolution test cases successfully.",
        successful_resolutions, total_live_tests
    );
    assert_eq!(successful_resolutions, total_live_tests);
}

#[tokio::test]
async fn test_live_dramacool_html_structure_inspection() {
    let url = "https://ww1.dramacool.cx/crash-landing-on-you-episode-1.html";
    let html = get_text_with(
        &PROXY,
        url,
        Duration::from_secs(8),
        &[
            ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
            ("Referer", "https://ww1.dramacool.cx/"),
        ],
    )
    .await;

    assert!(html.is_some(), "Direct fetch to DramaCool episode page should succeed");
    let body = html.unwrap();
    println!("[Live Inspection] Page length: {} bytes", body.len());

    let sources = extract_sources_from_html(&body);
    println!("[Live Inspection] Extracted {} sources from live page", sources.len());
    for s in &sources {
        println!("  Extracted: url={} embed={:?}", s.url, s.is_embed);
        assert!(!s.url.is_empty());
        assert!(s.url.contains("embed") || s.url.contains(".m3u8") || s.url.contains(".mp4"));
    }
    assert!(!sources.is_empty(), "Live page must contain extractable sources");
}
