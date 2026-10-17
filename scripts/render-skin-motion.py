"""Compose fixed-clock NanaUI captures without changing their motion or timing.

python scripts/render-skin-motion.py [target/skin-motion]
Requires Pillow. Capture each theme with NANATYPE_SNAPSHOT_MOTION_FRAMES=128.
"""
from pathlib import Path
import json
import sys
from PIL import Image, ImageChops, ImageDraw, ImageFont, ImageStat

root = Path(__file__).resolve().parents[1]
source = root / (sys.argv[1] if len(sys.argv) > 1 else "target/skin-motion")
names = dict(gothic="蔷薇哥特", celestial="星月魔法", forest="森林精灵",
             tidal="潮汐水族馆", lantern="朱砂华灯", citrus="柠檬汽水")
font = ImageFont.truetype("C:/Windows/Fonts/msyh.ttc", 22)
small = ImageFont.truetype("C:/Windows/Fonts/msyh.ttc", 19)


def ornament_details():
    board = Image.new("RGB", (1260, 840), "#f1f0e9")
    titles = ["花窗与窗棂", "镂空星盘", "羽片与叶脉", "伞膜与触须", "束口与流苏", "果肉与叶脉"]
    for i, (name, title) in enumerate(names.items()):
        x, y = 12 + i % 3 * 420, 10 + i // 3 * 420
        dark = name in ("gothic", "celestial", "tidal")
        draw = ImageDraw.Draw(board)
        draw.rounded_rectangle((x, y, x + 396, y + 406), 14, fill="#20292f" if dark else "#f3f0de")
        draw.text((x + 14, y + 12), title + " · " + titles[i], font=small,
                  fill="#F3F0DE" if dark else "#314C40")
        art = Image.open(root / "crates/theme/builtin" / name / "ornament.png").convert("RGBA")
        art.thumbnail((370, 340), Image.Resampling.LANCZOS)
        board.paste(art, (x + (396 - art.width) // 2, y + 52), art)
    board.save(root / "docs/skin-ornament-details.png")


ornament_details()
checks = {name: json.loads((source / f"nanatype.{name}" / "motion-check.json").read_text())
          for name in names}
frames = checks["tidal"]["frames"]
period = checks["tidal"]["period_ms"]
assert all(c["frames"] == frames and c["period_ms"] == period for c in checks.values())
assert all(c[key] for c in checks.values() for key in (
    "reading_stable", "alpha_stable", "loop_exact", "cache_stable", "event_reading_stable"))
boards = []
details = {name: [] for name in ("tidal", "forest", "lantern")}
first_shots, previous_shots = {}, {}
steps = {name: [] for name in names}


def delta(a, b):
    return round(sum(ImageStat.Stat(ImageChops.difference(a, b)).sum))


for frame in range(frames):
    board = Image.new("RGB", (1360, 736), "#edf1f6")
    draw = ImageDraw.Draw(board)
    draw.text((22, 14), "NanaType · 六套主题动效（原速 · 实际 GPU 渲染）", fill="#26364A", font=font)
    for index, (name, title) in enumerate(names.items()):
        shot = Image.open(source / f"nanatype.{name}" / f"motion-{frame:03}.png").convert("RGBA")
        rgb = shot.convert("RGB")
        if frame == 0:
            first_shots[name] = rgb
        else:
            steps[name].append(delta(previous_shots[name], rgb))
        previous_shots[name] = rgb
        if name in details:
            detail = shot.crop((shot.width - 234, 16, shot.width - 16, shot.height - 16))
            back = Image.new("RGBA", detail.size, "#10131a")
            back.alpha_composite(detail)
            # Keep pale paper behind the plant and lantern close-up previews.
            if name != "tidal":
                back = Image.new("RGBA", detail.size, "#F3F1E5" if name == "forest" else "#FAF0DD")
                back.alpha_composite(detail)
            details[name].append(back.convert("RGB").resize((436, 440), Image.Resampling.LANCZOS))
        shot.thumbnail((634, 170), Image.Resampling.LANCZOS)
        back = Image.new("RGBA", shot.size, "#10131a" if name in ("gothic", "celestial", "tidal") else
                         {"forest": "#F3F1E5", "lantern": "#FAF0DD", "citrus": "#FFF8DD"}[name])
        back.alpha_composite(shot)
        x, y = 12 + index % 2 * 676, 54 + index // 2 * 226
        draw.rounded_rectangle((x, y, x + 660, y + 214), 12, fill="white")
        draw.text((x + 14, y + 8), title, fill="#26364A", font=small)
        board.paste(back.convert("RGB"), (x + (660 - shot.width) // 2, y + 36 + (170 - shot.height) // 2))
    boards.append(board)

for name in names:
    seam = delta(previous_shots[name], first_shots[name])
    peak = max(steps[name])
    # The wrap may be an ordinary motion step, never an abrupt extra jump.
    assert seam <= peak * 2 + 512, f"{name}: abrupt wrap ({seam} versus {peak})"
    checks[name].update(seam_smooth=True, seam_rgb_delta=seam, max_step_rgb_delta=peak)


def save_gif(images, path):
    # One fixed palette prevents the stationary text and grain from flickering.
    palette = images[0].quantize(colors=256)
    indexed = [im.quantize(palette=palette, dither=Image.Dither.NONE) for im in images]
    # GIF durations are centiseconds; distribute rounding while preserving 16s.
    durations = [10 * (round((i + 1) * period / frames / 10) - round(i * period / frames / 10))
                 for i in range(frames)]
    indexed[0].save(path, save_all=True, append_images=indexed[1:], duration=durations,
                    loop=0, disposal=1, optimize=True)
    # Palette and delta-frame optimization must preserve the rendered previews.
    with Image.open(path) as saved:
        for frame in (0, frames // 4, frames - 1):
            saved.seek(frame)
            assert ImageChops.difference(saved.convert("RGB"), indexed[frame].convert("RGB")).getbbox() is None


def save_webp(images, path):
    durations = [round((i + 1) * period / frames) - round(i * period / frames) for i in range(frames)]
    images[0].save(path, save_all=True, append_images=images[1:], duration=durations,
                   loop=0, lossless=False, quality=92, method=2, minimize_size=True)
    with Image.open(path) as saved:
        for frame in (0, frames // 4, frames - 1):
            saved.seek(frame)
            error = ImageStat.Stat(ImageChops.difference(saved.convert("RGB"), images[frame])).mean
            assert max(error) < 2.5, f"preview compression changed the colours: {error}"


# WebP avoids a shared 256-colour palette; GIF remains a compatibility copy.
save_webp(boards, root / "docs/skins-motion.webp")
for name, images in details.items():
    save_webp(images, root / f"docs/{name}-motion.webp")
save_gif(boards, root / "docs/skins-motion.gif")
save_gif(details["tidal"], root / "docs/tidal-motion.gif")
(root / "docs/skin-motion-checks.json").write_text(
    json.dumps(checks, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
print("Saved WebP, GIF compatibility previews and skin-motion-checks.json")
