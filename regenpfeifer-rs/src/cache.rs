//! A simple sharded concurrent memo cache: emphasized-syllable -> match result.
//! The same syllables recur across hundreds of thousands of words, so memoizing
//! the (emphasize-fixed) match output globally is the dominant speedup.

use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const SHARDS: usize = 512;

pub static HITS: AtomicU64 = AtomicU64::new(0);
pub static MISSES: AtomicU64 = AtomicU64::new(0);
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
        // cheap FNV-1a hash
        let mut h: u64 = 0xcbf29ce484222325;
        for b in key.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        &self.shards[(h as usize) % SHARDS]
    }

    /// Get a cached result, or compute via `f`, store, and return it.
    pub fn get_or_compute<F>(&self, key: &str, f: F) -> Arc<[String]>
    where
        F: FnOnce() -> Vec<String>,
    {
        {
            let guard = self.shard_of(key).lock().unwrap();
            if let Some(v) = guard.get(key) {
                HITS.fetch_add(1, Ordering::Relaxed);
                return v.clone();
            }
        }
        MISSES.fetch_add(1, Ordering::Relaxed);
        let t = std::time::Instant::now();
        let computed: Arc<[String]> = f().into();
        MATCH_NANOS.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        let mut guard = self.shard_of(key).lock().unwrap();
        // another thread may have inserted; keep first
        guard
            .entry(key.to_string())
            .or_insert_with(|| computed.clone())
            .clone()
    }
}

#[cfg(feature = "instr")]
pub static ITERS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "instr")]
pub static CANDS: AtomicU64 = AtomicU64::new(0);
