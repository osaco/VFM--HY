//! Phase 4 glue between vfm-core library/queue state and the Slint LibraryWindow / queue
//! panel. All mutations to the queue and database happen here on the UI thread; libVLC is
//! never touched from this module (callers provide a small playback sink trait).

use std::path::{Path, PathBuf};

use slint::{SharedString, VecModel};
use vfm_core::{
    parse_m3u, FolderScanner, LibraryEntry, LibraryFilter, LibraryStore, MediaItem, MediaKind,
    Queue, ScanOptions, SortMode, StreamEntry, ViewMode,
};

use crate::{LibraryWindow, MediaRow, PlayerWindow, QueueRow};

// Re-exports used by the shell's wiring.
pub use vfm_core::ThumbnailCache;

/// Which library section is showing. Mirrors the sidebar index in Slint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Home,
    Videos,
    Music,
    Network,
    Playlists,
    Recent,
    Pinned,
    Folders,
}

impl Section {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Videos,
            2 => Self::Music,
            3 => Self::Network,
            4 => Self::Playlists,
            5 => Self::Recent,
            6 => Self::Pinned,
            7 => Self::Folders,
            _ => Self::Home,
        }
    }
}

/// State owned by the library controller.
pub struct LibraryController {
    pub section: Section,
    pub query: String,
    pub sort: SortMode,
    pub view: ViewMode,
    pub rows: Vec<LibraryEntry>,
    pub selected: Option<usize>,
    pub thumbs: ThumbnailCache,
    scan_cancel: Option<vfm_core::ScanCancellation>,
}

impl LibraryController {
    pub fn new(thumbs: ThumbnailCache) -> Self {
        Self {
            section: Section::Home,
            query: String::new(),
            sort: SortMode::NameAsc,
            view: ViewMode::List,
            rows: Vec::new(),
            selected: None,
            thumbs,
            scan_cancel: None,
        }
    }

    /// Reload the visible rows from the store for the current section.
    pub fn reload(&mut self, db: &LibraryStore) -> Result<(), rusqlite::Error> {
        let mut entries = match self.section {
            Section::Home => {
                let mut e = db.recent_entries(24)?;
                // Continue-watching first: items with a saved position but not watched.
                e.sort_by_key(|x| (!x.watched && x.position_ms > 0) as u8);
                e.reverse();
                e
            }
            Section::Videos => db
                .all_entries()?
                .into_iter()
                .filter(|e| e.kind == MediaKind::Video)
                .collect(),
            Section::Music => db
                .all_entries()?
                .into_iter()
                .filter(|e| e.kind == MediaKind::Music)
                .collect(),
            Section::Network => {
                // Streams are shown as synthetic entries with a `stream:` prefix.
                db.streams()?
                    .into_iter()
                    .map(|s| {
                        let mut e = LibraryEntry::new(PathBuf::from(format!("stream:{}", s.url)))
                            .unwrap_or_else(|| LibraryEntry {
                                path: PathBuf::from(&s.url),
                                title: s.title.clone(),
                                kind: MediaKind::Stream,
                                duration_ms: 0,
                                size_bytes: 0,
                                added: 0,
                                last_played: 0,
                                position_ms: 0,
                                watched: false,
                                pinned: false,
                                thumbnail: None,
                            });
                        e.kind = MediaKind::Stream;
                        e.title = s.title;
                        e
                    })
                    .collect()
            }
            Section::Playlists => Vec::new(), // populated on demand when a playlist is opened
            Section::Recent => db.recent_entries(200)?,
            Section::Pinned => db.pinned_entries()?,
            Section::Folders => {
                let mut out = Vec::new();
                for folder in db.folders()? {
                    out.extend(db.entries_in_folder(&folder)?);
                }
                // de-duplicate by path (folders can overlap)
                let mut seen = std::collections::HashSet::new();
                out.retain(|e| seen.insert(e.path.clone()));
                out
            }
        };
        let filter = LibraryFilter {
            query: self.query.clone(),
            kind: None,
            unwatched_only: false,
            sort: self.sort,
        };
        entries = filter.apply(entries);
        self.rows = entries;
        self.selected = None;
        Ok(())
    }

    /// Start a cancellable incremental folder scan, upserting each discovered file.
    pub fn scan_folder(
        &mut self,
        db: &LibraryStore,
        folder: &Path,
        replace: bool,
    ) -> Result<usize, rusqlite::Error> {
        if replace {
            let existing = db.entries_in_folder(folder)?;
            for e in &existing {
                db.set_watched(&e.path, false)?; // keep rows; caller may delete if desired
            }
        }
        db.add_folder(folder)?;
        let scanner = FolderScanner::new();
        self.scan_cancel = Some(scanner.cancellation());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut count = 0;
        for entry in scanner.scan(folder, &ScanOptions::default()) {
            if self.scan_cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
                break;
            }
            let mut entry = entry;
            entry.added = now;
            db.upsert_entry(&entry)?;
            count += 1;
        }
        self.scan_cancel = None;
        self.reload(db)?;
        Ok(count)
    }

    #[allow(dead_code)] // wired when the scan-progress UI lands
    pub fn cancel_scan(&self) {
        if let Some(c) = &self.scan_cancel {
            c.cancel();
        }
    }

    pub fn entry(&self, index: usize) -> Option<&LibraryEntry> {
        self.rows.get(index)
    }

    #[allow(dead_code)] // wired to the library context menu in Phase 7
    pub fn mark_watched(
        &mut self,
        db: &LibraryStore,
        index: usize,
        watched: bool,
    ) -> Result<(), rusqlite::Error> {
        if let Some(e) = self.rows.get(index) {
            db.set_watched(&e.path, watched)?;
            self.reload(db)?;
        }
        Ok(())
    }

    pub fn toggle_pin(&mut self, db: &LibraryStore, index: usize) -> Result<bool, rusqlite::Error> {
        let Some(e) = self.rows.get(index) else {
            return Ok(false);
        };
        let next = !e.pinned;
        db.set_pinned(&e.path, next)?;
        self.reload(db)?;
        Ok(next)
    }
}

/// Convert a core entry into the Slint row representation.
pub fn to_slint(entry: &LibraryEntry, thumbs: &ThumbnailCache, duration_label: String) -> MediaRow {
    let subtitle = match entry.kind {
        MediaKind::Stream => String::from("network stream"),
        MediaKind::Music => entry.title.clone(),
        MediaKind::Video => {
            let ep = entry
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            ep
        }
    };
    let (has_thumbnail, thumbnail) = match thumbs.get(&entry.path) {
        Some(p) => (true, slint::Image::load_from_path(&p).unwrap_or_default()),
        None => (false, slint::Image::default()),
    };
    MediaRow {
        path: SharedString::from(entry.path.to_string_lossy().as_ref()),
        title: SharedString::from(entry.title.as_str()),
        subtitle: SharedString::from(subtitle.as_str()),
        kind: SharedString::from(entry.kind.name()),
        duration: SharedString::from(duration_label.as_str()),
        size: SharedString::from(entry.size_label().as_str()),
        progress: entry.progress(),
        watched: entry.watched,
        pinned: entry.pinned,
        has_thumbnail,
        thumbnail,
    }
}

pub fn push_rows(win: &LibraryWindow, controller: &LibraryController) {
    let model = VecModel::<MediaRow>::default();
    for entry in &controller.rows {
        let duration = if entry.duration_ms > 0 {
            vfm_core::format_timecode(entry.duration_ms)
        } else {
            String::new()
        };
        model.push(to_slint(entry, &controller.thumbs, duration));
    }
    win.set_rows(slint::ModelRc::new(model));
}

pub fn queue_rows(queue: &Queue) -> Vec<QueueRow> {
    queue
        .items()
        .iter()
        .map(|item| QueueRow {
            path: SharedString::from(item.path.to_string_lossy().as_ref()),
            title: SharedString::from(item.title.as_str()),
            duration: if item.duration_ms > 0 {
                SharedString::from(vfm_core::format_timecode(item.duration_ms).as_str())
            } else {
                SharedString::default()
            },
            playing: false,
        })
        .collect()
}

pub fn push_queue_rows(win: &PlayerWindow, queue: &Queue) {
    let model = VecModel::<QueueRow>::default();
    for mut row in queue_rows(queue) {
        if let Some(i) = queue.current_index() {
            row.playing = queue
                .items()
                .get(i)
                .map(|m| row.path.as_str() == m.path.to_string_lossy().as_ref())
                .unwrap_or(false);
        }
        model.push(row);
    }
    win.set_queue_rows(slint::ModelRc::new(model));
    win.set_queue_current(queue.current_index().map(|i| i as i32).unwrap_or(-1));
}

/// Read a stream source from pasted text. A URL is used directly; a local M3U file is parsed.
pub fn stream_entries_from_text(text: &str) -> Vec<StreamEntry> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let path = Path::new(trimmed);
    if path.is_file() {
        if let Ok(content) = std::fs::read_to_string(path) {
            let parsed = parse_m3u(&content, Some(trimmed));
            if !parsed.is_empty() {
                return parsed;
            }
        }
        return Vec::new();
    }
    if trimmed.to_ascii_lowercase().ends_with(".m3u8")
        || trimmed.to_ascii_lowercase().ends_with(".m3u")
    {
        // Remote playlist: hand the URL to libVLC, which expands it itself.
        return vec![StreamEntry {
            url: trimmed.to_string(),
            title: Path::new(trimmed)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| trimmed.to_string()),
        }];
    }
    vec![StreamEntry {
        url: trimmed.to_string(),
        title: trimmed.to_string(),
    }]
}

pub fn item_from_entry(entry: &LibraryEntry) -> MediaItem {
    let mut item = MediaItem::new(entry.path.clone());
    item.title = entry.title.clone();
    item.duration_ms = entry.duration_ms;
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_indices_map_to_sections() {
        assert_eq!(Section::from_index(0), Section::Home);
        assert_eq!(Section::from_index(1), Section::Videos);
        assert_eq!(Section::from_index(3), Section::Network);
        assert_eq!(Section::from_index(7), Section::Folders);
        assert_eq!(Section::from_index(99), Section::Home);
    }

    #[test]
    fn stream_text_handles_urls_and_playlists() {
        let direct = stream_entries_from_text("  https://example.test/live.m3u8  ");
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].url, "https://example.test/live.m3u8");
        assert!(stream_entries_from_text("   ").is_empty());

        let dir = std::env::temp_dir().join(format!("vfm-m3u-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m3u = dir.join("list.m3u8");
        std::fs::write(
            &m3u,
            "#EXTM3U\n#EXTINF:-1,Channel A\nhttps://example.test/a.m3u8\n",
        )
        .unwrap();
        let parsed = stream_entries_from_text(m3u.to_string_lossy().as_ref());
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].title, "Channel A");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn queue_rows_reflect_items() {
        let mut q = Queue::default();
        q.enqueue(MediaItem::new("a.mkv"));
        q.enqueue(MediaItem::new("b.mkv"));
        let rows = queue_rows(&q);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "a");
        assert!(!rows[0].playing);
    }

    #[test]
    fn entry_to_media_item_preserves_metadata() {
        let mut e = LibraryEntry::new("ep.mkv").unwrap();
        e.title = "Episode 1".into();
        e.duration_ms = 42_000;
        let item = item_from_entry(&e);
        assert_eq!(item.title, "Episode 1");
        assert_eq!(item.duration_ms, 42_000);
        assert_eq!(item.path, e.path);
    }
}
