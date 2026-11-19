"""从 f0000 生成托盘图标与应用图标。"""
import os

from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "extracted_frames", "0000.png")

# 384x384 帧里头部大致范围（由 union bbox 推算），裁成正方形并做圆形蒙版
HEAD_BOX = (18, 8, 296, 286)


def rounded(img, size):
    img = img.resize((size, size), Image.LANCZOS)
    mask = Image.new("L", (size * 4, size * 4), 0)
    d = ImageDraw.Draw(mask)
    d.ellipse((0, 0, size * 4 - 1, size * 4 - 1), fill=255)
    mask = mask.resize((size, size), Image.LANCZOS)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(img, (0, 0), mask)
    return out


def main():
    src = Image.open(SRC).convert("RGBA")
    head = src.crop(HEAD_BOX)

    os.makedirs(os.path.join(ROOT, "assets"), exist_ok=True)
    for s in (16, 20, 24, 32, 48, 64):
        rounded(head, s).save(os.path.join(ROOT, "assets", f"tray_{s}.png"))

    rounded(head, 256).save(
        os.path.join(ROOT, "assets", "icon.ico"),
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    rounded(head, 512).save(os.path.join(ROOT, "assets", "icon.png"))
    print("icons written to assets/")


if __name__ == "__main__":
    main()
