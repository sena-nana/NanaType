"""Compose actual NanaUI captures, without re-drawing the skins in Python.

python scripts/render-classic-skin-preview.py [target/classic-preview]
Requires display.png in each nanatype.<id> directory. Existing new-six shots
are read from target/shader-preview-alpha to keep their accepted preview intact.
"""
from pathlib import Path
import sys
import tomllib
from PIL import Image, ImageDraw, ImageFont

root = Path(__file__).resolve().parents[1]
source = root / (sys.argv[1] if len(sys.argv) > 1 else 'target/classic-preview')
classic = ['halo', 'night', 'parchment', 'adv', 'rpg', 'vanilla', 'bubble',
           'subtitle', 'livechat', 'comic', 'terminal', 'pixel', 'sakura', 'ink',
           'echo_vanilla', 'echo_glass', 'echo_bubble', 'echo_void']
new = ['gothic', 'celestial', 'forest', 'tidal', 'lantern', 'citrus']
font = ImageFont.truetype('C:/Windows/Fonts/msyh.ttc', 22)
small = ImageFont.truetype('C:/Windows/Fonts/msyh.ttc', 19)

def compose(names, title, dest):
    board = Image.new('RGB', (1360, 58 + ((len(names)+1)//2)*226), '#EDF1F6')
    draw = ImageDraw.Draw(board)
    draw.text((22, 14), title, font=font, fill='#26364A')
    for index, name in enumerate(names):
        manifest = tomllib.loads((root/f'crates/theme/builtin/{name}.toml').read_text(encoding='utf-8'))
        x, y = 12 + index % 2 * 676, 54 + index // 2 * 226
        if index == len(names)-1 and len(names) % 2:
            x = (1360-660)//2
        draw.rounded_rectangle((x, y, x+660, y+214), 12, fill='white')
        draw.text((x+14, y+8), manifest['meta']['name']['zh-CN'], font=small, fill='#26364A')
        folder = source if (source/f'nanatype.{name}/display.png').exists() else root/'target/shader-preview-alpha'
        shot = Image.open(folder/f'nanatype.{name}/display.png').convert('RGBA')
        shot.thumbnail((634, 170), Image.Resampling.LANCZOS)
        bg = '#10131A' if manifest.get('chrome', {}).get('appearance') == 'dark' else '#EFF3F7'
        plate = Image.new('RGBA', shot.size, bg)
        plate.alpha_composite(shot)
        board.paste(plate.convert('RGB'), (x+(660-shot.width)//2, y+36+(170-shot.height)//2))
    board.save(root/dest)

compose(classic, 'NanaType · 原有十八套皮肤细化（实际渲染）', 'docs/skins-classic-preview.png')
compose(['night', 'parchment', 'rpg', 'bubble', 'comic', 'sakura'],
        'NanaType · 边框、名牌与材质细化', 'docs/skins-classic-details.png')
compose(classic+new+['crayon'], 'NanaType · 25 套内置主题', 'docs/skins-preview.png')

compose(['halo', 'parchment', 'vanilla', 'livechat', 'comic'],
        'NanaType · 五套主题定向细化（实际渲染）', 'docs/skins-classic-refinements.png')

# Show the real frame and magnified joins, without redrawing their artwork.
shot = Image.open(source/'nanatype.parchment/display.png').convert('RGBA')
detail = Image.new('RGB', (1000, 486), '#EDF1F6')
draw = ImageDraw.Draw(detail)
draw.text((22, 13), '羊皮纸 · 古籍卷叶压印边饰（实际渲染与局部放大）', font=font, fill='#26364A')
detail.paste(shot, ((1000-shot.width)//2, 48), shot)
for x, title, bounds in [
    (12, '左上 · 细茎连接框线与名牌', (18, 0, 186, 78)),
    (506, '右下 · 角部稍密，沿长边渐疏', (shot.width-170, shot.height-78, shot.width-18, shot.height-18)),
]:
    draw.rounded_rectangle((x, 250, x+482, 474), 10, fill='white')
    draw.text((x+14, 260), title, font=small, fill='#26364A')
    crop = shot.crop(bounds)
    crop = crop.resize((crop.width*2, crop.height*2), Image.Resampling.LANCZOS)
    detail.paste(crop, (x+(482-crop.width)//2, 299+(164-crop.height)//2), crop)
detail.save(root/'docs/parchment-border-preview.png')
