# Phase 10 summary — Layout customization and profiles (VFM)

Status: **complete for review**. The control bar is described by a data model, a customize
screen edits that model with a live preview, and named profiles save and restore layouts as
JSON.

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

* The tail cluster (`audio`, `subtitles`, `settings`, `queue`) now has `bar-*` flags and the
  slot chain gives a hidden button's width back, so the remaining buttons stay on the design's
  42 px pitch instead of leaving a hole.
* The layout is persisted in `Settings::layout` and applied at startup and after every edit.
* The right-click menu gained **Customize controls**; `--customize` opens the window directly.

## Quality gates

| Gate | Result |
| --- | --- |
| generator checks | current |
| `cargo fmt` | clean |
| `cargo clippy -D warnings` | clean |
| `cargo test` | **236 passed** — 80 app, 144 core, 2 platform, 10 wrapper |
| `cargo build --release -p vfm-app` | finished |

## Screenshots

| File | Shows |
| --- | --- |
| `docs/compare/phase-10-customize-dark-en.png` | Palette, zone preview, placed list with reorder/remove, profile column |
| `docs/compare/phase-10-customize-dark-ar.png` | Same window in Arabic RTL |

## Run commands

```powershell
.\target\release\vfm-spike.exe --customize
```

## Known issues and deviations

1. **Ordering is stored and previewed but not yet rendered by the player.** The shipped strip
   positions its controls with fixed Figma-measured slots; rebuilding it from the model is the
   remaining step. What the player applies today is *which* tail buttons exist (audio,
   subtitles, settings, queue), which is the part that changes the strip's geometry safely.
2. Drag and drop is click-to-add + arrow reordering rather than free dragging with snap guides;
   the model supports either, the interaction is the simpler one.
3. Profiles store a settings snapshot field but the UI does not edit it yet — profiles are
   layout-only in this build.
