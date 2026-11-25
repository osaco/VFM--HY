//! Phase 5 glue: external subtitle discovery/loading, track lists and the live subtitle
//! session. libVLC's own SPU rendering is disabled so our Slint overlay is the only renderer.

use slint::{SharedString, VecModel};
use std::path::{Path, PathBuf};
use vfm_core::{SubtitleFormat, SubtitleSession};

use crate::PlayerWindow;

/// Runtime subtitle state shared with the UI ticker.
#[derive(Default)]
pub struct SubtitleController {
    pub session: SubtitleSession,
    pub source: Option<PathBuf>,
}

impl SubtitleController {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)] // used by the UI when external subtitles are unloaded
    pub fn clear(&mut self) {
        self.session = SubtitleSession::default();
        self.source = None;
    }

    /// Load an external subtitle file with encoding detection and format dispatch.
    pub fn load(&mut self, path: &Path) -> Result<usize, String> {
        let format = SubtitleFormat::from_path(path)
            .ok_or_else(|| format!("unsupported subtitle extension: {}", path.display()))?;
        let bytes = std::fs::read(path).map_err(|e| format!("read subtitle: {e}"))?;
        let session = SubtitleSession::from_bytes(&bytes, format);
        let count = session.len();
        self.session = session;
        self.source = Some(path.to_path_buf());
        Ok(count)
    }
}

/// Find external subtitle files next to a media file. Returns matches sorted by name;
/// `subtitle_matches_video` decides whether the stem matches.
pub fn discover_subtitles(media: &Path) -> Vec<PathBuf> {
    let Some(parent) = media.parent() else {
        return Vec::new();
    };
    let Ok(read) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| SubtitleFormat::from_path(p).is_some())
        .filter(|p| vfm_core::subtitle_matches_video(media, p))
        .collect();
    found.sort();
    found
}

/// A popover-ready track list: display names, the row index of the active track, and the
/// libVLC id behind each row so a click can select the real track rather than a row number.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackList {
    pub names: Vec<SharedString>,
    pub current_index: i32,
    pub ids: Vec<i32>,
}

impl Default for TrackList {
    fn default() -> Self {
        Self {
            names: Vec::new(),
            // -1 means "nothing selected"; 0 would point at a row that does not exist.
            current_index: -1,
            ids: Vec::new(),
        }
    }
}

/// Build the popover list from libVLC's own track descriptions.
///
/// The disabled entry (`id == -1`) is dropped because the popover already has an explicit
/// "Off" row. A track whose demuxer supplied no name falls back to `"<prefix> N"` — that is a
/// label for a real track, not a fabricated entry: the list is exactly as long as libVLC says.
pub fn live_tracks(tracks: &[vlc_wrapper::LiveTrack], prefix: &str, current: i32) -> TrackList {
    let mut out = TrackList::default();
    for track in tracks {
        if track.id < 0 {
            continue;
        }
        if track.id == current {
            out.current_index = out.names.len() as i32;
        }
        let label = if track.name.trim().is_empty() {
            format!("{prefix} {}", out.names.len() + 1)
        } else {
            track.name.clone()
        };
        out.names.push(SharedString::from(label));
        out.ids.push(track.id);
    }
    out
}

/// Resolve a popover row index to the libVLC track id it represents. A negative row means
/// the explicit "Off" row and maps to `-1` (libVLC's disabled track id).
pub fn track_id_at(list: &TrackList, row: i32) -> Option<i32> {
    if row < 0 {
        return Some(-1);
    }
    list.ids.get(row as usize).copied()
}

pub fn push_track_names(ui: &PlayerWindow, audio: &TrackList, subtitles: &TrackList) {
    let a = VecModel::<SharedString>::default();
    for n in &audio.names {
        a.push(n.clone());
    }
    ui.set_audio_track_names(slint::ModelRc::new(a));
    ui.set_audio_track_current(audio.current_index);

    let s = VecModel::<SharedString>::default();
    for n in &subtitles.names {
        s.push(n.clone());
    }
    ui.set_subtitle_track_names(slint::ModelRc::new(s));
    ui.set_subtitle_track_current(subtitles.current_index);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_srt_and_counts_cues() {
        let dir = std::env::temp_dir().join(format!("vfm-sub-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let srt = dir.join("test.srt");
        std::fs::write(&srt, "1\n00:00:01,000 --> 00:00:03,000\nHi\n").unwrap();
        let mut c = SubtitleController::new();
        assert_eq!(c.load(&srt).unwrap(), 1);
        assert!(c.source.is_some());
        c.clear();
        assert!(c.session.is_empty());
        assert!(c.source.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_unknown_extension() {
        let mut c = SubtitleController::new();
        assert!(c.load(Path::new("notes.txt")).is_err());
    }

    #[test]
    fn discovers_matching_subtitles_only() {
        let dir = std::env::temp_dir().join(format!("vfm-sub-disc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let media = dir.join("Ep.mkv");
        std::fs::write(&media, b"x").unwrap();
        std::fs::write(dir.join("Ep.srt"), b"x").unwrap();
        std::fs::write(dir.join("Ep.ar.vtt"), b"x").unwrap();
        std::fs::write(dir.join("Other.srt"), b"x").unwrap();
        let found = discover_subtitles(&media);
        assert_eq!(found.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn live_tracks_use_engine_names_and_skip_the_disabled_entry() {
        let tracks = vec![
            vlc_wrapper::LiveTrack {
                id: -1,
                name: "Disable".into(),
            },
            vlc_wrapper::LiveTrack {
                id: 2,
                name: "English".into(),
            },
            vlc_wrapper::LiveTrack {
                id: 7,
                name: String::new(),
            },
        ];
        let list = live_tracks(&tracks, "Audio", 2);
        assert_eq!(list.names.len(), 2);
        assert_eq!(list.names[0], "English");
        // A nameless real track still gets a positional label, not a fake extra row.
        assert_eq!(list.names[1], "Audio 2");
        assert_eq!(list.ids, vec![2, 7]);
        assert_eq!(list.current_index, 0);
    }

    #[test]
    fn track_id_at_maps_rows_and_off() {
        let list = TrackList {
            names: vec!["A".into()],
            current_index: 0,
            ids: vec![5],
        };
        assert_eq!(track_id_at(&list, 0), Some(5));
        assert_eq!(track_id_at(&list, -1), Some(-1));
        assert_eq!(track_id_at(&list, 3), None);
    }

    #[test]
    fn an_empty_engine_list_produces_no_rows() {
        let list = live_tracks(&[], "Sub", -1);
        assert!(list.names.is_empty());
        assert!(list.ids.is_empty());
        assert_eq!(list.current_index, -1);
    }
}
