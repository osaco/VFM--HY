//! State and rules for the Open-media dialog.
//!
//! The window is a thin shell: every decision (is this a path or a URL, which sout chain,
//! is the port valid) happens here so it can be unit-tested without a player or a window.

use std::path::PathBuf;

use slint::{ModelRc, SharedString, VecModel};
use vfm_core::open_media::{self, CaptureSpec, Target};
use vfm_core::Language;

use crate::OpenMediaWindow;

pub const MODE_FILE: i32 = 0;
pub const MODE_FOLDER: i32 = 1;
pub const MODE_NETWORK: i32 = 2;
pub const MODE_CAPTURE: i32 = 3;
pub const MODE_CLIPBOARD: i32 = 4;
pub const MODE_RECENT: i32 = 5;
pub const MODE_CONVERT: i32 = 6;
pub const MODE_STREAM: i32 = 7;

/// Why an action could not start, as a localisable key suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    BadPath,
    BadUrl,
    BadPort,
    NoSource,
}

impl Failure {
    pub fn key(self) -> &'static str {
        match self {
            Self::BadPath => "open.status.bad-path",
            Self::BadUrl => "open.status.bad-url",
            Self::BadPort => "open.status.bad-port",
            Self::NoSource => "open.status.no-source",
        }
    }
}

/// Editable state of the dialog.
#[derive(Debug, Clone)]
pub struct OpenDialogState {
    pub mode: i32,
    pub path_text: String,
    pub url_text: String,
    pub clipboard_text: String,
    pub capture_video: String,
    pub capture_audio: String,
    pub capture_size: String,
    pub capture_fps: String,
    pub destination: String,
    pub preset_index: i32,
    pub stream_port: String,
    pub stream_path: String,
    /// File the convert/stream panels act on (the media currently playing).
    pub source: Option<PathBuf>,
}

impl Default for OpenDialogState {
    fn default() -> Self {
        Self {
            mode: MODE_FILE,
            path_text: String::new(),
            url_text: String::new(),
            clipboard_text: String::new(),
            capture_video: String::new(),
            capture_audio: String::new(),
            capture_size: "1280x720".to_string(),
            capture_fps: "30".to_string(),
            destination: String::new(),
            preset_index: 0,
            stream_port: "8080".to_string(),
            stream_path: "stream".to_string(),
            source: None,
        }
    }
}

impl OpenDialogState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Switch panels, carrying the current media into the convert/stream panels.
    pub fn set_mode(&mut self, mode: i32) {
        self.mode = mode.clamp(0, MODE_STREAM);
        if self.mode == MODE_CONVERT && self.destination.is_empty() {
            if let Some(source) = &self.source {
                let preset = open_media::CONVERT_PRESETS
                    .get(self.preset_index.max(0) as usize)
                    .copied()
                    .unwrap_or(open_media::CONVERT_PRESETS[0]);
                self.destination = open_media::default_destination(source, preset.extension)
                    .display()
                    .to_string();
            }
        }
        if self.mode != MODE_CONVERT {
            self.preset_index = 0;
        }
    }

    /// Re-derive the destination when the preset changes (extension follows the container).
    pub fn set_preset(&mut self, index: i32) {
        let presets: &[open_media::ConvertPreset] = if self.mode == MODE_STREAM {
            &[]
        } else {
            open_media::CONVERT_PRESETS
        };
        if self.mode == MODE_STREAM {
            let count = open_media::STREAM_PRESETS.len() as i32;
            self.preset_index = index.clamp(0, count - 1);
            return;
        }
        if presets.is_empty() {
            return;
        }
        self.preset_index = index.clamp(0, presets.len() as i32 - 1);
        if let (Some(source), Some(preset)) = (&self.source, presets.get(index as usize)) {
            self.destination = open_media::default_destination(source, preset.extension)
                .display()
                .to_string();
        }
    }

    /// What the Play button should open, for the source panels.
    pub fn target(&self) -> Result<Target, Failure> {
        match self.mode {
            MODE_FILE => {
                let target = open_media::classify(&self.path_text).ok_or(Failure::BadPath)?;
                match &target {
                    Target::File(path) if !path.exists() => Err(Failure::BadPath),
                    _ => Ok(target),
                }
            }
            MODE_FOLDER => {
                let path = PathBuf::from(self.path_text.trim());
                if path.is_dir() {
                    Ok(Target::File(path))
                } else {
                    Err(Failure::BadPath)
                }
            }
            MODE_NETWORK => open_media::classify(&self.url_text).ok_or(Failure::BadUrl),
            MODE_CAPTURE => {
                let (location, _) = self.capture_spec().to_location_and_options();
                Ok(Target::Location(location))
            }
            MODE_CLIPBOARD => open_media::classify(&self.clipboard_text).ok_or(Failure::BadUrl),
            // Recent rows are picked individually, so there is no single target here.
            MODE_RECENT => Err(Failure::NoSource),
            _ => Err(Failure::NoSource),
        }
    }

    /// Options that go with the capture location (`dshow-vdev=…` and friends).
    pub fn capture_spec(&self) -> CaptureSpec {
        let (width, height) = parse_size(&self.capture_size);
        CaptureSpec {
            video_device: self.capture_video.trim().to_string(),
            audio_device: self.capture_audio.trim().to_string(),
            width,
            height,
            fps: self.capture_fps.trim().parse::<u32>().unwrap_or(0),
        }
    }

    /// `(destination, sout chain)` for the Convert panel.
    pub fn convert_plan(&self) -> Result<(PathBuf, String), Failure> {
        let source = self.source.as_ref().ok_or(Failure::NoSource)?;
        let destination = PathBuf::from(self.destination.trim());
        if destination.as_os_str().is_empty() {
            return Err(Failure::BadPath);
        }
        let preset = open_media::CONVERT_PRESETS
            .get(self.preset_index.max(0) as usize)
            .copied()
            .unwrap_or(open_media::CONVERT_PRESETS[0]);
        let mut destination = destination;
        if destination.extension().is_none() {
            destination.set_extension(preset.extension);
        }
        // Converting onto the source would truncate the file libVLC is reading.
        if destination == *source {
            return Err(Failure::BadPath);
        }
        Ok((
            destination.clone(),
            open_media::convert_sout(&preset, &destination),
        ))
    }

    /// `(port, sout chain, viewer URL)` for the Stream panel.
    pub fn stream_plan(&self) -> Result<(u16, String, String), Failure> {
        let port = self
            .stream_port
            .trim()
            .parse::<u16>()
            .map_err(|_| Failure::BadPort)?;
        if port < 1024 {
            return Err(Failure::BadPort);
        }
        let preset = open_media::STREAM_PRESETS
            .get(self.preset_index.max(0) as usize)
            .copied()
            .unwrap_or(open_media::STREAM_PRESETS[0]);
        Ok((
            port,
            open_media::stream_sout(&preset, port, &self.stream_path),
            open_media::stream_url(&preset, port, &self.stream_path),
        ))
    }

    /// Labels for the preset chips, per panel.
    pub fn preset_labels(&self) -> Vec<&'static str> {
        if self.mode == MODE_STREAM {
            vec!["HTTP TS", "HTTP PS", "RTSP"]
        } else {
            vec!["MP4", "WebM", "MKV", "MP3", "M4A"]
        }
    }

    /// Action button text key for the active panel.
    pub fn action_key(&self) -> &'static str {
        match self.mode {
            MODE_CONVERT => "open.action.convert",
            MODE_STREAM => "open.action.stream",
            _ => "open.action.play",
        }
    }
}

fn parse_size(text: &str) -> (u32, u32) {
    let mut parts = text
        .trim()
        .split(['x', 'X', '*', ' '])
        .filter(|p| !p.is_empty());
    let width = parts
        .next()
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    let height = parts
        .next()
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    (width, height)
}

/// Publish the capture devices Windows actually reported into the capture panel.
///
/// Empty lists stay empty on purpose: the panel hides the picker and the typed fields remain
/// the fallback. No placeholder name is ever invented.
pub fn push_devices(win: &OpenMediaWindow, devices: &vfm_platform::CaptureDevices) {
    let to_model = |names: &[String]| -> ModelRc<SharedString> {
        ModelRc::from(std::rc::Rc::new(VecModel::from(
            names
                .iter()
                .cloned()
                .map(SharedString::from)
                .collect::<Vec<_>>(),
        )))
    };
    win.set_capture_video_devices(to_model(&devices.video));
    win.set_capture_audio_devices(to_model(&devices.audio));
}

/// Push the whole state into the window.
pub fn push(win: &OpenMediaWindow, state: &OpenDialogState, language: Language) {
    use crate::i18n;

    let modes: Vec<SharedString> = [
        "open.mode.file",
        "open.mode.folder",
        "open.mode.network",
        "open.mode.capture",
        "open.mode.clipboard",
        "open.mode.recent",
        "open.mode.convert",
        "open.mode.stream",
    ]
    .iter()
    .map(|key| SharedString::from(i18n::translate(language, key)))
    .collect();
    win.set_modes(ModelRc::from(std::rc::Rc::new(VecModel::from(modes))));
    win.set_preset_labels(ModelRc::from(std::rc::Rc::new(VecModel::from(
        state
            .preset_labels()
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    ))));

    win.set_mode(state.mode);
    win.set_path_text(state.path_text.clone().into());
    win.set_url_text(state.url_text.clone().into());
    win.set_clipboard_text(state.clipboard_text.clone().into());
    win.set_capture_video(state.capture_video.clone().into());
    win.set_capture_audio(state.capture_audio.clone().into());
    win.set_capture_size(state.capture_size.clone().into());
    win.set_capture_fps(state.capture_fps.clone().into());
    win.set_destination_text(state.destination.clone().into());
    win.set_stream_port(state.stream_port.clone().into());
    win.set_stream_path(state.stream_path.clone().into());
    win.set_preset_index(state.preset_index);
    win.set_browse_label(i18n::translate(language, "open.browse").into());
    win.set_cancel_label(i18n::translate(language, "open.cancel").into());
    win.set_action_label(i18n::translate(language, state.action_key()).into());
    win.set_recent_empty(i18n::translate(language, "open.recent.empty").into());
    // A short sample path reads better in the field than the full hint, which is shown
    // underneath the field anyway.
    win.set_path_hint(
        if state.mode == MODE_FOLDER {
            "D:/media"
        } else {
            "C:/media/movie.mkv"
        }
        .into(),
    );
    if state.mode == MODE_STREAM {
        let url = state
            .stream_plan()
            .map(|(_, _, url)| url)
            .unwrap_or_else(|_| {
                open_media::stream_url(&open_media::STREAM_PRESETS[0], 8080, &state.stream_path)
            });
        win.set_stream_url(url.into());
    }
}

/// Fill the Recent panel from the library's most-recently-played rows.
pub fn push_recent(win: &OpenMediaWindow, entries: &[vfm_core::LibraryEntry]) {
    let rows: Vec<crate::RecentRow> = entries
        .iter()
        .map(|entry| crate::RecentRow {
            title: entry.title.clone().into(),
            detail: format!("{} · {}", entry.path.display(), entry.size_label()).into(),
            path: entry.path.display().to_string().into(),
        })
        .collect();
    win.set_recent(ModelRc::from(std::rc::Rc::new(VecModel::from(rows))));
}

/// Set a status line from an i18n key.
pub fn status(win: &OpenMediaWindow, language: Language, key: &str, error: bool) {
    win.set_status(crate::i18n::translate(language, key).into());
    win.set_status_error(error);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_mode_rejects_missing_paths() {
        let mut state = OpenDialogState::new();
        state.path_text = r"D:\definitely\not\here.mkv".into();
        assert_eq!(state.target(), Err(Failure::BadPath));
    }

    #[test]
    fn network_mode_accepts_bare_hosts() {
        let mut state = OpenDialogState::new();
        state.mode = MODE_NETWORK;
        state.url_text = "example.com/live.m3u8".into();
        assert_eq!(
            state.target(),
            Ok(Target::Location("http://example.com/live.m3u8".into()))
        );
    }

    #[test]
    fn network_mode_rejects_empty_input() {
        let mut state = OpenDialogState::new();
        state.mode = MODE_NETWORK;
        assert_eq!(state.target(), Err(Failure::BadUrl));
    }

    #[test]
    fn capture_mode_builds_a_dshow_location() {
        let mut state = OpenDialogState::new();
        state.mode = MODE_CAPTURE;
        state.capture_video = "Integrated Camera".into();
        state.capture_size = "1920x1080".into();
        state.capture_fps = "25".into();
        assert_eq!(state.target(), Ok(Target::Location("dshow://".into())));
        let spec = state.capture_spec();
        assert_eq!(spec.width, 1920);
        assert_eq!(spec.height, 1080);
        assert_eq!(spec.fps, 25);
    }

    #[test]
    fn convert_needs_a_source() {
        let state = OpenDialogState::new();
        assert_eq!(state.convert_plan(), Err(Failure::NoSource));
    }

    #[test]
    fn convert_refuses_to_overwrite_the_source() {
        let mut state = OpenDialogState::new();
        state.mode = MODE_CONVERT;
        state.source = Some(PathBuf::from(r"D:\media\clip.mkv"));
        state.destination = r"D:\media\clip.mkv".into();
        assert_eq!(state.convert_plan(), Err(Failure::BadPath));
    }

    #[test]
    fn convert_fills_the_extension_from_the_preset() {
        let mut state = OpenDialogState::new();
        state.mode = MODE_CONVERT;
        state.source = Some(PathBuf::from(r"D:\media\clip.mkv"));
        state.destination = r"D:\media\out".into();
        let (path, sout) = state.convert_plan().unwrap();
        assert_eq!(path, PathBuf::from(r"D:\media\out.mp4"));
        assert!(sout.contains("mux=mp4"));
    }

    #[test]
    fn changing_preset_retargets_the_destination() {
        let mut state = OpenDialogState::new();
        state.mode = MODE_CONVERT;
        state.source = Some(PathBuf::from(r"D:\media\clip.mkv"));
        state.set_preset(3); // MP3
        assert!(state.destination.ends_with("clip (converted).mp3"));
    }

    #[test]
    fn stream_rejects_privileged_ports() {
        let mut state = OpenDialogState::new();
        state.mode = MODE_STREAM;
        state.stream_port = "80".into();
        assert_eq!(state.stream_plan(), Err(Failure::BadPort));
        state.stream_port = "not-a-port".into();
        assert_eq!(state.stream_plan(), Err(Failure::BadPort));
    }

    #[test]
    fn stream_plan_carries_the_viewer_url() {
        let mut state = OpenDialogState::new();
        state.mode = MODE_STREAM;
        state.stream_port = "9090".into();
        state.stream_path = "live".into();
        let (port, sout, url) = state.stream_plan().unwrap();
        assert_eq!(port, 9090);
        assert!(sout.contains("dst=:9090/live"));
        assert_eq!(url, "http://<this-pc>:9090/live");
    }

    #[test]
    fn action_label_follows_the_panel() {
        let mut state = OpenDialogState::new();
        assert_eq!(state.action_key(), "open.action.play");
        state.mode = MODE_CONVERT;
        assert_eq!(state.action_key(), "open.action.convert");
        state.mode = MODE_STREAM;
        assert_eq!(state.action_key(), "open.action.stream");
    }

    #[test]
    fn size_parsing_tolerates_separators() {
        assert_eq!(parse_size("1920x1080"), (1920, 1080));
        assert_eq!(parse_size("640 X 480"), (640, 480));
        assert_eq!(parse_size("junk"), (0, 0));
    }
}
