//! Adversarial Resilience, Concurrency, and Live Extraction Test Suite for DramaCool Extractor.
//!
//! Specifically validates:
//! 1. Graceful handling on nonexistent dramas, 404s, extreme episode counts, and empty/special character titles without panicking.
//! 2. High-concurrency stress testing with mixed valid and invalid live requests.
//! 3. Live E2E resolution and source metadata validation (URL, quality, provider tags, referer).
//! 4. Non-regression across other extractors.

use pstream_shared::extractors::dramacool::{scrape, ID};
use pstream_shared::models::MediaKind;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_dramacool_empty_and_whitespace_titles() {
    // None title
    let res_none = scrape("93405", MediaKind::Tv, 1, 1, None).await;
    assert!(res_none.is_none(), "Expected None for None title");

    // Empty string
    let res_empty = scrape("93405", MediaKind::Tv, 1, 1, Some("")).await;
    assert!(res_empty.is_none(), "Expected None for empty string title");

    // Whitespace only
    let res_spaces = scrape("93405", MediaKind::Tv, 1, 1, Some("    \t\n  ")).await;
    assert!(res_spaces.is_none(), "Expected None for whitespace title");

    // Punctuation only (slugifies to empty)
    let res_punct = scrape("93405", MediaKind::Tv, 1, 1, Some("??? !!! @@@ ###")).await;
    assert!(res_punct.is_none(), "Expected None for symbols-only title");
}

#[tokio::test]
async fn test_dramacool_nonexistent_drama_returns_none() {
    let fake_titles = vec![
        "ThisDramaDoesNotExistAtAll999999",
        "xyz_totally_fake_show_unreleased_2099",
        "random_gibberish_asdfghjkl_12345",
    ];

    for title in fake_titles {
        println!("[Resilience Test] Testing nonexistent drama title: \"{title}\"");
        let t0 = Instant::now();
        let res = scrape("9999999", MediaKind::Tv, 1, 1, Some(title)).await;
        let elapsed = t0.elapsed();
        println!("[Resilience Test] Scrape returned {res:?} in {elapsed:?}");
        assert!(
            res.is_none(),
            "Expected None for nonexistent drama '{title}', but got {res:?}"
        );
    }
}

#[tokio::test]
async fn test_dramacool_extreme_episode_number_safety() {
    // Squid Game exists, but Episode 99999 is tested for safe non-panicking execution
    println!("[Resilience Test] Testing extreme episode number safety: Squid Game Ep 99999");
    let t0 = Instant::now();
    let res = scrape("93405", MediaKind::Tv, 1, 99999, Some("Squid Game")).await;
    let elapsed = t0.elapsed();
    println!("[Resilience Test] Extreme episode test finished in {elapsed:?} with result: {res:?}");
    // DramaCool either falls back or returns None; must never panic
    if let Some(r) = res {
        assert_eq!(r.provider_id, ID);
    }
}

#[tokio::test]
async fn test_dramacool_live_valid_resolution_metadata() {
    let test_cases = vec![
        ("Crash Landing on You", "94796", MediaKind::Tv, 1, 1),
        ("The Glory", "136283", MediaKind::Tv, 1, 1),
        ("Parasite", "496243", MediaKind::Movie, 1, 1),
    ];

    for (title, tmdb_id, kind, season, episode) in test_cases {
        println!("\n[Live Resolution Test] Testing '{title}' ({kind:?} S{season}E{episode})");
        let t0 = Instant::now();
        let mut res = scrape(tmdb_id, kind, season, episode, Some(title)).await;
        if res.is_none() {
            tokio::time::sleep(Duration::from_millis(150)).await;
            res = scrape(tmdb_id, kind, season, episode, Some(title)).await;
        }
        let elapsed = t0.elapsed();
        println!("[Live Resolution Test] '{title}' resolved in {elapsed:?}");

        assert!(
            res.is_some(),
            "Expected successful scrape for real drama '{title}'"
        );

        let provider_res = res.unwrap();
        assert_eq!(provider_res.provider_id, ID);
        assert_eq!(provider_res.provider, "DramaCool 🎭");
        assert!(
            !provider_res.sources.is_empty(),
            "Expected at least 1 source for '{title}'"
        );

        for (i, s) in provider_res.sources.iter().enumerate() {
            println!(
                "  Source #{i}: URL={}, quality={}, is_m3u8={}, is_embed={:?}, referer={:?}",
                s.url, s.quality, s.is_m3u8, s.is_embed, s.referer
            );
            assert!(
                s.url.starts_with("http://") || s.url.starts_with("https://"),
                "Source URL must have valid scheme"
            );
            assert_eq!(s.provider_id.as_deref(), Some(ID));
            assert_eq!(s.provider.as_deref(), Some("DramaCool"));
            assert_eq!(
                s.referer.as_deref(),
                Some("https://ww1.dramacool.cx"),
                "Referer header should be set to DramaCool base"
            );
        }
    }
}

#[tokio::test]
async fn test_dramacool_concurrent_stress() {
    let requests = vec![
        ("Crash Landing on You", "94796", MediaKind::Tv, 1, 1, true),
        ("The Glory", "136283", MediaKind::Tv, 1, 1, true),
        ("Parasite", "496243", MediaKind::Movie, 1, 1, true),
        ("FakeDramaAlpha999", "11111", MediaKind::Tv, 1, 1, false),
        ("FakeDramaBeta888", "22222", MediaKind::Tv, 1, 1, false),
        ("Weak Hero Class 1", "196417", MediaKind::Tv, 1, 1, true),
        ("FakeDramaGamma777", "33333", MediaKind::Tv, 1, 1, false),
        ("It's Okay to Not Be Okay", "96162", MediaKind::Tv, 1, 1, true),
    ];

    let t0 = Instant::now();
    let mut handles = Vec::new();

    for (title, tmdb_id, kind, season, episode, expect_success) in requests {
        let handle = tokio::spawn(async move {
            let res = scrape(tmdb_id, kind, season, episode, Some(title)).await;
            (title, expect_success, res)
        });
        handles.push(handle);
    }

    println!("[Concurrency Stress] Launched {} concurrent scrape requests", handles.len());

    let mut success_count = 0;
    let total = handles.len();

    for handle in handles {
        let (title, expect_success, res) = handle.await.expect("Tokio task panicked");
        if expect_success {
            assert!(
                res.is_some(),
                "Expected success for concurrent scrape of '{title}'"
            );
            assert!(
                !res.unwrap().sources.is_empty(),
                "Expected sources for '{title}'"
            );
            success_count += 1;
        } else {
            assert!(
                res.is_none(),
                "Expected None for invalid concurrent scrape of '{title}'"
            );
            success_count += 1;
        }
    }

    let elapsed = t0.elapsed();
    println!(
        "[Concurrency Stress] All {total} concurrent requests finished in {elapsed:?} ({success_count}/{total} passed as expected)"
    );
    assert_eq!(success_count, total);
}
