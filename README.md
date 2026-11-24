# VFM — a VLC-class player for Windows, rebuilt around the agreed design

VFM plays everything libVLC plays, with a hand-built Slint interface that follows the Figma
source in `design/`: a single dark-or-light player window, a transport strip, an adjustment
sidebar, a library, a queue, subtitle tooling, settings, and a first-run onboarding flow.

The project is a Rust workspace:

| Crate | Role |
| --- | --- |
| `crates/vlc-wrapper` | Safe Rust API over libVLC (`unsafe` isolated here) |
| `crates/core` | Platform-agnostic logic: settings, library, playlists, queue, subtitles, hotkeys, layout, open-media rules |
| `crates/platform` | The Win32 calls the UI cannot make itself (theme probe, file pickers, clipboard) |
| `crates/app` | Slint UI, controllers, and the `vfm-spike` binary |

## Requirements

* Windows 10/11 (x64)
* Rust 1.85+ with the MSVC toolchain
* A libVLC 3.x runtime. The repository expects it in `vendor/libvlc` (see
  `vendor/README.md`); set `LIBVLC_PATH` if it lives elsewhere.

## Build and run

```powershell
$env:LIBVLC_PATH = "$PWD\vendor\libvlc"
cargo build --release -p vfm-app
.\target\release\vfm-spike.exe "D:\media\episode.mkv"
```

Useful flags:

| Flag | Purpose |
| --- | --- |
| *(none)* | Start in the library — what the desktop shortcut does |
| `--library` | Start in the library explicitly |
| `--settings` / `--hotkeys` / `--onboarding` | Open one surface directly |
| `--open [0-7]` | Open the media dialog on a panel |
| `--customize` | Open the control-bar customizer |
| `--subs-search` | Open the OpenSubtitles dialog |
| `--data-dir DIR` | Use a separate settings/library directory |
| `--bench N` | Run N seconds of telemetry and exit |
| `--gallery` | Design-system component gallery |

## Keyboard shortcuts (defaults, remappable in Settings)

| Key | Action |
| --- | --- |
| `Space` | Play / pause |
| `S` / `N` / `P` | Stop / next / previous |
| `←` `→` | Seek ±10 s |
| `↑` `↓` | Volume |
| `M` | Mute |
| `F` | Fullscreen |
| `Ctrl+P` | Picture-in-picture |
| `A` / `V` / `W` | Audio / subtitles / video panel |
| `B` | A-B loop (press: set A, press: set B, press: clear) |
| `[` `]` `=` | Slower / faster / normal playback speed |
| `Z` | Cycle zoom |
| `E` | Frame step · `R` snapshot · `Q` queue · `Ctrl+L` library |
| `Ctrl+B` | Add bookmark · `Ctrl+I` settings · `Ctrl+\` hide UI |

Everything above goes through the remappable hotkey table
(`Settings → Hotkeys`); `Ctrl+O` opens media, `Ctrl+D` a folder, `Ctrl+N`
a network stream. Drag & drop works on every window and accepts files,
folders and playlists.

## Quality gates

```powershell
python tools/generate_strings.py --check
python tools/generate_theme.py --check
python tools/generate_icons.py --check
cargo fmt --all -- --check
cargo clippy --release --workspace --all-targets -- -D warnings
cargo test --release --workspace
```

`cargo test` needs `sqlite3.dll` beside the test binaries on this host: copy
`vendor/sqlite/lib/sqlite3.dll` into `target/release/deps/` once per fresh target directory.

## Documentation

* `PHASES.md` — the agreed phase plan
* `docs/phase-1.md` … `docs/phase-10.md` — what each phase delivered, its gates, its known
  issues
* `docs/testing.md` — how the harnesses and fixtures work
* `docs/compare/` — screenshots taken from the running build

## Licence

VFM itself is MIT-licensed. It links against libVLC (LGPL-2.1+) at runtime; see
`THIRD_PARTY_LICENSES.md` for the full list and the LGPL obligations that come with shipping
the runtime.
