"""分析原始帧：帧间差异 + 绿幕前景质心，用于切分运动区间与验证身体稳定性。"""
import glob
import json
import os
import sys

import numpy as np
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
raw_name = "raw"
for i, a in enumerate(sys.argv):
    if a == "--raw" and i + 1 < len(sys.argv):
        raw_name = sys.argv[i + 1]
raw_dir = os.path.join(ROOT, "build", raw_name)
frames = sorted(glob.glob(os.path.join(raw_dir, "*.png")))

rows = []
prev = None
for i, path in enumerate(frames):
    im = np.asarray(Image.open(path).convert("RGB")).astype(np.int16)
    diff = 0.0 if prev is None else float(np.abs(im - prev).mean())

    r, g, b = im[..., 0], im[..., 1], im[..., 2]
    fg = ~((g > 90) & (g > r + 35) & (g > b + 35))
    area = int(fg.sum())
    ys, xs = np.nonzero(fg)
    if len(xs):
        cx, cy = float(xs.mean()), float(ys.mean())
        x0, x1, y0, y1 = int(xs.min()), int(xs.max()), int(ys.min()), int(ys.max())
    else:
        cx = cy = 0.0
        x0 = x1 = y0 = y1 = 0

    rows.append(
        {
            "i": i,
            "diff": round(diff, 3),
            "area": area,
            "cx": round(cx, 1),
            "cy": round(cy, 1),
            "bbox": [x0, y0, x1, y1],
        }
    )
    prev = im

with open(os.path.join(ROOT, "build", "motion.json"), "w", encoding="utf-8") as f:
    json.dump(rows, f, ensure_ascii=False)

diffs = np.array([r["diff"] for r in rows])
print(f"frames={len(rows)} diff min={diffs.min():.2f} max={diffs.max():.2f} mean={diffs.mean():.2f}")
print("per-frame diff (i:diff), 0-based:")
line = []
for r in rows:
    line.append(f"{r['i']}:{r['diff']}")
    if len(line) == 12:
        print("  " + "  ".join(line))
        line = []
if line:
    print("  " + "  ".join(line))

# 静止段判定：滑动窗口均值低于阈值
win = 5
thr = 1.5
quiet = []
for i in range(len(rows)):
    lo, hi = max(0, i - win), min(len(rows), i + win + 1)
    quiet.append(float(diffs[lo:hi].mean()) < thr)
print("\nquiet frames (window mean < %.1f):" % thr)
seg, start = [], None
for i, q in enumerate(quiet):
    if q and start is None:
        start = i
    elif not q and start is not None:
        seg.append((start, i - 1))
        start = None
if start is not None:
    seg.append((start, len(quiet) - 1))
for s, e in seg:
    if e - s >= 2:
        print(f"  {s}..{e}  ({e - s + 1} frames)")

cx = np.array([r["cx"] for r in rows])
cy = np.array([r["cy"] for r in rows])
print(f"\ncentroid x range={cx.min():.1f}..{cx.max():.1f} (std {cx.std():.1f})")
print(f"centroid y range={cy.min():.1f}..{cy.max():.1f} (std {cy.std():.1f})")
areas = np.array([r["area"] for r in rows])
print(f"fg area range={areas.min()}..{areas.max()} (mean {areas.mean():.0f})")
