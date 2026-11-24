//! Declarative description of every user-facing setting.
//!
//! The settings window is generated from this table, so adding a setting means adding a
//! field to [`Settings`] and one entry here — never a second copy of the default or range
//! in the UI. Tests assert the two stay in sync, that keys are unique, and that every
//! default survives a write/read round-trip.

use crate::library::SortMode;
use crate::queue::RepeatMode;
use crate::settings::{
    Deinterlace, DoubleClickAction, Language, Settings, SkipLoopFilter, StartPage, Theme,
    WheelAction,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    General,
    Interface,
    Privacy,
    Controls,
    Playback,
    Video,
    Audio,
    Subtitles,
    Library,
    Network,
    Performance,
    Recording,
    Extensions,
}

impl Category {
    pub const ALL: &'static [Self] = &[
        Self::General,
        Self::Interface,
        Self::Privacy,
        Self::Controls,
        Self::Playback,
        Self::Video,
        Self::Audio,
        Self::Subtitles,
        Self::Library,
        Self::Network,
        Self::Performance,
        Self::Recording,
        Self::Extensions,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Interface => "interface",
            Self::Privacy => "privacy",
            Self::Controls => "controls",
            Self::Playback => "playback",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Subtitles => "subtitles",
            Self::Library => "library",
            Self::Network => "network",
            Self::Performance => "performance",
            Self::Recording => "recording",
            Self::Extensions => "extensions",
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::General => "settings.cat.general",
            Self::Interface => "settings.cat.interface",
            Self::Privacy => "settings.cat.privacy",
            Self::Controls => "settings.cat.controls",
            Self::Playback => "settings.cat.playback",
            Self::Video => "settings.cat.video",
            Self::Audio => "settings.cat.audio",
            Self::Subtitles => "settings.cat.subtitles",
            Self::Library => "settings.cat.library",
            Self::Network => "settings.cat.network",
            Self::Performance => "settings.cat.performance",
            Self::Recording => "settings.cat.recording",
            Self::Extensions => "settings.cat.extensions",
        }
    }
}

/// One selectable option of a choice setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub id: &'static str,
    /// Explicit label key; empty means "derive it from `id`".
    pub label: &'static str,
}

const fn choice(id: &'static str) -> Choice {
    // The option label is `opt.<id>` unless the caller spells out a different key.
    Choice { id, label: "" }
}

/// Resolve a stored string to the canonical option id, so a hand-edited settings file
/// still lands on a real option instead of an empty label.
fn match_choice(options: &'static [Choice], value: &str) -> &'static str {
    options
        .iter()
        .find(|option| option.id.eq_ignore_ascii_case(value))
        .map(|option| option.id)
        .unwrap_or(options[0].id)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingKind {
    Toggle,
    Choice(&'static [Choice]),
    Slider { min: f64, max: f64, step: f64 },
    Text { secret: bool },
    Color,
    Folder,
}

/// A setting's current value, normalised for the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Number(f64),
    Text(String),
    /// One of the ids declared by the setting's [`SettingKind::Choice`].
    Choice(&'static str),
}

impl SettingValue {
    pub fn as_bool(&self) -> bool {
        matches!(self, Self::Bool(true))
    }

    pub fn as_number(&self) -> f64 {
        match self {
            Self::Number(n) => *n,
            Self::Bool(true) => 1.0,
            Self::Bool(false) => 0.0,
            _ => 0.0,
        }
    }

    pub fn as_text(&self) -> String {
        match self {
            Self::Text(t) => t.clone(),
            Self::Choice(c) => (*c).to_string(),
            Self::Number(n) => format!("{n}"),
            Self::Bool(b) => b.to_string(),
        }
    }

    pub fn as_choice(&self) -> &'static str {
        match self {
            Self::Choice(c) => c,
            _ => "",
        }
    }

    /// Render for the row's trailing read-out.
    pub fn display(&self, suffix: &str) -> String {
        match self {
            Self::Bool(true) => "On".to_string(),
            Self::Bool(false) => "Off".to_string(),
            Self::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{}{suffix}", *n as i64)
                } else {
                    format!("{n:.2}{suffix}")
                }
            }
            Self::Text(t) => t.clone(),
            Self::Choice(t) => (*t).to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingId {
    Theme,
    Language,
    Accent,
    RememberWindow,
    Osd,
    Waveform,
    SidebarTab,
    StartPage,
    RememberRecent,
    ClearRecentOnExit,
    OpenSubtitlesKey,
    Diagnostics,
    SeekStepMs,
    VolumeStep,
    WheelAction,
    ClickPause,
    DoubleClick,
    Volume,
    Repeat,
    Shuffle,
    RememberPosition,
    ResumeMinRemainingMs,
    AutoAdvance,
    StartPaused,
    DefaultSpeed,
    AspectRatio,
    Deinterlace,
    Zoom,
    Brightness,
    Contrast,
    Saturation,
    HardwareDecoding,
    Equalizer,
    PreferOriginalAudio,
    MuteOnStart,
    SubtitleAutoLoad,
    SubtitleScale,
    SubtitleEncoding,
    SubtitleOutline,
    SubtitlePosition,
    SubtitleSearchLanguage,
    LibraryFolder,
    LibraryScanOnStart,
    LibrarySort,
    NetworkTimeoutMs,
    NetworkCachingMs,
    SkipLoopFilter,
    SnapshotFolder,
    SnapshotFormat,
    SnapshotPrefix,
    ExtensionsFolder,
}

const THEME_CHOICES: &[Choice] = &[choice("system"), choice("dark"), choice("light")];
const LANGUAGE_CHOICES: &[Choice] = &[choice("en"), choice("ar")];
const START_PAGE_CHOICES: &[Choice] = &[choice("player"), choice("library")];
const WHEEL_CHOICES: &[Choice] = &[choice("volume"), choice("seek")];
const DOUBLE_CLICK_CHOICES: &[Choice] = &[choice("fullscreen"), choice("play")];
const REPEAT_CHOICES: &[Choice] = &[choice("off"), choice("all"), choice("one")];
const RATIO_CHOICES: &[Choice] = &[
    choice("auto"),
    choice("16:9"),
    choice("4:3"),
    choice("1:1"),
    choice("21:9"),
];
const ON_OFF_AUTO: &[Choice] = &[choice("auto"), choice("off"), choice("on")];
const ENCODING_CHOICES: &[Choice] = &[
    choice("auto"),
    choice("utf-8"),
    choice("cp1256"),
    choice("cp1252"),
    choice("latin1"),
];
const EQ_CHOICES: &[Choice] = &[
    choice("flat"),
    choice("rock"),
    choice("pop"),
    choice("jazz"),
    choice("classical"),
    choice("electronic"),
    choice("vocal"),
    choice("bass"),
];
const SORT_CHOICES: &[Choice] = &[
    Choice {
        id: "name_asc",
        label: "sort.name-asc",
    },
    Choice {
        id: "name_desc",
        label: "sort.name-desc",
    },
    Choice {
        id: "date_newest",
        label: "sort.date-newest",
    },
    Choice {
        id: "date_oldest",
        label: "sort.date-oldest",
    },
    Choice {
        id: "duration_desc",
        label: "sort.duration-desc",
    },
    Choice {
        id: "duration_asc",
        label: "sort.duration-asc",
    },
    Choice {
        id: "size_desc",
        label: "sort.size-desc",
    },
    Choice {
        id: "size_asc",
        label: "sort.size-asc",
    },
    Choice {
        id: "unwatched_first",
        label: "sort.unwatched-first",
    },
    Choice {
        id: "watched_first",
        label: "sort.watched-first",
    },
];
const IMAGE_FORMAT_CHOICES: &[Choice] = &[choice("png"), choice("jpg")];
const SUB_LANGUAGE_CHOICES: &[Choice] = &[choice("en"), choice("ar"), choice("fr"), choice("de")];

impl SettingId {
    /// Declaration order — this is the order the settings window renders.
    pub const ALL: &'static [Self] = &[
        Self::Theme,
        Self::Language,
        Self::Accent,
        Self::RememberWindow,
        Self::Osd,
        Self::Waveform,
        Self::SidebarTab,
        Self::StartPage,
        Self::RememberRecent,
        Self::ClearRecentOnExit,
        Self::OpenSubtitlesKey,
        Self::Diagnostics,
        Self::SeekStepMs,
        Self::VolumeStep,
        Self::WheelAction,
        Self::ClickPause,
        Self::DoubleClick,
        Self::Volume,
        Self::Repeat,
        Self::Shuffle,
        Self::RememberPosition,
        Self::ResumeMinRemainingMs,
        Self::AutoAdvance,
        Self::StartPaused,
        Self::DefaultSpeed,
        Self::AspectRatio,
        Self::Deinterlace,
        Self::Zoom,
        Self::Brightness,
        Self::Contrast,
        Self::Saturation,
        Self::HardwareDecoding,
        Self::Equalizer,
        Self::PreferOriginalAudio,
        Self::MuteOnStart,
        Self::SubtitleAutoLoad,
        Self::SubtitleScale,
        Self::SubtitleEncoding,
        Self::SubtitleOutline,
        Self::SubtitlePosition,
        Self::SubtitleSearchLanguage,
        Self::LibraryFolder,
        Self::LibraryScanOnStart,
        Self::LibrarySort,
        Self::NetworkTimeoutMs,
        Self::NetworkCachingMs,
        Self::SkipLoopFilter,
        Self::SnapshotFolder,
        Self::SnapshotFormat,
        Self::SnapshotPrefix,
        Self::ExtensionsFolder,
    ];

    /// Stable dotted key, used in the settings file and in the import/export format.
    pub fn key(self) -> &'static str {
        match self {
            Self::Theme => "general.theme",
            Self::Language => "general.language",
            Self::Accent => "general.accent",
            Self::RememberWindow => "general.remember-window",
            Self::Osd => "interface.osd",
            Self::Waveform => "interface.waveform",
            Self::SidebarTab => "interface.sidebar-tab",
            Self::StartPage => "interface.start-page",
            Self::RememberRecent => "privacy.remember-recent",
            Self::ClearRecentOnExit => "privacy.clear-recent-on-exit",
            Self::OpenSubtitlesKey => "privacy.opensubtitles-key",
            Self::Diagnostics => "privacy.diagnostics",
            Self::SeekStepMs => "controls.seek-step",
            Self::VolumeStep => "controls.volume-step",
            Self::WheelAction => "controls.wheel",
            Self::ClickPause => "controls.click-pause",
            Self::DoubleClick => "controls.double-click",
            Self::Volume => "playback.volume",
            Self::Repeat => "playback.repeat",
            Self::Shuffle => "playback.shuffle",
            Self::RememberPosition => "playback.resume",
            Self::ResumeMinRemainingMs => "playback.resume-min-remaining",
            Self::AutoAdvance => "playback.auto-advance",
            Self::StartPaused => "playback.start-paused",
            Self::DefaultSpeed => "playback.default-speed",
            Self::AspectRatio => "video.aspect-ratio",
            Self::Deinterlace => "video.deinterlace",
            Self::Zoom => "video.zoom",
            Self::Brightness => "video.brightness",
            Self::Contrast => "video.contrast",
            Self::Saturation => "video.saturation",
            Self::HardwareDecoding => "video.hardware-decoding",
            Self::Equalizer => "audio.equalizer",
            Self::PreferOriginalAudio => "audio.prefer-original",
            Self::MuteOnStart => "audio.mute-on-start",
            Self::SubtitleAutoLoad => "subtitles.auto-load",
            Self::SubtitleScale => "subtitles.scale",
            Self::SubtitleEncoding => "subtitles.encoding",
            Self::SubtitleOutline => "subtitles.outline",
            Self::SubtitlePosition => "subtitles.position",
            Self::SubtitleSearchLanguage => "subtitles.search-language",
            Self::LibraryFolder => "library.folder",
            Self::LibraryScanOnStart => "library.scan-on-start",
            Self::LibrarySort => "library.sort",
            Self::NetworkTimeoutMs => "network.timeout",
            Self::NetworkCachingMs => "network.caching",
            Self::SkipLoopFilter => "performance.skip-loop-filter",
            Self::SnapshotFolder => "recording.snapshot-folder",
            Self::SnapshotFormat => "recording.snapshot-format",
            Self::SnapshotPrefix => "recording.snapshot-prefix",
            Self::ExtensionsFolder => "extensions.folder",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|id| id.key() == key)
    }

    pub fn category(self) -> Category {
        match self {
            Self::Theme | Self::Language | Self::Accent | Self::RememberWindow => Category::General,
            Self::Osd | Self::Waveform | Self::SidebarTab | Self::StartPage => Category::Interface,
            Self::RememberRecent
            | Self::ClearRecentOnExit
            | Self::OpenSubtitlesKey
            | Self::Diagnostics => Category::Privacy,
            Self::SeekStepMs
            | Self::VolumeStep
            | Self::WheelAction
            | Self::ClickPause
            | Self::DoubleClick => Category::Controls,
            Self::Volume
            | Self::Repeat
            | Self::Shuffle
            | Self::RememberPosition
            | Self::ResumeMinRemainingMs
            | Self::AutoAdvance
            | Self::StartPaused
            | Self::DefaultSpeed => Category::Playback,
            Self::AspectRatio
            | Self::Deinterlace
            | Self::Zoom
            | Self::Brightness
            | Self::Contrast
            | Self::Saturation
            | Self::HardwareDecoding => Category::Video,
            Self::Equalizer | Self::PreferOriginalAudio | Self::MuteOnStart => Category::Audio,
            Self::SubtitleAutoLoad
            | Self::SubtitleScale
            | Self::SubtitleEncoding
            | Self::SubtitleOutline
            | Self::SubtitlePosition
            | Self::SubtitleSearchLanguage => Category::Subtitles,
            Self::LibraryFolder | Self::LibraryScanOnStart | Self::LibrarySort => Category::Library,
            Self::NetworkTimeoutMs | Self::NetworkCachingMs => Category::Network,
            Self::SkipLoopFilter => Category::Performance,
            Self::SnapshotFolder | Self::SnapshotFormat | Self::SnapshotPrefix => {
                Category::Recording
            }
            Self::ExtensionsFolder => Category::Extensions,
        }
    }

    /// Hidden behind the category's "Advanced" expander.
    pub fn advanced(self) -> bool {
        matches!(
            self,
            Self::Diagnostics
                | Self::DoubleClick
                | Self::DefaultSpeed
                | Self::HardwareDecoding
                | Self::PreferOriginalAudio
                | Self::SubtitlePosition
                | Self::NetworkCachingMs
                | Self::SkipLoopFilter
                | Self::SnapshotPrefix
                | Self::ExtensionsFolder
        )
    }

    pub fn kind(self) -> SettingKind {
        match self {
            Self::Theme => SettingKind::Choice(THEME_CHOICES),
            Self::Language => SettingKind::Choice(LANGUAGE_CHOICES),
            Self::Accent => SettingKind::Color,
            Self::RememberWindow
            | Self::Osd
            | Self::Waveform
            | Self::RememberRecent
            | Self::ClearRecentOnExit
            | Self::Diagnostics
            | Self::ClickPause
            | Self::Shuffle
            | Self::RememberPosition
            | Self::AutoAdvance
            | Self::StartPaused
            | Self::HardwareDecoding
            | Self::PreferOriginalAudio
            | Self::MuteOnStart
            | Self::SubtitleAutoLoad
            | Self::SubtitleOutline
            | Self::LibraryScanOnStart => SettingKind::Toggle,
            Self::SidebarTab => SettingKind::Slider {
                min: 0.0,
                max: 4.0,
                step: 1.0,
            },
            Self::StartPage => SettingKind::Choice(START_PAGE_CHOICES),
            Self::OpenSubtitlesKey => SettingKind::Text { secret: true },
            Self::SeekStepMs => SettingKind::Slider {
                min: 1_000.0,
                max: 60_000.0,
                step: 1_000.0,
            },
            Self::VolumeStep => SettingKind::Slider {
                min: 1.0,
                max: 20.0,
                step: 1.0,
            },
            Self::WheelAction => SettingKind::Choice(WHEEL_CHOICES),
            Self::DoubleClick => SettingKind::Choice(DOUBLE_CLICK_CHOICES),
            Self::Volume => SettingKind::Slider {
                min: 0.0,
                max: 125.0,
                step: 1.0,
            },
            Self::Repeat => SettingKind::Choice(REPEAT_CHOICES),
            Self::ResumeMinRemainingMs => SettingKind::Slider {
                min: 0.0,
                max: 300_000.0,
                step: 5_000.0,
            },
            Self::DefaultSpeed => SettingKind::Slider {
                min: 0.25,
                max: 4.0,
                step: 0.25,
            },
            Self::AspectRatio => SettingKind::Choice(RATIO_CHOICES),
            Self::Deinterlace | Self::SkipLoopFilter => SettingKind::Choice(ON_OFF_AUTO),
            Self::Zoom => SettingKind::Slider {
                min: 0.25,
                max: 4.0,
                step: 0.05,
            },
            Self::Brightness | Self::Contrast | Self::Saturation => SettingKind::Slider {
                min: 0.0,
                max: 2.0,
                step: 0.05,
            },
            Self::Equalizer => SettingKind::Choice(EQ_CHOICES),
            Self::SubtitleScale => SettingKind::Slider {
                min: 0.5,
                max: 2.0,
                step: 0.05,
            },
            Self::SubtitleEncoding => SettingKind::Choice(ENCODING_CHOICES),
            Self::SubtitlePosition => SettingKind::Slider {
                min: 0.0,
                max: 1.0,
                step: 0.02,
            },
            Self::SubtitleSearchLanguage => SettingKind::Choice(SUB_LANGUAGE_CHOICES),
            Self::LibraryFolder | Self::SnapshotFolder | Self::ExtensionsFolder => {
                SettingKind::Folder
            }
            Self::LibrarySort => SettingKind::Choice(SORT_CHOICES),
            Self::NetworkTimeoutMs => SettingKind::Slider {
                min: 1_000.0,
                max: 60_000.0,
                step: 1_000.0,
            },
            Self::NetworkCachingMs => SettingKind::Slider {
                min: 0.0,
                max: 10_000.0,
                step: 500.0,
            },
            Self::SnapshotFormat => SettingKind::Choice(IMAGE_FORMAT_CHOICES),
            Self::SnapshotPrefix => SettingKind::Text { secret: false },
        }
    }

    pub fn label_key(self) -> &'static str {
        // The label key is derived from the dotted key so the two can never drift.
        // `settings.<group>.<name>.label`
        self.keyed("label")
    }

    pub fn help_key(self) -> &'static str {
        self.keyed("help")
    }

    fn keyed(self, suffix: &'static str) -> &'static str {
        // A small const table is cheaper and safer than runtime string building.
        match (self, suffix) {
            (Self::Theme, "label") => "settings.general.theme.label",
            (Self::Theme, _) => "settings.general.theme.help",
            (Self::Language, "label") => "settings.general.language.label",
            (Self::Language, _) => "settings.general.language.help",
            (Self::Accent, "label") => "settings.general.accent.label",
            (Self::Accent, _) => "settings.general.accent.help",
            (Self::RememberWindow, "label") => "settings.general.remember-window.label",
            (Self::RememberWindow, _) => "settings.general.remember-window.help",
            (Self::Osd, "label") => "settings.interface.osd.label",
            (Self::Osd, _) => "settings.interface.osd.help",
            (Self::Waveform, "label") => "settings.interface.waveform.label",
            (Self::Waveform, _) => "settings.interface.waveform.help",
            (Self::SidebarTab, "label") => "settings.interface.sidebar-tab.label",
            (Self::SidebarTab, _) => "settings.interface.sidebar-tab.help",
            (Self::StartPage, "label") => "settings.interface.start-page.label",
            (Self::StartPage, _) => "settings.interface.start-page.help",
            (Self::RememberRecent, "label") => "settings.privacy.remember-recent.label",
            (Self::RememberRecent, _) => "settings.privacy.remember-recent.help",
            (Self::ClearRecentOnExit, "label") => "settings.privacy.clear-recent-on-exit.label",
            (Self::ClearRecentOnExit, _) => "settings.privacy.clear-recent-on-exit.help",
            (Self::OpenSubtitlesKey, "label") => "settings.privacy.opensubtitles-key.label",
            (Self::OpenSubtitlesKey, _) => "settings.privacy.opensubtitles-key.help",
            (Self::Diagnostics, "label") => "settings.privacy.diagnostics.label",
            (Self::Diagnostics, _) => "settings.privacy.diagnostics.help",
            (Self::SeekStepMs, "label") => "settings.controls.seek-step.label",
            (Self::SeekStepMs, _) => "settings.controls.seek-step.help",
            (Self::VolumeStep, "label") => "settings.controls.volume-step.label",
            (Self::VolumeStep, _) => "settings.controls.volume-step.help",
            (Self::WheelAction, "label") => "settings.controls.wheel.label",
            (Self::WheelAction, _) => "settings.controls.wheel.help",
            (Self::ClickPause, "label") => "settings.controls.click-pause.label",
            (Self::ClickPause, _) => "settings.controls.click-pause.help",
            (Self::DoubleClick, "label") => "settings.controls.double-click.label",
            (Self::DoubleClick, _) => "settings.controls.double-click.help",
            (Self::Volume, "label") => "settings.playback.volume.label",
            (Self::Volume, _) => "settings.playback.volume.help",
            (Self::Repeat, "label") => "settings.playback.repeat.label",
            (Self::Repeat, _) => "settings.playback.repeat.help",
            (Self::Shuffle, "label") => "settings.playback.shuffle.label",
            (Self::Shuffle, _) => "settings.playback.shuffle.help",
            (Self::RememberPosition, "label") => "settings.playback.resume.label",
            (Self::RememberPosition, _) => "settings.playback.resume.help",
            (Self::ResumeMinRemainingMs, "label") => "settings.playback.resume-min-remaining.label",
            (Self::ResumeMinRemainingMs, _) => "settings.playback.resume-min-remaining.help",
            (Self::AutoAdvance, "label") => "settings.playback.auto-advance.label",
            (Self::AutoAdvance, _) => "settings.playback.auto-advance.help",
            (Self::StartPaused, "label") => "settings.playback.start-paused.label",
            (Self::StartPaused, _) => "settings.playback.start-paused.help",
            (Self::DefaultSpeed, "label") => "settings.playback.default-speed.label",
            (Self::DefaultSpeed, _) => "settings.playback.default-speed.help",
            (Self::AspectRatio, "label") => "settings.video.aspect-ratio.label",
            (Self::AspectRatio, _) => "settings.video.aspect-ratio.help",
            (Self::Deinterlace, "label") => "settings.video.deinterlace.label",
            (Self::Deinterlace, _) => "settings.video.deinterlace.help",
            (Self::Zoom, "label") => "settings.video.zoom.label",
            (Self::Zoom, _) => "settings.video.zoom.help",
            (Self::Brightness, "label") => "settings.video.brightness.label",
            (Self::Brightness, _) => "settings.video.brightness.help",
            (Self::Contrast, "label") => "settings.video.contrast.label",
            (Self::Contrast, _) => "settings.video.contrast.help",
            (Self::Saturation, "label") => "settings.video.saturation.label",
            (Self::Saturation, _) => "settings.video.saturation.help",
            (Self::HardwareDecoding, "label") => "settings.video.hardware-decoding.label",
            (Self::HardwareDecoding, _) => "settings.video.hardware-decoding.help",
            (Self::Equalizer, "label") => "settings.audio.equalizer.label",
            (Self::Equalizer, _) => "settings.audio.equalizer.help",
            (Self::PreferOriginalAudio, "label") => "settings.audio.prefer-original.label",
            (Self::PreferOriginalAudio, _) => "settings.audio.prefer-original.help",
            (Self::MuteOnStart, "label") => "settings.audio.mute-on-start.label",
            (Self::MuteOnStart, _) => "settings.audio.mute-on-start.help",
            (Self::SubtitleAutoLoad, "label") => "settings.subtitles.auto-load.label",
            (Self::SubtitleAutoLoad, _) => "settings.subtitles.auto-load.help",
            (Self::SubtitleScale, "label") => "settings.subtitles.scale.label",
            (Self::SubtitleScale, _) => "settings.subtitles.scale.help",
            (Self::SubtitleEncoding, "label") => "settings.subtitles.encoding.label",
            (Self::SubtitleEncoding, _) => "settings.subtitles.encoding.help",
            (Self::SubtitleOutline, "label") => "settings.subtitles.outline.label",
            (Self::SubtitleOutline, _) => "settings.subtitles.outline.help",
            (Self::SubtitlePosition, "label") => "settings.subtitles.position.label",
            (Self::SubtitlePosition, _) => "settings.subtitles.position.help",
            (Self::SubtitleSearchLanguage, "label") => "settings.subtitles.search-language.label",
            (Self::SubtitleSearchLanguage, _) => "settings.subtitles.search-language.help",
            (Self::LibraryFolder, "label") => "settings.library.folder.label",
            (Self::LibraryFolder, _) => "settings.library.folder.help",
            (Self::LibraryScanOnStart, "label") => "settings.library.scan-on-start.label",
            (Self::LibraryScanOnStart, _) => "settings.library.scan-on-start.help",
            (Self::LibrarySort, "label") => "settings.library.sort.label",
            (Self::LibrarySort, _) => "settings.library.sort.help",
            (Self::NetworkTimeoutMs, "label") => "settings.network.timeout.label",
            (Self::NetworkTimeoutMs, _) => "settings.network.timeout.help",
            (Self::NetworkCachingMs, "label") => "settings.network.caching.label",
            (Self::NetworkCachingMs, _) => "settings.network.caching.help",
            (Self::SkipLoopFilter, "label") => "settings.performance.skip-loop-filter.label",
            (Self::SkipLoopFilter, _) => "settings.performance.skip-loop-filter.help",
            (Self::SnapshotFolder, "label") => "settings.recording.snapshot-folder.label",
            (Self::SnapshotFolder, _) => "settings.recording.snapshot-folder.help",
            (Self::SnapshotFormat, "label") => "settings.recording.snapshot-format.label",
            (Self::SnapshotFormat, _) => "settings.recording.snapshot-format.help",
            (Self::SnapshotPrefix, "label") => "settings.recording.snapshot-prefix.label",
            (Self::SnapshotPrefix, _) => "settings.recording.snapshot-prefix.help",
            (Self::ExtensionsFolder, "label") => "settings.extensions.folder.label",
            (Self::ExtensionsFolder, _) => "settings.extensions.folder.help",
        }
    }
}

impl Choice {
    /// Option label key: an explicit key wins, otherwise `opt.<id>`.
    pub fn label_key(&self) -> &'static str {
        if self.label.is_empty() {
            match self.id {
                "system" => "opt.system",
                "dark" => "opt.dark",
                "light" => "opt.light",
                "en" => "opt.en",
                "ar" => "opt.ar",
                "fr" => "opt.fr",
                "de" => "opt.de",
                "player" => "opt.player",
                "library" => "opt.library",
                "volume" => "opt.volume",
                "seek" => "opt.seek",
                "fullscreen" => "opt.fullscreen",
                "play" => "opt.play",
                "off" => "opt.off",
                "on" => "opt.on",
                "auto" => "opt.auto",
                "all" => "opt.all",
                "one" => "opt.one",
                "16:9" => "opt.ratio-16-9",
                "4:3" => "opt.ratio-4-3",
                "1:1" => "opt.ratio-1-1",
                "21:9" => "opt.ratio-21-9",
                "utf-8" => "opt.utf8",
                "cp1256" => "opt.cp1256",
                "cp1252" => "opt.cp1252",
                "latin1" => "opt.latin1",
                "flat" => "opt.flat",
                "rock" => "opt.rock",
                "pop" => "opt.pop",
                "jazz" => "opt.jazz",
                "classical" => "opt.classical",
                "electronic" => "opt.electronic",
                "vocal" => "opt.vocal",
                "bass" => "opt.bass",
                "png" => "opt.png",
                "jpg" => "opt.jpg",
                _ => "opt.unknown",
            }
        } else {
            self.label
        }
    }
}

impl Settings {
    pub fn value(&self, id: SettingId) -> SettingValue {
        use SettingValue as V;
        match id {
            SettingId::Theme => V::Choice(self.theme.id()),
            SettingId::Language => V::Choice(self.language.id()),
            SettingId::Accent => V::Text(self.accent.clone()),
            SettingId::RememberWindow => V::Bool(self.remember_window),
            SettingId::Osd => V::Bool(self.osd),
            SettingId::Waveform => V::Bool(self.waveform),
            SettingId::SidebarTab => V::Number(self.sidebar_tab as f64),
            SettingId::StartPage => V::Choice(self.start_page.id()),
            SettingId::RememberRecent => V::Bool(self.remember_recent),
            SettingId::ClearRecentOnExit => V::Bool(self.clear_recent_on_exit),
            SettingId::OpenSubtitlesKey => V::Text(self.opensubtitles_api_key.clone()),
            SettingId::Diagnostics => V::Bool(self.diagnostics),
            SettingId::SeekStepMs => V::Number(self.seek_step_ms as f64),
            SettingId::VolumeStep => V::Number(self.volume_step as f64),
            SettingId::WheelAction => V::Choice(self.wheel_action.id()),
            SettingId::ClickPause => V::Bool(self.click_pause),
            SettingId::DoubleClick => V::Choice(self.double_click.id()),
            SettingId::Volume => V::Number(self.volume as f64),
            SettingId::Repeat => V::Choice(match self.repeat {
                RepeatMode::Off => "off",
                RepeatMode::All => "all",
                RepeatMode::One => "one",
            }),
            SettingId::Shuffle => V::Bool(self.shuffle),
            SettingId::RememberPosition => V::Bool(self.remember_position),
            SettingId::ResumeMinRemainingMs => V::Number(self.resume_min_remaining_ms as f64),
            SettingId::AutoAdvance => V::Bool(self.auto_advance),
            SettingId::StartPaused => V::Bool(self.start_paused),
            SettingId::DefaultSpeed => V::Number(self.default_speed),
            SettingId::AspectRatio => V::Choice(match_choice(RATIO_CHOICES, &self.aspect_ratio)),
            SettingId::Deinterlace => V::Choice(self.deinterlace.id()),
            SettingId::Zoom => V::Number(self.zoom),
            SettingId::Brightness => V::Number(self.brightness),
            SettingId::Contrast => V::Number(self.contrast),
            SettingId::Saturation => V::Number(self.saturation),
            SettingId::HardwareDecoding => V::Bool(self.hardware_decoding),
            SettingId::Equalizer => V::Choice(match_choice(EQ_CHOICES, &self.equalizer)),
            SettingId::PreferOriginalAudio => V::Bool(self.prefer_original_audio),
            SettingId::MuteOnStart => V::Bool(self.mute_on_start),
            SettingId::SubtitleAutoLoad => V::Bool(self.subtitle_auto_load),
            SettingId::SubtitleScale => V::Number(self.subtitle_scale),
            SettingId::SubtitleEncoding => {
                V::Choice(match_choice(ENCODING_CHOICES, &self.subtitle_encoding))
            }
            SettingId::SubtitleOutline => V::Bool(self.subtitle_outline),
            SettingId::SubtitlePosition => V::Number(self.subtitle_position),
            SettingId::SubtitleSearchLanguage => V::Choice(match_choice(
                SUB_LANGUAGE_CHOICES,
                &self.subtitle_search_language,
            )),
            SettingId::LibraryFolder => V::Text(self.library_folder.clone()),
            SettingId::LibraryScanOnStart => V::Bool(self.library_scan_on_start),
            SettingId::LibrarySort => V::Choice(self.library_sort.id()),
            SettingId::NetworkTimeoutMs => V::Number(self.network_timeout_ms as f64),
            SettingId::NetworkCachingMs => V::Number(self.network_caching_ms as f64),
            SettingId::SkipLoopFilter => V::Choice(self.skip_loop_filter.id()),
            SettingId::SnapshotFolder => V::Text(self.snapshot_folder.clone()),
            SettingId::SnapshotFormat => {
                V::Choice(match_choice(IMAGE_FORMAT_CHOICES, &self.snapshot_format))
            }
            SettingId::SnapshotPrefix => V::Text(self.snapshot_prefix.clone()),
            SettingId::ExtensionsFolder => V::Text(self.extensions_folder.clone()),
        }
    }

    /// Write a value back. Returns `false` when the value does not fit the setting's kind,
    /// so a malformed import can be rejected without touching the store.
    pub fn set_value(&mut self, id: SettingId, value: &SettingValue) -> bool {
        match id {
            SettingId::Theme => self.theme = Theme::from_id(value.as_choice()),
            SettingId::Language => self.language = Language::from_id(value.as_choice()),
            SettingId::Accent => self.accent = value.as_text(),
            SettingId::RememberWindow => self.remember_window = value.as_bool(),
            SettingId::Osd => self.osd = value.as_bool(),
            SettingId::Waveform => self.waveform = value.as_bool(),
            SettingId::SidebarTab => self.sidebar_tab = value.as_number().max(0.0) as u32,
            SettingId::StartPage => self.start_page = StartPage::from_id(value.as_choice()),
            SettingId::RememberRecent => self.remember_recent = value.as_bool(),
            SettingId::ClearRecentOnExit => self.clear_recent_on_exit = value.as_bool(),
            SettingId::OpenSubtitlesKey => self.opensubtitles_api_key = value.as_text(),
            SettingId::Diagnostics => self.diagnostics = value.as_bool(),
            SettingId::SeekStepMs => self.seek_step_ms = value.as_number() as i64,
            SettingId::VolumeStep => self.volume_step = value.as_number() as i32,
            SettingId::WheelAction => self.wheel_action = WheelAction::from_id(value.as_choice()),
            SettingId::ClickPause => self.click_pause = value.as_bool(),
            SettingId::DoubleClick => {
                self.double_click = DoubleClickAction::from_id(value.as_choice())
            }
            SettingId::Volume => self.volume = value.as_number() as i32,
            SettingId::Repeat => {
                self.repeat = match value.as_choice() {
                    "all" => RepeatMode::All,
                    "one" => RepeatMode::One,
                    _ => RepeatMode::Off,
                }
            }
            SettingId::Shuffle => self.shuffle = value.as_bool(),
            SettingId::RememberPosition => self.remember_position = value.as_bool(),
            SettingId::ResumeMinRemainingMs => {
                self.resume_min_remaining_ms = value.as_number() as i64
            }
            SettingId::AutoAdvance => self.auto_advance = value.as_bool(),
            SettingId::StartPaused => self.start_paused = value.as_bool(),
            SettingId::DefaultSpeed => self.default_speed = value.as_number(),
            SettingId::AspectRatio => {
                // libVLC wants NULL for "let the file decide", which we store as "".
                let picked = value.as_choice();
                self.aspect_ratio = if picked == "auto" {
                    String::new()
                } else {
                    picked.to_string()
                };
            }
            SettingId::Deinterlace => self.deinterlace = Deinterlace::from_id(value.as_choice()),
            SettingId::Zoom => self.zoom = value.as_number(),
            SettingId::Brightness => self.brightness = value.as_number(),
            SettingId::Contrast => self.contrast = value.as_number(),
            SettingId::Saturation => self.saturation = value.as_number(),
            SettingId::HardwareDecoding => self.hardware_decoding = value.as_bool(),
            SettingId::Equalizer => self.equalizer = value.as_text(),
            SettingId::PreferOriginalAudio => self.prefer_original_audio = value.as_bool(),
            SettingId::MuteOnStart => self.mute_on_start = value.as_bool(),
            SettingId::SubtitleAutoLoad => self.subtitle_auto_load = value.as_bool(),
            SettingId::SubtitleScale => self.subtitle_scale = value.as_number(),
            SettingId::SubtitleEncoding => {
                let picked = value.as_choice();
                self.subtitle_encoding = if picked == "auto" {
                    String::new()
                } else {
                    picked.to_string()
                };
            }
            SettingId::SubtitleOutline => self.subtitle_outline = value.as_bool(),
            SettingId::SubtitlePosition => self.subtitle_position = value.as_number(),
            SettingId::SubtitleSearchLanguage => self.subtitle_search_language = value.as_text(),
            SettingId::LibraryFolder => self.library_folder = value.as_text(),
            SettingId::LibraryScanOnStart => self.library_scan_on_start = value.as_bool(),
            SettingId::LibrarySort => self.library_sort = SortMode::from_id(value.as_choice()),
            SettingId::NetworkTimeoutMs => self.network_timeout_ms = value.as_number() as i64,
            SettingId::NetworkCachingMs => self.network_caching_ms = value.as_number() as i64,
            SettingId::SkipLoopFilter => {
                self.skip_loop_filter = SkipLoopFilter::from_id(value.as_choice())
            }
            SettingId::SnapshotFolder => self.snapshot_folder = value.as_text(),
            SettingId::SnapshotFormat => self.snapshot_format = value.as_text(),
            SettingId::SnapshotPrefix => self.snapshot_prefix = value.as_text(),
            SettingId::ExtensionsFolder => self.extensions_folder = value.as_text(),
        }
        self.clamp();
        true
    }

    /// Restore one setting to the value a fresh install would have.
    pub fn reset_value(&mut self, id: SettingId) {
        let default = Settings::default();
        let value = default.value(id);
        self.set_value(id, &value);
    }

    /// Dotted-key form of [`Settings::value`], for import/export and the debug overlay.
    pub fn value_of_key(&self, key: &str) -> Option<SettingValue> {
        SettingId::from_key(key).map(|id| self.value(id))
    }
}

/// Settings in a category, in declaration order.
pub fn in_category(category: Category) -> Vec<SettingId> {
    SettingId::ALL
        .iter()
        .copied()
        .filter(|id| id.category() == category)
        .collect()
}

/// Every text a row shows, so search can match labels, help, keys and option names.
fn haystack(id: SettingId, text: &impl Fn(&str) -> String) -> Vec<String> {
    let mut parts = vec![
        text(id.label_key()).to_lowercase(),
        text(id.help_key()).to_lowercase(),
        id.key().to_lowercase(),
        id.key().replace('-', " ").to_lowercase(),
    ];
    if let SettingKind::Choice(options) = id.kind() {
        for option in options {
            parts.push(text(option.label_key()).to_lowercase());
            parts.push(option.id.to_lowercase());
        }
    }
    parts
}

/// Search every setting. All whitespace-separated tokens must appear somewhere in the
/// row (label, help, key or option names), which matches how the search field behaves.
pub fn search(text: &impl Fn(&str) -> String, query: &str) -> Vec<SettingId> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return SettingId::ALL.to_vec();
    }
    SettingId::ALL
        .iter()
        .copied()
        .filter(|id| {
            let parts = haystack(*id, text);
            tokens
                .iter()
                .all(|token| parts.iter().any(|part| part.contains(token)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en(key: &str) -> String {
        // Stand-in catalog: the key itself, so search tests exercise the mechanism
        // without depending on the app's i18n tables.
        key.replace(['.', '-'], " ")
    }

    #[test]
    fn keys_are_unique_and_parse_back() {
        let mut seen = std::collections::HashSet::new();
        for id in SettingId::ALL {
            assert!(seen.insert(id.key()), "duplicate key {}", id.key());
            assert_eq!(SettingId::from_key(id.key()), Some(*id));
        }
    }

    #[test]
    fn every_category_has_at_least_one_setting() {
        for category in Category::ALL {
            assert!(
                !in_category(*category).is_empty(),
                "category {} is empty",
                category.key()
            );
        }
    }

    #[test]
    fn every_setting_round_trips_its_default() {
        for id in SettingId::ALL {
            let mut s = Settings::default();
            let default = s.value(*id);
            // Perturb, then restore: a setter that ignores the value would fail here.
            let other = match default {
                SettingValue::Bool(b) => SettingValue::Bool(!b),
                SettingValue::Number(n) => SettingValue::Number(n + 1.0),
                SettingValue::Text(_) => SettingValue::Text("changed".into()),
                SettingValue::Choice(c) => {
                    if let SettingKind::Choice(options) = id.kind() {
                        let next = options
                            .iter()
                            .find(|o| o.id != c)
                            .map(|o| o.id)
                            .unwrap_or(c);
                        SettingValue::Choice(next)
                    } else {
                        SettingValue::Choice(c)
                    }
                }
            };
            assert!(s.set_value(*id, &other), "{} rejected a value", id.key());
            s.reset_value(*id);
            assert_eq!(s.value(*id), default, "{} did not reset", id.key());
        }
    }

    #[test]
    fn choice_values_are_declared_options() {
        for id in SettingId::ALL {
            let SettingKind::Choice(options) = id.kind() else {
                continue;
            };
            let s = Settings::default();
            let value = s.value(*id);
            let chosen = value.as_choice();
            assert!(
                options.iter().any(|o| o.id == chosen),
                "{} default {chosen} is not one of its options",
                id.key()
            );
        }
    }

    #[test]
    fn slider_defaults_sit_inside_their_range() {
        for id in SettingId::ALL {
            let SettingKind::Slider { min, max, step } = id.kind() else {
                continue;
            };
            let default = Settings::default().value(*id).as_number();
            assert!(
                default >= min && default <= max,
                "{} default out of range",
                id.key()
            );
            assert!(step > 0.0, "{} has a non-positive step", id.key());
        }
    }

    #[test]
    fn advanced_settings_are_a_subset() {
        let advanced = SettingId::ALL.iter().filter(|id| id.advanced()).count();
        assert!(advanced > 0 && advanced < SettingId::ALL.len());
    }

    #[test]
    fn search_matches_label_and_options() {
        assert!(search(&en, "theme").contains(&SettingId::Theme));
        // "dark" is only an option name, never a label.
        assert!(search(&en, "dark").contains(&SettingId::Theme));
        // Multi-token queries narrow rather than widen.
        assert_eq!(search(&en, "playback volume"), vec![SettingId::Volume]);
        assert!(search(&en, "zzzz-nothing").is_empty());
        assert_eq!(search(&en, "   ").len(), SettingId::ALL.len());
    }

    #[test]
    fn value_of_key_is_the_keyed_accessor() {
        let s = Settings::default();
        assert_eq!(
            s.value_of_key("general.theme"),
            Some(SettingValue::Choice("system"))
        );
        assert_eq!(s.value_of_key("nope.nope"), None);
    }
}
