//! Library discovery, browsing and filtering. Pure logic: no OS-specific APIs, no libVLC.
//! Folder scanning is incremental and cancellable so the UI can run it on a worker thread.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Extensions VFM recognises as playable video files.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "m4v", "wmv", "flv", "ts", "mpg", "mpeg", "3gp",
];
/// Extensions VFM recognises as playable audio files.
pub const MUSIC_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "m4a", "aac", "wma", "opus", "aiff",
];

/// Broad family a media file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    #[default]
    Video,
    Music,
    Stream,
}

impl MediaKind {
    /// Classify by file extension. URL streams have no extension and are handled separately.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
            Some(Self::Video)
        } else if MUSIC_EXTENSIONS.contains(&ext.as_str()) {
            Some(Self::Music)
        } else {
            None
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Music => "music",
            Self::Stream => "stream",
        }
    }
}

/// One row in a library view. Metadata not yet known stays at its neutral default.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryEntry {
    pub path: PathBuf,
    pub title: String,
    pub kind: MediaKind,
    pub duration_ms: i64,
    pub size_bytes: u64,
    /// Unix seconds when the item entered the library.
    pub added: i64,
    /// Unix seconds of the last playback.
    pub last_played: i64,
    pub position_ms: i64,
    pub watched: bool,
    pub pinned: bool,
    /// Cached thumbnail file, when one has been generated.
    pub thumbnail: Option<PathBuf>,
}

impl LibraryEntry {
    pub fn new(path: impl Into<PathBuf>) -> Option<Self> {
        let path = path.into();
        let kind = MediaKind::from_path(&path)?;
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Some(Self {
            path,
            title,
            kind,
            duration_ms: 0,
            size_bytes: 0,
            added: 0,
            last_played: 0,
            position_ms: 0,
            watched: false,
            pinned: false,
            thumbnail: None,
        })
    }

    /// 0.0 ..= 1.0 watch progress, safe for zero/unknown durations.
    pub fn progress(&self) -> f32 {
        if self.duration_ms <= 0 || self.position_ms <= 0 {
            return 0.0;
        }
        (self.position_ms as f32 / self.duration_ms as f32).clamp(0.0, 1.0)
    }

    pub fn is_stream(&self) -> bool {
        self.kind == MediaKind::Stream
    }

    /// Human-readable file size with binary units.
    pub fn size_label(&self) -> String {
        let bytes = self.size_bytes;
        if bytes < 1024 {
            return format!("{bytes} B");
        }
        const UNITS: &[&str] = &["KB", "MB", "GB", "TB"];
        let mut value = bytes as f64 / 1024.0;
        let mut unit = 0;
        while value >= 1024.0 && unit + 1 < UNITS.len() {
            value /= 1024.0;
            unit += 1;
        }
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Controls which files a folder walk picks up and how deep it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    pub include_video: bool,
    pub include_music: bool,
    pub max_depth: usize,
    /// Skip entries whose name starts with a dot (`.git`, `@eaDir`, …).
    pub skip_hidden: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            include_video: true,
            include_music: true,
            max_depth: 32,
            skip_hidden: true,
        }
    }
}

/// Cancellable handle returned by [`FolderScanner`]. Clone it to signal a running scan.
#[derive(Debug, Clone, Default)]
pub struct ScanCancellation(Arc<AtomicBool>);

impl ScanCancellation {
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

/// Discovers media below a root folder. Deterministic (sorted) and cancellation-aware.
#[derive(Debug, Clone, Default)]
pub struct FolderScanner {
    cancellation: ScanCancellation,
}

impl FolderScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancellation(&self) -> ScanCancellation {
        self.cancellation.clone()
    }

    /// Walk `root` iteratively (no recursion limit) and return discovered entries sorted by
    /// path. Directory symlinks are skipped by comparing canonical paths, so a loop cannot
    /// hang the scan. I/O errors on individual entries are skipped rather than aborting.
    pub fn scan(&self, root: &Path, options: &ScanOptions) -> Vec<LibraryEntry> {
        if self.cancellation.is_cancelled() || !root.is_dir() {
            return Vec::new();
        }
        let mut found = Vec::new();
        let mut visited = HashSet::new();
        let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

        while let Some((dir, depth)) = stack.pop() {
            if self.cancellation.is_cancelled() {
                break;
            }
            let Ok(canonical) = dir.canonicalize() else {
                continue;
            };
            if !visited.insert(canonical) {
                continue;
            }
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut children: Vec<PathBuf> =
                read.filter_map(|e| e.ok().map(|e| e.path())).collect();
            children.sort();

            for path in children {
                if self.cancellation.is_cancelled() {
                    break;
                }
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if options.skip_hidden && name.starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    if depth < options.max_depth {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                let Some(kind) = MediaKind::from_path(&path) else {
                    continue;
                };
                if (kind == MediaKind::Video && !options.include_video)
                    || (kind == MediaKind::Music && !options.include_music)
                {
                    continue;
                }
                if let Some(mut entry) = LibraryEntry::new(path) {
                    entry.size_bytes = std::fs::metadata(&entry.path).map(|m| m.len()).unwrap_or(0);
                    found.push(entry);
                }
            }
        }
        found.sort_by(|a, b| a.path.cmp(&b.path));
        found
    }
}

/// A parsed M3U/M3U8 stream entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEntry {
    pub url: String,
    pub title: String,
}

/// Parse an M3U / M3U8 playlist into stream entries. `#EXTINF:` lines provide the title;
/// comment lines and blanks are ignored. Relative URLs are resolved against `base` when it
/// is a URL with a scheme, and against the parent directory when it is a local path.
pub fn parse_m3u(input: &str, base: Option<&str>) -> Vec<StreamEntry> {
    let mut entries = Vec::new();
    let mut pending_title: Option<String> = None;
    let input = input.trim_start_matches('\u{feff}');

    for raw in input.lines() {
        let line = raw.trim().trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            // Format: "#EXTINF:-1,Artist - Title" — title is everything after the first comma.
            let title = info
                .split_once(',')
                .map(|(_, t)| t.trim().to_string())
                .unwrap_or_default();
            pending_title = Some(title);
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let url = resolve_url(line, base);
        let title = pending_title
            .take()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                Path::new(line)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| line.to_string())
            });
        entries.push(StreamEntry { url, title });
    }
    entries
}

fn resolve_url(line: &str, base: Option<&str>) -> String {
    if line.contains("://") || base.is_none() {
        return line.to_string();
    }
    let base = base.unwrap_or_default();
    if base.contains("://") {
        // Simple URL join for the common case of a relative path under a stream directory.
        let (prefix, _) = base.rsplit_once('/').unwrap_or((base, ""));
        format!("{prefix}/{line}")
    } else {
        let parent = Path::new(base).parent().unwrap_or_else(|| Path::new("."));
        parent.join(line).to_string_lossy().into_owned()
    }
}

/// Sort orders exposed by the library toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortMode {
    NameAsc,
    NameDesc,
    DateNewest,
    DateOldest,
    DurationDesc,
    DurationAsc,
    SizeDesc,
    SizeAsc,
    UnwatchedFirst,
    WatchedFirst,
}

impl SortMode {
    pub const ALL: &'static [Self] = &[
        Self::NameAsc,
        Self::NameDesc,
        Self::DateNewest,
        Self::DateOldest,
        Self::DurationDesc,
        Self::DurationAsc,
        Self::SizeDesc,
        Self::SizeAsc,
        Self::UnwatchedFirst,
        Self::WatchedFirst,
    ];

    /// Stable id matching the serde representation, used by the settings schema.
    pub fn id(self) -> &'static str {
        match self {
            Self::NameAsc => "name_asc",
            Self::NameDesc => "name_desc",
            Self::DateNewest => "date_newest",
            Self::DateOldest => "date_oldest",
            Self::DurationDesc => "duration_desc",
            Self::DurationAsc => "duration_asc",
            Self::SizeDesc => "size_desc",
            Self::SizeAsc => "size_asc",
            Self::UnwatchedFirst => "unwatched_first",
            Self::WatchedFirst => "watched_first",
        }
    }

    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .iter()
            .copied()
            .find(|mode| mode.id() == id)
            .unwrap_or(Self::NameAsc)
    }

    /// Stable English label. The UI passes these through the i18n layer instead of
    /// displaying them directly; kept here for tests and for the settings fallback.
    pub fn label(self) -> &'static str {
        match self {
            Self::NameAsc => "Name A–Z",
            Self::NameDesc => "Name Z–A",
            Self::DateNewest => "Recently added",
            Self::DateOldest => "Oldest first",
            Self::DurationDesc => "Longest first",
            Self::DurationAsc => "Shortest first",
            Self::SizeDesc => "Largest first",
            Self::SizeAsc => "Smallest first",
            Self::UnwatchedFirst => "Unwatched first",
            Self::WatchedFirst => "Watched first",
        }
    }
}

/// How the library content area presents its entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    Grid,
    #[default]
    List,
    Detail,
}

impl ViewMode {
    pub fn next(self) -> Self {
        match self {
            Self::Grid => Self::List,
            Self::List => Self::Detail,
            Self::Detail => Self::Grid,
        }
    }
}

/// Search + sort + kind filter applied to a set of entries before it reaches the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryFilter {
    pub query: String,
    pub kind: Option<MediaKind>,
    pub unwatched_only: bool,
    pub sort: SortMode,
}

impl Default for LibraryFilter {
    fn default() -> Self {
        Self {
            query: String::new(),
            kind: None,
            unwatched_only: false,
            sort: SortMode::NameAsc,
        }
    }
}

impl LibraryFilter {
    /// Filter and sort a snapshot of entries. Always returns a new vector so callers can
    /// keep the unsorted source for breadcrumb/folder navigation.
    pub fn apply(&self, entries: Vec<LibraryEntry>) -> Vec<LibraryEntry> {
        let needle = self.query.trim().to_ascii_lowercase();
        let mut rows: Vec<LibraryEntry> = entries
            .into_iter()
            .filter(|e| self.kind.is_none_or(|k| e.kind == k))
            .filter(|e| !self.unwatched_only || !e.watched)
            .filter(|e| {
                needle.is_empty()
                    || e.title.to_ascii_lowercase().contains(&needle)
                    || e.path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains(&needle)
            })
            .collect();

        match self.sort {
            SortMode::NameAsc => rows.sort_by_key(|e| e.title.clone()),
            SortMode::NameDesc => rows.sort_by_key(|e| std::cmp::Reverse(e.title.clone())),
            SortMode::DateNewest => rows.sort_by_key(|e| std::cmp::Reverse(e.added)),
            SortMode::DateOldest => rows.sort_by_key(|e| e.added),
            SortMode::DurationDesc => rows.sort_by_key(|e| std::cmp::Reverse(e.duration_ms)),
            SortMode::DurationAsc => rows.sort_by_key(|e| e.duration_ms),
            SortMode::SizeDesc => rows.sort_by_key(|e| std::cmp::Reverse(e.size_bytes)),
            SortMode::SizeAsc => rows.sort_by_key(|e| e.size_bytes),
            SortMode::UnwatchedFirst => rows.sort_by_key(|e| e.watched),
            SortMode::WatchedFirst => rows.sort_by_key(|e| std::cmp::Reverse(e.watched)),
        }
        rows
    }
}

/// Content-addressed thumbnail cache. File names are a stable FNV-1a hash of the media path
/// so long Windows paths never exceed the 260-character limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailCache {
    root: PathBuf,
}

impl ThumbnailCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Stable 64-bit hash of the path string (case-normalised on Windows).
    pub fn key_for(path: &Path) -> String {
        let text = path
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}.png")
    }

    pub fn path_for(&self, path: &Path) -> PathBuf {
        self.root.join(Self::key_for(path))
    }

    pub fn get(&self, path: &Path) -> Option<PathBuf> {
        let candidate = self.path_for(path);
        candidate.is_file().then_some(candidate)
    }

    pub fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.root)
    }

    /// Remove cached thumbnails not present in `keep`. Returns the number removed.
    pub fn prune(&self, keep: &[PathBuf]) -> usize {
        let wanted: HashSet<String> = keep.iter().map(|p| Self::key_for(p)).collect();
        let Ok(read) = std::fs::read_dir(&self.root) else {
            return 0;
        };
        let mut removed = 0;
        for file in read.filter_map(Result::ok) {
            let name = file.file_name().to_string_lossy().into_owned();
            if name.ends_with(".png")
                && !wanted.contains(&name)
                && std::fs::remove_file(file.path()).is_ok()
            {
                removed += 1;
            }
        }
        removed
    }
}

/// A bounded slice of a large list — the basis of virtualised library views. Keeping only
/// the visible window in Slint means 10k+ items do not create 10k UI elements.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub rows: Vec<T>,
    pub offset: usize,
    pub total: usize,
    pub page_size: usize,
}

impl<T: Clone> Page<T> {
    pub fn slice(all: &[T], offset: usize, page_size: usize) -> Self {
        let page_size = page_size.max(1);
        let offset = offset.min(all.len());
        let end = offset.saturating_add(page_size).min(all.len());
        Self {
            rows: all[offset..end].to_vec(),
            offset,
            total: all.len(),
            page_size,
        }
    }

    pub fn has_next(&self) -> bool {
        self.offset + self.rows.len() < self.total
    }

    pub fn has_previous(&self) -> bool {
        self.offset > 0
    }

    pub fn scroll_fraction(&self) -> f32 {
        if self.total <= self.page_size {
            return 0.0;
        }
        self.offset as f32 / (self.total - self.page_size) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_media_extensions_case_insensitively() {
        assert_eq!(
            MediaKind::from_path(Path::new("S01E01.MKV")),
            Some(MediaKind::Video)
        );
        assert_eq!(
            MediaKind::from_path(Path::new("song.FLAC")),
            Some(MediaKind::Music)
        );
        assert_eq!(MediaKind::from_path(Path::new("notes.txt")), None);
        assert_eq!(MediaKind::from_path(Path::new("noext")), None);
    }

    #[test]
    fn entry_size_and_progress_are_safe() {
        let mut e = LibraryEntry::new("show/ep1.mkv").unwrap();
        assert_eq!(e.size_label(), "0 B");
        e.size_bytes = 2 * 1024 * 1024;
        assert_eq!(e.size_label(), "2.0 MB");
        assert_eq!(e.progress(), 0.0);
        e.duration_ms = 100_000;
        e.position_ms = 25_000;
        assert_eq!(e.progress(), 0.25);
        e.position_ms = 200_000;
        assert_eq!(e.progress(), 1.0);
    }

    #[test]
    fn scanner_walks_nested_folders_and_sorts() {
        let root = std::env::temp_dir().join(format!("vfm-scan-{}", std::process::id()));
        let show = root.join("Show/Season 2");
        std::fs::create_dir_all(&show).unwrap();
        std::fs::write(root.join("Show/ep2.mkv"), b"x").unwrap();
        std::fs::write(show.join("ep1.mp4"), b"x").unwrap();
        std::fs::write(show.join("cover.jpg"), b"x").unwrap();
        std::fs::write(root.join(".hidden.mkv"), b"x").unwrap();

        let entries = FolderScanner::new().scan(&root, &ScanOptions::default());
        assert_eq!(entries.len(), 2);
        assert!(entries[0].path.ends_with("Season 2/ep1.mp4"));
        assert!(entries[1].path.ends_with("Show/ep2.mkv"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scanner_respects_depth_kind_and_cancellation() {
        let root = std::env::temp_dir().join(format!("vfm-scan2-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/deep.mkv"), b"x").unwrap();
        std::fs::write(root.join("song.mp3"), b"x").unwrap();

        let shallow = ScanOptions {
            max_depth: 1,
            ..Default::default()
        };
        let entries = FolderScanner::new().scan(&root, &shallow);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.ends_with("song.mp3"));

        // With max_depth 1 the only media under root is song.mp3; excluding music leaves none.
        let video_only = ScanOptions {
            include_music: false,
            max_depth: 1,
            ..Default::default()
        };
        assert!(FolderScanner::new().scan(&root, &video_only).is_empty());

        let scanner = FolderScanner::new();
        let cancel = scanner.cancellation();
        cancel.cancel();
        assert!(scanner.scan(&root, &ScanOptions::default()).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn m3u_parses_extinf_and_absolute_urls() {
        let text = "#EXTM3U\n#EXTINF:-1,News - Live\nhttps://example.test/live.m3u8\n#EXTINF:120,Movie\nhttps://example.test/m.mp4\n";
        let streams = parse_m3u(text, None);
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].title, "News - Live");
        assert_eq!(streams[0].url, "https://example.test/live.m3u8");
        assert_eq!(streams[1].title, "Movie");
    }

    #[test]
    fn m3u_resolves_relative_urls() {
        let text = "#EXTINF:-1,Relative\nchannel/stream.m3u8\n";
        let web = parse_m3u(text, Some("https://example.test/lists/list.m3u"));
        assert_eq!(web[0].url, "https://example.test/lists/channel/stream.m3u8");
        let local = parse_m3u(text, Some("C:/lists/list.m3u"));
        assert!(local[0]
            .url
            .replace('\\', "/")
            .ends_with("lists/channel/stream.m3u8"));
    }

    #[test]
    fn filter_searches_sorts_and_hides_watched() {
        let mut a = LibraryEntry::new("a.mkv").unwrap();
        a.title = "Zebra".into();
        a.added = 1;
        a.size_bytes = 10;
        a.duration_ms = 5_000;
        let mut b = LibraryEntry::new("b.mkv").unwrap();
        b.title = "Apple".into();
        b.added = 2;
        b.size_bytes = 100;
        b.duration_ms = 20_000;
        b.watched = true;
        let rows = vec![a.clone(), b.clone()];

        let name = LibraryFilter {
            sort: SortMode::NameAsc,
            ..Default::default()
        }
        .apply(rows.clone());
        assert_eq!(name[0].title, "Apple");
        let newest = LibraryFilter {
            sort: SortMode::DateNewest,
            ..Default::default()
        }
        .apply(rows.clone());
        assert_eq!(newest[0].title, "Apple");
        assert_eq!(newest[1].title, "Zebra");
        let oldest = LibraryFilter {
            sort: SortMode::DateOldest,
            ..Default::default()
        }
        .apply(rows.clone());
        assert_eq!(oldest[0].title, "Zebra");
        let largest = LibraryFilter {
            sort: SortMode::SizeDesc,
            ..Default::default()
        }
        .apply(rows.clone());
        assert_eq!(largest[0].title, "Apple");
        let query = LibraryFilter {
            query: "zeb".into(),
            ..Default::default()
        }
        .apply(rows.clone());
        assert_eq!(query.len(), 1);
        let unwatched = LibraryFilter {
            unwatched_only: true,
            ..Default::default()
        }
        .apply(rows);
        assert_eq!(unwatched.len(), 1);
        assert_eq!(unwatched[0].title, "Zebra");
    }

    #[test]
    fn sort_mode_labels_cover_all_variants() {
        for mode in SortMode::ALL {
            assert!(!mode.label().is_empty());
        }
        assert_eq!(SortMode::ALL.len(), 10);
    }

    #[test]
    fn view_mode_cycles() {
        let v = ViewMode::default();
        assert_eq!(v, ViewMode::List);
        assert_eq!(v.next(), ViewMode::Detail);
        assert_eq!(v.next().next(), ViewMode::Grid);
        assert_eq!(v.next().next().next(), ViewMode::List);
    }

    #[test]
    fn thumbnail_keys_are_stable_and_path_unique() {
        let a = ThumbnailCache::key_for(Path::new("C:/Shows/Ep1.mkv"));
        let b = ThumbnailCache::key_for(Path::new("c:\\shows\\ep1.mkv"));
        let c = ThumbnailCache::key_for(Path::new("C:/Shows/Ep2.mkv"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.ends_with(".png"));
    }

    #[test]
    fn page_windows_large_lists() {
        let all: Vec<usize> = (0..10_000).collect();
        let first = Page::slice(&all, 0, 50);
        assert_eq!(first.rows.len(), 50);
        assert_eq!(first.total, 10_000);
        assert!(first.has_next());
        assert!(!first.has_previous());
        assert_eq!(first.scroll_fraction(), 0.0);
        let last = Page::slice(&all, 9_990, 50);
        assert_eq!(last.rows.len(), 10);
        assert!(!last.has_next());
        assert_eq!(Page::slice(&all, 99_999, 50).offset, 10_000);
    }
}
