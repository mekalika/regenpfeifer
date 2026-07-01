//! A simple sharded concurrent memo cache: emphasized-syllable -> match result.
//! The same syllables recur across hundreds of thousands of words, so memoizing
//! the (emphasize-fixed) match output globally is the dominant speedup.

use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

#[cfg(feature = "stats")]
use std::sync::atomic::{AtomicU64, Ordering};

const SHARDS: usize = 512;

// Hit/miss/timing counters are gated behind the `stats` feature so the default
// build carries zero instrumentation overhead (notably no Instant::now() per miss).
#[cfg(feature = "stats")]
pub static HITS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "stats")]
pub static MISSES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "stats")]
pub static MATCH_NANOS: AtomicU64 = AtomicU64::new(0);

pub struct MatchCache {
    shards: Vec<Mutex<FxHashMap<String, Arc<[String]>>>>,
}

impl MatchCache {
    pub fn new() -> Self {
        let mut shards = Vec::with_capacity(SHARDS);
        for _ in 0..SHARDS {
            shards.push(Mutex::new(FxHashMap::default()));
        }
        MatchCache { shards }
    }

    #[inline]
    fn shard_of(&self, key: &str) -> &Mutex<FxHashMap<String, Arc<[String]>>> {
        // Reuse rustc-hash's FxHasher (already a dependency) for shard selection;
        // the shard only distributes locks, so any well-spread hash will do.
        let mut h = rustc_hash::FxHasher::default();
        key.hash(&mut h);
        &self.shards[(h.finish() as usize) % SHARDS]
    }

    /// Get a cached result, or compute via `f`, store, and return it.
    pub fn get_or_compute<F>(&self, key: &str, f: F) -> Arc<[String]>
    where
        F: FnOnce() -> Vec<String>,
    {
        {
            let guard = self.shard_of(key).lock().unwrap();
            if let Some(v) = guard.get(key) {
                #[cfg(feature = "stats")]
                HITS.fetch_add(1, Ordering::Relaxed);
                return v.clone();
            }
        }
        #[cfg(feature = "stats")]
        MISSES.fetch_add(1, Ordering::Relaxed);
        #[cfg(feature = "stats")]
        let t = std::time::Instant::now();
        let computed: Arc<[String]> = f().into();
        #[cfg(feature = "stats")]
        MATCH_NANOS.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        let mut guard = self.shard_of(key).lock().unwrap();
        // another thread may have inserted; keep first
        guard
            .entry(key.to_string())
            .or_insert_with(|| computed.clone())
            .clone()
    }
}
