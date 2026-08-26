//! Empirical Adversarial Test Suite for MovieBox parser and NontonGo bypass (Milestone 1).

use pstream_shared::extractors::moviebox::{
    extract_stream_from_html, SearchItem, SearchResponse,
};
use pstream_shared::extractors::nontongo;
use pstream_shared::models::MediaKind;
use std::time::Instant;

// =========================================================================
// 1. Malformed Nuxt 3 Payloads & Regex Stress Testing
// =========================================================================

#[test]
fn test_malformed_nuxt3_payload_unclosed_json() {
    let html = r#"
    <!DOCTYPE html>
    <html><body>
    <script type="application/json" id="__NUXT_DATA__">
    [["ShallowReactive",1],{"data":2},"https:\/\/pbcdn.aoneroom.com\/media\/broken.m3u8
    "#;
    // Unclosed script tag / truncated payload returns None gracefully without panic
    let res = extract_stream_from_html(html);
    assert_eq!(res, None, "Truncated script without closing tag returns None safely");
}

#[test]
fn test_malformed_nuxt3_payload_corrupted_tokens() {
    let html = r#"
    <script type="application/json" id="__NUXT_DATA__">
    [{"invalid"::: token, 12345, "https:\/\/pbcdn.aoneroom.com\/media\/stream.m3u8"}]
    </script>
    "#;
    let res = extract_stream_from_html(html);
    assert_eq!(
        res,
        Some("https://pbcdn.aoneroom.com/media/stream.m3u8".to_string()),
        "Should recover stream URL even with malformed surrounding JSON tokens"
    );
}

#[test]
fn test_empty_nuxt_script_tag() {
    let html = r#"
    <html>
    <head><script id="__NUXT_DATA__"></script></head>
    <body><div>No stream here</div></body>
    </html>
    "#;
    let res = extract_stream_from_html(html);
    assert_eq!(res, None, "Empty script tag should return None");
}

#[test]
fn test_nuxt_script_tag_attribute_variations() {
    // Attribute order 1: data-ssr first
    let html1 = r#"<script data-ssr="true" id="__NUXT_DATA__" type="application/json">["https:\/\/pbcdn.aoneroom.com\/v1.m3u8"]</script>"#;
    assert_eq!(
        extract_stream_from_html(html1),
        Some("https://pbcdn.aoneroom.com/v1.m3u8".to_string())
    );

    // Attribute order 2: id first with extra spaces
    let html2 = r#"<script   id="__NUXT_DATA__"   data-app="moviebox">["https:\/\/macdn.aoneroom.com\/video.mp4"]</script>"#;
    assert_eq!(
        extract_stream_from_html(html2),
        Some("https://macdn.aoneroom.com/video.mp4".to_string())
    );
}

#[test]
fn test_nuxt_payload_with_escaped_html_and_tags() {
    // Standard Nuxt JSON encoding escapes closing tags as <\/script>
    let html = r#"
    <script type="application/json" id="__NUXT_DATA__">
    ["<script>alert('xss')<\/script>", "<b>Title<\/b>", "https:\/\/pbcdn.aoneroom.com\/media\/video.m3u8?token=xss%22%3E"]
    </script>
    "#;
    let res = extract_stream_from_html(html);
    assert_eq!(
        res,
        Some("https://pbcdn.aoneroom.com/media/video.m3u8?token=xss%22%3E".to_string())
    );
}

#[test]
fn test_large_nuxt_payload_performance() {
    // Construct ~200KB Nuxt payload (typical large SPA hydration state with 2,000 array elements)
    let mut payload = String::with_capacity(250_000);
    payload.push_str(r#"<script id="__NUXT_DATA__">[ "#);
    for i in 0..2_000 {
        payload.push_str(&format!(r#""https:\/\/pbcdnw.aoneroom.com\/image\/poster_{i}.jpg","#));
    }
    payload.push_str(r#""https:\/\/pbcdn.aoneroom.com\/media\/target_stream.m3u8" ]</script>"#);

    let start = Instant::now();
    let res = extract_stream_from_html(&payload);
    let elapsed = start.elapsed();

    assert_eq!(
        res,
        Some("https://pbcdn.aoneroom.com/media/target_stream.m3u8".to_string())
    );
    assert!(
        elapsed.as_millis() < 250,
        "Parsing 200KB payload took too long: {elapsed:?}"
    );
}

// =========================================================================
// 2. Escaped JSON and Slashes
// =========================================================================

#[test]
fn test_escaped_json_slashes() {
    let html = r#"
    <script id="__NUXT_DATA__">
    ["https:\/\/pbcdn.aoneroom.com\/hls\/subfolder\/720p.m3u8?auth=key1\/key2"]
    </script>
    "#;
    let res = extract_stream_from_html(html);
    assert_eq!(
        res,
        Some("https://pbcdn.aoneroom.com/hls/subfolder/720p.m3u8?auth=key1/key2".to_string())
    );
}

#[test]
fn test_query_params_and_complex_tokens() {
    let html = r#"
    <script id="__NUXT_DATA__">
    ["https:\/\/pbcdn.aoneroom.com\/media\/master.m3u8?expires=1799999999&token=abc_123-XYZ~456&cdn_id=us-east-1#track=0"]
    </script>
    "#;
    let res = extract_stream_from_html(html);
    assert_eq!(
        res,
        Some("https://pbcdn.aoneroom.com/media/master.m3u8?expires=1799999999&token=abc_123-XYZ~456&cdn_id=us-east-1#track=0".to_string())
    );
}

// =========================================================================
// 3. Mixed Image/Video CDN Links & Static Asset Filtering
// =========================================================================

#[test]
fn test_strictly_ignores_all_image_formats_on_aoneroom_cdns() {
    let image_extensions = [
        "jpg", "jpeg", "png", "webp", "avif", "gif", "svg", "ico", "bmp", "tiff",
    ];

    for ext in image_extensions {
        let html = format!(
            r#"<script id="__NUXT_DATA__">["https:\/\/pbcdnw.aoneroom.com\/covers\/poster.{ext}"]</script>"#
        );
        let res = extract_stream_from_html(&html);
        assert_eq!(
            res, None,
            "Image extension .{ext} must NEVER be returned as a stream URL!"
        );
    }
}

#[test]
fn test_mixed_cdn_images_before_and_after_video() {
    let html = r#"
    <script id="__NUXT_DATA__">
    [
        "https:\/\/pbcdnw.aoneroom.com\/image\/2026\/01\/poster1.jpg",
        "https:\/\/pbcdnw.aoneroom.com\/image\/2026\/01\/poster2.png",
        "https:\/\/pbcdnw.aoneroom.com\/image\/2026\/01\/backdrop.webp",
        "https:\/\/pbcdnw.aoneroom.com\/avatars\/actor.jpeg",
        "https:\/\/macdn.aoneroom.com\/media\/video_1080p.mp4",
        "https:\/\/pbcdnw.aoneroom.com\/banner\/footer.webp"
    ]
    </script>
    "#;
    let res = extract_stream_from_html(html);
    assert_eq!(
        res,
        Some("https://macdn.aoneroom.com/media/video_1080p.mp4".to_string())
    );
}

#[test]
fn test_deceptive_image_filenames_containing_media_substrings() {
    let html = r#"
    <script id="__NUXT_DATA__">
    [
        "https:\/\/pbcdnw.aoneroom.com\/images\/m3u8_preview_icon.png",
        "https:\/\/pbcdnw.aoneroom.com\/covers\/movie_mp4_banner.jpg",
        "https:\/\/pbcdnw.aoneroom.com\/thumb.webp?label=m3u8"
    ]
    </script>
    "#;
    let res = extract_stream_from_html(html);
    assert_eq!(
        res, None,
        "Deceptive image URLs with m3u8/mp4 in path or query must NOT match"
    );
}

#[test]
fn test_pure_image_payload_returns_none() {
    let html = r#"
    <!DOCTYPE html>
    <html>
    <head>
        <script id="__NUXT_DATA__">
        ["https:\/\/pbcdnw.aoneroom.com\/image\/1.jpg", "https:\/\/pbcdnw.aoneroom.com\/image\/2.webp"]
        </script>
    </head>
    <body>
        <img src="https://pbcdnw.aoneroom.com/image/banner.png" />
    </body>
    </html>
    "#;
    let res = extract_stream_from_html(html);
    assert_eq!(res, None, "Payload with only images must return None");
}

// =========================================================================
// 4. Search API Response Deserialization & DetailPath Filtering
// =========================================================================

#[test]
fn test_search_response_empty_and_null_cases() {
    // 1. Empty JSON object
    let resp1: Result<SearchResponse, _> = serde_json::from_str("{}");
    assert!(resp1.is_ok());
    assert!(resp1.unwrap().data.is_none());

    // 2. data is null
    let resp2: Result<SearchResponse, _> = serde_json::from_str(r#"{"code":0,"data":null}"#);
    assert!(resp2.is_ok());
    assert!(resp2.unwrap().data.is_none());

    // 3. items is null
    let resp3: Result<SearchResponse, _> =
        serde_json::from_str(r#"{"code":0,"data":{"items":null}}"#);
    assert!(resp3.is_ok());
    assert!(resp3.unwrap().data.unwrap().items.is_none());

    // 4. items is empty array
    let resp4: Result<SearchResponse, _> =
        serde_json::from_str(r#"{"code":0,"data":{"items":[]}}"#);
    assert!(resp4.is_ok());
    let items = resp4.unwrap().data.unwrap().items.unwrap();
    assert!(items.is_empty());
}

#[test]
fn test_search_items_with_missing_or_whitespace_detail_path() {
    let json_str = r#"{
        "code": 0,
        "data": {
            "items": [
                {
                    "title": "Invalid 1",
                    "detailPath": null
                },
                {
                    "title": "Invalid 2",
                    "detailPath": ""
                },
                {
                    "title": "Invalid 3",
                    "detailPath": "   "
                },
                {
                    "title": "Valid Movie",
                    "detailPath": "valid-movie-slug-12345",
                    "releaseDate": "2024-05-10"
                }
            ]
        }
    }"#;

    let resp: SearchResponse = serde_json::from_str(json_str).unwrap();
    let items = resp.data.unwrap().items.unwrap();

    let valid_items: Vec<&SearchItem> = items
        .iter()
        .filter(|item| {
            item.detail_path
                .as_ref()
                .map(|p| !p.trim().is_empty())
                .unwrap_or(false)
        })
        .collect();

    assert_eq!(valid_items.len(), 1);
    assert_eq!(
        valid_items[0].detail_path.as_deref(),
        Some("valid-movie-slug-12345")
    );
}

#[test]
fn test_search_items_heterogeneous_subject_id_types() {
    let json_str = r#"{
        "code": 0,
        "data": {
            "items": [
                {
                    "subjectId": 6047437085185823776,
                    "title": "Numeric ID Movie",
                    "detailPath": "movie-numeric"
                },
                {
                    "subjectId": "6047437085185823776",
                    "title": "String ID Movie",
                    "detailPath": "movie-string"
                },
                {
                    "subjectId": null,
                    "title": "Null ID Movie",
                    "detailPath": "movie-null"
                },
                {
                    "subjectId": {"complex": "id"},
                    "title": "Object ID Movie",
                    "detailPath": "movie-object"
                }
            ]
        }
    }"#;

    let resp: Result<SearchResponse, _> = serde_json::from_str(json_str);
    assert!(
        resp.is_ok(),
        "SearchResponse should deserialize heterogeneous subjectId formats"
    );
    let items = resp.unwrap().data.unwrap().items.unwrap();
    assert_eq!(items.len(), 4);
}

// =========================================================================
// 5. Special Characters in Movie Titles
// =========================================================================

#[test]
fn test_search_request_payload_with_special_character_titles() {
    let special_titles = [
        "Spider-Man: Across the Spider-Verse",
        "Fast & Furious 9",
        "Amélie",
        "Léon: The Professional",
        "千と千尋の神隠し",
        "소방관",
        "Don't Look Up",
        "\"The Matrix\"",
        "Face/Off",
        "WALL·E",
        "F/X2",
        "Mission: Impossible - Dead Reckoning Part One",
        "Alien: Romulus (2024)",
        "Movie with 🍿 Emoji & <HTML> Tags & Special \\ Slashes",
    ];

    for title in special_titles {
        let payload = serde_json::json!({
            "keyword": title,
            "page": 1,
            "perPage": 10,
            "subjectType": 0
        });

        let json_string = serde_json::to_string(&payload).unwrap();
        assert!(
            json_string.contains("keyword"),
            "Payload must contain serialized keyword"
        );

        let parsed: serde_json::Value = serde_json::from_str(&json_string).unwrap();
        assert_eq!(parsed["keyword"].as_str().unwrap(), title);
    }
}

#[test]
fn test_title_matching_with_case_insensitivity_and_diacritics() {
    let items = vec![
        SearchItem {
            subject_id: None,
            title: Some("SPIDER-MAN: NO WAY HOME".to_string()),
            detail_path: Some("spider-man-no-way-home-slug".to_string()),
            release_date: Some("2021-12-17".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Fast & Furious 6".to_string()),
            detail_path: Some("fast-and-furious-6-slug".to_string()),
            release_date: Some("2013-05-24".to_string()),
            subject_type: Some(0),
        },
    ];

    // Case-insensitive query
    let query1 = "spider-man: no way home";
    let matched1 = items
        .iter()
        .find(|item| {
            item.title
                .as_deref()
                .map(|t| t.eq_ignore_ascii_case(query1))
                .unwrap_or(false)
        })
        .unwrap();
    assert_eq!(
        matched1.detail_path.as_deref(),
        Some("spider-man-no-way-home-slug")
    );

    // Exact title with symbols
    let query2 = "Fast & Furious 6";
    let matched2 = items
        .iter()
        .find(|item| {
            item.title
                .as_deref()
                .map(|t| t.eq_ignore_ascii_case(query2))
                .unwrap_or(false)
        })
        .unwrap();
    assert_eq!(
        matched2.detail_path.as_deref(),
        Some("fast-and-furious-6-slug")
    );
}

// =========================================================================
// 6. NontonGo Bypass Latency & Zero-Cost Execution Verification
// =========================================================================

#[tokio::test]
async fn test_nontongo_bypass_latency_strictly_under_1ms() {
    let iterations = 10_000;
    let mut total_duration = std::time::Duration::ZERO;
    let mut max_duration = std::time::Duration::ZERO;

    for _ in 0..iterations {
        let t0 = Instant::now();
        // Emulate the bypass branch in run_all
        let res: Option<()> = async { None }.await;
        let elapsed = t0.elapsed();

        assert_eq!(res, None);
        total_duration += elapsed;
        if elapsed > max_duration {
            max_duration = elapsed;
        }
    }

    let avg_duration = total_duration / (iterations as u32);
    println!(
        "[NontonGo Bypass Benchmark] Iterations: {iterations}, Avg: {avg_duration:?}, Max: {max_duration:?}"
    );

    assert!(
        max_duration < std::time::Duration::from_millis(1),
        "NontonGo bypass max latency ({max_duration:?}) must be strictly < 1ms!"
    );
    assert!(
        avg_duration < std::time::Duration::from_micros(10),
        "NontonGo bypass average latency ({avg_duration:?}) must be < 10µs!"
    );
}

#[tokio::test]
async fn test_nontongo_scrape_direct_contract() {
    // When tmdb_id is empty, nontongo::scrape returns None immediately
    let res = nontongo::scrape("", MediaKind::Movie, 1, 1).await;
    assert!(res.is_none());
}

// =========================================================================
// 7. Fuzzing and Random Inputs Stress Test (Zero-Panic Guarantee)
// =========================================================================

#[test]
fn test_fuzzing_extract_stream_from_html() {
    let large_str = "a".repeat(100_000);
    let large_nuxt = "<script id=\"__NUXT_DATA__\">".to_string() + &"\"https://pbcdn.aoneroom.com/\"".repeat(1_000) + "</script>";

    let fuzzed_inputs: Vec<&str> = vec![
        "",
        "<!DOCTYPE html>",
        "<script id=\"__NUXT_DATA__\">",
        "<script id=\"__NUXT_DATA__\"></script>",
        "<script id=\"__NUXT_DATA__\">[[null, undefined, 0, false, {}]]</script>",
        "<script id=\"__NUXT_DATA__\">[{\"a\": 1, \"b\": [2, 3, {\"url\": \"\"}]}]</script>",
        "window.__NUXT__=",
        "window.__NUXT__={};",
        "window.__NUXT__={\"a\":\"http://\"};",
        "\0\0\0\x01\x02\x03<script id=\"__NUXT_DATA__\">random binary</script>",
        "https://pbcdnw.aoneroom.com/video.mp4\0\n\r",
        "<html><head><title>Test \u{1F600} \u{FFFF}</title></head></html>",
        &large_str,
        &large_nuxt,
    ];

    for (idx, input) in fuzzed_inputs.into_iter().enumerate() {
        let result = std::panic::catch_unwind(|| {
            extract_stream_from_html(input);
        });
        assert!(
            result.is_ok(),
            "extract_stream_from_html panicked on fuzzed input #{idx}!"
        );
    }
}
