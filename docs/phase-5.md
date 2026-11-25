# Phase 5 summary — Tracks, seek thumbnails, subtitles (VFM)

Status: **complete for this host**. Subtitles, track selection, the popover UI and the headless
seek-thumbnail grabber all work; the remaining caveats are listed under Known issues.

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
* **`encode_rgba_png`** — turns the raw RGBA8 frame from libVLC's callbacks into a PNG for the
  cache; round-trip unit test decodes it back and compares every byte.
* 4 unit tests. The app wires hover → cache lookup → headless grab → Slint image preview.

### Subtitle UI glue (`crates/app/src/subtitles_ui.rs`, new)

* **`SubtitleController`** — loads external subtitle files with BOM/encoding detection and
  format dispatch by extension; discovers matching sidecars next to a media file.
* **`live_tracks` / `track_id_at` / `push_track_names`** — build the popover list from libVLC's
  own `libvlc_audio_get_track_description` / `libvlc_video_get_spu_description` output, keep the
  real ids beside each row, and map a clicked row back to the id libVLC expects. A file with no
  subtitle track now shows an empty list instead of an invented "Sub 1" row.
* 6 unit tests (real names, sparse ids, the Off row, empty list).

### Player UI (`crates/app/ui/player.slint`)

* **Own subtitle renderer** — a composited `Text` overlay above the video with a four-copy
  outline, white fill, and per-cue font size/colour/position. The outline offset is driven by
  `subtitle-outline-width` (fed from the cue style), so the outline slider changes the actual
  rendering instead of a stored value. libVLC's SPU track is disabled when our renderer is
  active, so there are never two overlapping subtitles.
* **Audio + subtitle popovers** — opened from the control-bar equalizer/subtitles buttons or
  the right-click menu; list each track with the active one accented, plus "Off" and
  "Load subtitles" rows. Escape dismisses and unpins the chrome.
* **Seek thumbnail preview** — a floating image above the control bar, populated by a headless
  libVLC player for the hovered time bucket. The preview is centred above the bar rather than
  following the exact hover x.

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
.\target\release\VFM.exe .\testdata\phase5\Ep.mkv --no-single-instance
# or explicit
.\target\release\VFM.exe .\testdata\phase5\Ep.mkv --subtitle .\testdata\phase5\Ep.srt
```

## Known issues and deviations

1. **Seek thumbnail generation is wired and verified.** A private libVLC instance with audio
   disabled decodes one frame per hovered bucket through the same callback bridge as the main
   player, encodes it as PNG and stores it in `seek-thumbs/`. It waits for the engine clock to
   reach the target before taking a *new* frame, otherwise every thumbnail is the first frame.
   Fast sweeps cancel older grabs; a grab never opens a window or touches main playback. Proof:
   a 60 s clip with four 3 s colour segments produced a blue thumbnail for 7.5 s and a red one
   for 37.5 s, together with the `0:37` tooltip (`docs/compare/phase-5-seek-thumbnail.png`).
   libVLC seeks to keyframes, so a preview can land a few seconds before the hovered bucket —
   these are approximate previews, not frame-exact stills.
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
