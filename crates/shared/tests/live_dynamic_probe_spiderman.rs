//! Live dynamic probe tests for titles like Spider-Man: No Way Home and multi-source resolution.

use pstream_shared::extractors::{moviebox, run_all};
use pstream_shared::models::MediaKind;
use pstream_shared::utils::quality_rank;
use std::time::Instant;

#[tokio::test]
async fn test_live_probe_spiderman_no_way_home_resolution() {
    println!("\n=== Live Dynamic Probe: Spider-Man: No Way Home (2021) ===");
    let tmdb_id = "634649"; // Spider-Man: No Way Home
    let title = "Spider-Man: No Way Home";
    let year = 2021;

    let t0 = Instant::now();
    // 1. Probe MovieBox directly
    println!("[Live Probe] Testing MovieBox for \"{title}\" ({year})...");
    let moviebox_res = moviebox::scrape(title, Some(year)).await;
    let mb_elapsed = t0.elapsed();
    println!("[Live Probe] MovieBox completed in {mb_elapsed:?}");

    if let Some(ref res) = moviebox_res {
        println!("  ✅ MovieBox resolved {} source(s):", res.sources.len());
        for (i, s) in res.sources.iter().enumerate() {
            println!("    [{i}] URL: {}, Quality: {}, is_m3u8: {}", s.url, s.quality, s.is_m3u8);
            assert!(!s.quality.is_empty());
            assert_ne!(s.quality, "480p", "Expected high quality stream, not 480p fallback");
            assert!(
                s.url.contains(".m3u8") || s.url.contains(".mp4"),
                "Stream URL must be m3u8 or mp4"
            );
        }
    } else {
        println!("  ⚠️ MovieBox live scrape returned None (offline or BFF rate limit)");
    }

    // 2. Full pipeline run_all aggregation
    let t1 = Instant::now();
    let results = run_all(tmdb_id, MediaKind::Movie, 1, 1, Some(title), Some(year)).await;
    let total_elapsed = t1.elapsed();
    println!("[Live Probe] run_all returned {} provider results in {total_elapsed:?}", results.len());

    let mut all_sources: Vec<_> = results.iter().flat_map(|r| r.sources.iter().cloned()).collect();
    if !all_sources.is_empty() {
        // Apply resolver sorting logic
        all_sources.sort_by(|a, b| {
            let rank_a = quality_rank(&a.quality);
            let rank_b = quality_rank(&b.quality);
            if rank_a != rank_b {
                return rank_b.cmp(&rank_a);
            }
            let embed_a = a.is_embed.unwrap_or(false);
            let embed_b = b.is_embed.unwrap_or(false);
            embed_a.cmp(&embed_b)
        });

        let top_source = &all_sources[0];
        println!("  🌟 Top Source (data.sources[0]):");
        println!("    Provider: {:?}", top_source.provider);
        println!("    Quality: {}", top_source.quality);
        println!("    URL: {}", top_source.url);
        println!("    is_embed: {:?}", top_source.is_embed);
        println!("    is_m3u8: {}", top_source.is_m3u8);

        // Quality must be at least 720p or 1080p or auto
        let rank = quality_rank(&top_source.quality);
        assert!(
            rank >= 720,
            "data.sources[0] quality rank ({rank} / {}) should be >= 720 (HD or FHD)",
            top_source.quality
        );
    }
}

#[tokio::test]
async fn test_live_probe_avatar_and_remake_disambiguation() {
    println!("\n=== Live Dynamic Probe: Avatar (2009) vs Avatar (2024 / Re-release) ===");
    
    // MovieBox search BFF API probe with year filter
    let res_2009 = moviebox::scrape("Avatar", Some(2009)).await;
    println!("[Live Probe] Avatar (2009) result: {res_2009:?}");

    let res_2022 = moviebox::scrape("Avatar: The Way of Water", Some(2022)).await;
    println!("[Live Probe] Avatar: The Way of Water (2022) result: {res_2022:?}");
}
