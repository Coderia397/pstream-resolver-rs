use pstream_shared::models::MediaKind;
use pstream_shared::extractors::{oneshows, moviebox, bstsrs};

#[tokio::test]
#[ignore]
async fn dump_urls() {
    let id = "27205"; // Inception
    let title = "Inception";
    let year = 2010;
    
    let mut urls = Vec::new();
    
    if let Some(res) = oneshows::scrape(id, MediaKind::Movie, 1, 1, Some(title), Some(year)).await {
        if !res.sources.is_empty() {
            urls.push(("oneshows", res.sources[0].url.clone()));
        }
    }
    if let Some(res) = moviebox::scrape(title, Some(year)).await {
        if !res.sources.is_empty() {
            urls.push(("moviebox", res.sources[0].url.clone()));
        }
    }
    if let Some(res) = bstsrs::scrape(id, MediaKind::Movie, 1, 1, Some(title), Some(year)).await {
        if !res.sources.is_empty() {
            urls.push(("bstsrs", res.sources[0].url.clone()));
        }
    }
    
    for (prov, url) in urls {
        println!("PROVIDER_URL|{}|{}", prov, url);
    }
}
