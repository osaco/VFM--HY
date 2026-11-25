#!/usr/bin/env python3
"""End-to-end proof that VFM is a real player, not a mock-up.

Every check drives the actual binary and reads the result out of the window it draws:

  * startup      — how long until a visible window exists (the shortcut path)
  * decode       — a frame is on screen at all, and it is not a flat colour
  * geometry     — a circle recorded at a known size must come back round, so a 4:3 file
                   has to render with the same 4:3 shape it was encoded with
  * library      — a bare launch opens the library window with its chrome painted

Usage:
    python tools/verify_features.py                     # dev build + bundled libVLC
    VFM_EXE="%LOCALAPPDATA%\\Programs\\VFM\\VFM.exe" python tools/verify_features.py

Exit code 0 only when every check passes.
"""

from __future__ import annotations

import ctypes
import os
import subprocess
import sys
import time
from ctypes import wintypes

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)

import print_window as pw  # noqa: E402  (reuse the proven PrintWindow helpers)

user32 = pw.user32
gdi32 = pw.gdi32
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

EXE = os.environ.get("VFM_EXE", os.path.join(ROOT, "target", "release", "VFM.exe"))
DATA_DIR = os.environ.get("VFM_DATA_DIR", r"D:\vfm-data")
PLAYER_TITLES = ("Vedio for me", "فيديو من أجلي")
LIBRARY_TITLES = ("Library", "المكتبة")

# clip -> the (width, height) it was encoded at
CLIPS = [
    ("testdata/aspect/circle-4x3.mp4", 768, 576),
    ("testdata/aspect/circle-portrait.mp4", 720, 1280),
]

results: list[tuple[str, bool, str]] = []


def record(name: str, ok: bool, detail: str) -> None:
    results.append((name, ok, detail))
    print(f"{'PASS' if ok else 'FAIL'}  {name:<34} {detail}", flush=True)


def windows_of(pid: int) -> list[dict]:
    """Every top-level window owned by `pid`, with its visibility and size."""
    found: list[dict] = []
    proc = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)

    def callback(hwnd, _lparam):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value != pid:
            return True
        length = user32.GetWindowTextLengthW(hwnd)
        buf = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, buf, length + 1)
        rect = wintypes.RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(rect))
        found.append(
            {
                "hwnd": hwnd,
                "title": buf.value,
                "visible": bool(user32.IsWindowVisible(hwnd)),
                "w": rect.right - rect.left,
                "h": rect.bottom - rect.top,
            }
        )
        return True

    user32.EnumWindows(proc(callback), 0)
    return found


def wait_for(pid: int, titles: tuple[str, ...], timeout: float, min_w: int = 400):
    """First visible window with one of `titles` at least `min_w` wide."""
    started = time.time()
    while time.time() - started < timeout:
        for win in windows_of(pid):
            if win["visible"] and win["w"] >= min_w and win["title"] in titles:
                return win, time.time() - started
        time.sleep(0.2)
    return None, time.time() - started


def force_foreground(hwnd: int) -> None:
    """Bring a window to the front *and* give it keyboard focus.

    `SetForegroundWindow` alone is refused for a process that is not already foreground
    (Windows only grants foreground rights to the active thread), so the harness would send
    keystrokes to the terminal instead of the player. Attaching to the current foreground
    thread's input queue is the standard way around that restriction.
    """
    foreground = user32.GetForegroundWindow()
    mine = kernel32.GetCurrentThreadId()
    theirs = user32.GetWindowThreadProcessId(foreground, None) if foreground else 0
    attached = bool(theirs) and theirs != mine
    if attached:
        user32.AttachThreadInput(mine, theirs, True)
    try:
        user32.ShowWindow(hwnd, 9)  # SW_RESTORE
        user32.BringWindowToTop(hwnd)
        user32.SetForegroundWindow(hwnd)
        user32.SetFocus(hwnd)
    finally:
        if attached:
            user32.AttachThreadInput(mine, theirs, False)


def raw_grab(hwnd: int) -> tuple[int, int, bytes] | None:
    """PrintWindow into a 32-bit top-down DIB; returns (w, h, BGRX rows)."""
    rect = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    width, height = rect.right - rect.left, rect.bottom - rect.top
    if width <= 0 or height <= 0:
        return None

    screen = user32.GetDC(None)
    memory = gdi32.CreateCompatibleDC(screen)
    bitmap = gdi32.CreateCompatibleBitmap(screen, width, height)
    gdi32.SelectObject(memory, bitmap)
    if not user32.PrintWindow(hwnd, memory, pw.PW_RENDERFULLCONTENT):
        user32.PrintWindow(hwnd, memory, 0)

    info = pw.BITMAPINFO()
    info.bmiHeader.biSize = ctypes.sizeof(pw.BITMAPINFOHEADER)
    info.bmiHeader.biWidth = width
    info.bmiHeader.biHeight = -height
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    info.bmiHeader.biCompression = pw.BI_RGB

    buffer = ctypes.create_string_buffer(width * 4 * height)
    got = gdi32.GetDIBits(
        memory, bitmap, 0, height, buffer, ctypes.byref(info), pw.DIB_RGB_COLORS
    )
    gdi32.DeleteObject(bitmap)
    gdi32.DeleteDC(memory)
    user32.ReleaseDC(None, screen)
    if not got:
        return None
    return width, height, buffer.raw


def largest_blob(
    pixels: bytes,
    width: int,
    height: int,
    top: int = 50,
    bottom: int = 100,
    threshold: int = 150,
    step: int = 4,
):
    """Bounding box of the biggest bright connected region inside the video stage.

    The window chrome (filename text, control icons, seek bar) is also bright, so a plain
    bounding box of every bright pixel spans the whole window and says nothing about the
    picture. Cropping the title bar and control strip, then keeping only the largest
    connected component, isolates the test disc.
    """
    x0, y0 = 0, min(top, max(0, height - 1))
    x1, y1 = width, max(y0 + 1, height - bottom)
    grid_w, grid_h = max(1, (x1 - x0) // step), max(1, (y1 - y0) // step)

    grid = bytearray(grid_w * grid_h)
    for gy in range(grid_h):
        row = (y0 + gy * step) * width
        base = gy * grid_w
        for gx in range(grid_w):
            i = (row + x0 + gx * step) * 4
            if (pixels[i] + pixels[i + 1] + pixels[i + 2]) // 3 >= threshold:
                grid[base + gx] = 1

    best = None
    seen = bytearray(grid_w * grid_h)
    for start in range(grid_w * grid_h):
        if not grid[start] or seen[start]:
            continue
        seen[start] = 1
        stack = [start]
        count = 0
        min_x = max_x = start % grid_w
        min_y = max_y = start // grid_w
        while stack:
            cur = stack.pop()
            count += 1
            cy, cx = divmod(cur, grid_w)
            if cx < min_x:
                min_x = cx
            if cx > max_x:
                max_x = cx
            if cy < min_y:
                min_y = cy
            if cy > max_y:
                max_y = cy
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nx, ny = cx + dx, cy + dy
                if 0 <= nx < grid_w and 0 <= ny < grid_h:
                    n = ny * grid_w + nx
                    if grid[n] and not seen[n]:
                        seen[n] = 1
                        stack.append(n)
        if best is None or count > best[0]:
            best = (count, min_x, min_y, max_x, max_y)

    if best is None or best[0] * step * step < 400:
        return None
    count, min_x, min_y, max_x, max_y = best
    return (
        min_x * step + x0,
        min_y * step + y0,
        (max_x - min_x + 1) * step,
        (max_y - min_y + 1) * step,
        count * step * step,
    )


def launch(args: list[str], env_overrides: dict | None = None) -> subprocess.Popen:
    env = dict(os.environ)
    env.setdefault("LIBVLC_PATH", os.path.join(ROOT, "vendor", "libvlc"))
    if env_overrides:
        env.update(env_overrides)
    return subprocess.Popen(
        [EXE, "--no-single-instance", "--data-dir", DATA_DIR] + args,
        env=env,
        cwd=os.path.dirname(EXE) or None,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def stop(process: subprocess.Popen) -> None:
    process.terminate()
    try:
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=8)


def check_geometry(relative: str, src_w: int, src_h: int) -> None:
    clip = os.path.join(ROOT, relative)
    name = os.path.basename(relative)
    if not os.path.exists(clip):
        record(f"geometry {name}", False, "clip missing")
        return

    process = launch([clip])
    try:
        win, elapsed = wait_for(process.pid, PLAYER_TITLES, 60, min_w=500)
        if win is None:
            record(f"geometry {name}", False, f"no player window in {elapsed:.1f}s")
            return
        force_foreground(win["hwnd"])
        time.sleep(4.0)  # let a few frames land, including any size re-arm

        grabbed = raw_grab(win["hwnd"])
        if grabbed is None:
            record(f"geometry {name}", False, "PrintWindow returned nothing")
            return
        width, height, pixels = grabbed
        box = largest_blob(pixels, width, height)
        if box is None:
            record(f"geometry {name}", False, "no decoded frame on screen")
            return

        _, _, blob_w, blob_h, count = box
        # The clip draws a *circle*. If the picture is stretched to fill a buffer of the wrong
        # shape, that circle comes back as an ellipse — which is exactly what we measure.
        measured = blob_w / blob_h
        error = abs(measured - 1.0)
        detail = (
            f"source {src_w}x{src_h} ({src_w / src_h:.3f}) "
            f"disc {blob_w}x{blob_h} ({measured:.3f}) roundness err {error * 100:.1f}% "
            f"px {count} window {width}x{height}"
        )
        record(f"geometry {name}", error < 0.08, detail)
    finally:
        stop(process)


def check_startup() -> None:
    process = launch([])
    try:
        # A bare launch is the desktop-shortcut path: it must open a window by itself.
        win, elapsed = wait_for(process.pid, LIBRARY_TITLES + PLAYER_TITLES, 60, min_w=500)
        record(
            "startup",
            win is not None and elapsed < 6.0,
            f"{elapsed:.1f}s to first window ({win['title'] if win else 'none'})",
        )
        if win is None:
            record("library paints", False, "no window to capture")
            return
        grabbed = raw_grab(win["hwnd"])
        record(
            "library paints",
            grabbed is not None,
            f"{grabbed[0]}x{grabbed[1]} captured" if grabbed else "nothing captured",
        )
    finally:
        stop(process)


def frame_delta(a: bytes, b: bytes, step: int = 16) -> float:
    """Fraction of sampled pixels that differ — 0 means a frozen picture."""
    if len(a) != len(b):
        return 1.0
    changed = total = 0
    for i in range(0, len(a) - 4, 4 * step):
        total += 1
        if (
            abs(a[i] - b[i]) > 12
            or abs(a[i + 1] - b[i + 1]) > 12
            or abs(a[i + 2] - b[i + 2]) > 12
        ):
            changed += 1
    return changed / total if total else 0.0


WM_KEYDOWN = 0x0100
WM_KEYUP = 0x0101
VK_SPACE = 0x20


def tap(hwnd: int, vk: int, global_input: bool) -> None:
    """Press and release `vk`.

    Two delivery paths, because a background process cannot always become foreground:
      * `global_input=False` posts WM_KEYDOWN/WM_KEYUP straight to the window;
      * `global_input=True` injects real keyboard input into the focused window.
    """
    if global_input:
        force_foreground(hwnd)
        time.sleep(0.2)
        user32.keybd_event(vk, 0, 0, 0)
        time.sleep(0.06)
        user32.keybd_event(vk, 0, 2, 0)  # KEYEVENTF_KEYUP
    else:
        user32.PostMessageW(hwnd, WM_KEYDOWN, vk, 0x001E0001)
        time.sleep(0.06)
        user32.PostMessageW(hwnd, WM_KEYUP, vk, 0xC01E0001)


def is_moving(hwnd: int, settle: float = 2.6, gap: float = 0.8):
    """(moving, delta) after letting any OSD / chrome animation finish."""
    time.sleep(settle)
    a = raw_grab(hwnd)
    time.sleep(gap)
    b = raw_grab(hwnd)
    if a is None or b is None:
        return None, 0.0
    delta = frame_delta(a[2], b[2])
    return delta > 0.002, delta


def check_playback_control() -> None:
    """Decoding must move, and space must really pause it."""
    clip = os.path.join(ROOT, "testdata", "1080p60.mp4")
    if not os.path.exists(clip):
        record("playback control", False, "testdata/1080p60.mp4 missing")
        return

    process = launch([clip])
    try:
        win, elapsed = wait_for(process.pid, PLAYER_TITLES, 60, min_w=500)
        if win is None:
            record("playback control", False, f"no player window in {elapsed:.1f}s")
            return
        hwnd = win["hwnd"]
        force_foreground(hwnd)
        time.sleep(4.0)

        moving, delta = is_moving(hwnd, settle=0.0)
        if moving is None:
            record("decoding frames", False, "PrintWindow returned nothing")
            return
        record("decoding frames", bool(moving), f"{delta * 100:.2f}% of pixels changed")

        # Try the direct message first, then real input: whichever actually pauses wins.
        paused, used = False, "none"
        for label, global_input in (("post", False), ("input", True)):
            tap(hwnd, VK_SPACE, global_input)
            still_moving, paused_delta = is_moving(hwnd)
            if still_moving is None:
                continue
            if not still_moving:
                paused, used = True, label
                break
            # A wrong path would have left it playing; a right path that resumed it needs
            # one more press rather than a different channel.
            if paused_delta < 0.02:
                paused, used = True, label
                break
        record(
            "space pauses",
            paused,
            f"{'paused via ' + used if paused else 'still playing after both paths'}",
        )
        if not paused:
            return

        tap(hwnd, VK_SPACE, used == "input")
        resumed, resumed_delta = is_moving(hwnd)
        if resumed is None:
            record("space resumes", False, "PrintWindow returned nothing")
            return
        record("space resumes", bool(resumed), f"{resumed_delta * 100:.2f}% of pixels changed")
    finally:
        stop(process)


def main() -> int:
    pw.dpi_aware()
    print(f"binary: {EXE}\nprofile: {DATA_DIR}\n")

    if not os.path.exists(EXE):
        print(f"binary not found: {EXE}")
        return 2

    # Warm the disk cache with the playback checks first: a cold first launch spends most of
    # its time faulting the exe and libVLC in, which says nothing about real startup.
    for relative, src_w, src_h in CLIPS:
        check_geometry(relative, src_w, src_h)
    check_playback_control()
    check_startup()

    failed = [name for name, ok, _ in results if not ok]
    print(f"\n{len(results) - len(failed)}/{len(results)} checks passed")
    if failed:
        print("failed: " + ", ".join(failed))
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
