use nanatype_model::{AfterLine, Document, Effects, Inline, LineMeta, Marker, TextRun, VtsAction};

use crate::value::{
    fmt_continuous, fmt_entrance, fmt_font, fmt_num, fmt_pace, fmt_shadow, fmt_size, fmt_stroke,
    quote,
};

/// One open span: the text inside `[...]` and the name that closes it.
#[derive(Debug, Clone, PartialEq)]
struct SpanTag {
    open: String,
    name: &'static str,
    effect: bool,
}

fn style_tags(run: &TextRun) -> Vec<SpanTag> {
    let s = &run.style;
    let mut tags = Vec::new();
    let mut push = |open: String, name: &'static str| {
        tags.push(SpanTag {
            open,
            name,
            effect: false,
        })
    };
    let flag = |v: bool, name: &'static str| {
        if v {
            name.to_owned()
        } else {
            format!("{name}=0")
        }
    };
    if let Some(v) = s.bold {
        push(flag(v, "b"), "b");
    }
    if let Some(v) = s.italic {
        push(flag(v, "i"), "i");
    }
    if let Some(v) = s.underline {
        push(flag(v, "u"), "u");
    }
    if let Some(v) = s.strike {
        push(flag(v, "s"), "s");
    }
    if let Some(c) = s.color {
        push(format!("color={c}"), "color");
    }
    if let Some(size) = s.size {
        push(format!("size={}", fmt_size(size)), "size");
    }
    if let Some(font) = &s.font {
        push(format!("font={}", fmt_font(font)), "font");
    }
    if let Some(stroke) = s.stroke {
        push(format!("stroke={}", fmt_stroke(stroke)), "stroke");
    }
    if let Some(shadow) = s.shadow {
        push(format!("shadow={}", fmt_shadow(shadow)), "shadow");
    }
    tags
}

fn effect_tags(effects: &Effects, pace: Option<&nanatype_model::Pace>) -> Vec<SpanTag> {
    let mut tags = Vec::new();
    for c in &effects.continuous {
        tags.push(SpanTag {
            open: fmt_continuous(c),
            name: c.kind.key(),
            effect: true,
        });
    }
    if let Some(e) = effects.entrance {
        tags.push(SpanTag {
            open: fmt_entrance(e),
            name: crate::value::entrance_tag(e.kind),
            effect: true,
        });
    }
    if let Some(open) = pace.and_then(|p| fmt_pace(*p)) {
        let name = if open == "instant" {
            "instant"
        } else {
            "speed"
        };
        tags.push(SpanTag {
            open,
            name,
            effect: true,
        });
    }
    tags
}

fn run_tags(run: &TextRun) -> Vec<SpanTag> {
    let mut tags = style_tags(run);
    tags.extend(effect_tags(&run.effects, Some(&run.pace)));
    tags
}

struct Writer {
    out: String,
    open: Vec<SpanTag>,
}

impl Writer {
    fn transition(&mut self, target: &[SpanTag]) {
        let common = self
            .open
            .iter()
            .zip(target)
            .take_while(|(a, b)| a == b)
            .count();
        while self.open.len() > common {
            let tag = self.open.pop().expect("len > common");
            self.out.push_str("[/");
            self.out.push_str(tag.name);
            self.out.push(']');
        }
        for tag in &target[common..] {
            self.out.push('[');
            self.out.push_str(&tag.open);
            self.out.push(']');
            self.open.push(tag.clone());
        }
    }

    fn text(&mut self, text: &str) {
        for c in text.chars() {
            if c == '[' {
                self.out.push_str("[[");
            } else {
                self.out.push(c);
            }
        }
    }
}

/// Canonical markup for `doc`. `parse(serialize(d)).doc == d` for any
/// normalized document.
pub fn serialize(doc: &Document) -> String {
    let mut w = Writer {
        out: String::new(),
        open: Vec::new(),
    };
    for (index, paragraph) in doc.paragraphs.iter().enumerate() {
        if index > 0 {
            w.out.push('\n');
        }
        if let Some(align) = paragraph.align {
            w.transition(&[]);
            w.out.push_str(&format!("[align={}]", align.key()));
        }
        for inline in &paragraph.inlines {
            match inline {
                Inline::Text(run) => {
                    w.transition(&run_tags(run));
                    w.text(&run.text);
                }
                Inline::Ruby(ruby) => {
                    w.transition(&run_tags(&ruby.base));
                    w.out.push_str("[ruby=");
                    w.out.push_str(&quote(&ruby.annotation));
                    w.out.push(']');
                    w.text(&ruby.base.text);
                    w.out.push_str("[/ruby]");
                }
                Inline::Atom(atom) => {
                    let mut target: Vec<SpanTag> =
                        w.open.iter().take_while(|t| !t.effect).cloned().collect();
                    target.extend(effect_tags(&atom.effects, None));
                    w.transition(&target);
                    let name = match atom.kind {
                        nanatype_model::AtomKind::Sticker => "sticker",
                        nanatype_model::AtomKind::Emote => "emote",
                        nanatype_model::AtomKind::Image => "img",
                    };
                    w.out.push('[');
                    w.out.push_str(name);
                    w.out.push('=');
                    w.out.push_str(&quote(&atom.asset.to_string()));
                    if atom.height_em != 1.0 {
                        w.out.push_str(&format!(" h={}", fmt_num(atom.height_em)));
                    }
                    if !atom.alt.is_empty() {
                        w.out.push_str(&format!(" alt={}", quote(&atom.alt)));
                    }
                    w.out.push(']');
                }
                Inline::Mention(mention) => {
                    let mut target: Vec<SpanTag> =
                        w.open.iter().take_while(|t| !t.effect).cloned().collect();
                    target.extend(effect_tags(&mention.effects, None));
                    w.transition(&target);
                    let user = &mention.user;
                    w.out.push_str("[at");
                    if !user.uid.is_empty() {
                        w.out.push('=');
                        w.out.push_str(&quote(&user.uid));
                    }
                    w.out.push_str(&format!(" name={}", quote(&user.name)));
                    if let Some(avatar) = &user.avatar {
                        w.out
                            .push_str(&format!(" avatar={}", quote(&avatar.to_string())));
                    }
                    w.out.push(']');
                }
                Inline::Marker(marker) => {
                    w.out.push('[');
                    w.out.push_str(&marker_tag(marker));
                    w.out.push(']');
                }
            }
        }
        if paragraph.align.is_some() {
            w.transition(&[]);
            w.out.push_str("[/align]");
        }
    }
    w.transition(&[]);
    w.out
}

fn marker_tag(marker: &Marker) -> String {
    match marker {
        Marker::Wait { ms } => format!("wait={ms}"),
        Marker::Pause { auto_ms: None } => "pause".into(),
        Marker::Pause { auto_ms: Some(ms) } => format!("pause auto={ms}"),
        Marker::Page => "page".into(),
        Marker::Sound { sound, gain } => {
            if *gain == 1.0 {
                format!("se={}", quote(sound))
            } else {
                format!("se={} vol={}", quote(sound), fmt_num(*gain))
            }
        }
        Marker::Expression { name, intensity } => match intensity {
            None => format!("expr={}", quote(name)),
            Some(x) => format!("expr={} intensity={}", quote(name), fmt_num(*x)),
        },
        Marker::Portrait { variant } => format!("portrait={}", quote(variant)),
        Marker::ScreenShake { amp, ms } => format!("screenshake={},{ms}", fmt_num(*amp)),
        Marker::Vts(action) => match action {
            VtsAction::Hotkey { hotkey } => format!("vts hotkey={}", quote(hotkey)),
            VtsAction::Expression { file, active, fade } => {
                let mut out = format!("vts expr={}", quote(file));
                if !active {
                    out.push_str(" active=off");
                }
                if let Some(f) = fade {
                    out.push_str(&format!(" fade={}", fmt_num(*f)));
                }
                out
            }
            VtsAction::Param { name, value, ms } => {
                let mut out = format!("vts param={} value={}", quote(name), fmt_num(*value));
                if *ms > 0 {
                    out.push_str(&format!(" ms={ms}"));
                }
                out
            }
        },
    }
}

/// Markup with a `[@...]` header carrying the line's meta, when it has any.
pub fn serialize_line(doc: &Document, meta: &LineMeta) -> String {
    let mut header = Vec::new();
    if let Some(ms) = meta.auto_advance_ms {
        header.push(format!("auto={ms}"));
    }
    match meta.after {
        None => {}
        Some(AfterLine::Hold) => header.push("after=hold".into()),
        Some(AfterLine::Fade {
            delay_ms,
            per_unit_ms,
        }) => header.push(format!("after=fade delay={delay_ms} per={per_unit_ms}")),
        Some(AfterLine::Clear { delay_ms }) => header.push(format!("after=clear delay={delay_ms}")),
    }
    let body = serialize(doc);
    if header.is_empty() {
        body
    } else {
        format!("[@ {}]{body}", header.join(" "))
    }
}
