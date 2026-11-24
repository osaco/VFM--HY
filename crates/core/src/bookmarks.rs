//! Bookmarks: named points inside a media file (Ctrl+B, timeline list, seek-bar markers).
//!
//! Bookmarks live in the same SQLite file as the library, keyed by media path so they
//! travel with the file rather than with a queue slot. The table already existed from
//! Phase 0; this module adds the domain model, de-duplication and ordering rules.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::store::LibraryStore;
use crate::time::format_timecode;

/// Two bookmarks closer than this are treated as the same spot: pressing Ctrl+B twice
/// on the same moment should update one bookmark, not create a pair.
pub const DEDUPE_TOLERANCE_MS: i64 = 1_500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub id: i64,
    pub media: PathBuf,
    pub time_ms: i64,
    pub name: String,
}

impl Bookmark {
    /// Label shown on the seek bar and in the list when the user did not type one.
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            format_timecode(self.time_ms)
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Error)]
pub enum BookmarkError {
    #[error("bookmark database error: {0}")]
    Db(#[from] rusqlite::Error),
}

/// Add a bookmark, replacing the label of a bookmark already sitting at that moment.
///
/// Returns the stored bookmark. An empty `name` falls back to the timecode, so the list
/// never shows blank rows.
pub fn add(
    db: &LibraryStore,
    media: &Path,
    time_ms: i64,
    name: &str,
) -> Result<Bookmark, BookmarkError> {
    let time_ms = time_ms.max(0);
    let name = if name.trim().is_empty() {
        format_timecode(time_ms)
    } else {
        name.trim().to_string()
    };

    if let Some(existing) = list(db, media)?
        .into_iter()
        .find(|item| (item.time_ms - time_ms).abs() <= DEDUPE_TOLERANCE_MS)
    {
        db.rename_bookmark(existing.id, &name)?;
        db.move_bookmark(existing.id, time_ms)?;
        return Ok(Bookmark {
            name,
            time_ms,
            ..existing
        });
    }

    let id = db.add_bookmark(media, time_ms, &name)?;
    Ok(Bookmark {
        id,
        media: media.to_path_buf(),
        time_ms,
        name,
    })
}

/// All bookmarks for a media file, ordered by time.
pub fn list(db: &LibraryStore, media: &Path) -> Result<Vec<Bookmark>, BookmarkError> {
    let rows = db.bookmarks_with_id(media)?;
    Ok(rows
        .into_iter()
        .map(|(id, time_ms, name)| Bookmark {
            id,
            media: media.to_path_buf(),
            time_ms,
            name,
        })
        .collect())
}

pub fn rename(db: &LibraryStore, id: i64, name: &str) -> Result<(), BookmarkError> {
    db.rename_bookmark(id, name.trim())?;
    Ok(())
}

pub fn remove(db: &LibraryStore, id: i64) -> Result<(), BookmarkError> {
    db.delete_bookmark(id)?;
    Ok(())
}

/// The bookmark a "jump to previous/next bookmark" command should land on.
///
/// `forward` picks the first bookmark strictly after `time_ms`; otherwise the last one
/// strictly before it. Returns `None` at the ends so the caller can leave the position
/// alone instead of jumping to an arbitrary spot.
pub fn neighbour(items: &[Bookmark], time_ms: i64, forward: bool) -> Option<&Bookmark> {
    if forward {
        items.iter().find(|item| item.time_ms > time_ms)
    } else {
        items.iter().rev().find(|item| item.time_ms < time_ms)
    }
}

/// Bookmark closest to `time_ms`, used to highlight the active row in the list.
pub fn closest(items: &[Bookmark], time_ms: i64) -> Option<&Bookmark> {
    items
        .iter()
        .min_by_key(|item| (item.time_ms - time_ms).abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media() -> PathBuf {
        PathBuf::from("D:/Shows/Solaris/S01E01.mkv")
    }

    #[test]
    fn bookmarks_round_trip_in_time_order() {
        let db = LibraryStore::in_memory().unwrap();
        add(&db, &media(), 60_000, "second").unwrap();
        add(&db, &media(), 5_000, "first").unwrap();

        let items = list(&db, &media()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].time_ms, 5_000);
        assert_eq!(items[0].name, "first");
        assert_eq!(items[1].time_ms, 60_000);
    }

    #[test]
    fn an_empty_label_falls_back_to_the_timecode() {
        let db = LibraryStore::in_memory().unwrap();
        let saved = add(&db, &media(), 125_000, "   ").unwrap();
        assert_eq!(saved.name, "2:05");
    }

    #[test]
    fn re_marking_the_same_spot_updates_instead_of_duplicating() {
        let db = LibraryStore::in_memory().unwrap();
        let first = add(&db, &media(), 30_000, "intro").unwrap();
        let second = add(&db, &media(), 30_800, "opening titles").unwrap();

        assert_eq!(first.id, second.id, "same spot must reuse the row");
        assert_eq!(second.name, "opening titles");
        assert_eq!(list(&db, &media()).unwrap().len(), 1);
    }

    #[test]
    fn a_spot_far_away_creates_a_second_bookmark() {
        let db = LibraryStore::in_memory().unwrap();
        add(&db, &media(), 30_000, "intro").unwrap();
        add(&db, &media(), 90_000, "after the intro").unwrap();
        assert_eq!(list(&db, &media()).unwrap().len(), 2);
    }

    #[test]
    fn rename_and_delete_work_by_id() {
        let db = LibraryStore::in_memory().unwrap();
        let saved = add(&db, &media(), 10_000, "a").unwrap();
        rename(&db, saved.id, "renamed").unwrap();
        assert_eq!(list(&db, &media()).unwrap()[0].name, "renamed");

        remove(&db, saved.id).unwrap();
        assert!(list(&db, &media()).unwrap().is_empty());
    }

    #[test]
    fn neighbour_walks_forwards_and_backwards_and_stops_at_the_ends() {
        let db = LibraryStore::in_memory().unwrap();
        add(&db, &media(), 10_000, "a").unwrap();
        add(&db, &media(), 20_000, "b").unwrap();
        add(&db, &media(), 30_000, "c").unwrap();
        let items = list(&db, &media()).unwrap();

        assert_eq!(neighbour(&items, 0, true).map(|b| b.time_ms), Some(10_000));
        assert_eq!(
            neighbour(&items, 10_000, true).map(|b| b.time_ms),
            Some(20_000)
        );
        assert_eq!(neighbour(&items, 30_000, true), None);
        assert_eq!(
            neighbour(&items, 30_000, false).map(|b| b.time_ms),
            Some(20_000)
        );
        assert_eq!(neighbour(&items, 10_000, false), None);
    }

    #[test]
    fn bookmarks_are_scoped_to_their_media_file() {
        let db = LibraryStore::in_memory().unwrap();
        add(&db, &media(), 10_000, "a").unwrap();
        let other = PathBuf::from("D:/Shows/Solaris/S01E02.mkv");
        assert!(list(&db, &other).unwrap().is_empty());
    }

    #[test]
    fn closest_picks_the_nearer_bookmark() {
        let db = LibraryStore::in_memory().unwrap();
        add(&db, &media(), 10_000, "a").unwrap();
        add(&db, &media(), 100_000, "b").unwrap();
        let items = list(&db, &media()).unwrap();
        assert_eq!(
            closest(&items, 12_000).map(|b| b.name.clone()),
            Some("a".into())
        );
        assert_eq!(
            closest(&items, 90_000).map(|b| b.name.clone()),
            Some("b".into())
        );
    }
}
