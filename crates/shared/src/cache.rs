//! In-memory TTL cache for resolved streams.
//!
//! Deliberately a plain map behind an RwLock rather than a cache crate: the
//! working set is a few hundred entries at most, and the phone benefits far
//! more from a small binary than from a sharded LRU.
//!
//! Entries are swept lazily on read, plus a bulk sweep when the map grows
//! past `MAX_ENTRIES`, so a long-running process can't grow without bound.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Above this, do a full sweep of expired entries before inserting.
/// Matches MAX_CACHE_ENTRIES in the JS.
const MAX_ENTRIES: usize = 5_000;

struct Entry {
    value: serde_json::Value,
    expires: Instant,
}

static STORE: Lazy<RwLock<HashMap<String, Entry>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn get(key: &str) -> Option<serde_json::Value> {
    // Fast path: shared lock, and only take the write lock if we found
    // something that turned out to be stale.
    {
        let store = STORE.read().ok()?;
        match store.get(key) {
            None => return None,
            Some(e) if e.expires > Instant::now() => return Some(e.value.clone()),
            Some(_) => {} // expired — fall through and evict
        }
    }
    if let Ok(mut store) = STORE.write() {
        store.remove(key);
    }
    None
}

pub fn put(key: String, value: serde_json::Value, ttl: Duration) {
    let Ok(mut store) = STORE.write() else { return };

    if store.len() >= MAX_ENTRIES {
        let now = Instant::now();
        store.retain(|_, e| e.expires > now);
    }

    store.insert(
        key,
        Entry {
            value,
            expires: Instant::now() + ttl,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn returns_live_entry_and_drops_expired_one() {
        put("live".into(), json!({"a": 1}), Duration::from_secs(60));
        assert_eq!(get("live"), Some(json!({"a": 1})));

        put("dead".into(), json!({"b": 2}), Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(get("dead"), None);
    }
}
