# Phase 8 summary — Settings, hotkeys and onboarding (VFM)

Status: **complete for review**. Every preference the player exposes is now declared once in a
schema, rendered by a generated settings window, persisted to `settings.toml`, and applied live
without a restart. Shortcuts are rebindable with conflict detection, and a first-run flow picks
theme, accent and language.

## What was done

### Settings schema (`crates/core/src/settings_schema.rs`, new)

* One declarative table drives the whole UI: `SettingId` (51 settings) × `SettingKind`
  (`Toggle`, `Choice`, `Slider`, `Text`, `Folder`) × `Category` (13: General, Interface,
  Privacy, Controls, Playback, Video, Audio, Subtitles, Library, Network, Performance,
  Recording, Extensions).
* `SettingValue` is the single currency: `Bool`, `Choice`, `Number`, `Text`. `value_of_key` /
  `set_value` read and write a `Settings` struct by key string, so the window never needs a
  match arm per row — adding a setting is one table entry plus one field.
* Four settings are stored as free text but presented as choices (aspect ratio, subtitle
  encoding, subtitle search language, snapshot format). They report `Choice` and normalise the
  stored `"auto"` back to an empty string, so the combo has a real "Auto" entry.
* `Choice::label_key` falls back to `opt.<id>`, which keeps new option lists localisable without
  touching the generator.

### Settings persistence (`crates/core/src/settings.rs`)

* `SETTINGS_VERSION = 2`. `from_toml` upgrades older files in place, then `clamp()`s every
  numeric field, so a hand-edited or truncated file can never put the player in an invalid
  state (volume 900 %, 0.1 s seek step, negative gamma…).
* New fields cover theme/language/accent, OSD, waveform, sidebar tab, start page, privacy,
  seek and volume steps, wheel and click behaviour, repeat/shuffle/resume/auto-advance/
  start-paused/speed, aspect ratio, deinterlace, zoom, brightness/contrast/saturation,
  hardware decoding, equaliser, subtitle scale/encoding/outline/position/search language,
  library folder and sort, network timeout and caching, loop-filter skipping, snapshot folder/
  format/prefix, extensions folder, and `onboarding_done`.

### Settings window (`crates/app/ui/settings.slint`, `crates/app/src/settings_ui.rs`, new)

* Two-pane layout that follows the agreed design: category rail with a per-category modified
  count, search field, and a scrolling row list.
* Search filters across every category by label, key and help text; the category rail switches
  to a filtered mode while a query is active.
* **Show advanced** reveals the second tier (network, performance, privacy, extensions) so the
  default view stays short.
* Each row renders by `kind`: a switch, a combo with a popup option list, a slider with a live
  value, a text field, or a folder row with a Browse button.
* Rows that differ from their default show a dot, an inline reset affordance and an info
  affordance that expands a two-line explanation in place.
* Footer shows `N modified` with **Reset all**; the header carries the search field and
  **Done**.
* `SettingsUi` keeps the models in `RefCell`s and exposes `refresh_models`,
  `apply_bool/choice/number/text`, `reset`, `reset_all` and `commit` **without** a window, so
  the whole interaction model is unit-testable; the window-pushing helpers (`refresh`,
  `refresh_chrome`, `refresh_categories`) are thin wrappers over those.

### Hotkeys (`crates/core/src/hotkeys.rs`, `crates/app/src/hotkeys_ui.rs`, new)

* 35 actions (`Action::all`), each with a stable id, a label key and a default binding that
  matches the keys the player actually shipped in Phases 3–7.
* `KEYS_VERSION = 2`; `load` merges a stored config over the defaults, so a new action appears
  bound to its default instead of vanishing.
* The shortcut dialog lists every action with a key cap; clicking a cap captures the next key
  press (`FocusScope` + `key-pressed`), normalises it to `Ctrl+Shift+Alt+Key`, and refuses
  duplicates by flagging both rows in `danger` with a conflict note.
* Import/export writes `keys.toml` beside `settings.toml`, so a layout can be moved between
  machines.
* The player no longer hard-codes keys: `player.slint` forwards every key press to
  `root.hotkey(text, ctrl, shift, alt) -> bool`, and `main.rs` resolves it through the config.
  `Escape` is handled first so popovers always close.

### Onboarding (`crates/app/ui/onboarding.slint`, new)

* First run (or `--onboarding`) shows theme cards, eight accent swatches and a shortcut
  cheat-sheet, then **Start**. `onboarding_done` is written when it finishes, so it appears
  exactly once.
* Choosing an accent previews it on the onboarding window *and* the player window immediately.

### Platform crate (`crates/platform`, new)

* `vfm-app` is `#![deny(unsafe_code)]`, so the one Win32 call the app needs lives here:
  `system_uses_light_theme()` reads `AppsUseLightTheme` with `RegGetValueW`. `reg.exe` is
  blocked by the sandbox and shelling out was the wrong shape anyway.
* `Theme::System` now resolves at startup and whenever the setting changes.

### Live apply (`crates/app/src/main.rs`)

* `apply_live` pushes the settings that affect the running player: OSD, waveform, click/wheel
  behaviour, subtitle size/position/outline. Theme, accent, aspect ratio, zoom, video
  adjustments, equaliser and volume are dispatched per key so a slider drag only touches the
  thing it changes.
* `apply_accent` sets the accent **and** its readable ink: the design's near-black works on the
  warm default, but a saturated accent (blue, red, violet) flips the ink to white when black no
  longer wins by a clear contrast margin.
* The persisted accent is now honoured at startup for the player, PiP and library windows; it
  was previously only applied inside the settings window.

### CLI

* `--settings`, `--hotkeys`, `--onboarding` open one surface directly (used for the
  screenshots). `--settings` and `--hotkeys` skip the "no media" guard and hide the player
  window when they are the only surface requested.

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

Final run (2026-11-21):

| Gate | Result |
| --- | --- |
| `generate_strings.py --check` | current |
| `generate_theme.py --check` | `theme.slint is current` |
| `generate_icons.py --check` | `icon sets are current` |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | clean, no warnings |
| `cargo test --release --workspace` | **193 passed / 0 failed** — 62 app, 120 core, 1 platform, 10 wrapper |
| `cargo build --release -p vfm-app` | finished |

Phase 7 ended at 160 tests; Phase 8 adds 33 (settings schema 11, settings 6, hotkeys 7,
settings_ui 5, hotkeys_ui 3, platform 1).

## Screenshots

| File | Shows |
| --- | --- |
| `docs/compare/phase-8-settings-dark-en.png` | Settings, General: category rail with modified counts, search, switch/combo/swatch rows, `2 modified` |
| `docs/compare/phase-8-settings-light-en.png` | Same window in the light theme |
| `docs/compare/phase-8-settings-dark-ar.png` | Arabic RTL: rail on the right, controls on the left, labels right-aligned |
| `docs/compare/phase-8-hotkeys-dark-en.png` | Shortcut dialog: key caps per action, capture state, conflict warning |
| `docs/compare/phase-8-onboarding-dark-en.png` | First run: theme cards, accent swatches, shortcut cheat-sheet, Start |

## Run commands

```powershell
$env:LIBVLC_PATH = "D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc"
.\target\release\vfm-spike.exe "D:\media\episode.mkv" --no-single-instance
.\target\release\vfm-spike.exe --settings      # settings only
.\target\release\vfm-spike.exe --hotkeys       # shortcut dialog only
.\target\release\vfm-spike.exe --onboarding    # replay the first-run flow
```

## Known issues and deviations

1. Folder rows have no native picker — **Browse** opens Explorer at the current path and the
   field is edited by hand. A proper `IFileOpenDialog` lands in Phase 9 (it is the same dialog
   the open-media flows need).
2. `Open file`, `Open folder` and `Open network` are bindable but only route to the library
   window today; the player has no open dialog of its own yet (Phase 9).
3. `Theme::System` is read once per process. Windows' theme-change message is not subscribed,
   so toggling the OS theme mid-session needs a restart.
4. Equaliser presets are stored and applied through libVLC's own equaliser; there is no
   per-band editor UI yet.
5. The shortcut capture accepts modifier combinations only; bare keys that the OS reserves
   (e.g. `F12`) are passed through and never reach the player.
6. Rotation/flip, the variable subtitle outline, the headless seek-thumbnail grabber, native
   file pickers, branding and the 4K GPU path remain open from earlier phases.
