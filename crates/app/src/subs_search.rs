//! Phase 7: OpenSubtitles search-dialog glue.
//!
//! Network calls run on a worker thread and push plain rows back to the UI, so the dialog
//! never blocks the player and the UI thread never touches the HTTP client.

use slint::{ModelRc, SharedString, VecModel, Weak};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use vfm_core::{OpenSubtitlesClient, SubtitleApiError, SubtitleHit};

use crate::subtitles_ui::SubtitleController;
use crate::{i18n, PlayerWindow, SubtitleHitRow};

fn s(text: impl Into<String>) -> SharedString {
    SharedString::from(text.into())
}

/// Turn search hits into the two-line rows the dialog renders.
pub fn rows_for(hits: &[SubtitleHit]) -> Vec<SubtitleHitRow> {
    hits.iter()
        .map(|hit| {
            let title = if hit.release.trim().is_empty() {
                hit.file_name.clone()
            } else {
                hit.release.clone()
            };
            let mut meta = format!("{} · {}", hit.language.to_uppercase(), hit.format);
            if hit.hearing_impaired {
                meta.push_str(" · HI");
            }
            meta.push_str(&format!(" · {} ↓", hit.downloads));
            SubtitleHitRow {
                title: s(title),
                meta: s(meta),
            }
        })
        .collect()
}

pub fn push_rows(ui: &PlayerWindow, hits: &[SubtitleHit]) {
    ui.set_subs_rows(ModelRc::new(VecModel::from(rows_for(hits))));
}

fn set_status(ui: &Weak<PlayerWindow>, text: String) {
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(u) = ui.upgrade() {
            u.set_subs_status(text.into());
            u.set_subs_busy(false);
        }
    });
}

/// Search by file hash first (exact match), falling back to the typed query. An empty
/// query means "this file", which is the common case.
pub fn search(
    ui: Weak<PlayerWindow>,
    key: String,
    media: PathBuf,
    query: String,
    hits_store: Arc<Mutex<Vec<SubtitleHit>>>,
) {
    let language = i18n::ui_language();
    let empty = i18n::translate(language, "opensubtitles.empty").to_string();
    let results = i18n::translate(language, "opensubtitles.results").to_string();
    let _ = std::thread::Builder::new()
        .name("vfm-opensubs".into())
        .spawn(move || {
            let client = OpenSubtitlesClient::new(key, vfm_core::OPENSUBTITLES_USER_AGENT);
            let outcome = if query.trim().is_empty() {
                vfm_core::movie_hash(&media)
                    .map_err(|error| SubtitleApiError::Network(error.to_string()))
                    .and_then(|(hash, size)| client.search_by_hash(&hash, size, &[]))
            } else {
                client.search_by_name(query.trim(), &[])
            };
            let (hits, status) = match outcome {
                Ok(found) if found.is_empty() => (Vec::new(), empty),
                Ok(found) => {
                    let status = format!("{results}: {}", found.len());
                    (found, status)
                }
                Err(error) => (Vec::new(), error.to_string()),
            };
            *hits_store.lock().unwrap() = hits.clone();
            let ui_push = ui.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = ui_push.upgrade() {
                    push_rows(&u, &hits);
                }
            });
            set_status(&ui, status);
        });
}

/// Strip any directory part from the server-supplied name so a download can never write
/// outside the subtitle folder.
pub fn safe_file_name(name: &str, file_id: i64) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if base.is_empty() {
        return format!("opensubtitles-{file_id}.srt");
    }
    if base.rsplit('.').next().map(str::len).unwrap_or(0) > 4 {
        format!("{base}.srt")
    } else {
        base
    }
}

/// Fetch the chosen subtitle, store it next to the other downloaded subtitles and load it
/// into the live session.
pub fn download(
    ui: Weak<PlayerWindow>,
    key: String,
    hit: SubtitleHit,
    dir: PathBuf,
    subtitles: Arc<Mutex<SubtitleController>>,
) {
    let _ = std::thread::Builder::new()
        .name("vfm-opensubs-dl".into())
        .spawn(move || {
            let client = OpenSubtitlesClient::new(key, vfm_core::OPENSUBTITLES_USER_AGENT);
            let outcome = client
                .download(hit.file_id)
                .and_then(|ticket| client.fetch(&ticket).map(|bytes| (ticket, bytes)));
            let status = match outcome {
                Ok((ticket, bytes)) => {
                    let path = dir.join(safe_file_name(&ticket.file_name, hit.file_id));
                    if let Err(error) = std::fs::create_dir_all(&dir) {
                        error.to_string()
                    } else if let Err(error) = std::fs::write(&path, &bytes) {
                        error.to_string()
                    } else {
                        match subtitles.lock().unwrap().load(&path) {
                            Ok(cues) => format!("{} · {cues}", ticket.file_name),
                            Err(error) => error,
                        }
                    }
                }
                Err(error) => error.to_string(),
            };
            set_status(&ui, status);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(release: &str, file_name: &str) -> SubtitleHit {
        SubtitleHit {
            file_id: 42,
            language: "en".into(),
            release: release.into(),
            downloads: 1200,
            rating: 4.5,
            hearing_impaired: true,
            format: "srt".into(),
            file_name: file_name.into(),
        }
    }

    #[test]
    fn rows_prefer_the_release_name_and_fall_back_to_the_file_name() {
        let rows = rows_for(&[
            hit("Solaris.S01E01.1080p", "x.srt"),
            hit("  ", "fallback.srt"),
        ]);
        assert_eq!(rows[0].title.as_str(), "Solaris.S01E01.1080p");
        assert_eq!(rows[1].title.as_str(), "fallback.srt");
    }

    #[test]
    fn the_meta_line_carries_language_format_and_downloads() {
        let rows = rows_for(&[hit("a", "a.srt")]);
        assert_eq!(rows[0].meta.as_str(), "EN · srt · HI · 1200 ↓");
    }

    #[test]
    fn a_plain_hit_has_no_hi_marker() {
        let mut plain = hit("a", "a.srt");
        plain.hearing_impaired = false;
        let rows = rows_for(&[plain]);
        assert!(!rows[0].meta.as_str().contains("HI"));
    }

    #[test]
    fn download_names_cannot_escape_the_target_folder() {
        assert_eq!(safe_file_name("../../evil.srt", 1), "evil.srt");
        assert_eq!(safe_file_name("C:\\Windows\\evil.srt", 1), "evil.srt");
        assert_eq!(safe_file_name("", 7), "opensubtitles-7.srt");
    }

    #[test]
    fn a_name_without_an_extension_gets_srt() {
        assert_eq!(safe_file_name("Solaris.S01E01", 3), "Solaris.S01E01.srt");
        assert_eq!(safe_file_name("Solaris.srt", 3), "Solaris.srt");
    }
}
