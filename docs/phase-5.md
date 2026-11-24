# Phase 5 summary — Tracks, seek thumbnails, subtitles (VFM)

Status: **complete for review**. Quality gates green; work stops here pending your acceptance.

## What was done

### Subtitle engine (`crates/core/src/subtitles.rs`)

The phase-4 subtitle parser was extended into a full runtime model:

* **`SubtitleSession`** — an ordered cue list plus live delay, size scale, position offset and
  enable flag. `active_at(time_ms)` returns the visible cue with delay and style overrides
  applied **without mutating the source cues**, so changing size/position/delay never restarts
  playback (the phase acceptance criterion). `shift`/`reset_delay`/`set_cues` cover timeline
  sync and track swaps.
* **`subtitle_matches_video`** — stem-based matching for auto-loading (handles
  `Ep.srt`, `Ep.ar.vtt`, `Ep_en.ass` beside `Ep.mkv`).
* 5 new unit tests (19 subtitle tests total): delay application, enable/disable, clamping,
  style overrides, and name matching.

### Seek thumbnails (`crates/core/src/seek_thumbs.rs`, new)

* **`bucket_position`** — quantises a hover position into a stable time bucket so nearby
  hovers reuse one grab.
* **`SeekThumbnailCache`** — bounded, thread-safe LRU with file-backed entries and eviction;
  `GrabCancellation` is a sticky cancel token for in-flight grabs.
* 3 unit tests. The app wires hover → cache lookup → Slint image preview (grabbing new
  frames is the remaining integration step; see Known issues).

### Subtitle UI glue (`crates/app/src/subtitles_ui.rs`, new)

* **`SubtitleController`** — loads external subtitle files with BOM/encoding detection and
  format dispatch by extension; discovers matching sidecars next to a media file.
* **`track_names` / `push_track_names`** — build human-readable track lists for the popovers
  and mark the active track.
* 4 unit tests.

### Player UI (`crates/app/ui/player.slint`)

* **Own subtitle renderer** — a composited `Text` overlay above the video with a four-copy
  outline, white fill, and per-cue font size/colour/position. libVLC's SPU track is disabled
  when our renderer is active, so there are never two overlapping subtitles. The overlay has
  `z: 5` and sits above the video but below the chrome.
* **Audio + subtitle popovers** — opened from the control-bar equalizer/subtitles buttons or
  the right-click menu; list each track with the active one accented, plus "Off" and
  "Load subtitles" rows. Escape dismisses and unpins the chrome.
* **Seek thumbnail preview** — a floating image above the control bar shown when a cached
  thumbnail exists for the hovered time bucket.

### Rust wiring (`crates/app/src/main.rs`)

* New `--subtitle <file>...` flag for explicit external subtitles; otherwise a matching
  sidecar next to the first media file is auto-discovered and loaded.
* Telemetry ticker computes the active cue from the media clock (4 Hz) and pushes the text,
  size, colour, outline and position into the player through `invoke_from_event_loop`, so
  live style/sync changes appear immediately.
* Track selection callbacks call `set_audio_track` / `set_subtitle_track`, refresh the
  popover list, and switch between libVLC rendering (track ≥ 0) and our overlay (Off).
* `on_load_subtitles` discovers and loads sidecar subtitles for the current media.
* 13 new EN/AR i18n keys; generated bindings regenerated.

## Quality gates

```
cargo fmt --all -- --check
python tools/generate_theme.py --check
python tools/generate_strings.py --check
python tools/generate_icons.py --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p vfm-app
```

All pass. Tests: **17 app + 77 core + 6 wrapper = 100** (up from 90). Clippy clean apart from
the host's known non-fatal incremental-lock warnings.

Exact run commands:

```powershell
$env:LIBVLC_PATH = "D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc"
# auto-discovers Ep.srt next to Ep.mkv
.\target\release\vfm-spike.exe .\testdata\phase5\Ep.mkv --no-single-instance
# or explicit
.\target\release\vfm-spike.exe .\testdata\phase5\Ep.mkv --subtitle .\testdata\phase5\Ep.srt
```

## Known issues and deviations

1. **Seek thumbnail generation is not yet wired.** The cache, bucketing, cancellation and UI
   preview are implemented and tested, but no headless libVLC frame grabber populates the
   cache yet, so the preview only appears if a file pre-exists in `seek-thumbs/`. The headless
   grabber is the remaining integration step and must not disturb main playback.
2. **ASS/SSA styling is a basic subset** — script styles, `Dialogue` rows, colours and inline
   italic/bold are parsed; complex override tags, drawing commands and bitmap ASS are not.
   Those fall back to libVLC rendering via track selection.
3. **Encoding detection** covers UTF-8/UTF-16 BOMs with lossy fallback; legacy code pages
   (e.g. Windows-1256 for Arabic SRT) are not auto-detected yet.
4. **Broadcast subtitles** (`subtitle.external`) currently auto-discovers matching sidecars;
   a file-picker dialog is deferred to Phase 9.
5. **Seek thumbnail DPI / RTL** placement follows the control bar but is anchored to the
   window centre, not the exact hover x — acceptable for review, refinable in Phase 11.
6. Pre-existing open items from earlier phases are unchanged: cone-icon branding decision,
   4K GPU path, Figma DPI comparison at 125/150/200 %.
