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

/// Build human-readable track names for the popovers.
/// `count` is the libVLC track count (including the disabled "Track ID -1" entry, which we
/// filter out); `current` is the active track id.
pub fn track_names(prefix: &str, count: i32, current: i32) -> (Vec<SharedString>, i32) {
    let mut names = Vec::new();
    let mut current_index = -1i32;
    for id in 0..count.max(0) {
        // libVLC track ids are usually 0..count-1; skip the special -1 disabled id.
        if id < 0 {
            continue;
        }
        if id == current {
            current_index = names.len() as i32;
        }
        names.push(SharedString::from(format!("{prefix} {}", id + 1).as_str()));
    }
    (names, current_index)
}

pub fn push_track_names(
    ui: &PlayerWindow,
    audio: (Vec<SharedString>, i32),
    subtitles: (Vec<SharedString>, i32),
) {
    let a = VecModel::<SharedString>::default();
    for n in audio.0 {
        a.push(n);
    }
    ui.set_audio_track_names(slint::ModelRc::new(a));
    ui.set_audio_track_current(audio.1);

    let s = VecModel::<SharedString>::default();
    for n in subtitles.0 {
        s.push(n);
    }
    ui.set_subtitle_track_names(slint::ModelRc::new(s));
    ui.set_subtitle_track_current(subtitles.1);
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
    fn track_names_skip_disabled_and_mark_current() {
        let (names, current) = track_names("Audio", 3, 2);
        assert_eq!(names.len(), 3);
        assert_eq!(current, 2);
        assert_eq!(names[0], "Audio 1");
        let (_, none) = track_names("Sub", 0, -1);
        assert_eq!(none, -1);
    }
}
