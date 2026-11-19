"""绿幕抠像：把视频帧转成透明 RGBA。

设计要点（针对"白毛/灰毛不能被误删"）：
1. 用 G - max(R,B) 作为"绿度"，而不是 G - (R+B)/2。
   白毛被绿幕反射后通常仍满足 R≈B 且都较高，G-max(R,B) 很小，因此安全。
2. 叠加"高亮中性"保护：亮度高且色度中性的一律判为不透明。
3. 内部洞填充：猫眼偏黄绿、容易被判成背景，用 fill_holes 补回。
4. despill：绿边像素把 G 压到 max(R,B)，消除绿边。
5. 所有帧共用同一个 bbox 与缩放系数，保证帧间严格对齐。

用法:
  python scripts/chroma_key.py --debug          # 只处理若干关键帧并输出检查图
  python scripts/chroma_key.py --all            # 处理全部 241 帧
"""
import argparse
import glob
import json
import os

import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RAW = os.path.join(ROOT, "build", "raw")
OUT_FRAMES = os.path.join(ROOT, "extracted_frames")
BUILD = os.path.join(ROOT, "build")

T_LO = 0.18   # 绿度低于此值 -> 完全保留
T_HI = 0.46   # 绿度高于此值 -> 完全透明
BRIGHT_PROTECT = 0.72  # min(R,G,B) 高于此值视为高亮中性（白毛保护）
FEATHER_SIGMA = 0.8
MIN_BLOB = 300  # 小于该面积的孤立前景块视为噪点


def green_alpha(rgb):
    """返回 [0,1] 的前景 alpha。"""
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    greenness = g - np.maximum(r, b)
    alpha = np.clip((T_HI - greenness) / (T_HI - T_LO), 0.0, 1.0)

    # 高亮中性保护：白毛、白胡须、亮灰毛
    mn = np.minimum(np.minimum(r, g), b)
    protect = (mn > BRIGHT_PROTECT) & (greenness < 0.30)
    alpha[protect] = 1.0

    # 稀疏白胡须（亮度中高、绿度低）也保护
    mid_protect = (mn > 0.60) & (greenness < 0.16)
    alpha[mid_protect] = 1.0

    return alpha, greenness


def despill(rgb, alpha):
    """把绿色溢出压到 max(R,B)，只作用于可能溢色的区域。"""
    r, g, b = rgb[..., 0].copy(), rgb[..., 1].copy(), rgb[..., 2].copy()
    cap = np.maximum(r, b)
    spill = g > cap
    g[spill] = cap[spill]
    return np.stack([r, g, b], axis=-1)


def decontaminate(rgb, alpha):
    """颜色去污染：半透明像素的 RGB 往往混了绿幕的暗色，去绿后会变成黑边。
    这里把每个半透明像素的颜色替换为最近的"实心"像素颜色（通常是猫体或胡须本体），
    使边缘在浅色背景上不再显脏。
    """
    solid = alpha > 0.95
    if not solid.any():
        return rgb
    _, (iy, ix) = ndimage.distance_transform_edt(~solid, return_indices=True)
    near = rgb[iy, ix]
    out = rgb.copy()
    mask = (~solid) & (alpha > 0.01)
    out[mask] = near[mask]
    return out


def clean_mask(alpha):
    solid = alpha > 0.5

    # 去掉孤立小噪点（保留所有面积够大的块）
    lab, n = ndimage.label(solid)
    if n > 0:
        sizes = ndimage.sum(solid, lab, range(1, n + 1))
        keep = np.zeros(n + 1, dtype=bool)
        for i, s in enumerate(sizes, start=1):
            if s >= MIN_BLOB:
                keep[i] = True
        solid = keep[lab]

    # 填内部洞（眼睛、口鼻阴影）
    solid = ndimage.binary_fill_holes(solid)

    # 轻微闭运算修补边缘缺口
    solid = ndimage.binary_closing(solid, structure=np.ones((3, 3)))

    return solid


def fast_bbox(name):
    """只求前景外接框，跳过羽化与合成，用于第一遍并集统计。"""
    im = Image.open(os.path.join(RAW, name)).convert("RGB")
    rgb = np.asarray(im).astype(np.float32) / 255.0
    alpha, _ = green_alpha(rgb)
    solid = alpha > 0.5
    ys, xs = np.nonzero(solid)
    if not len(xs):
        return (0, 0, 1, 1)
    return (int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1)


def process(name, bbox=None, size=384, debug_bg=(255, 0, 255)):
    path = os.path.join(RAW, name)
    im = Image.open(path).convert("RGB")
    rgb = np.asarray(im).astype(np.float32) / 255.0

    alpha_raw, _ = green_alpha(rgb)
    solid = clean_mask(alpha_raw)

    # 软边：重新用羽化后的 alpha，但限定在 solid 外扩范围内
    soft = ndimage.gaussian_filter(alpha_raw.astype(np.float32), FEATHER_SIGMA)
    alpha = np.where(solid, np.maximum(soft, 0.92), soft * 0.55)
    alpha = np.clip(alpha, 0.0, 1.0)

    rgb2 = despill(rgb, alpha)
    rgb2 = decontaminate(rgb2, alpha)
    rgba = np.concatenate([rgb2, alpha[..., None]], axis=-1)

    if bbox is None:
        ys, xs = np.nonzero(solid)
        bbox = (int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1)

    return rgba, bbox, (rgb, alpha_raw, solid)


def crop_resize(rgba, bbox, size):
    x0, y0, x1, y1 = bbox
    w, h = x1 - x0, y1 - y0
    side = max(w, h)
    cx, cy = (x0 + x1) / 2.0, (y0 + y1) / 2.0
    # 以最长边为正方形边长，围绕中心取框，保证所有帧同尺度
    fx0, fy0 = int(round(cx - side / 2)), int(round(cy - side / 2))
    fx1, fy1 = fx0 + side, fy0 + side

    canvas = np.zeros((side, side, 4), dtype=np.float32)
    sx0, sy0 = max(0, fx0), max(0, fy0)
    sx1, sy1 = min(rgba.shape[1], fx1), min(rgba.shape[0], fy1)
    dx0, dy0 = sx0 - fx0, sy0 - fy0
    canvas[dy0:dy0 + (sy1 - sy0), dx0:dx0 + (sx1 - sx0)] = rgba[sy0:sy1, sx0:sx1]

    img = Image.fromarray((canvas * 255).round().astype(np.uint8), "RGBA")
    return img.resize((size, size), Image.LANCZOS)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--debug", action="store_true")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--size", type=int, default=384)
    ap.add_argument("--raw", default="raw", help="build 下的原始帧目录名")
    ap.add_argument("--out", default="extracted_frames", help="输出目录（相对项目根）")
    args = ap.parse_args()

    global RAW, OUT_FRAMES
    RAW = os.path.join(ROOT, "build", args.raw)
    OUT_FRAMES = os.path.join(ROOT, args.out)

    names = sorted(os.path.basename(p) for p in glob.glob(os.path.join(RAW, "*.png")))
    os.makedirs(OUT_FRAMES, exist_ok=True)

    if args.debug:
        step = max(1, len(names) // 11)
        picks = [names[i] for i in range(0, len(names), step)][:12]
        # 统一 bbox：用全部帧的并集（这里先用采样帧估算）
        ux0 = uy0 = 10 ** 9
        ux1 = uy1 = -1
        cache = {}
        for n in picks:
            rgba, bb, aux = process(n)
            cache[n] = (rgba, aux)
            ux0, uy0 = min(ux0, bb[0]), min(uy0, bb[1])
            ux1, uy1 = max(ux1, bb[2]), max(uy1, bb[3])
        union = (ux0, uy0, ux1, uy1)
        print("sample union bbox", union, "size", (ux1 - ux0, uy1 - uy0))

        cell = 240
        cols = 6
        rows = (len(picks) + 1 + cols - 1) // cols
        sheet = Image.new("RGB", (cell * cols, cell * rows), (30, 30, 30))
        draw = ImageDraw.Draw(sheet)
        for i, n in enumerate(picks):
            rgba, (rgb, alpha_raw, solid) = cache[n]
            out = crop_resize(rgba, union, cell)
            bg = Image.new("RGBA", out.size, (255, 0, 255, 255))
            comp = Image.alpha_composite(bg, out).convert("RGB")
            x, y = (i % cols) * cell, (i // cols) * cell
            sheet.paste(comp, (x, y))
            draw.text((x + 4, y + 4), n, fill=(255, 255, 0))
            draw.text((x + 4, y + cell - 14), f"a_mean={alpha_raw.mean():.2f}", fill=(255, 255, 0))
        out_path = os.path.join(BUILD, "chk_key.png")
        sheet.save(out_path)
        print("wrote", out_path)

        # 单独存一张全分辨率合成图，便于放大看边缘
        rgba, _, _ = process(names[48])
        big = crop_resize(rgba, union, 720)
        bg = Image.new("RGBA", big.size, (255, 0, 255, 255))
        Image.alpha_composite(bg, big).convert("RGB").save(os.path.join(BUILD, "chk_key_single.png"))
        Image.alpha_composite(bg, big).split()[-1].save(os.path.join(BUILD, "chk_key_alpha.png"))
        big.save(os.path.join(BUILD, "chk_key_rgba.png"))
        print("wrote single-frame checks")
        return

    if args.all:
        # 第一遍：算并集 bbox
        ux0 = uy0 = 10 ** 9
        ux1 = uy1 = -1
        boxes = {}
        for n in names:
            bb = fast_bbox(n)
            boxes[n] = bb
            ux0, uy0 = min(ux0, bb[0]), min(uy0, bb[1])
            ux1, uy1 = max(ux1, bb[2]), max(uy1, bb[3])
        union = (ux0, uy0, ux1, uy1)
        print("union bbox", union, "size", (ux1 - ux0, uy1 - uy0))

        meta = []
        for i, n in enumerate(names):
            rgba, _, _ = process(n, bbox=union)
            img = crop_resize(rgba, union, args.size)
            img.save(os.path.join(OUT_FRAMES, f"{i:04d}.png"))
            ys, xs = np.nonzero(np.asarray(img)[..., 3] > 8)
            meta.append({"i": i, "src": n, "cx": float(xs.mean()), "cy": float(ys.mean()) if len(ys) else 0.0})
            if i % 40 == 0:
                print(f"  {i}/{len(names)}")

        with open(os.path.join(BUILD, "frames_meta.json"), "w", encoding="utf-8") as f:
            json.dump({"union_bbox": union, "size": args.size, "frames": meta}, f, ensure_ascii=False, indent=1)
        print(f"done -> {OUT_FRAMES} ({len(names)} frames, {args.size}px)")
        return

    print("nothing to do: pass --debug or --all")


if __name__ == "__main__":
    main()
