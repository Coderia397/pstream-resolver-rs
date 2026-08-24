//! `/proxy/stream` — port of the proxy handler in `local-resolver/server.mjs`.
//!
//! Two jobs:
//!   * m3u8 manifests are rewritten so every segment URL points back here,
//!     which is what lets a visitor play a stream whose CDN is IP-bound to
//!     this device.
//!   * everything else is relayed through as bytes.
//!
//! Behaviour is kept deliberately identical to the JS version, including the
//! quirks — see the note on Range in `stream`.

use axum::{
    body::Body,
    extract::Query,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize)]
pub struct ProxyQuery {
    url: Option<String>,
    /// JSON object of extra request headers, as sent by the extractors.
    headers: Option<String>,
}

fn cors() -> [(header::HeaderName, HeaderValue); 2] {
    [
        (
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        ),
        (
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_static("Content-Length,Content-Range"),
        ),
    ]
}

fn fail(code: StatusCode, msg: String) -> Response {
    (code, cors(), axum::Json(json!({ "error": msg }))).into_response()
}

/// Rewrite an m3u8 so every URI in it comes back through this proxy.
///
/// Relative URIs are resolved against `base_url` first — segment lists are
/// usually relative, and the player has no way to resolve them once the
/// manifest is being served from a different origin than it came from.
pub fn rewrite_manifest(text: &str, base_url: &reqwest::Url, headers_param: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);

    for line in text.split('\n') {
        let trimmed = line.trim_end_matches('\r').trim();

        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix('#') {
            // Tags carry their own nested URIs (keys, media renditions,
            // I-frame playlists); those need proxying too.
            if rest.to_ascii_uppercase().contains("URI=") {
                out.push_str(&rewrite_uri_attr(trimmed, base_url, headers_param));
            } else {
                out.push_str(trimmed);
            }
            out.push('\n');
            continue;
        }

        match base_url.join(trimmed) {
            Ok(abs) => {
                out.push_str(&proxied_url(abs.as_str(), headers_param));
            }
            // Unparseable — pass through rather than emit a broken proxy URL.
            Err(_) => out.push_str(trimmed),
        }
        out.push('\n');
    }

    // The JS joins with \n and adds no trailing newline.
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Rewrite the `URI="..."` attribute inside an m3u8 tag, preserving quoting.
fn rewrite_uri_attr(line: &str, base_url: &reqwest::Url, headers_param: &str) -> String {
    let upper = line.to_ascii_uppercase();
    let Some(idx) = upper.find("URI=") else {
        return line.to_string();
    };

    let after = &line[idx + 4..];
    let (quote, inner) = match after.chars().next() {
        Some(q @ ('"' | '\'')) => {
            let body = &after[1..];
            match body.find(q) {
                Some(end) => (Some(q), &body[..end]),
                None => return line.to_string(),
            }
        }
        // Unquoted attribute runs to the next comma.
        _ => {
            let end = after.find(',').unwrap_or(after.len());
            (None, &after[..end])
        }
    };

    let Ok(abs) = base_url.join(inner) else {
        return line.to_string();
    };

    let replacement = proxied_url(abs.as_str(), headers_param);
    let tail_start = match quote {
        Some(_) => idx + 4 + 1 + inner.len() + 1,
        None => idx + 4 + inner.len(),
    };
    let tail = line.get(tail_start..).unwrap_or("");
    let q = quote.map(|c| c.to_string()).unwrap_or_default();

    format!("{}URI={q}{replacement}{q}{tail}", &line[..idx])
}

fn proxied_url(absolute: &str, headers_param: &str) -> String {
    format!(
        "/proxy/stream?url={}{headers_param}",
        urlencoding::encode(absolute)
    )
}

pub async fn stream(Query(q): Query<ProxyQuery>) -> Response {
    let Some(raw) = q.url.filter(|u| !u.is_empty()) else {
        return fail(StatusCode::BAD_REQUEST, "url parameter required".into());
    };

    // Already decoded once by the query parser; the JS decodes again because
    // some callers double-encode. Only accept the second decode if it still
    // parses as a URL.
    let target_raw = urlencoding::decode(&raw)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| raw.clone());

    let Ok(target) = reqwest::Url::parse(&target_raw) else {
        return fail(StatusCode::BAD_REQUEST, "url parameter is not a valid URL".into());
    };

    let custom: HashMap<String, String> = q
        .headers
        .as_deref()
        .and_then(|h| serde_json::from_str(h).ok())
        .unwrap_or_default();

    let mut req = crate::http::GIGA
        .get(target.clone())
        .timeout(UPSTREAM_TIMEOUT)
        .header(header::ACCEPT, "*/*")
        .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9");

    // NOTE: like the JS version, the caller's Range header is not forwarded.
    // Seeking therefore always refetches from zero. Kept as-is so this port
    // doesn't change behaviour; worth fixing separately.
    for (k, v) in &custom {
        req = req.header(k.as_str(), v.as_str());
    }

    let upstream = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[proxy/stream] error: {e}");
            return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("Proxy failed: {e}"));
        }
    };

    let status = upstream.status();
    if !status.is_success() && status != StatusCode::PARTIAL_CONTENT {
        return fail(status, format!("Upstream returned status {}", status.as_u16()));
    }

    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let looks_like_manifest = content_type.contains("mpegurl")
        || content_type.contains("m3u8")
        || target.as_str().contains(".m3u8");

    if looks_like_manifest {
        let body = match upstream.text().await {
            Ok(t) => t,
            Err(e) => return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("Proxy failed: {e}")),
        };

        if body.starts_with("#EXTM3U") {
            let headers_param = q
                .headers
                .as_deref()
                .map(|h| format!("&headers={}", urlencoding::encode(h)))
                .unwrap_or_default();

            let rewritten = rewrite_manifest(&body, &target, &headers_param);
            return (
                StatusCode::OK,
                cors(),
                [
                    (header::CONTENT_TYPE, "application/vnd.apple.mpegurl"),
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                rewritten,
            )
                .into_response();
        }

        // Content-type lied — fall through and hand back what we read.
        return (StatusCode::OK, cors(), body).into_response();
    }

    // Binary segment or subtitle track: relay it.
    let mut out = HeaderMap::new();
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(if content_type.is_empty() {
            "application/octet-stream"
        } else {
            &content_type
        })
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    out.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    for h in [header::CONTENT_LENGTH, header::CONTENT_RANGE] {
        if let Some(v) = upstream.headers().get(&h) {
            out.insert(h, v.clone());
        }
    }

    let body = Body::from_stream(upstream.bytes_stream());
    (status, cors(), out, body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_relative_segments_to_absolute_proxy_urls() {
        let base = reqwest::Url::parse("https://cdn.example.com/hls/master.m3u8").unwrap();
        let m3u8 = "#EXTM3U\n#EXT-X-TARGETDURATION:6\nseg1.ts\n";
        let out = rewrite_manifest(m3u8, &base, "");

        assert!(out.contains("#EXT-X-TARGETDURATION:6"));
        assert!(out.contains("/proxy/stream?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fseg1.ts"));
    }

    #[test]
    fn rewrites_uri_attribute_and_keeps_quotes_and_tail() {
        let base = reqwest::Url::parse("https://cdn.example.com/hls/master.m3u8").unwrap();
        let line = "#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x1234";
        let out = rewrite_manifest(line, &base, "");

        assert!(out.contains("METHOD=AES-128"));
        assert!(out.contains("IV=0x1234"), "tail after URI must survive: {out}");
        assert!(out.contains("URI=\"/proxy/stream?url=https%3A%2F%2Fcdn.example.com%2Fhls%2Fkey.bin\""));
    }
}
