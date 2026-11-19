"""生成带帧号的联系表（contact sheet）。

用法:
  python scripts/make_contact_sheet.py --start 0 --end 240 --step 4 --cols 10 --cell 240 --out build/cs_a.png
"""
import argparse
import os

from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def load_font(size):
    for name in ("arial.ttf", "segoeui.ttf", "msyh.ttc"):
        p = os.path.join("C:\\Windows\\Fonts", name)
        if os.path.exists(p):
            try:
                return ImageFont.truetype(p, size)
            except OSError:
                continue
    return ImageFont.load_default()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--start", type=int, default=0)
    ap.add_argument("--end", type=int, default=240)
    ap.add_argument("--step", type=int, default=1)
    ap.add_argument("--cols", type=int, default=10)
    ap.add_argument("--cell", type=int, default=240)
    ap.add_argument("--out", required=True)
    ap.add_argument("--pad", type=int, default=14, help="每格下方留给帧号的高度")
    ap.add_argument("--frames", default="", help="逗号分隔的帧号，指定后忽略 start/end/step")
    ap.add_argument("--raw", default="raw", help="build 下的子目录名")
    ap.add_argument("--maxframe", type=int, default=240)
    args = ap.parse_args()

    raw = os.path.join(ROOT, "build", args.raw)
    if args.frames:
        idxs = [int(x) for x in args.frames.split(",") if x.strip() != ""]
    else:
        idxs = list(range(args.start, min(args.end, args.maxframe) + 1, args.step))
    rows = (len(idxs) + args.cols - 1) // args.cols
    cw, ch = args.cell, args.cell + args.pad
    sheet = Image.new("RGB", (cw * args.cols, ch * rows), (24, 24, 24))
    draw = ImageDraw.Draw(sheet)
    font = load_font(max(13, args.cell // 11))

    for n, i in enumerate(idxs):
        p = os.path.join(raw, f"f_{i:04d}.png")
        if not os.path.exists(p):
            continue
        im = Image.open(p).convert("RGB").resize((args.cell, args.cell), Image.LANCZOS)
        x, y = (n % args.cols) * cw, (n // args.cols) * ch
        sheet.paste(im, (x, y))
        draw.text((x + 6, y + args.cell + 1), str(i), fill=(255, 255, 0), font=font)

    out = args.out if os.path.isabs(args.out) else os.path.join(ROOT, args.out)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    sheet.save(out)
    print(f"{out}  {sheet.size[0]}x{sheet.size[1]}  frames={len(idxs)}")


if __name__ == "__main__":
    main()
