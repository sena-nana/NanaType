"""Compose fixed-clock GPU captures of the restrained classic effects.

python scripts/render-classic-skin-motion.py [target/classic-motion]
Frames retain their original 16-second timing. No motion is synthesized here.
"""
from pathlib import Path
import json
import sys
import tomllib
from PIL import Image, ImageDraw, ImageFont

root = Path(__file__).resolve().parents[1]
source = root / (sys.argv[1] if len(sys.argv) > 1 else 'target/classic-motion')
names = ['night', 'rpg', 'bubble', 'livechat', 'terminal', 'pixel', 'sakura']
checks = {n: json.loads((source/f'nanatype.{n}/motion-check.json').read_text()) for n in names}
count, period = checks['night']['frames'], checks['night']['period_ms']
assert all(c['frames'] == count and c['period_ms'] == period for c in checks.values())
assert all(c[k] for c in checks.values() for k in
           ['reading_stable', 'alpha_stable', 'loop_exact', 'cache_stable', 'event_reading_stable'])
assert all(checks[n]['resting_status_stable'] for n in ['rpg', 'livechat', 'terminal'])
font = ImageFont.truetype('C:/Windows/Fonts/msyh.ttc', 22)
small = ImageFont.truetype('C:/Windows/Fonts/msyh.ttc', 18)
styles = {n: tomllib.loads((root/f'crates/theme/builtin/{n}/skin.toml').read_text(encoding='utf-8')) for n in names}
boards = []
details = []
labels = ['数据点沿电路传送', '等待时水晶跳格', '爱心挤压与回弹',
          '打字时三点弹跳', '命令忙碌符号换格', '液晶状态逐格切换', '花枝绕接点轻摆']
for index in range(count):
    board = Image.new('RGB', (1360, 58+4*190), '#EDF1F6')
    draw = ImageDraw.Draw(board)
    draw.text((22, 14), 'NanaType · 各自的物件动效（原速 · 实际 GPU 渲染）', font=font, fill='#26364A')
    for k, name in enumerate(names):
        x, y = 12 + k % 2 * 676, 54 + k // 2 * 190
        draw.rounded_rectangle((x, y, x+660, y+178), 10, fill='white')
        draw.text((x+14, y+7), tomllib.loads((root / f'crates/theme/builtin/{name}/package.toml').read_text(encoding='utf-8'))['name']['zh-CN'], font=small, fill='#26364A')
        shot = Image.open(source/f'nanatype.{name}/motion-{index:03}.png').convert('RGBA')
        shot.thumbnail((634, 140), Image.Resampling.LANCZOS)
        dark = styles[name].get('chrome', {}).get('appearance') == 'dark'
        back = Image.new('RGBA', shot.size, '#10131A' if dark else '#EFF3F7')
        back.alpha_composite(shot)
        board.paste(back.convert('RGB'), (x+(660-shot.width)//2, y+33+(140-shot.height)//2))
    boards.append(board)
    # Magnify actual pixels so the design differences remain inspectable in
    # a chat preview. These are crops of the same captures, not mock animation.
    detail = Image.new('RGB', (1360, 58+2*180), '#EDF1F6')
    draw = ImageDraw.Draw(detail)
    draw.text((22, 14), 'NanaType · 各自的动效理由（实图局部放大 · 原速）', font=font, fill='#26364A')
    for k, name in enumerate(names):
        x, y = 12 + k % 4 * 338, 54 + k // 4 * 180
        draw.rounded_rectangle((x, y, x+326, y+168), 10, fill='white')
        title = tomllib.loads((root / f'crates/theme/builtin/{name}/package.toml').read_text(encoding='utf-8'))['name']['zh-CN']
        draw.text((x+12, y+7), f'{title} · {labels[k]}', font=small, fill='#26364A')
        shot = Image.open(source/f'nanatype.{name}/motion-{index:03}.png').convert('RGBA')
        d = styles[name]['dialogue']
        inset = d.get('overscan', 18)
        mt, mr, mb, ml = d.get('box', {}).get('margin', [0, 0, 0, 0])
        width = (shot.width-inset*2-ml-mr)*d.get('box', {}).get('width_fraction', 1)
        left = inset+ml+(shot.width-inset*2-ml-mr-width)/2
        right = left+width
        if name == 'terminal':
            crop = shot.crop((int(left+6), int(inset+mt+2), int(left+86), int(inset+mt+25)))
        else:
            crop = shot.crop((int(right-148), int(inset+mt), int(right-2), int(inset+mt+40)))
        crop = crop.resize((crop.width*2, crop.height*2), Image.Resampling.NEAREST)
        bg = '#10131A' if styles[name].get('chrome', {}).get('appearance') == 'dark' else '#EFF3F7'
        back = Image.new('RGBA', crop.size, bg)
        back.alpha_composite(crop)
        detail.paste(back.convert('RGB'), (x+(326-crop.width)//2, y+49+(102-crop.height)//2))
    details.append(detail)
durations = [round((k+1)*period/count)-round(k*period/count) for k in range(count)]
boards[0].save(root/'docs/skins-classic-motion.webp', save_all=True,
               append_images=boards[1:], duration=durations, loop=0,
               lossless=False, quality=92, method=2, minimize_size=True)
details[0].save(root/'docs/skins-classic-motion-details.webp', save_all=True,
               append_images=details[1:], duration=durations, loop=0,
               lossless=False, quality=95, method=2)
(root/'docs/classic-skin-motion-checks.json').write_text(json.dumps(checks, indent=2)+'\n')
print(f'{len(names)} skins, {count} frames, {period} ms, all invariants passed')
