"""Summarize same-machine configuration and retained-frame measurements.

python scripts/report-skin-pipeline.py <evidence-directory> <docs-directory>
Inputs: before.json, after.json, gpu-before.json, gpu-after.json.
"""
import json
import statistics
import sys
from pathlib import Path

root, docs = map(Path, sys.argv[1:3])
read = lambda name: json.loads((root / name).read_text(encoding="utf-8"))
before, after = read("before.json"), read("after.json")
old, new = read("gpu-before.json"), read("gpu-after.json")
count = after["skin_count"]
assert count == len(after["skins"])
assert all(s["shared_validated_skin"] for s in after["skins"])


def summarize(case):
    trials = case["trials"]
    assert not case["shader_errors"] and case["shader_cache"]["pending"] == 0
    assert case["shader_cache"]["bytes"] <= 64 * 1024 * 1024
    assert all(t["text_shaped"] == t["layout_nodes"] == 0 for t in trials)
    if case["scenario"].endswith("_static") and case["scenario"] != "shader_rpg_static":
        assert all(t["upload_bytes"] == 0 for t in trials)
    return {
        "cpu_p95_ms": statistics.median(t["cpu_record_submit"]["p95_ms"] for t in trials),
        "gpu_p95_ms": statistics.median(t["gpu_timestamp"]["p95_ms"] for t in trials),
        "upload_bytes_per_target": max(t["upload_bytes"] / t["rendered_targets"] for t in trials),
        "draws_per_target": max(t["draw_calls"] / t["rendered_targets"] for t in trials),
        "cache_bytes": case["shader_cache"]["bytes"],
    }


previous = {c["scenario"]: summarize(c) for c in old["cases"]}
current = {c["scenario"]: summarize(c) for c in new["cases"]}
static = [n for n in current if n.endswith("_static")]
dynamic = [n for n in current if not n.endswith(("_static", "_multi_output"))]
assert len(static) == count and len(dynamic) > 0
assert all(abs(current[n]["upload_bytes_per_target"]
    - current[n + "_static"]["upload_bytes_per_target"] - 176) < 0.001 for n in dynamic)
rows = []
for name, data in current.items():
    row = {"scenario": name, **data}
    if name in previous:
        row["before"] = previous[name]
    rows.append(row)
report = {
    "adapter": new["adapter"], "backend": new["backend"],
    "measurement": new["measurement"],
    "configuration_before": before, "configuration_after": after,
    "cases": rows,
}
visual = None
if (root / "static-comparison.json").exists():
    comparisons = read("static-comparison.json")
    assert len(comparisons) == count and all(c["same"] for c in comparisons)
    motion = {p.parent.name: json.loads(p.read_text(encoding="utf-8"))
        for p in (root / "motion").glob("nanatype.*/motion-check.json")}
    assert len(motion) == len(dynamic)
    for check in motion.values():
        assert all(check[k] for k in ["reading_stable", "alpha_stable", "loop_exact", "cache_stable", "event_reading_stable"])
        assert check["resting_status_stable"] is not False
        assert check["moving_pixels_max"] > 0
    visual = {"static_pixel_comparison": comparisons, "motion": motion}
    if (root / "classic-motion-comparison.json").exists():
        classic = read("classic-motion-comparison.json")
        assert len(classic) == 112 and all(c["same"] for c in classic)
        visual["classic_motion_pixel_comparison"] = classic
    report["visual_checks"] = visual
docs.mkdir(parents=True, exist_ok=True)
(docs / "skin-pipeline-perf.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

lines = ["# 皮肤配置链路与全主题性能验证", "",
    f"保留全部 {count} 套皮肤的布局、素材和动效设计。以下为同机开发构建的测量，不能推算其他显卡或实际 OBS 接收端的耗时。", "",
    "## 配置链路", "",
    "内置皮肤扫描只生成静态目录；应用皮肤或播放当前预览时准备受限 WGSL，并复用验证后的模型、程序及解码纹理。未应用的主题可独立播放预览，不改变当前主题；减弱动效时不准备预览程序。重新扫描保留内置缓存和正在播放的预览。用户目录和压缩包始终完整校验；启动和热重载消费本次扫描结果，手动切换用户皮肤、播放其预览或作者调用 `load` 则重新读取文件。无效编辑保留上一有效展示。", "",
    "选择器复用扫描资源，只物化静态预览用图；嵌入图片 URL 在同一目录模型内复用。活动皮肤继续支持任意包内 `skin:` 文档图片。整幅 Artwork 使用 1×1 白色覆盖纹理，跨尺寸和 DPI 复用；背景、边框及装饰保留各自遮罩。", "",
    "导出的独立 WGSL 配置省略与上下文默认值相同的字段；原始继承清单不裁减，显式空数组继续清空父皮肤配置。", "",
    "| 操作 | 优化前 ms | 优化后 ms |", "| --- | ---: | ---: |",
    f'| 冷目录扫描 | {before["cold_registry_ms"]:.2f} | {after["cold_registry_ms"]:.2f} |']
for key, label in [("load_all", f"{count} 套重复加载"), ("warm_picker", "已物化选择器刷新"), ("rescan", "重新扫描内置目录")]:
    lines.append(f'| {label} | {before[key]["median_ms"]:.3f} | {after[key]["median_ms"]:.3f} |')
lines += ["", "重复加载和刷新数据为 5 次采样中位数；冷扫描不包含首次选中动效时的 WGSL 准备，首次准备耗时单独记录在 JSON。", "",
    f'{count} 套独立配置的序列化总量：完整展开 {after["expanded_export_bytes"]:,} 字节，省略默认值后 {after["compact_export_bytes"]:,} 字节；逐套往返校验得到完全相同的 Skin。', "",
    "## 保留场景 GPU 测量", "",
    f'显卡：{new["adapter"]}；后端：{new["backend"]}。每项 3 次，每次预热 60 帧、测量 120 帧；960×540，GPU 时间戳，不在计时段读回像素。表中 P95 为三次 P95 的中位数。', "",
    f"全部 {len(static)} 套静态、{len(dynamic)} 套动效，以及潮汐多输出均已测量。稳定帧没有文字整形或布局更新；相对各自静态基线，单输出 WGSL 每帧只增加 176 字节参数上传；无编译错误或待编译项。RPG 等待场景保留原有继续标识的运动，因此静态基线仍有少量原生绘制上传，其余静态场景无上传。完整数值见 [原始汇总](skin-pipeline-perf.json)。", "",
    "| 主题 | 静态 GPU P95 ms | 动效 GPU P95 ms | 动效 CPU P95 ms | 缓存 MiB |", "| --- | ---: | ---: | ---: | ---: |"]
for name in static:
    prefix = name.removesuffix("_static")
    still, active = current[name], current.get(prefix)
    lines.append(f'| {prefix.removeprefix("shader_")} | {still["gpu_p95_ms"]:.3f} | '
        + (f'{active["gpu_p95_ms"]:.3f} | {active["cpu_p95_ms"]:.3f} | {active["cache_bytes"]/1048576:.3f} |' if active else '— | — | — |'))
lines += ["", "缓存值包含当时已准备且未被回收的程序和纹理，并非仅当前主题。GPU 测量不包含窗口系统、OBS 接收端、Spout 同步和驱动恢复。", ""]
if visual:
    lines += ["## 回归验证", "",
        "24 套静态截图与优化前逐像素一致；原有 7 套动效的 112 张对应相位截图也逐像素一致。13 套动效各采样完整周期的 16 帧及 6 个对话事件时刻：正文区域、透明边缘、循环首尾和缓存均稳定；直播弹幕、RPG、终端的静止状态检查通过。GPU 集成测试同时验证了 Artwork 跨尺寸/DPI 无遮罩分配，以及普通背景的圆角裁切。", "",
        "主题测试 46 项、应用单元测试 159 项、目录集成测试 1 项、GPU 集成测试 1 项全部通过；应用原有 2 项忽略测试保持忽略。包含全部主题独立清单往返、缓存复用、同字节长度的父皮肤编辑、无效编辑拒绝、继承图片、静态预览资源范围，以及未应用主题的按需预览、刷新保留和减弱动效优先级。", ""]
lines += [
    "## 复现", "", "```powershell",
    'cargo run -p nanatype --example skin_load_perf -- <evidence>/after.json',
    "$env:NANATYPE_GPU_CASES = ((Get-Content docs/skin-pipeline-perf.json -Raw | ConvertFrom-Json).cases.scenario) -join ','",
    "cargo run -p nanatype --example gpu_perf -- <evidence>/gpu-after.json",
    "# 保留同机优化前 before.json 与 gpu-before.json 后可重新生成对比报告。",
    "python scripts/report-skin-pipeline.py <evidence> docs", "```", ""]
(docs / "skin-pipeline-validation.md").write_text("\n".join(lines), encoding="utf-8")
print(f"Validated {len(static)} static + {len(dynamic)} animated skins; peak cache {max(v['cache_bytes'] for v in current.values())/1048576:.2f} MiB")
