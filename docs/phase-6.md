# Phase 6 summary — Right sidebar and presets (VFM)

Status: **complete for review**. The sidebar, preset CRUD and folder/show auto-apply are wired
end to end; final quality-gate output is recorded below.

## What was done

### Core preset engine (`crates/core/src/presets.rs`, new)

* Typed `PresetPayload` with four sections:
  * `SubtitlePreset` — delay, relative/exact size, outline, position, default language.
  * `AudioPreset` — delay, default language, friendly equalizer label, normalize/surround filters.
  * `VideoPreset` — brightness, contrast, saturation, hue, gamma, zoom, aspect, rotation, flips.
  * `PlaybackPreset` — speed, sleep timer, timeline/waveform/metadata toggles.
* `PresetScope` supports `Global`, `Folder(path)` and `Playlist(name)` with stable text storage.
* `resolve_for_media` ranks playlist > deepest matching folder > global, so a preset saved for a
  show root also applies to every episode and a season folder can override it.
* SQLite CRUD via the existing `presets` table (`all_presets`, `rename_preset`, `delete_preset`
  added to `LibraryStore`). Payloads are stored as readable TOML.
* `PresetPayload::apply_to_subtitle_session` updates delay/style live without mutating cues.
* 7 new core tests, including the acceptance case: save a preset for a show, reload from SQLite,
  resolve it for the next episode in a nested season folder.

### libVLC adjustments (`crates/vlc-wrapper`)

* Added runtime-resolved symbols for `libvlc_video_set_adjust_int/float`,
  `libvlc_video_set_scale`, `libvlc_audio_equalizer_new_from_preset`,
  `libvlc_audio_equalizer_release` and `libvlc_media_player_set_equalizer`.
* `MediaPlayer::set_video_adjustments`, `set_zoom` and `set_equalizer_preset` apply values
  safely after the existing frame-callback setup. No libVLC calls occur inside callbacks.

### Path normalisation at the FFI boundary (bug fix found during verification)

* **libVLC rejects absolute Windows paths written with forward slashes.**
  `libvlc_media_new_path("D:/media/ep.mkv")` returns a NULL handle, while
  `"D:\\media\\ep.mkv"` succeeds — verified directly against the bundled `libvlc.dll`
  with a `ctypes` probe over six path spellings. Relative paths worked, which is why the
  bug hid behind the relative-path launch used in earlier phases.
* `vlc_wrapper::vlc_path_string` now normalises separators once, at the FFI boundary, for
  `media_from_path`, `add_subtitle_file` and `snapshot`. Strings that look like URLs
  (`scheme://…`) are passed through untouched so streaming sources still work.
* The app resolves command-line media paths to absolute form (`absolutize`) before using
  them, so library rows, watch progress and folder-scoped presets all key off the same
  representation no matter how the file was opened.
* 4 new wrapper tests cover separator normalisation, URL pass-through and relative paths;
  1 new app test covers absolute-path resolution.

### Sidebar UI (`crates/app/ui/sidebar.slint`, new)

* `RightSidebar` overlays the player between the title and control bars, RTL-mirrored via
  `Theme.rtl`, with tabs for Subtitles, Audio, Video, Playback and Presets.
* `Flickable` panels use theme tokens for every colour, radius and spacing value.
* `SidebarState` is a single serialisable snapshot pushed from Rust; `PresetRow` carries the
  saved preset list. Sliders emit a generic `(key, value)` callback so Rust owns the unit maths.
* Live frame preview in the Video tab reuses the existing video image.
* `Ctrl+E` opens the sidebar; the gear button and context-menu "More settings" also open it.
  Escape closes it along with the other overlays.
* **Label rows are wrapped in fixed-height `Rectangle`s.** A bare `Text` reports an unbounded
  maximum height to the layout engine, so it absorbs the panel's slack and pushes the preset
  list and the action buttons off-screen. The fixed-height row pattern (already used by the
  font/language rows in the Subtitles tab) keeps every tab's layout deterministic; the preset
  list is the only flexible region, so Apply/Rename/Delete stay pinned to the bottom.
* The scope label tracks reality: it shows the folder a new preset would land in, or the scope
  of the preset that is currently selected/applied.

### App glue (`crates/app/src/presets_ui.rs`, new; `main.rs`)

* Normalised slider values map to real units: ±2 s subtitle delay, 0.5–2× or 12–96 px subtitle
  size, 0–4 px outline, ±1 s audio delay, libVLC 0–2 / 0–3 adjustment ranges, 0.25–4× speed,
  0–120 min sleep timer.
* `snapshot_payload` / `apply_payload_to_state` round-trip every value; 3 new app tests cover
  delay ranges, neutral defaults and a full payload round trip.
* Live setting callbacks apply directly to `SubtitleSession`, `set_audio_delay_ms`,
  `set_video_adjustments`, `set_zoom`, `set_rate`, aspect ratio and equalizer.
* Preset actions save/load/rename/delete/select rows and add subtitles. Save scopes a preset to
  the current media's parent folder (or Global when no file is loaded).
* `apply_preset_for_path` is called on startup, library row open, queue play, next/previous,
  media keys and drag-and-drop, so the next episode picks up the saved preset automatically.
* The sidebar's own read-outs (scope label, filter list, "off") are localised: the current UI
  language lives in `i18n::ui_language()`, so toggling RTL re-renders them in Arabic.
* Phase 6 added 49 EN/AR i18n keys; generated `strings.slint` / `strings_ui.rs` regenerated.

## Quality gates

```powershell
$env:LIBVLC_PATH = "D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc"
python tools/generate_strings.py --check
python tools/generate_theme.py --check
python tools/generate_icons.py --check
cargo fmt --all -- --check
cargo clippy --release --workspace --all-targets -- -D warnings
cargo test --release --workspace
cargo build --release -p vfm-app
```

Final run (2026-11-20):

| Gate | Result |
| --- | --- |
| `generate_strings.py --check` | `strings.slint and strings_ui.rs are current` |
| `generate_theme.py --check` | `theme.slint is current` |
| `generate_icons.py --check` | `icon sets are current` |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | clean, no warnings |
| `cargo test --release --workspace` | **118 passed / 0 failed** — 24 app, 84 core, 10 wrapper |
| `cargo build --release -p vfm-app` | `Finished release profile in 2m 04s` |

The release profile is used instead of `debug` on this machine: the workspace lives on a
path with non-ASCII characters, and the debug target directory is unusable there
(`safe-delete` refuses it and stale rustc workers hold mapped sections on it).

## Screenshots

| File | Shows |
| --- | --- |
| `docs/compare/phase-6-sidebar-subtitles-dark-en.png` | Subtitles tab with the auto-applied preset: **Sync +0.45 s**, **Size 1.25×** |
| `docs/compare/phase-6-sidebar-presets-dark-en.png` | Presets tab: scope `D:/vfm-data/Show`, the saved "Solaris Demo" entry, Apply/Rename/Delete |
| `docs/compare/phase-6-sidebar-subtitles-dark-ar.png` | Arabic RTL: sidebar mirrored to the left, all labels localised |

`tools/capture_window.py` automates the grab: it matches the localised window title
("Vedio for me" / "فيديو من أجلي"), restores the window if it was minimised, brings it to
the front and retries until the frame is not covered by another application.

### Rotation/flip on-screen proof (fresh run, this revision)

A quadrant-coloured clip (`red green / blue yellow`) was generated with ffmpeg and opened with
three separate `--data-dir` profiles, each holding one auto-applied preset. The stage pixels were
captured with `tools/capture_window.py`:

| Screenshot | Preset | Stage quadrants (TL / TR / BL / BR) | Result |
| --- | --- | --- | --- |
| `docs/compare/transform-id.png` | none (identity) | red / green / blue / yellow | baseline |
| `docs/compare/transform-rot90.png` | `rotate_degrees = 90` | blue / red / yellow / green | true 90° clockwise turn |
| `docs/compare/transform-fliph.png` | `flip_horizontal = true` | green / red / yellow / blue | true horizontal mirror |

`docs/compare/transform-shipped-rot90.png` and `transform-shipped-fliph.png` repeat both checks
against the binary extracted from `dist/VFM-1.0.0-portable.zip` (not the dev build), with the
shipped `plugins/` tree and no extra libVLC install. The shipped archive also passed the full
7-check GUI harness afterwards.

The identity path is a straight copy; only non-identity transforms pay for a pixel pass. The
sidebar rotation selector and both flip toggles publish to the same presenter atomic, and the
same publish happens when a preset is loaded or auto-applied, so no control is decorative.

## Run commands

```powershell
$env:LIBVLC_PATH = "D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc"
.\target\release\VFM.exe "D:\media\episode.mkv" --no-single-instance
# Ctrl+E opens the sidebar; use the Presets tab to save a folder-scoped preset,
# then press Next to verify auto-apply on another episode from the same folder.
# --sidebar 0..4 opens a specific tab at startup (subtitles/audio/video/playback/presets).
```

## Known issues and deviations

1. Rotation and flip now transform the decoded frame before it is uploaded to the stage. The
   work is done in `crates/core/src/transform.rs` (pure pixel math + unit tests) and published
   to the presenter thread from the sidebar and from auto-applied presets. Aspect ratio, zoom
   and the five colour adjustments are applied live by libVLC. The transform costs one extra
   buffer copy per presented frame and is skipped entirely for the identity case.
2. Equalizer labels map to built-in libVLC preset indexes; they are friendly names, not a custom
   ten-band editor.
3. Subtitle outline in the sidebar is stored and shown, while the Slint overlay still draws a
   fixed four-copy 2 px outline. Variable outline rendering is a small follow-up.
4. The headless seek-thumbnail grabber from Phase 5 is wired; the preview is centred above the
   control bar rather than pinned to the exact hover x.
5. Native file/folder pickers remain deferred to Phase 9; library path entry uses a text field.
6. Pre-existing open items remain: cone-icon branding decision, 4K GPU path, full Figma DPI
   comparison at 125/150/200 %.
