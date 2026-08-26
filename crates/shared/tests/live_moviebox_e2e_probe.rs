//! End-to-End Live Probe for MovieBox Search and Stream Resolution

use pstream_shared::extractors::moviebox::{extract_stream_from_html, SearchResponse};
use std::time::Duration;

#[tokio::test]
async fn test_moviebox_live_search_and_detail_e2e() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let search_api = "https://h5-api.aoneroom.com/wefeed-h5api-bff/subject/search";
    let detail_base = "https://movieboxonline.net/movies";

    let titles = vec!["Avatar", "Inception", "Interstellar", "Fight Club", "The Dark Knight"];

    for title in titles {
        println!("\n=======================================================");
        println!("[Probe] Testing live title: '{title}'");

        let search_body = serde_json::json!({
            "keyword": title,
            "page": 1,
            "perPage": 10,
            "subjectType": 0
        });

        let resp = client
            .post(search_api)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/plain, */*")
            .header("Origin", "https://h5.aoneroom.com")
            .header("Referer", "https://h5.aoneroom.com/")
            .json(&search_body)
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                println!("[Probe] Search request failed: {e}");
                continue;
            }
        };

        let status = resp.status();
        println!("[Probe] Search API Status: {status}");
        let search_res: SearchResponse = match resp.json().await {
            Ok(res) => res,
            Err(e) => {
                println!("[Probe] Failed to deserialize SearchResponse: {e}");
                continue;
            }
        };

        let items = search_res.data.and_then(|d| d.items).unwrap_or_default();
        println!("[Probe] Found {} items in search results", items.len());

        for (i, item) in items.iter().enumerate().take(5) {
            println!(
                "  Item #{i}: title={:?}, releaseDate={:?}, detailPath={:?}",
                item.title, item.release_date, item.detail_path
            );
        }

        let first_item = items.iter().find(|i| {
            i.detail_path
                .as_ref()
                .map(|p| !p.trim().is_empty())
                .unwrap_or(false)
        });

        if let Some(item) = first_item {
            let detail_path = item.detail_path.as_ref().unwrap();
            let detail_url = format!("{detail_base}/{}", detail_path.trim_start_matches('/'));
            println!("[Probe] Fetching detail URL: {detail_url}");

            let detail_resp = client
                .get(&detail_url)
                .header(
                    "Accept",
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                )
                .header("Accept-Language", "en-US,en;q=0.9")
                .header("Referer", "https://movieboxonline.net/")
                .send()
                .await;

            match detail_resp {
                Ok(d_resp) => {
                    let d_status = d_resp.status();
                    println!("[Probe] Detail page status: {d_status}");
                    let html = d_resp.text().await.unwrap_or_default();
                    println!("[Probe] Detail page HTML length: {} bytes", html.len());

                    let stream = extract_stream_from_html(&html);
                    println!("[Probe] Stream extraction result: {:?}", stream);

                    if let Some(ref url) = stream {
                        println!("[Probe] ✅ Successfully resolved stream URL: {url}");
                    } else {
                        println!("[Probe] ⚠️ No stream URL found in detail page HTML");
                        // Check if __NUXT_DATA__ or __NUXT__ exists in HTML
                        if html.contains("__NUXT_DATA__") {
                            println!("[Probe] HTML contains __NUXT_DATA__ tag");
                        }
                        if html.contains("__NUXT__") {
                            println!("[Probe] HTML contains __NUXT__");
                        }
                    }
                }
                Err(e) => {
                    println!("[Probe] Detail page fetch error: {e}");
                }
            }
        }
    }
}
