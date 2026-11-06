# NanaType 文档

按你要完成的事情选择入口。大多数用户只需要阅读 **快速上手** 和 **常用功能**。

## 快速上手

1. [下载与启动](../README.md)：安装、首次打开和第一次发送台词。
2. [使用指南](usage.md)：输入、播放、窗口、历史记录和配置方案。
3. [接入 OBS](obs-spout.md)：把展示框、历史记录或联机伙伴送进直播画面。
4. [台词标记](markup.md)：为文字加入停顿、动效、音效和角色联动。

## 常用功能

| 我想…… | 阅读 |
| --- | --- |
| 使用语音输入 | [语音输入](voice-input.md) |
| 读取或发送 Twitch 聊天 | [Twitch](twitch.md) |
| 接入 blivechat、彗星号或 LAPLACE | [彗星号](comet.md) · [LAPLACE](laplace.md) |
| 和其他主播联机 | [使用指南：联机](usage.md#联机) |
| 使用 Steam 版 | [Steam 版](steam.md) |
| 安装皮肤 | [使用指南：表情包与皮肤](usage.md#表情包与皮肤) |
| 制作皮肤（高级参考） | [皮肤格式](skins.md) |
| 导入 Telegram 或本地表情包 | [使用指南：表情包与皮肤](usage.md#表情包与皮肤) |
| 查看、导出或清理历史记录 | [使用指南：历史记录与配置方案](usage.md#历史记录与配置方案) |
| 处理更新失败 | [检查更新](updates.md) |

## 制作者

- [NanaType Editor 包编辑器](package-editor.md)：创建、预览、签名和调试包。
- [皮肤格式](skins.md)：制作皮肤、音效包和响应式布局。
- [插件与包接口](plugins.md)：Lua 插件、声明式数据和安全边界。
- [字体](fonts.md)：安装与引用字体。

## 开发者

- [从源码构建](building.md)：开发环境、测试和打包。
- [日志](logging.md)：收集诊断信息。
- [渲染管线](rendering-pipeline.md)：窗口与 GPU 输出的实现说明。

## 设计与项目记录

以下内容面向贡献者，用于记录设计决策、性能验证和阶段计划，不是日常使用说明：

- [统一包整体设计](package-design.md) · [分阶段计划](package-roadmap.md)
- [皮肤重设计](skin-redesign.md) · [皮肤艺术方向](skin-art-direction.md)
- [经典皮肤细化记录](classic-skin-refinement.md) · [皮肤素材来源](skin-art-sources.md)
- [渲染优化验证记录](rendering-performance.md) · [发布配置性能](release-profile-performance.md)
- [插件性能、预算与隔离](plugin-performance.md) · [界面规则](ui-rules.md)
- [情书皮肤实现记录](love-letter.md)

## 语言

- 简体中文：当前目录
- [English](en/README.md)
- [日本語](ja/README.md)

如果你第一次使用 NanaType，建议从 [使用指南](usage.md) 开始；如果你要改代码，直接看 [从源码构建](building.md)。
