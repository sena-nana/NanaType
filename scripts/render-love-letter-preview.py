"""Assemble actual NanaUI captures into a still, contact sheet and motion preview."""
import argparse
from pathlib import Path
from PIL import Image, ImageDraw


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("captures", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    names = ["wide-enter-0120", "wide-enter-0240", "wide-enter-0600", "wide-stable", "wide-exit-280", "wide-exit-600"]
    sheet = Image.new("RGB", (960, len(names) * 165), "#F4EBE6")
    draw = ImageDraw.Draw(sheet)
    for row, name in enumerate(names):
        frame = Image.open(args.captures / f"{name}.png").convert("RGBA")
        frame.thumbnail((932, 140), Image.Resampling.LANCZOS)
        sheet.paste(frame, (14, row * 165 + 22), frame)
        draw.text((16, row * 165 + 5), name, fill="#784B56")
    sheet.save(args.output / "love-letter-stages.png")
    Image.open(args.captures / "wide-stable.png").save(args.output / "love-letter.png")
    paths = sorted(args.captures.glob("wide-enter-*.png")) + [args.captures / "wide-stable.png"] * 12 + sorted(args.captures.glob("wide-exit-*.png"))
    frames = []
    for path in paths:
        frame = Image.open(path).convert("RGBA")
        backdrop = Image.new("RGBA", frame.size, "#F4EBE6")
        backdrop.alpha_composite(frame)
        frames.append(backdrop.convert("RGB"))
    frames[0].save(args.output / "love-letter-motion.webp", save_all=True, append_images=frames[1:], duration=40, loop=0, lossless=True)
    frames[0].save(args.output / "love-letter-motion.gif", save_all=True, append_images=frames[1:], duration=40, loop=0, disposal=2)
    print(f"Saved {len(frames)} motion frames and still previews to {args.output}")


if __name__ == "__main__":
    main()
