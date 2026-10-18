# 内置皮肤的字体

内置皮肤只写系统字体名时，没装那些字体的电脑会用普通界面字体代替：像素、水墨这几套就失去了味道。这里说明程序带了哪些开源字体、怎样编进程序，以及以后加字体的规矩。

> **状态**：随附 Fusion Pixel、霞鹜文楷与 Tangerine Bold 三款字体。前两款于 2026-10-13 从官方 GitHub Release 获取，Tangerine 于 2026-10-18 从 Google Fonts 获取；许可、来源、SHA-256 与子集记录见 [crates/theme/fonts/README.md](../crates/theme/fonts/README.md)。

## 机制

- 字体放在 `crates/theme/fonts/<名字>.ttf`（或 `.otf`），旁边必须有同名许可 `<名字>.LICENSE.txt`（上游的 `OFL.txt` 原样复制，含版权行）。缺许可时编译失败。
- 皮肤写 `file = "builtin:fonts/<名字>.ttf"`。用户皮肤也能引用。路径不能离开 `crates/theme/`。
- `build.rs` 扫描该目录，打开 Cargo 特性 `builtin-fonts`（应用默认打开）时用 `include_bytes!` 嵌入字体与许可，否则什么都不嵌入。
- 选中皮肤时才向 NanaUI 注册（`app/src/skin_fonts.rs`）；正文、名牌、装饰三个角色或几套皮肤共用同一个字体时只注册一次，切到不用它的皮肤时注销。注册时用的是字体文件自己的家族名（见下表），排在 `families` 最前。
- 本次构建没带的字体不报错，直接回落到 `families`；`fonts/` 里根本没有的名字给一条警告（多半是拼错）。
- 皮肤库只在 `families` 里没有一个可用字体（已安装，或是 NanaUI 自带的 Noto Sans SC），而且也没有随附可用的字体文件时，才给皮肤标上「替代字体」。

### 体积开关

| 方式 | 作用 |
| --- | --- |
| `cargo build --no-default-features -p nanatype` | 不嵌入任何内置字体 |
| `NANATYPE_BUILTIN_FONTS_BUDGET_KB=<KiB>` | 体积上限，默认 12288（12 MiB）。超出时编译失败 |

嵌入是逐字节的，程序增大的体积就是 `fonts/` 里字体文件的总大小（不压缩；安装包的压缩会省一部分）。现在两款字体合计 3,595,708 字节（约 3.4 MiB）；调试版 `snapshots` 示例开、关 `builtin-fonts` 相差 3,621,376 字节（字体加许可全文与少量代码）。

## 随附的字体

“十四行诗”另随附 Tangerine Bold（Version 1.3，65,464 字节，Toshi Omagari，OFL-1.1），用于默认英文花体，中文复用霞鹜文楷。2026-10-18 从 Google Fonts 获取完整字体，未修改或子集化；校验和与许可记录见字体目录 README。三款随附字体合计 3,661,172 字节。新增可选 `[fonts.latin]` 角色，字段与其他字体角色相同，仅用于未指定字体的拉丁文本段；显式字体及个人字体设置优先。

| 皮肤 | 字体（注册的家族名） | 版本 | 作者 | 许可 | 上游文件 | 子集 | 文件 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `pixel`、`rpg`（正文、名牌） | Fusion Pixel 12px Proportional 简体（`Fusion Pixel 12px Prop zh-Hans`） | 2026.09.25 | TakWolf | OFL-1.1，无保留字体名 | `fusion-pixel-12px-proportional-zh_hans.ttf`，7,053,672 字节，36,436 字形 | GB2312 一二级 + 界面用字 + 假名与标点：7,983 字形，1,453,472 字节 | `FusionPixel12pxProportionalSC.ttf` |
| `ink`（正文、名牌、装饰） | 霞鹜文楷 Regular（`LXGW WenKai`） | 1.522 | LXGW（基于 Fontworks 的 Klee One） | OFL-1.1，无保留字体名 | `LXGWWenKai-Regular.ttf`，25,575,676 字节，46,867 字形 | GB2312 一级（3,755 常用字）+ 界面用字 + 假名与标点：5,479 字形，2,142,236 字节 | `LXGWWenKai-Regular.ttf` |
| `echo_*` | Noto Sans SC（NanaUI 自带） | — | — | OFL-1.1 | — | — | 不需要文件：`families` 末尾加了 `"Noto Sans SC"`，前面的字体都没装时用它 |

下载的原始文件与 SHA-256：

| 文件 | 来源 | 大小 | SHA-256 |
| --- | --- | --- | --- |
| `fusion-pixel-font-12px-proportional-ttf-v2026.09.25.zip` | <https://github.com/TakWolf/fusion-pixel-font/releases/tag/2026.09.25> | 49,414,580 | `47585b9c4b6d8d57380e9dec75bc7e8043fdbfa77d1843c3aaa19a1e7bd15d50` |
| └ `fusion-pixel-12px-proportional-zh_hans.ttf` | 上面压缩包内 | 7,053,672 | `b2ee68647dc257fa697e4d0c78f9b59461bc504a023b9a3b5ad66f358bbb1613` |
| `LXGWWenKai-Regular.ttf` | <https://github.com/lxgw/LxgwWenKai/releases/tag/v1.522> | 25,575,676 | `39ad71264b588165b469e35e6afb162a378dacd1f95348160240ba9038ac3009` |
| `OFL.txt`（文楷，tag v1.522） | <https://github.com/lxgw/LxgwWenKai/blob/v1.522/OFL.txt> | 4,448 | `c38b1994a5e48ac30ac7d1da7d0409fd8fd8127dfe28a13d6e787d5b1ef34a5e` |
| `LICENSE-OFL`（Fusion Pixel，tag 2026.09.25） | <https://github.com/TakWolf/fusion-pixel-font/blob/2026.09.25/LICENSE-OFL> | 4,418 | `bc518cf64b8032c07690f33cc270c35c179255a6ac8efa7c165ebae7e8f76a63`（与压缩包里的 `OFL.txt` 相同） |

Fusion Pixel 的字形部分来自 Ark Pixel（OFL-1.1，TakWolf）、Cubic 11（OFL-1.1，含 JF Dot M+H 12 与 M+ BITMAP FONTS 的自由许可声明）和 Galmuri（OFL-1.1，Lee Minseo）。压缩包里这三份许可与 Fusion Pixel 自己的 `OFL.txt` 一起原样放进了 `FusionPixel12pxProportionalSC.LICENSE.txt`。

皮肤的 `families` 没改，仍是回落链：不带字体的构建（`--no-default-features`）与以前完全一样。像素字体按 12 px 的整数倍显示最清楚，字号不是 12 的倍数时笔画边缘会发虚：`pixel` 与 `rpg` 的正文、名牌、历史文字（含弹幕行）都用 24 px，行高 1.5（36 px），`pixel_font_skins_stay_on_the_12px_grid` 测试检查这一点。

### 暂不随附

| 皮肤 | 字体 | 原因 |
| --- | --- | --- |
| `parchment` | Noto Serif SC | 需要 600 与 700 两个字重，子集后每个约 1.5–3 MB；SimSun / 宋体是可以接受的回落 |
| `terminal` | Cascadia Mono | Windows 11 自带，Consolas / Menlo 回落已足够；其许可保留 “Cascadia Code” 名，子集须改名 |
| — | Zpix（最像素） | 非 OFL 的自定义许可，随软件分发需另行授权；已由 Fusion Pixel 替代 |

## 子集

用 fontTools（`pip install fonttools`）的 `pyftsubset`，命令与字表的生成方法见 [crates/theme/fonts/README.md](../crates/theme/fonts/README.md)。要点：

- 保留全部 name 记录：OFL 要求版权与许可声明随字体保留，家族名也要原样留着，皮肤才对得上。
- 字表：GB2312（像素字体用一二级，文楷用一级 3,755 字代替《通用规范汉字表》的 3,500 字）+ 扫描 `*.ftl`、内置皮肤、`app/` 源码得到的全部非 ASCII 字符 + 假名、CJK 标点、全角字符等几个区段。
- 去掉 hinting 与竖排度量（`vhea`/`vmtx`），保留全部 OpenType 特性（两款字体的 GSUB/GPOS 都不到 10 KB）。
- 超出子集的字会逐字回落到系统字体，台词里偶尔夹一个宋体/黑体字，这是体积换来的代价。

**保留字体名（RFN）**：OFL 第 3 条规定，字体声明了保留字体名时，修改版（子集也算）不能再用这个名字。子集前先看上游 `OFL.txt` 版权行有没有 “with Reserved Font Name …”，再看字体 name 表的版权（ID 0）与许可（ID 13）记录：

- Fusion Pixel 2026.09.25、Ark Pixel、Cubic 11、Galmuri、霞鹜文楷 1.522：都**没有**声明保留字体名（已核实许可文件与 name 表），子集沿用原名。
- 有保留名时：要么不子集，要么确认保留名不在家族名里，否则改家族名并同步修改皮肤的 `families`。例：NanaUI 带的 Noto Sans SC 2.004 保留的是 “Source”，“Noto Sans SC” 不受影响；Cascadia 保留 “Cascadia Code”。

## 随附的署名与许可

OFL 1.1 第 2 条要求每份副本都带上版权声明与许可全文。本仓库的做法：

- `crates/theme/fonts/<名字>.LICENSE.txt`：上游许可原样复制（构建强制要求），随字体一起编进程序（`BuiltinFont::license`）。
- **关于页**：「开源字体」一行，点「查看许可」展开一个可滚动的面板，逐个列出本次构建嵌入的字体：家族名、作者、版本、许可名与许可全文（都从嵌入的字体与许可读出，不带字体的构建不显示这一行）。
- **安装包**：`packaging/nanatype.iss` 把 `crates/theme/fonts/*.LICENSE.txt`、`packaging/licenses/`（NanaUI 自带 Noto Sans SC 的 `NotoSansSC-OFL.txt`）、仓库根目录的 `THIRD_PARTY_NOTICES.md` 与打包时生成的 `rust-crates.txt`（各 Rust 依赖自己的许可文件）装到 `{app}\licenses\`；`scripts/package.ps1 -Portable` 的压缩包里也有同样的 `licenses/` 文件夹。
- 仓库根目录的 [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) 汇总署名：字体、`dxcompiler.dll` 与全部 Rust 依赖（后者由 `pwsh scripts/third-party-notices.ps1` 从 `cargo metadata --offline` 生成，CI 用 `-Check` 检查它是否过期、每个依赖是否都有许可表达式）。

## 加入一个字体的步骤

1. 从官方 Release 下载，核对许可（OFL-1.1 或同样允许随软件分发的许可）与保留字体名，记下版本与 SHA-256。
2. 按上面的方法子集，放进 `crates/theme/fonts/`，旁边放 `<名字>.LICENSE.txt`；在 `crates/theme/fonts/README.md` 记下来源、版本、SHA-256、子集命令与日期，在 `THIRD_PARTY_NOTICES.md` 加上署名。
3. 皮肤里写 `file = "builtin:fonts/<名字>.ttf"`，`families` 保持不变作为回落。
4. `cargo test -p nanatype-theme`：测试会检查每个嵌入的字体能解析、带许可、合计不超过预算，每个内置皮肤的 `builtin:` 引用都在 `fonts/` 里。
