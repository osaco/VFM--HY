//! Phase 6 glue between the preset engine, the right sidebar and libVLC.
//! Slider values are normalised 0..1; real units live here so the UI never hard-codes
//! magic ranges and presets round-trip through the same conversions.

use slint::{ModelRc, SharedString, VecModel};
use vfm_core::{
    AudioPreset, Language, LibraryStore, PlaybackPreset, Preset, PresetPayload, SubtitlePreset,
    VideoPreset,
};

use crate::{i18n, PlayerWindow, PresetRow, SidebarState};

fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn normalise(value: f32, min: f32, max: f32) -> f32 {
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

pub fn delay_ms(value: f32) -> i64 {
    ((clamp01(value) * 2.0 - 1.0) * 2000.0).round() as i64
}

pub fn delay_slider(ms: i64) -> f32 {
    normalise(ms.clamp(-2000, 2000) as f32, -2000.0, 2000.0)
}

pub fn audio_delay_ms(value: f32) -> i64 {
    ((clamp01(value) * 2.0 - 1.0) * 1000.0).round() as i64
}

pub fn audio_delay_slider(ms: i64) -> f32 {
    normalise(ms.clamp(-1000, 1000) as f32, -1000.0, 1000.0)
}

pub fn size_scale(value: f32) -> f32 {
    0.5 + clamp01(value) * 1.5
}

pub fn size_slider(scale: f32) -> f32 {
    normalise(scale, 0.5, 2.0)
}

pub fn precise_px(value: f32) -> f32 {
    12.0 + clamp01(value) * 84.0
}

pub fn precise_slider(px: f32) -> f32 {
    normalise(px, 12.0, 96.0)
}

pub fn position(value: f32) -> f32 {
    clamp01(value)
}

pub fn outline(value: f32) -> f32 {
    clamp01(value) * 4.0
}

pub fn outline_slider(px: f32) -> f32 {
    normalise(px, 0.0, 4.0)
}

pub fn brightness(value: f32) -> f32 {
    clamp01(value) * 2.0
}

pub fn brightness_slider(value: f32) -> f32 {
    normalise(value, 0.0, 2.0)
}

pub fn contrast(value: f32) -> f32 {
    clamp01(value) * 2.0
}

pub fn contrast_slider(value: f32) -> f32 {
    normalise(value, 0.0, 2.0)
}

pub fn saturation(value: f32) -> f32 {
    clamp01(value) * 3.0
}

pub fn saturation_slider(value: f32) -> f32 {
    normalise(value, 0.0, 3.0)
}

pub fn hue(value: f32) -> f32 {
    clamp01(value) * 360.0 - 180.0
}

pub fn hue_slider(value: f32) -> f32 {
    normalise(value, -180.0, 180.0)
}

pub fn gamma(value: f32) -> f32 {
    clamp01(value) * 2.0
}

pub fn gamma_slider(value: f32) -> f32 {
    normalise(value, 0.0, 2.0)
}

pub fn zoom(value: f32) -> f32 {
    0.5 + clamp01(value) * 2.5
}

pub fn zoom_slider(value: f32) -> f32 {
    normalise(value, 0.5, 3.0)
}

pub fn speed(value: f32) -> f32 {
    0.25 + clamp01(value) * 3.75
}

pub fn speed_slider(value: f32) -> f32 {
    normalise(value, 0.25, 4.0)
}

pub fn sleep_minutes(value: f32) -> u32 {
    (clamp01(value) * 24.0).round() as u32 * 5
}

pub fn sleep_slider(minutes: u32) -> f32 {
    normalise(minutes.min(120) as f32, 0.0, 120.0)
}

fn s(text: impl Into<String>) -> SharedString {
    SharedString::from(text.into())
}

pub fn default_state() -> SidebarState {
    let language = i18n::ui_language();
    let mut state = SidebarState {
        show: false,
        tab: 0,
        eq_index: 0,
        aspect_index: 0,
        audio_device_index: 0,
        rotation_index: 0,
        active_preset_index: -1,
        preset_name: s(""),
        preset_scope_label: s(i18n::translate(language, "preset.scope-global")),
        subtitle_font_name: s("Inter"),
        subtitle_language_name: s("Auto"),
        audio_filters_label: s("—"),
        subtitle_delay: delay_slider(0),
        subtitle_size: size_slider(1.0),
        subtitle_position: 0.1,
        subtitle_outline: outline_slider(1.0),
        subtitle_delay_label: s(""),
        subtitle_size_label: s(""),
        subtitle_position_label: s(""),
        subtitle_outline_label: s(""),
        subtitle_relative: true,
        subtitle_timeline: true,
        audio_delay: audio_delay_slider(0),
        audio_delay_label: s(""),
        audio_normalize: false,
        audio_surround: false,
        brightness: brightness_slider(1.0),
        contrast: contrast_slider(1.0),
        saturation: saturation_slider(1.0),
        hue: hue_slider(0.0),
        gamma: gamma_slider(1.0),
        zoom: zoom_slider(1.0),
        flip_h: false,
        flip_v: false,
        brightness_label: s(""),
        contrast_label: s(""),
        saturation_label: s(""),
        hue_label: s(""),
        gamma_label: s(""),
        zoom_label: s(""),
        speed: speed_slider(1.0),
        sleep_minutes: sleep_slider(0),
        speed_label: s(""),
        sleep_label: s(""),
        waveform: false,
        metadata: false,
    };
    refresh_labels(&mut state);
    state
}

/// Recompute all human-readable value labels after a slider or preset change.
pub fn refresh_labels(state: &mut SidebarState) {
    let language = i18n::ui_language();
    let size = if state.subtitle_relative {
        format!("{:.2}×", size_scale(state.subtitle_size))
    } else {
        format!("{:.0} px", precise_px(state.subtitle_size))
    };
    state.subtitle_delay_label = s(format!(
        "{:+0.2} s",
        delay_ms(state.subtitle_delay) as f32 / 1000.0
    ));
    state.subtitle_size_label = s(size);
    state.subtitle_position_label = s(format!("{:.0}%", position(state.subtitle_position) * 100.0));
    state.subtitle_outline_label = s(format!("{:.1} px", outline(state.subtitle_outline)));
    state.audio_delay_label = s(format!(
        "{:+0.2} s",
        audio_delay_ms(state.audio_delay) as f32 / 1000.0
    ));
    state.brightness_label = s(format!("{:.2}", brightness(state.brightness)));
    state.contrast_label = s(format!("{:.2}", contrast(state.contrast)));
    state.saturation_label = s(format!("{:.2}", saturation(state.saturation)));
    state.hue_label = s(format!("{:.0}°", hue(state.hue)));
    state.gamma_label = s(format!("{:.2}", gamma(state.gamma)));
    state.zoom_label = s(format!("{:.2}×", zoom(state.zoom)));
    state.speed_label = s(format!("{:.2}×", speed(state.speed)));
    state.sleep_label = if sleep_minutes(state.sleep_minutes) == 0 {
        s(i18n::translate(language, "tracks.off"))
    } else {
        s(format!("{} min", sleep_minutes(state.sleep_minutes)))
    };
    let mut filters = Vec::new();
    if state.audio_normalize {
        filters.push(i18n::translate(language, "sidebar.audio.normalize"));
    }
    if state.audio_surround {
        filters.push(i18n::translate(language, "sidebar.audio.surround"));
    }
    state.audio_filters_label = s(if filters.is_empty() {
        i18n::translate(language, "tracks.off").to_string()
    } else {
        filters.join(", ")
    });
}

/// Human-readable scope label: where a preset applies, and where a new one lands.
pub fn scope_label(scope: &vfm_core::PresetScope, language: Language) -> String {
    match scope {
        vfm_core::PresetScope::Global => {
            i18n::translate(language, "preset.scope-global").to_string()
        }
        vfm_core::PresetScope::Folder(path) => path.to_string_lossy().into_owned(),
        vfm_core::PresetScope::Playlist(name) => name.clone(),
    }
}

/// Scope a newly saved preset gets for the media that is currently open.
pub fn scope_for_media(path: &std::path::Path) -> vfm_core::PresetScope {
    if path.exists() {
        if let Some(parent) = path.parent() {
            return vfm_core::PresetScope::Folder(parent.to_path_buf());
        }
    }
    vfm_core::PresetScope::Global
}

pub fn snapshot_payload(state: &SidebarState) -> PresetPayload {
    PresetPayload {
        subtitle: SubtitlePreset {
            enabled: true,
            delay_ms: delay_ms(state.subtitle_delay),
            size_scale: size_scale(state.subtitle_size),
            precise_size: !state.subtitle_relative,
            precise_size_px: precise_px(state.subtitle_size),
            position: position(state.subtitle_position),
            outline: outline(state.subtitle_outline),
            default_language: state.subtitle_language_name.to_string(),
        },
        audio: AudioPreset {
            delay_ms: audio_delay_ms(state.audio_delay),
            default_language: "Auto".into(),
            equalizer: AudioPreset::EQ_LABELS
                .get(state.eq_index.max(0) as usize)
                .copied()
                .unwrap_or("Flat")
                .into(),
            normalize_volume: state.audio_normalize,
            headphone_surround: state.audio_surround,
        },
        video: VideoPreset {
            brightness: brightness(state.brightness),
            contrast: contrast(state.contrast),
            saturation: saturation(state.saturation),
            hue: hue(state.hue),
            gamma: gamma(state.gamma),
            zoom: zoom(state.zoom),
            aspect_ratio: VideoPreset::ASPECT_LABELS
                .get(state.aspect_index.max(0) as usize)
                .copied()
                .unwrap_or("Auto")
                .into(),
            rotate_degrees: state.rotation_index.max(0) * 90,
            flip_horizontal: state.flip_h,
            flip_vertical: state.flip_v,
        },
        playback: PlaybackPreset {
            speed: speed(state.speed),
            sleep_minutes: sleep_minutes(state.sleep_minutes),
            show_subtitle_timeline: state.subtitle_timeline,
            show_waveform: state.waveform,
            show_metadata: state.metadata,
        },
    }
}

pub fn apply_payload_to_state(state: &mut SidebarState, payload: &PresetPayload) {
    let relative = !payload.subtitle.precise_size;
    state.subtitle_delay = delay_slider(payload.subtitle.delay_ms);
    state.subtitle_size = if relative {
        size_slider(payload.subtitle.size_scale)
    } else {
        precise_slider(payload.subtitle.precise_size_px)
    };
    state.subtitle_position = position(payload.subtitle.position);
    state.subtitle_outline = outline_slider(payload.subtitle.outline);
    state.subtitle_relative = relative;
    state.subtitle_language_name = s(payload.subtitle.default_language.clone());

    state.audio_delay = audio_delay_slider(payload.audio.delay_ms);
    state.eq_index = payload.audio.equalizer_index() as i32;
    state.audio_normalize = payload.audio.normalize_volume;
    state.audio_surround = payload.audio.headphone_surround;

    state.brightness = brightness_slider(payload.video.brightness);
    state.contrast = contrast_slider(payload.video.contrast);
    state.saturation = saturation_slider(payload.video.saturation);
    state.hue = hue_slider(payload.video.hue);
    state.gamma = gamma_slider(payload.video.gamma);
    state.zoom = zoom_slider(payload.video.zoom);
    state.aspect_index = payload.video.aspect_index() as i32;
    state.rotation_index = (payload.video.rotate_degrees / 90).clamp(0, 3);
    state.flip_h = payload.video.flip_horizontal;
    state.flip_v = payload.video.flip_vertical;

    state.speed = speed_slider(payload.playback.speed);
    state.sleep_minutes = sleep_slider(payload.playback.sleep_minutes);
    state.subtitle_timeline = payload.playback.show_subtitle_timeline;
    state.waveform = payload.playback.show_waveform;
    state.metadata = payload.playback.show_metadata;
    refresh_labels(state);
}

pub fn push_preset_rows(ui: &PlayerWindow, presets: &[Preset], active_name: &str) {
    let rows: Vec<PresetRow> = presets
        .iter()
        .map(|preset| PresetRow {
            name: s(preset.name.clone()),
            scope: s(preset.scope.label()),
            active: preset.name == active_name,
        })
        .collect();
    ui.set_preset_rows(ModelRc::new(VecModel::from(rows)));
}

pub fn push_options(ui: &PlayerWindow, language: Language) {
    ui.set_eq_options(ModelRc::new(VecModel::from(
        AudioPreset::EQ_LABELS
            .iter()
            .map(|label| s(eq_translation(language, label)))
            .collect::<Vec<_>>(),
    )));
    let mut aspects = vec![s(i18n::translate(language, "aspect.auto"))];
    aspects.extend(
        VideoPreset::ASPECT_LABELS
            .iter()
            .skip(1)
            .map(|label| s(*label)),
    );
    ui.set_aspect_options(ModelRc::new(VecModel::from(aspects)));
}

fn eq_translation(language: Language, label: &str) -> &'static str {
    match label {
        "Flat" => i18n::translate(language, "eq.flat"),
        "Bass boost" => i18n::translate(language, "eq.bass"),
        "Treble boost" => i18n::translate(language, "eq.treble"),
        "Vocal" => i18n::translate(language, "eq.vocal"),
        "Night" => i18n::translate(language, "eq.night"),
        _ => "Flat",
    }
}

/// Apply a payload to live libVLC state and the subtitle session. Safe on the UI thread.
pub fn apply_to_player(
    player: &vlc_wrapper::MediaPlayer,
    subtitles: &std::sync::Mutex<crate::subtitles_ui::SubtitleController>,
    payload: &PresetPayload,
) {
    let mut sc = subtitles.lock().unwrap();
    payload.apply_to_subtitle_session(&mut sc.session);
    drop(sc);
    player.set_audio_delay_ms(payload.audio.delay_ms);
    let _ = player.set_rate(payload.playback.speed);
    player.set_video_adjustments(
        payload.video.contrast,
        payload.video.brightness,
        payload.video.hue,
        payload.video.saturation,
        payload.video.gamma,
    );
    player.set_zoom(payload.video.zoom);
    let _ = player.set_aspect_ratio(payload.video.aspect_cstr());
    let _ = player.set_equalizer_preset(Some(payload.audio.equalizer_index() as u32));
}

/// Resolve and apply the best preset for a media path, updating the UI snapshot.
pub fn apply_for_media(
    db: &LibraryStore,
    ui: &PlayerWindow,
    player: &vlc_wrapper::MediaPlayer,
    subtitles: &std::sync::Mutex<crate::subtitles_ui::SubtitleController>,
    path: &std::path::Path,
) -> Option<String> {
    let language = i18n::ui_language();
    let presets = vfm_core::load_presets(db).ok()?;
    let resolved = vfm_core::resolve_for_media(&presets, path, None)?;
    apply_to_player(player, subtitles, &resolved.payload);
    let mut state = ui.get_sidebar_state();
    apply_payload_to_state(&mut state, &resolved.payload);
    // Rotation and mirroring are pixel transforms performed by the presenter thread,
    // not engine settings. Publish them so an auto-applied preset really turns/mirrors
    // the picture instead of only changing the sidebar read-outs.
    crate::store_video_transform(&state);
    state.preset_scope_label = s(scope_label(&resolved.scope, language));
    ui.set_sidebar_state(state);
    push_preset_rows(ui, &presets, &resolved.name);
    Some(resolved.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_sliders_round_trip() {
        for ms in [-2000, -250, 0, 750, 2000] {
            assert_eq!(delay_ms(delay_slider(ms)), ms);
        }
        for ms in [-1000, -125, 0, 500, 1000] {
            assert_eq!(audio_delay_ms(audio_delay_slider(ms)), ms);
        }
    }

    #[test]
    fn value_ranges_match_core_defaults() {
        assert_eq!(brightness(0.5), 1.0);
        assert_eq!(contrast(0.5), 1.0);
        assert_eq!(saturation(1.0 / 3.0), 1.0);
        assert!((hue(0.5)).abs() < f32::EPSILON);
        assert_eq!(gamma(0.5), 1.0);
        assert_eq!(zoom(0.2), 1.0);
        assert_eq!(speed(0.2), 1.0);
        assert_eq!(sleep_minutes(0.0), 0);
        assert_eq!(sleep_minutes(1.0), 120);
    }

    #[test]
    fn payload_survives_state_round_trip() {
        let mut payload = PresetPayload::default();
        payload.subtitle.delay_ms = 450;
        payload.subtitle.precise_size = true;
        payload.subtitle.precise_size_px = 38.0;
        payload.video.brightness = 1.4;
        payload.audio.equalizer = "Night".into();
        payload.playback.speed = 1.5;
        payload.playback.sleep_minutes = 30;

        let mut state = default_state();
        apply_payload_to_state(&mut state, &payload);
        let rebuilt = snapshot_payload(&state);
        assert_eq!(rebuilt.subtitle.delay_ms, 450);
        assert!(rebuilt.subtitle.precise_size);
        assert!((rebuilt.subtitle.precise_size_px - 38.0).abs() < 1.0);
        assert!((rebuilt.video.brightness - 1.4).abs() < 0.01);
        assert_eq!(rebuilt.audio.equalizer, "Night");
        assert!((rebuilt.playback.speed - 1.5).abs() < 0.01);
        assert_eq!(rebuilt.playback.sleep_minutes, 30);
    }

    #[test]
    fn scope_label_is_localised_for_global_scopes() {
        assert_eq!(
            scope_label(&vfm_core::PresetScope::Global, Language::En),
            "All media"
        );
        assert_eq!(
            scope_label(&vfm_core::PresetScope::Global, Language::Ar),
            "كل الوسائط"
        );
    }

    #[test]
    fn a_new_preset_scopes_to_the_folder_of_the_open_media() {
        let media = std::path::PathBuf::from("D:/vfm-data/Show/Ep01.mkv");
        match scope_for_media(&media) {
            vfm_core::PresetScope::Folder(folder) => {
                assert_eq!(folder, std::path::PathBuf::from("D:/vfm-data/Show"));
            }
            other => panic!("expected a folder scope, got {other:?}"),
        }
        // A path that does not exist cannot be scoped to a folder.
        assert!(matches!(
            scope_for_media(std::path::Path::new("D:/nowhere/Ep01.mkv")),
            vfm_core::PresetScope::Global
        ));
    }
}
