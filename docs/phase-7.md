# Phase 7 summary — Advanced features (VFM)

Status: **complete for review**. Bookmarks, chapters, the waveform seek bar, the metadata panel,
OpenSubtitles search, the continue-watching prompt and music mode are wired end to end; final
quality-gate output is recorded below.

## What was done

### Bookmarks (`crates/core/src/bookmarks.rs`, new)

* `Bookmark { id, media, time_ms, name }`, keyed by media path so a bookmark travels with the
  file rather than with a queue slot.
* `add` de-duplicates within `DEDUPE_TOLERANCE_MS` (1.5 s): pressing `Ctrl+B` twice on the same
  moment renames and re-times the existing row instead of creating a pair. An empty label falls
  back to the timecode, so the list never shows a blank row.
* `list`, `rename`, `remove`, `neighbour` (walk forwards/backwards and stop at the ends) and
  `closest` (highlight the active row).
* `LibraryStore` gained `bookmarks_with_id`, `rename_bookmark`, `delete_bookmark` and
  `move_bookmark`; `add_bookmark` now returns the new row id.
* 9 tests.

### Chapters, tags and tracks (libVLC layer)

* New FFI: `libvlc_media_parse`, `libvlc_media_get_meta`, `libvlc_media_tracks_get/release`,
  `libvlc_media_player_get_full_chapter_descriptions`, `libvlc_chapter_descriptions_release`,
  with `#[repr(C)]` mirrors of `libvlc_track_t`, `libvlc_audio_track_t`,
  `libvlc_video_track_t` and `libvlc_chapter_description_t`.
* Safe wrappers: `Media::metadata()`, `Media::tracks()`, `Media::parse()`,
  `MediaPlayer::chapters()`.
* **`parse()` matters.** `libvlc_media_tracks_get` returns an empty list and
  `libvlc_media_get_meta` returns empty strings on a handle that has never been played. Both
  readers now parse on demand, and the app cross-checks "audio only" against the live player
  (`video_track_count() == 0`) so a half-parsed file can never hide a real video track. This
  bug showed up as music mode covering a video that was clearly playing.
* 4 new wrapper tests.

### Waveform (`crates/core/src/waveform.rs`, new)

* `analyse` decodes with symphonia 0.5 into `DEFAULT_BUCKETS` (900) peak buckets, normalises to
  0..1 and downsamples for display. Decode errors skip a packet instead of discarding the file;
  a video without an audio track reports `WaveformError::NoAudio`.
* `Waveform::to_bytes` / `from_bytes` — a 12-byte header plus little-endian `f32` peaks, cached
  per file by `WaveformCache` (FNV-1a key over path, size and mtime).
* Analysis runs on its own thread; the result is pushed to the UI as a plain `[float]` model.
* 7 tests, including a generated WAV and a video-only file.

### Waveform seek bar (`components.slint`, `player.slint`)

* `VfmSeekBar` gained `markers: [float]` and `waveform-peaks: [float]`.
* Bookmark ticks are 3×12 px rounded bars over the track, mirrored in RTL.
* **The waveform is one `Rectangle` per bucket, not a `Path`.** A `Path` keeps the viewbox
  aspect ratio, so a 1000×100 viewbox inside a 505×24 bar scaled to 24 % of the width and sat
  centred in the middle of the bar. One bar per bucket stretches exactly, and the played accent
  fill covers the played portion with no second colour ramp.
* RTL mirroring is done in Rust (`waveform_peaks_for(..., rtl)`) rather than with
  `transform-scale-x`, which is exact and testable.

### Timeline panel (`sidebar.slint`, `timeline_ui.rs`)

* The Playback tab now carries the chapter list, the bookmark list and the metadata panel under
  the existing speed/sleep/toggle rows. `TimelineRow { name, time, position, active }` and
  `MetadataRow { label, value }` are pushed from Rust.
* Chapters number themselves when the container has no titles; the chapter containing the
  playhead is highlighted, and an open-ended last chapter stays active to the very end.
* Bookmarks: click a row to seek, "Add bookmark" in the section header, "Remove" below the list.
  The list scrolls; the Flickable's `content-height` grows with the row counts.
* Metadata panel shows only populated fields: title, show, season, episode, artist, album,
  album artist, genre, date, publisher, actors, language, then `WxH · fps · kb/s` per video
  track and `ch · Hz · kb/s` per audio track.
* 12 tests in `timeline_ui` (row builders, markers, mirroring, localisation).

### OpenSubtitles (`crates/core/src/opensubtitles.rs`, `crates/app/src/subs_search.rs`, new)

* Pure helpers: `movie_hash` (size + sum of LE `u64` words over the first and last 64 KiB, 16
  hex digits), `hash_search_url`, `name_search_url`, `parse_search_response`,
  `parse_download_response`, `error_for_status` (401/403 → Rejected, 429 → RateLimited).
* `OpenSubtitlesClient` over `ureq` 3 with `native-tls` (Windows SChannel — no ring/rustls
  toolchain) and a 20 s global timeout.
* Search and download run on worker threads and push plain rows back through
  `invoke_from_event_loop`, so the player never blocks on HTTP.
* `safe_file_name` strips any directory part from the server-supplied name, so a download can
  never write outside `<data-dir>/subtitles`.
* An empty query means "search this exact file by hash"; typing a name switches to a title
  search. The dialog stays inert without an API key — no request is ever sent implicitly.
* The key lives in `Settings::opensubtitles_api_key` (serde default, so older `settings.toml`
  files still load) and is committed when the dialog closes.
* 9 core tests + 5 app tests.

### Continue watching

* When a file with saved progress is opened, the ticker parks the playhead at the saved position
  and shows a "Continue watching" card above the control strip with `Resume` / `Start over`.
* **Pause is retried.** libVLC drops `set_pause` while the demuxer is still opening, so the
  first attempt drifted ~15 s past the saved point. The seek lands immediately and the pause is
  retried each tick until `libvlc_Playing`, re-seeking to the target as it does so.

### Music mode

* Audio-only files replace the video stage with a cover card built from the accent colour, the
  track title (falling back to the file name) and `artist · album` from the tags.
* The waveform is forced on for audio-only media even when the sidebar toggle is off, because it
  is the only thing on screen during music playback.

### App glue (`main.rs`)

* The 4 Hz ticker is the single place that notices a media change, so every open path (startup,
  drag-and-drop, queue, media keys) refreshes chapters, bookmarks, tags, the waveform and music
  mode the same way. The refresh also fires once more when the duration first becomes known
  (libVLC reports 0 while opening).
* An empty path clears the timeline instead of leaving the previous file's rows attached.
* `Ctrl+B` adds a bookmark and reveals the Playback tab; `Ctrl+,` / `Ctrl+.` jump to the
  previous/next bookmark.
* Phase 7 added 29 EN/AR i18n keys; generated `strings.slint` / `strings_ui.rs` regenerated.

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
| `cargo test --release --workspace` | **160 passed / 0 failed** — 42 app, 108 core, 10 wrapper |
| `cargo build --release -p vfm-app` | `Finished release profile in 1m 49s` |

Phase 6 ended at 118 tests; Phase 7 adds 42 (bookmarks 9, waveform 7, OpenSubtitles 9,
timeline_ui 12, subs_search 5).

`cargo test` needs `sqlite3.dll` beside the test binaries on this host (the bundled SQLite is
not used — see `.cargo/config.toml`). Copy `vendor/sqlite/lib/sqlite3.dll` into
`target/release/deps/` once per fresh target directory.

## Screenshots

| File | Shows |
| --- | --- |
| `docs/compare/phase-7-timeline-dark-en.png` | Playback tab: active chapter highlighted, bookmark list with `Add bookmark`, white ticks on the seek bar |
| `docs/compare/phase-7-music-mode-dark-en.png` | Music mode: cover card, `Late Sessions` / `Nour Haddad · Night Drive`, full-width waveform |
| `docs/compare/phase-7-continue-watching-dark-en.png` | "Continue watching" card parked at `00:01:00 / 00:03:00` with Resume / Start over |
| `docs/compare/phase-7-opensubtitles-dark-en.png` | OpenSubtitles dialog: API-key field, query + Search, status line, Close |
| `docs/compare/phase-7-timeline-dark-ar.png` | Arabic RTL: panel mirrored to the left, chapter/bookmark rows with timecodes on the leading edge |

## Run commands

```powershell
$env:LIBVLC_PATH = "D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc"
.\target\release\VFM.exe "D:\media\episode.mkv" --no-single-instance
# --sidebar 3 opens the Playback tab with chapters and bookmarks.
# Ctrl+B bookmarks the current moment; Ctrl+, / Ctrl+. jump between bookmarks.
# --subs-search opens the OpenSubtitles dialog directly (used for the screenshot above).
```

## Known issues and deviations

1. The Playback tab now holds more content than a 340 px panel shows at once (speed, sleep,
   three toggles, chapters, bookmarks, metadata). It scrolls with the wheel but has no visible
   scrollbar yet; a `VfmScrollBar` binding is the obvious follow-up.
2. The waveform is regenerated only when a file is opened, so toggling RTL mid-session leaves
   the strip in its previous direction until the next file.
3. Album art is not extracted; music mode draws an accent-coloured placeholder. Reading embedded
   covers needs an image decode path (libVLC `libvlc_media_get_meta` returns no artwork).
4. OpenSubtitles needs a user-supplied API key; there is no key-provisioning flow.
5. Rotation/flip, the variable subtitle outline and the headless seek-thumbnail grabber remain
   open from earlier phases.
