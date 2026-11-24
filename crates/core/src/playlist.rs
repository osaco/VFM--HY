//! Saved playlists (collections) — deliberately separate from the transient Queue.

use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub paths: Vec<PathBuf>,
    pub created: SystemTime,
}

impl Playlist {
    pub fn new(id: i64, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            paths: Vec::new(),
            created: SystemTime::now(),
        }
    }

    pub fn rename(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn add(&mut self, path: impl Into<PathBuf>) -> bool {
        let path = path.into();
        if self.paths.contains(&path) {
            return false;
        }
        self.paths.push(path);
        true
    }

    pub fn add_many(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        let before = self.paths.len();
        for p in paths {
            self.add(p);
        }
        self.paths.len() - before
    }

    pub fn remove(&mut self, path: &std::path::Path) -> bool {
        if let Some(i) = self.paths.iter().position(|p| p == path) {
            self.paths.remove(i);
            true
        } else {
            false
        }
    }

    pub fn contains(&self, path: &std::path::Path) -> bool {
        self.paths.iter().any(|p| p == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn add_is_idempotent() {
        let mut p = Playlist::new(1, "Show");
        assert!(p.add("a.mkv"));
        assert!(!p.add("a.mkv"));
        assert_eq!(p.paths.len(), 1);
    }

    #[test]
    fn rename_and_remove() {
        let mut p = Playlist::new(1, "Old");
        p.add("a.mkv");
        p.rename("New");
        assert_eq!(p.name, "New");
        assert!(p.remove(Path::new("a.mkv")));
        assert!(!p.remove(Path::new("a.mkv")));
        assert!(p.paths.is_empty());
    }

    #[test]
    fn add_many_returns_new_count() {
        let mut p = Playlist::new(2, "Mixed");
        p.add("a.mkv");
        assert_eq!(p.add_many(["a.mkv", "b.mkv"].map(PathBuf::from)), 1);
        assert_eq!(p.paths.len(), 2);
    }
}
