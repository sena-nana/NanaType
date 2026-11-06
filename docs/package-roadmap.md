# 统一混装包分阶段计划（项目记录）

总任务是 [NanaType #4](https://github.com/sena-nana/NanaType/issues/4)，格式和安全边界见[统一包整体设计](package-design.md)。每阶段独立交付、独立验收；本页记录证据，不代表所有实验都是已发布功能。

## 阶段与验收范围

| 阶段 | Issue | 交付重点 | 状态 |
| --- | --- | --- | --- |
| 1 | [#5](https://github.com/sena-nana/NanaType/issues/5) | 元信息、授权对象、签名/完整性、风险告知与逐插件同意 | 已实现；原生窗口/安装包仍交阶段 5 |
| 2 | [#6](https://github.com/sena-nana/NanaType/issues/6) | 皮肤/混装包编辑器、IDE、隔离调试、保护与导出 | 已实现；原生 DPI、IME、文件选择器仍待验收 |
| 3 | [#7](https://github.com/sena-nana/NanaType/issues/7) | `data` 声明、预算校验、受限解析器、`nana.data.request` | 已实现；完整冷启动与原生平台仍待验收 |
| 4 | [#8](https://github.com/sena-nana/NanaType/issues/8) | `[pipelines]`、纹理/uniform、情书笔顺迁移 | 已实现；release 与原生平台仍待验收 |
| 5 | [#9](https://github.com/sena-nana/NanaType/issues/9) | 1/4/8 插件性能、隔离、异常恢复、发布矩阵 | 部分验收；Issue 仍保留后续平台项 |

共同格式始终只有一份 `package.toml`、一个 `.ntpkg` 容器和依赖命名空间；皮肤、资源、Lua 是可组合内容，仅含资源或 Shader 的包不会启动 Lua。签名验证指定内容与保护策略，但不证明现实作者身份，也不是不可绕过 DRM；同意绑定完整包及依赖闭包。

## 已完成的核心事实

- 严格解析拒绝未知键；能力声明必须和内容一致。运行中脚本、资源依赖或包内容变化会重算指纹并终止进程。
- 编辑器支持四类工程、打开/复制/导出、文件树、格式化、诊断定位、素材和数据/管线表单、真实渲染器预览、草稿/撤销；签名保护文件只读，私钥存 Windows DPAPI 或 macOS 钥匙串。
- 数据支持 `text`/`json`/`nthz` 声明、读前预算、导入完整校验、内容哈希缓存；Lua 仅可读取 `[lua].reads`，请求预算和错误回复上限见[整体设计](package-design.md)。
- `text.stroke_time_map` 管线、有限队列、过期检查、材质绑定和情书迁移已验收；旧 `dialogue.reveal.handwriting` 不再接受。
- 正式应用的审查、同意、导入导出、工作进程启停和回收已移出 UI 线程；迟到结果按会话/进程代次丢弃。三语、900/440/300px 诊断截图和 28 套皮肤快照用于离屏回归。

## 证据与限制（2026-10-20）

验证包括主题、设置、插件、Lua、应用库、同意和编辑器测试；关键日志、快照和清单位于 `target/issue*-*.log`、`target/package-editor/`、`target/package-consent/`，不随源码分发。一次文字扰动 GPU 测试只在并行运行时失败，独立和串行均通过，原因未定，稳定性继续跟踪。

Windows 已验证工作进程作业对象、单进程、256 MiB 提交上限和关闭作业终止；这不是文件/网络 OS 隔离，AppContainer 默认关闭且未实机探针。macOS Seatbelt、Linux 托盘和应用层测试分别有记录，但不替代签名、公证、安装器、原生 DPI/IME、系统文件选择器、静态 CRT、macOS 公证或 Linux 实机验收。正式签名按用户要求跳过；Inno Setup 自动下载曾被策略以 `blocked by policy` 拒绝，因此便携包校验不等于安装器验收。

性能的复现命令和固定 revision 见[插件性能、预算与隔离](plugin-performance.md)。阶段关闭前，先更新该页的状态、平台、源码指纹和已知限制，避免把单机 debug/离屏结果写成普遍性能承诺。
