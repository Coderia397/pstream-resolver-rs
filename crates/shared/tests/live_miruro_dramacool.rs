use pstream_shared::extractors::{dramacool, miruro};
use pstream_shared::models::MediaKind;

#[tokio::test]
async fn test_miruro_live() {
    let m = miruro::scrape("27205", MediaKind::Movie, 1, 1, Some("Inception")).await;
    println!("Miruro result: {:?}", m);
    // assert!(m.is_some());
}

#[tokio::test]
async fn test_dramacool_live() {
    let res = dramacool::scrape("93405", MediaKind::Tv, 1, 1, Some("Squid Game"), Some(2021)).await;
    println!("DramaCool live result: {res:?}");
    assert!(
        res.is_some(),
        "Expected DramaCool to resolve sources for Squid Game"
    );
    let provider_res = res.unwrap();
    assert!(
        !provider_res.sources.is_empty(),
        "Expected non-empty sources from DramaCool"
    );
    println!("DramaCool found {} sources", provider_res.sources.len());
    for s in &provider_res.sources {
        println!(
            "  Source URL: {} (quality: {:?}, is_m3u8: {:?}, is_embed: {:?})",
            s.url, s.quality, s.is_m3u8, s.is_embed
        );
        assert!(!s.url.is_empty());
    }
}
