use pstream_shared::extractors::oneshows;
use pstream_shared::models::MediaKind;

#[tokio::test]
#[ignore]
async fn test_historical_catalog_sweep_rigorous() {
    let targets = vec![
        // Unreleased (Decoy Filter Test - Should gracefully yield no sources or error, NOT 3 sources)
        ("969681", MediaKind::Movie, 1, 1, "Spider-Man: Brand New Day", 2026),
        ("1184918", MediaKind::Movie, 1, 1, "The Odyssey", 2026),
        ("95350", MediaKind::Tv, 1, 1, "Lanterns", 2026),

        // Recent Blockbusters
        ("533535", MediaKind::Movie, 1, 1, "Deadpool & Wolverine", 2024),
        ("1022789", MediaKind::Movie, 1, 1, "Inside Out 2", 2024),
        ("693134", MediaKind::Movie, 1, 1, "Dune: Part Two", 2024),

        // Legacy / Classics
        ("27205", MediaKind::Movie, 1, 1, "Inception", 2010),
        ("603", MediaKind::Movie, 1, 1, "The Matrix", 1999),
        ("155", MediaKind::Movie, 1, 1, "The Dark Knight", 2008),

        // TV Shows
        ("1396", MediaKind::Tv, 2, 3, "Breaking Bad", 2008),
        ("1399", MediaKind::Tv, 1, 1, "Game of Thrones", 2011),
        ("100088", MediaKind::Tv, 1, 1, "The Last of Us", 2023),
    ];

    println!("{:<45} | {:<4} | {:<5} | {}", "Title", "Year", "Type", "Result");
    println!("{:-<45}-|-{:-<4}-|-{:-<5}-|-{:-<40}", "", "", "", "");

    for (id, kind, season, episode, title, year) in targets {
        let res = oneshows::scrape(id, kind, season, episode, Some(title), Some(year)).await;
        
        let status = match res {
            Some(r) if !r.sources.is_empty() => {
                let direct_count = r.sources.iter().filter(|s| s.is_direct()).count();
                let top_qual = &r.sources[0].quality;
                format!("SUCCESS ({} sources, peak: {})", direct_count, top_qual)
            },
            Some(_) => "NO PLAYABLE SOURCES".to_string(),
            None => "NOT YET AIRED / BLOCKED / NO STREAM".to_string(),
        };

        let kind_str = if kind.is_movie() { "Movie" } else { "TV" };
        println!("{:<45} | {:<4} | {:<5} | {}", title, year, kind_str, status);
        
        // Rate limit mitigation for vsembed
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
