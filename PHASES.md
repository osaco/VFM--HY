# VFM — Execution phases

One phase per session. Do not start the next phase until the previous result is accepted.
Each phase ends with: summary, `docs/compare/` screenshots (where UI changed), known issues,
exact run commands, then STOP.

## Status at a glance (verified 2026-11-25)

The phase list below is the original plan. The verified state of the code lives in
`docs/phase-1.md` … `docs/phase-11.md`, `docs/testing.md` and `docs/compare/`. This table is a
pointer, not a second source of truth; the roadmap is not "finished" and the phase documents
list the real gaps.

| Phase | State | Honest remainder |
| --- | --- | --- |
| 0 — Spike | Built and re-verified | 1080p60 ≈36 fps (RGBA) / 60 decoded fps (RV32); 4K30 ≈9 fps; NV12 + GPU path not implemented |
| 1 — Core engine | Implemented | UI polls libVLC on a 4 Hz tick rather than consuming every typed event |
| 2 — Design system | Implemented | — |
| 3 — Player view | Implemented | — |
| 4 — Queue & library | Implemented | 10k-item windowing not proven end to end; thumbnail generation is cache-first |
| 5 — Tracks/subtitles/thumbs | Implemented | Seek preview is centre-anchored, not pinned to the hover x; rotation/flip transform the rendered frame |
| 6 — Sidebar & presets | Implemented | Rotation/flip change the rendered frame; verified on screen in `docs/compare/transform-*.png` |
| 7 — Advanced | Implemented | Embedded album art is not extracted; music mode uses a generated card |
| 8 — Settings/hotkeys/a11y | Implemented | Windows high-contrast mode is not honoured |
| 9 — Media dialogs | Implemented | Capture devices are enumerated from DirectShow, with the typed field kept for anything not present at launch; conversion progress is estimated; RTSP serving is not offered |
| 10 — Layout customisation | Partly implemented | The player applies which tail buttons exist, not their full order |
| 11 — Hardening/release | Portable bundle verified 7/7 | DPI 125/150/200 %, multi-monitor captures, installer signing remain open |

**Baseline correction:** the supplied snapshot did not compile. Two build errors (a missing
`SidebarState` field and a non-`Send` `Rc` crossing into the event loop) and one stale hotkey
test were fixed on 2026-11-25 before any feature work; the previous "236 passed" claim was
historical and did not describe this source tree.

**Benchmark correction (2026-11-25):** the `--bench` harness had been reporting zero frames
for relative media paths because `Path::canonicalize` returns a `\\?\D:\...` verbatim path and
libVLC cannot open those — the input ends immediately with no decoded frames. `absolutize()`
now strips the verbatim prefix, the harness measures fps over the window that actually
played (not the process lifetime) and reports seek latency. Fresh numbers are in
`docs/spike.md` §4; they show the CPU callback path at ~36 fps for 1080p60 (RGBA) and ~9 fps
for 4K30, so 4K remains a known limitation.

**Package verification (2026-11-25):** the portable ZIP was extracted outside the repository,
run with `LIBVLC_PATH` unset (it found its own `libvlc.dll`/`plugins/`) and passed the seven-check
GUI harness 7/7. The digest is written to `dist/SHA256SUMS.txt` by `tools/package.ps1`. The
installer is still unbuilt — Inno Setup is not installed on this host — and the EXE icon is not
embedded at link time.

## Phase 0 — Architecture spike (done)
Finish Phase 0 on this Windows machine: make the workspace compile, pass fmt/clippy/tests,
render a real video with HUD overlaid, run the 1080p60 and 4K30 benchmarks (RGBA vs RV32),
fill `docs/spike.md` with real numbers. If 4K30 >2% drops or >25% CPU, implement NV12 + GPU
shader path. Review all unsafe code in `vlc-wrapper`. Stop.

**Acceptance:** video plays smoothly at 1080p, real numbers, no warnings.

## Phase 1 — Core engine (implemented)
Phase 1 — Core engine. Implement in `vlc-wrapper`/`core`:
- Player: play/pause/stop, accurate seek/time/length, volume/mute, speed, audio/subtitle track
  enumeration + selection, audio delay, subtitle delay, aspect ratio, A–B repeat, frame step,
  snapshot, chapters.
- Typed events from libVLC -> async channel -> UI state (state machine in core with tests).
- Queue model (auto-play next, shuffle, repeat, reorder) separate from Playlist model
  (saved collections). Persist both.
- SQLite schema + migrations: media items, folders, playlists, queue, watch progress
  (resume position, watched flag), bookmarks, presets.
- Settings store (typed, defaults, versioned, TOML) + change notifications.
- Windows integration: CLI args/open-with, file associations helper, drag&drop, media keys +
  SMTC, single instance, remember window state.
Tests for queue/playlist/progress logic. No new UI beyond what's needed to test.

**Acceptance:** multiple files play in sequence, resume from last position, tests pass.

## Phase 2 — Design system and components
Phase 2 — Design system. Generate a theme module from `design/tokens.md` (colors, type scale,
spacing, soft radii, elevation, motion) with dark + light themes and a runtime-changeable
accent color (default orange). Bundle Arabic+Latin fonts. Implement the i18n layer (en, ar)
with RTL mirroring. Create the two icon sets (two-tone menu icons, monochrome control icons)
as SVG assets. Build reusable components with all states: icon button, button, toggle, slider
(numeric + thick seekbar variant + cone volume variant), dropdown, tabs, list row, card, grid
item, context menu, tooltip, dialog, sidebar panel, scrollbar, search field, breadcrumb bar,
toast/OSD. Provide a hidden "component gallery" window to review every component against Figma.

**Acceptance:** gallery matches Figma in both themes and in Arabic RTL.

## Phase 3 — Player view
Phase 3 — Player view per SPEC.md: bottom control bar (play enlarged between prev/next on the
left; queue, subtitles, audio, volume cone, settings/sidebar, fullscreen on the right), thick
seek bar with time tooltip, top bar (menu, title, fullscreen, PiP, window controls) with
show-on-hover-near-edge / hide-on-leave for both bars (also in fullscreen), right-click context
menu (+ "Hide UI"), shortcut OSD that shows the key pressed + action, full keyboard shortcut
map, fullscreen, mini player / PiP window, custom title bar keeping Windows snap/resize/
maximize, DPI 100–200%, multi-monitor. Click-to-play, double-click fullscreen, mouse-wheel
volume, drag-and-drop files into the window.

**Acceptance:** screenshots match Figma, all shortcuts work.

## Phase 4 — Queue and Library
Phase 4 — Queue & Library.
- Queue panel (drag/drop, reorder, remove, now playing, clear) and "add to queue" everywhere.
- Library: Home (continue watching, recent files/playlists/streams), Videos, Music
  (songs/albums/artists/genres with cover art, tag reading via `lofty`), Network streams
  (paste URL or M3U/M3U8 -> auto playlist), Playlists (create/rename/delete/add from anywhere),
  Recent, Pinned sidebar items.
- Folder watching/scanning in background (incremental, cancellable), thumbnails cache,
  watched/unwatched tracking.
- Toolbar: back/forward, clickable breadcrumb address bar, view modes (grid/list/detail),
  sort (name/date/duration/size/unwatched-first/watched-first), search, overflow menu
  (add file/folder).
- Compact library sidebar inside the player (drag items to queue) and full-screen library view
  with mini player.
- Handle 10k+ items with virtualized lists.

**Acceptance:** add a TV-show folder, browse it and play without leaving the video.

## Phase 5 — Tracks, seek thumbnails, subtitles
Phase 5 — Tracks, seek thumbnails, subtitles.
- Subtitle and audio popovers from the control bar (track lists; audio also lists output
  devices), video tracks via sidebar + context menu only.
- Seek-bar hover thumbnails using a separate headless libVLC player/frame grabber with caching
  and cancellation; must not affect main playback.
- Own subtitle renderer in Slint: SRT + WebVTT first (parse, timing, styling: font, relative/
  precise size, outline, color, position, delay), then ASS/SSA basic styling. Fall back to
  libVLC rendering for unsupported/bitmap formats.
- External subtitle loading (drag&drop, file picker), encoding detection.
- Subtitle timeline data model (cue list with start/end) for Phase 6.

**Acceptance:** change subtitle size/position/delay live without restarting.

## Phase 6 — Right sidebar and presets
Phase 6 — Right sidebar per SPEC.md.
- Subtitles tab: delay slider, font/size/outline/position, relative vs precise sizing, default
  track/language, subtitle timeline (drag to shift sync), "Add subtitles" dialog with target
  (file/folder/playlist) and auto-matching with human-readable match modes (dropdown labels,
  no numbers), manual per-episode matching saved for future episodes.
- Audio tab: delay, default track/language, equalizer (libVLC equalizer) with friendly presets,
  audio filters.
- Video tab: brightness/contrast/saturation/hue/gamma, rotate/flip, zoom, aspect ratio, with
  live preview thumbnails (GPU/shader or CPU on a sampled frame).
- Playback tab: speed, sleep timer, toggles (show subtitle timeline, waveform seek bar,
  metadata).
- PRESETS: save/load/rename/delete named presets (subtitle delay+style+default track, audio
  delay+default track+EQ, video filters), auto-apply to a show/folder, persisted in SQLite.

**Acceptance:** save a preset for a show and have it auto-apply to the next episode.

## Phase 7 — Advanced features
Phase 7 — Advanced features.
- Bookmarks (Ctrl+B quick bookmark, sidebar timeline list, rename/delete, markers on seek bar)
  + chapter list.
- Waveform seek bar (decode audio separately with `symphonia`, cache, downsample).
- Metadata panel.
- OpenSubtitles integration (REST API, user-supplied API key/login in settings, search by
  hash/name, download + auto-match, rate-limit handling, graceful offline).
- Continue-watching prompt placed near the center playback controls with a short countdown.
- Music mode: large cover art, waveform bar, albums/artists views.

## Phase 8 — Settings, hotkeys, onboarding
Phase 8 — Settings, hotkeys, onboarding.
- Settings window with search; categories: General (theme light/dark/system, accent color),
  Interface, Privacy, Controls, Playback, Video, Audio, Subtitles (+OSD), Library, Network,
  Performance, Recording/Streaming, Extensions. Basic settings first, "Advanced" section at
  the bottom of each category. Every setting has an info icon with help text. No numeric-coded
  options; no duplicate settings.
- Hotkeys dialog (own dialog, remap, conflict detection, reset, import/export).
- Onboarding (theme + accent + shortcut intro), shown once.
- Full Arabic translation pass and RTL QA for every screen.

## Phase 9 — Media dialogs
Phase 9 — Media dialogs, each its own simple dialog: Open file, Open folder, Network stream,
Capture device (DirectShow via libVLC), Open from clipboard, Recent media, Convert (libVLC
transcode via sout with presets, progress, cancel), Stream to another device (sout/HTTP/RTSP
presets). Reuse the design system; proper error messages.

## Phase 10 — Layout customization and profiles
Phase 10 — Layout customization. Customize screen with a central frame showing the control
bar; drag & drop to add/remove/reorder controls from a palette; snap guides and outlines; live
preview; reset to default. Layout profiles: each profile stores layout + selected settings;
switch/rename/duplicate/delete/export/import (JSON).

## Phase 11 — Hardening and release
Phase 11 — Hardening & release.
- Performance: startup time, memory, 10k+ library, seek latency, 4K playback, long sessions
  (leak check).
- Robustness: missing/corrupt libvlc, unsupported files, network failures, permission errors;
  crash logging + opt-in diagnostics file.
- Accessibility: keyboard navigation, focus rings, high contrast, screen-reader names.
- Full Figma comparison of every screen at 100/125/150/200% DPI, light + dark, EN + AR.
- Packaging: portable zip + installer (WiX or Inno Setup) bundling libvlc.dll, libvlccore.dll,
  plugins/, app icon, THIRD_PARTY_LICENSES (LGPL notices), file associations, uninstaller.
- GitHub Actions: build, test, package artifacts. Final README and docs/testing.md complete.
