"""V2：由 8 个方位锚点生成 frames.json 与 contact_sheet.png。

V2 素材的身体固定、头部按 上→右上→右→右下→下→左下→左→左上→上 顺时针转一整圈，
首尾闭合。因此**帧序本身就是朝向角**：
    帧 0 = 上（屏幕坐标 -90°），帧 k = -90° + k 度，帧 360 回到上。
运行时直接按鼠标方位角取帧，不再做"轨迹顺序"映射。
"""
import json
import os

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FRAMES = os.path.join(ROOT, "extracted_frames")
N = 361  # 0..360
MAIN_START = 32  # 帧 0..31 是抬头动画，不属于朝向角

# 方位角 -> 主序列帧号：frame = MAIN_START + deg/360*(360-MAIN_START)
def f(deg):
    return round(MAIN_START + deg / 360 * (360 - MAIN_START))


# (帧号, 表观 yaw, 表观 pitch, 方位)
ANCHORS = [
    (f(0), 0, 40, "上"),
    (f(45), 30, 28, "右上"),
    (f(90), 45, 0, "右"),
    (f(135), 30, -28, "右下"),
    (f(180), 0, -40, "下"),
    (f(225), -30, -28, "左下"),
    (f(270), -45, 0, "左"),
    (f(315), -30, 28, "左上"),
    (f(360), 0, 40, "上（闭环）"),
]


def interp():
    fr = np.array([a[0] for a in ANCHORS], dtype=float)
    yaw = np.array([a[1] for a in ANCHORS], dtype=float)
    pit = np.array([a[2] for a in ANCHORS], dtype=float)
    xs = np.arange(N)
    return np.interp(xs, fr, yaw), np.interp(xs, fr, pit)


def load_font(size):
    for name in ("msyh.ttc", "segoeui.ttf", "arial.ttf"):
        p = os.path.join("C:\\Windows\\Fonts", name)
        if os.path.exists(p):
            try:
                return ImageFont.truetype(p, size)
            except OSError:
                pass
    return ImageFont.load_default()


def contact_sheet(yaw, pit, out, cols=8, cell=180, step=8):
    idxs = list(range(0, N, step))
    rows = (len(idxs) + cols - 1) // cols
    pad = 32
    cw, ch = cell, cell + pad
    sheet = Image.new("RGB", (cw * cols, ch * rows), (26, 26, 28))
    d = ImageDraw.Draw(sheet)
    f1 = load_font(max(12, cell // 13))
    f2 = load_font(max(11, cell // 16))

    for n, i in enumerate(idxs):
        im = Image.open(os.path.join(FRAMES, f"{i:04d}.png")).convert("RGBA")
        bg = Image.new("RGBA", im.size, (255, 255, 255, 255))
        im = Image.alpha_composite(bg, im).convert("RGB").resize((cell, cell), Image.LANCZOS)
        x, y = (n % cols) * cw, (n // cols) * ch
        sheet.paste(im, (x, y))
        d.text((x + 5, y + cell + 2), f"#{i}", fill=(255, 214, 102), font=f1)
        d.text((x + 5, y + cell + 16), f"y{yaw[i]:+.0f} p{pit[i]:+.0f}", fill=(150, 150, 155), font=f2)
    sheet.save(out)
    print(f"contact_sheet -> {out}  {sheet.size[0]}x{sheet.size[1]}  ({len(idxs)} frames)")


def main():
    yaw, pit = interp()

    payload = {
        "frameCount": N,
        "size": 384,
        "format": "webp",
        "closed": True,
        "angleMode": "index",
        "degreeOffset": -90,
        "mainStart": MAIN_START,
        "idleFrame": 0,
        "headLiftFrames": [0, MAIN_START - 1],
        "note": (
            "V2 素材：身体固定、头部顺时针转 360°。"
            "帧 0..31 是从低头抬起到看正上方的抬头动画，不参与角度索引（仅用作极点 idle）；"
            "方位角只映射到帧 32..360，帧 32 与帧 360 姿态基本重合。"
            "yaw/pitch 为按 contact sheet 目测的表观角度，用于文档与调试，不参与索引。"
        ),
        "angleAnchors": [
            {"frame": f, "yaw": y, "pitch": p, "dir": name} for f, y, p, name in ANCHORS
        ],
        "angleKeys": [
            {"frame": i, "yaw": round(float(yaw[i]), 2), "pitch": round(float(pit[i]), 2)}
            for i in range(N)
        ],
    }

    out_json = os.path.join(ROOT, "assets", "frames.json")
    with open(out_json, "w", encoding="utf-8") as f:
        json.dump(payload, f, ensure_ascii=False, indent=1)
    print(f"frames.json -> {out_json}  (N={N}, closed=True)")

    contact_sheet(yaw, pit, os.path.join(ROOT, "contact_sheet.png"))


if __name__ == "__main__":
    main()
