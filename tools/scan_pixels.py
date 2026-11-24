"""Scan color transitions along a row or column of a PNG via ffmpeg raw output.

Usage:
    python tools/scan_pixels.py row <y> <x0> <x1> [--file design/Player.png] [--tol 6]
    python tools/scan_pixels.py col <x> <y0> <y1> [--file design/Player.png] [--tol 6]

Prints "coord  #RRGGBB  (r,g,b)" for every run of near-constant color, so control-bar
geometry can be measured from the Figma exports without an image library.
"""

from __future__ import annotations

import argparse
import subprocess
import sys


def grab(path: str, x: int, y: int, w: int, h: int) -> bytes:
    cmd = [
        "ffmpeg", "-v", "error", "-i", path,
        "-vf", f"crop={w}:{h}:{x}:{y}",
        "-f", "rawvideo", "-pix_fmt", "rgb24", "-",
    ]
    return subprocess.run(cmd, capture_output=True, check=True).stdout


def near(a: tuple[int, int, int], b: tuple[int, int, int], tol: int) -> bool:
    return all(abs(a[i] - b[i]) <= tol for i in range(3))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("axis", choices=["row", "col"])
    ap.add_argument("fixed", type=int)
    ap.add_argument("start", type=int)
    ap.add_argument("end", type=int)
    ap.add_argument("--file", default="design/Player.png")
    ap.add_argument("--tol", type=int, default=6)
    args = ap.parse_args()

    span = args.end - args.start
    if span <= 0:
        print("empty range", file=sys.stderr)
        return 2

    if args.axis == "row":
        data = grab(args.file, args.start, args.fixed, span, 1)
    else:
        data = grab(args.file, args.fixed, args.start, 1, span)

    prev: tuple[int, int, int] | None = None
    run_start = args.start
    for i in range(0, len(data) - 2, 3):
        c = (data[i], data[i + 1], data[i + 2])
        if prev is None:
            prev, run_start = c, args.start
            continue
        if not near(c, prev, args.tol):
            length = args.start + i // 3 - run_start
            print(f"{run_start:>6}..{run_start + length - 1:<6} len={length:<5} "
                  f"#{prev[0]:02X}{prev[1]:02X}{prev[2]:02X}  {prev}")
            prev, run_start = c, args.start + i // 3
    length = args.end - run_start
    if prev is not None:
        print(f"{run_start:>6}..{args.end - 1:<6} len={length:<5} "
              f"#{prev[0]:02X}{prev[1]:02X}{prev[2]:02X}  {prev}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
