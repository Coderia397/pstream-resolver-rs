//! Wire types shared by every extractor.
//!
//! The serde renames here are load-bearing: this JSON is already consumed by
//! the frontend, so the shape must match `local-resolver/server.mjs` exactly.

use serde::{Deserialize, Serialize};

/// What kind of thing we're resolving. TMDB calls them movie / tv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Movie,
    Tv,
}

impl MediaKind {
    /// The JS side accepts "movie", "film" or anything else meaning tv.
    pub fn parse(s: &str) -> Self {
        match s {
            "movie" | "film" => Self::Movie,
            _ => Self::Tv,
        }
    }

    pub fn is_movie(self) -> bool {
        matches!(self, Self::Movie)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub url: String,
    pub quality: String,
    #[serde(rename = "isM3U8")]
    pub is_m3u8: bool,
    #[serde(rename = "noProxy")]
    pub no_proxy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(rename = "providerId", skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Some hosts 403 a segment request without the originating page as
    /// Referer; the player forwards this through /proxy/stream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
    /// True when the URL is an iframe embed rather than a direct manifest.
    #[serde(rename = "isEmbed", skip_serializing_if = "Option::is_none")]
    pub is_embed: Option<bool>,
}

impl Source {
    /// An m3u8 source that browsers can hit directly (host sends ACAO: *).
    pub fn direct_m3u8(url: impl Into<String>, quality: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            quality: quality.into(),
            is_m3u8: true,
            no_proxy: true,
            provider: None,
            provider_id: None,
            referer: None,
            is_embed: None,
        }
    }

    /// Attach the page the manifest came from, for hosts that check Referer.
    pub fn with_referer(mut self, referer: impl Into<String>) -> Self {
        self.referer = Some(referer.into());
        self
    }

    /// Mark explicitly as a direct manifest rather than an iframe embed.
    pub fn not_embed(mut self) -> Self {
        self.is_embed = Some(false);
        self
    }

    /// Tag a source with the provider that produced it.
    pub fn tagged(mut self, provider: &str, provider_id: &str) -> Self {
        self.provider = Some(provider.to_string());
        self.provider_id = Some(provider_id.to_string());
        self
    }

}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtitle {
    pub url: String,
    pub lang: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResult {
    pub success: bool,
    pub provider: String,
    #[serde(rename = "providerId")]
    pub provider_id: String,
    pub sources: Vec<Source>,
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
}

impl ProviderResult {
    pub fn new(provider: &str, provider_id: &str, sources: Vec<Source>) -> Self {
        Self {
            success: true,
            provider: provider.to_string(),
            provider_id: provider_id.to_string(),
            sources,
            subtitles: Vec::new(),
        }
    }

    /// Extractors return None for "no sources"; an empty vec would otherwise
    /// serialise as a success with nothing playable in it.
    pub fn some_if_any(provider: &str, provider_id: &str, sources: Vec<Source>) -> Option<Self> {
        if sources.is_empty() {
            None
        } else {
            Some(Self::new(provider, provider_id, sources))
        }
    }
}
