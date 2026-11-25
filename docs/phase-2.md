# Phase 2 summary — Design system and components (VFM)

Status: **complete for review**. One phase per session rule applies; work stops here pending your acceptance.

## What was done

### Token-driven theme

* Added `design/tokens.md` derived component metrics (control heights, icon sizes, focus/border widths, seek/track heights, menu/dialog/tooltip/tab/scrollbar sizes, thumbnail sizes) and a shared `text.on-accent` token.
* Extended `tools/generate_theme.py` so `crates/app/ui/theme.slint` is generated from `design/tokens.md`, including dark/light palettes, runtime `accent`, runtime `rtl`, font selection, type scale, spacing, radii, elevation, motion, and component metrics. Removed the unused `easing` property because Slint 1.19 cannot expose it publicly.
* Refactored `crates/app/ui/spike.slint` so the Phase 0/1 HUD uses only Theme tokens, Strings globals, and the generated SVG control icons. No VLC name, cone, or branded asset appears.

### Fonts

* Bundled Inter Variable and Noto Sans Arabic Variable under `assets/fonts/` with their OFL licenses.
* `crates/app/src/fonts.rs` registers both fonts with fontique before Slint platform initialization, sets generic families, appends fallbacks, and shares the collection with Slint.
* `Theme.font` resolves to the Arabic or Latin family at runtime from `Theme.rtl`.

### i18n and RTL

* `crates/app/src/i18n.rs` holds the English/Arabic catalog with `translate()` and `is_rtl()`. Tests assert every key exists in both languages.
* Added `tools/generate_strings.py`, which generates:
  * `crates/app/ui/strings.slint` — a Slint `Strings` global with one property per key.
  * `crates/app/src/strings_ui.rs` — typed `apply_app()` / `apply_gallery()` bridges.
  * The generated Rust is passed through `rustfmt`, so `cargo fmt --all -- --check` and generator `--check` both pass.
* `--gallery` and the spike HUD language toggle both switch the catalog and set `Theme.rtl`; RTL mirroring is implemented in component logic (fill directions, text alignment, control order, seek/volume mapping) rather than by manually duplicating screens.

### Icon sets

* Added `tools/generate_icons.py`, producing 34 matching SVG icons in each set:
  * `assets/icons/control/` — monochrome control icons.
  * `assets/icons/menu/` — two-tone menu icons (amber accent + neutral gray).
* The shapes are original geometric glyphs. The cone-shaped volume control is an abstract widening volume meter, not a branded media-player cone.

### Reusable components

Added `crates/app/ui/components.slint` with all required components and explicit state coverage:

| Component | States / features |
|---|---|
| `VfmButton` | default / hover / pressed / focus / disabled / selected, optional icon, primary variant, forced preview states |
| `VfmIconButton` | same states, tooltip binding, active state |
| `VfmTooltip` | themed bubble |
| `VfmToggle` | default / hover / pressed / focus / disabled, checked state |
| `VfmSlider` | buffered track, drag, keyboard, RTL-aware fill |
| `VfmSeekBar` | thick seek variant, hover time tooltip, RTL-aware mapping |
| `VfmVolumeCone` | widening abstract cone volume meter, keyboard/drag, RTL-aware |
| `VfmDropdown` | open/closed, selected row, outside popup, keyboard Escape |
| `VfmTabs` | selected underline, hover/pressed |
| `VfmListRow` | selected, trailing text, icon, all pointer/focus states |
| `VfmCard` | thumbnail, progress, selected/hover |
| `VfmGridItem` | thumbnail, progress, selected/hover |
| `VfmContextMenu` | rows, shortcuts, separators, hover |
| `VfmDialog` | modal overlay, primary/secondary actions |
| `VfmSidebarPanel` | header, items, selected row |
| `VfmScrollBar` | draggable thumb, hover/pressed |
| `VfmSearchField` | placeholder overlay, clear action, focus |
| `VfmBreadcrumb` | current crumb, click callback, RTL alignment |
| `VfmToast` | message + action |

### Hidden gallery

* Added `crates/app/ui/gallery.slint`, exported as `GalleryWindow` from `ui/lib.slint`.
* Added `--gallery` to `vfm-spike`. It runs without libVLC, media arguments, SQLite, or SMTC, and provides toolbar controls for dark/light, EN/AR, accent cycling, and close.
* The gallery arranges every component into groups: states, controls, inputs, navigation, data display, overlays, and icon sets.

## Quality gates

All passed on 2026-11-20:

```text
cargo fmt --all -- --check                              OK
python tools/generate_theme.py --check                  OK
python tools/generate_strings.py --check                OK
cargo clippy --workspace --all-targets -- -D warnings   OK
cargo test --workspace                                  7 app + 39 core + 6 wrapper; 0 failures
cargo build --release -p vfm-app                        OK
```

Non-fatal Cargo incremental-lock warnings from this host still appear during clippy; they do not affect the exit code. See `docs/testing.md`.

## Screenshots vs Figma

The gallery is a design-system review surface, not yet a Phase 3 screen, so a pixel-perfect full-screen comparison is not expected. The following captures are provided for token/component comparison against `design/*.png`:

| File | Content | Figma reference |
|---|---|---|
| `docs/compare/phase-2-gallery-dark-en.png` | Dark theme, English, default accent | `design/Player.png`, `design/name=...two-tone=no.png`, `design/right-click menu.png` |
| `docs/compare/phase-2-gallery-dark-ar.png` | Dark theme, Arabic RTL | RTL mirroring intent from the approved brief |
| `docs/compare/phase-2-gallery-light-en.png` | Light theme, English | Derived light palette in `design/tokens.md` |

Observed alignment: amber accent, neutral gray inactive icons, soft radii, compact rows, and the right-click menu row rhythm match the sampled Figma sources. The gallery intentionally uses a denser layout than the final player screen; Phase 3 will do exact screen-level comparison.

## Exact commands to reproduce

From the workspace root in PowerShell:

```powershell
cargo fmt --all -- --check
python tools\generate_theme.py --check
python tools\generate_strings.py --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p vfm-app

# Theme / strings / icon generators (only needed when tokens, keys, or icon shapes change)
python tools\generate_theme.py
python tools\generate_strings.py
python tools\generate_icons.py

# Hidden component gallery; equivalent to clicking the toolbar controls
& .\target\release\VFM.exe --gallery
& .\target\release\VFM.exe --gallery --gallery-theme light
& .\target\release\VFM.exe --gallery --gallery-language ar

# Window captures used above
ffmpeg -y -f gdigrab -framerate 1 -i title="Component gallery" -frames:v 1 docs\compare\phase-2-gallery-dark-en.png
ffmpeg -y -f gdigrab -framerate 1 -i title="Component gallery" -frames:v 1 docs\compare\phase-2-gallery-light-en.png
ffmpeg -y -f gdigrab -framerate 1 -i title="معرض المكوّنات" -frames:v 1 docs\compare\phase-2-gallery-dark-ar.png
```

## What is left / known issues

* `design/SPEC.md` is still missing; behavior continues to come from the approved redesign brief recorded in `design/tokens.md`.
* The gallery is hidden behind `--gallery`; Phase 3 will integrate these components into the real player shell.
* Full-screen Figma pixel comparison is deferred to Phase 3 as specified in `PHASES.md`.
* The 4K RV32 CPU-copy path remains too slow for presentation; the NV12/GPU proposal in `docs/spike.md` is unchanged.
* File associations remain implemented but not validated through the Windows shell UI; real SMTC button presses also remain a manual check.
* Cargo emits non-fatal incremental lock cleanup warnings on this host.
* This workspace is not currently a Git repository, so no Phase 2 commit was created.
