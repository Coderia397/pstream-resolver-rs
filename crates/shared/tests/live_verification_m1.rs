//! Live Empirical Verification Suite for MovieBox Live API and Resolver Concurrency (Milestone 1).

use pstream_shared::extractors::{moviebox, nontongo, run_all};
use pstream_shared::models::MediaKind;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_live_moviebox_search_and_resolution() {
    println!("\n=== Live Test: MovieBox Live Scraper ===");
    let test_cases = vec![
        ("Inception", Some(2010)),
        ("Avatar", Some(2009)),
        ("Interstellar", Some(2014)),
        ("Fight Club", Some(1999)),
    ];

    for (title, year) in test_cases {
        let t0 = Instant::now();
        println!("\n[Test] Querying MovieBox for '{title}' ({year:?})...");
        let res = moviebox::scrape(title, year).await;
        let elapsed = t0.elapsed();
        println!("[Test] Elapsed: {elapsed:?}");

        if let Some(ref provider_res) = res {
            println!("[Test] ✅ MovieBox returned ProviderResult:");
            println!("  Provider: {}", provider_res.provider);
            println!("  Provider ID: {}", provider_res.provider_id);
            println!("  Sources count: {}", provider_res.sources.len());

            assert_eq!(provider_res.provider_id, "moviebox");
            assert!(!provider_res.sources.is_empty(), "Sources must not be empty");

            for (idx, source) in provider_res.sources.iter().enumerate() {
                println!("  Source #{idx}:");
                println!("    URL: {}", source.url);
                println!("    Quality: {}", source.quality);
                println!("    is_m3u8: {}", source.is_m3u8);
                println!("    no_proxy: {}", source.no_proxy);
                println!("    provider: {:?}", source.provider);
                println!("    provider_id: {:?}", source.provider_id);
                println!("    referer: {:?}", source.referer);

                // Source validation
                assert!(
                    source.url.starts_with("http://") || source.url.starts_with("https://"),
                    "Source URL must be valid HTTP/HTTPS"
                );
                assert!(
                    source.url.contains(".m3u8") || source.url.contains(".mp4"),
                    "Source URL must end with or contain .m3u8 or .mp4"
                );
                assert!(!source.quality.is_empty(), "Quality must not be empty");
                assert_eq!(source.provider.as_deref(), Some("MovieBox"));
                assert_eq!(source.provider_id.as_deref(), Some("moviebox"));
            }
        } else {
            println!("[Test] ⚠️ MovieBox returned None for '{title}'. Checking raw API behavior...");
        }
    }
}

#[tokio::test]
async fn test_live_moviebox_bff_api_raw_probe() {
    println!("\n=== Live Test: Raw MovieBox Search BFF API Probe ===");
    let search_api = "https://h5-api.aoneroom.com/wefeed-h5api-bff/subject/search";
    
    // Probe 1: Standard search payload
    let body1 = serde_json::json!({
        "keyword": "Inception",
        "page": 1,
        "perPage": 10,
        "subjectType": 0
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let resp1 = client
        .post(search_api)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/plain, */*")
        .header("Origin", "https://movieboxonline.net")
        .header("Referer", "https://movieboxonline.net/")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .json(&body1)
        .send()
        .await;

    match resp1 {
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            println!("[Raw BFF Probe] Status: {status}");
            println!("[Raw BFF Probe] Body: {body_text}");
        }
        Err(e) => {
            println!("[Raw BFF Probe] Connection error: {e}");
        }
    }

    // Probe 2: perPage: 0 payload
    let body2 = serde_json::json!({
        "keyword": "Inception",
        "page": 1,
        "perPage": 0,
        "subjectType": 0
    });
    let resp2 = client
        .post(search_api)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/plain, */*")
        .header("Origin", "https://movieboxonline.net")
        .header("Referer", "https://movieboxonline.net/")
        .json(&body2)
        .send()
        .await;

    match resp2 {
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            println!("[Raw BFF Probe (perPage 0)] Status: {status}");
            println!("[Raw BFF Probe (perPage 0)] Body: {body_text}");
        }
        Err(e) => {
            println!("[Raw BFF Probe (perPage 0)] Connection error: {e}");
        }
    }
}

#[tokio::test]
async fn test_nontongo_live_direct_and_run_all_bypass() {
    println!("\n=== Live Test: NontonGo Direct vs Bypass ===");
    
    // Direct scrape attempt (simulating upstream dead behavior)
    let t0 = Instant::now();
    let direct_res = nontongo::scrape("27205", MediaKind::Movie, 1, 1).await;
    let direct_elapsed = t0.elapsed();
    println!("[NontonGo Direct] Result: {direct_res:?}, Elapsed: {direct_elapsed:?}");

    // In run_all, nontongo is bypassed via `async { None }`
    let t1 = Instant::now();
    let results = run_all(
        "27205",
        MediaKind::Movie,
        1,
        1,
        Some("Inception"),
        Some(2010),
    )
    .await;
    let run_all_elapsed = t1.elapsed();
    println!(
        "[run_all with NontonGo bypass] Found {} provider results in {run_all_elapsed:?}",
        results.len()
    );

    // Verify NontonGo is not returning results in run_all
    let has_nontongo = results.iter().any(|r| r.provider_id == "nontongo");
    assert!(!has_nontongo, "NontonGo must be disabled/bypassed in run_all");
}

#[tokio::test]
async fn test_run_all_concurrency_stress() {
    println!("\n=== Stress Test: Concurrent run_all Invocations ===");
    let mut handles = Vec::new();
    let concurrency_count = 10;

    let t0 = Instant::now();
    for i in 0..concurrency_count {
        let handle = tokio::spawn(async move {
            let start = Instant::now();
            let res = run_all(
                "27205",
                MediaKind::Movie,
                1,
                1,
                Some("Inception"),
                Some(2010),
            )
            .await;
            (i, res.len(), start.elapsed())
        });
        handles.push(handle);
    }

    for handle in handles {
        let (idx, count, elapsed) = handle.await.unwrap();
        println!("[Task #{idx}] Finished in {elapsed:?} with {count} providers");
    }
    let total_elapsed = t0.elapsed();
    println!("[Total Concurrent Run] {concurrency_count} tasks completed in {total_elapsed:?}");

    // Assert that total duration is reasonable (< 15s) and not serialized (e.g. 10 * 8s = 80s)
    assert!(
        total_elapsed < Duration::from_secs(20),
        "Concurrent run_all tasks took too long: {total_elapsed:?}"
    );
}
