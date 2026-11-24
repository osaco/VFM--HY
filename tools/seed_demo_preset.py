"""Seed a demo preset for the Phase 6 screenshot / manual verification run.

Creates a folder-scoped preset so the player auto-applies it to every episode of
the demo show, exactly like a preset saved from the sidebar.
"""

from __future__ import annotations

import sqlite3
import sys
from pathlib import Path

PAYLOAD = """
[subtitle]
enabled = true
delay_ms = 450
size_scale = 1.25
precise_size = false
precise_size_px = 28.0
position = 0.1
outline = 1.0
default_language = "Auto"

[audio]
delay_ms = 0
default_language = "Auto"
equalizer = "Flat"
normalize_volume = false
headphone_surround = false

[video]
brightness = 1.0
contrast = 1.0
saturation = 1.0
hue = 0.0
gamma = 1.0
zoom = 1.0
aspect_ratio = "Auto"
rotate_degrees = 0
flip_horizontal = false
flip_vertical = false

[playback]
speed = 1.0
sleep_minutes = 0
show_subtitle_timeline = true
show_waveform = false
show_metadata = false
"""


def main() -> None:
    db_path = Path(sys.argv[1] if len(sys.argv) > 1 else r"D:\vfm-data\library.sqlite")
    scope_folder = sys.argv[2] if len(sys.argv) > 2 else r"D:\vfm-data\Show"
    name = sys.argv[3] if len(sys.argv) > 3 else "Solaris Demo"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS presets ("
        "id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE,"
        "scope TEXT NOT NULL, payload TEXT NOT NULL)"
    )
    conn.execute(
        "INSERT INTO presets(name,scope,payload) VALUES(?,?,?) "
        "ON CONFLICT(name) DO UPDATE SET scope=excluded.scope, payload=excluded.payload",
        (name, f"folder:{scope_folder}", PAYLOAD.strip()),
    )
    conn.commit()
    rows = conn.execute("SELECT name,scope FROM presets").fetchall()
    conn.close()
    print(f"seeded {db_path}: {rows}")


if __name__ == "__main__":
    main()
