//! Shared string, year parsing, quality ranking, and stream sorting utilities.

use crate::models::Source;
use once_cell::sync::Lazy;
use regex::Regex;

/// Regex matching parenthesized or bracketed 4-digit years:
/// e.g. "(2024)", "[2022]", "(2008-2013)", "(2024/2025)"
static PAREN_YEAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?:\(|\[)(19\d{2}|20\d{2})(?:[-–/]\d{2,4})?(?:\)|\])"#).expect("paren year regex")
});

/// Regex matching ISO / date format with 4-digit year:
/// e.g. "2024-05-10", "2010/07/16"
static DATE_YEAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\b(19\d{2}|20\d{2})[-/]\d{1,2}[-/]\d{1,2}\b"#).expect("date year regex")
});

/// Regex matching general delimited 4-digit year:
/// e.g. "slug-2024", "movie_1999", "2024"
static GENERAL_YEAR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?:^|[^0-9a-zA-Z])(19\d{2}|20\d{2})(?:[^0-9a-zA-Z]|$)"#)
        .expect("general year regex")
});

/// Parse a 4-digit year (`1900..=2099`) from a title, date, slug, or header string.
///
/// Priority:
/// 1. Parenthesized or bracketed year: `"Blade Runner 2049 (2017)"` -> `Some(2017)`
/// 2. ISO / date prefix: `"2024-05-10"` -> `Some(2024)`
/// 3. Delimited 4-digit year: `"avatar-2024-slug"` -> `Some(2024)`
///
/// Rejects resolution tags (e.g. `"1080p"`, `"720p"`) and arbitrary numbers.
pub fn parse_year(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Parenthesized / bracketed year takes highest precedence
    if let Some(caps) = PAREN_YEAR_RE.captures(trimmed) {
        if let Some(m) = caps.get(1) {
            if let Ok(y) = m.as_str().parse::<u32>() {
                return Some(y);
            }
        }
    }

    // 2. ISO date prefix
    if let Some(caps) = DATE_YEAR_RE.captures(trimmed) {
        if let Some(m) = caps.get(1) {
            if let Ok(y) = m.as_str().parse::<u32>() {
                return Some(y);
            }
        }
    }

    // 3. General delimited year (takes the last valid occurrence if multiple exist)
    let mut last_year = None;
    for caps in GENERAL_YEAR_RE.captures_iter(trimmed) {
        if let Some(m) = caps.get(1) {
            if let Ok(y) = m.as_str().parse::<u32>() {
                last_year = Some(y);
            }
        }
    }

    last_year
}

/// Check if an item's year matches the target year within a specified tolerance (+/- `tolerance` years).
///
/// - If `target_year` is `None`: returns `true` (unconstrained).
/// - If `target_year` is `Some(target)` and `item_year` is `Some(item)`:
///   returns `true` if `|item - target| <= tolerance`.
/// - If `target_year` is `Some(_)` and `item_year` is `None`: returns `false`.
pub fn matches_year_tolerance(
    item_year: Option<u32>,
    target_year: Option<u32>,
    tolerance: u32,
) -> bool {
    match (item_year, target_year) {
        (_, None) => true,
        (Some(item), Some(target)) => item.abs_diff(target) <= tolerance,
        (None, Some(_)) => false,
    }
}

/// Convert a title string into a standardized slug format.
///
/// Converts to lowercase, drops apostrophes/quotes without inserting hyphens
/// (`"Pastor's"` -> `"pastors"`, `"“The Glory”"` -> `"the-glory"`),
/// replaces whitespace and punctuation with single hyphens, collapses consecutive
/// hyphens, and trims leading/trailing hyphens.
pub fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    let mut prev_hyphen = true; // prevent leading hyphens

    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_hyphen = false;
        } else if c == '\'' || c == '\"' || c == '`' || c == '’' || c == '‘' || c == '“' || c == '”' {
            // Drop quotes/apostrophes without creating hyphens
            continue;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }

    if slug.ends_with('-') {
        slug.pop();
    }

    slug
}

/// Compute an integer rank for a video quality string to enable descending sorting.
///
/// Ranking hierarchy:
/// - 4K / 2160p / UHD: `2160`
/// - 1080p / FHD / 1080: `1080`
/// - auto / master.m3u8 / adaptive: `800`
/// - 720p / HD / 720: `720`
/// - 480p / SD / 480: `480`
/// - 360p / 360: `360`
/// - 240p / 240: `240`
/// - Explicit numeric tags (e.g. "1440p" -> 1440)
/// - Unknown / empty: `0`
pub fn quality_rank(quality: &str) -> u32 {
    let lower = quality.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return 0;
    }

    if lower.contains("2160") || lower.contains("4k") || lower.contains("uhd") {
        2160
    } else if lower.contains("1080") || lower.contains("fhd") {
        1080
    } else if lower.contains("auto") || lower.contains("master") || lower.contains("adaptive") {
        800
    } else if lower.contains("720") || (lower.contains("hd") && !lower.contains("fhd") && !lower.contains("uhd")) {
        720
    } else if lower.contains("480") || lower.contains("sd") {
        480
    } else if lower.contains("360") {
        360
    } else if lower.contains("240") {
        240
    } else {
        // Attempt to extract digits if custom resolution is given (e.g. "1440p")
        let digits: String = lower.chars().filter(|c| c.is_ascii_digit()).collect();
        digits.parse::<u32>().unwrap_or(0)
    }
}

/// Normalize heterogeneous quality labels to standard resolution strings
/// (`"2160p"`, `"1080p"`, `"720p"`, `"480p"`, `"360p"`, `"240p"`, `"auto"`).
///
/// Prevents duplication bugs like `"1080pp"`.
pub fn normalize_quality(quality: &str) -> String {
    let lower = quality.trim().to_ascii_lowercase();
    if lower.is_empty() || lower == "auto" || lower.contains("master") || lower.contains("adaptive") {
        return "auto".to_string();
    }

    let rank = quality_rank(quality);
    match rank {
        2160 => "2160p".to_string(),
        1080 => "1080p".to_string(),
        720 => "720p".to_string(),
        480 => "480p".to_string(),
        360 => "360p".to_string(),
        240 => "240p".to_string(),
        800 => "auto".to_string(),
        other if other > 0 => format!("{}p", other),
        _ => "auto".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioMetadata {
    pub audio: Option<String>,
    pub audio_languages: Vec<String>,
    pub is_original: bool,
    pub is_multi_audio: bool,
}

/// Detect audio language and original/dub status from URL, title, and provider hints.
pub fn detect_audio_metadata(
    url: &str,
    title_hint: Option<&str>,
    provider_id: Option<&str>,
) -> AudioMetadata {
    let lower_url = url.to_ascii_lowercase();
    let lower_title = title_hint.map(|t| t.to_ascii_lowercase()).unwrap_or_default();
    let combined = format!("{} {}", lower_url, lower_title);

    // Multi-audio / dual-audio check
    let is_multi = combined.contains("multi")
        || combined.contains("dual")
        || combined.contains("2audio")
        || combined.contains("tri-audio");

    if is_multi {
        return AudioMetadata {
            audio: Some("multi".to_string()),
            audio_languages: vec!["en".to_string()],
            is_original: true,
            is_multi_audio: true,
        };
    }

    // Provider-specific explicit defaults
    if let Some(pid) = provider_id {
        match pid {
            "vixsrc" => {
                if lower_url.contains("lang=en") {
                    return AudioMetadata {
                        audio: Some("en".to_string()),
                        audio_languages: vec!["en".to_string()],
                        is_original: true,
                        is_multi_audio: true,
                    };
                }
            }
            "moviebox" => {
                return AudioMetadata {
                    audio: Some("en".to_string()),
                    audio_languages: vec!["en".to_string()],
                    is_original: true,
                    is_multi_audio: false,
                };
            }
            _ => {}
        }
    }

    // Helper token matcher checking word boundaries (dots, hyphens, slashes, underscores, spaces)
    let has_token = |tokens: &[&str]| -> bool {
        for t in tokens {
            for delim_start in ['.', '-', '_', '/', ' ', '?', '&', '=', '[', '('] {
                for delim_end in ['.', '-', '_', '/', ' ', '?', '&', '=', ']', ')'] {
                    let pat = format!("{}{}{}", delim_start, t, delim_end);
                    if combined.contains(&pat) {
                        return true;
                    }
                }
            }
        }
        false
    };

    // Foreign dub checks
    if has_token(&["ru", "rus", "russian", "dub-ru", "ru-dub", "dub.ru", "dvo", "mvo"])
        || combined.contains(".ru/")
        || combined.contains("vsembed.ru")
        || combined.contains("дубляж")
        || combined.contains("озвучка")
    {
        return AudioMetadata {
            audio: Some("ru".to_string()),
            audio_languages: vec!["ru".to_string()],
            is_original: false,
            is_multi_audio: false,
        };
    }

    if has_token(&["ita", "italian", "italiano", "doppiaggio"]) {
        return AudioMetadata {
            audio: Some("it".to_string()),
            audio_languages: vec!["it".to_string()],
            is_original: false,
            is_multi_audio: false,
        };
    }

    if has_token(&["esp", "castellano", "latino", "spanish", "espanol", "doblaje"]) {
        return AudioMetadata {
            audio: Some("es".to_string()),
            audio_languages: vec!["es".to_string()],
            is_original: false,
            is_multi_audio: false,
        };
    }

    if has_token(&["fre", "french", "francais", "vff", "vfq", "truefrench"])
        || (has_token(&["vf"]) && !combined.contains("vostfr"))
    {
        return AudioMetadata {
            audio: Some("fr".to_string()),
            audio_languages: vec!["fr".to_string()],
            is_original: false,
            is_multi_audio: false,
        };
    }

    if has_token(&["ger", "german", "deutsch", "synchron"]) {
        return AudioMetadata {
            audio: Some("de".to_string()),
            audio_languages: vec!["de".to_string()],
            is_original: false,
            is_multi_audio: false,
        };
    }

    if has_token(&["hin", "hindi", "tel", "tam", "tamil", "telugu", "bollywood"]) {
        return AudioMetadata {
            audio: Some("hi".to_string()),
            audio_languages: vec!["hi".to_string()],
            is_original: false,
            is_multi_audio: false,
        };
    }

    if has_token(&["jap", "japanese", "jpn"]) {
        return AudioMetadata {
            audio: Some("ja".to_string()),
            audio_languages: vec!["ja".to_string()],
            is_original: true,
            is_multi_audio: false,
        };
    }

    if has_token(&["kor", "korean"]) {
        return AudioMetadata {
            audio: Some("ko".to_string()),
            audio_languages: vec!["ko".to_string()],
            is_original: true,
            is_multi_audio: false,
        };
    }

    if has_token(&["por", "portuguese", "pt-br", "dublado"]) {
        return AudioMetadata {
            audio: Some("pt".to_string()),
            audio_languages: vec!["pt".to_string()],
            is_original: false,
            is_multi_audio: false,
        };
    }

    if has_token(&["tur", "turkish", "turkce"]) {
        return AudioMetadata {
            audio: Some("tr".to_string()),
            audio_languages: vec!["tr".to_string()],
            is_original: false,
            is_multi_audio: false,
        };
    }

    // English markers
    if has_token(&["eng", "english", "en", "vo", "vostfr", "vost"])
        || combined.contains("lang=en")
        || combined.contains("/en/")
    {
        return AudioMetadata {
            audio: Some("en".to_string()),
            audio_languages: vec!["en".to_string()],
            is_original: true,
            is_multi_audio: false,
        };
    }

    // Neutral / Untagged
    AudioMetadata {
        audio: None,
        audio_languages: Vec::new(),
        is_original: false,
        is_multi_audio: false,
    }
}

/// Compare two `Source` items under the multi-tier hierarchy:
/// 1. Direct streams (`s.is_direct()`) strictly precede iframe embeds (`s.is_embed()`).
/// 2. Audio language affinity score strictly precedes quality rank (Original / English / Multi > foreign dub).
/// 3. Within the same tier and audio affinity, higher quality rank strictly precedes lower quality rank.
pub fn compare_sources_with_lang_adaptive(
    a: &Source,
    b: &Source,
    orig_lang: Option<&str>,
    preferred_lang: Option<&str>,
) -> std::cmp::Ordering {
    let embed_a = a.is_embed();
    let embed_b = b.is_embed();

    if embed_a != embed_b {
        // false (direct) < true (embed) => direct stream sorted first
        return embed_a.cmp(&embed_b);
    }

    let score_a = a.audio_score(orig_lang, preferred_lang);
    let score_b = b.audio_score(orig_lang, preferred_lang);
    if score_a != score_b {
        // higher score first
        return score_b.cmp(&score_a);
    }

    let rank_a = quality_rank(&a.quality);
    let rank_b = quality_rank(&b.quality);
    rank_b.cmp(&rank_a)
}

/// Compare two `Source` items with default English / Original audio preference.
pub fn compare_sources_adaptive(a: &Source, b: &Source) -> std::cmp::Ordering {
    compare_sources_with_lang_adaptive(a, b, None, Some("en"))
}

/// Sort a slice of `Source` structs in-place:
/// Direct streams before embeds, higher audio affinity before foreign dubs, then descending by quality rank.
pub fn sort_sources_by_quality(sources: &mut [Source]) {
    sources.sort_by(compare_sources_adaptive);
}

/// Sort a slice of `Source` structs in-place with explicit original and preferred language awareness.
pub fn sort_sources_with_lang(
    sources: &mut [Source],
    orig_lang: Option<&str>,
    preferred_lang: Option<&str>,
) {
    sources.sort_by(|a, b| compare_sources_with_lang_adaptive(a, b, orig_lang, preferred_lang));
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 1. parse_year tests ──────────────────────────────────────────────────

    #[test]
    fn parse_year_parenthesized_and_bracketed() {
        assert_eq!(parse_year("(2024)"), Some(2024));
        assert_eq!(parse_year("Squid Game Season 2 (2024)"), Some(2024));
        assert_eq!(parse_year("Breaking Bad (2008-2013)"), Some(2008));
        assert_eq!(parse_year("[2022] Series Name"), Some(2022));
        assert_eq!(parse_year("Show (2024/2025)"), Some(2024));
    }

    #[test]
    fn parse_year_iso_and_date_strings() {
        assert_eq!(parse_year("2024-05-10"), Some(2024));
        assert_eq!(parse_year("2010-07-16"), Some(2010));
        assert_eq!(parse_year("1999/12/31"), Some(1999));
    }

    #[test]
    fn parse_year_slug_and_token_strings() {
        assert_eq!(parse_year("avatar-2024-rerelease"), Some(2024));
        assert_eq!(parse_year("avatar-2009-slug"), Some(2009));
        assert_eq!(parse_year("slug-2024"), Some(2024));
        assert_eq!(parse_year("2024"), Some(2024));
        assert_eq!(parse_year("movie_1994_hd"), Some(1994));
    }

    #[test]
    fn parse_year_handles_numbers_in_titles() {
        // Parenthesized year takes precedence over title digits
        assert_eq!(parse_year("Blade Runner 2049 (2017)"), Some(2017));
        assert_eq!(parse_year("1984 (1956)"), Some(1956));
        assert_eq!(parse_year("2001: A Space Odyssey (1968)"), Some(1968));
        assert_eq!(parse_year("2012 (2009)"), Some(2009));
    }

    #[test]
    fn parse_year_rejects_resolutions_and_invalid_tokens() {
        assert_eq!(parse_year("1080p"), None);
        assert_eq!(parse_year("720p"), None);
        assert_eq!(parse_year("480p"), None);
        assert_eq!(parse_year("FHD 1080"), None);
        assert_eq!(parse_year("video-sd.mp4"), None);
        assert_eq!(parse_year(""), None);
        assert_eq!(parse_year("   "), None);
        assert_eq!(parse_year("Inception"), None);
        assert_eq!(parse_year("123456"), None);
        assert_eq!(parse_year("3000"), None);
    }

    // ── 2. matches_year_tolerance tests ─────────────────────────────────────

    #[test]
    fn matches_year_tolerance_exact_and_within_bounds() {
        assert!(matches_year_tolerance(Some(2024), Some(2024), 1));
        assert!(matches_year_tolerance(Some(2023), Some(2024), 1));
        assert!(matches_year_tolerance(Some(2025), Some(2024), 1));
        assert!(matches_year_tolerance(Some(2024), Some(2024), 0));
    }

    #[test]
    fn matches_year_tolerance_rejects_out_of_bounds() {
        assert!(!matches_year_tolerance(Some(2022), Some(2024), 1));
        assert!(!matches_year_tolerance(Some(2026), Some(2024), 1));
        assert!(!matches_year_tolerance(Some(1999), Some(2024), 1));
        assert!(!matches_year_tolerance(Some(2023), Some(2024), 0));
    }

    #[test]
    fn matches_year_tolerance_option_handling() {
        // Target None => always matches
        assert!(matches_year_tolerance(Some(1999), None, 1));
        assert!(matches_year_tolerance(None, None, 1));

        // Target Some and Item None => does not match strict filter
        assert!(!matches_year_tolerance(None, Some(2024), 1));
    }

    // ── 3. slugify tests ─────────────────────────────────────────────────────

    #[test]
    fn slugify_standard_and_special_characters() {
        assert_eq!(slugify("Breaking Bad"), "breaking-bad");
        assert_eq!(slugify("It's Okay to Not Be Okay"), "its-okay-to-not-be-okay");
        assert_eq!(slugify("Don't Look Up"), "dont-look-up");
        assert_eq!(slugify("\"The Glory\""), "the-glory");
        assert_eq!(slugify("`Taxi Driver`"), "taxi-driver");
        assert_eq!(slugify("What's Wrong with Secretary Kim"), "whats-wrong-with-secretary-kim");
        assert_eq!(slugify("The Glory: Part 2"), "the-glory-part-2");
        assert_eq!(slugify("Fast & Furious"), "fast-furious");
        assert_eq!(slugify("Spider-Man: Across the Spider-Verse"), "spider-man-across-the-spider-verse");
        assert_eq!(slugify("Squid --- Game !!! (2021)???"), "squid-game-2021");
        assert_eq!(slugify("Love (ft. Marriage and Divorce)"), "love-ft-marriage-and-divorce");
        assert_eq!(slugify("100 Days My Prince [Special]"), "100-days-my-prince-special");
        assert_eq!(slugify("9-1-1: Lone Star"), "9-1-1-lone-star");
        assert_eq!(slugify("D.P."), "d-p");
    }

    #[test]
    fn slugify_empty_and_unicode() {
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("   "), "");
        assert_eq!(slugify("\t\n\r"), "");
        assert_eq!(slugify("Café Minamdang"), "caf-minamdang");
        assert_eq!(slugify("Amélie"), "am-lie");
        assert_eq!(slugify("오징어 게임"), "");
    }

    // ── 4. quality_rank tests ────────────────────────────────────────────────

    #[test]
    fn quality_rank_standard_resolutions() {
        assert_eq!(quality_rank("2160p"), 2160);
        assert_eq!(quality_rank("4K"), 2160);
        assert_eq!(quality_rank("4K UHD"), 2160);
        assert_eq!(quality_rank("1080p"), 1080);
        assert_eq!(quality_rank("1080"), 1080);
        assert_eq!(quality_rank("1080pp"), 1080);
        assert_eq!(quality_rank("FHD 1080"), 1080);
        assert_eq!(quality_rank("FHD"), 1080);
        assert_eq!(quality_rank("720p"), 720);
        assert_eq!(quality_rank("720"), 720);
        assert_eq!(quality_rank("HD"), 720);
        assert_eq!(quality_rank("480p"), 480);
        assert_eq!(quality_rank("SD"), 480);
        assert_eq!(quality_rank("360p"), 360);
        assert_eq!(quality_rank("240p"), 240);
        assert_eq!(quality_rank("auto"), 800);
        assert_eq!(quality_rank("master.m3u8"), 800);
        assert_eq!(quality_rank("1440p"), 1440);
        assert_eq!(quality_rank("unknown"), 0);
        assert_eq!(quality_rank(""), 0);
    }

    // ── 5. normalize_quality tests ───────────────────────────────────────────

    #[test]
    fn normalize_quality_variations() {
        assert_eq!(normalize_quality("1080"), "1080p");
        assert_eq!(normalize_quality("1080p"), "1080p");
        assert_eq!(normalize_quality("1080pp"), "1080p");
        assert_eq!(normalize_quality("FHD 1080"), "1080p");
        assert_eq!(normalize_quality("FHD"), "1080p");
        assert_eq!(normalize_quality("720"), "720p");
        assert_eq!(normalize_quality("HD"), "720p");
        assert_eq!(normalize_quality("SD"), "480p");
        assert_eq!(normalize_quality("480"), "480p");
        assert_eq!(normalize_quality("360p"), "360p");
        assert_eq!(normalize_quality("4K"), "2160p");
        assert_eq!(normalize_quality("auto"), "auto");
        assert_eq!(normalize_quality("master.m3u8"), "auto");
        assert_eq!(normalize_quality(""), "auto");
    }

    // ── 6. sort_sources_by_quality tests ─────────────────────────────────────

    #[test]
    fn sort_sources_by_quality_orders_descending_and_preserves_order() {
        let mut sources = vec![
            Source::direct_m3u8("http://a/sd.mp4", "480p").tagged("ProvA", "a"),
            Source::direct_m3u8("http://b/1080p_1.m3u8", "1080p").tagged("ProvB", "b"),
            Source::direct_m3u8("http://c/720p.m3u8", "720p").tagged("ProvC", "c"),
            Source::direct_m3u8("http://d/1080p_2.m3u8", "1080p").tagged("ProvD", "d"),
            Source::direct_m3u8("http://e/auto.m3u8", "auto").tagged("ProvE", "e"),
            Source::direct_m3u8("http://f/4k.m3u8", "2160p").tagged("ProvF", "f"),
        ];

        sort_sources_by_quality(&mut sources);

        assert_eq!(sources[0].quality, "2160p");
        assert_eq!(sources[0].url, "http://f/4k.m3u8");

        assert_eq!(sources[1].quality, "1080p");
        assert_eq!(sources[1].url, "http://b/1080p_1.m3u8"); // tie preserved

        assert_eq!(sources[2].quality, "1080p");
        assert_eq!(sources[2].url, "http://d/1080p_2.m3u8"); // tie preserved

        assert_eq!(sources[3].quality, "auto");
        assert_eq!(sources[3].url, "http://e/auto.m3u8");

        assert_eq!(sources[4].quality, "720p");
        assert_eq!(sources[4].url, "http://c/720p.m3u8");

        assert_eq!(sources[5].quality, "480p");
        assert_eq!(sources[5].url, "http://a/sd.mp4");
    }

    #[test]
    fn compare_sources_adaptive_direct_vs_embed_and_qualities() {
        let direct_720 = Source::direct_m3u8("http://example.com/720.m3u8", "720p");
        let embed_1080 = Source::embed("http://example.com/embed1080", "1080p");
        let direct_1080 = Source::direct_m3u8("http://example.com/1080.m3u8", "1080p");
        let embed_720 = Source::embed("http://example.com/embed720", "720p");

        // Direct 720p beats Embed 1080p
        assert_eq!(compare_sources_adaptive(&direct_720, &embed_1080), std::cmp::Ordering::Less);
        assert_eq!(compare_sources_adaptive(&embed_1080, &direct_720), std::cmp::Ordering::Greater);

        // Within direct tier: 1080p beats 720p
        assert_eq!(compare_sources_adaptive(&direct_1080, &direct_720), std::cmp::Ordering::Less);
        assert_eq!(compare_sources_adaptive(&direct_720, &direct_1080), std::cmp::Ordering::Greater);

        // Within embed tier: 1080p beats 720p
        assert_eq!(compare_sources_adaptive(&embed_1080, &embed_720), std::cmp::Ordering::Less);
        assert_eq!(compare_sources_adaptive(&embed_720, &embed_1080), std::cmp::Ordering::Greater);

        // Equal tier and quality
        let direct_1080_alt = Source::direct_m3u8("http://example.com/1080_alt.m3u8", "1080p");
        assert_eq!(compare_sources_adaptive(&direct_1080, &direct_1080_alt), std::cmp::Ordering::Equal);
    }

    #[test]
    fn sort_sources_by_quality_two_tier_prioritizes_direct_over_embed() {
        let mut sources = vec![
            Source::embed("http://embed.com/1080", "1080p").tagged("ProvEmbed1080", "e1"),
            Source::direct_m3u8("http://direct.com/480.mp4", "480p").tagged("ProvDirect480", "d1"),
            Source::embed("http://embed.com/4k", "2160p").tagged("ProvEmbed4K", "e2"),
            Source::direct_m3u8("http://direct.com/720.m3u8", "720p").tagged("ProvDirect720", "d2"),
            Source::direct_m3u8("http://direct.com/1080.m3u8", "1080p").tagged("ProvDirect1080", "d3"),
        ];

        sort_sources_by_quality(&mut sources);

        // Tier 1 (Direct streams): 1080p, 720p, 480p
        assert_eq!(sources[0].url, "http://direct.com/1080.m3u8");
        assert_eq!(sources[0].quality, "1080p");
        assert!(sources[0].is_direct());

        assert_eq!(sources[1].url, "http://direct.com/720.m3u8");
        assert_eq!(sources[1].quality, "720p");
        assert!(sources[1].is_direct());

        assert_eq!(sources[2].url, "http://direct.com/480.mp4");
        assert_eq!(sources[2].quality, "480p");
        assert!(sources[2].is_direct());

        // Tier 2 (Embed streams): 2160p (4K), 1080p
        assert_eq!(sources[3].url, "http://embed.com/4k");
        assert_eq!(sources[3].quality, "2160p");
        assert!(sources[3].is_embed());

        assert_eq!(sources[4].url, "http://embed.com/1080");
        assert_eq!(sources[4].quality, "1080p");
        assert!(sources[4].is_embed());
    }

    #[test]
    fn detect_audio_metadata_identifies_lexicon_tokens() {
        let meta_multi = detect_audio_metadata("http://cdn.com/movie.2024.multi.1080p.m3u8", None, None);
        assert_eq!(meta_multi.audio.as_deref(), Some("multi"));
        assert!(meta_multi.is_multi_audio);
        assert!(meta_multi.is_original);

        let meta_ru = detect_audio_metadata("http://cdn.com/movie.dub.ru.m3u8", None, None);
        assert_eq!(meta_ru.audio.as_deref(), Some("ru"));
        assert!(!meta_ru.is_original);

        let meta_vsembed = detect_audio_metadata("https://vsembed.ru/vs_src.php?id=123", None, None);
        assert_eq!(meta_vsembed.audio.as_deref(), Some("ru"));

        let meta_ita = detect_audio_metadata("http://cdn.com/film.italian.1080p.m3u8", None, None);
        assert_eq!(meta_ita.audio.as_deref(), Some("it"));
        assert!(!meta_ita.is_original);

        let meta_es = detect_audio_metadata("http://cdn.com/pelicula.latino.720p.m3u8", None, None);
        assert_eq!(meta_es.audio.as_deref(), Some("es"));
        assert!(!meta_es.is_original);

        let meta_vixsrc_en = detect_audio_metadata("https://vixsrc.to/playlist/123?lang=en", None, Some("vixsrc"));
        assert_eq!(meta_vixsrc_en.audio.as_deref(), Some("en"));
        assert!(meta_vixsrc_en.is_original);

        let meta_moviebox = detect_audio_metadata("https://macdn.aoneroom.com/video.mp4", None, Some("moviebox"));
        assert_eq!(meta_moviebox.audio.as_deref(), Some("en"));
        assert!(meta_moviebox.is_original);

        let meta_ja = detect_audio_metadata("http://cdn.com/anime.japanese.1080p.m3u8", None, None);
        assert_eq!(meta_ja.audio.as_deref(), Some("ja"));
        assert!(meta_ja.is_original);
    }

    #[test]
    fn adaptive_sorting_prioritizes_english_over_foreign_high_res() {
        let direct_en_720 = Source::direct_m3u8("http://cdn.com/en_720.m3u8", "720p")
            .with_audio("en", true);
        let direct_ru_1080 = Source::direct_m3u8("http://cdn.com/ru_1080.m3u8", "1080p")
            .with_audio("ru", false);
        let direct_it_4k = Source::direct_m3u8("http://cdn.com/it_4k.m3u8", "2160p")
            .with_audio("it", false);
        let direct_multi_1080 = Source::direct_m3u8("http://cdn.com/multi_1080.m3u8", "1080p")
            .with_audio("multi", true);

        // English 720p beats Russian 1080p and Italian 4K dubs
        assert_eq!(compare_sources_adaptive(&direct_en_720, &direct_ru_1080), std::cmp::Ordering::Less);
        assert_eq!(compare_sources_adaptive(&direct_en_720, &direct_it_4k), std::cmp::Ordering::Less);

        // Multi-audio 1080p beats English 720p (multi-audio has score 4, en has score 4, but multi has 1080p vs 720p)
        assert_eq!(compare_sources_adaptive(&direct_multi_1080, &direct_en_720), std::cmp::Ordering::Less);

        // Within foreign dubs: higher quality wins (Italian 4K beats Russian 1080p)
        assert_eq!(compare_sources_adaptive(&direct_it_4k, &direct_ru_1080), std::cmp::Ordering::Less);

        // Direct foreign dub still beats Embed English stream (direct tier is preserved)
        let embed_en_1080 = Source::embed("http://embed.com/en", "1080p").with_audio("en", true);
        assert_eq!(compare_sources_adaptive(&direct_ru_1080, &embed_en_1080), std::cmp::Ordering::Less);
    }

    #[test]
    fn adaptive_sorting_prioritizes_original_language_when_specified() {
        let direct_ja_720 = Source::direct_m3u8("http://cdn.com/ja_720.m3u8", "720p")
            .with_audio("ja", true);
        let direct_en_1080 = Source::direct_m3u8("http://cdn.com/en_1080.m3u8", "1080p")
            .with_audio("en", false);

        // When orig_lang is Japanese (anime), Japanese 720p beats English dub 1080p!
        let cmp = compare_sources_with_lang_adaptive(&direct_ja_720, &direct_en_1080, Some("ja"), Some("en"));
        assert_eq!(cmp, std::cmp::Ordering::Less);

        // When orig_lang is not specified, English 1080p beats Japanese (due to quality rank)
        let cmp_default = compare_sources_adaptive(&direct_en_1080, &direct_ja_720);
        assert_eq!(cmp_default, std::cmp::Ordering::Less);
    }
}
