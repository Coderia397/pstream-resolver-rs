//! CinemaOS — custom extractor.

use crate::http::PROXY;
use crate::models::{MediaKind, ProviderResult, Source};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{AesGcm, aes::Aes256};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, REFERER, USER_AGENT};
use serde_json::Value;
use sha2::Sha256;
use std::time::Duration;

const BASE: &str = "https://cinemaos.live";
const NAME: &str = "CinemaOS 🎥";
pub const ID: &str = "cinemaos";

fn generate_secret(
    tmdb_id: &str,
    imdb_id: Option<&str>,
    season_id: Option<u32>,
    episode_id: Option<u32>,
) -> String {
    let e = "a7f3b9c2e8d4f1a6b5c9e2d7f4a8b3c6e1d9f7a4b2c8e5d3f9a6b4c1e7d2f8a5";
    let d = "d3f8a5b2c9e6d1f7a4b8c5e2d9f3a6b1c7e4d8f2a9b5c3e7d4f1a8b6c2e9d5f3";

    let mut parts = vec![];
    if !tmdb_id.is_empty() {
        parts.push(format!("tmdbId:{}", tmdb_id));
    }
    if let Some(imdb) = imdb_id {
        if !imdb.is_empty() {
            parts.push(format!("imdbId:{}", imdb));
        }
    }
    if let Some(s) = season_id {
        parts.push(format!("seasonId:{}", s));
    }
    if let Some(ep) = episode_id {
        parts.push(format!("episodeId:{}", ep));
    }

    let s = parts.join("|");

    type HmacSha256 = Hmac<Sha256>;
    let mut mac1 = <HmacSha256 as Mac>::new_from_slice(e.as_bytes()).unwrap();
    mac1.update(s.as_bytes());
    let res1 = hex::encode(mac1.finalize().into_bytes());

    let mut mac2 = <HmacSha256 as Mac>::new_from_slice(d.as_bytes()).unwrap();
    mac2.update(res1.as_bytes());
    hex::encode(mac2.finalize().into_bytes())
}

fn decrypt_data(data: &Value) -> Option<Value> {
    let encrypted_hex = data.get("encrypted")?.as_str()?;
    let iv_hex = data.get("cin")?.as_str()?;
    let tag_hex = data.get("mao")?.as_str()?;

    let encrypted = hex::decode(encrypted_hex).ok()?;
    let iv = hex::decode(iv_hex).ok()?;
    let tag = hex::decode(tag_hex).ok()?;

    let key_hex = "a1b2c3d4e4f6477658455678901477567890abcdef1234567890abcdef123456";

    let key = if let Some(salt_val) = data.get("salt") {
        let salt_hex = salt_val.as_str()?;
        let salt = hex::decode(salt_hex).ok()?;
        let mut key_buf = [0u8; 32];
        pbkdf2_hmac::<Sha256>(key_hex.as_bytes(), &salt, 100000, &mut key_buf);
        key_buf.to_vec()
    } else {
        use sha2::Digest;
        let mut hasher = Sha256::new();
        hasher.update(&iv);
        hasher.finalize().to_vec()
    };

    // 16-byte nonce requires custom typenum instead of default Aes256Gcm (which is 12 bytes)
    type Aes256Gcm16 = AesGcm<Aes256, aes_gcm::aead::consts::U16>;
    let cipher = Aes256Gcm16::new_from_slice(&key).ok()?;
    
    // In AES-GCM, the authentication tag is appended to the ciphertext
    let mut ciphertext = encrypted.clone();
    ciphertext.extend_from_slice(&tag);

    let payload = Payload {
        msg: &ciphertext,
        aad: &[],
    };

    let decrypted = cipher.decrypt(iv.as_slice().into(), payload).ok()?;
    serde_json::from_slice(&decrypted).ok()
}

pub async fn scrape(
    tmdb_id: &str,
    kind: MediaKind,
    season: u32,
    episode: u32,
) -> Option<ProviderResult> {
    let (m_type, season_id, episode_id) = if kind.is_movie() {
        ("movie", None, None)
    } else {
        ("tv", Some(season), Some(episode))
    };

    let secret = generate_secret(tmdb_id, None, season_id, episode_id);
    let ck = "6775dc8e702c08643385273df088c14952c590ddda02d14f";
    // We try mb2 scraper (MovieBoxV2) which is usually reliable on this site
    let api_url = format!(
        "{}/api/providerv5/scrape?type={}&tmdbId={}&secret={}&_ck={}&scraper=mb2",
        BASE, m_type, tmdb_id, secret, ck
    );

    let watch_path = if kind.is_movie() {
        format!("/watch/movie/{tmdb_id}")
    } else {
        format!("/watch/tv/{tmdb_id}/{season}/{episode}")
    };
    let referer = format!("{}{}", BASE, watch_path);

    println!("[{}] Probing {}", ID, api_url);

    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"),
    );
    headers.insert(REFERER, HeaderValue::from_str(&referer).ok()?);
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

    let resp = PROXY
        .get(&api_url)
        .headers(headers)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .ok()?;
        
    let data: Value = resp.json().await.ok()?;

    let scrape_data = if let Some(encrypted_flag) = data.get("encrypted") {
        if encrypted_flag.as_bool() == Some(true) {
            let inner_data = data.get("data")?;
            decrypt_data(inner_data)?
        } else {
            data
        }
    } else {
        data
    };

    let sources = scrape_data.get("sources")?.as_object()?;
    
    // Extract the highest resolution or the first available stream.
    let mut streams = Vec::new();
    for (quality, info) in sources {
        if let Some(url) = info.get("url").and_then(|v| v.as_str()) {
            let res = format!("{}p", quality);
            streams.push(Source::direct_m3u8(url, &res));
        }
    }

    ProviderResult::some_if_any(NAME, ID, streams)
}


