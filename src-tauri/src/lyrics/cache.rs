#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use super::models::NormalizedLyrics;

const MAX_CACHE_ENTRIES: usize = 100;
const CACHE_TTL_SECS: u64 = 3600; // 1 giờ

struct CacheEntry {
    lyrics: NormalizedLyrics,
    inserted_at: Instant,
}

pub struct LyricsCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
}

impl LyricsCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, track_key: &str) -> Option<NormalizedLyrics> {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = map.get(track_key) {
            if entry.inserted_at.elapsed().as_secs() < CACHE_TTL_SECS {
                return Some(entry.lyrics.clone());
            } else {
                map.remove(track_key);
            }
        }
        None
    }

    pub fn insert(&self, track_key: String, lyrics: NormalizedLyrics) {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= MAX_CACHE_ENTRIES {
            let oldest_key = map
                .iter()
                .min_by_key(|(_, v)| v.inserted_at)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest_key {
                map.remove(&k);
            }
        }
        map.insert(
            track_key,
            CacheEntry {
                lyrics,
                inserted_at: Instant::now(),
            },
        );
    }

    #[allow(dead_code)]
    pub fn clear(&self) {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        map.clear();
    }
}
