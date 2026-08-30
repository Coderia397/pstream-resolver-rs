//! Shared between the phone resolver and the giga backend.
//!
//! Everything here is service-agnostic: the provider extractors, the HTTP
//! clients they go out through, the resolve cache, and the CORS and rate-limit
//! policy.
//!
//! It lives in one crate so the 13 extractors exist in exactly one place.
//! Providers change their markup often; two copies would drift, and the copy
//! that wasn't being looked at would be the one that broke.

pub mod cache;
pub mod cors;
pub mod extractors;
pub mod health;
pub mod http;
pub mod models;
pub mod probe;

pub mod ratelimit;
pub mod subdl;
pub mod utils;
pub mod youtube;

pub use models::{MediaKind, ProviderResult, Source, Subtitle};
pub use utils::{
    matches_year_tolerance, normalize_quality, parse_year, quality_rank, slugify,
    sort_sources_by_quality,
};
