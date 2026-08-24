//! Provider health — port of the `health` map in `local-resolver/server.mjs`.
//!
//! Counts hits and misses per provider so ordering can favour whichever one
//! is actually delivering. In-memory only and deliberately not persisted:
//! it's a hint, not state worth surviving a restart, and a provider that was
//! healthy yesterday tells you little about now.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;

#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub ok: u64,
    pub fail: u64,
    pub last_ms: u128,
}

/// Below this many samples a rate is noise, so callers get the neutral 0.5.
const MIN_SAMPLES: u64 = 5;

static HEALTH: Lazy<RwLock<HashMap<&'static str, Stats>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn record(provider_id: &'static str, ok: bool, took: Duration) {
    let Ok(mut h) = HEALTH.write() else { return };
    let e = h.entry(provider_id).or_default();
    if ok {
        e.ok += 1;
    } else {
        e.fail += 1;
    }
    e.last_ms = took.as_millis();
}

/// Success rate in 0.0..=1.0, or 0.5 when there aren't enough samples to say.
pub fn success_rate(provider_id: &str) -> f64 {
    let Ok(h) = HEALTH.read() else { return 0.5 };
    match h.get(provider_id) {
        Some(s) if s.ok + s.fail >= MIN_SAMPLES => s.ok as f64 / (s.ok + s.fail) as f64,
        _ => 0.5,
    }
}

/// Snapshot for a future `/api/health` endpoint.
#[allow(dead_code)]
pub fn snapshot() -> Vec<(&'static str, Stats)> {
    HEALTH
        .read()
        .map(|h| h.iter().map(|(k, v)| (*k, *v)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_neutral_until_enough_samples() {
        for _ in 0..(MIN_SAMPLES - 1) {
            record("thin", true, Duration::from_millis(10));
        }
        assert_eq!(success_rate("thin"), 0.5);

        record("thin", true, Duration::from_millis(10));
        assert_eq!(success_rate("thin"), 1.0);
    }

    #[test]
    fn unknown_provider_is_neutral() {
        assert_eq!(success_rate("never-seen"), 0.5);
    }
}
