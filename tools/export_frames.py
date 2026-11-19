"""把 extracted_frames 的 PNG 转成 WebP 供运行时加载，显著减小打包体积。

WebP q94 在这批照片素材上约为 PNG 的 24%，2 倍放大下肉眼无差异。
"""
import glob
import os

from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "extracted_frames")
DST = os.path.join(ROOT, "web", "frames")
QUALITY = 94


def main():
    os.makedirs(DST, exist_ok=True)
    # 清掉旧帧，避免帧数变化后残留
    for old in glob.glob(os.path.join(DST, "*.*")):
        os.remove(old)

    files = sorted(glob.glob(os.path.join(SRC, "*.png")))
    total = 0
    for i, p in enumerate(files):
        im = Image.open(p).convert("RGBA")
        out = os.path.join(DST, f"{i:04d}.webp")
        im.save(out, "WEBP", quality=QUALITY, method=4)
        total += os.path.getsize(out)
    print(f"exported {len(files)} webp -> {DST}  ({total / 1024 / 1024:.1f} MB)")


if __name__ == "__main__":
    main()
