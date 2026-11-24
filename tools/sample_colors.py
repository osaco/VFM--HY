import subprocess, struct, collections, os, sys

FF = r"D:\ffmpeg\ffmpeg-master-latest-win64-gpl-shared\ffmpeg-master-latest-win64-gpl-shared\bin\ffmpeg.exe"
BASE = r"D:\تجاربي\VLC MAINE 2 HY\design"
names = ["Player.png", "Player-1.png", "Settings - global.png", "right-click menu.png"]
for name in names:
    p = os.path.join(BASE, name)
    with open(p, "rb") as f:
        h = f.read(24)
    w, ht = struct.unpack(">II", h[16:24])
    r = subprocess.run([FF, "-v", "quiet", "-i", p, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], capture_output=True)
    raw = r.stdout
    if not raw:
        print("FAIL", name, r.stderr[:200]); continue
    c = collections.Counter(tuple(raw[i:i+3]) for i in range(0, len(raw), 3))
    print("\n=== %s  %dx%d ===" % (name, w, ht))
    for col, n in c.most_common(12):
        print("  #%02X%02X%02X  %8d  %5.1f%%" % (col[0], col[1], col[2], n, 100.0 * n / (len(raw) // 3)))
    for label, fx, fy in [("titlebar",0.5,0.015),("sidebar",0.06,0.5),("video",0.6,0.3),("controlbar",0.5,0.965),("accent",0.27,0.965)]:
        x, y = int(w*fx), int(ht*fy); i = (y*w+x)*3
        if i+2 < len(raw):
            print("  probe %-11s #%02X%02X%02X" % (label, raw[i], raw[i+1], raw[i+2]))
