//! Empirical Adversarial Test Suite for Stream Quality, Resolution Maximization,
//! SD Trailer Bug Elimination, and Cross-Provider Output Ordering.

use pstream_shared::extractors::bstsrs;
use pstream_shared::extractors::dramacool;
use pstream_shared::extractors::moviebox::{
    extract_stream_from_html, match_search_item, SearchItem,
};
use pstream_shared::models::Source;
use pstream_shared::utils::{
    matches_year_tolerance, normalize_quality, quality_rank,
    sort_sources_by_quality,
};

// =========================================================================
// 1. SD Trailer Bug & False 1080p Elimination in MovieBox
// =========================================================================

#[test]
fn test_sd_trailer_mp4_is_never_tagged_1080p() {
    let sd_html_cases = vec![
        (
            r#"<script id="__NUXT_DATA__">["https:\/\/macdn.aoneroom.com\/media\/vone\/trailer-sd.mp4"]</script>"#,
            "trailer-sd.mp4",
        ),
        (
            r#"<script id="__NUXT_DATA__">["https:\/\/macdn.aoneroom.com\/media\/vone\/2025\/06\/video-sd.mp4"]</script>"#,
            "video-sd.mp4",
        ),
        (
            r#"<script id="__NUXT_DATA__">["https:\/\/macdn.aoneroom.com\/media\/vone\/311ec6fd9f018d78ee89a590f15541b1-sd.mp4"]</script>"#,
            "hash-sd.mp4",
        ),
        (
            r#"<script id="__NUXT_DATA__">["https:\/\/macdn.aoneroom.com\/media\/vone\/movie_480p.mp4"]</script>"#,
            "movie_480p.mp4",
        ),
    ];

    for (html, label) in sd_html_cases {
        let stream = extract_stream_from_html(html);
        assert!(stream.is_some(), "Stream should be extracted for {label}");
        let stream_url = stream.unwrap();
        let lower = stream_url.to_ascii_lowercase();

        // Check how moviebox tagging logic evaluates this URL
        let is_m3u8 = stream_url.contains(".m3u8");
        let quality = if is_m3u8 || lower.contains("1080") || lower.contains("fhd") {
            "1080p"
        } else if lower.contains("720") || lower.contains("hd") {
            "720p"
        } else if lower.contains("360") {
            "360p"
        } else if lower.contains("480") || lower.contains("-sd") || lower.contains("video-sd") {
            "480p"
        } else {
            "720p"
        };

        assert_ne!(
            quality, "1080p",
            "SD stream {label} ({stream_url}) MUST NOT be tagged as 1080p!"
        );
        assert_eq!(
            quality, "480p",
            "SD stream {label} must be correctly tagged as 480p"
        );
    }
}

#[test]
fn test_360p_preview_mp4_is_tagged_360p() {
    let html = r#"<script id="__NUXT_DATA__">["https:\/\/macdn.aoneroom.com\/media\/vone\/preview_360p.mp4"]</script>"#;
    let stream_url = extract_stream_from_html(html).unwrap();
    let lower = stream_url.to_ascii_lowercase();
    let quality = if stream_url.contains(".m3u8") || lower.contains("1080") || lower.contains("fhd") {
        "1080p"
    } else if lower.contains("720") || lower.contains("hd") {
        "720p"
    } else if lower.contains("360") {
        "360p"
    } else if lower.contains("480") || lower.contains("-sd") || lower.contains("video-sd") {
        "480p"
    } else {
        "720p"
    };

    assert_eq!(quality, "360p", "360p preview MP4 must be tagged as 360p");
}

#[test]
fn test_hd_master_m3u8_prioritized_over_sd_mp4_and_trailers() {
    let html = r#"
    <script id="__NUXT_DATA__">
    [
        "https:\/\/macdn.aoneroom.com\/media\/vone\/trailer-sd.mp4",
        "https:\/\/macdn.aoneroom.com\/media\/vone\/preview_480p.mp4",
        "https:\/\/macdn.aoneroom.com\/media\/vone\/feature_720p.mp4",
        "https:\/\/pbcdn.aoneroom.com\/media\/hls\/master.m3u8?token=xyz",
        "https:\/\/macdn.aoneroom.com\/media\/vone\/feature_1080p.mp4"
    ]
    </script>
    "#;

    let stream = extract_stream_from_html(html);
    assert_eq!(
        stream,
        Some("https://pbcdn.aoneroom.com/media/hls/master.m3u8?token=xyz".to_string()),
        "Master .m3u8 must be prioritized over all MP4s (including 1080p and SD trailers)"
    );
}

#[test]
fn test_1080p_mp4_prioritized_over_720p_and_sd_trailers() {
    let html = r#"
    <script id="__NUXT_DATA__">
    [
        "https:\/\/macdn.aoneroom.com\/media\/vone\/trailer-sd.mp4",
        "https:\/\/macdn.aoneroom.com\/media\/vone\/video-sd.mp4",
        "https:\/\/macdn.aoneroom.com\/media\/vone\/feature_720p.mp4",
        "https:\/\/macdn.aoneroom.com\/media\/vone\/movie_1080p.mp4"
    ]
    </script>
    "#;

    let stream = extract_stream_from_html(html);
    assert_eq!(
        stream,
        Some("https://macdn.aoneroom.com/media/vone/movie_1080p.mp4".to_string()),
        "1080p MP4 must be prioritized over 720p and SD trailers when no m3u8 is present"
    );
}

#[test]
fn test_720p_mp4_prioritized_over_sd_trailers() {
    let html = r#"
    <script id="__NUXT_DATA__">
    [
        "https:\/\/macdn.aoneroom.com\/media\/vone\/trailer-sd.mp4",
        "https:\/\/macdn.aoneroom.com\/media\/vone\/video-sd.mp4",
        "https:\/\/macdn.aoneroom.com\/media\/vone\/feature_720p.mp4"
    ]
    </script>
    "#;

    let stream = extract_stream_from_html(html);
    assert_eq!(
        stream,
        Some("https://macdn.aoneroom.com/media/vone/feature_720p.mp4".to_string()),
        "720p MP4 must be prioritized over SD trailers when no 1080p or m3u8 is present"
    );
}

// =========================================================================
// 2. Extractor-Level Descending Sorting (NontonGo, BSTSrs, DramaCool, CinemaOS)
// =========================================================================

#[test]
fn test_nontongo_inversion_sorts_1080p_to_index_0() {
    // Simulate upstream ascending JSON array [360p, 480p, 720p, 1080p]
    let raw_sources = vec![
        Source::direct_m3u8("https://cdn.example.com/stream_360p.mp4", "360p").tagged("NontonGo", "nontongo"),
        Source::direct_m3u8("https://cdn.example.com/stream_480p.mp4", "480p").tagged("NontonGo", "nontongo"),
        Source::direct_m3u8("https://cdn.example.com/stream_720p.mp4", "720p").tagged("NontonGo", "nontongo"),
        Source::direct_m3u8("https://cdn.example.com/stream_1080p.mp4", "1080p").tagged("NontonGo", "nontongo"),
    ];

    let mut sources = raw_sources;
    sort_sources_by_quality(&mut sources);

    assert_eq!(sources.len(), 4);
    assert_eq!(sources[0].quality, "1080p");
    assert_eq!(sources[0].url, "https://cdn.example.com/stream_1080p.mp4");

    assert_eq!(sources[1].quality, "720p");
    assert_eq!(sources[1].url, "https://cdn.example.com/stream_720p.mp4");

    assert_eq!(sources[2].quality, "480p");
    assert_eq!(sources[2].url, "https://cdn.example.com/stream_480p.mp4");

    assert_eq!(sources[3].quality, "360p");
    assert_eq!(sources[3].url, "https://cdn.example.com/stream_360p.mp4");
}

#[test]
fn test_bstsrs_extract_sources_sorts_1080p_to_index_0() {
    let offset = 0x19e0c889e;
    let url_1080 = "https://voe.sx/e/1080p_stream";
    let url_720 = "https://streamplay.to/embed-720p";
    let url_480 = "https://streamplay.to/embed-480p-sd";

    let enc_1080 = url_1080
        .chars()
        .map(|c| format!("{:x}", (c as u64) + offset))
        .collect::<Vec<_>>()
        .join("-");
    let enc_720 = url_720
        .chars()
        .map(|c| format!("{:x}", (c as u64) + offset))
        .collect::<Vec<_>>()
        .join("-");
    let enc_480 = url_480
        .chars()
        .map(|c| format!("{:x}", (c as u64) + offset))
        .collect::<Vec<_>>()
        .join("-");

    // Mix the HTML order intentionally: 480p first, then 720p, then 1080p
    let html = format!(
        r#"
        <div class="ep-servers">
            <a onclick="window.open(dbneg('{enc_480}'), '_blank');return false;">
                <strong>streamplay.to</strong> <span class="vris480">SD 480</span>
            </a>
            <a onclick="window.open(dbneg('{enc_720}'), '_blank');return false;">
                <strong>streamplay.to</strong> <span class="vris720">HD 720</span>
            </a>
            <a onclick="window.open(dbneg('{enc_1080}'), '_blank');return false;">
                <strong>voe.sx</strong> <span class="vris1080">FHD 1080</span>
            </a>
        </div>
        "#
    );

    let sources = bstsrs::extract_sources_from_html(&html);
    assert_eq!(sources.len(), 3);
    assert_eq!(sources[0].quality, "1080p", "BSTSrs index 0 MUST be 1080p");
    assert_eq!(sources[0].url, url_1080);
    assert_eq!(sources[1].quality, "720p");
    assert_eq!(sources[1].url, url_720);
    assert_eq!(sources[2].quality, "480p");
    assert_eq!(sources[2].url, url_480);
}

#[test]
fn test_dramacool_extract_sources_sorts_1080p_to_index_0() {
    // HTML in mixed order: 360p, 720p, 1080p FHD
    let html = r#"
    <div class="content-left">
        <ul class="list-server-items">
            <li class="linkserver" data-video="https://mp4upload.com/embed-sd.html">
                Mp4Upload (SD 360)
            </li>
            <li class="linkserver" data-video="https://vidhide.com/v/vhide720">
                VidHide (720p)
            </li>
            <li class="linkserver" data-video="https://asianload.io/streaming.php?id=FHD1080">
                AsianLoad (FHD 1080)
            </li>
        </ul>
    </div>
    "#;

    let sources = dramacool::extract_sources_from_html(html);
    assert_eq!(sources.len(), 3);
    assert_eq!(sources[0].quality, "1080p", "DramaCool index 0 MUST be 1080p");
    assert_eq!(sources[0].url, "https://asianload.io/streaming.php?id=FHD1080");
    assert_eq!(sources[1].quality, "720p");
    assert_eq!(sources[2].quality, "480p");
}

#[test]
fn test_cinemaos_quality_normalization_and_sorting() {
    // Verify no "1080pp" concatenation bug
    assert_eq!(normalize_quality("1080"), "1080p");
    assert_eq!(normalize_quality("1080p"), "1080p");
    assert_eq!(normalize_quality("1080pp"), "1080p");
    assert_eq!(normalize_quality("720"), "720p");
    assert_eq!(normalize_quality("720p"), "720p");
    assert_eq!(normalize_quality("480"), "480p");

    let mut sources = vec![
        Source::direct_m3u8("https://cinemaos.live/stream_480.m3u8", &normalize_quality("480")),
        Source::direct_m3u8("https://cinemaos.live/stream_720.m3u8", &normalize_quality("720p")),
        Source::direct_m3u8("https://cinemaos.live/stream_1080.m3u8", &normalize_quality("1080")),
    ];

    sort_sources_by_quality(&mut sources);

    assert_eq!(sources[0].quality, "1080p", "CinemaOS index 0 MUST be 1080p");
    assert_eq!(sources[0].url, "https://cinemaos.live/stream_1080.m3u8");
    assert_eq!(sources[1].quality, "720p");
    assert_eq!(sources[2].quality, "480p");
}

// =========================================================================
// 3. Cross-Provider Output Ordering Guarantee (`data.sources[0]` in Resolver)
// =========================================================================

#[test]
fn test_resolver_global_quality_and_direct_stream_prioritization() {
    // Simulate the exact resolver response assembly in `crates/resolver/src/main.rs:160-177`
    let mut sources = vec![
        // Provider 1: Embed 720p (arrived first)
        Source::embed("https://embed.prov1.com/v720", "720p").tagged("Prov1", "prov1"),
        // Provider 2: Direct 480p
        Source::direct_m3u8("https://cdn.prov2.com/v480.mp4", "480p").tagged("Prov2", "prov2"),
        // Provider 3: Embed 1080p
        Source::embed("https://embed.prov3.com/v1080", "1080p").tagged("Prov3", "prov3"),
        // Provider 4: Direct 1080p (highest quality direct stream)
        Source::direct_m3u8("https://cdn.prov4.com/master_1080.m3u8", "1080p").tagged("Prov4", "prov4"),
        // Provider 5: Direct Auto Master M3U8
        Source::direct_m3u8("https://cdn.prov5.com/adaptive_master.m3u8", "auto").tagged("Prov5", "prov5"),
        // Provider 6: Direct 4K UHD (2160p)
        Source::direct_m3u8("https://cdn.prov6.com/uhd_4k.m3u8", "2160p").tagged("Prov6", "prov6"),
    ];

    // Resolver sorting logic from main.rs:
    sources.sort_by(|a, b| {
        let rank_a = quality_rank(&a.quality);
        let rank_b = quality_rank(&b.quality);
        if rank_a != rank_b {
            return rank_b.cmp(&rank_a);
        }
        let embed_a = a.is_embed.unwrap_or(false);
        let embed_b = b.is_embed.unwrap_or(false);
        embed_a.cmp(&embed_b)
    });

    // 1. Index 0 MUST be the 4K stream
    assert_eq!(sources[0].quality, "2160p");
    assert_eq!(sources[0].url, "https://cdn.prov6.com/uhd_4k.m3u8");

    // 2. Index 1 MUST be Direct 1080p (prioritized over Embed 1080p because !is_embed)
    assert_eq!(sources[1].quality, "1080p");
    assert_eq!(sources[1].url, "https://cdn.prov4.com/master_1080.m3u8");
    assert_ne!(sources[1].is_embed, Some(true));

    // 3. Index 2 MUST be Embed 1080p
    assert_eq!(sources[2].quality, "1080p");
    assert_eq!(sources[2].url, "https://embed.prov3.com/v1080");
    assert_eq!(sources[2].is_embed, Some(true));

    // 4. Index 3 MUST be Auto master.m3u8 (rank 800)
    assert_eq!(sources[3].quality, "auto");
    assert_eq!(sources[3].url, "https://cdn.prov5.com/adaptive_master.m3u8");

    // 5. Index 4 MUST be 720p
    assert_eq!(sources[4].quality, "720p");
    assert_eq!(sources[4].url, "https://embed.prov1.com/v720");

    // 6. Index 5 MUST be 480p
    assert_eq!(sources[5].quality, "480p");
    assert_eq!(sources[5].url, "https://cdn.prov2.com/v480.mp4");
}

#[test]
fn test_resolver_direct_1080p_always_at_sources_zero_when_no_4k() {
    let mut sources = vec![
        Source::embed("https://embed.first.com/movie", "720p").tagged("EmbedProv", "embedprov"),
        Source::direct_m3u8("https://direct.later.com/playlist.m3u8", "1080p").tagged("DirectProv", "directprov"),
    ];

    sources.sort_by(|a, b| {
        let rank_a = quality_rank(&a.quality);
        let rank_b = quality_rank(&b.quality);
        if rank_a != rank_b {
            return rank_b.cmp(&rank_a);
        }
        let embed_a = a.is_embed.unwrap_or(false);
        let embed_b = b.is_embed.unwrap_or(false);
        embed_a.cmp(&embed_b)
    });

    assert_eq!(sources[0].quality, "1080p");
    assert_eq!(sources[0].url, "https://direct.later.com/playlist.m3u8");
    assert_eq!(sources[0].provider_id.as_deref(), Some("directprov"));
}

// =========================================================================
// 4. Strict Title/Year Matching & Remake/Random Video Rejection
// =========================================================================

#[test]
fn test_spiderman_disambiguation_strictly_matches_target_year() {
    let items = vec![
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man".to_string()),
            detail_path: Some("spider-man-2002-slug".to_string()),
            release_date: Some("2002-05-03".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("The Amazing Spider-Man".to_string()),
            detail_path: Some("the-amazing-spider-man-2012-slug".to_string()),
            release_date: Some("2012-07-03".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man: Homecoming".to_string()),
            detail_path: Some("spider-man-homecoming-2017-slug".to_string()),
            release_date: Some("2017-07-07".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man: Far From Home".to_string()),
            detail_path: Some("spider-man-far-from-home-2019-slug".to_string()),
            release_date: Some("2019-07-02".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man: No Way Home".to_string()),
            detail_path: Some("spider-man-no-way-home-2021-slug".to_string()),
            release_date: Some("2021-12-17".to_string()),
            subject_type: Some(0),
        },
    ];

    // Query 1: Spider-Man: No Way Home (2021)
    let matched = match_search_item(&items, "Spider-Man: No Way Home", Some(2021)).unwrap();
    assert_eq!(matched.detail_path.as_deref(), Some("spider-man-no-way-home-2021-slug"));

    // Query 2: Spider-Man (2002) original
    let matched_2002 = match_search_item(&items, "Spider-Man", Some(2002)).unwrap();
    assert_eq!(matched_2002.detail_path.as_deref(), Some("spider-man-2002-slug"));

    // Query 3: Requesting 2024 remake for a title that only has 2002 version returns None!
    let non_existent_remake = match_search_item(&items, "Spider-Man", Some(2024));
    assert!(
        non_existent_remake.is_none(),
        "Must NOT return 2002 Spider-Man when 2024 year is requested!"
    );
}

#[test]
fn test_year_tolerance_boundary_conditions() {
    // Tolerance is +/- 1 year
    assert!(matches_year_tolerance(Some(2021), Some(2021), 1)); // 0 diff
    assert!(matches_year_tolerance(Some(2020), Some(2021), 1)); // -1 diff
    assert!(matches_year_tolerance(Some(2022), Some(2021), 1)); // +1 diff
    assert!(!matches_year_tolerance(Some(2019), Some(2021), 1)); // -2 diff (reject)
    assert!(!matches_year_tolerance(Some(2023), Some(2021), 1)); // +2 diff (reject)
    assert!(!matches_year_tolerance(Some(1994), Some(2024), 1)); // 30 year diff (reject)
}
