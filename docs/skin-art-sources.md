# 装饰图的参考与源文件

本文记录参考来源、版权边界和可重建文件。馆藏图片只用于观察结构、比例与线条；仓库装饰是重新绘制的 SVG，不裁贴、不逐像素描摹，也不随应用分发馆藏原图。

## 来源与设计落点

| 主题 | 参考 | 重新绘制的内容 |
| --- | --- | --- |
| 羊皮纸 | [Armagh 装帧样式](https://armaghrobinsonlibrary.co.uk/decorative-bookbinding-styles/)、[Huntington 装帧记录](https://hdl.huntington.org/digital/collection/p15150coll7/id/77524/)、[V&A Leman Binding](https://www.vam.ac.uk/blog/caring-for-our-collections/the-leman-binding) | 双压线、角部叶簇和连续茎；不使用照片 |
| 蔷薇哥特 | [The Met Ornamental Window](https://www.metmuseum.org/art/collection/search/466633)（Public Domain） | 十二瓣花窗、中心蔷薇、卷草和尖拱 |
| 星月魔法 | [The Met Astrolabe](https://www.metmuseum.org/art/collection/search/923639)（Public Domain） | 同心刻度、星图、轨道和悬挂环，保留月相配色 |
| 森林精灵 | [Haeckel Filicinae](https://commons.wikimedia.org/wiki/File:Haeckel_Filicinae.jpg)、[Smithsonian Maidenhair Fern](https://www.si.edu/object/maidenhair-fern%3Achndm_1939-85-71)（CC0） | 羽状小叶、叶脉、收尖叶片和卷芽，作风格化装饰 |
| 潮汐水族馆 | [Haeckel Discomedusae](https://commons.wikimedia.org/wiki/File:Haeckel_Discomedusae_18.jpg)、[Smithsonian 水母图版](https://library.si.edu/image-gallery/108984) | 薄透伞膜、放射管、伞缘垂片和错落触须 |
| 朱砂华灯 | [The Met Lantern](https://www.metmuseum.org/art/collection/search/51039)（Public Domain） | 朱砂灯体、金骨架、梅花、珠结和流苏，不复刻原器形 |
| 柠檬汽水 | [USDA Meyer 柠檬图](https://commons.wikimedia.org/wiki/File:Pomological_Watercolor_POM00006424_(cropped).jpg) | 果肉汁囊、果皮孔、叶脉和包装风格，不采用原水彩 |

查阅日期：2026-10-17。具体权利以链接的媒体记录为准。新增 `parchment/ornament.svg` 与 `sakura/ornament.svg` 为原创、CC0-1.0，无外链、字体或下载素材；原水墨馆藏授权记录保持不变。

## 文件与许可

- `crates/theme/builtin/<主题>/ornament.svg`：可编辑源，只在仓库保存；构建跳过 SVG 和点文件，不嵌入应用或导出包。
- `ornament.png`：两倍尺寸透明纹理，由受限 WGSL 的 `sample0` 读取；哥特为 1024×640 双栏，其余为 512×640。
- `artwork.wgsl`：绘制底色、框体、材质、光线并合成装饰；没有新增执行权限。
- `still.png`：同一生产 GPU 代理烘焙的静态完整画面；运行时和 `.ntpkg` 只使用 PNG。

六套仍是一个 WGSL、零作者参数、零装饰节点，沿用既有资源校验、继承、导入/导出和缓存预算。源图细节见 [skin-ornament-details.png](skin-ornament-details.png)。

## 重建

```powershell
python scripts/assets/build-skin-ornaments.py
npm install --prefix scripts/assets
node scripts/assets/render-skin-ornaments.cjs
cargo run -p nanatype --example skin_artwork -- crates/theme/builtin
python scripts/assets/package-artwork-sample.py
```

依赖固定在 `scripts/assets/package.json` 与 `scripts/assets/requirements.txt`；Python 只需标准库。重新编译后烘焙图和纹理才会进入资源。果肉细节使用固定种子，重复生成不依赖当前时间。
