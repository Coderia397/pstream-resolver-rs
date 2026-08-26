//! pstream local-resolver — Rust port of `local-resolver/server.mjs`.
//!
//! Ships as one static aarch64-musl binary so the phone needs no runtime and
//! no node_modules. Route surface and JSON shapes match the JS server, since
//! the deployed frontend already talks to them.

use axum::{
    extract::Query,
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use pstream_shared::{
    cache, cors, extractors, health, probe, ratelimit, subdl, youtube, MediaKind,
    ProviderResult,
};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const DEFAULT_PORT: u16 = 8790;

/// Resolved URLs stay valid far longer than this (VixSrc playlist tokens live
/// ~60 days), so a 6h TTL is well short of expiry while still letting a title
/// that breaks upstream self-heal within a few hours.
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

fn ok_json(req: &HeaderMap, v: serde_json::Value) -> Response {
    (StatusCode::OK, cors::headers_for(req), Json(v)).into_response()
}

fn err_json(req: &HeaderMap, code: StatusCode, v: serde_json::Value) -> Response {
    (code, cors::headers_for(req), Json(v)).into_response()
}

// ── /  and  /api/ping ────────────────────────────────────────────────────────

async fn ping(headers: HeaderMap) -> Response {
    ok_json(&headers, json!({ "ok": true, "service": "local-resolver" }))
}

// ── /api/stream ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct StreamQuery {
    #[serde(alias = "id", alias = "tmdbId")]
    tmdb_id: Option<String>,
    #[serde(alias = "type")]
    kind: Option<String>,
    season: Option<u32>,
    episode: Option<u32>,
    /// LookMovie and MovieBox have no TMDB lookup and search by name, so they
    /// only participate when the caller supplies one.
    title: Option<String>,
    year: Option<u32>,
}

/// TMDB ids go straight into provider URL paths (`/api/tv/{id}/{season}/…`),
/// so anything but digits could reshape the request path. The length cap also
/// stops a caller flooding the cache with junk keys and evicting real entries.
fn is_numeric_within(s: &str, max_digits: usize) -> bool {
    !s.is_empty() && s.len() <= max_digits && s.bytes().all(|b| b.is_ascii_digit())
}

async fn api_stream(headers: HeaderMap, Query(q): Query<StreamQuery>) -> Response {
    let tmdb_id = q.tmdb_id.unwrap_or_default();
    if !is_numeric_within(&tmdb_id, 12) {
        return err_json(
            &headers,
            StatusCode::BAD_REQUEST,
            json!({ "success": false, "error": "tmdbId must be numeric" }),
        );
    }

    // Same bounds as the JS: 4 digits of season, 5 of episode. Serde already
    // rejected non-numeric input by parsing into u32; this caps magnitude so a
    // caller can't build an absurd path or cache key.
    let season = q.season.unwrap_or(1);
    let episode = q.episode.unwrap_or(1);
    if season > 9_999 || episode > 99_999 {
        return err_json(
            &headers,
            StatusCode::BAD_REQUEST,
            json!({ "success": false, "error": "season/episode must be numeric" }),
        );
    }

    // Only "tv" selects tv; everything else is a movie, matching the JS.
    let type_label = if q.kind.as_deref() == Some("tv") { "tv" } else { "movie" };
    let kind = MediaKind::parse(type_label);

    // Titles only feed a search query — cap the length so one caller can't
    // push huge strings through a provider or into our logs.
    let title: Option<String> = q
        .title
        .map(|t| t.chars().take(200).collect::<String>())
        .filter(|t| !t.is_empty());

    // Serve a hot title from memory — no provider request at all. Cache hits
    // are deliberately NOT rate-limited: they cost us nothing.
    let cache_key = format!("{type_label}:{tmdb_id}:{season}:{episode}");
    if let Some(hit) = cache::get(&cache_key) {
        println!("[stream] cache hit {cache_key}");
        return (
            StatusCode::OK,
            cors::headers_for(&headers),
            [(HeaderName::from_static("x-cache"), HeaderValue::from_static("HIT"))],
            Json(hit),
        )
            .into_response();
    }

    // Past this point we'd hit a provider over our single IP — so meter it.
    let ip = ratelimit::client_ip(&headers);
    if ratelimit::check(&ip) {
        println!("[resolve] rate-limited {ip}");
        return (
            StatusCode::TOO_MANY_REQUESTS,
            cors::headers_for(&headers),
            [(HeaderName::from_static("retry-after"), HeaderValue::from_static("60"))],
            Json(json!({ "success": false, "error": "Too many requests — please slow down." })),
        )
            .into_response();
    }

    // Order is preserved, so `working[0]` is the earliest-listed provider that
    // succeeded — matching the JS, which names that one as the headline
    // provider even though every source is returned.
    let working: Vec<ProviderResult> =
        extractors::run_all(&tmdb_id, kind, season, episode, title.as_deref(), q.year).await;

    let Some(winner) = working.first() else {
        return err_json(
            &headers,
            StatusCode::NOT_FOUND,
            json!({
                "success": false,
                "error": "No stream found. All providers are currently unavailable."
            }),
        );
    };

    let payload = json!({
        "success": true,
        "provider": winner.provider,
        "providerId": winner.provider_id,
        "sources": working.iter().flat_map(|r| r.sources.iter()).collect::<Vec<_>>(),
        "subtitles": working.iter().flat_map(|r| r.subtitles.iter()).collect::<Vec<_>>(),
    });

    cache::put(cache_key, payload.clone(), CACHE_TTL);
    ok_json(&headers, payload)
}

// ── /api/providers/health ────────────────────────────────────────────────────

/// Which providers are on, and how each has actually been doing.
///
/// Exists because a provider going dark is otherwise invisible: `run_all`
/// treats "no sources" and "site is gone" identically, so nine of them can
/// stop working and the only symptom is that results quietly get thinner.
/// The counters are in-memory, so they describe this process since it started.
async fn api_providers_health(headers: HeaderMap) -> Response {
    let stats: std::collections::HashMap<&str, health::Stats> =
        health::snapshot().into_iter().collect();

    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut push = |id: &str, name: &str, enabled: bool, note: &str| {
        let s = stats.get(id).copied().unwrap_or_default();
        let attempts = s.ok + s.fail;
        rows.push(json!({
            "id": id,
            "name": name,
            "enabled": enabled,
            "ok": s.ok,
            "fail": s.fail,
            "successRate": if attempts == 0 { serde_json::Value::Null }
                           else { json!((s.ok as f64 / attempts as f64 * 100.0).round()) },
            "lastMs": s.last_ms,
            "note": note,
        }));
    };

    for p in extractors::PROVIDERS {
        push(p.id, p.name, p.enabled, "");
    }
    // The three with bespoke logic aren't in the table.
    push(extractors::vixsrc::ID, "VixSrc ⚡", true, "");
    push(extractors::moviebox::ID, "MovieBox 📦", true, "");
    push(
        extractors::nontongo::ID,
        "NontonGo 🍿",
        false,
        "Disabled: 504 Gateway Timeout",
    );

    let enabled = rows.iter().filter(|r| r["enabled"] == json!(true)).count();
    ok_json(
        &headers,
        json!({ "enabled": enabled, "total": rows.len(), "providers": rows }),
    )
}

// ── /api/youtube/search ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct YoutubeQuery {
    #[serde(alias = "query")]
    q: Option<String>,
    #[serde(alias = "maxResults")]
    max_results: Option<usize>,
}

async fn api_youtube(headers: HeaderMap, Query(p): Query<YoutubeQuery>) -> Response {
    let query = p.q.unwrap_or_default();
    if query.is_empty() {
        return err_json(
            &headers,
            StatusCode::BAD_REQUEST,
            json!({ "results": [], "error": "q parameter required" }),
        );
    }

    // Cap so one caller can't ask us to walk a whole results page.
    let max = p.max_results.unwrap_or(5).clamp(1, 20);
    let results = youtube::search(&query.chars().take(200).collect::<String>(), max).await;
    ok_json(&headers, json!({ "results": results }))
}

// ── /api/subtitles/subdl ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SubdlQuery {
    #[serde(alias = "id", alias = "tmdbId")]
    tmdb_id: Option<String>,
    #[serde(alias = "type")]
    kind: Option<String>,
    season: Option<u32>,
    episode: Option<u32>,
    #[serde(alias = "language")]
    langs: Option<String>,
}

async fn api_subdl(headers: HeaderMap, Query(p): Query<SubdlQuery>) -> Response {
    let tmdb_id = p.tmdb_id.unwrap_or_default();
    if !is_numeric_within(&tmdb_id, 12) {
        return err_json(
            &headers,
            StatusCode::BAD_REQUEST,
            json!({ "subtitles": [], "error": "tmdbId must be numeric" }),
        );
    }

    let body = subdl::search(subdl::SearchArgs {
        tmdb_id: &tmdb_id,
        is_tv: p.kind.as_deref() == Some("tv"),
        season: p.season.unwrap_or(1).min(9_999),
        episode: p.episode.unwrap_or(1).min(99_999),
        langs: &p.langs.unwrap_or_else(|| "EN".to_string()),
    })
    .await;

    ok_json(&headers, body)
}

// ── /api/deploy ──────────────────────────────────────────────────────────────

/// Pull main and restart. Fails closed: with no DEPLOY_SECRET set we report
/// 404 rather than 403, so an unconfigured instance doesn't advertise that
/// the endpoint exists at all.
async fn api_deploy(headers: HeaderMap) -> Response {
    let secret = std::env::var("DEPLOY_SECRET").unwrap_or_default();
    if secret.is_empty() {
        return err_json(
            &headers,
            StatusCode::NOT_FOUND,
            json!({ "success": false, "error": "not_found" }),
        );
    }

    let provided = headers
        .get("x-deploy-secret")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.trim_start_matches("Bearer ").trim().to_string())
        })
        .unwrap_or_default();

    if provided.is_empty() || provided != secret {
        return err_json(
            &headers,
            StatusCode::FORBIDDEN,
            json!({ "success": false, "error": "Invalid deploy secret" }),
        );
    }

    let repo = std::env::var("DEPLOY_REPO_DIR").unwrap_or_else(|_| ".".to_string());

    match std::process::Command::new("git")
        .args(["pull", "origin", "main"])
        .current_dir(&repo)
        .output()
    {
        Ok(o) if o.status.success() => {
            let pull = String::from_utf8_lossy(&o.stdout).trim().to_string();
            println!("[deploy] git pull: {pull}");
            // Let the response flush before the supervisor restarts us.
            tokio::spawn(async {
                tokio::time::sleep(Duration::from_millis(500)).await;
                println!("[deploy] restarting");
                std::process::exit(0);
            });
            ok_json(
                &headers,
                json!({ "success": true, "pull": pull, "restarting": true }),
            )
        }
        Ok(o) => {
            let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
            eprintln!("[deploy] failed: {e}");
            err_json(
                &headers,
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "success": false, "error": e }),
            )
        }
        Err(e) => {
            eprintln!("[deploy] failed to spawn git: {e}");
            err_json(
                &headers,
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "success": false, "error": e.to_string() }),
            )
        }
    }
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let app = Router::new()
        .route("/", get(ping))
        .route("/api/ping", get(ping))
        .route("/api/stream", get(api_stream))
        .route("/api/youtube/search", get(api_youtube))
        .route("/api/subtitles/subdl", get(api_subdl))
        .route("/api/providers/health", get(api_providers_health))

        .route("/api/media-probe", get(probe::media_probe))
        .route("/api/deploy", post(api_deploy))
        .fallback(|method: Method, headers: HeaderMap, uri: axum::http::Uri| async move {
            if method == Method::OPTIONS {
                return (StatusCode::NO_CONTENT, cors::headers_for(&headers)).into_response();
            }
            println!("[404] {method} {uri}");
            err_json(
                &headers,
                StatusCode::NOT_FOUND,
                json!({ "success": false, "error": "not_found" }),
            )
        });

    let addr = format!("0.0.0.0:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[local-resolver] cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };

    println!("[local-resolver] listening on http://localhost:{port}");
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[local-resolver] server error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_validation_works() {
        assert!(is_numeric_within("12345", 10));
        assert!(!is_numeric_within("", 10));
        assert!(!is_numeric_within("123a", 10));
        assert!(!is_numeric_within("12345678901", 10));
    }

    #[tokio::test]
    async fn providers_health_reports_expected_status() {
        let headers = HeaderMap::new();
        let resp = api_providers_health(headers).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

