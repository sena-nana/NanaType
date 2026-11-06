[简体中文](markup.md) · [English](en/markup.md) · [日本語](ja/markup.md)

# 台词标记速查

标记写在输入框编辑区，工具栏和侧栏可以自动插入。解析不会失败：未知标签按文字显示并在状态栏提示；`[[` 表示字面量 `[`。成对标记可嵌套。

## 样式与动效

| 用途 | 标记 |
| --- | --- |
| 粗体、斜体、下划线、删除线 | `[b]…[/b]` `[i]…[/i]` `[u]…[/u]` `[s]…[/s]`；`[b=0]` 取消默认粗体 |
| 颜色、字号、对齐、字体 | `[color=#e86a92]…[/color]`；`[size=1.5]` 或 `[size=32px]`；`[align=center]`；`[font=accent]` 或 `[font="Noto Serif SC"]` |
| 描边、阴影、注音 | `[stroke=#fff,2]`；`[shadow=#000a,2,2,4]`；`[ruby=かんじ]漢字[/ruby]` |
| 持续动效 | `[shake]`、`[wave]`、`[jump]`、`[rainbow]`、`[pulse]`、`[flicker]`；可加 `amp=`、`speed=` |
| 出场动效 | `[fade]`、`[pop=120]`、`[slide]`；整句只取覆盖字数最多的一种 |
| 速度与出现 | `[speed=0.5]`、`[speed=40ms]`、`[instant]` |
| 节奏 | `[em]`、`[wait=500]`、`[pause]`、`[pause auto=2000]`、`[page]` |

持续效果可以组合；`amp=0` 关闭单项，减少动态效果会关闭全部持续效果。长文本溢出由皮肤 `overflow` 决定：`scroll`（默认）、`page` 或 `clip`。

## 音效、画面与联动

`[se=coin vol=0.6]` 播放皮肤音效；`[expr=smile]` 联动立绘及 VTube Studio/OSC/WebSocket；`[portrait=happy]` 只换立绘；`[screenshake=6,300]` 震屏；`[sticker=cats/hello h=1.6]`、`[emote=wave]` 插入贴纸或皮肤表情；`[img="file:C:/pic.png" alt="图"]` 插图；`[at=1234 name="观众" avatar=blob:bili-face-1234]` 插入观众提及。

VTube Studio 标记：`[vts hotkey=挥手]`、`[vts expr=Smile.exp3.json]`、`[vts expr=Smile.exp3.json active=off fade=0.3]`、`[vts param=MouthSmile value=0.8 ms=1500]`。标记在打字到对应位置时发送；Skip 会立即补发跳过部分。联动设置还可启用口型同步和句末复位。

## 句首设置与模板

```text
[@auto=1500 after=fade delay=800 per=20]
```

`auto` 是自动推进延时；`after` 为 `hold`、`fade` 或 `clear`；`speaker` 可写角色名或 1 起始的角色序号。`[@speaker="Guest Star"]` 适用于含空格的名称。快捷短语支持 `{{name}}` 和 `{{time}}`。

“跟随皮肤”可清除覆盖，也可使用 `[size=inherit]…[/size]`、`[font=inherit]…[/font]`。像素字号范围为 4–400px，倍率为 0.5–3×。工具栏只处理文字外观；控制侧栏负责节奏、画面、音效和表情芯片。
