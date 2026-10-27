# Steam 版

Steam 版与 GitHub 发布版共用同一套代码，用 Cargo feature 区分：

| | GitHub 发布版（默认） | Steam 版 |
| --- | --- | --- |
| 构建 | `cargo build -p nanatype` | `cargo build -p nanatype --no-default-features --features builtin-fonts,steam` |
| 更新 | 检查 GitHub Release（`updates`，见 [检查更新](updates.md)） | 由 SteamPipe 更新，不含更新检查代码 |
| 弹幕来源 | Twitch、第三方弹幕 | Twitch、第三方弹幕 |
| 创意工坊 | — | 皮肤页、播放页访问已订阅的皮肤与音效包 |

`steam` 不能与 `updates` 同时开启（`app/src/lib.rs` 的 `compile_error!`）。`scripts/check-product-split.sh` 和 CI 会检查 Steam 版不链接 `nanatype-update`，其他构建也不链接 Steamworks。

## 连接来源

Twitch 和 blivechat/彗星号在两个版本中都可用。Steam 版不包含 GitHub 更新检查代码；blivechat 的房间号和身份码模式均保留。

## 与 GitHub 版共享配置

两版使用同一个应用 ID（`dev.sena-nana.nanatype`）和 bundle ID，设置、包目录、缓存、日志与凭据位置相同。Steam 包使用 nana-packager 的 `steam` 后端，不写便携标记，因此用户数据不会落到 Steam 安装目录。两版共用单实例锁，不会同时运行。

- 开机自启写同一个启动项（Windows `HKCU\…\Run\NanaType`、macOS LaunchAgent），以最后设置的那一版为准。
- 创意工坊内容不复制进 `packages/`。选中工坊皮肤或音效包后打开 GitHub 版，会暂时用默认皮肤或皮肤音效，但不改写设置；回到 Steam 版即恢复。基于工坊皮肤「复制」出的皮肤会把工坊皮肤打包进自己的 `dependencies/`，两版都能使用。

## Steam ID 与本地配置

App ID、depot ID 不写进仓库：

1. 复制 `.cargo/config.example.toml` 为 `.cargo/config.toml`（已在 `.gitignore` 中），填入 `[env]`：
   - `NANATYPE_STEAM_APP_ID`：Steam App ID。Cargo 构建时传给 `crates/steam/build.rs` 编译进程序。
   - `NANATYPE_STEAM_DEPOT_WINDOWS`、`NANATYPE_STEAM_DEPOT_MACOS`：打包脚本写入 depot 构建脚本。
   - 可选 `NANATYPE_STEAM_TAG_SKIN`、`NANATYPE_STEAM_TAG_SOUNDS`：创意工坊浏览按钮使用的标签，默认 `Skin`、`Sound Pack`。
2. 也可以直接设成环境变量，CI 中用 secrets 提供。

链接 Steam 的 release 构建缺少 App ID 时直接报错；debug 构建会退回 Valve 的测试应用 Spacewar（480），并给出警告。程序通过 `Client::init_app` 指定 App ID，不需要 `steam_appid.txt`，打包时也不放这个文件。

## 开发与调试

- `cargo run -p nanatype --no-default-features --features builtin-fonts,steam` 默认不连接 Steam，创意工坊显示为不可用；设置 `NANATYPE_STEAM=1` 并保持 Steam 客户端运行时才会连接。
- Release 版从 Steam 外启动时会通过 Steam 重新启动自己（`restart_through_steam`，在单实例锁之前），Steam 未运行时照常启动，只是创意工坊不可用。
- 打包校验的自检运行（`NANA_PACKAGE_VALIDATE`）不会连接 Steam，也不会重启。

## 创意工坊

- 已订阅、已安装的条目作为只读外部包目录交给包注册表（`SkinRegistry::set_external`），条目文件夹根目录必须是一个完整的包（`package.toml` 在最外层）。
- 包 ID 冲突时，内置包优先，其次是用户安装的包，最后是条目 ID 较小的工坊条目；被遮蔽的条目在订阅列表中显示原因。
- 订阅列表显示下载进度、等待下载、未下载、无法使用和冲突状态，可打开条目页面、重新下载或取消订阅。取消订阅后条目立即从列表和注册表中移除，Steam 在应用退出后删除文件；如果它正被选为皮肤或音效包，会恢复默认。
- 工坊包不能导出或删除，包管理页中「删除」改为「取消订阅」。Lua 功能包仍需要逐个审核同意。
- 「浏览创意工坊」优先在 Steam 覆盖层中打开对应标签的页面；覆盖层关闭时改用 `steam://openurl/` 交给 Steam 客户端。覆盖层会挂到所有窗口，包括透明展示窗口与 Spout/Syphon 输出，正式发布前需实测是否会带进直播画面，必要时在 Steamworks 后台关闭覆盖层。

### 发布条目

在 NanaType Editor 中导出 `.ntpkg` 并解压到一个文件夹，或直接使用包目录，用 SteamCMD 的 `workshop_build_item` 上传，内容根目录就是包目录。皮肤加标签 `Skin`，音效包加标签 `Sound Pack`（或你在本地配置中设置的标签）。

## 打包

需要 `nana-packager`（同 [发布流程](updates.md)）以及 `.cargo/config.toml` 中的 ID。

```bash
pwsh scripts/package.ps1 -Steam
```

```bash
scripts/package-macos.sh --steam
```

- 两个脚本都用 Steam feature 构建，把 `packaging/steam/nana-package.steam.toml` 填好 ID 后写到 `target/steam/nana-package.toml`，再打包和校验；不生成安装包、zip 或 dmg。
- Windows 把 steamworks-sys 附带的 `steam_api64.dll` 放在 `NanaType.exe` 旁。
- macOS 的 `libsteam_api.dylib` 放进 `Contents/Frameworks`，并改写可执行文件的加载路径后重新 ad-hoc 签名，因为包内的 `Contents/MacOS` 只允许有可执行文件。
- 许可文件由 `third-party-notices.ps1 -Steam` 和 `macos-licenses.py … nanatype-steam` 生成，只列 Steam 版实际链接的 crate。
- 产物在 `dist/app/`，depot 构建脚本和文件清单在 `dist/steam/`（`depot_build_<depot>.vdf`、`nana-steam-build.json`），用 SteamCMD 上传。

上传凭据只由 SteamCMD 管理，仓库和脚本都不读取。
