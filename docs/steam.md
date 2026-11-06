# Steam 发布与运行说明

本页面向使用者和维护 Steam 构建的开发者。Steam 版沿用桌面版的本地数据、皮肤和插件边界；Steam 运行时、云存档或启动参数异常时，先退出应用并保留日志，再按[日志说明](logging.md)导出诊断包。

## 用户侧

1. 在 Steam 库中安装并启动 NanaType。
2. 首次启动完成后，在设置中选择语言、皮肤和数据目录；不要把令牌、私密台词或日志上传到公共工单。
3. 更新或验证文件后重新启动；若窗口黑屏，先禁用高级动效和离屏输出，再重试。
4. 报告问题时附 NanaType 版本、系统、GPU、复现步骤与脱敏日志，不要附 API key、Cookie、访问令牌或完整聊天记录。

Steam 输入、覆盖层和启动器不会改变插件权限。插件仍在独立 worker 中运行；包只能使用声明的 data/pipeline 能力，不能任意访问网络或文件。

## 构建与验证

```powershell
cargo build -p nanatype --profile dist --locked
cargo test --workspace --all-targets --locked
cargo run -p nanatype --release --example snapshots -- target/steam-snapshots zh-CN
```

Windows 发布包使用 `dist` profile。通过 `nana-packager` 生成便携包，再用 Inno Setup 生成安装器；若策略阻止 Inno Setup，记录 `blocked by policy` 并保留便携包，不要绕过组织安全策略。安装器验证应覆盖首次启动、升级、卸载、非 ASCII 用户目录、无网络启动、GPU 后备和 Steam Overlay。

## 发布检查

发布前固定 commit、Rust/toolchain、资源清单和版本号；校验 `.ntpkg` 签名与 BLAKE3、内置字体许可、默认配置和崩溃日志脱敏。验证 Steam 启动参数、工作目录和用户数据迁移；不要把开发机绝对路径写入包。构建失败或签名不匹配时停止发布并保留原始日志。
