[简体中文](obs-spout.md) · [English](en/obs-spout.md) · [日本語](ja/obs-spout.md)

# OBS 输出

NanaType 可以把展示框、联机伙伴和历史记录分别送进 OBS。Windows 推荐 **Spout2**，macOS 推荐 **Syphon**；也可以直接捕获透明窗口。

完整的首次设置步骤见[使用指南 › 接入 OBS](usage.md#接入-obs)。本页主要处理多路输出、性能设置和常见问题。

## Windows：使用 Spout2

1. 在 NanaType 打开“设置 › 输出 / OBS › Spout2 输出”。
2. 在 OBS 安装 Spout2 插件，添加 *Spout2 Capture* 来源。
3. 在来源中选择发送端，并把合成模式设为 *Premultiplied Alpha*。
4. 在 NanaType 发送一条台词，确认 OBS 画面出现。

可用发送端名称：

| 画面 | 发送端 |
| --- | --- |
| 自己的展示框 | `NanaType Display` |
| 联机伙伴 | `NanaType Partner 1` 至 `NanaType Partner 7` |
| 共享历史 | `NanaType History` |

伙伴输出按座位工作：谁坐在某个座位，谁的画面就从对应发送端输出；座位空着时不会输出。名称必须互不重复。

## macOS：使用 Syphon

1. 在 NanaType 打开“设置 › 输出 / OBS › Syphon 输出”。
2. 在 OBS 添加 *Syphon Client* 来源。
3. 选择对应的 `NanaType Display`、`NanaType Partner …` 或 `NanaType History` 发送端。
4. 合成模式使用 *Premultiplied Alpha*。

也可以使用 macOS 的屏幕捕获捕获展示框窗口。Spout2 仅适用于 Windows。

## 直接捕获窗口

不使用共享纹理时，在 OBS 添加 *Window Capture*：

- Windows：捕获方式选择 *Windows 10 (1903 and up)*。
- macOS：使用 *macOS Screen Capture*。

窗口捕获会受到窗口位置、系统缩放和其他窗口遮挡影响；直播时优先使用 Spout2 或 Syphon。

## 关闭、重开与切换输出

- 关闭输出只会停止对应发送端，NanaType 的展示和播放不会停止。
- 关闭后在短时间内使用相同名称、尺寸和透明度重新打开，通常可以快速恢复 OBS 画面。
- 修改发送端名称、尺寸或透明度后，OBS 需要重新选择来源。
- 关闭较长时间后，发送端会重新创建，OBS 可能短暂显示空白；如果没有恢复，请在 OBS 中重新选择发送端。
- 隐藏 NanaType 窗口不会停止已经开启的共享输出。

## 与游戏同时运行

在 Windows 的“设置 › 输出 › 性能”中，可以按需开启：

- **提高 GPU 优先级**：提高整个进程的 GPU 调度优先级。系统不允许时会回退到普通优先级，并显示“未生效”。
- **提高 CPU 优先级**：只提高绘制和发送画面的线程，不改变聊天、下载和解码线程。
- **CPU 亲和性**：在有大小核的设备上让画面线程优先使用性能核；没有大小核时显示“无需设置”。

三项默认关闭，重启后会重新应用。设置中的“运行 GPU”可以选择 NanaType 使用的显卡：

- 默认使用“自动”。
- NanaType 和 OBS 使用同一块显卡时，共享纹理开销通常更低。
- 修改显卡选择后需要重启应用。
- 选择的显卡不存在时会回退到自动选择；系统环境变量 `WGPU_ADAPTER_NAME` 已设置时，以环境变量为准。

## 排查无画面或黑屏

1. 确认 NanaType 中对应输出已打开。
2. 确认 OBS 使用了正确的插件来源和发送端名称。
3. 将合成模式改为 *Premultiplied Alpha*。
4. 确认发送端名称没有被其他程序占用。
5. 关闭并重新打开该输出；如果仍无画面，在 OBS 中删除并重新添加来源。
6. 多显卡设备上，确认 NanaType 与 OBS 使用同一块 GPU。

若仍无法恢复，请先用窗口捕获确认 NanaType 本身是否正常显示，再查看[日志](logging.md)中的诊断信息。
