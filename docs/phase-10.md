# Phase 10 summary — Layout customization and profiles (VFM)

Status: **partly complete**. The control bar is described by a data model, a customize screen
edits that model with a live preview, and named profiles save and restore layouts as JSON.
The player now renders the four tail buttons in the saved order (not just their visibility),
but the other palette controls are still preview-only.

## What was done

### Layout model (`crates/core/src/layout.rs`, new)

* `ControlId` — 20 placeable controls with stable kebab-case ids, each with an i18n label key
  `layout.<id>`.
* `Layout { leading, centre, trailing }` with `add`, `remove` (returns the zone it came from),
  `move_up`/`move_down`, `contains`, `all`.
* `sanitise()` drops duplicates and unknown ids and always restores the play button, so a
  hand-edited or imported layout can never produce an unusable bar.
* Serde round-trip (`to_json` / `from_json`) — the stored format is the same JSON used for
  export.
* `Profile { name, layout, settings }` adds a name and an optional settings snapshot.
* 11 tests.

### Customize window (`crates/app/ui/customize.slint`, `crates/app/src/layout_ui.rs`, new)

* Three columns: the palette of every control (placed ones dimmed), the bar preview plus the
  ordered list of placed controls, and the profile list.
* The preview draws the three zones of the real strip, so the summary line under it
  (`Play / pause · Stop`) is what the player will show.
* Every edit re-pushes the window and the player immediately — there is no Apply step.
* Profiles: save (auto-named), load, delete, export to `<data-dir>/layouts/layout-<name>.json`,
  import from any JSON file. Import validates through `Profile::from_json`, so a corrupt file
  is rejected with a status message instead of breaking the bar.
* 5 tests over the controller (add/remove, unknown ids, profile save/apply/delete, empty name,
  bad import).

### Player integration (`crates/app/ui/player.slint`, `crates/app/src/main.rs`)

* The tail cluster (`audio`, `subtitles`, `settings`, `queue`) is rendered from the saved
  trailing order: each control receives a slot measured from the right edge
  (`Layout::trailing_slots`, tested in `core`), so moving a button in the customizer moves the
  real button. Removing one closes the gap instead of leaving a hole.
* The default layout now lists exactly the four controls the shipped bar draws, so a fresh
  install shows the queue and settings buttons instead of hiding them.
* The layout is persisted in `Settings::layout` and applied at startup and after every edit.
* The right-click menu gained **Customize controls**; `--customize` opens the window directly.

## Quality gates

| Gate | Result |
| --- | --- |
| generator checks | current |
| `cargo fmt` | clean |
| `cargo clippy -D warnings` | clean |
| `cargo test` | **260 passed** — 86 app, 160 core, 4 platform, 10 wrapper |
| `cargo build --release -p vfm-app` | finished |

## Screenshots

| File | Shows |
| --- | --- |
| `docs/compare/phase-10-customize-dark-en.png` | Palette, zone preview, placed list with reorder/remove, profile column |
| `docs/compare/phase-10-customize-dark-ar.png` | Same window in Arabic RTL |

## Run commands

```powershell
.\target\release\VFM.exe --customize
```

## Known issues and deviations

1. **Only the four shipped tail controls are rendered.** Their order now comes from the saved
   layout, but the other palette entries (repeat, shuffle, speed, chapters, video, snapshot,
   pip, fullscreen) still appear only in the preview. A full data-driven strip is the remaining
   step.
2. Drag and drop is click-to-add + arrow reordering rather than free dragging with snap guides;
   the model supports either, the interaction is the simpler one.
3. Profiles store a settings snapshot field but the UI does not edit it yet — profiles are
   layout-only in this build.
