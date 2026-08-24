//! Shared HTTP clients — the Rust port of `utils/http.js`.
//!
//! Two clients, same split as the JS side:
//!   GIGA  — straight out of this machine, browser-shaped headers.
//!   PROXY — via RESIDENTIAL_PROXY_URL when one is configured.
//!
//! On the phone both are usually the same thing: it is already on a
//! residential mobile IP, which is the whole reason the resolver lives there.
//!
//! TLS is rustls, not OpenSSL. OpenSSL cannot link into the static musl
//! binary we ship to the device, so this choice is not negotiable.

use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, HeaderValue};
use std::time::Duration;

/// Rotated so a provider doesn't see one fingerprint for every request.
const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
];

/// Cheap round-robin. Ordering doesn't matter, only that it varies.
pub fn random_ua() -> &'static str {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let i = N.fetch_add(1, Ordering::Relaxed);
    USER_AGENTS[i % USER_AGENTS.len()]
}

/// Mirrors BROWSER_HEADERS in utils/http.js. Several providers fingerprint
/// on these, so keep them in step with the JS side when it changes.
fn browser_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    let pairs: &[(&str, &str)] = &[
        ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"),
        ("Accept-Language", "en-US,en;q=0.9"),
        ("Cache-Control", "no-cache"),
        ("Pragma", "no-cache"),
        ("Sec-Ch-Ua", "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Google Chrome\";v=\"120\""),
        ("Sec-Ch-Ua-Mobile", "?0"),
        ("Sec-Ch-Ua-Platform", "\"Windows\""),
        ("Sec-Fetch-Dest", "document"),
        ("Sec-Fetch-Mode", "navigate"),
        ("Sec-Fetch-Site", "none"),
        ("Sec-Fetch-User", "?1"),
        ("Upgrade-Insecure-Requests", "1"),
    ];
    for (k, v) in pairs {
        if let Ok(val) = HeaderValue::from_str(v) {
            h.insert(*k, val);
        }
    }
    h
}

fn base_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(random_ua())
        .default_headers(browser_headers())
        .cookie_store(true)
        .gzip(true)
        .brotli(true)
        .connect_timeout(Duration::from_secs(5))
        // Providers redirect a lot; more than this is a loop.
        .redirect(reqwest::redirect::Policy::limited(10))
}

/// Direct client. 10s ceiling, same as gigaAxios.
pub static GIGA: Lazy<reqwest::Client> = Lazy::new(|| {
    base_builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("GIGA client build failed")
});

/// Residential-proxy client when RESIDENTIAL_PROXY_URL is set, else direct.
/// 15s ceiling, same as proxyAxios — proxied hops are slower.
pub static PROXY: Lazy<reqwest::Client> = Lazy::new(|| {
    let mut b = base_builder().timeout(Duration::from_secs(15));
    if let Ok(url) = std::env::var("RESIDENTIAL_PROXY_URL") {
        if !url.trim().is_empty() {
            match reqwest::Proxy::all(url.trim()) {
                Ok(p) => b = b.proxy(p),
                Err(e) => eprintln!("[http] bad RESIDENTIAL_PROXY_URL, ignoring: {e}"),
            }
        }
    }
    b.build().expect("PROXY client build failed")
});

/// Client that does not verify TLS certificates.
///
/// Mirrors the `new https.Agent({ rejectUnauthorized: false })` that a couple
/// of extractors use, because those hosts serve broken or self-signed chains
/// and otherwise cannot be reached at all. Only for providers that already
/// need it — everything else goes through GIGA or PROXY.
///
/// Nothing sensitive is sent over this: it fetches public pages and the only
/// thing extracted is a manifest URL.
pub static INSECURE: Lazy<reqwest::Client> = Lazy::new(|| {
    base_builder()
        .timeout(Duration::from_secs(15))
        .danger_accept_invalid_certs(true)
        .build()
        .expect("INSECURE client build failed")
});

/// GET a URL and return the body as text, or None on any failure.
///
/// Extractors treat every error the same way — try the next provider — so
/// collapsing the error into None keeps their call sites flat.
pub async fn get_text(client: &reqwest::Client, url: &str, timeout: Duration) -> Option<String> {
    let resp = client.get(url).timeout(timeout).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}

/// As `get_text`, plus extra headers (Referer and Origin, mostly).
pub async fn get_text_with(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
    extra: &[(&str, &str)],
) -> Option<String> {
    let mut req = client.get(url).timeout(timeout);
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}
