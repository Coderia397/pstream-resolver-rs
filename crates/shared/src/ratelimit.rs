//! Per-IP rate limiting — port of `rateLimited` in `local-resolver/server.mjs`.
//!
//! Every resolve goes out over this device's one IP. Anyone who finds the URL
//! could otherwise hammer it — burning mobile data and, worse, getting that
//! single IP throttled or banned by the providers, which takes the whole site
//! down for everyone.
//!
//! Cache hits are deliberately *not* counted: they cost nothing and never
//! touch a provider. Only real provider work is metered.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Provider-hitting requests per IP per window.
const MAX: u32 = 30;
const WINDOW: Duration = Duration::from_secs(60);

/// Sweep expired buckets once the map gets this big, so a burst of unique
/// source IPs can't grow it without bound. The JS uses a setInterval; doing
/// it on write avoids keeping a timer alive on a battery-powered device.
const SWEEP_AT: usize = 4_096;

struct Bucket {
    count: u32,
    reset: Instant,
}

static BUCKETS: Lazy<RwLock<HashMap<String, Bucket>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Record a request from `ip` and report whether it exceeded the limit.
pub fn check(ip: &str) -> bool {
    let now = Instant::now();
    let Ok(mut b) = BUCKETS.write() else {
        // Poisoned lock shouldn't take the resolver down; fail open.
        return false;
    };

    if b.len() >= SWEEP_AT {
        b.retain(|_, e| e.reset > now);
    }

    match b.get_mut(ip) {
        Some(e) if now <= e.reset => {
            e.count += 1;
            e.count > MAX
        }
        // Absent, or the window has rolled over.
        _ => {
            b.insert(
                ip.to_string(),
                Bucket {
                    count: 1,
                    reset: now + WINDOW,
                },
            );
            false
        }
    }
}

/// Best-effort client IP.
///
/// Behind the Cloudflare tunnel `cf-connecting-ip` is the real visitor and is
/// set by Cloudflare itself, so it's trustworthy here in a way a bare
/// `x-forwarded-for` would not be. The first entry of XFF is the fallback.
pub fn client_ip(headers: &axum::http::HeaderMap) -> String {
    for name in ["cf-connecting-ip", "x-forwarded-for"] {
        if let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()) {
            if let Some(first) = v.split(',').next() {
                let first = first.trim();
                if !first.is_empty() {
                    return first.to_string();
                }
            }
        }
    }
    "?".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit_then_blocks() {
        let ip = "test-limit-1";
        for i in 1..=MAX {
            assert!(!check(ip), "request {i} should be allowed");
        }
        assert!(check(ip), "request {} should be blocked", MAX + 1);
    }

    #[test]
    fn tracks_each_ip_separately() {
        let a = "test-sep-a";
        let b = "test-sep-b";
        for _ in 0..=MAX {
            check(a);
        }
        assert!(check(a), "a is over the limit");
        assert!(!check(b), "b must be unaffected by a");
    }

    #[test]
    fn reads_cf_connecting_ip_in_preference_to_xff() {
        use axum::http::{HeaderMap, HeaderValue};
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_static("1.1.1.1, 2.2.2.2"));
        assert_eq!(client_ip(&h), "1.1.1.1");

        h.insert("cf-connecting-ip", HeaderValue::from_static("3.3.3.3"));
        assert_eq!(client_ip(&h), "3.3.3.3");
    }
}
