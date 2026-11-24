//! Named player presets with folder/show auto-apply resolution.
//!
//! Payloads are stored as TOML in the existing `presets(name, scope, payload)` table.
//! Keeping the model in `core` makes matching and CRUD unit-testable without libVLC or Slint.

use std::path::{Component, Path, PathBuf};

use crate::{LibraryStore, SubtitleSession};
use serde::{Deserialize, Serialize};

/// Subtitle part of a saved preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtitlePreset {
    pub enabled: bool,
    pub delay_ms: i64,
    /// 0.5–2.0 multiplier used by relative sizing.
    pub size_scale: f32,
    /// When true, use `precise_size_px` instead of the authored relative size.
    pub precise_size: bool,
    pub precise_size_px: f32,
    /// 0 = bottom, 1 = top.
    pub position: f32,
    pub outline: f32,
    pub default_language: String,
}

impl Default for SubtitlePreset {
    fn default() -> Self {
        Self {
            enabled: true,
            delay_ms: 0,
            size_scale: 1.0,
            precise_size: false,
            precise_size_px: 28.0,
            position: 0.1,
            outline: 1.0,
            default_language: "Auto".into(),
        }
    }
}

/// Audio part of a saved preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioPreset {
    pub delay_ms: i64,
    pub default_language: String,
    pub equalizer: String,
    pub normalize_volume: bool,
    pub headphone_surround: bool,
}

impl Default for AudioPreset {
    fn default() -> Self {
        Self {
            delay_ms: 0,
            default_language: "Auto".into(),
            equalizer: "Flat".into(),
            normalize_volume: false,
            headphone_surround: false,
        }
    }
}

impl AudioPreset {
    /// Friendly equalizer labels used by the sidebar dropdown.
    pub const EQ_LABELS: &'static [&'static str] =
        &["Flat", "Bass boost", "Treble boost", "Vocal", "Night"];

    pub fn equalizer_index(&self) -> usize {
        Self::EQ_LABELS
            .iter()
            .position(|label| *label == self.equalizer)
            .unwrap_or(0)
    }
}

/// Video part of a saved preset. Values are libVLC adjustment ranges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoPreset {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub hue: f32,
    pub gamma: f32,
    pub zoom: f32,
    pub aspect_ratio: String,
    pub rotate_degrees: i32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
}

impl Default for VideoPreset {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            hue: 0.0,
            gamma: 1.0,
            zoom: 1.0,
            aspect_ratio: "Auto".into(),
            rotate_degrees: 0,
            flip_horizontal: false,
            flip_vertical: false,
        }
    }
}

impl VideoPreset {
    pub const ASPECT_LABELS: &'static [&'static str] = &["Auto", "16:9", "4:3", "1:1", "2.35:1"];

    pub fn aspect_index(&self) -> usize {
        Self::ASPECT_LABELS
            .iter()
            .position(|label| *label == self.aspect_ratio)
            .unwrap_or(0)
    }

    /// libVLC expects a C string for a fixed ratio, or NULL for auto.
    pub fn aspect_cstr(&self) -> Option<&str> {
        if self.aspect_ratio == "Auto" {
            None
        } else {
            Some(&self.aspect_ratio)
        }
    }
}

/// Playback part of a saved preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackPreset {
    pub speed: f32,
    pub sleep_minutes: u32,
    pub show_subtitle_timeline: bool,
    pub show_waveform: bool,
    pub show_metadata: bool,
}

impl Default for PlaybackPreset {
    fn default() -> Self {
        Self {
            speed: 1.0,
            sleep_minutes: 0,
            show_subtitle_timeline: true,
            show_waveform: false,
            show_metadata: false,
        }
    }
}

/// All adjustable settings captured by one named preset.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PresetPayload {
    pub subtitle: SubtitlePreset,
    pub audio: AudioPreset,
    pub video: VideoPreset,
    pub playback: PlaybackPreset,
}

impl PresetPayload {
    /// Apply the subtitle part to a live session without touching its cues.
    pub fn apply_to_subtitle_session(&self, session: &mut SubtitleSession) {
        session.delay_ms = self.subtitle.delay_ms;
        session.size_scale = self.subtitle.size_scale.clamp(0.5, 2.0);
        session.override_size = self.subtitle.precise_size;
        session.base_size_px = if self.subtitle.precise_size {
            self.subtitle.precise_size_px.clamp(12.0, 96.0)
        } else {
            28.0
        };
        // Cues are authored around 0.1; the session stores an offset from that baseline.
        session.position_offset = (self.subtitle.position - 0.1).clamp(-0.1, 0.9);
        session.enabled = self.subtitle.enabled;
    }
}

/// Where a preset applies. `Folder` is the show/folder auto-apply case from the spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetScope {
    Global,
    Folder(PathBuf),
    Playlist(String),
}

impl PresetScope {
    pub fn to_storage(&self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Folder(p) => format!("folder:{}", p.to_string_lossy()),
            Self::Playlist(name) => format!("playlist:{name}"),
        }
    }

    pub fn from_storage(text: &str) -> Result<Self, PresetError> {
        if let Some(path) = text.strip_prefix("folder:") {
            return Ok(Self::Folder(PathBuf::from(path)));
        }
        if let Some(name) = text.strip_prefix("playlist:") {
            return Ok(Self::Playlist(name.into()));
        }
        if text == "global" {
            return Ok(Self::Global);
        }
        Err(PresetError::Scope(text.into()))
    }

    pub fn label(&self) -> String {
        match self {
            Self::Global => "All media".into(),
            Self::Folder(p) => p.to_string_lossy().into_owned(),
            Self::Playlist(name) => name.clone(),
        }
    }

    pub fn folder(&self) -> Option<&Path> {
        match self {
            Self::Folder(p) => Some(p),
            _ => None,
        }
    }

    /// Higher rank wins. Folder depth breaks ties so a season folder beats a show root.
    pub fn match_rank(&self, media: &Path, playlist: Option<&str>) -> Option<usize> {
        match self {
            Self::Global => Some(1),
            Self::Folder(folder) => {
                let parent = media.parent()?;
                folder_matches(parent, folder)
                    .then(|| 100 + normalize_path(folder).split('/').count())
            }
            Self::Playlist(name) => playlist.is_some_and(|value| value == name).then_some(1_000),
        }
    }
}

/// A named preset loaded from or stored in SQLite.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    pub scope: PresetScope,
    pub payload: PresetPayload,
}

impl Preset {
    pub fn new(name: impl Into<String>, scope: PresetScope, payload: PresetPayload) -> Self {
        Self {
            name: name.into(),
            scope,
            payload,
        }
    }

    pub fn from_storage(name: String, scope: &str, payload: &str) -> Result<Self, PresetError> {
        Ok(Self {
            name,
            scope: PresetScope::from_storage(scope)?,
            payload: toml::from_str(payload)?,
        })
    }

    pub fn matches_media(&self, media: &Path, playlist: Option<&str>) -> bool {
        self.scope.match_rank(media, playlist).is_some()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("invalid preset payload: {0}")]
    Decode(#[from] toml::de::Error),
    #[error("could not encode preset payload: {0}")]
    Encode(#[from] toml::ser::Error),
    #[error("unknown preset scope: {0}")]
    Scope(String),
}

/// Load every preset, keeping deterministic name order from SQL.
pub fn load_all(db: &LibraryStore) -> Result<Vec<Preset>, PresetError> {
    db.all_presets()?
        .into_iter()
        .map(|(name, scope, payload)| Preset::from_storage(name, &scope, &payload))
        .collect()
}

pub fn save(db: &LibraryStore, preset: &Preset) -> Result<(), PresetError> {
    let payload = toml::to_string(&preset.payload)?;
    db.save_preset(&preset.name, &preset.scope.to_storage(), &payload)?;
    Ok(())
}

pub fn rename(db: &LibraryStore, old: &str, new: &str) -> Result<(), PresetError> {
    db.rename_preset(old, new)?;
    Ok(())
}

pub fn remove(db: &LibraryStore, name: &str) -> Result<(), PresetError> {
    db.delete_preset(name)?;
    Ok(())
}

/// Pick the most specific preset for the media about to be played.
pub fn resolve_for_media<'a>(
    presets: &'a [Preset],
    media: &Path,
    playlist: Option<&str>,
) -> Option<&'a Preset> {
    presets
        .iter()
        .filter_map(|preset| {
            preset
                .scope
                .match_rank(media, playlist)
                .map(|rank| (rank, preset))
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, preset)| preset)
}

fn normalize_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => parts.push(
                prefix
                    .as_os_str()
                    .to_string_lossy()
                    .trim_end_matches(':')
                    .to_ascii_lowercase(),
            ),
            Component::RootDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().to_ascii_lowercase()),
            Component::CurDir => {}
            Component::ParentDir => parts.push("..".into()),
        }
    }
    parts.join("/")
}

fn folder_matches(parent: &Path, folder: &Path) -> bool {
    let parent = normalize_path(parent);
    let folder = normalize_path(folder);
    if folder.is_empty() {
        return false;
    }
    parent == folder || parent.starts_with(&format!("{folder}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(name: &str, scope: PresetScope, delay: i64) -> Preset {
        let mut payload = PresetPayload::default();
        payload.subtitle.delay_ms = delay;
        Preset::new(name, scope, payload)
    }

    #[test]
    fn scope_storage_round_trips() {
        let folder = PresetScope::Folder(PathBuf::from("C:/Shows/S01"));
        assert_eq!(
            PresetScope::from_storage(&folder.to_storage()).unwrap(),
            folder
        );
        assert_eq!(
            PresetScope::from_storage("playlist:Anime").unwrap(),
            PresetScope::Playlist("Anime".into())
        );
        assert_eq!(
            PresetScope::from_storage("global").unwrap(),
            PresetScope::Global
        );
        assert!(PresetScope::from_storage("unknown:x").is_err());
    }

    #[test]
    fn crud_survives_reparse() {
        let db = LibraryStore::in_memory().unwrap();
        let original = preset(
            "Show A",
            PresetScope::Folder(PathBuf::from("D:/Shows/A")),
            250,
        );
        save(&db, &original).unwrap();
        let loaded = load_all(&db).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], original);

        rename(&db, "Show A", "Show A - AR").unwrap();
        let renamed = load_all(&db).unwrap();
        assert_eq!(renamed[0].name, "Show A - AR");
        assert_eq!(renamed[0].payload.subtitle.delay_ms, 250);

        remove(&db, "Show A - AR").unwrap();
        assert!(load_all(&db).unwrap().is_empty());
    }

    #[test]
    fn folder_scope_auto_applies_to_next_episode() {
        let presets = vec![preset(
            "Show A",
            PresetScope::Folder(PathBuf::from("D:/Shows/A")),
            120,
        )];
        let next = Path::new("D:/Shows/A/Season 1/Ep02.mkv");
        let resolved = resolve_for_media(&presets, next, None).unwrap();
        assert_eq!(resolved.name, "Show A");
        assert_eq!(resolved.payload.subtitle.delay_ms, 120);

        let outside = resolve_for_media(&presets, Path::new("D:/Shows/B/Ep01.mkv"), None);
        assert!(outside.is_none());
    }

    #[test]
    fn acceptance_saved_show_preset_applies_to_next_episode() {
        let db = LibraryStore::in_memory().unwrap();
        let show = PathBuf::from("D:/Shows/Solaris");
        let original = preset("Solaris", PresetScope::Folder(show.clone()), 320);
        save(&db, &original).unwrap();

        // Simulate a fresh player opening a different episode from the same folder.
        let loaded = load_all(&db).unwrap();
        let next_episode = show.join("Season 1").join("E02.mkv");
        let resolved = resolve_for_media(&loaded, &next_episode, None).unwrap();
        assert_eq!(resolved.name, "Solaris");
        assert_eq!(resolved.payload.subtitle.delay_ms, 320);
    }

    #[test]
    fn most_specific_scope_wins() {
        let presets = vec![
            preset("Global", PresetScope::Global, 10),
            preset("Show", PresetScope::Folder(PathBuf::from("D:/Shows/A")), 20),
            preset(
                "Season",
                PresetScope::Folder(PathBuf::from("D:/Shows/A/Season 2")),
                30,
            ),
        ];
        let resolved =
            resolve_for_media(&presets, Path::new("D:/Shows/A/Season 2/Ep05.mkv"), None).unwrap();
        assert_eq!(resolved.name, "Season");
    }

    #[test]
    fn playlist_scope_and_global_fallback() {
        let presets = vec![
            preset("Global", PresetScope::Global, 5),
            preset("Anime", PresetScope::Playlist("Anime".into()), 50),
        ];
        let media = Path::new("D:/Random/clip.mkv");
        assert_eq!(
            resolve_for_media(&presets, media, Some("Anime"))
                .unwrap()
                .name,
            "Anime"
        );
        assert_eq!(
            resolve_for_media(&presets, media, Some("Movies"))
                .unwrap()
                .name,
            "Global"
        );
    }

    #[test]
    fn payload_applies_style_without_mutating_cues() {
        let mut payload = PresetPayload::default();
        payload.subtitle.delay_ms = 500;
        payload.subtitle.precise_size = true;
        payload.subtitle.precise_size_px = 42.0;
        payload.subtitle.position = 0.6;
        let mut session = SubtitleSession::from_text(
            "1\n00:00:00,000 --> 00:00:02,000\nhello\n",
            crate::SubtitleFormat::Srt,
        );
        let before = session.cues()[0].clone();
        payload.apply_to_subtitle_session(&mut session);
        assert_eq!(session.delay_ms, 500);
        assert!(session.override_size);
        assert_eq!(session.base_size_px, 42.0);
        assert!((session.position_offset - 0.5).abs() < 0.001);
        assert_eq!(session.cues()[0], before);
    }
}
