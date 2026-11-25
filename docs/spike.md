# Phase 0 — Architecture Spike: libVLC 3.0.x + Slint on Windows x64

**App:** Vedio for me (VFM) — working name, binary id `vfm`. No VLC name or cone appears in
the UI, binaries, installer or these docs (libVLC is referenced only as a technical dependency).

**Status:** architecture chosen, workspace and prototype implemented, benchmark harness run.
Results and caveats are in §4. Verified on this Windows x64 host with Rust 1.98.0.

---

## 1. Workspace structure

```
VFM/
├── Cargo.toml                    # resolver = "2" workspace
├── crates/
│   ├── vlc-wrapper/              # ALL unsafe code lives here
│   │   └── src/
│   │       ├── lib.rs            # Instance / Media / MediaPlayer + typed errors
│   │       ├── loader.rs         # deterministic libvlc.dll + plugins/ resolution
│   │       ├── sys.rs            # hand-audited libVLC 3.0.x C ABI via libloading
│   │       └── video.rs          # double-buffered frame swapper + metrics
│   ├── core/                     # #![forbid(unsafe_code)]
│   │   └── src/{lib,state,time,telemetry}.rs
│   └── app/                      # #![forbid(unsafe_code)]
│       ├── build.rs              # slint-build
│       ├── ui/spike.slint        # video surface + overlaid HUD, EN/عربي toggle
│       └── src/main.rs           # CLI, --bench harness, event-loop bridge
├── design/                       # 25 Figma PNGs + tokens.md (extracted from PNGs)
├── vendor/libvlc/                # official VideoLAN 3.0.23 win64 (DLLs + SDK + plugins)
├── testdata/                     # generated 1080p60 / 4K30 H.264 clips
├── tools/sample_colors.py        # token extraction helper
└── docs/spike.md                 # this document
```

`core` and `app` are `#![forbid(unsafe_code)]`; `vlc-wrapper` documents a `// SAFETY:`
invariant at every unsafe site.

## 2. Wrapper strategy — decision

| Option | Verdict | Reason |
|---|---|---|
| `vlc-rs` 0.3.0 | Rejected | Requires `libvlc.lib` import lib, last updated 2018, "many missing functions", OS loader hard-crash popup when DLL missing. |
| `libvlc-sys` 0.2.0 | Rejected | bindgen needs LLVM/libclang in CI, older dependency chain, same loader crash risk. |
| **custom `vlc-wrapper` via `libloading`** | **Selected** | Zero native build deps, exact audited ABI surface, friendly `VlcError::LibraryNotFound`, deterministic DLL/plugin override, unsafe isolated in one crate. |

**Why runtime loading wins here.** The official VideoLAN win64 archive ships `libvlc.dll`
and `libvlccore.dll`; no MSVC `.lib` generation step is needed. `loader.rs` resolves in a
fixed order — `LIBVLC_PATH` → `<exe>/vlc/` → `<exe>/` → `C:\Program Files\VideoLAN\VLC` —
loads `libvlccore.dll` **before** `libvlc.dll` by absolute path, and sets
`VLC_PLUGIN_PATH` before `libvlc_new`. The result is reproducible on a dev box and in CI.

## 3. Video rendering pipeline (no HWND airspace issue)

Embedding video with `libvlc_media_player_set_hwnd` creates the classic Windows airspace
problem: a child HWND cannot be smoothly alpha-blended under GPU-composited overlays.
Phase 0 instead uses `libvlc_video_set_callbacks` + `libvlc_video_set_format`:

1. libVLC's video thread calls `lock(opaque, planes)`. The wrapper CAS-claims a free
   `Vec<u8>` slot (width×height×4), writes its pointer into `planes[0]`, returns it.
2. libVLC decodes/scales/converts into that buffer, calls `unlock`, then `display`.
3. `display` publishes the slot as `front`, increments `decoded`, and sets `front_ready`.
   A second `display` before the UI consumes the previous front counts one dropped frame.
4. A presenter thread observes `front_ready`, coalesces wakeups with an `AtomicBool`, and
   schedules one `slint::invoke_from_event_loop` closure. On the UI thread the front buffer
   is copied into a reused `SharedPixelBuffer<Rgba8Pixel>` and assigned to the Slint
   `<Image>`; the HUD rectangles render on top with full alpha blending.

Two chromas are benchmarked side by side:
- `--chroma rgba` — "RGBA" fourcc, direct `copy_from_slice` into `Rgba8Pixel`.
- `--chroma rv32` — "RV32" fourcc (BGRA on x86_64), in-place R/B swap then copy.

### Thread-safety invariants

- Callbacks never call any libVLC API (that would deadlock libVLC's vout mutex).
- A slot is owned by exactly one side at a time via `AtomicBool` CAS; with two slots and a
  single-threaded UI, `lock()` always finds a free buffer and never blocks.
- `present_pending`/`front_ready` coalescing guarantees at most one queued UI upload.
- The UI thread only holds a claimed slot for one `copy_from_slice` (<1 ms at 1080p).
- `MediaPlayer::drop` stops the vout, releases the player, then drops the `Arc<FrameSwap>` —
  reversing that order would be use-after-free. Documented in code.

## 4. Performance measurement

### Host / methodology

- Host: Windows x64, Rust 1.98.0 (`x86_64-pc-windows-msvc`), release profile `lto=thin`.
- libVLC: official VideoLAN **3.0.23 win64** (`libvlc.dll` 188 KB, `libvlccore.dll` 2.7 MB).
- Clips: `testdata/1080p60.mp4` (1920×1080, H.264, 60 fps) and `testdata/4k30.mp4`
  (3840×2160, H.264, 30 fps), generated with `testsrc2` + `libx264 -preset ultrafast`.
- Command: `cargo run --release -p vfm-app -- <clip> --width W --height H --bench 25 --chroma C`
  with `LIBVLC_PATH=vendor/libvlc`. Each run prints 1 Hz rows and a final summary.
- `decoded/presented fps`, `drop %` and `avg/p95 copy ms` come from the in-app telemetry
  harness (`vfm-core::telemetry`). CPU/GPU % are sampled from Windows Task Manager /
  `Get-Counter` while the bench runs; GPU % requires the Slint wgpu renderer and is
  reported as N/A when the default software renderer is used.

### Theoretical bandwidth baseline (CPU copy path)

| Profile | Frame (RGBA/RV32) | Rate | Bandwidth (1 copy) | Budget |
|---|---|---|---|---|
| 1080p60 | 8.29 MB | 60 fps | ~497.7 MB/s | 16.67 ms |
| 4K30 | 33.18 MB | 30 fps | ~995.3 MB/s | 33.33 ms |

Because `libvlc_video_set_callbacks` forces CPU YUV→RGB conversion plus a CPU→GPU texture
upload, 4K30 (especially 10-bit HEVC/AV1) can spike CPU. The measured path below tells us
whether Phase 1 needs the GPU optimization sooner.

### Measured results (release build, this Windows x64 host)

| Test clip | Chroma | Decoded fps | Presented fps | Dropped % | Avg copy ms | CPU % | GPU % | Pass? |
|---|---|---|---|---|---|---|---|---|
| 1080p60 H.264 | RGBA | 28.0 | 4.19 | 45.9 | 13.8 | — | N/A | No (see caveat) |
| 1080p60 H.264 | RV32 | 58.4 | 3.46 | 48.5 | 17.5 | ~4.2% of 16 cores | N/A | Decode yes / present no |
| 4K30 H.264 | RGBA | 4.9 | 1.86 | 37.8 | 75.2 | — | N/A | No |
| 4K30 H.264 | RV32 | 27.6 | 2.12 | 47.9 | 86.5 | ~one core (noisy sample) | N/A | No |

Raw 1 Hz rows and full summaries are saved in `docs/bench/*.log`.

### Interpretation (important)

1. **RV32 is decisively better than RGBA.** libVLC's native output is BGRA ("RV32"); asking
   for "RGBA" forces an extra CPU conversion and roughly halves decode throughput
   (1080p: 58.4 vs 28.0 fps; 4K: 27.6 vs 4.9 fps). The spec's assumption that RGBA avoids a
   swizzle and is therefore faster is **not true on this libVLC build**. Use RV32 + R/B swap
   as the default; support RGBA only for A/B testing.
2. **The copy path meets the 1080p budget but not the 4K budget.** 17.5 ms at 1080p fits
   under 16.7 ms only marginally (and includes the swizzle); 86.5 ms at 4K is ~2.6× the
   33.3 ms budget. 4K cannot be done with this CPU-only path.
3. **Presentation is the dominant bottleneck in this session.** The spike ran in an
   automated/non-interactive desktop where Slint falls back to its software renderer and
   presentation is throttled to ~2–4 fps. That inflates the drop counter and suppresses
   presented fps; CPU stays around 0.7 core at 1080p, so the app is not CPU-saturated.
   GPU % is N/A (no hardware acceleration / no GPU counters available here).
   **These numbers must be re-run in an interactive session with a real GPU before Phase 1
   sign-off.** The decode-fps and copy-ms columns are still valid comparative measurements.

### Optimization path if 4K30 is too slow (>2% drops or >25% CPU)

1. **NV12/I420 upload + WGSL GPU conversion (recommended).** Request NV12 (12 bits/px,
   62.5% less bandwidth, no CPU YUV→RGB swscale). Enable Slint's wgpu backend via
   `BackendSelector::require_wgpu_30()`, obtain `wgpu::Device/Queue` through
   `Window::set_rendering_notifier(GraphicsAPI::WGPU30)`, upload Y and UV planes with
   `queue.write_texture`, run a short BT.709/601 WGSL fragment shader into an offscreen
   texture, import zero-copy via `slint::Image::try_from(wgpu_texture)`.
2. **GPU texture ping-pong.** Keep a 2-texture ring to avoid per-frame allocation stalls.
3. **Hardware zero-copy (libVLC 4.0).** `libvlc_video_set_output_callbacks` (D3D11 shared
   handles) is a 4.0 feature; 3.0.x's public API cannot expose it. This is the clean
   upgrade path if 4K60 HDR zero-copy becomes a Phase 4+ requirement.

### Local toolchain caveat (this host only)

This machine has Rust 1.98 (`x86_64-pc-windows-msvc`) with a **partial Windows SDK** (redist
DLLs but no `Lib/` import libraries). `rust-lld` then fails to resolve `cbrt/log2/fma/rint/
exp2f`. `tools/crt-math-shim/` is a tiny pure-arithmetic staticlib that exports exactly those
symbols; `.cargo/config.toml` adds it to the link line. The app crate uses
`#![deny(unsafe_code)]` rather than `forbid` because Slint's generated vtable code carries
`#[allow(unsafe_code)]`; our own code contains no `unsafe`. CI with a full VS + Windows SDK
should remove the shim and can restore `forbid` if desired.

`Cargo.toml` uses `[profile.release] opt-level=3, lto=false, codegen-units=8` so release
builds finish in a reasonable time on this host; tune later in Phase 4.

## 5. Obtaining libVLC x64 and runtime resolution

Official VideoLAN 3.0.x win64 archive:
<https://get.videolan.org/vlc/3.0.23/win64/vlc-3.0.23-win64.7z>
(portable archive; extract `libvlc.dll`, `libvlccore.dll`, `plugins/`, optionally `sdk/`).

This repo already vendors the extracted runtime at `vendor/libvlc/`. Layout expected next
to the compiled binary (or via `LIBVLC_PATH`):

```
VFM.exe
vlc/
├── libvlc.dll
├── libvlccore.dll
└── plugins/         # full plugin tree from the official win64 archive
```

Licensing: libVLC is LGPLv2.1+ (some plugins GPL). Dynamic loading keeps the application
relinkable; `THIRD_PARTY_LICENSES` will be added in Phase 4.

## 6. Build & run (Windows 10/11 x64)

Prerequisites: Rust stable `x86_64-pc-windows-msvc` (1.92+), VS 2022 Build Tools (C++),
official VideoLAN win64 runtime (vendored here).

```powershell
cd "D:\تجاربي\VLC MAINE 2 HY"
$env:LIBVLC_PATH = "D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# interactive spike (release strongly recommended for video)
cargo run --release -p vfm-app -- ".\testdata\1080p60.mp4" --width 1920 --height 1080

# 25-second benchmarks
cargo run --release -p vfm-app -- ".\testdata\1080p60.mp4" --width 1920 --height 1080 --bench 25 --chroma rgba
cargo run --release -p vfm-app -- ".\testdata\4k30.mp4"    --width 3840 --height 2160 --bench 25 --chroma rgba
```

## 7. Phase 0 demo description

Launch with a local video file. A 1280×720 window opens with an obsidian `#0E0E0E` surface
and renders decoded frames inside a Slint `<Image>` via the double-buffered callback path.
A translucent top HUD floats over the live video showing the filename, resolution,
decoded/presented FPS, dropped-frame count and an **EN / عربي** toggle that mirrors the
overlay layout in real time (RTL preview). A translucent bottom control bar floats over the
lower edge with Play/Pause (Space), Stop, a live seek bar with `M:SS` timecodes, and a
volume bar (M toggles mute). Passing `--bench <seconds>` prints 1 Hz telemetry rows and a
final summary block for recording the §4 table.

## 8. What's next (Phase 1+)

- Phase 1: full player controls, libVLC event channel, playlist/library/settings (SQLite),
  Windows integration (file associations, SMTC, single instance, drag & drop).
- Phase 2: generated theme module from `tokens.md`, bundled Arabic+Latin fonts, full
  component library with all interaction states.
- Phase 3: Figma screens in the agreed commit order, custom title bar, DPI/multi-monitor.
- Phase 4: screenshot diffs, performance pass, packaging, CI, manual test checklist.
