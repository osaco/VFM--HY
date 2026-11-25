//! Open-media plumbing: what a typed string means, and how to build the libVLC chains for
//! capture, conversion and streaming.
//!
//! Everything here is pure string/path work so the dialogs stay thin and the rules are
//! testable without a player.

use std::path::{Path, PathBuf};

use crate::library::MediaKind;

/// Files worth handing to the player from a drag & drop or an "open folder" request.
///
/// Anything that is not a media file is dropped rather than queued, and a dropped folder
/// contributes the media files directly inside it. Sorting keeps a multi-file drop in a
/// predictable order (explorer hands paths over in whatever order it likes).
pub fn collect_media(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        if path.is_dir() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|entry| entry.is_file() && MediaKind::from_path(entry).is_some())
                .collect();
            found.sort();
            out.extend(found);
        } else if MediaKind::from_path(&path).is_some() {
            out.push(path);
        }
    }
    out
}

/// Where a media item comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A local file or folder on disk.
    File(PathBuf),
    /// A network or capture location handed to `libvlc_media_new_location`.
    Location(String),
}

impl Target {
    /// Human-readable form used in status lines and error messages.
    pub fn display(&self) -> String {
        match self {
            Self::File(path) => path.display().to_string(),
            Self::Location(url) => url.clone(),
        }
    }

    /// Whether libVLC should be given this as a location rather than a path.
    pub fn is_location(&self) -> bool {
        matches!(self, Self::Location(_))
    }
}

/// Schemes that libVLC accepts as a location. `dshow://` and `screen://` are capture
/// sources, the rest are network protocols.
const SCHEMES: &[&str] = &[
    "http://",
    "https://",
    "rtsp://",
    "rtsps://",
    "rtp://",
    "udp://",
    "mms://",
    "mmsh://",
    "ftp://",
    "sftp://",
    "smb://",
    "file://",
    "dshow://",
    "screen://",
    "imem://",
];

/// Classify a typed or pasted string.
///
/// Returns `None` for an empty string. A string with a known scheme is a location; anything
/// else is treated as a path, with `~` expanded and Windows separators normalised.
pub fn classify(input: &str) -> Option<Target> {
    let text = input.trim().trim_matches('"');
    if text.is_empty() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if SCHEMES.iter().any(|scheme| lower.starts_with(scheme)) {
        return Some(Target::Location(text.to_string()));
    }
    // "C:\media\a.mkv" and "\\server\share\a.mkv" are paths even though they contain ':'.
    let looks_like_windows_path = text.len() > 1
        && (text.as_bytes()[1] == b':'
            || text.starts_with("\\\\")
            || text.starts_with("./")
            || text.starts_with(".\\")
            || text.starts_with('/'));
    if looks_like_windows_path {
        return Some(Target::File(PathBuf::from(expand_home(text))));
    }
    // A bare name that exists on disk wins over the "it must be a URL" guess.
    let expanded = expand_home(text);
    if Path::new(&expanded).exists() {
        return Some(Target::File(PathBuf::from(expanded)));
    }
    // "example.com/stream.m3u8" and "10.0.0.5:8080" are almost always network sources.
    if text.contains("://") || text.contains('/') || text.contains(':') {
        let prefixed = if text.contains("://") {
            text.to_string()
        } else {
            format!("http://{text}")
        };
        return Some(Target::Location(prefixed));
    }
    Some(Target::File(PathBuf::from(expanded)))
}

fn expand_home(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            return format!("{home}\\{rest}");
        }
    }
    text.to_string()
}

/// A DirectShow capture source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSpec {
    pub video_device: String,
    pub audio_device: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for CaptureSpec {
    fn default() -> Self {
        Self {
            video_device: String::new(),
            audio_device: String::new(),
            width: 1280,
            height: 720,
            fps: 30,
        }
    }
}

impl CaptureSpec {
    /// `dshow://` plus the options that pick the device and its format.
    ///
    /// Empty device names mean "whatever DirectShow lists first", which is what libVLC does
    /// when the option is absent, so they are skipped rather than sent as empty strings.
    pub fn to_location_and_options(&self) -> (String, Vec<String>) {
        let mut options = Vec::new();
        if !self.video_device.trim().is_empty() {
            options.push(format!(
                "dshow-vdev={}",
                quote_value(self.video_device.trim())
            ));
        }
        if !self.audio_device.trim().is_empty() {
            options.push(format!(
                "dshow-adev={}",
                quote_value(self.audio_device.trim())
            ));
        }
        if self.width > 0 && self.height > 0 {
            options.push(format!("dshow-size={}x{}", self.width, self.height));
        }
        if self.fps > 0 {
            options.push(format!("dshow-fps={}", self.fps));
        }
        ("dshow://".to_string(), options)
    }

    /// One-line description for the dialog footer.
    pub fn summary(&self) -> String {
        let video = if self.video_device.trim().is_empty() {
            "default video".to_string()
        } else {
            self.video_device.trim().to_string()
        };
        let audio = if self.audio_device.trim().is_empty() {
            "default audio".to_string()
        } else {
            self.audio_device.trim().to_string()
        };
        format!(
            "{video} · {audio} · {}×{} @ {} fps",
            self.width, self.height, self.fps
        )
    }
}

fn quote_value(value: &str) -> String {
    if value.contains(' ') {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

/// A conversion target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertPreset {
    pub id: &'static str,
    pub extension: &'static str,
    pub video_codec: &'static str,
    pub audio_codec: &'static str,
    pub video_bitrate: u32,
    pub audio_bitrate: u32,
    pub mux: &'static str,
}

/// Video/audio conversions offered by the Convert dialog.
pub const CONVERT_PRESETS: &[ConvertPreset] = &[
    ConvertPreset {
        id: "mp4-h264",
        extension: "mp4",
        video_codec: "h264",
        audio_codec: "mp4a",
        video_bitrate: 2500,
        audio_bitrate: 128,
        mux: "mp4",
    },
    ConvertPreset {
        id: "webm-vp8",
        extension: "webm",
        video_codec: "vp80",
        audio_codec: "vorb",
        video_bitrate: 1800,
        audio_bitrate: 128,
        mux: "webm",
    },
    ConvertPreset {
        id: "mkv-h264",
        extension: "mkv",
        video_codec: "h264",
        audio_codec: "mp4a",
        video_bitrate: 4000,
        audio_bitrate: 192,
        mux: "mkv",
    },
    ConvertPreset {
        id: "mp3",
        extension: "mp3",
        video_codec: "",
        audio_codec: "mp3",
        video_bitrate: 0,
        audio_bitrate: 192,
        mux: "raw",
    },
    ConvertPreset {
        id: "m4a",
        extension: "m4a",
        video_codec: "",
        audio_codec: "mp4a",
        video_bitrate: 0,
        audio_bitrate: 192,
        mux: "mp4",
    },
];

impl ConvertPreset {
    pub fn by_id(id: &str) -> Option<&'static Self> {
        CONVERT_PRESETS.iter().find(|preset| preset.id == id)
    }

    /// `true` when the preset drops the video track (audio-only outputs).
    pub fn is_audio_only(&self) -> bool {
        self.video_codec.is_empty()
    }
}

/// Build the `:sout=` chain that writes `destination` in this preset's format.
///
/// Audio-only presets use `vcodec=none` — without it libVLC would try to mux a video track
/// into an MP3 file and fail at open time.
pub fn convert_sout(preset: &ConvertPreset, destination: &Path) -> String {
    let destination = destination.display().to_string();
    let video = if preset.is_audio_only() {
        "vcodec=none".to_string()
    } else {
        format!("vcodec={},vb={}", preset.video_codec, preset.video_bitrate)
    };
    format!(
        "#transcode{{{video},acodec={},ab={},channels=2,samplerate=44100}}\
         :std{{access=file,mux={},dst={}}}",
        preset.audio_codec,
        preset.audio_bitrate,
        preset.mux,
        quote_value(&destination)
    )
}

/// A streaming target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamPreset {
    pub id: &'static str,
    pub mux: &'static str,
    pub video_bitrate: u32,
}

pub const STREAM_PRESETS: &[StreamPreset] = &[
    StreamPreset {
        id: "http-ts",
        mux: "ts",
        video_bitrate: 1500,
    },
    StreamPreset {
        id: "http-ps",
        mux: "ps",
        video_bitrate: 3000,
    },
    StreamPreset {
        id: "rtsp",
        mux: "ts",
        video_bitrate: 1500,
    },
];

impl StreamPreset {
    pub fn by_id(id: &str) -> Option<&'static Self> {
        STREAM_PRESETS.iter().find(|preset| preset.id == id)
    }
}

/// Build the `:sout=` chain that serves the current media over the network.
///
/// HTTP presets use libVLC's `dst=:port/path` shorthand so the chain works regardless of which
/// interface the machine listens on. RTSP needs the `rtp{sdp=rtsp://…}` access output, which
/// starts a real RTSP server; it must not be advertised as HTTP.
pub fn stream_sout(preset: &StreamPreset, port: u16, path: &str) -> String {
    let path = path.trim().trim_start_matches('/');
    let path = if path.is_empty() { "stream" } else { path };
    let transcode = format!(
        "#transcode{{vcodec=h264,vb={},acodec=mp4a,ab=128,channels=2,samplerate=44100}}",
        preset.video_bitrate
    );
    if preset.id == "rtsp" {
        format!("{transcode}:rtp{{sdp=rtsp://:{port}/{path}}}")
    } else {
        format!(
            "{transcode}:std{{access=http,mux={},dst=:{port}/{path}}}",
            preset.mux
        )
    }
}

/// URL a viewer should open for a stream served on `port`/`path`.
pub fn stream_url(preset: &StreamPreset, port: u16, path: &str) -> String {
    let path = path.trim().trim_start_matches('/');
    let path = if path.is_empty() { "stream" } else { path };
    let scheme = if preset.id == "rtsp" { "rtsp" } else { "http" };
    format!("{scheme}://<this-pc>:{port}/{path}")
}

/// Default destination next to the source, e.g. `movie.mp4` → `movie (converted).mp4`.
pub fn default_destination(source: &Path, extension: &str) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let parent = source.parent().unwrap_or_else(|| Path::new(""));
    parent.join(format!("{stem} (converted).{extension}"))
}

/// File-dialog filters for the Open file dialog, in the order the dialog shows them.
pub const OPEN_FILTERS: &[(&str, &[&str])] = &[
    (
        "Video files",
        &[
            "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "m2ts",
            "3gp", "ogv",
        ],
    ),
    (
        "Audio files",
        &[
            "mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "wma", "alac",
        ],
    ),
    ("Subtitles", &["srt", "ass", "ssa", "vtt", "sub"]),
    ("Playlists", &["m3u", "m3u8", "xspf", "pls"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_keeps_only_media_files() {
        let kept = collect_media(vec![
            PathBuf::from(r"D:\media\episode.mkv"),
            PathBuf::from(r"D:\media\notes.txt"),
            PathBuf::from(r"D:\media\song.flac"),
        ]);
        assert_eq!(
            kept,
            vec![
                PathBuf::from(r"D:\media\episode.mkv"),
                PathBuf::from(r"D:\media\song.flac"),
            ]
        );
    }

    #[test]
    fn drop_of_a_folder_takes_the_media_inside_it() {
        let dir = std::env::temp_dir().join("vfm-drop-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.mkv"), b"x").unwrap();
        std::fs::write(dir.join("a.mp4"), b"x").unwrap();
        std::fs::write(dir.join("readme.md"), b"x").unwrap();

        let kept = collect_media(vec![dir.clone()]);
        assert_eq!(kept, vec![dir.join("a.mp4"), dir.join("b.mkv")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_without_media_contributes_nothing() {
        let dir = std::env::temp_dir().join("vfm-drop-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        assert!(collect_media(vec![dir.clone()]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(classify("   "), None);
        assert_eq!(classify("\"\""), None);
    }

    #[test]
    fn urls_keep_their_scheme() {
        assert_eq!(
            classify("https://example.com/a.m3u8"),
            Some(Target::Location("https://example.com/a.m3u8".into()))
        );
        assert_eq!(
            classify("rtsp://10.0.0.2:554/live"),
            Some(Target::Location("rtsp://10.0.0.2:554/live".into()))
        );
    }

    #[test]
    fn windows_paths_are_files() {
        let target = classify(r"D:\media\episode.mkv").unwrap();
        assert!(matches!(target, Target::File(_)));
        assert!(!target.is_location());
    }

    #[test]
    fn bare_hosts_gain_http() {
        assert_eq!(
            classify("example.com/live.m3u8"),
            Some(Target::Location("http://example.com/live.m3u8".into()))
        );
        assert_eq!(
            classify("10.0.0.5:8080"),
            Some(Target::Location("http://10.0.0.5:8080".into()))
        );
    }

    #[test]
    fn quotes_are_stripped() {
        assert_eq!(
            classify("\"https://example.com/x.mp4\""),
            Some(Target::Location("https://example.com/x.mp4".into()))
        );
    }

    #[test]
    fn capture_skips_empty_devices() {
        let spec = CaptureSpec::default();
        let (location, options) = spec.to_location_and_options();
        assert_eq!(location, "dshow://");
        assert!(options.contains(&"dshow-size=1280x720".to_string()));
        assert!(options.iter().all(|o| !o.contains("dshow-vdev")));
    }

    #[test]
    fn capture_quotes_spaced_device_names() {
        let spec = CaptureSpec {
            video_device: "Integrated Camera".into(),
            audio_device: "Microphone (USB)".into(),
            ..CaptureSpec::default()
        };
        let (_, options) = spec.to_location_and_options();
        assert!(options.contains(&"dshow-vdev=\"Integrated Camera\"".to_string()));
        assert!(options.contains(&"dshow-adev=\"Microphone (USB)\"".to_string()));
    }

    #[test]
    fn audio_only_convert_drops_video() {
        let preset = ConvertPreset::by_id("mp3").unwrap();
        let sout = convert_sout(preset, Path::new(r"D:\out.mp3"));
        assert!(sout.contains("vcodec=none"));
        assert!(sout.contains("mux=raw"));
        assert!(!sout.contains("vb="));
    }

    #[test]
    fn video_convert_carries_bitrate_and_mux() {
        let preset = ConvertPreset::by_id("mp4-h264").unwrap();
        let sout = convert_sout(preset, Path::new(r"D:\out.mp4"));
        assert!(sout.contains("vcodec=h264,vb=2500"));
        assert!(sout.contains("acodec=mp4a,ab=128"));
        assert!(sout.contains("mux=mp4"));
    }

    #[test]
    fn stream_chain_uses_port_and_path() {
        let preset = StreamPreset::by_id("http-ts").unwrap();
        let sout = stream_sout(preset, 8080, "/live");
        assert!(sout.contains("dst=:8080/live"));
        assert!(sout.contains("mux=ts"));
        assert_eq!(
            stream_url(preset, 8080, "live"),
            "http://<this-pc>:8080/live"
        );
    }

    #[test]
    fn rtsp_preset_serves_real_rtsp_not_http() {
        let preset = StreamPreset::by_id("rtsp").unwrap();
        let sout = stream_sout(preset, 8554, "live");
        assert!(sout.contains("rtp{sdp=rtsp://:8554/live}"));
        assert!(!sout.contains("access=http"));
        assert_eq!(
            stream_url(preset, 8554, "live"),
            "rtsp://<this-pc>:8554/live"
        );
    }

    #[test]
    fn empty_stream_path_falls_back() {
        let preset = StreamPreset::by_id("http-ts").unwrap();
        assert!(stream_sout(preset, 9000, "  ").contains("dst=:9000/stream"));
    }

    #[test]
    fn default_destination_sits_beside_the_source() {
        let out = default_destination(Path::new(r"D:\media\clip.mkv"), "mp4");
        assert_eq!(out, PathBuf::from(r"D:\media\clip (converted).mp4"));
    }

    #[test]
    fn unknown_preset_ids_are_rejected() {
        assert!(ConvertPreset::by_id("nope").is_none());
        assert!(StreamPreset::by_id("nope").is_none());
    }
}
