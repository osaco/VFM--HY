//! Versioned, typed settings stored as TOML with change notifications.
//!
//! Every field here is addressable through [`crate::settings_schema`], which owns the
//! labels, ranges and defaults the settings window renders. Keep the two in step: the
//! schema tests assert that each declared setting round-trips through this struct.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Bumped when a field is added or its meaning changes. Older files keep their known
/// fields and pick up defaults for the rest through `#[serde(default)]`.
pub const SETTINGS_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,

    // ---- General -------------------------------------------------------------------
    pub theme: Theme,
    pub language: Language,
    pub accent: String,
    /// Reopen at the size and position the window had when it closed.
    pub remember_window: bool,

    // ---- Interface -----------------------------------------------------------------
    /// Transient on-screen messages for hotkeys and mode changes.
    pub osd: bool,
    /// Draw the audio envelope inside the seek bar.
    pub waveform: bool,
    /// Right sidebar tab restored on start (0..4).
    pub sidebar_tab: u32,
    /// Window shown first on launch.
    pub start_page: StartPage,

    // ---- Privacy -------------------------------------------------------------------
    /// Keep the recently-played list.
    pub remember_recent: bool,
    /// Wipe the recently-played list on a clean exit.
    pub clear_recent_on_exit: bool,
    /// OpenSubtitles REST API key. Empty keeps the subtitle search dialog disabled, so
    /// the app never sends a request without an explicit opt-in.
    pub opensubtitles_api_key: String,
    /// Write a diagnostics file next to the settings when the player panics.
    pub diagnostics: bool,

    // ---- Controls ------------------------------------------------------------------
    /// Arrow-key seek step in milliseconds.
    pub seek_step_ms: i64,
    /// Arrow-key volume step in percent.
    pub volume_step: i32,
    pub wheel_action: WheelAction,
    /// Single click on the video toggles playback.
    pub click_pause: bool,
    pub double_click: DoubleClickAction,

    // ---- Playback ------------------------------------------------------------------
    pub volume: i32,
    pub repeat: crate::queue::RepeatMode,
    pub shuffle: bool,
    pub remember_position: bool,
    pub resume_min_remaining_ms: i64,
    /// Move to the next queue item when the current one ends.
    pub auto_advance: bool,
    /// Open paused so a file can be inspected before it plays.
    pub start_paused: bool,
    /// Playback rate applied when a file opens (1.0 = normal).
    pub default_speed: f64,

    // ---- Video ---------------------------------------------------------------------
    /// Empty means "let the file decide"; otherwise a libVLC ratio such as "16:9".
    pub aspect_ratio: String,
    pub deinterlace: Deinterlace,
    pub zoom: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    /// `false` forces software decoding (helps with broken GPU drivers).
    pub hardware_decoding: bool,

    // ---- Audio ---------------------------------------------------------------------
    /// Equalizer preset name; "flat" is the neutral curve.
    pub equalizer: String,
    /// Pick the first audio track instead of the container's default.
    pub prefer_original_audio: bool,
    pub mute_on_start: bool,

    // ---- Subtitles -----------------------------------------------------------------
    pub subtitle_auto_load: bool,
    /// Text scale as a multiplier of the base subtitle size.
    pub subtitle_scale: f64,
    /// Empty means "detect"; otherwise a codec name such as "utf-8".
    pub subtitle_encoding: String,
    pub subtitle_outline: bool,
    /// Vertical position of the subtitle block, 0 = top, 1 = bottom.
    pub subtitle_position: f64,
    /// Preferred language for OpenSubtitles searches.
    pub subtitle_search_language: String,

    // ---- Library -------------------------------------------------------------------
    pub library_folder: String,
    pub library_scan_on_start: bool,
    pub library_sort: crate::library::SortMode,

    // ---- Network -------------------------------------------------------------------
    pub network_timeout_ms: i64,
    /// Demuxer cache in milliseconds; higher survives slow links, higher latency.
    pub network_caching_ms: i64,

    // ---- Performance ---------------------------------------------------------------
    pub skip_loop_filter: SkipLoopFilter,

    // ---- Recording -----------------------------------------------------------------
    pub snapshot_folder: String,
    /// "png" or "jpg".
    pub snapshot_format: String,
    /// Prepended to generated snapshot file names.
    pub snapshot_prefix: String,

    // ---- Extensions ----------------------------------------------------------------
    pub extensions_folder: String,

    // ---- Onboarding ----------------------------------------------------------------
    pub onboarding_done: bool,

    // ---- Layout (Phase 10) ---------------------------------------------------------
    /// Serialised control-bar layout; empty means "use the default".
    #[serde(default)]
    pub layout: String,

    pub last_window: Option<WindowState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
    #[default]
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Ar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StartPage {
    Library,
    #[default]
    Player,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WheelAction {
    #[default]
    Volume,
    Seek,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DoubleClickAction {
    #[default]
    Fullscreen,
    Play,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Deinterlace {
    #[default]
    Auto,
    Off,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SkipLoopFilter {
    #[default]
    Auto,
    Off,
    On,
}

impl Deinterlace {
    /// libVLC spells the mode as a single option string.
    pub fn as_vlc(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "",
            Self::On => "yadif",
        }
    }
}

impl SkipLoopFilter {
    /// Values accepted by `--avcodec-skiploopfilter`.
    pub fn as_vlc(self) -> &'static str {
        match self {
            Self::Auto => "0",
            Self::Off => "1",
            Self::On => "4",
        }
    }
}

impl StartPage {
    pub fn id(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Player => "player",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "library" => Self::Library,
            _ => Self::Player,
        }
    }
}

impl WheelAction {
    pub fn id(self) -> &'static str {
        match self {
            Self::Volume => "volume",
            Self::Seek => "seek",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "seek" => Self::Seek,
            _ => Self::Volume,
        }
    }
}

impl DoubleClickAction {
    pub fn id(self) -> &'static str {
        match self {
            Self::Fullscreen => "fullscreen",
            Self::Play => "play",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "play" => Self::Play,
            _ => Self::Fullscreen,
        }
    }
}

impl Deinterlace {
    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
            Self::On => "on",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "off" => Self::Off,
            "on" => Self::On,
            _ => Self::Auto,
        }
    }
}

impl SkipLoopFilter {
    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
            Self::On => "on",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "off" => Self::Off,
            "on" => Self::On,
            _ => Self::Auto,
        }
    }
}

impl Theme {
    pub fn id(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::System => "system",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        }
    }
}

impl Language {
    pub fn id(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ar => "ar",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "ar" => Self::Ar,
            _ => Self::En,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: Theme::System,
            language: Language::En,
            accent: "#D37918".into(),
            remember_window: true,
            osd: true,
            waveform: true,
            sidebar_tab: 0,
            start_page: StartPage::Player,
            remember_recent: true,
            clear_recent_on_exit: false,
            opensubtitles_api_key: String::new(),
            diagnostics: false,
            seek_step_ms: 5_000,
            volume_step: 5,
            wheel_action: WheelAction::Volume,
            click_pause: true,
            double_click: DoubleClickAction::Fullscreen,
            volume: 100,
            repeat: crate::queue::RepeatMode::Off,
            shuffle: false,
            remember_position: true,
            resume_min_remaining_ms: 30_000,
            auto_advance: true,
            start_paused: false,
            default_speed: 1.0,
            aspect_ratio: String::new(),
            deinterlace: Deinterlace::Auto,
            zoom: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            hardware_decoding: true,
            equalizer: "flat".into(),
            prefer_original_audio: false,
            mute_on_start: false,
            subtitle_auto_load: true,
            subtitle_scale: 1.0,
            subtitle_encoding: String::new(),
            subtitle_outline: true,
            subtitle_position: 0.92,
            subtitle_search_language: "en".into(),
            library_folder: String::new(),
            library_scan_on_start: false,
            library_sort: crate::library::SortMode::NameAsc,
            network_timeout_ms: 10_000,
            network_caching_ms: 1_500,
            skip_loop_filter: SkipLoopFilter::Auto,
            snapshot_folder: String::new(),
            snapshot_format: "png".into(),
            snapshot_prefix: "vfm".into(),
            extensions_folder: String::new(),
            onboarding_done: false,
            layout: String::new(),
            last_window: None,
        }
    }
}

impl Settings {
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Parse settings; unknown/future versions keep their known fields and are upgraded in place.
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        let mut parsed: Self = toml::from_str(s)?;
        // Stamp the current version after a successful parse. Fields this build does not
        // know are dropped by serde and new fields take their defaults, so the in-memory
        // value is always current; the next flush writes the migrated file.
        parsed.version = SETTINGS_VERSION;
        parsed.clamp();
        Ok(parsed)
    }

    /// Pull every numeric field back into its declared range.
    ///
    /// A hand-edited settings file (or an older build) can hold values the UI would never
    /// produce; clamping here means the rest of the app can trust the numbers.
    pub fn clamp(&mut self) {
        self.volume = self.volume.clamp(0, 125);
        self.volume_step = self.volume_step.clamp(1, 20);
        self.seek_step_ms = self.seek_step_ms.clamp(1_000, 60_000);
        self.resume_min_remaining_ms = self.resume_min_remaining_ms.clamp(0, 300_000);
        self.default_speed = self.default_speed.clamp(0.25, 4.0);
        self.zoom = self.zoom.clamp(0.25, 4.0);
        self.brightness = self.brightness.clamp(0.0, 2.0);
        self.contrast = self.contrast.clamp(0.0, 2.0);
        self.saturation = self.saturation.clamp(0.0, 2.0);
        self.subtitle_scale = self.subtitle_scale.clamp(0.5, 2.0);
        self.subtitle_position = self.subtitle_position.clamp(0.0, 1.0);
        self.network_timeout_ms = self.network_timeout_ms.clamp(1_000, 60_000);
        self.network_caching_ms = self.network_caching_ms.clamp(0, 10_000);
        self.sidebar_tab = self.sidebar_tab.min(4);
        if !matches!(self.snapshot_format.as_str(), "png" | "jpg") {
            self.snapshot_format = "png".into();
        }
    }

    /// Snapshot directory, falling back to the data directory when unset.
    pub fn snapshot_dir(&self, data_dir: &Path) -> PathBuf {
        if self.snapshot_folder.trim().is_empty() {
            data_dir.join("snapshots")
        } else {
            PathBuf::from(self.snapshot_folder.trim())
        }
    }
}

/// A shared settings value with subscriber notifications.
type Subscriber = Box<dyn Fn(&Settings) + Send + Sync>;

pub struct SettingsStore {
    inner: Arc<Mutex<Settings>>,
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for SettingsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsStore")
            .field("path", &self.path)
            .field(
                "subscribers",
                &self.subscribers.lock().map(|s| s.len()).unwrap_or(0),
            )
            .finish()
    }
}

impl Clone for SettingsStore {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            subscribers: Arc::clone(&self.subscribers),
            path: self.path.clone(),
        }
    }
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Settings::default())),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            path: None,
        }
    }

    pub fn with_file(path: impl Into<PathBuf>) -> Result<Self, SettingsError> {
        let path = path.into();
        let settings = if path.is_file() {
            let text = std::fs::read_to_string(&path)?;
            Settings::from_toml(&text)?
        } else {
            Settings::default()
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(settings)),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            path: Some(path),
        })
    }

    pub fn get(&self) -> Settings {
        self.inner.lock().expect("settings poisoned").clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> Result<(), SettingsError> {
        self.update_quiet(f);
        self.flush()
    }

    /// Apply a change and notify subscribers without touching the file.
    ///
    /// Used for values that change continuously while dragging a slider; call [`flush`]
    /// when the gesture ends so a drag does not write the settings file on every pixel.
    pub fn update_quiet(&self, f: impl FnOnce(&mut Settings)) {
        let changed = {
            let mut guard = self.inner.lock().expect("settings poisoned");
            f(&mut guard);
            guard.clamp();
            guard.clone()
        };
        let subs = self.subscribers.lock().expect("subscribers poisoned");
        for sub in subs.iter() {
            sub(&changed);
        }
    }

    pub fn subscribe(&self, f: impl Fn(&Settings) + Send + Sync + 'static) {
        self.subscribers
            .lock()
            .expect("subscribers poisoned")
            .push(Box::new(f));
    }

    pub fn flush(&self) -> Result<(), SettingsError> {
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let text = self.get().to_toml()?;
            std::fs::write(path, text)?;
        }
        Ok(())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid settings TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("could not serialise settings: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_toml() {
        let s = Settings::default();
        let text = s.to_toml().unwrap();
        let back = Settings::from_toml(&text).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.accent, "#D37918");
        assert_eq!(back.version, SETTINGS_VERSION);
    }

    #[test]
    fn unknown_version_is_upgraded() {
        let text = "version = 99\nlanguage = \"ar\"\n";
        let s = Settings::from_toml(text).unwrap();
        assert_eq!(s.version, SETTINGS_VERSION);
        assert_eq!(s.language, Language::Ar);
    }

    #[test]
    fn version_one_files_gain_the_new_fields() {
        // A Phase 7 file only knew the old subset; everything else must default.
        let text = "version = 1\ntheme = \"light\"\nvolume = 70\n";
        let s = Settings::from_toml(text).unwrap();
        assert_eq!(s.version, SETTINGS_VERSION);
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.volume, 70);
        assert_eq!(s.seek_step_ms, 5_000);
        assert!(s.waveform);
        assert_eq!(s.equalizer, "flat");
        assert!(!s.onboarding_done);
    }

    #[test]
    fn subscribers_are_notified_on_update() {
        let store = SettingsStore::new();
        let seen = Arc::new(Mutex::new(0));
        let counter = Arc::clone(&seen);
        store.subscribe(move |_| {
            *counter.lock().unwrap() += 1;
        });
        store.update(|s| s.volume = 55).unwrap();
        assert_eq!(*seen.lock().unwrap(), 1);
        assert_eq!(store.get().volume, 55);
    }

    #[test]
    fn update_clamps_out_of_range_values() {
        let store = SettingsStore::new();
        store
            .update(|s| {
                s.volume = 400;
                s.subtitle_position = 2.0;
                s.sidebar_tab = 9;
            })
            .unwrap();
        let s = store.get();
        assert_eq!(s.volume, 125);
        assert_eq!(s.subtitle_position, 1.0);
        assert_eq!(s.sidebar_tab, 4);
    }

    #[test]
    fn file_round_trip() {
        let dir = std::env::temp_dir().join(format!("vfm-settings-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        let store = SettingsStore::with_file(&path).unwrap();
        store.update(|s| s.language = Language::Ar).unwrap();
        let reopened = SettingsStore::with_file(&path).unwrap();
        assert_eq!(reopened.get().language, Language::Ar);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn enum_ids_round_trip() {
        assert_eq!(Theme::from_id(Theme::Light.id()), Theme::Light);
        assert_eq!(Language::from_id(Language::Ar.id()), Language::Ar);
        assert_eq!(StartPage::from_id("library"), StartPage::Library);
        assert_eq!(WheelAction::from_id("seek"), WheelAction::Seek);
        assert_eq!(DoubleClickAction::from_id("play"), DoubleClickAction::Play);
        assert_eq!(Deinterlace::from_id("on"), Deinterlace::On);
        assert_eq!(SkipLoopFilter::from_id("off"), SkipLoopFilter::Off);
    }

    #[test]
    fn snapshot_dir_falls_back_to_data_dir() {
        let s = Settings::default();
        assert_eq!(
            s.snapshot_dir(Path::new("D:/data")),
            PathBuf::from("D:/data/snapshots")
        );
        let mut custom = s;
        custom.snapshot_folder = "D:/shots".into();
        assert_eq!(
            custom.snapshot_dir(Path::new("D:/data")),
            PathBuf::from("D:/shots")
        );
    }
}
