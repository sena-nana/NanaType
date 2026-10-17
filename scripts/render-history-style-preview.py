"""Compose native display/history captures for all 24 built-in skins.

python scripts/render-history-style-preview.py target/history-style-fixed
Render first with snapshots and NANATYPE_SNAPSHOT_VIEWS=display,history.
The checkerboard only shows transparency; skin pixels come from NanaUI.
"""
from pathlib import Path
import json
import sys
import tomllib
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / (sys.argv[1] if len(sys.argv) > 1 else 'target/history-style-fixed')
GROUPS = [
    ['halo', 'night', 'parchment', 'adv', 'rpg', 'vanilla'],
    ['bubble', 'subtitle', 'livechat', 'comic', 'terminal', 'pixel'],
    ['sakura', 'ink', 'echo_vanilla', 'echo_glass', 'echo_bubble', 'echo_void'],
    ['gothic', 'celestial', 'forest', 'tidal', 'lantern', 'citrus'],
]
FONT = ImageFont.truetype('C:/Windows/Fonts/msyh.ttc', 20)


def checkerboard(image):
    background = Image.new('RGBA', image.size)
    draw = ImageDraw.Draw(background)
    for y in range(0, image.height, 16):
        for x in range(0, image.width, 16):
            color = '#727984' if (x // 16 + y // 16) % 2 else '#808792'
            draw.rectangle((x, y, x + 15, y + 15), fill=color)
    background.alpha_composite(image)
    return background.convert('RGB')


captures = []
for number, group in enumerate(GROUPS, 1):
    board = Image.new('RGB', (1470, 1460), '#eef1f5')
    draw = ImageDraw.Draw(board)
    for index, skin in enumerate(group):
        manifest = tomllib.loads((ROOT / f'crates/theme/builtin/{skin}.toml').read_text('utf-8'))
        x, y = index % 3 * 490, index // 3 * 730
        draw.text((x + 14, y + 8), f"{manifest['meta']['name']['zh-CN']} / {skin}",
                  font=FONT, fill='#151e30')
        for view, box in [('display', (x + 12, y + 42, 466, 116)),
                          ('history', (x + 35, y + 166, 420, 560))]:
            path = SOURCE / f'nanatype.{skin}/{view}.png'
            with Image.open(path) as image:
                image.load()
                captures.append({'skin': skin, 'view': view, 'size': image.size})
                image = image.convert('RGBA')
                image.thumbnail(box[2:], Image.Resampling.LANCZOS)
                board.paste(checkerboard(image), box[:2])
    board.save(SOURCE / f'comparison-{number}.png')
(SOURCE / 'captures.json').write_text(json.dumps(captures, indent=2), encoding='utf-8')
print(f'Composed {len(captures)} native captures into 4 comparison sheets in {SOURCE}')
