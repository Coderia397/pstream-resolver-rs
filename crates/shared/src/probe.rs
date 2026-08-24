//! `/api/media-probe` — fetch the head of a remote file for the frontend.
//!
//! The browser can't read Debrid CDN URLs directly (no CORS headers), so it
//! asks us for the first 2 MB and parses the MKV/MP4 headers itself to list
//! the internal audio and subtitle tracks.
//!
//! ## Why this one has a guard the JS doesn't
//!
//! `index.js` fetches whatever URL it is handed, with no restriction. That is
//! a server-side request forgery hole: this process listens on the same host
//! it would be fetching from, so `?url=http://127.0.0.1:8790/api/deploy` or a
//! LAN address turns the endpoint into a way to reach anything the phone can
//! reach. It matters more here than on a cloud host — the phone sits on a home
//! or hotspot network with other devices on it.
//!
//! Public addresses are unaffected, which is all the Debrid CDNs are.

use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::net::IpAddr;
use std::time::Duration;

/// Enough for an EBML/MP4 header; the frontend asks for no more.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct ProbeQuery {
    url: Option<String>,
}

fn bad(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

/// Addresses that are never a legitimate media CDN and always a way back into
/// something local.
fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_documentation()
                // 100.64.0.0/10, carrier-grade NAT — the phone's own mobile range
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                // fc00::/7 unique-local and fe80::/10 link-local
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Reject a target that points somewhere local.
///
/// Only literal addresses and obvious local names are caught here. A hostname
/// that resolves to a private address still gets through, because checking
/// that properly means resolving before connecting and then pinning the
/// connection to the address we checked — worth doing if this endpoint ever
/// runs somewhere with real internal services next to it.
fn host_is_local(host: &str) -> bool {
    let h = host.trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();

    if h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") || h.ends_with(".internal") {
        return true;
    }
    match h.parse::<IpAddr>() {
        Ok(ip) => is_blocked_ip(ip),
        // A regular hostname; see the caveat above.
        Err(_) => false,
    }
}

pub async fn media_probe(Query(q): Query<ProbeQuery>) -> Response {
    let Some(raw) = q.url.filter(|u| !u.is_empty()) else {
        return bad(StatusCode::BAD_REQUEST, "Missing ?url= parameter");
    };

    // Already decoded once by the query parser; some callers double-encode.
    let decoded = urlencoding::decode(&raw)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| raw.clone());

    let Ok(target) = reqwest::Url::parse(&decoded) else {
        return bad(StatusCode::BAD_REQUEST, "Invalid URL");
    };
    if !matches!(target.scheme(), "http" | "https") {
        return bad(StatusCode::BAD_REQUEST, "Only http and https are supported");
    }
    match target.host_str() {
        Some(h) if !host_is_local(h) => {}
        _ => return bad(StatusCode::FORBIDDEN, "Host not allowed"),
    }

    let resp = crate::http::GIGA
        .get(target.clone())
        .timeout(Duration::from_secs(15))
        .header(header::RANGE, format!("bytes=0-{}", MAX_BYTES - 1))
        .header(header::ACCEPT, "*/*")
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return bad(StatusCode::BAD_GATEWAY, &format!("Probe failed: {e}")),
    };

    let status = resp.status();
    if !status.is_success() && status != StatusCode::PARTIAL_CONTENT {
        return bad(
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            &format!("Probe failed: upstream returned {}", status.as_u16()),
        );
    }

    // Hard cap regardless of what the server sent: a host that ignores Range
    // would otherwise stream an entire film through the phone's mobile data.
    let body = match resp.bytes().await {
        Ok(b) if b.len() as u64 > MAX_BYTES => b.slice(0..MAX_BYTES as usize),
        Ok(b) => b,
        Err(e) => return bad(StatusCode::BAD_GATEWAY, &format!("Probe failed: {e}")),
    };

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_the_ways_back_into_this_device() {
        for h in [
            "localhost",
            "127.0.0.1",
            "::1",
            "[::1]",
            "10.19.114.26",   // the phone's own hotspot address
            "192.168.1.10",
            "172.16.0.5",
            "169.254.169.254", // cloud metadata
            "100.70.1.76",     // carrier-grade NAT, the phone's mobile address
            "printer.local",
            "0.0.0.0",
        ] {
            assert!(host_is_local(h), "{h} should be blocked");
        }
    }

    #[test]
    fn allows_ordinary_public_hosts() {
        for h in [
            "cdn.real-debrid.com",
            "srv321.abjust.store",
            "8.8.8.8",
            "example.com",
            "172.32.0.1", // just outside 172.16/12
            "100.128.0.1", // just outside 100.64/10
        ] {
            assert!(!host_is_local(h), "{h} should be allowed");
        }
    }
}
