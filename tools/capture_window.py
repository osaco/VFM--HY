#!/usr/bin/env python3
"""Capture the VFM player window for documentation screenshots.

The window title is localised ("Vedio for me" / "فيديو من أجلي"), so the tool
matches either title. It restores the window if it was minimised, brings it to
the front, and retries the grab until the frame is not covered by another
application.

Usage:
    python tools/capture_window.py <output.png> [--tab N] [--data-dir DIR]
"""

from __future__ import annotations

import argparse
import ctypes
import os
import struct
import subprocess
import sys
import time
from ctypes import wintypes

TITLES = ("Vedio for me", "فيديو من أجلي")
APP = os.environ.get("VFM_EXE", r"D:\تجاربي\VLC MAINE 2 HY\target\release\VFM.exe")
LIBVLC = os.environ.get("LIBVLC_PATH", r"D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc")

user32 = ctypes.WinDLL("user32", use_last_error=True)
EnumWindows = user32.EnumWindows
EnumWindowsProc = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)
GetWindowTextW = user32.GetWindowTextW
GetWindowTextLengthW = user32.GetWindowTextLengthW
ShowWindow = user32.ShowWindow
SetForegroundWindow = user32.SetForegroundWindow
BringWindowToTop = user32.BringWindowToTop

SW_RESTORE = 9


def find_window(titles: tuple[str, ...] = TITLES) -> tuple[int, str] | None:
    hits: list[tuple[int, str]] = []

    def callback(hwnd, _lparam):
        length = GetWindowTextLengthW(hwnd)
        buf = ctypes.create_unicode_buffer(length + 1)
        GetWindowTextW(hwnd, buf, length + 1)
        if buf.value in titles:
            hits.append((hwnd, buf.value))
        return True

    EnumWindows(EnumWindowsProc(callback), 0)
    return hits[0] if hits else None


def png_size(path: str) -> tuple[int, int]:
    with open(path, "rb") as handle:
        data = handle.read(24)
    return struct.unpack(">II", data[16:24])


def launch(args: argparse.Namespace) -> subprocess.Popen:
    env = dict(os.environ)
    env["LIBVLC_PATH"] = LIBVLC
    command = [APP]
    if args.media:
        command.append(args.media)
    command += ["--no-single-instance", "--data-dir", args.data_dir]
    if args.tab is not None:
        command += ["--sidebar", str(args.tab)]
    command += list(args.extra or [])
    return subprocess.Popen(command, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def grab(title: str, output: str, attempts: int = 12, titles: tuple[str, ...] | None = None) -> bool:
    native = os.path.abspath(output).replace("\\", "/")
    for _ in range(attempts):
        window = find_window(titles or TITLES)
        if window is None:
            time.sleep(0.5)
            continue
        hwnd, window_title = window
        ShowWindow(hwnd, SW_RESTORE)
        SetForegroundWindow(hwnd)
        BringWindowToTop(hwnd)
        # Park the pointer in the corner so it never sits on top of a control.
        user32.SetCursorPos(1918, 1078)
        # A freshly shown window needs a moment to paint; grabbing too early yields the
        # desktop showing through the frame.
        time.sleep(1.5)
        if os.path.exists(native):
            os.remove(native)
        subprocess.run(
            [
                "ffmpeg", "-hide_banner", "-loglevel", "error",
                "-f", "gdigrab", "-draw_mouse", "0", "-i", f"title={window_title}",
                "-frames:v", "1", "-update", "1", "-y", native,
            ],
            check=False,
        )
        if os.path.exists(native) and os.path.getsize(native) > 40_000:
            print(f"captured {window_title} -> {native} {png_size(native)}")
            return True
    return False


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output")
    parser.add_argument("--tab", type=int, default=None, help="sidebar tab to open")
    parser.add_argument("--media", default=r"D:\vfm-data\Show\Ep01.mkv")
    parser.add_argument("--data-dir", default=r"D:\vfm-data")
    parser.add_argument("--no-launch", action="store_true", help="capture the running instance")
    parser.add_argument("--title", default=None, help="exact window title to grab")
    parser.add_argument("--extra", action="append", default=[], help="extra CLI args for the app")
    parser.add_argument("--wait", type=float, default=9.0, help="seconds to wait after launch")
    args = parser.parse_args()

    process = None
    if not args.no_launch:
        process = launch(args)
        time.sleep(args.wait)
    try:
        titles = (args.title,) if args.title else TITLES
        return 0 if grab(titles[0], args.output, titles=titles) else 1
    finally:
        if process is not None:
            process.terminate()


if __name__ == "__main__":
    sys.exit(main())
