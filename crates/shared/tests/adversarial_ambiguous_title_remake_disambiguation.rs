//! Comprehensive Empirical Test Suite for Ambiguous Title Disambiguation,
//! Era Remakes, Subtitle/Sequel Collisions, Timezone Boundary Dates,
//! and Total Elimination of "Random/Wrong Video" Playback.

use pstream_shared::extractors::bstsrs::parse_search_show_slug;
use pstream_shared::extractors::dramacool::parse_search_slug;
use pstream_shared::extractors::moviebox::{
    match_search_item, parse_release_year, SearchItem,
};
use pstream_shared::utils::matches_year_tolerance;

// =========================================================================
// 1. Movie Remakes Across Eras (Identical Titles from Different Decades)
// =========================================================================

#[test]
fn test_moviebox_remake_matrix_exact_year_disambiguation() {
    // Matrix of classic originals vs modern remakes with identical titles
    let remakes = vec![
        // (Title, Year 1, Slug 1, Year 2, Slug 2)
        ("The Fall Guy", 1981, "the-fall-guy-1981-slug", 2024, "the-fall-guy-2024-slug"),
        ("Dune", 1984, "dune-1984-slug", 2021, "dune-2021-slug"),
        ("The Lion King", 1994, "the-lion-king-1994-slug", 2019, "the-lion-king-2019-slug"),
        ("Total Recall", 1990, "total-recall-1990-slug", 2012, "total-recall-2012-slug"),
        ("RoboCop", 1987, "robocop-1987-slug", 2014, "robocop-2014-slug"),
        ("The Invisible Man", 1933, "the-invisible-man-1933-slug", 2020, "the-invisible-man-2020-slug"),
        ("Little Women", 1994, "little-women-1994-slug", 2019, "little-women-2019-slug"),
        ("Pinocchio", 1940, "pinocchio-1940-slug", 2022, "pinocchio-2022-slug"),
        ("Road House", 1989, "road-house-1989-slug", 2024, "road-house-2024-slug"),
        ("Mean Girls", 2004, "mean-girls-2004-slug", 2024, "mean-girls-2024-slug"),
        ("Nosferatu", 1922, "nosferatu-1922-slug", 2024, "nosferatu-2024-slug"),
        ("The Mummy", 1999, "the-mummy-1999-slug", 2017, "the-mummy-2017-slug"),
        ("Point Break", 1991, "point-break-1991-slug", 2015, "point-break-2015-slug"),
        ("Ghostbusters", 1984, "ghostbusters-1984-slug", 2016, "ghostbusters-2016-slug"),
        ("West Side Story", 1961, "west-side-story-1961-slug", 2021, "west-side-story-2021-slug"),
    ];

    for (title, y1, s1, y2, s2) in remakes {
        // Both items exist in search results (classic first to simulate legacy order)
        let items = vec![
            SearchItem {
                subject_id: None,
                title: Some(title.to_string()),
                detail_path: Some(s1.to_string()),
                release_date: Some(format!("{y1}-05-15")),
                subject_type: Some(0),
            },
            SearchItem {
                subject_id: None,
                title: Some(title.to_string()),
                detail_path: Some(s2.to_string()),
                release_date: Some(format!("{y2}-08-20")),
                subject_type: Some(0),
            },
        ];

        // 1. Requesting the modern remake MUST resolve the modern slug
        let chosen_modern = match_search_item(&items, title, Some(y2))
            .unwrap_or_else(|| panic!("Failed to resolve modern remake for {title} ({y2})"));
        assert_eq!(
            chosen_modern.detail_path.as_deref(),
            Some(s2),
            "Must resolve modern remake {s2} for {title} ({y2})"
        );

        // 2. Requesting the classic original MUST resolve the classic slug
        let chosen_classic = match_search_item(&items, title, Some(y1))
            .unwrap_or_else(|| panic!("Failed to resolve classic original for {title} ({y1})"));
        assert_eq!(
            chosen_classic.detail_path.as_deref(),
            Some(s1),
            "Must resolve classic original {s1} for {title} ({y1})"
        );
    }
}

#[test]
fn test_moviebox_remake_rejection_when_target_year_not_present() {
    // When a user requests a 2024 film, but the search API returns ONLY the 1981 film,
    // the system MUST reject it and return None instead of playing the 1981 movie!
    let items = vec![
        SearchItem {
            subject_id: None,
            title: Some("The Fall Guy".to_string()),
            detail_path: Some("the-fall-guy-1981-slug".to_string()),
            release_date: Some("1981-11-04".to_string()),
            subject_type: Some(0),
        },
    ];

    let result = match_search_item(&items, "The Fall Guy", Some(2024));
    assert!(
        result.is_none(),
        "CRITICAL: Must return None and reject 1981 movie when 2024 is requested (eliminating random video bug)"
    );
}

#[test]
fn test_moviebox_triplet_remakes_halloween_and_mummy() {
    // Halloween: 1978 original vs 2007 remake vs 2018 sequel/reboot
    let halloween_items = vec![
        SearchItem {
            subject_id: None,
            title: Some("Halloween".to_string()),
            detail_path: Some("halloween-1978".to_string()),
            release_date: Some("1978-10-25".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Halloween".to_string()),
            detail_path: Some("halloween-2007".to_string()),
            release_date: Some("2007-08-31".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Halloween".to_string()),
            detail_path: Some("halloween-2018".to_string()),
            release_date: Some("2018-10-19".to_string()),
            subject_type: Some(0),
        },
    ];

    assert_eq!(
        match_search_item(&halloween_items, "Halloween", Some(1978)).unwrap().detail_path.as_deref(),
        Some("halloween-1978")
    );
    assert_eq!(
        match_search_item(&halloween_items, "Halloween", Some(2007)).unwrap().detail_path.as_deref(),
        Some("halloween-2007")
    );
    assert_eq!(
        match_search_item(&halloween_items, "Halloween", Some(2018)).unwrap().detail_path.as_deref(),
        Some("halloween-2018")
    );
}

// =========================================================================
// 2. Sequel and Subtitle Disambiguation
// =========================================================================

#[test]
fn test_spiderman_franchise_exhaustive_disambiguation() {
    let spiderman_items = vec![
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man".to_string()),
            detail_path: Some("spider-man-2002".to_string()),
            release_date: Some("2002-05-03".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man 2".to_string()),
            detail_path: Some("spider-man-2-2004".to_string()),
            release_date: Some("2004-06-30".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man 3".to_string()),
            detail_path: Some("spider-man-3-2007".to_string()),
            release_date: Some("2007-05-04".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("The Amazing Spider-Man".to_string()),
            detail_path: Some("the-amazing-spider-man-2012".to_string()),
            release_date: Some("2012-07-03".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man: Homecoming".to_string()),
            detail_path: Some("spider-man-homecoming-2017".to_string()),
            release_date: Some("2017-07-07".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man: Far From Home".to_string()),
            detail_path: Some("spider-man-far-from-home-2019".to_string()),
            release_date: Some("2019-07-02".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man: No Way Home".to_string()),
            detail_path: Some("spider-man-no-way-home-2021".to_string()),
            release_date: Some("2021-12-17".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Spider-Man: Across the Spider-Verse".to_string()),
            detail_path: Some("spider-man-across-the-spider-verse-2023".to_string()),
            release_date: Some("2023-06-02".to_string()),
            subject_type: Some(0),
        },
    ];

    // Verify exact matching on every single installment
    assert_eq!(
        match_search_item(&spiderman_items, "Spider-Man: No Way Home", Some(2021))
            .unwrap()
            .detail_path
            .as_deref(),
        Some("spider-man-no-way-home-2021")
    );
    assert_eq!(
        match_search_item(&spiderman_items, "Spider-Man", Some(2002))
            .unwrap()
            .detail_path
            .as_deref(),
        Some("spider-man-2002")
    );
    assert_eq!(
        match_search_item(&spiderman_items, "Spider-Man 2", Some(2004))
            .unwrap()
            .detail_path
            .as_deref(),
        Some("spider-man-2-2004")
    );
    assert_eq!(
        match_search_item(&spiderman_items, "Spider-Man: Across the Spider-Verse", Some(2023))
            .unwrap()
            .detail_path
            .as_deref(),
        Some("spider-man-across-the-spider-verse-2023")
    );
}

#[test]
fn test_sequel_with_subtitles_and_similar_titles() {
    let items = vec![
        SearchItem {
            subject_id: None,
            title: Some("Dune".to_string()),
            detail_path: Some("dune-2021".to_string()),
            release_date: Some("2021-10-22".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Dune: Part Two".to_string()),
            detail_path: Some("dune-part-two-2024".to_string()),
            release_date: Some("2024-03-01".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Blade Runner".to_string()),
            detail_path: Some("blade-runner-1982".to_string()),
            release_date: Some("1982-06-25".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Blade Runner 2049".to_string()),
            detail_path: Some("blade-runner-2049-2017".to_string()),
            release_date: Some("2017-10-06".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Top Gun".to_string()),
            detail_path: Some("top-gun-1986".to_string()),
            release_date: Some("1986-05-16".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Top Gun: Maverick".to_string()),
            detail_path: Some("top-gun-maverick-2022".to_string()),
            release_date: Some("2022-05-27".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Gladiator".to_string()),
            detail_path: Some("gladiator-2000".to_string()),
            release_date: Some("2000-05-05".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Gladiator II".to_string()),
            detail_path: Some("gladiator-ii-2024".to_string()),
            release_date: Some("2024-11-22".to_string()),
            subject_type: Some(0),
        },
    ];

    assert_eq!(
        match_search_item(&items, "Dune: Part Two", Some(2024)).unwrap().detail_path.as_deref(),
        Some("dune-part-two-2024")
    );
    assert_eq!(
        match_search_item(&items, "Dune", Some(2021)).unwrap().detail_path.as_deref(),
        Some("dune-2021")
    );
    assert_eq!(
        match_search_item(&items, "Blade Runner 2049", Some(2017)).unwrap().detail_path.as_deref(),
        Some("blade-runner-2049-2017")
    );
    assert_eq!(
        match_search_item(&items, "Top Gun: Maverick", Some(2022)).unwrap().detail_path.as_deref(),
        Some("top-gun-maverick-2022")
    );
    assert_eq!(
        match_search_item(&items, "Gladiator II", Some(2024)).unwrap().detail_path.as_deref(),
        Some("gladiator-ii-2024")
    );
}

// =========================================================================
// 3. Timezone & International Release Boundary Stress Matrix (+/- 1 Year)
// =========================================================================

#[test]
fn test_timezone_and_release_date_fuzziness_boundaries() {
    let target_year = 2024;

    // Test cases: (Release Date String, Expected Year Parsed, Should Match Target 2024)
    let date_cases = vec![
        ("2024-05-10", Some(2024), true),   // Exact mid-year
        ("2024-01-01", Some(2024), true),   // Exact day 1
        ("2024-12-31", Some(2024), true),   // Exact last day
        ("2023-12-31", Some(2023), true),   // International premiere 1 day earlier (diff 1)
        ("2023-11-15", Some(2023), true),   // Festival premiere in autumn (diff 1)
        ("2025-01-05", Some(2025), true),   // Theatrical delay to Jan (diff 1)
        ("2025-03-20", Some(2025), true),   // Delayed release (diff 1)
        ("2022-12-31", Some(2022), false),  // 2 years prior -> REJECT
        ("2026-01-01", Some(2026), false),  // 2 years ahead -> REJECT
        ("1994-06-15", Some(1994), false),  // 30 years prior -> REJECT
    ];

    for (date_str, expected_y, should_match) in date_cases {
        let parsed_y = parse_release_year(date_str);
        assert_eq!(
            parsed_y, expected_y,
            "Failed parsing year from {date_str}"
        );

        let matches = matches_year_tolerance(parsed_y, Some(target_year), 1);
        assert_eq!(
            matches, should_match,
            "Date '{date_str}' (parsed {parsed_y:?}) should_match={should_match} for target {target_year}"
        );

        // Verify with match_search_item
        let items = vec![SearchItem {
            subject_id: None,
            title: Some("Target Movie".to_string()),
            detail_path: Some("target-movie-slug".to_string()),
            release_date: Some(date_str.to_string()),
            subject_type: Some(0),
        }];

        let matched = match_search_item(&items, "Target Movie", Some(target_year));
        if should_match {
            assert!(
                matched.is_some(),
                "Must match '{date_str}' within +/- 1 year tolerance"
            );
        } else {
            assert!(
                matched.is_none(),
                "Must reject '{date_str}' outside +/- 1 year tolerance"
            );
        }
    }
}

// =========================================================================
// 4. BSTSrs TV Show Remakes & Year-Aware Slugs Disambiguation
// =========================================================================

#[test]
fn test_bstsrs_tv_remake_slug_disambiguation() {
    let search_html = r#"
    <div class="search-results">
        <div class="result-item">
            <a href="/show/avatar-the-last-airbender">Avatar: The Last Airbender (2005)</a>
        </div>
        <div class="result-item">
            <a href="/show/avatar-the-last-airbender-2024">Avatar: The Last Airbender (2024)</a>
        </div>
        <div class="result-item">
            <a href="/show/shogun-1980">Shōgun (1980)</a>
        </div>
        <div class="result-item">
            <a href="/show/shogun-2024">Shōgun (2024)</a>
        </div>
        <div class="result-item">
            <a href="/show/doctor-who">Doctor Who (2005)</a>
        </div>
        <div class="result-item">
            <a href="/show/doctor-who-2023">Doctor Who (2023)</a>
        </div>
        <div class="result-item">
            <a href="/show/the-twilight-zone-1959">The Twilight Zone (1959)</a>
        </div>
        <div class="result-item">
            <a href="/show/the-twilight-zone-2019">The Twilight Zone (2019)</a>
        </div>
        <div class="result-item">
            <a href="/show/percy-jackson-and-the-olympians-2023">Percy Jackson and the Olympians (2023)</a>
        </div>
    </div>
    "#;

    // 1. Avatar 2024 live action vs 2005 animated
    assert_eq!(
        parse_search_show_slug(search_html, "Avatar: The Last Airbender", Some(2024)),
        Some("avatar-the-last-airbender-2024".to_string())
    );
    assert_eq!(
        parse_search_show_slug(search_html, "Avatar: The Last Airbender", Some(2005)),
        Some("avatar-the-last-airbender".to_string())
    );

    // 2. Shōgun 2024 vs 1980
    assert_eq!(
        parse_search_show_slug(search_html, "Shōgun", Some(2024)),
        Some("shogun-2024".to_string())
    );
    assert_eq!(
        parse_search_show_slug(search_html, "Shogun", Some(1980)),
        Some("shogun-1980".to_string())
    );

    // 3. Doctor Who 2023 vs 2005
    assert_eq!(
        parse_search_show_slug(search_html, "Doctor Who", Some(2023)),
        Some("doctor-who-2023".to_string())
    );
    assert_eq!(
        parse_search_show_slug(search_html, "Doctor Who", Some(2005)),
        Some("doctor-who".to_string())
    );

    // 4. The Twilight Zone 2019 vs 1959
    assert_eq!(
        parse_search_show_slug(search_html, "The Twilight Zone", Some(2019)),
        Some("the-twilight-zone-2019".to_string())
    );
    assert_eq!(
        parse_search_show_slug(search_html, "The Twilight Zone", Some(1959)),
        Some("the-twilight-zone-1959".to_string())
    );
}

// =========================================================================
// 5. DramaCool Asian Drama Multi-Season & Sequel Disambiguation
// =========================================================================

#[test]
fn test_dramacool_multi_season_and_sequel_disambiguation() {
    let dramacool_html = r#"
    <ul class="list-episode-item">
        <li>
            <a href="https://ww1.dramacool.cx/drama-detail/squid-game" title="Squid Game (2021)">
                <h3>Squid Game (2021)</h3>
            </a>
        </li>
        <li>
            <a href="https://ww1.dramacool.cx/drama-detail/squid-game-season-2" title="Squid Game Season 2 (2024)">
                <h3>Squid Game Season 2 (2024)</h3>
            </a>
        </li>
        <li>
            <a href="https://ww1.dramacool.cx/drama-detail/squid-game-season-3" title="Squid Game Season 3 (2025)">
                <h3>Squid Game Season 3 (2025)</h3>
            </a>
        </li>
        <li>
            <a href="https://ww1.dramacool.cx/drama-detail/the-glory" title="The Glory (2022)">
                <h3>The Glory (2022)</h3>
            </a>
        </li>
        <li>
            <a href="https://ww1.dramacool.cx/drama-detail/the-glory-part-2" title="The Glory: Part 2 (2023)">
                <h3>The Glory: Part 2 (2023)</h3>
            </a>
        </li>
        <li>
            <a href="https://ww1.dramacool.cx/drama-detail/sweet-home" title="Sweet Home (2020)">
                <h3>Sweet Home (2020)</h3>
            </a>
        </li>
        <li>
            <a href="https://ww1.dramacool.cx/drama-detail/sweet-home-season-2" title="Sweet Home Season 2 (2023)">
                <h3>Sweet Home Season 2 (2023)</h3>
            </a>
        </li>
        <li>
            <a href="https://ww1.dramacool.cx/drama-detail/sweet-home-season-3" title="Sweet Home Season 3 (2024)">
                <h3>Sweet Home Season 3 (2024)</h3>
            </a>
        </li>
    </ul>
    "#;

    // Squid Game seasons
    assert_eq!(
        parse_search_slug(dramacool_html, "Squid Game", Some(2021)),
        Some("squid-game".to_string())
    );
    assert_eq!(
        parse_search_slug(dramacool_html, "Squid Game Season 2", Some(2024)),
        Some("squid-game-season-2".to_string())
    );
    assert_eq!(
        parse_search_slug(dramacool_html, "Squid Game Season 3", Some(2025)),
        Some("squid-game-season-3".to_string())
    );

    // The Glory parts
    assert_eq!(
        parse_search_slug(dramacool_html, "The Glory", Some(2022)),
        Some("the-glory".to_string())
    );
    assert_eq!(
        parse_search_slug(dramacool_html, "The Glory: Part 2", Some(2023)),
        Some("the-glory-part-2".to_string())
    );

    // Sweet Home seasons
    assert_eq!(
        parse_search_slug(dramacool_html, "Sweet Home", Some(2020)),
        Some("sweet-home".to_string())
    );
    assert_eq!(
        parse_search_slug(dramacool_html, "Sweet Home Season 2", Some(2023)),
        Some("sweet-home-season-2".to_string())
    );
    assert_eq!(
        parse_search_slug(dramacool_html, "Sweet Home Season 3", Some(2024)),
        Some("sweet-home-season-3".to_string())
    );
}

// =========================================================================
// 6. Zero False Positives / Random Video Rejection Oracle
// =========================================================================

#[test]
fn test_zero_false_positives_on_completely_unrelated_search_results() {
    // Scenario: User searches for "Deadpool & Wolverine" (2024).
    // API returns only older Wolverine standalone movies.
    let unrelated_items = vec![
        SearchItem {
            subject_id: None,
            title: Some("X-Men Origins: Wolverine".to_string()),
            detail_path: Some("x-men-origins-wolverine-2009".to_string()),
            release_date: Some("2009-05-01".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("The Wolverine".to_string()),
            detail_path: Some("the-wolverine-2013".to_string()),
            release_date: Some("2013-07-26".to_string()),
            subject_type: Some(0),
        },
    ];

    let result = match_search_item(&unrelated_items, "Deadpool & Wolverine", Some(2024));
    assert!(
        result.is_none(),
        "Must NOT return older unrelated movies when target year is 2024"
    );
}

#[test]
fn test_zero_false_positives_on_missing_detail_paths_or_corrupted_dates() {
    let malformed_items = vec![
        SearchItem {
            subject_id: None,
            title: Some("Inception".to_string()),
            detail_path: None, // Missing detail path
            release_date: Some("2010-07-16".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Inception".to_string()),
            detail_path: Some("".to_string()), // Empty detail path
            release_date: Some("2010-07-16".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Inception".to_string()),
            detail_path: Some("   ".to_string()), // Whitespace detail path
            release_date: Some("2010-07-16".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Inception".to_string()),
            detail_path: Some("inception-valid-slug".to_string()),
            release_date: Some("invalid-date-string".to_string()), // Corrupted date
            subject_type: Some(0),
        },
    ];

    // Since none of the valid detail paths has a valid 2010 date, must return None!
    let result = match_search_item(&malformed_items, "Inception", Some(2010));
    assert!(
        result.is_none(),
        "Must return None when no valid item has both detail path and matching release year"
    );
}

// =========================================================================
// 7. Property-Based Robustness / Fuzzing Disambiguation Harness
// =========================================================================

#[test]
fn test_fuzzy_title_matching_with_punctuation_and_casing() {
    let items = vec![
        SearchItem {
            subject_id: None,
            title: Some("Mission: Impossible - Dead Reckoning Part One".to_string()),
            detail_path: Some("mission-impossible-dead-reckoning-2023".to_string()),
            release_date: Some("2023-07-12".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Mission: Impossible - Fallout".to_string()),
            detail_path: Some("mission-impossible-fallout-2018".to_string()),
            release_date: Some("2018-07-27".to_string()),
            subject_type: Some(0),
        },
    ];

    // Variations of query string
    let queries = vec![
        "Mission Impossible Dead Reckoning Part One",
        "mission: impossible - dead reckoning part one",
        "  Mission: Impossible - Dead Reckoning Part One  ",
        "MISSION: IMPOSSIBLE - DEAD RECKONING PART ONE",
    ];

    for q in queries {
        let matched = match_search_item(&items, q, Some(2023));
        assert!(
            matched.is_some(),
            "Query '{q}' should match normalized title for 2023"
        );
        assert_eq!(
            matched.unwrap().detail_path.as_deref(),
            Some("mission-impossible-dead-reckoning-2023")
        );
    }
}

#[test]
fn test_numerical_titles_and_years_in_names_disambiguation() {
    let numerical_items = vec![
        SearchItem {
            subject_id: None,
            title: Some("1984".to_string()),
            detail_path: Some("1984-film-1956".to_string()),
            release_date: Some("1956-03-01".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("1984".to_string()),
            detail_path: Some("1984-film-1984".to_string()),
            release_date: Some("1984-10-10".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("1917".to_string()),
            detail_path: Some("1917-film-2019".to_string()),
            release_date: Some("2019-12-25".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("2012".to_string()),
            detail_path: Some("2012-film-2009".to_string()),
            release_date: Some("2009-11-13".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("2001: A Space Odyssey".to_string()),
            detail_path: Some("2001-a-space-odyssey-1968".to_string()),
            release_date: Some("1968-04-03".to_string()),
            subject_type: Some(0),
        },
    ];

    // "1984" film released in 1984
    assert_eq!(
        match_search_item(&numerical_items, "1984", Some(1984)).unwrap().detail_path.as_deref(),
        Some("1984-film-1984")
    );

    // "1984" film released in 1956
    assert_eq!(
        match_search_item(&numerical_items, "1984", Some(1956)).unwrap().detail_path.as_deref(),
        Some("1984-film-1956")
    );

    // "1917" film released in 2019
    assert_eq!(
        match_search_item(&numerical_items, "1917", Some(2019)).unwrap().detail_path.as_deref(),
        Some("1917-film-2019")
    );

    // "2012" film released in 2009
    assert_eq!(
        match_search_item(&numerical_items, "2012", Some(2009)).unwrap().detail_path.as_deref(),
        Some("2012-film-2009")
    );

    // "2001: A Space Odyssey" released in 1968
    assert_eq!(
        match_search_item(&numerical_items, "2001: A Space Odyssey", Some(1968)).unwrap().detail_path.as_deref(),
        Some("2001-a-space-odyssey-1968")
    );
}

#[test]
fn test_cross_franchise_versus_and_teamup_disambiguation() {
    let crossover_items = vec![
        SearchItem {
            subject_id: None,
            title: Some("Godzilla vs. Kong".to_string()),
            detail_path: Some("godzilla-vs-kong-2021".to_string()),
            release_date: Some("2021-03-24".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Godzilla x Kong: The New Empire".to_string()),
            detail_path: Some("godzilla-x-kong-the-new-empire-2024".to_string()),
            release_date: Some("2024-03-27".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Alien vs. Predator".to_string()),
            detail_path: Some("alien-vs-predator-2004".to_string()),
            release_date: Some("2004-08-12".to_string()),
            subject_type: Some(0),
        },
        SearchItem {
            subject_id: None,
            title: Some("Aliens vs. Predator: Requiem".to_string()),
            detail_path: Some("aliens-vs-predator-requiem-2007".to_string()),
            release_date: Some("2007-12-25".to_string()),
            subject_type: Some(0),
        },
    ];

    assert_eq!(
        match_search_item(&crossover_items, "Godzilla x Kong: The New Empire", Some(2024))
            .unwrap()
            .detail_path
            .as_deref(),
        Some("godzilla-x-kong-the-new-empire-2024")
    );
    assert_eq!(
        match_search_item(&crossover_items, "Godzilla vs. Kong", Some(2021))
            .unwrap()
            .detail_path
            .as_deref(),
        Some("godzilla-vs-kong-2021")
    );
    assert_eq!(
        match_search_item(&crossover_items, "Aliens vs. Predator: Requiem", Some(2007))
            .unwrap()
            .detail_path
            .as_deref(),
        Some("aliens-vs-predator-requiem-2007")
    );
}

#[test]
fn test_fuzzing_perturbed_search_items_zero_panics_and_strict_bounds() {
    let long_title = "Super Long Title ".repeat(10);
    let titles = [
        "Avatar", "The Matrix", "Spider-Man", "1984", "Dune", "Squid Game",
        "Transformers: Rise of the Beasts", "Amélie", "Café ☕", "", "   ",
        "Null \0 Byte", "!@#$%^&*()", long_title.as_str(),
    ];
    let dates = [
        "2024-05-10", "2023-12-31", "2025-01-01", "1999-12-31", "1922",
        "invalid", "", "   ", "9999-99-99", "0000-00-00", "May 15, 2024",
    ];

    for (i, t) in titles.iter().enumerate() {
        for (j, d) in dates.iter().enumerate() {
            let target_year = if (i + j) % 2 == 0 { Some(2024) } else { None };
            let items = vec![
                SearchItem {
                    subject_id: Some(serde_json::json!(i)),
                    title: Some(t.to_string()),
                    detail_path: Some(format!("slug-{i}-{j}")),
                    release_date: Some(d.to_string()),
                    subject_type: Some(0),
                },
            ];

            let result = match_search_item(&items, t, target_year);
            if let Some(target_y) = target_year {
                if let Some(chosen) = result {
                    let chosen_y = chosen.release_date.as_deref().and_then(parse_release_year);
                    assert!(
                        chosen_y.is_some(),
                        "Matched item must have parseable release year when target_year is provided"
                    );
                    let diff = chosen_y.unwrap().abs_diff(target_y);
                    assert!(
                        diff <= 1,
                        "Matched item year {chosen_y:?} must be within +/- 1 year of target {target_y}"
                    );
                }
            }
        }
    }
}
