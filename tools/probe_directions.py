"""真机八方向验证：把鼠标依次移到猫的 8 个方位，截取窗口，拼成对照图。

用法: python scripts/probe_directions.py
前置: 桌宠已启动，且窗口大致在屏幕中央（否则部分方位会落到屏幕外）
"""
import ctypes
import os
import time
from ctypes import wintypes as wt

from PIL import Image, ImageDraw, ImageFont, ImageGrab

ctypes.windll.shcore.SetProcessDpiAwareness(2)
u32 = ctypes.windll.user32

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "build", "dirs")
DIST = 150


def find_pet():
    for title in ("圆宝桌宠",):
        h = u32.FindWindowW(None, title)
        if h:
            return h
    # 无标题时退回：
    return 0


def main():
    os.makedirs(OUT, exist_ok=True)
    hwnd = find_pet()
    if not hwnd:
        print("pet window not found")
        return
    r = wt.RECT()
    u32.GetWindowRect(hwnd, ctypes.byref(r))
    cx = (r.left + r.right) // 2
    cy = (r.top + r.bottom) // 2
    w, h = r.right - r.left, r.bottom - r.top
    print(f"window rect=({r.left},{r.top},{w}x{h}) center=({cx},{cy})")

    dirs = [
        ("up", 0, -1),
        ("upright", 1, -1),
        ("right", 1, 0),
        ("downright", 1, 1),
        ("down", 0, 1),
        ("downleft", -1, 1),
        ("left", -1, 0),
        ("upleft", -1, -1),
    ]

    shots = []
    for name, sx, sy in dirs:
        px = int(cx + sx * DIST / (1.414 if sx and sy else 1))
        py = int(cy + sy * DIST / (1.414 if sx and sy else 1))
        u32.SetCursorPos(px, py)
        time.sleep(0.75)
        im = ImageGrab.grab(bbox=(r.left, r.top, r.right, r.bottom))
        p = os.path.join(OUT, f"{name}.png")
        im.save(p)
        shots.append((name, p))
        print(f"  {name:10s} cursor=({px},{py}) -> {os.path.basename(p)}")

    # idle：鼠标移到猫中心
    u32.SetCursorPos(cx, cy)
    time.sleep(0.9)
    im = ImageGrab.grab(bbox=(r.left, r.top, r.right, r.bottom))
    p = os.path.join(OUT, "idle.png")
    im.save(p)
    shots.append(("idle", p))

    cell = 220
    cols = 3
    rows = 3
    sheet = Image.new("RGB", (cell * cols, cell * rows), (245, 245, 245))
    d = ImageDraw.Draw(sheet)
    f = ImageFont.truetype("C:/Windows/Fonts/segoeui.ttf", 18)

    order = [
        ("upleft", 0, 0),
        ("up", 1, 0),
        ("upright", 2, 0),
        ("left", 0, 1),
        ("idle", 1, 1),
        ("right", 2, 1),
        ("downleft", 0, 2),
        ("down", 1, 2),
        ("downright", 2, 2),
    ]
    m = {n: p for n, p in shots}
    for name, gx, gy in order:
        if name not in m:
            continue
        im = Image.open(m[name]).convert("RGB").resize((cell, cell), Image.LANCZOS)
        sheet.paste(im, (gx * cell, gy * cell))
        d.text((gx * cell + 6, gy * cell + 4), name, fill=(200, 0, 0), font=f)
    out = os.path.join(ROOT, "build", "dirs_grid.png")
    sheet.save(out)
    print("grid ->", out)


if __name__ == "__main__":
    main()
