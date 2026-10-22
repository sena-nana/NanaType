"""Development tool: WCAG 2.x contrast of the redesign palettes in docs/skin-redesign.md.

    python scripts/assets/skin-redesign-contrast.py > docs/skin-redesign/contrast.md

Each entry is (label, foreground, background, minimum). Translucent backgrounds are
composited over the skin's face colour first. Overlay skins without a face (subtitle,
sonnet) are measured against their outline or glow and noted as such.
"""

FACE = "face"

SKINS = {
    "halo": ("#FFFFFF", [
        ("正文", "#1B2F4B", FACE, 4.5), ("名牌", "#FFFFFF", "#087FD1", 3.0),
        ("引用", "#5A6B85", FACE, 4.5), ("提及", "#0B5FA8", "#0876D01F", 4.5)]),
    "vanilla": ("#FBF8F1", [
        ("正文", "#24201B", FACE, 4.5), ("名牌", "#8A6A3A", FACE, 3.0),
        ("引用", "#6E655A", FACE, 4.5), ("提及", "#5B4128", "#EFE6D2", 4.5)]),
    "livechat": ("#FFFFFF", [
        ("正文", "#1F2A36", FACE, 4.5), ("名牌", "#137C9A", "#EEF7FB", 3.0),
        ("引用", "#5F6E80", FACE, 4.5), ("提及", "#106F8A", "#23ADE524", 4.5),
        ("LIVE", "#FFFFFF", "#F5506F", 3.0)]),
    "night": ("#120B1C", [
        ("正文", "#F3E9FF", FACE, 4.5), ("名牌", "#FFE3F4", FACE, 3.0),
        ("引用", "#B9A6CF", FACE, 4.5), ("提及", "#FFC2E6", "#FF4FB824", 4.5)]),
    "terminal": ("#0D0A05", [
        ("正文", "#FFC547", FACE, 4.5), ("名牌", "#FFB000", FACE, 3.0),
        ("引用", "#A88536", FACE, 4.5), ("提及", "#FFD27A", "#FFB00024", 4.5)]),
    "machine_learning": ("#15171C", [
        ("正文", "#E8ECF2", FACE, 4.5), ("名牌", "#A99BFF", FACE, 3.0),
        ("引用", "#7C8394", FACE, 4.5), ("提及", "#CFC7FF", "#A99BFF1F", 4.5)]),
    "adv": ("#0D0D12", [
        ("正文", "#FFFFFF", FACE, 4.5), ("名牌", "#FFFFFF", FACE, 3.0),
        ("引用", "#B8BCC6", FACE, 4.5), ("提及", "#FFFFFF", "#FFFFFF1F", 4.5)]),
    "rpg": ("#2A3FB8", [
        ("正文", "#FFFFFF", FACE, 4.5), ("名牌", "#FFFFFF", "#10206E", 3.0),
        ("引用", "#C6D0FF", FACE, 4.5), ("提及", "#FFFFFF", "#FFFFFF26", 4.5)]),
    "pixel": ("#9BBC0F", [
        ("正文", "#0F380F", FACE, 4.5), ("名牌", "#9BBC0F", "#0F380F", 3.0),
        ("引用", "#0F380F", FACE, 4.5), ("提及", "#0F380F", "#8BAC0F", 4.5)]),
    "comic": ("#FFFFFF", [
        ("正文", "#111111", FACE, 4.5), ("名牌", "#111111", "#FFD400", 3.0),
        ("引用", "#555555", FACE, 4.5), ("提及", "#111111", "#EDEDED", 4.5)]),
    "bubble": ("#FFE8F1", [
        ("正文", "#4A2440", FACE, 4.5), ("名牌", "#5A1E42", "#FF7AA8", 3.0),
        ("引用", "#86566F", FACE, 4.5), ("提及", "#7A2E59", "#FFD6E6", 4.5)]),
    "subtitle": ("#000000", [
        ("正文（对描边）", "#FFFFFF", FACE, 4.5), ("名牌（对描边）", "#FFD27F", FACE, 3.0),
        ("引用（对描边）", "#E6E6E6", FACE, 4.5)]),
    "sonnet": ("#211B25", [
        ("正文（对光晕底）", "#FFF8EA", FACE, 4.5), ("名牌（对光晕底）", "#E5BF83", FACE, 3.0),
        ("引用（对光晕底）", "#CDB89A", FACE, 4.5)]),
    "parchment": ("#F1DFBC", [
        ("正文", "#3A2617", FACE, 4.5), ("名牌", "#FFF1D6", "#5C341C", 3.0),
        ("引用", "#7A5A3C", FACE, 4.5), ("提及", "#4A2E17", "#E3C99A", 4.5)]),
    "ink": ("#FBFAF6", [
        ("正文", "#1E1E1E", FACE, 4.5), ("名牌", "#1E1E1E", FACE, 3.0),
        ("引用", "#6E6A62", FACE, 4.5), ("提及", "#8E241F", "#B0302A14", 4.5)]),
    "crayon": ("#FFF9EC", [
        ("正文", "#51413A", FACE, 4.5), ("名牌", "#A65B49", FACE, 3.0),
        ("引用", "#6E6058", FACE, 4.5), ("提及", "#8A4A3C", "#B46A5824", 4.5)]),
    "love_letter": ("#FFFBF2", [
        ("正文", "#4E2A33", FACE, 4.5), ("名牌", "#7E1F35", FACE, 3.0),
        ("引用", "#7E5F68", FACE, 4.5), ("提及", "#7E1F35", "#7E1F3514", 4.5)]),
    "forest": ("#EEF0E2", [
        ("正文", "#22382B", FACE, 4.5), ("名牌", "#2E4A36", "#B8CDA4B3", 3.0),
        ("引用", "#536957", FACE, 4.5), ("提及", "#2E4A36", "#DDE5CF", 4.5)]),
    "sakura": ("#FFF2F6", [
        ("正文", "#6B2141", FACE, 4.5), ("名牌", "#C2416F", FACE, 3.0),
        ("引用", "#8E4C68", FACE, 4.5), ("提及", "#8E2A55", "#F8D3E1", 4.5)]),
    "gothic": ("#2A1420", [
        ("正文", "#F3E6EC", FACE, 4.5), ("名牌", "#F3E6EC", "#17111A", 3.0),
        ("引用", "#B79AA8", FACE, 4.5), ("提及", "#F3C6D3", "#B3224A33", 4.5)]),
    "celestial": ("#2A1E52", [
        ("正文", "#F4EEFF", FACE, 4.5), ("名牌", "#E7CC94", "#1B1638", 3.0),
        ("引用", "#A99CC8", FACE, 4.5), ("提及", "#E7CC94", "#C9A35A26", 4.5)]),
    "tidal": ("#0E3A44", [
        ("正文", "#E5F8F5", FACE, 4.5), ("名牌", "#0E3A44", "#EEF9F6", 3.0),
        ("引用", "#8FC1BD", FACE, 4.5), ("提及", "#BDF0EB", "#79D4CE26", 4.5)]),
    "lantern": ("#24140F", [
        ("正文", "#F6E7D4", FACE, 4.5), ("名牌", "#FFF1D8", "#B83B38", 3.0),
        ("引用", "#C7A88C", FACE, 4.5), ("提及", "#F2CE8A", "#D9A84E26", 4.5)]),
    "citrus": ("#FFF6D6", [
        ("正文", "#4A3720", FACE, 4.5), ("名牌", "#5A3B12", "#FFC93C", 3.0),
        ("引用", "#7A6243", FACE, 4.5), ("提及", "#2F6E5C", "#7CC4AE33", 4.5)]),
}


def rgba(hex_):
    h = hex_.lstrip("#")
    r, g, b = (int(h[i:i + 2], 16) / 255 for i in (0, 2, 4))
    a = int(h[6:8], 16) / 255 if len(h) == 8 else 1.0
    return r, g, b, a


def over(top, below):
    r, g, b, a = top
    br, bg, bb, _ = below
    return r * a + br * (1 - a), g * a + bg * (1 - a), b * a + bb * (1 - a), 1.0


def luminance(c):
    def lin(v):
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    r, g, b, _ = c
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)


def contrast(fg, bg):
    hi, lo = sorted((luminance(fg), luminance(bg)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


def main():
    print("# 重设计配色对比度\n")
    print("由 `scripts/assets/skin-redesign-contrast.py` 生成。正文、引用、提及按 4.5:1，名牌按大字 3:1；"
          "半透明底先叠到皮肤面色上再计算。Echo-Live 四套沿用原作配色，不列入。\n")
    print("| 皮肤 | 项 | 前景 | 背景 | 对比度 | 要求 | 结果 |")
    print("|---|---|---|---|---:|---:|---|")
    failures = []
    for skin, (face_hex, rows) in SKINS.items():
        face = rgba(face_hex)
        for label, fg_hex, bg_hex, need in rows:
            bg = face if bg_hex == FACE else over(rgba(bg_hex), face)
            ratio = contrast(over(rgba(fg_hex), bg), bg)
            ok = ratio >= need
            if not ok:
                failures.append(f"{skin} {label}")
            shown_bg = face_hex if bg_hex == FACE else bg_hex
            print(f"| {skin} | {label} | `{fg_hex}` | `{shown_bg}` | {ratio:.2f} | {need:g} | {'通过' if ok else '**不足**'} |")
    print()
    print("未通过：" + ("、".join(failures) if failures else "无"))


if __name__ == "__main__":
    main()
