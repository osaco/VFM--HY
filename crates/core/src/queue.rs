//! Queue model — the transient "play next" list. Distinct from a saved Playlist.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaItem {
    pub path: PathBuf,
    pub title: String,
    pub duration_ms: i64,
}

impl MediaItem {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Self {
            path,
            title,
            duration_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("queue index {index} is out of range (length {len})")]
pub struct InvalidIndex {
    pub index: usize,
    pub len: usize,
}

impl InvalidIndex {
    fn check(len: usize, index: usize) -> Result<(), Self> {
        if index >= len {
            Err(Self { index, len })
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct Queue {
    items: Vec<MediaItem>,
    current: Option<usize>,
    /// Stack of visited indices used to make `previous()` exact across shuffles.
    history: Vec<usize>,
    shuffle: bool,
    repeat: RepeatMode,
    /// Deterministic LCG state so shuffle is unit-testable and reproducible.
    rng: u64,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new(0x5eed_1234)
    }
}

impl Queue {
    pub fn new(seed: u64) -> Self {
        Self {
            items: Vec::new(),
            current: None,
            history: Vec::new(),
            shuffle: false,
            repeat: RepeatMode::Off,
            rng: seed,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn items(&self) -> &[MediaItem] {
        &self.items
    }
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }
    pub fn current(&self) -> Option<&MediaItem> {
        self.current.and_then(|i| self.items.get(i))
    }
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_shuffle(&mut self, on: bool) {
        self.shuffle = on;
    }
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    fn next_rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        self.rng >> 33
    }

    pub fn enqueue(&mut self, item: MediaItem) {
        self.items.push(item);
    }

    pub fn enqueue_many(&mut self, items: impl IntoIterator<Item = MediaItem>) {
        self.items.extend(items);
    }

    pub fn insert(&mut self, index: usize, item: MediaItem) -> Result<(), InvalidIndex> {
        if index > self.items.len() {
            return Err(InvalidIndex {
                index,
                len: self.items.len(),
            });
        }
        self.items.insert(index, item);
        if let Some(c) = self.current {
            if index <= c {
                self.current = Some(c + 1);
            }
        }
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Result<(), InvalidIndex> {
        InvalidIndex::check(self.items.len(), index)?;
        self.items.remove(index);
        self.current = match self.current {
            Some(c) if c == index => None,
            Some(c) if c > index => Some(c - 1),
            other => other,
        };
        self.history.retain(|h| *h < self.items.len());
        Ok(())
    }

    /// Drag-reorder. Returns the new index of the moved item.
    pub fn move_item(&mut self, from: usize, to: usize) -> Result<usize, InvalidIndex> {
        InvalidIndex::check(self.items.len(), from)?;
        InvalidIndex::check(self.items.len(), to)?;
        let item = self.items.remove(from);
        let to = to.min(self.items.len());
        self.items.insert(to, item);
        if let Some(c) = self.current {
            self.current = Some(if c == from {
                to
            } else if from < c && to >= c {
                c - 1
            } else if from > c && to <= c {
                c + 1
            } else {
                c
            });
        }
        Ok(to)
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.current = None;
        self.history.clear();
    }

    pub fn play(&mut self, index: usize) -> Result<(), InvalidIndex> {
        InvalidIndex::check(self.items.len(), index)?;
        if let Some(c) = self.current {
            self.history.push(c);
        }
        self.current = Some(index);
        Ok(())
    }

    /// Index of the next item to play, or None when the queue is exhausted.
    pub fn advance(&mut self) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }
        if self.repeat == RepeatMode::One {
            return self.current;
        }
        let next = match self.current {
            None => 0,
            Some(c) if self.shuffle && self.items.len() > 1 => {
                let n = self.items.len();
                let mut idx = (self.next_rand() as usize) % (n - 1);
                if idx >= c {
                    idx += 1;
                }
                idx
            }
            Some(c) => {
                let n = c + 1;
                if n >= self.items.len() {
                    if self.repeat == RepeatMode::All {
                        0
                    } else {
                        return None;
                    }
                } else {
                    n
                }
            }
        };
        if let Some(c) = self.current {
            self.history.push(c);
        }
        self.current = Some(next);
        Some(next)
    }

    pub fn previous(&mut self) -> Option<usize> {
        let prev = self.history.pop()?;
        if prev < self.items.len() {
            self.current = Some(prev);
            Some(prev)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: usize) -> Queue {
        let mut q = Queue::new(42);
        for i in 0..n {
            q.enqueue(MediaItem::new(format!("ep{i}.mkv")));
        }
        q
    }

    #[test]
    fn plays_in_order_and_stops_at_end() {
        let mut q = q(3);
        assert_eq!(q.advance(), Some(0));
        assert_eq!(q.advance(), Some(1));
        assert_eq!(q.advance(), Some(2));
        assert_eq!(q.advance(), None);
    }

    #[test]
    fn repeat_all_wraps_and_repeat_one_holds() {
        let mut q = q(2);
        q.set_repeat(RepeatMode::All);
        assert_eq!(q.advance(), Some(0));
        assert_eq!(q.advance(), Some(1));
        assert_eq!(q.advance(), Some(0));
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.advance(), Some(0));
    }

    #[test]
    fn shuffle_never_repeats_current_index() {
        let mut q = q(6);
        q.set_shuffle(true);
        let mut prev = q.advance().unwrap();
        for _ in 0..50 {
            let next = q.advance().unwrap();
            assert_ne!(next, prev, "shuffle repeated the current item");
            prev = next;
        }
    }

    #[test]
    fn previous_walks_history() {
        let mut q = q(3);
        q.advance();
        q.advance();
        assert_eq!(q.previous(), Some(0));
        assert_eq!(q.previous(), None);
    }

    #[test]
    fn remove_and_move_keep_current_valid() {
        let mut q = q(4);
        q.play(2).unwrap();
        assert_eq!(q.current_index(), Some(2));
        q.remove(1).unwrap();
        assert_eq!(q.current_index(), Some(1));
        assert_eq!(q.move_item(0, 2).unwrap(), 2);
        assert_eq!(q.current().unwrap().title, "ep2");
    }

    #[test]
    fn invalid_indices_are_rejected() {
        let mut q = q(1);
        assert!(q.insert(2, MediaItem::new("x")).is_err());
        assert!(q.remove(5).is_err());
        assert!(q.move_item(0, 9).is_err());
        assert!(q.play(1).is_err());
    }
}
