//! Tag value grammar shared by the parser and the serializer.

use nanatype_model::{
    AfterLine, Continuous, ContinuousKind, Emphasis, Entrance, EntranceKind, FontSlot, Pace, Rgba8,
    Shadow, Size, Stroke,
};

/// `[name=value key=value]` after the brackets are stripped.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tag {
    pub name: String,
    pub close: bool,
    pub value: Option<String>,
    pub args: Vec<(String, String)>,
}

impl Tag {
    pub fn arg(&self, key: &str) -> Option<&str> {
        self.args
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Parse the inside of `[...]`. `None` means this is not tag syntax at all.
pub(crate) fn parse_tag(inner: &str) -> Option<Tag> {
    let mut rest = inner.trim_start();
    let close = rest.starts_with('/');
    if close {
        rest = &rest[1..];
    }
    let name_len = rest
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '@' || *c == '_' || *c == '-'))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if name_len == 0 {
        return None;
    }
    let name = rest[..name_len].to_ascii_lowercase();
    rest = &rest[name_len..];
    let mut value = None;
    if let Some(after) = rest.strip_prefix('=') {
        let (v, tail) = take_value(after)?;
        value = Some(v);
        rest = tail;
    }
    let mut args = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let key_len = rest
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-'))
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        if key_len == 0 {
            return None;
        }
        let key = rest[..key_len].to_ascii_lowercase();
        rest = &rest[key_len..];
        let after = rest.strip_prefix('=')?;
        let (v, tail) = take_value(after)?;
        args.push((key, v));
        rest = tail;
    }
    if close && (value.is_some() || !args.is_empty()) {
        return None;
    }
    Some(Tag {
        name,
        close,
        value,
        args,
    })
}

fn take_value(input: &str) -> Option<(String, &str)> {
    if let Some(quoted) = input.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = quoted.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => out.push(chars.next()?.1),
                '"' => return Some((out, &quoted[i + 1..])),
                c => out.push(c),
            }
        }
        None
    } else {
        let end = input
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(input.len());
        Some((input[..end].to_owned(), &input[end..]))
    }
}

/// Quote a value when it would not survive [`take_value`] bare.
pub(crate) fn quote(value: &str) -> String {
    let bare = !value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, ']' | '"' | '[' | '\\'));
    if bare {
        value.to_owned()
    } else {
        let mut out = String::from("\"");
        for c in value.chars() {
            if matches!(c, '"' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    }
}

pub(crate) fn num(text: &str) -> Option<f32> {
    let v: f32 = text.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

pub(crate) fn ms(text: &str) -> Option<u32> {
    text.trim().trim_end_matches("ms").parse().ok()
}

/// Shortest decimal that parses back to the same `f32`.
pub(crate) fn fmt_num(v: f32) -> String {
    let mut s = format!("{v}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

pub(crate) fn flag(value: Option<&str>) -> Option<bool> {
    match value {
        None => Some(true),
        Some("1" | "on" | "true" | "yes") => Some(true),
        Some("0" | "off" | "false" | "no") => Some(false),
        Some(_) => None,
    }
}

pub(crate) fn size(text: &str) -> Option<Size> {
    let t = text.trim();
    if let Some(px) = t.strip_suffix("px") {
        num(px).filter(|v| *v > 0.0).map(Size::Px)
    } else {
        num(t.trim_end_matches("em"))
            .filter(|v| *v > 0.0)
            .map(Size::Em)
    }
}

pub(crate) fn fmt_size(size: Size) -> String {
    match size {
        Size::Em(em) => fmt_num(em),
        Size::Px(px) => format!("{}px", fmt_num(px)),
    }
}

pub(crate) fn stroke(text: &str) -> Option<Stroke> {
    let mut parts = text.split(',');
    let color: Rgba8 = parts.next()?.parse().ok()?;
    let width_px = parts.next().map(num).unwrap_or(Some(2.0))?;
    if parts.next().is_some() || width_px < 0.0 {
        return None;
    }
    Some(Stroke { color, width_px })
}

pub(crate) fn fmt_stroke(s: Stroke) -> String {
    format!("{},{}", s.color, fmt_num(s.width_px))
}

pub(crate) fn shadow(text: &str) -> Option<Shadow> {
    let parts: Vec<&str> = text.split(',').collect();
    let color: Rgba8 = parts.first()?.parse().ok()?;
    let at = |i: usize, d: f32| parts.get(i).map(|p| num(p)).unwrap_or(Some(d));
    if parts.len() > 4 {
        return None;
    }
    Some(Shadow {
        color,
        dx: at(1, 2.0)?,
        dy: at(2, 2.0)?,
        blur: at(3, 0.0)?.max(0.0),
    })
}

pub(crate) fn fmt_shadow(s: Shadow) -> String {
    format!(
        "{},{},{},{}",
        s.color,
        fmt_num(s.dx),
        fmt_num(s.dy),
        fmt_num(s.blur)
    )
}

pub(crate) fn font(text: &str) -> Option<FontSlot> {
    Some(match text {
        "" => return None,
        "body" => FontSlot::Body,
        "name" => FontSlot::Name,
        "accent" => FontSlot::Accent,
        family => FontSlot::Family(family.to_owned()),
    })
}

pub(crate) fn fmt_font(f: &FontSlot) -> String {
    match f {
        FontSlot::Body => "body".into(),
        FontSlot::Name => "name".into(),
        FontSlot::Accent => "accent".into(),
        FontSlot::Family(name) => quote(name),
    }
}

pub(crate) fn continuous(tag: &Tag, kind: ContinuousKind) -> Option<Continuous> {
    let amp = match tag.arg("amp").or(tag.value.as_deref()) {
        Some(v) => num(v)?,
        None => 1.0,
    };
    let speed = match tag.arg("speed") {
        Some(v) => num(v)?,
        None => 1.0,
    };
    Some(Continuous { kind, amp, speed })
}

pub(crate) fn fmt_continuous(c: &Continuous) -> String {
    let mut out = c.kind.key().to_owned();
    if c.amp != 1.0 {
        out.push_str(&format!(" amp={}", fmt_num(c.amp)));
    }
    if c.speed != 1.0 {
        out.push_str(&format!(" speed={}", fmt_num(c.speed)));
    }
    out
}

pub(crate) const ENTRANCE_DEFAULT_MS: u16 = 160;

pub(crate) fn entrance(tag: &Tag, kind: EntranceKind) -> Option<Entrance> {
    let ms = match tag.value.as_deref().or(tag.arg("ms")) {
        Some(v) => u16::try_from(self::ms(v)?).ok()?,
        None => ENTRANCE_DEFAULT_MS,
    };
    Some(Entrance { kind, ms })
}

pub(crate) fn entrance_tag(kind: EntranceKind) -> &'static str {
    match kind {
        EntranceKind::None => "noentrance",
        EntranceKind::Fade => "fade",
        EntranceKind::Pop => "pop",
        EntranceKind::SlideUp => "slide",
    }
}

pub(crate) fn fmt_entrance(e: Entrance) -> String {
    let name = entrance_tag(e.kind);
    if e.ms == ENTRANCE_DEFAULT_MS || e.kind == EntranceKind::None {
        name.to_owned()
    } else {
        format!("{name}={}", e.ms)
    }
}

pub(crate) fn emphasis(tag: &Tag) -> Option<Emphasis> {
    let ms = match tag.value.as_deref().or(tag.arg("ms")) {
        Some(v) => u16::try_from(self::ms(v)?).ok()?,
        None => Emphasis::DEFAULT_MS,
    };
    Some(Emphasis { ms })
}

pub(crate) fn fmt_emphasis(e: Emphasis) -> String {
    if e.ms == Emphasis::DEFAULT_MS {
        "em".to_owned()
    } else {
        format!("em={}", e.ms)
    }
}

pub(crate) fn pace(text: &str) -> Option<Pace> {
    let t = text.trim();
    if let Some(fixed) = t.strip_suffix("ms") {
        return fixed.trim().parse().ok().map(Pace::FixedMs);
    }
    num(t).filter(|v| *v > 0.0).map(Pace::Scale)
}

pub(crate) fn fmt_pace(p: Pace) -> Option<String> {
    match p {
        Pace::Inherit => None,
        Pace::Scale(s) => Some(format!("speed={}", fmt_num(s))),
        Pace::FixedMs(ms) => Some(format!("speed={ms}ms")),
        Pace::Instant => Some("instant".into()),
    }
}

pub(crate) fn after(text: &str, tag: &Tag) -> Option<AfterLine> {
    let delay = tag.arg("delay").map(ms).unwrap_or(Some(0))?;
    Some(match text {
        "hold" => AfterLine::Hold,
        "fade" => AfterLine::Fade {
            delay_ms: tag.arg("delay").map(ms).unwrap_or(Some(1200))?,
            per_unit_ms: tag.arg("per").map(ms).unwrap_or(Some(40))?,
        },
        "clear" => AfterLine::Clear { delay_ms: delay },
        _ => return None,
    })
}
