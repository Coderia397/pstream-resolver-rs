use std::time::Duration;

#[tokio::test]
async fn test_search_api_header_matrix() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let url = "https://h5-api.aoneroom.com/wefeed-h5api-bff/subject/search";
    let body = serde_json::json!({
        "keyword": "Avatar",
        "page": 1,
        "perPage": 5,
        "subjectType": 0
    });

    let matrix = vec![
        ("movieboxonline origin", Some("https://movieboxonline.net"), Some("https://movieboxonline.net/")),
        ("aoneroom origin/referer", Some("https://h5.aoneroom.com"), Some("https://h5.aoneroom.com/")),
        ("no origin, aoneroom referer", None, Some("https://h5.aoneroom.com/")),
        ("no origin, no referer", None, None),
    ];

    for (name, origin, referer) in matrix {
        let mut req = client.post(url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/plain, */*");
        if let Some(o) = origin {
            req = req.header("Origin", o);
        }
        if let Some(r) = referer {
            req = req.header("Referer", r);
        }
        let res = req.json(&body).send().await.unwrap();
        let status = res.status();
        let body_str = res.text().await.unwrap_or_default();
        let is_ok = body_str.contains("\"code\":0");
        println!("[Matrix: {name}] Status: {status}, Success: {is_ok}, Snippet: {}", &body_str[..body_str.len().min(100)]);
    }
}
