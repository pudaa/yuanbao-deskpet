"""用图像特征交叉验证每帧朝向，辅助 contact sheet 人工标定。

两个代理量（仅用于排序与校验，不是真实角度）：
- yaw_proxy: 头部 ROI 内低饱和高亮区域（鼻梁/口周白毛）的质心 x 相对 ROI 中心的归一化偏移。
  猫头转向右侧时，鼻梁白毛整体右移 -> 该值变大。
- pitch_proxy: 头部 ROI 内前景最上沿（耳尖）的 y 位置，抬头时耳尖上升 -> 该值变小。
"""
import glob
import json
import os

import numpy as np
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RAW = os.path.join(ROOT, "build", "raw")

# 源视频 720x720 下的头部 ROI
HX0, HX1 = 140, 430
HY0, HY1 = 130, 385


def probe(path):
    rgb = np.asarray(Image.open(path).convert("RGB")).astype(np.float32) / 255.0
    roi = rgb[HY0:HY1, HX0:HX1]
    r, g, b = roi[..., 0], roi[..., 1], roi[..., 2]
    mx = np.maximum(np.maximum(r, g), b)
    mn = np.minimum(np.minimum(r, g), b)

    white = (mn > 0.60) & ((mx - mn) < 0.12)
    ys, xs = np.nonzero(white)
    if len(xs) > 30:
        wx = xs.mean() / roi.shape[1]
        wy = ys.mean() / roi.shape[0]
    else:
        wx = wy = 0.5

    fg = ~((g > 0.35) & (g > r + 0.14) & (g > b + 0.14))
    fys, fxs = np.nonzero(fg)
    top = fys.min() / roi.shape[0] if len(fys) else 1.0

    return float(wx), float(wy), float(top)


def main():
    names = sorted(glob.glob(os.path.join(RAW, "*.png")))
    rows = []
    for i, p in enumerate(names):
        wx, wy, top = probe(p)
        rows.append({"i": i, "wx": round(wx, 4), "wy": round(wy, 4), "top": round(top, 4)})

    with open(os.path.join(ROOT, "build", "probe.json"), "w", encoding="utf-8") as f:
        json.dump(rows, f, indent=1)

    wx = np.array([r["wx"] for r in rows])
    top = np.array([r["top"] for r in rows])
    print("frame   wx      top     yaw_proxy(z)  pitch_proxy(z)")
    wz = (wx - wx.mean()) / (wx.std() + 1e-9)
    tz = (top - top.mean()) / (top.std() + 1e-9)
    for i in range(0, len(rows), 10):
        print(f"{i:5d}  {rows[i]['wx']:.4f}  {rows[i]['top']:.4f}   {wz[i]:+6.2f}       {tz[i]:+6.2f}")
    print(f"\nwx  min={wx.min():.4f} max={wx.max():.4f}")
    print(f"top min={top.min():.4f} max={top.max():.4f}")
    imin, imax = int(np.argmin(wx)), int(np.argmax(wx))
    print(f"wx  argmin(most left)  frame={imin}")
    print(f"wx  argmax(most right) frame={imax}")
    print(f"top argmin(head highest) frame={int(np.argmin(top))}")
    print(f"top argmax(head lowest)  frame={int(np.argmax(top))}")


if __name__ == "__main__":
    main()
