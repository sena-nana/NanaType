"""Summarize real GPU measurements and the fixed-clock capture invariants.

python scripts/report-classic-skin-validation.py <gpu-perf.json> <motion-root>
"""
from pathlib import Path
import json
import sys
import tomllib

root = Path(__file__).resolve().parents[1]
raw = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
motion = Path(sys.argv[2])
names = ['night', 'rpg', 'bubble', 'livechat', 'terminal', 'pixel', 'sakura']
cases = {c['scenario']: c for c in raw['cases']}
assert len(cases) == len(names)*2
assert all(not c['shader_errors'] and not c['shader_cache']['pending'] for c in cases.values())
checks = {n: json.loads((motion/f'nanatype.{n}/motion-check.json').read_text()) for n in names}
assert all(c[k] for c in checks.values() for k in
           ['reading_stable', 'alpha_stable', 'loop_exact', 'cache_stable', 'event_reading_stable'])
assert all(checks[n]['resting_status_stable'] for n in ['rpg', 'livechat', 'terminal'])

def maximum(case, group, field='p95_ms'):
    values = [t[group][field] for t in case['trials'] if t[group]]
    return max(values) if values else 0.

def per_frame(case, field):
    return sum(t[field] for t in case['trials']) / sum(t['rendered_targets'] for t in case['trials'])

lines = [
    '# 经典皮肤验证记录', '',
    f"测量设备：{raw['adapter']}，{raw['backend']}。每例三轮，各 60 帧预热、120 帧测量；960×540，GPU 时间戳，不包含截图回读。表中 P95 取三轮中最大值，属于同机绘制成本比较。静态例也强制绘制每帧；实际关闭 WGSL 的闲置界面不会因此持续调度。", '',
    '| 主题 | 静态 GPU P95 ms | 动效 GPU P95 ms | 动效 CPU P95 ms | 静态/动效绘制次数每帧 | 动效上传字节每帧 |',
    '| --- | ---: | ---: | ---: | ---: | ---: |',
]
for name in names:
    static, animated = cases[f'shader_{name}_static'], cases[f'shader_{name}']
    manifest = tomllib.loads((root/f'crates/theme/builtin/{name}.toml').read_text(encoding='utf-8'))
    title = manifest['meta']['name']['zh-CN']
    lines.append(f"| {title} | {maximum(static,'gpu_timestamp'):.4f} | {maximum(animated,'gpu_timestamp'):.4f} | {maximum(animated,'cpu_record_submit'):.4f} | {per_frame(static,'draw_calls'):.1f}/{per_frame(animated,'draw_calls'):.1f} | {per_frame(animated,'upload_bytes'):.0f} |")

peak = max(c['shader_cache']['bytes'] for c in cases.values())/(1024*1024)
shaped = sum(t['text_shaped'] for c in cases.values() for t in c['trials'])
layouts = sum(t['layout_nodes'] for c in cases.values() for t in c['trials'])
lines += ['', f'顺序切换全部七套时，程序、遮罩与效果纹理缓存峰值 {peak:.2f} MiB。所有编译错误列表为空；测量帧中文字塑形总计 {shaped} 次，布局节点更新总计 {layouts} 个。每套循环及事件采样前后，程序数、遮罩数与缓存字节数保持不变；上传统计保留原始值，176 字节是每次效果绘制的固定 uniform，不是纹理重传。', '',
          f"七套均逐像素检查完整 {checks['night']['period_ms']/1000:.0f} 秒周期、每套 {checks['night']['frames']} 帧。正文及回复引用区不变、透明度不变、周期首尾完全相同；另检查开句和换说话人在 0、125、250、500、1000、2000 ms 的阅读区域。另外十一套使用静态设计，不携带运行时 WGSL，也不请求自定义效果帧；羊皮纸等图案由离线烘焙工具生成。直播弹幕按打字状态、RPG 按等待状态取样；另逐像素确认直播完成、RPG 非等待和终端等待状态在时钟推进后保持静止。", '',
          '原始数据：[GPU 测量](classic-skin-perf.json)、[循环检查](classic-skin-motion-checks.json)。设计与重建步骤见[十八套设计记录](classic-skin-refinement.md)。该渲染样例不测量 OBS 接收端、跨进程同步或设备驱动故障隔离。', '']
(root/'docs/classic-skin-validation.md').write_text('\n'.join(lines), encoding='utf-8')
(root/'docs/classic-skin-perf.json').write_text(json.dumps(raw, indent=2)+'\n', encoding='utf-8')
print(f'{len(cases)} cases, {peak:.2f} MiB peak, {shaped} shaped, {layouts} layout nodes')
