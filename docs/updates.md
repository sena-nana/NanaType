# 检查更新

NanaType 从 GitHub Release 获取新版本。文件可以经第三方代理下载，是否可信只看签名与摘要，与下载来源无关。

## 流程

1. 发布工作流（`.github/workflows/release.yml`，推送 `vX.Y.Z` 标签触发）打包 Windows 安装包与便携包、macOS（Apple 芯片）dmg 与 zip，生成 `latest.json`（每个文件的平台、类型、大小、SHA-256）及其 Ed25519 签名 `latest.json.sig`，创建草稿 Release。
2. 客户端（`crates/update`）同时向 GitHub、内置代理和用户填写的代理请求 `releases/latest/download/latest.json` 与签名，用内置公钥验签，取版本最新的结果，并按响应快慢排列来源。
3. 下载时边写边算 SHA-256，大小或摘要不符就换下一个来源；校验通过才交给系统打开。

客户端不访问 `api.github.com`，任何能代理 `github.com` 文件下载的服务都能用。代理返回的错误页、过期缓存或被改过的文件都会被拒绝；多个来源同时比较版本号，代理缓存的旧清单不会掩盖新版本。

## 代理

使用前缀写法：`<代理>https://github.com/sena-nana/NanaType/releases/...`。内置于 `crates/update/src/mirror.rs`，2026-10 实测（测试点不在中国大陆）：

| 代理 | Release 文件 | 备注 |
| --- | --- | --- |
| gh-proxy.com | 可用，SHA-256 与原文件一致 | Cloudflare 后端 |
| ghfast.top | 可用，一致 | |
| ghproxy.net | 可用，一致 | 适合 100 MB 以下 |
| gh.llkk.cc、mirror.ghproxy.com | 不可达 | 未收录 |
| jsDelivr | 不支持 Release 文件 | 未收录 |

它们都是个人运营的公益服务，可能限速、换域名或停服，因此只作传输通道。**设置 › 通用 › 更新 › 自定义代理** 可再加一个以 `https://` 开头的前缀，优先尝试。

## 检查与安装

开启自动检查时，启动约 10 秒后后台检查，24 小时内只查一次；发现新版本时提示，在 **设置 › 关于** 查看说明、跳过或下载。下载在缓存目录的 `updates/`，下次启动清空。

- Windows 安装版（程序目录有 `unins000.exe`）：运行 `setup.exe`，NanaType 先退出。
- Windows 便携版：打开 zip 所在文件夹，由用户解压替换。
- macOS：打开 dmg。Intel Mac 暂无构建产物。

## 签名密钥

```bash
cargo run -p nanatype-update --example keygen
```

`public` 写入 `crates/update/src/sign.rs` 的 `PUBLIC_KEY`；`secret` 存为仓库 Secret `NANATYPE_UPDATE_KEY` 并离线备份，不要提交。工作流用 `--example manifest` 生成并签名清单，密钥与公钥不匹配时失败。

轮换：旧版本只认旧公钥，先发布一个用旧密钥签名、内置新公钥的版本，再更换 Secret。私钥泄露后任何人都能签发清单，应立即轮换，并提示用户从 GitHub 手动下载新版。

## 发布

1. 修改根 `Cargo.toml` 的 `workspace.package.version` 并提交。
2. `git tag vX.Y.Z && git push origin vX.Y.Z`（标签须与版本一致）。
3. 检查草稿 Release 后发布。只有正式发布后 `releases/latest` 才指向它；带 `-` 的标签标为预发布，不推送给正式版。
