#!/usr/bin/env python3
"""Capture a window through PrintWindow, which works even when the window is occluded.

gdigrab grabs the screen, so a window that the compositor has not painted (or that sits
behind another window) comes back as the desktop. PrintWindow asks the window to render
itself into our bitmap instead.

Usage:
    python tools/print_window.py <output.png> --title "Settings" [--extra=--settings]
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
APP = os.environ.get("VFM_EXE", r"D:\تجاربي\VLC MAINE 2 HY\target\release\vfm-spike.exe")
LIBVLC = os.environ.get("LIBVLC_PATH", r"D:\تجاربي\VLC MAINE 2 HY\vendor\libvlc")

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)

PW_RENDERFULLCONTENT = 0x00000002
DIB_RGB_COLORS = 0
BI_RGB = 0


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", wintypes.LONG),
        ("biHeight", wintypes.LONG),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", wintypes.LONG),
        ("biYPelsPerMeter", wintypes.LONG),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]


def dpi_aware() -> None:
    try:
        user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2
    except Exception:
        user32.SetProcessDPIAware()


def find_window(title: str) -> int | None:
    hits: list[int] = []
    proc = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)

    def callback(hwnd, _lparam):
        length = user32.GetWindowTextLengthW(hwnd)
        buf = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, buf, length + 1)
        if buf.value == title:
            hits.append(hwnd)
        return True

    user32.EnumWindows(proc(callback), 0)
    return hits[0] if hits else None


def grab(hwnd: int, output: str) -> bool:
    rect = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    width, height = rect.right - rect.left, rect.bottom - rect.top
    if width <= 0 or height <= 0:
        return False

    screen = user32.GetDC(None)
    memory = gdi32.CreateCompatibleDC(screen)
    bitmap = gdi32.CreateCompatibleBitmap(screen, width, height)
    gdi32.SelectObject(memory, bitmap)
    ok = user32.PrintWindow(hwnd, memory, PW_RENDERFULLCONTENT)
    if not ok:
        ok = user32.PrintWindow(hwnd, memory, 0)

    info = BITMAPINFO()
    info.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    info.bmiHeader.biWidth = width
    info.bmiHeader.biHeight = -height  # top-down
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    info.bmiHeader.biCompression = BI_RGB

    stride = width * 4
    buffer = ctypes.create_string_buffer(stride * height)
    got = gdi32.GetDIBits(memory, bitmap, 0, height, buffer, ctypes.byref(info), DIB_RGB_COLORS)

    gdi32.DeleteObject(bitmap)
    gdi32.DeleteDC(memory)
    user32.ReleaseDC(None, screen)
    if not got:
        return False

    # Write a BMP and let ffmpeg produce the PNG (no Pillow on this host).
    bmp_header = struct.pack("<2sIHHI", b"BM", 14 + 40 + len(buffer.raw), 0, 0, 54)
    dib = struct.pack(
        "<IiiHHIIiiII", 40, width, -height, 1, 32, 0, len(buffer.raw), 2835, 2835, 0, 0
    )
    bmp_path = output + ".bmp"
    with open(bmp_path, "wb") as handle:
        handle.write(bmp_header + dib + buffer.raw)
    subprocess.run(["ffmpeg", "-y", "-v", "error", "-i", bmp_path, output], check=False)
    os.remove(bmp_path)
    return os.path.exists(output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output")
    parser.add_argument("--title", default=TITLES[0])
    parser.add_argument("--media", default="")
    parser.add_argument("--data-dir", default=r"D:\vfm-data")
    parser.add_argument("--extra", action="append", default=[])
    parser.add_argument("--wait", type=float, default=10.0)
    parser.add_argument("--no-launch", action="store_true")
    args = parser.parse_args()

    dpi_aware()
    process = None
    if not args.no_launch:
        env = dict(os.environ)
        env["LIBVLC_PATH"] = LIBVLC
        command = [APP, "--no-single-instance", "--data-dir", args.data_dir]
        if args.media:
            command.append(args.media)
        command += list(args.extra)
        process = subprocess.Popen(
            command, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )
        time.sleep(args.wait)

    try:
        for _ in range(20):
            hwnd = find_window(args.title)
            if hwnd is not None:
                user32.ShowWindow(hwnd, 9)
                user32.SetForegroundWindow(hwnd)
                user32.SetCursorPos(1918, 1078)
                time.sleep(0.6)
                if grab(hwnd, args.output):
                    print(f"captured {args.title} -> {args.output}")
                    return 0
            time.sleep(0.5)
        return 1
    finally:
        if process is not None:
            process.terminate()


if __name__ == "__main__":
    sys.exit(main())
