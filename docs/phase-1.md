# Phase 1 summary — Core engine (VFM)

Status: **complete for review**. One phase per session rule applies; work stops here pending your acceptance.

## What was done

### Playback wrapper (`vlc-wrapper`)

* Extended the runtime-loaded libVLC surface in `sys.rs` and the safe façade in `lib.rs`:
  play/pause/stop, time/length/position/rate, frame-step, audio/subtitle/video tracks, external subtitle loading, audio/subtitle delay, aspect ratio, snapshot, chapters, and media swap for queue use.
* Added a typed event bridge in `events.rs`:
  C callback reads only the leading `i32 type`, uses bounded `try_send`, makes no libVLC calls, and `EventHub::drop` detaches all callbacks before user data is freed.
* Kept all FFI `unsafe` inside this crate. `EventHub::attach` is now `pub(crate) unsafe` with a documented safety contract; the public API remains safe.

### Core (`vfm-core`, `#![forbid(unsafe_code)]`)

* Added pure state machine (`CoreEvent` / `PlayerAction`) with A–B repeat, end-of-media progress save, and queue advance.
* Added transient `Queue` (shuffle, repeat, reorder, previous history) separate from saved `Playlist`.
* Added `Playlist`, `WatchProgress` resume policy, `TelemetryAccumulator`, timecode parsing/formatting, versioned TOML `SettingsStore`, and SQLite `LibraryStore`.
* Fixed three acceptance-run bugs:
  1. Short clips were marked watched immediately — watched tail is now `min(5%, 30 s)`.
  2. `upsert_media(path, title, 0)` erased a known duration and broke resume — zero now preserves the stored duration.
  3. `set_time_ms` before `play()` was ignored by libVLC — engine harness now seeks after play and retries briefly.
* Replaced `Result<_, ()>` queue errors with `InvalidIndex`, renamed `Queue::next` to `advance`, removed complex subscriber type and dead code, and added regression tests.

### App (`vfm-app`, `#![deny(unsafe_code)]`)

* Added `--engine-test`, `--data-dir`, `--no-single-instance`, `--associate`, and `--unassociate` CLI paths.
* Added `engine_test()` headless harness: two-file sequential playback, typed events → `PlayerStateMachine` → actions, resume, watch-progress persistence.
* Added Windows helpers for HKCU associations, single-instance lock file, PID liveness check, and window geometry sanitization.
* Added native drag & drop via Slint `DropArea`: dropped files are appended to the queue and the first one starts playing.
* Added SMTC/media-key support using the official Windows bindings. The WinRT callback only sends a `MediaKey` over a channel; a Slint timer applies play/pause/stop/next/previous on the UI thread.
* Wired window restore/save into the Slint shell; `--associate` no longer requires a media argument; `F` now toggles fullscreen and arrows seek.
* Kept Phase 0 RV32/RGBA callback pipeline, HUD, and EN/عربي RTL toggle intact.

### Documentation and data

* `docs/spike.md` — Phase 0 benchmark and optimization path (unchanged this phase).
* `docs/testing.md` — new automated gates, headless acceptance procedure, manual checklist, thread-safety invariants, known limitations.
* `design/source.fig` — editable design archive supplied during this phase; `design/source-thumbnail.png` extracted. `CLAUDE.md` and `design/tokens.md` now record it as an input while PNGs remain the visual source of truth.
* `crates/app/src/smtc.rs` — safe WinRT SMTC bridge with documented thread boundary.
* `testdata/sample1080p-b.mp4` — generated second 1080p clip for resume testing.
* `testdata/engine-run-verified/` — verified SQLite + settings output from acceptance runs.

## Quality gates

All passed on 2026-11-20:

```text
cargo fmt --all -- --check                          OK
cargo clippy --workspace --all-targets -- -D warnings OK
cargo test --workspace                             4 app + 39 core + 6 wrapper; 0 failures
cargo build --release -p vfm-app                    OK
```

## Headless acceptance evidence

Clean directory, first run:

```text
SMTC attached
[0] play ./testdata/1080p60.mp4
[0] saved progress 0:02 / 0:35 (watched=false)
[1] play ./testdata/sample1080p-b.mp4
[1] saved progress 0:02 / 0:12 (watched=false)
engine test complete: 2 file(s), database at ./testdata/engine-run-verified\library.sqlite
```

Second run, same directory:

```text
[0] resume ./testdata/1080p60.mp4 at 0:02
[0] saved progress 0:04 / 0:35 (watched=false)
```

Verified SQLite row after three runs:

```text
('./testdata/1080p60.mp4', 6135, 35000, 0)
('./testdata/sample1080p-b.mp4', 1403, 12000, 0)
```

`settings.toml` contains `version = 1`, `language = "en"`, `accent = "#D37918"`, `remember_position = true`, and `resume_min_remaining_ms = 30000`.

## Screenshots vs Figma

**N/A for Phase 1.** The visible UI is still the Phase 0 engineering shell, not a Figma screen. `design/source.fig` is preserved for future component inspection, but pixel comparison begins in Phase 3 as specified in `PHASES.md`.

## Exact commands to reproduce

From the workspace root in PowerShell:

```powershell
$env:LIBVLC_PATH = "$PWD\vendor\libvlc"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p vfm-app
```

Headless engine test:

```powershell
& .\target\release\VFM.exe `
  .\testdata\1080p60.mp4 .\testdata\sample1080p-b.mp4 `
  --engine-test 3 --width 1920 --height 1080 --chroma rv32 `
  --no-single-instance --data-dir .\testdata\engine-run-check
```

GUI spike (video in Slint image, HUD overlaid):

```powershell
& .\target\release\VFM.exe .\testdata\1080p60.mp4 `
  --width 1920 --height 1080 --chroma rv32
```

File associations (optional, HKCU only):

```powershell
& .\target\release\VFM.exe --associate
& .\target\release\VFM.exe --unassociate
```

## What is left / known issues

* File associations are implemented via `reg.exe` but not yet validated through the Windows shell UI.
* SMTC attachment was verified headlessly (`SMTC attached`); actual system button presses still need a manual check on the target machine.
* `design/SPEC.md` is still missing; `CLAUDE.md` names it as a behavioral source of truth. The newly supplied `design/source.fig` does not replace it.
* The video callback path still assumes one fixed width/height per player, and 4K RV32 CPU copy remains too slow for presentation (see `docs/spike.md`); the YUV/GPU path is the Phase 0 proposed optimization.
* Wrapper control APIs exist for tracks, delays, snapshot, A–B, and frame-step, but their UI controls arrive in Phases 5–7.
* This workspace is not currently a Git repository, so no Phase 1 commit was created.
