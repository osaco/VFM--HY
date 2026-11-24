//! Seek-bar hover thumbnail cache. Pure cache/timing logic; the actual frame grab is done by
//! a headless libVLC player in the app layer. Caching and cancellation live here so they are
//! unit-testable and shared with the phase-4 thumbnail cache conventions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Bucket a hover position into a time key so nearby hovers reuse one frame.
pub fn bucket_position(position: f32, duration_ms: i64, buckets: u32) -> i64 {
    if duration_ms <= 0 || buckets == 0 {
        return 0;
    }
    let buckets = buckets as f64;
    let p = (position as f64).clamp(0.0, 1.0);
    ((p * buckets).floor() as i64).clamp(0, buckets as i64 - 1) * (duration_ms / buckets as i64)
}

/// Bounded FIFO/LRU-ish cache of grabbed thumbnail files keyed by (media path, time bucket).
#[derive(Debug, Clone)]
pub struct SeekThumbnailCache {
    root: PathBuf,
    capacity: usize,
    order: Arc<std::sync::Mutex<Vec<(PathBuf, i64)>>>,
    map: Arc<std::sync::Mutex<HashMap<(PathBuf, i64), PathBuf>>>,
}

impl SeekThumbnailCache {
    pub fn new(root: impl Into<PathBuf>, capacity: usize) -> Self {
        Self {
            root: root.into(),
            capacity: capacity.max(1),
            order: Arc::new(std::sync::Mutex::new(Vec::new())),
            map: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, media: &Path, time_ms: i64) -> PathBuf {
        let name = format!(
            "{}-{}.png",
            crate::library::ThumbnailCache::key_for(media).trim_end_matches(".png"),
            time_ms
        );
        self.root.join(name)
    }

    pub fn get(&self, media: &Path, time_ms: i64) -> Option<PathBuf> {
        let key = (media.to_path_buf(), time_ms);
        let map = self.map.lock().unwrap();
        let path = map.get(&key).cloned();
        if path.is_some() {
            // Move to the back (most recently used).
            let mut order = self.order.lock().unwrap();
            if let Some(i) = order.iter().position(|k| *k == key) {
                let entry = order.remove(i);
                order.push(entry);
            }
        }
        path.filter(|p| p.is_file())
    }

    /// Register a captured file. Evicts the oldest entry beyond capacity.
    pub fn put(&self, media: &Path, time_ms: i64, path: PathBuf) {
        let key = (media.to_path_buf(), time_ms);
        {
            let mut map = self.map.lock().unwrap();
            map.insert(key.clone(), path);
        }
        let mut order = self.order.lock().unwrap();
        order.retain(|k| *k != key);
        order.push(key.clone());
        while order.len() > self.capacity {
            let old = order.remove(0);
            if let Some(p) = self.map.lock().unwrap().remove(&old) {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.map.lock().unwrap().len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        self.map.lock().unwrap().clear();
        self.order.lock().unwrap().clear();
    }
}

/// Cancellation token for an in-flight thumbnail grab.
#[derive(Debug, Clone, Default)]
pub struct GrabCancellation(Arc<AtomicBool>);

impl GrabCancellation {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_are_bounded_and_repeatable() {
        assert_eq!(bucket_position(0.0, 120_000, 10), 0);
        assert_eq!(bucket_position(1.0, 120_000, 10), 108_000);
        assert_eq!(
            bucket_position(0.55, 120_000, 10),
            bucket_position(0.54, 120_000, 10)
        );
        assert_eq!(bucket_position(0.5, 0, 10), 0);
    }

    #[test]
    fn cache_stores_gets_and_evicts() {
        let dir = std::env::temp_dir().join(format!("vfm-seekcache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = SeekThumbnailCache::new(&dir, 2);
        let media = Path::new("show/ep.mkv");
        let a = cache.path_for(media, 1_000);
        let b = cache.path_for(media, 2_000);
        let c = cache.path_for(media, 3_000);
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        std::fs::write(&c, b"c").unwrap();
        cache.put(media, 1_000, a.clone());
        cache.put(media, 2_000, b.clone());
        assert!(cache.get(media, 1_000).is_some());
        cache.put(media, 3_000, c.clone());
        assert_eq!(cache.len(), 2);
        // `a` was touched by get() so it is now MRU; the untouched `b` is evicted.
        assert!(cache.get(media, 1_000).is_some());
        assert!(cache.get(media, 2_000).is_none());
        assert!(cache.get(media, 3_000).is_some());
        cache.clear();
        assert!(cache.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancellation_is_sticky() {
        let c = GrabCancellation::new();
        assert!(!c.is_cancelled());
        c.cancel();
        assert!(c.is_cancelled());
        c.cancel();
        assert!(c.is_cancelled());
    }
}
