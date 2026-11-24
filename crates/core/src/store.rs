//! SQLite persistence for library, playlists, queue, progress, bookmarks and presets.
//! Schema is versioned through `PRAGMA user_version`; migrations are additive.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::library::{LibraryEntry, MediaKind, StreamEntry};
use crate::progress::WatchProgress;

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS media (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    position_ms INTEGER NOT NULL DEFAULT 0,
    watched INTEGER NOT NULL DEFAULT 0,
    last_played INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_media_last_played ON media(last_played DESC);
CREATE TABLE IF NOT EXISTS folders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS playlist_items (
    playlist_id INTEGER NOT NULL,
    path TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, path)
);
CREATE TABLE IF NOT EXISTS queue (
    position INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS bookmarks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL,
    time_ms INTEGER NOT NULL,
    name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS presets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    scope TEXT NOT NULL,
    payload TEXT NOT NULL
);
";

/// Additive columns and tables introduced in schema v2 (Phase 4 library browsing).
const SCHEMA_V2: &str = "
ALTER TABLE media ADD COLUMN size_bytes INTEGER NOT NULL DEFAULT 0;
ALTER TABLE media ADD COLUMN added INTEGER NOT NULL DEFAULT 0;
ALTER TABLE media ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_media_pinned ON media(pinned DESC, last_played DESC);
CREATE TABLE IF NOT EXISTS streams (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    added INTEGER NOT NULL
);
";

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A handle to the VFM database. All methods take `&self`; SQLite serialises internally.
pub struct LibraryStore {
    conn: Connection,
}

impl std::fmt::Debug for LibraryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LibraryStore")
    }
}

impl LibraryStore {
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(SCHEMA_V1)?;
            conn.pragma_update(None, "user_version", 1)?;
        }
        if version < 2 {
            // `version` is read once above; ALTER TABLE fails if a v2 database somehow has
            // the columns already, so check before applying.
            let has_added: bool = conn
                .prepare("PRAGMA table_info(media)")?
                .query_map([], |r| r.get::<_, String>(1))?
                .filter_map(Result::ok)
                .any(|name| name == "added");
            if !has_added {
                conn.execute_batch(SCHEMA_V2)?;
            }
            conn.pragma_update(None, "user_version", 2)?;
        }
        Ok(Self { conn })
    }

    pub fn in_memory() -> Result<Self, rusqlite::Error> {
        Self::open(Path::new(":memory:"))
    }

    pub fn upsert_media(
        &self,
        path: &Path,
        title: &str,
        duration_ms: i64,
    ) -> Result<(), rusqlite::Error> {
        let p = path.to_string_lossy();
        self.conn.execute(
            "INSERT INTO media(path,title,duration_ms,last_played) VALUES(?1,?2,?3,?4)
             ON CONFLICT(path) DO UPDATE SET title=excluded.title,
             duration_ms=CASE WHEN excluded.duration_ms > 0 THEN excluded.duration_ms ELSE media.duration_ms END,
             last_played=excluded.last_played",
            params![p.as_ref(), title, duration_ms, now_secs()],
        )?;
        Ok(())
    }

    pub fn save_progress(&self, progress: &WatchProgress) -> Result<(), rusqlite::Error> {
        let p = progress.path.to_string_lossy();
        self.conn.execute(
            "UPDATE media SET position_ms=?2, duration_ms=?3, watched=?4, last_played=?5 WHERE path=?1",
            params![
                p.as_ref(),
                progress.position_ms,
                progress.duration_ms,
                i64::from(progress.watched),
                now_secs(),
            ],
        )?;
        Ok(())
    }

    pub fn progress(&self, path: &Path) -> Result<Option<WatchProgress>, rusqlite::Error> {
        let p = path.to_string_lossy();
        self.conn
            .query_row(
                "SELECT path,position_ms,duration_ms,watched,last_played FROM media WHERE path=?1",
                params![p.as_ref()],
                |r| {
                    Ok(WatchProgress {
                        path: PathBuf::from(r.get::<_, String>(0)?),
                        position_ms: r.get(1)?,
                        duration_ms: r.get(2)?,
                        watched: r.get::<_, i64>(3)? != 0,
                        last_played: UNIX_EPOCH
                            + std::time::Duration::from_secs(
                                r.get::<_, i64>(4).unwrap_or(0).max(0) as u64,
                            ),
                    })
                },
            )
            .optional()
    }

    pub fn recent(&self, limit: i64) -> Result<Vec<PathBuf>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM media ORDER BY last_played DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit], |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map(PathBuf::from)).collect()
    }

    pub fn add_folder(&self, path: &Path) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT OR IGNORE INTO folders(path) VALUES(?1)",
            params![path.to_string_lossy().as_ref()],
        )?;
        Ok(())
    }

    pub fn create_playlist(&self, name: &str) -> Result<i64, rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO playlists(name,created) VALUES(?1,?2)",
            params![name, now_secs()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE playlists SET name=?2 WHERE id=?1",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: i64) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "DELETE FROM playlist_items WHERE playlist_id=?1",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM playlists WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn add_to_playlist(&self, id: i64, path: &Path) -> Result<(), rusqlite::Error> {
        let pos: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM playlist_items WHERE playlist_id=?1",
            params![id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO playlist_items(playlist_id,path,position) VALUES(?1,?2,?3)",
            params![id, path.to_string_lossy().as_ref(), pos],
        )?;
        Ok(())
    }

    pub fn playlist_paths(&self, id: i64) -> Result<Vec<PathBuf>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM playlist_items WHERE playlist_id=?1 ORDER BY position")?;
        let rows = stmt.query_map(params![id], |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map(PathBuf::from)).collect()
    }

    pub fn save_queue(&self, paths: &[PathBuf]) -> Result<(), rusqlite::Error> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE; DELETE FROM queue; COMMIT;")?;
        for (i, p) in paths.iter().enumerate() {
            self.conn.execute(
                "INSERT INTO queue(position,path) VALUES(?1,?2)",
                params![i as i64, p.to_string_lossy().as_ref()],
            )?;
        }
        Ok(())
    }

    pub fn load_queue(&self) -> Result<Vec<PathBuf>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM queue ORDER BY position")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map(PathBuf::from)).collect()
    }

    pub fn add_bookmark(
        &self,
        path: &Path,
        time_ms: i64,
        name: &str,
    ) -> Result<i64, rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO bookmarks(path,time_ms,name) VALUES(?1,?2,?3)",
            params![path.to_string_lossy().as_ref(), time_ms, name],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn bookmarks(&self, path: &Path) -> Result<Vec<(i64, String)>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT time_ms,name FROM bookmarks WHERE path=?1 ORDER BY time_ms")?;
        let rows = stmt.query_map(params![path.to_string_lossy().as_ref()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
        rows.collect()
    }

    /// Bookmarks with their row id, so the UI can rename or delete a specific one.
    pub fn bookmarks_with_id(
        &self,
        path: &Path,
    ) -> Result<Vec<(i64, i64, String)>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,time_ms,name FROM bookmarks WHERE path=?1 ORDER BY time_ms,id")?;
        let rows = stmt.query_map(params![path.to_string_lossy().as_ref()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        rows.collect()
    }

    pub fn rename_bookmark(&self, id: i64, name: &str) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE bookmarks SET name=?2 WHERE id=?1",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn delete_bookmark(&self, id: i64) -> Result<(), rusqlite::Error> {
        self.conn
            .execute("DELETE FROM bookmarks WHERE id=?1", params![id])?;
        Ok(())
    }

    /// Move an existing bookmark to a new time, used when the user re-marks a spot.
    pub fn move_bookmark(&self, id: i64, time_ms: i64) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE bookmarks SET time_ms=?2 WHERE id=?1",
            params![id, time_ms],
        )?;
        Ok(())
    }

    pub fn save_preset(&self, name: &str, scope: &str, json: &str) -> Result<i64, rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO presets(name,scope,payload) VALUES(?1,?2,?3)
             ON CONFLICT(name) DO UPDATE SET scope=excluded.scope, payload=excluded.payload",
            params![name, scope, json],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn presets(&self, scope: &str) -> Result<Vec<(String, String)>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT name,payload FROM presets WHERE scope=?1 ORDER BY name")?;
        let rows = stmt.query_map(params![scope], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// All presets as `(name, scope, payload)`, used by the Phase 6 preset engine.
    pub fn all_presets(&self) -> Result<Vec<(String, String, String)>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT name,scope,payload FROM presets ORDER BY name")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    pub fn rename_preset(&self, old: &str, new: &str) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE presets SET name=?1 WHERE name=?2",
            params![new, old],
        )?;
        Ok(())
    }

    pub fn delete_preset(&self, name: &str) -> Result<(), rusqlite::Error> {
        self.conn
            .execute("DELETE FROM presets WHERE name=?1", params![name])?;
        Ok(())
    }

    // ---- Phase 4: library browsing -------------------------------------------------------

    /// Insert or refresh a discovered media file, preserving watch state.
    pub fn upsert_entry(&self, entry: &LibraryEntry) -> Result<(), rusqlite::Error> {
        let p = entry.path.to_string_lossy();
        self.conn.execute(
            "INSERT INTO media(path,title,duration_ms,size_bytes,added,pinned,last_played)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(path) DO UPDATE SET
               title=excluded.title,
               size_bytes=CASE WHEN excluded.size_bytes > 0 THEN excluded.size_bytes ELSE media.size_bytes END,
               added=CASE WHEN media.added > 0 THEN media.added ELSE excluded.added END",
            params![
                p.as_ref(),
                entry.title,
                entry.duration_ms,
                entry.size_bytes as i64,
                entry.added,
                i64::from(entry.pinned),
                entry.last_played,
            ],
        )?;
        Ok(())
    }

    fn map_entry(r: &rusqlite::Row<'_>) -> Result<LibraryEntry, rusqlite::Error> {
        let path = PathBuf::from(r.get::<_, String>(0)?);
        let kind = MediaKind::from_path(&path).unwrap_or(MediaKind::Video);
        Ok(LibraryEntry {
            path,
            title: r.get(1)?,
            kind,
            duration_ms: r.get(2)?,
            size_bytes: r.get::<_, i64>(3)?.max(0) as u64,
            added: r.get(4)?,
            last_played: r.get(5)?,
            position_ms: r.get(6)?,
            watched: r.get::<_, i64>(7)? != 0,
            pinned: r.get::<_, i64>(8)? != 0,
            thumbnail: None,
        })
    }

    fn query_entries(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<LibraryEntry>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, Self::map_entry)?;
        rows.collect()
    }

    const ENTRY_COLUMNS: &'static str =
        "path,title,duration_ms,size_bytes,added,last_played,position_ms,watched,pinned";

    pub fn all_entries(&self) -> Result<Vec<LibraryEntry>, rusqlite::Error> {
        self.query_entries(&format!("SELECT {} FROM media", Self::ENTRY_COLUMNS), &[])
    }

    pub fn recent_entries(&self, limit: i64) -> Result<Vec<LibraryEntry>, rusqlite::Error> {
        self.query_entries(
            &format!(
                "SELECT {} FROM media ORDER BY last_played DESC LIMIT ?1",
                Self::ENTRY_COLUMNS
            ),
            &[&limit],
        )
    }

    pub fn pinned_entries(&self) -> Result<Vec<LibraryEntry>, rusqlite::Error> {
        self.query_entries(
            &format!(
                "SELECT {} FROM media WHERE pinned=1 ORDER BY last_played DESC",
                Self::ENTRY_COLUMNS
            ),
            &[],
        )
    }

    /// All media whose path is under `folder` (case-insensitive prefix, path-separator safe).
    pub fn entries_in_folder(&self, folder: &Path) -> Result<Vec<LibraryEntry>, rusqlite::Error> {
        let prefix = format!(
            "{}/",
            folder
                .to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
        );
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM media WHERE replace(lower(path), '\\', '/') LIKE ?1 ORDER BY path",
            Self::ENTRY_COLUMNS
        ))?;
        let rows = stmt.query_map(params![format!("{prefix}%")], Self::map_entry)?;
        rows.collect()
    }

    pub fn set_pinned(&self, path: &Path, pinned: bool) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE media SET pinned=?2 WHERE path=?1",
            params![path.to_string_lossy().as_ref(), i64::from(pinned)],
        )?;
        Ok(())
    }

    pub fn set_watched(&self, path: &Path, watched: bool) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE media SET watched=?2, position_ms=CASE WHEN ?2=1 THEN 0 ELSE position_ms END WHERE path=?1",
            params![path.to_string_lossy().as_ref(), i64::from(watched)],
        )?;
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<PathBuf>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM folders ORDER BY path")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map(PathBuf::from)).collect()
    }

    pub fn add_stream(&self, stream: &StreamEntry) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT OR REPLACE INTO streams(url,title,added) VALUES(?1,?2,?3)",
            params![stream.url, stream.title, now_secs()],
        )?;
        Ok(())
    }

    pub fn streams(&self) -> Result<Vec<StreamEntry>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT url,title FROM streams ORDER BY added DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(StreamEntry {
                url: r.get(0)?,
                title: r.get(1)?,
            })
        })?;
        rows.collect()
    }

    pub fn remove_stream(&self, url: &str) -> Result<(), rusqlite::Error> {
        self.conn
            .execute("DELETE FROM streams WHERE url=?1", params![url])?;
        Ok(())
    }

    /// Saved playlist summaries for the library's Playlists section.
    pub fn playlists(&self) -> Result<Vec<(i64, String, usize)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.name, COUNT(i.path) FROM playlists p
             LEFT JOIN playlist_items i ON i.playlist_id=p.id
             GROUP BY p.id ORDER BY p.created DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_round_trip() {
        let db = LibraryStore::in_memory().unwrap();
        db.upsert_media(Path::new("ep.mkv"), "Episode", 120_000)
            .unwrap();
        let mut p = WatchProgress::new("ep.mkv", 30_000, 120_000);
        p.watched = false;
        db.save_progress(&p).unwrap();
        let loaded = db.progress(Path::new("ep.mkv")).unwrap().unwrap();
        assert_eq!(loaded.position_ms, 30_000);
        assert_eq!(loaded.duration_ms, 120_000);
        assert!(!loaded.watched);
        assert_eq!(db.progress(Path::new("missing")).unwrap(), None);
    }

    #[test]
    fn upsert_does_not_erase_known_duration_or_position() {
        let db = LibraryStore::in_memory().unwrap();
        db.upsert_media(Path::new("ep.mkv"), "Episode", 120_000)
            .unwrap();
        let p = WatchProgress::new("ep.mkv", 30_000, 120_000);
        db.save_progress(&p).unwrap();

        // A fresh open often does not know the duration yet; pass 0 and keep the old value.
        db.upsert_media(Path::new("ep.mkv"), "Episode", 0).unwrap();
        let loaded = db.progress(Path::new("ep.mkv")).unwrap().unwrap();
        assert_eq!(loaded.position_ms, 30_000);
        assert_eq!(loaded.duration_ms, 120_000);
        assert!(crate::should_resume(&loaded, 30_000));
    }

    #[test]
    fn playlist_crud() {
        let db = LibraryStore::in_memory().unwrap();
        let id = db.create_playlist("Favorites").unwrap();
        db.add_to_playlist(id, Path::new("a.mkv")).unwrap();
        db.add_to_playlist(id, Path::new("b.mkv")).unwrap();
        db.add_to_playlist(id, Path::new("a.mkv")).unwrap();
        assert_eq!(db.playlist_paths(id).unwrap().len(), 2);
        db.rename_playlist(id, "Best").unwrap();
        db.delete_playlist(id).unwrap();
        assert!(db.playlist_paths(id).unwrap().is_empty());
    }

    #[test]
    fn queue_and_bookmarks_persist() {
        let db = LibraryStore::in_memory().unwrap();
        db.save_queue(&[PathBuf::from("a.mkv"), PathBuf::from("b.mkv")])
            .unwrap();
        assert_eq!(db.load_queue().unwrap().len(), 2);
        db.add_bookmark(Path::new("a.mkv"), 12_000, "intro")
            .unwrap();
        assert_eq!(db.bookmarks(Path::new("a.mkv")).unwrap()[0].0, 12_000);
    }

    #[test]
    fn presets_round_trip() {
        let db = LibraryStore::in_memory().unwrap();
        db.save_preset("Show A", "folder", "{\"delay\":120}")
            .unwrap();
        let all = db.presets("folder").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].1, "{\"delay\":120}");
    }

    #[test]
    fn library_entries_pinned_watched_and_streams() {
        let db = LibraryStore::in_memory().unwrap();
        let mut a = LibraryEntry::new("C:/Shows/S01/ep1.mkv").unwrap();
        a.added = 10;
        a.size_bytes = 1024;
        a.pinned = true;
        let mut b = LibraryEntry::new("C:/Shows/S01/ep2.mkv").unwrap();
        b.added = 20;
        b.last_played = 200;
        db.upsert_entry(&a).unwrap();
        db.upsert_entry(&b).unwrap();

        assert_eq!(db.all_entries().unwrap().len(), 2);
        assert_eq!(db.pinned_entries().unwrap().len(), 1);
        assert_eq!(
            db.entries_in_folder(Path::new("C:/Shows/S01"))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            db.entries_in_folder(Path::new("C:/Shows/S02"))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(db.recent_entries(1).unwrap()[0].title, "ep2");

        db.set_watched(Path::new("C:/Shows/S01/ep1.mkv"), true)
            .unwrap();
        assert!(db
            .all_entries()
            .unwrap()
            .iter()
            .any(|e| e.path.ends_with("ep1.mkv") && e.watched));
        db.set_pinned(Path::new("C:/Shows/S01/ep2.mkv"), true)
            .unwrap();
        assert_eq!(db.pinned_entries().unwrap().len(), 2);

        let stream = StreamEntry {
            url: "https://example.test/live".into(),
            title: "Live".into(),
        };
        db.add_stream(&stream).unwrap();
        assert_eq!(db.streams().unwrap(), vec![stream.clone()]);
        db.remove_stream(&stream.url).unwrap();
        assert!(db.streams().unwrap().is_empty());

        let id = db.create_playlist("Favorites").unwrap();
        db.add_to_playlist(id, &a.path).unwrap();
        assert_eq!(db.playlists().unwrap()[0].2, 1);
    }

    #[test]
    fn schema_v2_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("vfm-schema-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lib.sqlite");
        {
            let db = LibraryStore::open(&path).unwrap();
            let mut e = LibraryEntry::new("a.mkv").unwrap();
            e.size_bytes = 42;
            db.upsert_entry(&e).unwrap();
        }
        {
            let db = LibraryStore::open(&path).unwrap();
            assert_eq!(db.all_entries().unwrap()[0].size_bytes, 42);
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
