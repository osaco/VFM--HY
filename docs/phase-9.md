# Phase 9 summary — Media dialogs (VFM)

Status: **partly complete**. One dialog covers every way media enters the player — file,
folder, network stream, capture device, clipboard, recents — plus conversion and streaming
from the current file. Native Windows pickers replace the placeholder "open Explorer" button,
including the settings folder fields. Capture enumeration, exact transcode progress and a
separate-client RTSP test remain open (see Known issues).

## What was done

### Platform pickers (`crates/platform`)

* `pick_files(title, label, extensions)` — `IFileOpenDialog` with a file-type filter and
  multi-select; returns real filesystem paths only (`FOS_FORCEFILESYSTEM`).
* `pick_folder(title)` — the same dialog with `FOS_PICKFOLDERS`.
* `clipboard_text()` — `CF_UNICODETEXT` read with a bounded scan, so a pathological clipboard
  cannot spin the UI thread.
* Each entry point initialises the COM apartment it uses and tears it down again, and cancels
  are reported as "no selection" rather than as errors.
* The settings window's **Browse** now opens this picker instead of Explorer; Explorer is only
  the fallback when the dialog is cancelled on an existing folder.

### Open-media rules (`crates/core/src/open_media.rs`, new)

* `classify(input)` decides what a typed string means: a known scheme (`http`, `https`, `rtsp`,
  `rtp`, `udp`, `mms`, `ftp`, `sftp`, `smb`, `file`, `dshow`, `screen`) is a location, a
  Windows path or an existing file is a file, and a bare host (`example.com/live.m3u8`,
  `10.0.0.5:8080`) gains `http://` rather than being mistaken for a filename.
* `CaptureSpec` builds `dshow://` plus `dshow-vdev`, `dshow-adev`, `dshow-size` and
  `dshow-fps`; empty device names are omitted so libVLC picks the system default, and names
  with spaces are quoted.
* `CONVERT_PRESETS` (MP4/H.264, WebM/VP8, MKV/H.264, MP3, M4A) and `STREAM_PRESETS`
  (HTTP-TS, HTTP-PS, RTSP) describe each target once; `convert_sout` and `stream_sout` turn a
  preset into a libVLC `#transcode{…}` chain. HTTP presets end in `:std{access=http,…}`; the
  RTSP preset ends in `:rtp{sdp=rtsp://…}`, which starts a real RTSP server instead of
  mislabelling an HTTP stream.
* Audio-only presets emit `vcodec=none` — without it libVLC tries to mux a video track into an
  MP3 and fails at open time.
* 13 tests.

### Dialog (`crates/app/ui/open.slint`, `crates/app/src/open_dialog.rs`, new)

* Left rail with eight panels: File, Folder, Network stream, Capture device, From clipboard,
  Recent media, Convert, Stream. The footer's primary button follows the panel
  (Play / Convert / Start streaming) and the status line reports errors inline.
* `OpenDialogState` holds every field and derives the plans: `target()` for the source panels,
  `convert_plan()` and `stream_plan()` for the two action panels. It refuses to convert onto
  the file libVLC is reading, fills the output extension from the preset, and rejects ports
  below 1024.
* 12 tests.
* Recent media is filled from `LibraryStore::recent_entries`, so the panel and the library
  agree on what "recent" means.

### Conversion and streaming (`crates/app/src/main.rs`)

* A side `MediaPlayer` owns the transcode, with `vout=dummy`/`aout=dummy` so no second video
  window appears and the main player keeps playing.
* A 250 ms pump follows `time_ms / length_ms` for the progress bar and stops on
  `Ended`/`Error`, reporting the finished path or the failure.
* **Stop** cancels the job and keeps the partial file.
* The stream panel shows the viewer URL (`http://<this-pc>:port/path`) and keeps serving until
  stopped.

### Wiring

* `Ctrl+O` (and the rebindable Open file / Open folder / Open network actions) opens the
  dialog on the matching panel; `--open [0..7]` opens it directly for screenshots and testing.
* Opening a file shows the player window, updates the title, seek bar, queue and PiP label,
  and applies folder presets exactly like a library open.
* Opening a folder scans it, replaces the queue with its playable files and starts the first.
* 47 new EN/AR strings; `strings.slint` / `strings_ui.rs` regenerated with `apply_open`.

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

| Gate | Result |
| --- | --- |
| generator checks | current |
| `cargo fmt` | clean |
| `cargo clippy -D warnings` | clean |
| `cargo test` | **218 passed** at the end of the phase — 74 app, 133 core, 1 platform, 10 wrapper |
| `cargo build --release -p vfm-app` | finished |

## Screenshots

| File | Shows |
| --- | --- |
| `docs/compare/phase-9-open-file-dark-en.png` | Open media, File panel |
| `docs/compare/phase-9-open-network-dark-en.png` | Network panel with a typed address |
| `docs/compare/phase-9-open-convert-dark-en.png` | Convert panel: destination, preset chips, progress |
| `docs/compare/phase-9-open-recent-dark-ar.png` | Arabic RTL, Recent media list |

## Run commands

```powershell
$env:LIBVLC_PATH = "D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc"
.\target\release\VFM.exe --open          # File panel
.\target\release\VFM.exe --open 6        # Convert panel
```

## Known issues and deviations

1. Capture devices are typed, not enumerated: there is no DirectShow device list yet, so the
   panel defaults to the system device and accepts a name. Enumeration is the next step.
2. Conversion progress is time-based; libVLC exposes no transcode-percentage counter, so the
   bar is an estimate that can stall near the end on very fast presets.
3. Streaming uses the preset's real access output: HTTP presets serve over `access=http`; the
   RTSP preset emits `rtp{sdp=rtsp://…}` and reports an `rtsp://` viewer URL. The RTSP chain has
   not yet been exercised from a separate client/device in this build.
4. The dialog keeps its own accent/theme copy; changing the theme while it is open applies on
   the next open (the player and settings windows update immediately).
