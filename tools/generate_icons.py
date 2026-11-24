#!/usr/bin/env python3
"""Generate the two VFM icon sets as consistent 24x24 SVG assets.

Sets:
  assets/icons/control  monochrome control icons (single neutral stroke/fill)
  assets/icons/menu     two-tone menu icons (neutral base + amber accent)

No third-party marks or branded shapes are used.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONTROL_DIR = ROOT / "assets" / "icons" / "control"
MENU_DIR = ROOT / "assets" / "icons" / "menu"

GRAY = "#8F8F8F"
ACCENT = "#D37918"
INK = "#0B0B0B"

# (kind, path data, tone) where tone 0 = neutral and tone 1 = accent.
SHAPES: dict[str, list[tuple[str, str, int]]] = {
    "play": [("fill", "M8 5 L19 12 L8 19 Z", 1)],
    "pause": [("fill", "M7 5 H10 V19 H7 Z", 1), ("fill", "M14 5 H17 V19 H14 Z", 1)],
    "stop": [("fill", "M7 7 H17 V17 H7 Z", 1)],
    "previous": [("fill", "M6 5 H8 V19 H6 Z", 1), ("fill", "M18 5 L18 19 L10 12 Z", 1)],
    "next": [("fill", "M16 5 H18 V19 H16 Z", 1), ("fill", "M6 5 L14 12 L6 19 Z", 1)],
    "volume": [
        ("stroke", "M4 9 H8 L13 5 V19 L8 15 H4 Z", 0),
        ("stroke", "M16 9 C17.3 10.2 17.3 13.8 16 15", 1),
        ("stroke", "M18.5 6.5 C20.6 8.8 20.6 15.2 18.5 17.5", 1),
    ],
    "volume-mute": [
        ("stroke", "M4 9 H8 L13 5 V19 L8 15 H4 Z", 0),
        ("stroke", "M16 9 L21 15", 1),
        ("stroke", "M21 9 L16 15", 1),
    ],
    "settings": [
        ("stroke", "M12 8.5 A3.5 3.5 0 1 0 12 15.5 A3.5 3.5 0 1 0 12 8.5", 0),
        ("stroke", "M12 2.5 V5 M12 19 V21.5 M2.5 12 H5 M19 12 H21.5 M5.3 5.3 L7 7 M17 17 L18.7 18.7 M18.7 5.3 L17 7 M7 17 L5.3 18.7", 1),
    ],
    "queue": [
        ("stroke", "M4 6 H14 M4 11 H14 M4 16 H10", 0),
        ("fill", "M15 14 L21 18 L15 22 Z", 1),
    ],
    "subtitles": [
        ("stroke", "M3 5 H21 V19 H3 Z", 0),
        ("fill", "M6 11 H13 V13 H6 Z M14 11 H18 V13 H14 Z M6 15 H10 V17 H6 Z", 1),
    ],
    "audio": [
        ("stroke", "M3 7 H21 V17 H3 Z", 0),
        ("stroke", "M6 12 V10 M9.5 14 V8 M13 12 V10 M16.5 15 V7 M20 12 V10", 1),
    ],
    "video": [
        ("stroke", "M3 6 H21 V18 H3 Z", 0),
        ("fill", "M10 9 L16 12 L10 15 Z", 1),
    ],
    "fullscreen": [
        ("stroke", "M4 9 V4 H9 M15 4 H20 V9 M20 15 V20 H15 M9 20 H4 V15", 1),
    ],
    "more": [
        ("fill", "M5 11 A1.5 1.5 0 1 0 5 14 A1.5 1.5 0 1 0 5 11", 1),
        ("fill", "M12 11 A1.5 1.5 0 1 0 12 14 A1.5 1.5 0 1 0 12 11", 1),
        ("fill", "M19 11 A1.5 1.5 0 1 0 19 14 A1.5 1.5 0 1 0 19 11", 1),
    ],
    "search": [
        ("stroke", "M10.5 4 A6.5 6.5 0 1 0 10.5 17 A6.5 6.5 0 1 0 10.5 4", 0),
        ("stroke", "M15.5 15.5 L20.5 20.5", 1),
    ],
    "close": [("stroke", "M5 5 L19 19 M19 5 L5 19", 1)],
    "minimize": [("stroke", "M5 12 H19", 0)],
    "maximize": [("stroke", "M5 5 H19 V19 H5 Z", 0)],
    "restore": [
        ("stroke", "M3 4 V10 H9", 0),
        ("stroke", "M4.6 15 A9 9 0 1 0 7 5.5 L3 10", 0),
    ],
    "pip": [
        ("stroke", "M3 5 H21 V19 H3 Z", 0),
        ("stroke", "M12 11 H19 V17 H12 Z", 1),
    ],
    "browse": [
        ("stroke", "M3 7 H9 L11 9 H21 V19 H3 Z", 0),
        ("stroke", "M8 13 H16", 1),
    ],
    "add": [("stroke", "M12 5 V19 M5 12 H19", 1)],
    "record": [
        ("stroke", "M12 3 A9 9 0 1 0 12 21 A9 9 0 1 0 12 3", 0),
        ("fill", "M9 9 H15 V15 H9 Z", 1),
    ],
    "hide-ui": [
        ("stroke", "M3 5 H21 V19 H3 Z", 0),
        ("stroke", "M5 19 L19 5", 1),
    ],
    "info": [
        ("stroke", "M12 3 A9 9 0 1 0 12 21 A9 9 0 1 0 12 3", 0),
        ("stroke", "M12 11 V16", 1),
        ("fill", "M11 7 H13 V9 H11 Z", 1),
    ],
    "check": [("stroke", "M5 12 L10 17 L19 7", 1)],
    "chevron-left": [("stroke", "M15 5 L8 12 L15 19", 0)],
    "chevron-right": [("stroke", "M9 5 L16 12 L9 19", 0)],
    "home": [
        ("stroke", "M4 11 L12 4 L20 11 V20 H4 Z", 0),
        ("stroke", "M10 20 V14 H14 V20", 1),
    ],
    "videos": [
        ("stroke", "M4 5 H20 V19 H4 Z", 0),
        ("fill", "M10 9 L16 12 L10 15 Z", 1),
    ],
    "music": [
        ("stroke", "M9 18 V6 L19 4 V16", 0),
        ("fill", "M6.5 18 A2.5 2.5 0 1 0 6.5 15 A2.5 2.5 0 1 0 6.5 18", 1),
        ("fill", "M16.5 16 A2.5 2.5 0 1 0 16.5 13 A2.5 2.5 0 1 0 16.5 16", 1),
    ],
    "network": [
        ("stroke", "M12 4 A8 8 0 1 0 12 20 A8 8 0 1 0 12 4", 0),
        ("stroke", "M4.5 12 H19.5 M12 4 C15 7 15 17 12 20 M12 4 C9 7 9 17 12 20", 1),
    ],
    "playlists": [
        ("stroke", "M5 6 H19 M5 11 H19 M5 16 H13", 0),
        ("fill", "M15 14 L20 18 L15 22 Z", 1),
    ],
    "playback": [
        ("stroke", "M12 4 A8 8 0 1 0 12 20 A8 8 0 1 0 12 4", 0),
        ("fill", "M10 9 L16 12 L10 15 Z", 1),
    ],
    "recent": [
        ("stroke", "M12 4 A8 8 0 1 0 12 20 A8 8 0 1 0 12 4", 0),
        ("stroke", "M12 8 V12 L15 14", 1),
    ],
    "sort": [
        ("stroke", "M4 7 H20 M6 12 H16 M8 17 H12", 0),
        ("fill", "M18 14 L22 18 L18 22 Z", 1),
    ],
    "grid": [
        ("stroke", "M4 4 H10 V10 H4 Z M14 4 H20 V10 H14 Z M4 14 H10 V20 H4 Z M14 14 H20 V20 H14 Z", 0),
    ],
    "list": [("stroke", "M4 6 H20 M4 12 H20 M4 18 H20", 0)],
    "equalizer": [
        ("stroke", "M4 9 V15 M8 6 V18 M12 10 V14 M16 4 V20 M20 8 V16", 1),
    ],
}


def svg(paths: list[tuple[str, str, int]], two_tone: bool, base: str | None = None) -> str:
    out = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none">'
    ]
    for kind, data, tone in paths:
        color = base or (ACCENT if two_tone and tone == 1 else GRAY)
        if kind == "fill":
            out.append(f'  <path d="{data}" fill="{color}"/>')
        else:
            out.append(
                f'  <path d="{data}" stroke="{color}" stroke-width="1.6" '
                'stroke-linecap="round" stroke-linejoin="round"/>'
            )
    out.append("</svg>")
    return "\n".join(out) + "\n"


# Glyphs that sit on top of a filled accent surface need ink, not the neutral gray.
ON_ACCENT = ["play", "pause"]


def main() -> None:
    check = "--check" in sys.argv
    CONTROL_DIR.mkdir(parents=True, exist_ok=True)
    MENU_DIR.mkdir(parents=True, exist_ok=True)
    stale: list[str] = []

    def emit(path, text: str) -> None:
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                stale.append(path.name)
            return
        path.write_text(text, encoding="utf-8")

    for name, paths in SHAPES.items():
        emit(CONTROL_DIR / f"{name}.svg", svg(paths, two_tone=False))
        emit(MENU_DIR / f"{name}.svg", svg(paths, two_tone=True))
    for name in ON_ACCENT:
        emit(CONTROL_DIR / f"{name}-on-accent.svg", svg(SHAPES[name], two_tone=False, base=INK))

    if check:
        if stale:
            raise SystemExit(f"stale icons: {', '.join(sorted(stale))}")
        print("icon sets are current")
        return
    print(
        f"wrote {len(SHAPES)} control icons, {len(SHAPES)} menu icons, "
        f"{len(ON_ACCENT)} on-accent glyphs"
    )


if __name__ == "__main__":
    main()
