//! Deep probe for MovieBox BFF API tokens and headers

use std::time::Duration;

#[tokio::test]
async fn test_probe_moviebox_headers() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap();

    let search_url = "https://h5-api.aoneroom.com/wefeed-h5api-bff/subject/search";

    // Test with various common headers
    let header_variations: Vec<(&str, Vec<(&str, &str)>)> = vec![
        ("Standard Origin/Referer", vec![
            ("Origin", "https://movieboxonline.net"),
            ("Referer", "https://movieboxonline.net/"),
        ]),
        ("With User-Agent", vec![
            ("Origin", "https://movieboxonline.net"),
            ("Referer", "https://movieboxonline.net/"),
            ("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"),
        ]),
        ("With Sec-Fetch headers", vec![
            ("Origin", "https://movieboxonline.net"),
            ("Referer", "https://movieboxonline.net/"),
            ("Sec-Fetch-Dest", "empty"),
            ("Sec-Fetch-Mode", "cors"),
            ("Sec-Fetch-Site", "cross-site"),
        ]),
        ("Aoneroom Referer", vec![
            ("Origin", "https://h5.aoneroom.com"),
            ("Referer", "https://h5.aoneroom.com/"),
        ]),
    ];

    let payload = serde_json::json!({
        "keyword": "Inception",
        "page": 1,
        "perPage": 10,
        "subjectType": 0
    });

    for (name, headers) in header_variations {
        let mut req = client.post(search_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/plain, */*");
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let res = req.json(&payload).send().await;
        match res {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                println!("[Header Probe: {name}] Status: {status}, Body: {text}");
            }
            Err(e) => {
                println!("[Header Probe: {name}] Err: {e}");
            }
        }
    }
}
