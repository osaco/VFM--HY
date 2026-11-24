# Phase 3 summary — Player view (VFM)

Status: **complete for review**. One phase per session rule applies; work stops here pending your acceptance.

## What was done

### Player screen rebuilt against measured Figma geometry

`design/Player.png` is a 7680×4320 export of a 1920×1080 frame, so it carries 4× detail. Every
control-strip metric was measured from it with `tools/scan_pixels.py` (new: scans a row or column
through ffmpeg and prints colour runs) instead of being eyeballed:

| Element | Figma (1920×1080) | Implementation |
|---|---|---|
| Title bar | 40 px, `#1B1B1B` | `Theme.player-topbar-height` |
| Control strip | 54 px, `#1B1B1B` | `Theme.player-bottombar-height` |
| Play button | 54 × 28, radius 8, accent fill | `player-play-width/height` |
| Icon hit target | 42 × 42, 20 px glyph | `Theme.player-icon-button` |
| Seek track | 28 px pill, `#090909` | `Theme.seek-height` |
| Played pill | 24 px, `accent.soft`, inset 2 px | `Theme.seek-fill-height` |
| Playhead thumb | 8 × 24, full accent | `Theme.seek-thumb-width` |
| Volume cone | 90 × 28 tapered wedge, `#7BA213` fill | `Theme.volume-cone-width/height` |

`crates/app/ui/player.slint` was rewritten around those numbers:

* **Title bar** — 40 px solid bar: brand chip on the left (doubles as the settings entry point),
  centred muted filename, then PiP / minimise / maximise-restore / fullscreen / close on the right.
  Dragging is handled by `WindowMoveArea`, so Windows snap, resize, and double-click-maximise keep
  working on a frameless window.
* **Control strip** — one 54 px row: previous, enlarged play/pause, next, elapsed, thick seek bar,
  duration, speaker, tapered volume cone, then audio / subtitles / settings / queue. The right-hand
  cluster is anchored from the inset inward (queue hugs the edge, the gear sits 50 px in, subtitles
  and audio step back on a 42 px pitch) so it matches Figma exactly rather than approximately.
* **Seek bar** — `VfmSeekBar` is now the player variant: 28 px track, 24 px played pill, 8 px thumb
  that widens on hover, buffered range, hover time tooltip, drag-to-scrub, arrow-key scrubbing.
* **Volume** — `VfmVolumeCone` is a real tapered wedge drawn with `Path` (not a stepped bar meter),
  green fill, mute state, drag and arrow-key control.
* **RTL** — a single `mx(x, w)` helper mirrors every anchored slot, so the Arabic layout is the same
  code path rather than a duplicated screen. Verified: transport moves to the right, seek fills from
  the right, volume and the icon cluster move to the left, text alignment flips.
* **Chrome behaviour** — auto-hide after 2 s of playback, reveal on mouse move, pin while the
  context menu is open, edge strips to bring the chrome back, `Ctrl+\` for Hide UI.
* **Right-click menu** — icon + label + shortcut rows, Escape/outside-click dismiss.
* **Shortcut OSD** — key chip + action label, 900 ms auto-dismiss.
* Removed the Phase-2 left icon rail: neither `Player-2/3/4.png` (player-only) nor `Player.png`
  (player + library) shows an icon rail in the player view. The compact library sidebar in
  `Player.png` belongs to the Phase 4 queue/library work.

### Rust wiring

* Play/pause, stop, next/previous (queue-aware), snapshot record, mute, volume wheel, seek hover
  tooltip, PiP show/hide with the frame mirrored to the second window, minimise, maximise, quit.
* Drag-and-drop enqueues dropped files and starts the first one.
* Transport read-out is now marshalled through `slint::invoke_from_event_loop`. **This fixed a real
  bug:** `Weak::upgrade()` returns `None` when called from a worker thread, so the clock, seek
  position, play state, and mute state had silently never updated since Phase 1 — the UI had been
  showing the design-time defaults. The presenter thread already used the event-loop pattern, which
  is why video frames were fine while the transport was frozen.
* The player now honours `settings.theme` and `settings.language` at startup instead of always
  booting dark/English, and mirrors both into the PiP window (Slint globals are per-window).
* Time labels use `format_timecode_long` (`HH:MM:SS`) to match Figma's `00:02:10`.

### Design-system fixes found while matching Figma

* **`accent.soft`, `accent.hover`, `accent.pressed` were inverted.** Slint's `Color.mix(other, f)`
  weights `self` by `f`, so `accent.mix(bg-surface, 0.88)` was 88 % accent — the "soft" wash was
  nearly the full accent, and hover/pressed were nearly white/black. Corrected to
  `accent-soft = accent.mix(bg-surface, 0.14)`, `accent-hover = accent.mix(#FFFFFF, 0.88)`,
  `accent-pressed = accent.mix(#000000, 0.88)`. This also fixes every Phase-2 hover and selected
  row, which had been over-saturated.
* Added `volume.fill` (`#7BA213` dark / `#5E7D0E` light) and the player metrics to
  `design/tokens.md`; `tools/generate_theme.py` now emits them from a single ordered list and
  fails if the token table and the generator drift.
* `tools/generate_icons.py` gained `--check` and two ink-on-accent glyphs
  (`play-on-accent.svg`, `pause-on-accent.svg`) so the play button can carry a dark glyph on the
  accent fill without a colour hack.

### Application icon

* Added `assets/branding/vfm.ico` (your file) plus a derived `vfm-256.png`.
* All four windows (player, PiP, spike, gallery) now set `icon:`, so the taskbar and Alt-Tab show
  the icon — confirmed in the taskbar capture.

## Quality gates

```powershell
cargo fmt --all -- --check
python tools\generate_theme.py --check
python tools\generate_strings.py --check
python tools\generate_icons.py --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p vfm-app
```

All pass. Tests: **8 app + 39 core + 6 wrapper = 53**, unchanged counts, no regressions.
Clippy is clean apart from the host's known non-fatal incremental-lock warnings.

## Screenshots vs Figma

| File | What it shows |
|---|---|
| `docs/compare/phase-3-vs-figma.png` | Side by side, same width: `design/Player-2.png` (left) vs implementation (right) |
| `docs/compare/phase-3-strip-vs-figma.png` | Control strips stacked at 1920 px: Figma (top) vs implementation (bottom) |
| `docs/compare/phase-3-player-dark-en-1920.png` | Full window rendered on a 1920-wide canvas at 100 % scale |
| `docs/compare/phase-3-player-dark-en.png` | Dark theme, English, default window |
| `docs/compare/phase-3-player-light-en.png` | Light theme, English |
| `docs/compare/phase-3-player-dark-ar.png` | Arabic RTL — mirrored chrome |

The strip comparison is the pixel-fidelity check: previous / play / next, the time labels, the seek
track start and end, the duration label, the speaker, the cone and all four icons land within a few
pixels of Figma.

## Known issues and deviations

1. **Icon branding conflict — needs your decision.** `VFM icon.ico` is an orange traffic cone with a
   melting smiley. A traffic cone is the VLC trademark shape, and this project's standing rule is
   "no VLC name, cone, or branding". It is wired in because you supplied it, but if you distribute
   the app this is the one asset most likely to attract a trademark complaint. Say the word and I
   will generate a non-cone mark (for example a play glyph in the accent chip) and swap it.
2. **`.exe` file icon is not embedded yet.** The window/taskbar icon works at runtime, but Explorer
   still shows the default binary icon because embedding a PE resource needs `rc.exe` (absent from
   this toolchain) or a resource crate that is not in the offline registry. This belongs to Phase 11
   packaging.
3. **Settings glyph differs by one icon.** Figma's strip shows a gear containing a play triangle
   (playback settings); the implementation uses the plain gear because that button opens Settings.
   Same slot, same size, same position.
4. **Centred filename in the title bar** is a deliberate reconciliation: the Phase 3 brief asks for a
   title in the top bar, neither Figma frame shows one. It is muted 12 px so it stays quiet.
5. **System theme is not detected.** `theme = "system"` boots dark until the settings phase adds
   `should_dark()` detection.
6. **DPI verified at 125 % only.** This host runs 125 %, so a 1280×720 logical window captures at
   1600×900 physical — that is why the default screenshots are 1600×900 and the 1:1 comparison uses
   a 1920-wide canvas with `SLINT_SCALE_FACTOR=1`. 100/150/200 % and multi-monitor placement still
   need a manual pass.
7. **Context-menu and OSD screenshots not captured.** Both are implemented and covered by the
   gallery, but this host has no scriptable right-click, so the captures would have needed a demo
   switch. I did not add one.
8. **4K performance unchanged** — the RV32 CPU copy path is still the bottleneck; the NV12/GPU
   proposal from Phase 0 remains open.

## Exact commands

```powershell
# build
cargo build --release -p vfm-app

# run with the local libVLC (default window, 125 % host)
$env:LIBVLC_PATH="$PWD\vendor\libvlc"
.\target\release\vfm-spike.exe .\testdata\1080p60.mp4 --width 1920 --height 1080 --chroma rv32 `
    --no-single-instance --data-dir .\testdata\phase3-run

# 1:1 Figma comparison (100 % scale, 1920-wide canvas)
$env:SLINT_SCALE_FACTOR="1"

# force a theme/language for screenshots
#   edit .\testdata\phase3-run\settings.toml: theme = "light" | "dark", language = "en" | "ar"

# capture the window (title changes with the language)
ffmpeg -y -f gdigrab -framerate 1 -i title="Vedio for me" -frames:v 1 out.png
ffmpeg -y -f gdigrab -framerate 1 -i title="فيديو من أجلي" -frames:v 1 out-ar.png

# measure Figma geometry
python tools\scan_pixels.py row 4195 0 1500 --tol 10
python tools\scan_pixels.py col 1200 4080 4320 --tol 8

# component gallery (Phase 2 regression check)
.\target\release\vfm-spike.exe --gallery --gallery-theme light --gallery-language ar
```
