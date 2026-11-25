# VFM test checklist — current build

Phase 1 is engine-first, Phase 2 built the themed design system, and Phase 3 replaced the spike shell with the real player screen, measured against `design/Player.png`. Design-system captures and the Figma comparisons live under `docs/compare/`.

The editable design archive is preserved as `design/source.fig` (with `design/source-thumbnail.png`). The PNG exports remain the visual source of truth; Phase 2 generated tokens/components from the sampled PNGs and kept the `.fig` archive for later re-export.

## 1. Environment

From the workspace root:

```powershell
$env:LIBVLC_PATH = "$PWD\vendor\libvlc"
```

`vendor\libvlc` must contain `libvlc.dll`, `libvlccore.dll`, and `plugins\`. The wrapper also falls back to `<exe>\vlc\`, `<exe>\`, and `C:\Program Files\VideoLAN\VLC`, in that order. If discovery fails, the app exits with a typed error instead of showing a native loader popup.

Local build note: `.cargo\config.toml` links `tools\crt-math-shim` and points `rusqlite` at the system SQLite under `vendor\sqlite`. Keep `sqlite3.dll` beside `VFM.exe`.

## 2. Automated quality gates

Run all four; all must pass with zero warnings:

```powershell
cargo fmt --all -- --check
python tools\generate_theme.py --check
python tools\generate_strings.py --check
python tools\generate_icons.py --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p vfm-app
```

Expected counts on the 2026-11-25 baseline: **84 app + 149 core + 4 platform + 10 wrapper + 0 doc-test failures** (247 total). Cargo may print non-fatal `error deleting lock file for incremental compilation ... Access is denied` warnings on this host; the commands still exit 0.

## 3. Headless engine acceptance (two files, resume, persistence)

Use a fresh data directory for the first run:

```powershell
& .\target\release\VFM.exe `
  .\testdata\1080p60.mp4 .\testdata\sample1080p-b.mp4 `
  --engine-test 3 --width 1920 --height 1080 --chroma rv32 `
  --no-single-instance --data-dir .\testdata\engine-run-check
```

Expected shape:

```text
[0] play .\testdata\1080p60.mp4
[0] saved progress 0:02 / 0:35 (watched=false)
[1] play .\testdata\sample1080p-b.mp4
[1] saved progress 0:02 / 0:12 (watched=false)
engine test complete: 2 file(s), database at ...
```

Run the same command again with `--engine-test 2` and the **same** data directory. The first file must resume; the 12-second clip intentionally does not resume under the default `resume_min_remaining_ms = 30000` policy:

```text
[0] resume .\testdata\1080p60.mp4 at 0:02
[0] saved progress 0:04 / 0:35 (watched=false)
[1] play .\testdata\sample1080p-b.mp4
```

Verify persistence directly:

```powershell
python -c "import sqlite3; c=sqlite3.connect(r'.\testdata\engine-run-check\library.sqlite'); print(list(c.execute('SELECT path,position_ms,duration_ms,watched FROM media ORDER BY path')))"
```

Also confirm `settings.toml` exists and contains `version = 1`, `remember_position = true`, and `accent = "#D37918"`.

**Verified result:** `testdata\engine-run-verified` holds a SQLite row of `('./testdata/1080p60.mp4', 6135, 35000, 0)` after three runs, and its `settings.toml` contains the versioned defaults above.

## 4. Manual UI / Windows checks

| # | Check | How | Expected | Status |
|---|-------|-----|----------|--------|
| 1 | Friendly libVLC discovery | Unset `LIBVLC_PATH`, run `VFM.exe <file>` | Typed “libVLC runtime not found” error; no Windows crash popup | Ready |
| 2 | Runtime discovery | Set `LIBVLC_PATH`, run with a media file | Prints plugin path, video appears in Slint `Image`, HUD composites over it | Ready |
| 3 | EN / عربي toggle | Press the language toggle in the HUD | Labels switch and control order mirrors for RTL | Ready |
| 4 | Keyboard shortcuts | Space, arrow keys, `F`, `M` in the spike `FocusScope` | Play/pause, seek, fullscreen/mute hooks respond without leaving UI thread | Ready |
| 5 | Coalesced frames | Watch HUD FPS/drop counters at 1080p60 | One UI upload per presented frame; no queue backlog | Ready |
| 6 | Single instance | Launch twice without `--no-single-instance` | Second launch exits with “already running”; lock file removed on exit | Ready |
| 7 | CLI / open-with | `VFM.exe .\testdata\1080p60.mp4` | File opens directly from argument | Ready |
| 8 | File associations | `--associate`, then `reg query HKCU\Software\Classes\VFM.Media /s`; then `--unassociate` | HKCU entries appear/disappear; no HKLM writes | Ready to validate |
| 9 | Window geometry | Resize/move/maximize, close, relaunch | Geometry restored and clamped to sane 640×360…8192×8192 bounds | Ready |
| 10 | Resume prompt | Play >5 s of a long file, quit, reopen same path | Resume policy offers the saved position; near-end media marked watched | Ready via engine test |
| 11 | Drag & drop | Drag media files onto the window | Drop hint appears; files are appended to the queue and the first one plays | Ready |
| 12 | Media keys / SMTC | Play/pause/stop/next/previous via system media controls | SMTC reports `SMTC attached`; button presses are handled on the UI thread | Ready |
| 13 | Snapshot / tracks / delays / A–B / frame-step | Exercise from UI | Wrapper + core APIs exist; UI controls arrive in Phases 5–7 | **Deferred** |
| 14 | RTL full layout | Compare with Figma RTL screens | Phase 1 spike shell only; full shell is Phase 3 | **Deferred** |

## 4b. Phase 2 design-system checks

```powershell
& .\target\release\VFM.exe --gallery
& .\target\release\VFM.exe --gallery --gallery-theme light
& .\target\release\VFM.exe --gallery --gallery-language ar
```

| # | Check | How | Expected | Status |
|---|-------|-----|----------|--------|
| 1 | Generator freshness | Run both generator `--check` commands | Both report current | Ready |
| 2 | Gallery opens headlessly | Run `--gallery` with no media / no libVLC | Gallery opens; no libVLC path printed | Ready |
| 3 | Theme switch | Click `Dark` / `Light` in gallery toolbar | All surfaces, text, borders, and overlays switch tokens | Ready |
| 4 | Language + RTL | Click `Language`; compare EN and AR | All Strings properties change; sliders, fills, breadcrumbs, and text align mirror | Ready |
| 5 | Accent runtime change | Click `Accent` repeatedly | Accent-driven controls update without restart | Ready |
| 6 | Component states | Review States section | Default/hover/pressed/focus/disabled/selected are visually distinct | Ready |
| 7 | Required component coverage | Review remaining sections | All 19 components from PHASES.md are present | Ready |
| 8 | Icon sets | Review Icon sets section | 39 monochrome and 39 two-tone SVGs render; no branded marks | Ready |
| 9 | Tooltip / dialog / toast | Hover icon buttons; click dialog button | Tooltips shown, modal opens/closes, toast visible | Ready |
| 10 | Design comparison | Compare `docs/compare/phase-2-gallery-*.png` to Figma exports | Accent, gray icons, radii, menu rhythm match sampled tokens | Ready |

## 4c. Phase 3 player checks

```powershell
$env:LIBVLC_PATH = "$PWD\vendor\libvlc"
& .\target\release\VFM.exe .\testdata\1080p60.mp4 --width 1920 --height 1080 `
  --chroma rv32 --no-single-instance --data-dir .\testdata\phase3-run
```

Theme and language come from `.\testdata\phase3-run\settings.toml`
(`theme = "dark" | "light"`, `language = "en" | "ar"`). For a 1:1 Figma capture also set
`$env:SLINT_SCALE_FACTOR = "1"` and use a 1920-wide window.

| # | Check | How | Expected | Status |
|---|-------|-----|----------|--------|
| 1 | Control strip geometry | Compare `docs/compare/phase-3-strip-vs-figma.png` | Bars, buttons, seek track, time labels and right cluster align with Figma within a few px | Ready |
| 2 | Transport clock | Play a file, watch the elapsed label | Clock advances as `HH:MM:SS`; duration shows the clip length | Ready |
| 3 | Play/pause glyph | Toggle playback | Button swaps pause/play and keeps its accent fill | Ready |
| 4 | Thick seek | Click and drag the track; hover it | Thumb follows, played pill grows, tooltip shows the hover time | Ready |
| 5 | Volume cone | Drag the wedge; scroll over the video | Cone fills green proportionally; wheel changes volume | Ready |
| 6 | Chrome auto-hide | Play, leave the mouse still 2 s; move again | Bars fade out while playing and come back on move | Ready |
| 7 | Hide UI | `Ctrl+\` | Both bars disappear; 8 px edge strips bring them back | Ready |
| 8 | Right-click menu | Right-click the video | Menu with icons and shortcuts; Escape dismisses | Ready |
| 9 | Shortcut OSD | Press Space / F / arrows | Key chip + action label appear, dismiss after ~0.9 s | Ready |
| 10 | Fullscreen | `F`, `Esc`, top-bar button, double-click video | Toggles fullscreen; bars still auto-hide | Ready |
| 11 | PiP window | Top-bar PiP button | 360×220 frameless window mirrors the live frame; close returns to the main window | Ready |
| 12 | Frameless title bar | Drag, double-click, snap to edge | Moves, maximises/restores, snaps without losing resize borders | Manual |
| 13 | Drag and drop | Drop a video file onto the window | File is enqueued and starts playing | Ready |
| 14 | Arabic RTL | Set `language = "ar"` | Transport mirrors right, seek fills from the right, volume and icons mirror left, Arabic text shapes correctly | Ready |
| 15 | Light theme | Set `theme = "light"` | Surfaces, bars, text and borders switch; accent and volume green stay legible | Ready |
| 16 | DPI scaling | Run at 100 / 125 / 150 / 200 % | Bars stay 40/54 logical px; no clipped or overlapping controls | Partial (125 % verified) |
| 17 | Multi-monitor | Move the window to a second display, restart | Geometry restored and rejected if offscreen | Partial |
| 18 | Taskbar icon | Start the player, look at the taskbar | Brand icon shown for the running window | Ready |

## 4d. Phase 4 queue & library checks

| # | Check | How | Expected | Status |
|---|---|---|---|---|
| 1 | Library-first launch | `vfm-spike --library --data-dir <dir>` | Full-screen library opens with no media loaded; sidebar shows eight sections | Ready |
| 2 | Open from player | Press `Ctrl+L` or choose Browse in the right-click menu | Player hides, library shows with the current section | Ready |
| 3 | Add a TV-show folder | Network/form → paste a folder path → submit | Folder is scanned; its video files appear in Folders and Videos | Ready |
| 4 | Browse without leaving video | Open a row from the library | Video plays in the shared player; mini-player keeps transport controls; player can be reopened | Ready |
| 5 | Queue panel | Click the queue button in the control bar | Panel shows now-playing + rows; play/remove/clear work and stay in sync with next/previous | Ready |
| 6 | Add to queue | Click the queue icon on a library row | Item appended without interrupting playback | Ready |
| 7 | Pinned / watched | Click pin on a row; toggle watched | Row moves into/out of Pinned and updates markers, persisted across restart | Ready |
| 8 | Search and sort | Type in search; change sort dropdown | Rows filter and reorder instantly | Ready |
| 9 | View modes | Toggle grid/list/detail | Content switches presentation; progress bars and thumbnails show when available | Ready |
| 10 | Network stream | Paste an `.m3u8` URL or local M3U file | Stream entries parsed (`#EXTINF` titles) and listed under Network | Ready |
| 11 | Arabic RTL | `--library` with `language = "ar"` | Sidebar mirrors right, toolbar/icons mirror, Arabic strings shape correctly | Ready |
| 12 | Persistence | Add folder/stream, restart with same `--data-dir` | Folders, streams, queue, pinned and watched flags all restored | Ready |

## 4e. Phase 5 tracks, thumbnails and subtitle checks

| # | Check | How | Expected | Status |
|---|---|---|---|---|
| 1 | Auto subtitle sidecar | Start with `testdata/phase5/Ep.mkv` | `Ep.srt` is discovered and rendered over the video | Ready |
| 2 | Explicit subtitle | Start with `--subtitle <file>` | Explicit cues replace auto-discovery | Ready |
| 3 | Live sync | Change `SubtitleSession.delay_ms` while playing | Cues shift without restarting playback | Ready |
| 4 | Audio/subtitle popovers | Click the equalizer / subtitle buttons or use the context menu | Track lists show human-readable names, active track accented | Ready |
| 5 | LibVLC fallback | Select a real subtitle track ≥ 0 | Our overlay switches off, no double subtitles | Ready |
| 6 | Seek preview | Hover a cached time bucket | Floating thumbnail appears above the control bar | Partial (cache only; grabber pending) |

## 4f. Phase 6 right sidebar & presets checks

| # | Check | How | Expected | Status |
|---|---|---|---|---|
| 1 | Open sidebar | Click the gear button or press `Ctrl+E` | Adjustment panel slides in from the correct side (RTL mirrored) | Ready |
| 2 | Tabs | Select Subtitles / Audio / Video / Playback / Presets | Each tab shows its own controls; values reflect live state | Ready |
| 3 | Live subtitle controls | Drag delay / size / position / outline sliders | Overlay updates on the next clock tick without a restart | Ready |
| 4 | Relative vs exact size | Toggle the size mode | Slider switches between `×` multiplier and `px` | Ready |
| 5 | Audio delay & EQ | Drag delay; choose an equalizer label | `libvlc` delay changes immediately; EQ preset applies | Ready |
| 6 | Video adjustments | Drag brightness/contrast/saturation/hue/gamma/zoom | Image changes live; preview shows the current frame | Ready |
| 7 | Aspect / rotation / flip | Choose aspect and rotation; toggle flips | Aspect applies to libVLC; rotation/flip stored in the preset | Partial (rotation/flip are stored, not GPU-applied yet) |
| 8 | Save preset | Name a preset on a show folder, click Save | Row appears with folder scope; payload persisted to SQLite | Ready |
| 9 | Auto-apply next episode | Play another episode from the same folder | Saved subtitle delay/style and audio/video settings apply automatically | Ready |
| 10 | Load / rename / delete | Double-click or Apply, rename, delete rows | State updates and survives restart with the same `--data-dir` | Ready |
| 11 | Arabic RTL | Switch language to Arabic and open the sidebar | Labels translate and the panel mirrors to the left edge | Ready |

## 5. Thread-safety invariants to preserve

1. Never call libVLC from a video or event callback; callbacks only swap buffers or `try_send` an `i32` event type.
2. Never block the UI thread on libVLC state queries; presenters coalesce uploads via `invoke_from_event_loop`.
3. `EventHub` detaches all callbacks before the user-data box or player handle is released.
4. `MediaPlayer::drop` order is: stop → detach events → release player → drop frame buffers.

## 6. Known limitations at end of Phase 1

* File-association helper is implemented with `reg.exe` but has not been validated through the Windows shell UI.
* SMTC attachment was verified headlessly; actual button-press routing still needs a manual check on the target machine.
* The spike video callback assumes one fixed width/height per player; the engine harness does not render UI, but the 4K CPU copy path remains too slow for presentation (see `docs/spike.md`).
* `design/SPEC.md` has not been supplied; `CLAUDE.md` lists it as a source of truth for behavior. Phase 2 used the approved redesign brief in `design/tokens.md` instead.
* Phase 3 added full-screen Figma comparisons in `docs/compare/` (`phase-3-vs-figma.png`, `phase-3-strip-vs-figma.png`).
* The supplied app icon (`assets/branding/vfm.ico`) is a traffic cone with a smiley; that conflicts with the project's "no VLC cone branding" rule and needs a product decision.
* The `.exe` file icon is not embedded (no `rc.exe` on this toolchain); the runtime window/taskbar icon is set from `Window.icon`.
