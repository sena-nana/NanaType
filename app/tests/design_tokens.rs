//! Application chrome is built from the design system in `app/src/ui/kit`:
//! views name a role (`Type::Label`, `gap::CONTROL`, `RadiusTier::Sm`,
//! `kit::cta`) rather than a number, so two windows that mean the same thing
//! cannot drift apart. This scans the chrome views for the literals that
//! would bypass it.
//!
//! Skin-drawn content (the dialogue box, the history, quotes, effects) is
//! styled by the skin and is not scanned, nor is the kit itself.
use std::path::{Path, PathBuf};

/// Files whose look comes from the skin, or that draw no chrome.
const SKIN_DRAWN: &[&str] = &[
    "ui/kit/",
    "ui/dialogue.rs",
    "ui/history.rs",
    "ui/quote.rs",
    "ui/surface.rs",
    "ui/surface_painter.rs",
    "ui/materials.rs",
    "ui/rich.rs",
    "ui/overflow.rs",
    "ui/effect_uniforms.rs",
    "ui/effects.rs",
    "ui/shader_effects.rs",
    "ui/text_light.rs",
    "ui/text_scramble.rs",
    "ui/sweep.rs",
    "ui/handwriting.rs",
    "ui/motion.rs",
    "ui/pipeline.rs",
    "ui/link_text.rs",
    "ui/settings/pages/skin_grid.rs",
];

/// Calls whose first argument must be a token, not a number.
const TOKEN_ARGS: &[&str] = &[
    "font_size = Some(",
    ".font_size(",
    "text_style(",
    "mono_style(",
    "one_line(",
    "Stack::row(",
    "Stack::column(",
    "Stack::fill_row(",
    "Stack::fill_column(",
    "padding_xy(",
    ".padding(",
    "IconGlyph::new(",
];

/// Spellings that bypass the kit outright.
const FORBIDDEN: &[&str] = &[
    "radius_px(",
    "border_radius = Some(",
    "ButtonKind::Primary",
    "Rgba8::rgb(",
    "Rgba8::rgba(",
    "box_shadows",
];

/// Whether `s` starts with a non-zero numeric literal (`8.0`, `12.`, `4`).
fn nonzero_literal(s: &str) -> bool {
    let s = s.trim_start();
    let lit: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '_')
        .collect();
    !lit.is_empty() && lit.chars().any(|c| c.is_ascii_digit() && c != '0')
}

/// An `[0.5, 0.55, 0.62, 0.28]`-style colour literal.
fn rgba_literal(line: &str) -> bool {
    line.match_indices('[').any(|(i, _)| {
        let rest = &line[i + 1..];
        let Some(end) = rest.find(']') else {
            return false;
        };
        let parts: Vec<&str> = rest[..end].split(',').map(str::trim).collect();
        parts.len() == 4
            && parts.iter().all(|p| {
                !p.is_empty()
                    && p.contains('.')
                    && p.chars().all(|c| c.is_ascii_digit() || c == '.')
            })
    })
}

fn violations(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (n, line) in code.lines().enumerate() {
        // The unit tests at the foot of a file build fixtures, not chrome.
        if line.starts_with("#[cfg(test)]") {
            break;
        }
        let trimmed = line.trim_start();
        // Comments, and values that are data rather than style (a chroma
        // key's default colour), marked as such.
        if trimmed.starts_with("//") || line.contains("// design-tokens: data") {
            continue;
        }
        for call in TOKEN_ARGS {
            for (i, _) in line.match_indices(call) {
                let args = &line[i + call.len()..];
                // `IconGlyph::new(x).size(16.0)` names the size after the icon.
                let args = if *call == "IconGlyph::new(" {
                    match args.find(".size(") {
                        Some(j) => &args[j + ".size(".len()..],
                        None => continue,
                    }
                } else {
                    args
                };
                let bad = if *call == "padding_xy(" {
                    args.split(')')
                        .next()
                        .unwrap_or("")
                        .split(',')
                        .any(nonzero_literal)
                } else {
                    nonzero_literal(args)
                };
                if bad {
                    out.push(format!("{}: {}", n + 1, trimmed));
                }
            }
        }
        for word in FORBIDDEN {
            if line.contains(word) {
                out.push(format!("{}: {}", n + 1, trimmed));
            }
        }
        if rgba_literal(line) {
            out.push(format!("{}: {}", n + 1, trimmed));
        }
    }
    out
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn chrome_views_use_the_design_tokens() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src.join("ui"), &mut files);
    files.sort();
    let mut report = String::new();
    for path in files {
        let rel = path
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if SKIN_DRAWN.iter().any(|s| rel.starts_with(s)) {
            continue;
        }
        let found = violations(&std::fs::read_to_string(&path).unwrap());
        if !found.is_empty() {
            report.push_str(&format!("\n{rel}:\n"));
            for v in found {
                report.push_str(&format!("  {v}\n"));
            }
        }
    }
    assert!(
        report.is_empty(),
        "chrome bypasses the design system (use the roles in app/src/ui/kit):{report}"
    );
}

#[test]
fn the_scan_catches_what_it_should() {
    for bad in [
        "Stack::row(8.0)",
        "text_style(13.0, 600)",
        "x.padding_xy(space::MD, 12.0)",
        ".radius_px(6.0)",
        "IconGlyph::new(ti::X).size(16.0)",
        "color: [0.5, 0.55, 0.62, 0.28],",
        "Button::new(x).kind(ButtonKind::Primary)",
    ] {
        assert_eq!(violations(bad).len(), 1, "{bad}");
    }
    for good in [
        "Stack::row(0.0)",
        "Stack::column(gap::CONTROL)",
        "x.padding_xy(inset::CARD, 0.0)",
        "IconGlyph::new(ti::X).size(icon::MD)",
        "Type::Label.style()",
    ] {
        assert!(violations(good).is_empty(), "{good}");
    }
}
