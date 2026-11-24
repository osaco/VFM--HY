# Design Tokens — Vedio for me (VFM)

> Extracted from the Figma PNG exports in `design/` by sampling dominant and probe colors
> (see `tools/sample_colors.py`). The editable Figma archive is preserved as `design/source.fig`
> with an extracted `design/source-thumbnail.png`; its canvas metadata reports a neutral
> `#292929` canvas background, which is a tooling canvas color, not an app surface token.
> This is a Phase 0 draft for review; Phase 2 will turn it into a generated Rust theme module
> with zero hard-coded styling outside it.

## Color

| Token | Hex | Source / usage |
|---|---|---|
| `bg.base` | `#0E0E0E` | Dominant app background (Player-1, Settings) |
| `bg.surface` | `#181818` | Panels, sidebar rows, context menu surface |
| `bg.elevated` | `#1B1B1B` | Cards, raised surfaces |
| `bg.control` | `#090909` | Player control bar strip |
| `bg.video` | `#2D2B29` | Neutral video-stage backdrop |
| `bg.overlay` | `#000000CC` | 80% translucent overlay behind HUD |
| `text.primary` | `#FFFFFF` | Headings, active labels |
| `text.body` | `#E0E0E0` | Body text on dark surfaces |
| `text.secondary` | `#8F8F8F` | Secondary labels, inactive icons |
| `text.muted` | `#CCCCCC` | De-emphasized metadata |
| `accent.default` | `#D37918` | Primary amber accent (play, seek, focus) |
| `accent.hover` | `#E08A2A` | Accent hover |
| `accent.pressed` | `#B86310` | Accent pressed |
| `accent.soft` | `#371F06` | Accent tint / selected row wash |
| `text.on-accent` | `#0B0B0B` | Text over accent buttons |
| `danger` | `#AB261D` | Error, destructive actions |
| `border.subtle` | `#2B2927` | Dividers on video-stage surfaces |
| `border.default` | `#404040` | Input borders |
| `volume.fill` | `#7BA213` | Volume cone fill (sampled from Player.png) |
| `focus.ring` | `#D37918` | 2px keyboard focus ring |

### Light theme palette

Derived for Phase 2 so the gallery can be reviewed in both themes; the dark palette above
remains the sampled Figma source. The accent is shared and changeable at runtime.

| Token | Hex | Usage |
|---|---|---|
| `bg.base` | `#F7F7F7` | Light app background |
| `bg.surface` | `#FFFFFF` | Panels, cards, menus |
| `bg.elevated` | `#FFFFFF` | Raised surfaces |
| `bg.control` | `#EFEFEF` | Control bar strip |
| `bg.video` | `#D9D7D4` | Neutral video-stage backdrop |
| `bg.overlay` | `#FFFFFFD9` | Light translucent overlay behind HUD |
| `text.primary` | `#111111` | Headings, active labels |
| `text.body` | `#2B2B2B` | Body text |
| `text.secondary` | `#666666` | Secondary labels |
| `text.muted` | `#777777` | Metadata |
| `text.on-accent` | `#0B0B0B` | Text over accent buttons |
| `volume.fill` | `#5E7D0E` | Volume cone fill, darkened for light surfaces |
| `danger` | `#C0392B` | Error, destructive actions |
| `border.subtle` | `#E0E0E0` | Dividers |
| `border.default` | `#BDBDBD` | Input borders |

## Typography

- **Latin**: Inter variable (`assets/fonts/Inter-Variable.ttf`), covering 400/500/600 weights.
- **Arabic**: Noto Sans Arabic variable (`assets/fonts/NotoSansArabic-Variable.ttf`), covering 400/500/600 weights; RTL shaping via Slint text layout.
- Scale: `11 / 12 / 13 / 14 / 16 / 20 / 24 / 32` px, line-height `1.4`.
- Weights: `400` body, `500` labels, `600` headings.

## Spacing

Base grid 4px: `4, 8, 12, 16, 24, 32, 48`. Control bar inset `16px`; HUD padding `12px 16px`.

## Radii

`4` small (chips), `8` controls/cards, `12` panels, `16` dialogs, `9999` pills.

### Derived component metrics

These are generated into `theme.slint` so screens never hard-code control geometry. They are
derived from the 4px spacing grid above and the Figma control-bar proportions.

| Token | Value | Usage |
|---|---|---|
| `control.height.sm` | `28px` | Compact chips, menu rows |
| `control.height.md` | `36px` | Buttons, inputs, tabs |
| `control.height.lg` | `44px` | Primary actions, dialog buttons |
| `icon.size.sm` | `16px` | Inline icons |
| `icon.size.md` | `20px` | Control-bar icons |
| `icon.size.lg` | `28px` | Menu and hero icons |
| `border.width` | `1px` | Inputs, cards, dividers |
| `focus.width` | `2px` | Keyboard focus ring |
| `track.height` | `4px` | Volume / numeric sliders |
| `seek.height` | `28px` | Thick seek bar track (measured in Player.png) |
| `seek.fill-height` | `24px` | Played portion inside the seek track |
| `seek.thumb-width` | `8px` | Seek playhead thumb |
| `player.topbar-height` | `40px` | Custom title bar |
| `player.bottombar-height` | `54px` | Player control strip |
| `player.icon-button` | `42px` | Control-strip icon hit target |
| `player.play-width` | `54px` | Enlarged play button |
| `player.play-height` | `28px` | Enlarged play button |
| `volume.cone-width` | `90px` | Tapered volume slider |
| `volume.cone-height` | `16px` | Tapered volume slider |
| `menu.width` | `220px` | Context menus, dropdowns |
| `menu.row-height` | `40px` | Context/dropdown rows |
| `dialog.width` | `420px` | Compact dialog width |
| `tooltip.width` | `140px` | Seek tooltip / help bubble |
| `tab.min-width` | `96px` | Tab strip cells |
| `scrollbar.width` | `8px` | Vertical scrollbar track |
| `thumbnail.width` | `132px` | Card / grid thumbnails |
| `thumbnail.height` | `74px` | Card / grid thumbnails |

## Elevation

- `e1`: `0 1px 2px rgba(0,0,0,.4)` — rows
- `e2`: `0 4px 12px rgba(0,0,0,.5)` — popovers/menus
- `e3`: `0 12px 32px rgba(0,0,0,.6)` — dialogs

## Motion

`fast 120ms`, `base 200ms`, `slow 320ms`, easing `cubic-bezier(.2,.8,.2,1)`.
Control-bar auto-hide fade: 200ms. HUD slide: translateY 8px + fade, 200ms.

## Design intent from the redesign concept brief (provided 2026-11-19)

The referenced redesign argues for a thin, quiet control bar; a deliberately **non-centered**
play button with previous/next grouped around it; a **thicker** seek bar at the bottom with
hover thumbnail preview; subtitles/audio/video track controls clustered on the **right**;
fullscreen and picture-in-picture on the right/top; top and bottom chrome that appears on
edge-hover and disappears on mouse-out; one consolidated menu plus an essential right-click
menu with a "hide UI" option; a **queue** distinct from saved **playlists**; a library that is
reachable without leaving the video (compact sidebar + drag into queue); four right-side
sidebar tabs (subtitles, audio, video, playback) with a visual subtitle-timeline; per-show
presets; flat information hierarchy in settings with human-readable labels; and drag-to-
customize layouts with saved profiles.

Mapping to this project:
- **Phase 0 (spike):** overlay HUD, seek bar, volume, EN/عربي RTL toggle. Hover auto-hide,
  seek thumbnails and the four-tab sidebar are deferred to Phase 3.
- **Phase 1:** queue vs. playlist split, drag-to-reorder, library browsing, settings/state
  persistence, external subtitle discovery.
- **Phase 2/3:** control-bar layout above, right-side track menus, auto-hide chrome,
  right-click menu, subtitle timeline, settings information hierarchy, custom title bar.
- **Phase 4:** drag-to-customize layouts and saved profiles (stretch goal).

### Branding conflict (resolved by existing rule)

The concept brief explicitly proposes keeping the traffic-cone mark and the VLC name/orange.
That conflicts with the hard project rule: **zero VLC name or cone in UI, binaries, or
installer**. The existing rule takes precedence. We keep the *UX patterns* and the amber
accent (`#D37918`, sampled from the Figma play button), but use an original neutral mark
(or none) and the app name **Vedio for me (VFM)**. If you want to reverse that decision,
say so explicitly before Phase 2.

## Notes

- The Figma exports are 4K/8K; sample relative ratios, not absolute px.
- Accent `#D37918` was sampled from the play button; `#AB261D` appears in media thumbnails only.
- No VLC branding or cone is used anywhere — tokens are brand-neutral.
