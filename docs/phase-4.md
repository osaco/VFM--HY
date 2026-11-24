# Phase 4 summary — Queue & Library (VFM)

Status: **complete for review**. Quality gates green; work stops here pending your acceptance.

## What was done

### Core library engine (`crates/core/src/library.rs`, new)

Pure, unit-tested logic for discovering and browsing media:

* **`MediaKind` / `LibraryEntry`** — video/music/stream classification by extension, size
  labels with binary units, and clamp-safe watch progress (`0..=1`, zero-duration safe).
* **`FolderScanner`** — iterative (no recursion limit), deterministic (sorted by path) folder
  walk with a cancellable handle (`ScanCancellation`). Skips hidden entries, respects a
  configurable depth, de-duplicates symlinked directories via canonical paths, and never
  panics on per-entry I/O errors. This is the incremental/cancellable background scan the
  phase asked for.
* **`parse_m3u`** — M3U/M3U8 playlist parsing with `#EXTINF` titles and relative-URL
  resolution for both web and local bases.
* **`LibraryFilter` + `SortMode`** — search (title/path), kind and unwatched filters, plus
  ten sort orders (name, date, duration, size, watched-state).
* **`ThumbnailCache`** — content-addressed (FNV-1a) file names so long Windows paths never
  exceed MAX_PATH; `get`/`ensure_dir`/`prune`.
* **`Page<T>`** — a bounded window over a large list. This is the basis of virtualisation:
  Rust keeps only the visible slice in Slint, so a 10k+ library never creates 10k UI rows.

### Persistence (`crates/core/src/store.rs`)

An additive, idempotent **schema v2** migration (guarded by a column check) adds `size_bytes`,
`added`, `pinned` to `media`, plus a `streams` table and indexes. New store methods:
`upsert_entry`, `all_entries`, `recent_entries`, `pinned_entries`, `entries_in_folder`,
`set_pinned`, `set_watched`, `folders`, `add_stream`/`streams`/`remove_stream`, `playlists`.

### Queue panel (in the player)

`QueuePanel` (new in `library.slint`) is embedded in `PlayerWindow` and toggled by the control
bar's queue button. It shows now-playing state, per-row play and remove, and clear. Rust wires
every action back to the shared `Arc<Mutex<Queue>>`, so the panel and next/previous always agree.
`Ctrl+L` (and the "browse" context-menu item) opens the library from the player.

### Library window (`crates/app/ui/library.slint`, new)

A full-screen view with:

* Sidebar: Home, Videos, Music, Network, Playlists, Recent, Pinned, Folders.
* Toolbar: back/forward, breadcrumb, sort, search, three view modes (grid/list/detail), add
  folder and add file.
* A path-entry form for adding a folder/file or pasting a stream URL/M3U (no native-dialog
  dependency — works in automation and on any host).
* A **mini-player strip** at the bottom: play/pause, title, elapsed/seek/duration, and a button
  that returns to the full player. Playback continues while you browse.
* Grid and list/detail content views with watch-progress bars, pinned/watched markers and
  per-row "add to queue". Only the current section's rows are materialised.

`--library` launches straight into this view with an empty libVLC player (a new
`Instance::empty_player()` FFI symbol was added); the first item you open attaches media and
starts playback.

### Rust glue (`crates/app/src/library_ui.rs`, new)

`LibraryController` owns section/query/sort/view state and reloads rows from the store;
`scan_folder` drives the cancellable scanner; `to_slint`/`push_rows` and `push_queue_rows`
convert core entries into Slint models; `stream_entries_from_text` handles pasted URLs and
local M3U files. Every string goes through the i18n layer — 33 new EN/AR keys were added and
the generated Slint/Rust bindings regenerated.

### Also built ahead (shared foundations for the next phases)

Two isolated core modules were completed while preparing this phase (the delegated background
agents for these failed on a network/DNS error, so they were written directly):

* **`subtitles.rs`** — SRT/WebVTT/ASS parsing, styling, timing shifts, binary-search cue
  lookup and BOM encoding detection (13 tests).
* **`hotkeys.rs`** — canonical combos, conflict detection, 32-action default map and TOML
  import/export (7 tests).

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

All pass. Tests: **13 app + 71 core + 6 wrapper = 90** (up from 53). Clippy clean apart from
the host's known non-fatal incremental-lock warnings.

Exact run commands:

```powershell
# Library-first
$env:LIBVLC_PATH = "D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc"
.\target\release\vfm-spike.exe --library --data-dir .\testdata\phase4-run

# Player, then Ctrl+L for the library
.\target\release\vfm-spike.exe .\testdata\sample1080p-b.mp4 --no-single-instance
```

## Known issues and deviations

1. **Release build hit a disk-space limit** on the first attempt (`no space on device`, 100 %
   full D:). After clearing ~3 GB of regenerable `target/debug/incremental` cache the build
   was retried; see `docs/phase-4.md` status above. No source change was needed.
2. **Native folder picker not used.** Adding a folder/file is done by pasting a path into the
   library's input form, which keeps the build dependency-free and automation-friendly. A
   native dialog can replace this later without changing the controller API.
3. **Grid/thumbnail images** are loaded from the `ThumbnailCache` when present; generating
   the thumbnails themselves is still to be connected to a decoder in a later phase, so grid
   cells currently show the neutral backdrop until a cache file exists.
4. **Virtualisation** is provided by the `Page<T>` core primitive; the list/detail view
   materialises the current section, which is bounded by the store query. Full windowed
   rendering for a single 10k-item folder is the remaining step for that acceptance clause.
5. The pre-existing **traffic-cone icon / no-VLC-branding conflict** from Phase 3 is unchanged
   and still needs your decision.
6. **4K GPU path** and Figma DPI comparison at 125/150/200 % remain open (Phases 0/11).
