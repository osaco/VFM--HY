# Phase 11 summary — Hardening and release (VFM)

Status: **release candidate**, with the caveat below. Crash reporting, packaging, CI and the
documentation set are in place. This phase is honest about what is *not* finished: the
remaining gaps are listed at the bottom rather than hidden.

## Verified baseline (2026-11-25)

The snapshot supplied for this review did not build. Before re-running any gate:

* `crates/app/src/presets_ui.rs` omitted the new `audio_device_index` field from
  `default_state()`, so `SidebarState` had no complete initializer.
* `crates/app/src/main.rs` moved an `Rc<RefCell<Vec<String>>>` into
  `slint::invoke_from_event_loop`, which requires `Send`; it is now an `Arc<Mutex<Vec<String>>>`.
* `crates/core/src/hotkeys.rs` asserted 35 actions while `Action::all()` had grown to 40.

After those three fixes the workspace builds and the full suite passes (see the fresh log in
`docs/baseline-2026-11-25.md`). The earlier “236 passed” figure described a different source
revision and must not be read as a result for this tree. The binary was also renamed from
`VFM.exe` to `VFM.exe` so the shipped artifact matches the product name.

## What was done

### Crash reporting and diagnostics (`crates/app/src/crash.rs`, new)

* A panic hook records the thread, location, payload and a forced backtrace to
  `<data-dir>/crash.log`, and still runs the previous hook so the console output is unchanged.
* The hook is installed before anything else in `main`, and the log directory follows
  `--data-dir` (or `%LOCALAPPDATA%\VFM`).
* `Settings::diagnostics` (and `VFM_DIAGNOSTICS=1`) turns on the startup diagnostics line; the
  crash log itself is always written, because a crash the user cannot report is worse than a
  file in their data directory.

### Packaging (`tools/package.ps1`, `packaging/vfm.iss`, new)

* `package.ps1` builds the release binary and lays out a portable bundle:
  `VFM.exe`, the libVLC DLLs, the whole `plugins/` tree, the documentation and the
  licence file, then zips it as `dist/VFM-<version>-portable.zip`.
* The runtime is copied as **loose files**, never statically linked or bundled into one
  executable — that is what keeps the LGPL obligation satisfiable (see
  `THIRD_PARTY_LICENSES.md`).
* `packaging/vfm.iss` is a complete Inno Setup 6 script: per-user install, Start-menu and
  optional desktop shortcut, optional file associations (`--associate` / `--unassociate`),
  English and Arabic installer languages, uninstaller entry.
* `package.ps1` compiles the installer only when ISCC is present, so the portable zip always
  works.

### CI (`.github/workflows/build.yml`, new)

* Windows runner: fetch a libVLC 3.x runtime, run the three generator checks, `fmt`,
  `clippy -D warnings`, the full test suite, the release build, then the packaging script and
  upload the zip as an artifact.
* The generated-source check runs before the build so a stale `strings.slint` fails the PR
  rather than surfacing as a compile error later.

### Documentation

* `README.md` — what the project is, the crate map, requirements, build/run, the CLI flags,
  the quality gates, and where the phase docs live.
* `THIRD_PARTY_LICENSES.md` — runtime dependencies with licences, fonts, and the LGPL
  obligations for libVLC in plain terms.
* `docs/phase-1.md` … `docs/phase-10.md` — one document per phase with its gates, screenshots
  and known issues.

### Accessibility and robustness (already in place, now checked)

* Every dialog closes on `Escape`; `VfmButton` and `Switch` are focusable and activate on
  `Space`/`Enter`; focus rings come from `Theme.focus-ring` in both themes.
* Slint's accesskit backend exposes roles and names, so screen readers see buttons, switches
  and text fields rather than a bitmap.
* A missing or broken libVLC produces a dialog-free, readable error from `Instance::new`
  instead of a panic; corrupt settings are clamped on load (`Settings::clamp`); a corrupt
  layout or profile JSON is rejected and the default is used.

## Quality gates

| Gate | Result |
| --- | --- |
| `generate_strings.py --check` | current |
| `generate_theme.py --check` | current |
| `generate_icons.py --check` | current |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | clean |
| `cargo test --release --workspace` | **247 passed** — 84 app, 149 core, 4 platform, 10 wrapper |
| `cargo build --release -p vfm-app` | finished |
| `tools/package.ps1 -SkipBuild` | portable zip written |

## Run commands

```powershell
# Full release bundle (portable zip, plus the installer when Inno Setup is installed)
powershell -ExecutionPolicy Bypass -File tools/package.ps1

# Just the binary
cargo build --release -p vfm-app
```

## What is still open

These are real gaps, not polish items. They are the honest remainder of the phase plan:

1. **DPI comparison.** Screenshots were taken at 100 % DPI only. The design tokens are
   DPI-independent, but 125/150/200 % has not been eyeballed screen by screen.
2. **Control-bar ordering.** The four shipped tail buttons now follow the saved order, but the
   other palette controls (repeat, shuffle, speed, chapters, video, snapshot, pip, fullscreen)
   are still preview-only.
3. **High contrast.** The theme has light and dark only; Windows high-contrast mode is not
   honoured.
4. **4K GPU path.** Playback is CPU-decoded through libVLC's default output; a 4K GPU path was
   never measured on this host.
5. **Capture devices.** Typed, not enumerated (Phase 9 note).
6. **Rotation/flip and headless seek thumbnails.** The subtitle outline width is now wired to
   the renderer, but rotation/flip are still stored without transforming the frame, and the
   seek-thumbnail grabber is still unwired.
7. **Branding.** Replaced the placeholder cone with an original VFM monogram
   (`assets/branding/vfm-256.png`, `vfm-512.png`, `vfm.ico`). The EXE still needs the icon
   embedded at link time.
8. **Installer signing.** The Inno Setup output is unsigned; a release build should be signed
   before distribution.
