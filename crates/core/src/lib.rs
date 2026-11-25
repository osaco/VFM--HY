#![forbid(unsafe_code)]

//! Platform-agnostic state, timecode and telemetry math for Vedio for me (VFM).
//! This crate never calls into an OS or into libVLC; it is trivially unit-testable.

pub mod bookmarks;
pub mod hotkeys;
pub mod layout;
pub mod library;
pub mod machine;
pub mod open_media;
pub mod opensubtitles;
pub mod playlist;
pub mod presets;
pub mod progress;
pub mod queue;
pub mod seek_thumbs;
pub mod settings;
pub mod settings_schema;
pub mod state;
pub mod store;
pub mod subtitles;
pub mod telemetry;
pub mod time;
pub mod transform;
pub mod waveform;

pub use bookmarks::{
    add as add_bookmark, closest as closest_bookmark, list as list_bookmarks,
    neighbour as neighbour_bookmark, remove as remove_bookmark, rename as rename_bookmark,
    Bookmark, BookmarkError, DEDUPE_TOLERANCE_MS,
};
pub use hotkeys::{
    apply_binding, default_bindings, find_conflicts, normalize_combo, Action, Binding, Conflict,
    HotkeyConfig, KEYS_VERSION,
};
pub use library::{
    parse_m3u, FolderScanner, LibraryEntry, LibraryFilter, MediaKind, Page, ScanCancellation,
    ScanOptions, SortMode, StreamEntry, ThumbnailCache, ViewMode, MUSIC_EXTENSIONS,
    VIDEO_EXTENSIONS,
};
pub use machine::{CoreEvent, PlayerAction, PlayerStateMachine};
pub use opensubtitles::{
    error_for_status as subtitle_api_error_for_status, hash_search_url, movie_hash,
    name_search_url, parse_download_response as parse_subtitle_download_response,
    parse_search_response as parse_subtitle_search_response, DownloadTicket, OpenSubtitlesClient,
    SubtitleApiError, SubtitleHit, API_ROOT as OPENSUBTITLES_API_ROOT,
    DEFAULT_USER_AGENT as OPENSUBTITLES_USER_AGENT,
};
pub use playlist::Playlist;
pub use presets::{
    load_all as load_presets, remove as remove_preset, rename as rename_preset, resolve_for_media,
    save as save_preset, AudioPreset, PlaybackPreset, Preset, PresetError, PresetPayload,
    PresetScope, SubtitlePreset, VideoPreset,
};
pub use progress::{should_resume, WatchProgress};
pub use queue::{MediaItem, Queue, RepeatMode};
pub use seek_thumbs::{bucket_position, encode_rgba_png, GrabCancellation, SeekThumbnailCache};
pub use settings::{
    Deinterlace, DoubleClickAction, Language, Settings, SettingsError, SettingsStore,
    SkipLoopFilter, StartPage, Theme, WheelAction, WindowState,
};
pub use settings_schema::{
    in_category as settings_in_category, search as search_settings, Category as SettingCategory,
    Choice as SettingChoice, SettingId, SettingKind, SettingValue,
};
pub use state::{PlaybackState, PlayerSnapshot};
pub use store::LibraryStore;
pub use subtitles::{
    cue_at, decode_subtitle_bytes, detect_encoding, parse, parse_ass, parse_srt, parse_webvtt,
    shift_cues, subtitle_matches_video, Cue, SubtitleFormat, SubtitleSession, SubtitleStyle,
    TimedCue,
};
pub use telemetry::{TelemetryAccumulator, TelemetrySnapshot};
pub use time::{format_timecode, format_timecode_long, parse_timecode};
pub use transform::VideoTransform;
pub use waveform::{
    analyse as analyse_waveform, downsample as downsample_waveform, Waveform, WaveformCache,
    WaveformError, DEFAULT_BUCKETS as DEFAULT_WAVEFORM_BUCKETS,
};
