use pstream_shared::extractors::run_all;
use pstream_shared::models::MediaKind;
use std::time::Duration;

#[tokio::test]
#[ignore]
async fn test_full_resolver_sweep() {
    let targets = vec![
        // Very New
        ("533535", MediaKind::Movie, 1, 1, "Deadpool & Wolverine", 2024),
        ("209867", MediaKind::Tv, 1, 1, "Fallout", 2024),
        // Medium Old
        ("157336", MediaKind::Movie, 1, 1, "Interstellar", 2014),
        ("66732", MediaKind::Tv, 1, 1, "Stranger Things", 2016),
        // Old
        ("238", MediaKind::Movie, 1, 1, "The Godfather", 1972),
        ("1398", MediaKind::Tv, 1, 1, "The Sopranos", 1999),
        // Unreleased (Decoy check)
        ("1184918", MediaKind::Movie, 1, 1, "The Odyssey", 2026),
        ("95350", MediaKind::Tv, 1, 1, "Lanterns", 2026),
    ];

    println!("{:<30} | {:<4} | {:<5} | {}", "Title", "Year", "Type", "Result (All Providers)");
    println!("{:-<30}-|-{:-<4}-|-{:-<5}-|-{:-<50}", "", "", "", "");

    for (id, kind, season, episode, title, year) in targets {
        let results = run_all(id, kind, season, episode, Some(title), Some(year)).await;
        
        let mut total_sources = 0;
        let mut direct_sources = 0;
        let mut peak_quality = String::from("None");

        for prov in results {
            total_sources += prov.sources.len();
            for src in prov.sources {
                if src.is_direct() {
                    direct_sources += 1;
                    if peak_quality == "None" || src.quality == "1080p" || src.quality == "auto" {
                        peak_quality = src.quality;
                    }
                }
            }
        }

        let status = if total_sources > 0 {
            format!("SUCCESS ({} total, {} direct, peak: {})", total_sources, direct_sources, peak_quality)
        } else {
            "NO PLAYABLE SOURCES".to_string()
        };

        let kind_str = if kind.is_movie() { "Movie" } else { "TV" };
        println!("{:<30} | {:<4} | {:<5} | {}", title, year, kind_str, status);
        
        // 4 second delay to prevent rate limiting across the 17 providers
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
}
