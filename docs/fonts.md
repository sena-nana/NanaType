[简体中文](fonts.md) · [English](en/fonts.md) · [日本語](ja/fonts.md)

# 内置皮肤字体

内置字体让像素、水墨等皮肤在未安装字体的电脑上仍保持设计。当前随附 Fusion Pixel、霞鹜文楷（LXGW WenKai）和 Tangerine Bold；来源、版本、SHA-256、子集命令及许可全文见 [字体目录 README](../crates/theme/fonts/README.md)。

## 使用规则

- 字体文件放在 `crates/theme/fonts/`，旁边必须有同名 `<名字>.LICENSE.txt`；缺许可会导致编译失败。
- 皮肤通过 `file = "builtin:fonts/<名字>.ttf"` 引用，路径不得离开 `crates/theme/`。
- `build.rs` 在 `builtin-fonts` 开启时用 `include_bytes!` 嵌入字体和许可；`--no-default-features` 可完全不嵌入。
- 选中皮肤时才向 NanaUI 注册，使用字体文件自己的家族名，并按 `families` 作为首选回落；未随附的字体回落到已安装字体或 NanaUI 的 Noto Sans SC。

```bash
cargo build --no-default-features -p nanatype
```

体积预算可由 `NANATYPE_BUILTIN_FONTS_BUDGET_KB=<KiB>` 设置，默认 12288 KiB。当前三款字体合计约 3.66 MB。像素字体按 12px 整数倍显示最清楚；`pixel` 与 `rpg` 文字使用 24px、行高 1.5，测试会检查这一点。

## 加入字体

1. 从官方 Release 获取字体，核对允许随软件分发的许可、保留字体名（RFN）、版本和 SHA-256。
2. 用 fontTools 的 `pyftsubset` 按项目字表子集化；保留全部 `name` 记录、OpenType 特性与许可要求的版权信息。将字体和同名许可放入 `crates/theme/fonts/`，并在字体 README 与 `THIRD_PARTY_NOTICES.md` 记录来源。
3. 在皮肤中增加 `builtin:fonts/...` 引用，保留原 `families` 回落链。
4. 运行测试：

```bash
cargo test -p nanatype-theme
```

OFL 1.1 第 2 条要求每份副本携带版权与许可全文；关于页和安装包的 `licenses/` 会从嵌入文件展示或打包。含 RFN 的字体不得直接沿用保留名；不能确认许可或不能合法再分发的字体不随附（例如非 OFL 的 Zpix）。
