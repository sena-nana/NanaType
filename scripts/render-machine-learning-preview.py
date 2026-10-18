"""Encode the consecutive native frames from machine_learning_motion."""

import argparse
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("captures", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    timing = json.loads((args.captures / "timing.json").read_text(encoding="utf-8"))
    paths = sorted(args.captures.glob("frame-*.png"))
    assert len(paths) == timing["frames"]
    assert all(path.name == f"frame-{i:04}.png" for i, path in enumerate(paths))
    args.output.mkdir(parents=True, exist_ok=True)
    font = ImageFont.truetype("C:/Windows/Fonts/msyh.ttc", 18)
    frames = []
    for i, path in enumerate(paths):
        elapsed = i * timing["frame_ms"]
        native = Image.open(path).convert("RGBA")
        frame = Image.new("RGBA", (native.width, native.height + 44), "#192131")
        frame.alpha_composite(native, (0, 44))
        if elapsed < timing["last_input_ms"] + 140:
            label = "实时输入 · 每字 140ms 乱码收敛"
        elif elapsed < timing["exit_ms"]:
            label = "锁定完成 · 未修改的字符保持稳定"
        elif elapsed < timing["exit_ms"] + 80:
            label = "退场 · 全文同时变成乱码"
        elif elapsed < timing["exit_ms"] + 400:
            label = "随机清除 · 前后字位交错消失，空位保留"
        else:
            label = "退场完成 · 无字符残留"
        ImageDraw.Draw(frame).text((28, 12), label, font=font, fill="#B7F4F1")
        frames.append(frame.convert("RGB"))

    webp = args.output / "machine-learning-motion.webp"
    frames[0].save(
        webp,
        save_all=True,
        append_images=frames[1:],
        duration=timing["frame_ms"],
        loop=0,
        lossless=True,
        method=4,
    )
    # GIF timestamps have 10 ms precision; every second native frame is 50 ms.
    gif_frames = frames[::2]
    gif_durations = [timing["frame_ms"] * 2] * len(gif_frames)
    gif = args.output / "machine-learning-motion.gif"
    gif_frames[0].save(
        gif,
        save_all=True,
        append_images=gif_frames[1:],
        duration=gif_durations,
        loop=0,
        disposal=2,
    )

    # A contact sheet verifies actual poses without replacing the motion frames.
    samples = [
        450,
        600,
        timing["last_input_ms"] + 300,
        timing["exit_ms"],
        timing["exit_ms"] + 200,
        timing["exit_ms"] + 400,
    ]
    sheet = Image.new("RGB", (892, 276 * len(samples)), "#192131")
    for row, elapsed in enumerate(samples):
        sheet.paste(frames[elapsed // timing["frame_ms"]], (0, 276 * row))
    sheet.save(args.output / "machine-learning-motion-stages.png")
    with Image.open(webp) as encoded:
        duration = 0
        for i in range(encoded.n_frames):
            encoded.seek(i)
            encoded.load()
            duration += encoded.info["duration"]
        assert encoded.is_animated
        assert duration == len(frames) * timing["frame_ms"]
        print(f"WebP: {encoded.n_frames} encoded frames, {duration}ms, {webp.stat().st_size} bytes")
    print(f"GIF: {gif.stat().st_size} bytes; native input: {len(frames)} frames at 40fps")


if __name__ == "__main__":
    main()
