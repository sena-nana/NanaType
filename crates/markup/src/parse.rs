use std::ops::Range;

use nanatype_model::{
    AssetRef, Atom, AtomKind, ChatUser, Continuous, ContinuousKind, Document, Effects, Emphasis,
    Entrance, EntranceKind, FontSlot, Inline, LineMeta, Marker, Mention, Pace, Paragraph, Rgba8,
    RubyGroup, Shadow, Size, Stroke, Style, TextRun, VtsAction,
};

use crate::value::{self, Tag, parse_tag};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// A well-formed tag nobody knows. It stays as literal text.
    UnknownTag(String),
    /// A known tag with a value it cannot use. It stays as literal text.
    BadValue { tag: String, value: String },
    /// `[/x]` with no open `[x]`. It stays as literal text.
    UnmatchedClose(String),
    /// `[x]` still open at the end. It is closed there.
    Unclosed(String),
    /// A tag inside `[ruby]` that ruby bases cannot carry. It is ignored.
    IgnoredInRuby(String),
    /// `[@...]` after the line already started. It stays as literal text.
    MisplacedHeader,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Byte range in the source.
    pub range: Range<usize>,
    pub kind: DiagnosticKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub doc: Document,
    pub meta: LineMeta,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
enum Attr {
    Bold(bool),
    Italic(bool),
    Underline(bool),
    Strike(bool),
    Color(Rgba8),
    Size(Option<Size>),
    Stroke(Stroke),
    Shadow(Shadow),
    Font(Option<FontSlot>),
    Continuous(Continuous),
    Entrance(Entrance),
    Emphasis(Emphasis),
    Pace(Pace),
}

struct Open {
    name: String,
    attr: Attr,
    range: Range<usize>,
}

struct RubyOpen {
    annotation: String,
    base: String,
    run: TextRun,
    range: Range<usize>,
}

struct Builder<'a> {
    source: &'a str,
    open: Vec<Open>,
    ruby: Option<RubyOpen>,
    paragraphs: Vec<Paragraph>,
    current: Vec<Inline>,
    align: Option<nanatype_model::ParagraphAlign>,
    align_open: Vec<(Option<nanatype_model::ParagraphAlign>, Range<usize>)>,
    next_align: Option<Option<nanatype_model::ParagraphAlign>>,
    text: String,
    meta: LineMeta,
    started: bool,
    diagnostics: Vec<Diagnostic>,
}

/// Parse markup. Never fails; see [`Parsed::diagnostics`].
pub fn parse(source: &str) -> Parsed {
    let mut b = Builder {
        source,
        open: Vec::new(),
        ruby: None,
        paragraphs: Vec::new(),
        current: Vec::new(),
        align: None,
        align_open: Vec::new(),
        next_align: None,
        text: String::new(),
        meta: LineMeta::default(),
        started: false,
        diagnostics: Vec::new(),
    };
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if bytes.get(i + 1) == Some(&b'[') {
                b.push_text("[");
                i += 2;
                continue;
            }
            if let Some(end) = tag_end(source, i) {
                b.tag(i..end + 1);
                i = end + 1;
                continue;
            }
        }
        let ch = source[i..].chars().next().expect("in bounds");
        let mut buf = [0u8; 4];
        b.push_text(ch.encode_utf8(&mut buf));
        i += ch.len_utf8();
    }
    b.finish()
}

/// Index of the `]` closing the tag that starts at `start`, honouring quotes.
fn tag_end(source: &str, start: usize) -> Option<usize> {
    let mut in_quote = false;
    let mut escaped = false;
    for (offset, c) in source[start + 1..].char_indices() {
        let at = start + 1 + offset;
        if in_quote {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_quote = false,
                (false, '\n') => return None,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_quote = true,
            ']' => return Some(at),
            '[' | '\n' => return None,
            _ => {}
        }
    }
    None
}

impl Builder<'_> {
    fn diag(&mut self, range: Range<usize>, kind: DiagnosticKind) {
        self.diagnostics.push(Diagnostic { range, kind });
    }

    fn push_text(&mut self, text: &str) {
        if text == "\n" {
            if self.ruby.is_some() {
                return;
            }
            self.flush();
            self.paragraphs.push(Paragraph {
                inlines: std::mem::take(&mut self.current),
                align: self.align,
            });
            if let Some(align) = self.next_align.take() {
                self.align = align;
            }
            self.started = true;
            return;
        }
        if !text.trim().is_empty() {
            self.started = true;
        }
        match &mut self.ruby {
            Some(ruby) => ruby.base.push_str(text),
            None => self.text.push_str(text),
        }
    }

    fn literal(&mut self, range: Range<usize>) {
        let raw = &self.source[range];
        for ch in raw.chars() {
            let mut buf = [0u8; 4];
            self.push_text(ch.encode_utf8(&mut buf));
        }
    }

    fn current_run(&self) -> TextRun {
        let mut run = TextRun::default();
        let mut style = Style::default();
        let mut effects = Effects::default();
        for open in &self.open {
            match &open.attr {
                Attr::Bold(v) => style.bold = Some(*v),
                Attr::Italic(v) => style.italic = Some(*v),
                Attr::Underline(v) => style.underline = Some(*v),
                Attr::Strike(v) => style.strike = Some(*v),
                Attr::Color(c) => style.color = Some(*c),
                Attr::Size(s) => style.size = *s,
                Attr::Stroke(s) => style.stroke = Some(*s),
                Attr::Shadow(s) => style.shadow = Some(*s),
                Attr::Font(f) => style.font = f.clone(),
                Attr::Continuous(c) => {
                    effects.continuous.retain(|e| e.kind != c.kind);
                    effects.continuous.push(*c);
                }
                Attr::Entrance(e) => effects.entrance = Some(*e),
                Attr::Emphasis(e) => effects.emphasis = Some(*e),
                Attr::Pace(p) => run.pace = *p,
            }
        }
        effects.continuous.sort_by_key(|e| e.kind as u8);
        run.style = style;
        run.effects = effects;
        run
    }

    fn flush(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let mut run = self.current_run();
        run.text = std::mem::take(&mut self.text);
        self.current.push(Inline::Text(run));
    }

    fn push_inline(&mut self, inline: Inline) {
        self.flush();
        self.started = true;
        self.current.push(inline);
    }

    fn tag(&mut self, range: Range<usize>) {
        let inner = &self.source[range.start + 1..range.end - 1];
        let Some(tag) = parse_tag(inner) else {
            self.literal(range);
            return;
        };
        if tag.name.starts_with('@') {
            self.header(&tag, range);
            return;
        }
        if tag.name == "align" {
            if self.ruby.is_some() {
                self.diag(range, DiagnosticKind::IgnoredInRuby(tag.name));
                return;
            }
            self.flush();
            if tag.close {
                if let Some((previous, _)) = self.align_open.pop() {
                    // Closing an aligned block keeps this paragraph's authored value;
                    // the outer value becomes active after the next newline.
                    self.next_align = Some(previous);
                } else {
                    self.diag(range.clone(), DiagnosticKind::UnmatchedClose(tag.name));
                    self.literal(range);
                }
            } else if let Some(align) = tag
                .value
                .as_deref()
                .and_then(nanatype_model::ParagraphAlign::from_key)
            {
                self.align_open.push((self.align, range));
                self.align = Some(align);
                self.next_align = None;
            } else {
                self.bad_value(&tag, range);
            }
            return;
        }
        if tag.close {
            self.close(&tag.name, range);
            return;
        }
        if tag.name == "ruby" {
            self.open_ruby(&tag, range);
            return;
        }
        if let Some(result) = span_attr(&tag) {
            match result {
                Some(attr) => {
                    if self.ruby.is_some() {
                        self.diag(range, DiagnosticKind::IgnoredInRuby(tag.name));
                        return;
                    }
                    self.flush();
                    self.open.push(Open {
                        name: tag.name,
                        attr,
                        range,
                    });
                }
                None => self.bad_value(&tag, range),
            }
            return;
        }
        if let Some(result) = self.point(&tag) {
            match result {
                Some(inline) => {
                    if self.ruby.is_some() {
                        self.diag(range, DiagnosticKind::IgnoredInRuby(tag.name));
                        return;
                    }
                    self.push_inline(inline);
                }
                None => self.bad_value(&tag, range),
            }
            return;
        }
        self.diag(range.clone(), DiagnosticKind::UnknownTag(tag.name));
        self.literal(range);
    }

    fn bad_value(&mut self, tag: &Tag, range: Range<usize>) {
        self.diag(
            range.clone(),
            DiagnosticKind::BadValue {
                tag: tag.name.clone(),
                value: tag.value.clone().unwrap_or_default(),
            },
        );
        self.literal(range);
    }

    fn header(&mut self, tag: &Tag, range: Range<usize>) {
        if self.started || tag.close {
            self.diag(range.clone(), DiagnosticKind::MisplacedHeader);
            self.literal(range);
            return;
        }
        // `[@auto=1500 after=fade]` and `[@ auto=1500]` are the same header.
        let mut entries = Vec::new();
        if tag.name.len() > 1 {
            entries.push((
                tag.name[1..].to_owned(),
                tag.value.clone().unwrap_or_default(),
            ));
        }
        entries.extend(tag.args.iter().cloned());
        let tag = &Tag {
            args: entries.clone(),
            ..tag.clone()
        };
        let mut meta = self.meta;
        for (key, v) in &entries {
            let ok = match key.as_str() {
                "auto" => value::ms(v).map(|ms| meta.auto_advance_ms = Some(ms)),
                "after" => value::after(v, tag).map(|a| meta.after = Some(a)),
                "delay" | "per" => Some(()),
                _ => None,
            };
            if ok.is_none() {
                self.diag(
                    range.clone(),
                    DiagnosticKind::BadValue {
                        tag: format!("@{key}"),
                        value: v.clone(),
                    },
                );
            }
        }
        self.meta = meta;
    }

    fn open_ruby(&mut self, tag: &Tag, range: Range<usize>) {
        let annotation = tag.value.clone().unwrap_or_default();
        if annotation.is_empty() || self.ruby.is_some() {
            self.bad_value(tag, range);
            return;
        }
        self.flush();
        self.ruby = Some(RubyOpen {
            annotation,
            base: String::new(),
            run: self.current_run(),
            range,
        });
    }

    fn close(&mut self, name: &str, range: Range<usize>) {
        if name == "ruby" {
            if let Some(ruby) = self.ruby.take() {
                let mut base = ruby.run;
                base.text = ruby.base;
                if !base.text.is_empty() {
                    self.push_inline(Inline::Ruby(RubyGroup {
                        base,
                        annotation: ruby.annotation,
                    }));
                }
                return;
            }
        } else if self.ruby.is_none()
            && let Some(index) = self.open.iter().rposition(|o| o.name == name)
        {
            self.flush();
            self.open.remove(index);
            return;
        }
        self.diag(
            range.clone(),
            DiagnosticKind::UnmatchedClose(name.to_owned()),
        );
        self.literal(range);
    }

    fn point(&self, tag: &Tag) -> Option<Option<Inline>> {
        let v = tag.value.as_deref();
        let marker = |m: Option<Marker>| Some(m.map(Inline::Marker));
        Some(match tag.name.as_str() {
            "wait" => return marker(v.and_then(value::ms).map(|ms| Marker::Wait { ms })),
            "pause" => {
                let auto = tag.arg("auto").or(v);
                return marker(match auto {
                    None => Some(Marker::Pause { auto_ms: None }),
                    Some(a) => value::ms(a).map(|ms| Marker::Pause { auto_ms: Some(ms) }),
                });
            }
            "page" => return marker(v.is_none().then_some(Marker::Page)),
            "se" => {
                let gain = match tag.arg("vol") {
                    Some(g) => value::num(g).filter(|g| *g >= 0.0),
                    None => Some(1.0),
                };
                return marker(match (v.filter(|s| !s.is_empty()), gain) {
                    (Some(sound), Some(gain)) => Some(Marker::Sound {
                        sound: sound.to_owned(),
                        gain,
                    }),
                    _ => None,
                });
            }
            "expr" => {
                let intensity = match tag.arg("intensity") {
                    Some(x) => match value::num(x) {
                        Some(x) => Some(x),
                        None => return Some(None),
                    },
                    None => None,
                };
                return marker(v.filter(|s| !s.is_empty()).map(|name| Marker::Expression {
                    name: name.to_owned(),
                    intensity,
                }));
            }
            "portrait" => {
                return marker(v.filter(|s| !s.is_empty()).map(|variant| Marker::Portrait {
                    variant: variant.to_owned(),
                }));
            }
            "screenshake" => {
                return marker(match v {
                    None => Some(Marker::ScreenShake { amp: 6.0, ms: 300 }),
                    Some(text) => {
                        let mut parts = text.split(',');
                        let amp = parts.next().and_then(value::num);
                        let ms = parts.next().map(value::ms).unwrap_or(Some(300));
                        match (amp, ms, parts.next()) {
                            (Some(amp), Some(ms), None) => Some(Marker::ScreenShake { amp, ms }),
                            _ => None,
                        }
                    }
                });
            }
            "vts" if v.is_none() => return marker(vts_action(tag).map(Marker::Vts)),
            "sticker" | "emote" | "img" => {
                let kind = match tag.name.as_str() {
                    "sticker" => AtomKind::Sticker,
                    "emote" => AtomKind::Emote,
                    _ => AtomKind::Image,
                };
                let Some(asset) = v.and_then(|v| atom_asset(kind, v)) else {
                    return Some(None);
                };
                let height_em = match tag.arg("h") {
                    Some(h) => match value::num(h).filter(|h| *h > 0.0) {
                        Some(h) => h,
                        None => return Some(None),
                    },
                    None => 1.0,
                };
                let effects = self.current_run().effects;
                Some(Inline::Atom(Atom {
                    kind,
                    asset,
                    height_em,
                    alt: tag.arg("alt").unwrap_or_default().to_owned(),
                    effects,
                }))
            }
            "at" => {
                let Some(name) = tag.arg("name").filter(|n| !n.trim().is_empty()) else {
                    return Some(None);
                };
                let avatar = match tag.arg("avatar") {
                    Some(a) => match a.parse::<AssetRef>() {
                        Ok(a) => Some(a),
                        Err(_) => return Some(None),
                    },
                    None => None,
                };
                Some(Inline::Mention(Mention {
                    user: ChatUser {
                        uid: v.unwrap_or_default().to_owned(),
                        name: name.to_owned(),
                        avatar,
                    },
                    effects: self.current_run().effects,
                }))
            }
            _ => return None,
        })
    }

    fn finish(mut self) -> Parsed {
        for (_, range) in std::mem::take(&mut self.align_open) {
            self.diag(range, DiagnosticKind::Unclosed("align".into()));
        }
        if let Some(ruby) = self.ruby.take() {
            self.diag(ruby.range.clone(), DiagnosticKind::Unclosed("ruby".into()));
            let mut base = ruby.run;
            base.text = ruby.base;
            if !base.text.is_empty() {
                self.push_inline(Inline::Ruby(RubyGroup {
                    base,
                    annotation: ruby.annotation,
                }));
            }
        }
        self.flush();
        let unclosed: Vec<_> = self
            .open
            .drain(..)
            .map(|o| (o.range, DiagnosticKind::Unclosed(o.name)))
            .collect();
        for (range, kind) in unclosed {
            self.diag(range, kind);
        }
        self.paragraphs.push(Paragraph {
            inlines: std::mem::take(&mut self.current),
            align: self.align,
        });
        let doc = Document {
            paragraphs: self.paragraphs,
        }
        .normalized();
        self.diagnostics.sort_by_key(|d| d.range.start);
        Parsed {
            doc,
            meta: self.meta,
            diagnostics: self.diagnostics,
        }
    }
}

fn atom_asset(kind: AtomKind, v: &str) -> Option<AssetRef> {
    if let Ok(asset) = v.parse::<AssetRef>() {
        return Some(asset);
    }
    if v.is_empty() || v.contains(':') {
        return None;
    }
    Some(match (kind, v.rsplit_once('/')) {
        (_, Some((lib, name))) if !lib.is_empty() && !name.is_empty() => AssetRef::Library {
            lib: lib.to_owned(),
            name: name.to_owned(),
        },
        (AtomKind::Emote, None) => AssetRef::Skin(format!("emotes/{v}")),
        (_, None) => AssetRef::Skin(format!("stickers/{v}")),
        _ => return None,
    })
}

/// `Some(Some(attr))` for a valid span tag, `Some(None)` for a span tag with
/// a bad value, `None` when `tag` is not a span tag.
fn span_attr(tag: &Tag) -> Option<Option<Attr>> {
    let v = tag.value.as_deref();
    let with = |f: fn(&str) -> Option<Attr>| Some(v.and_then(f));
    match tag.name.as_str() {
        "b" => Some(value::flag(v).map(Attr::Bold)),
        "i" => Some(value::flag(v).map(Attr::Italic)),
        "u" => Some(value::flag(v).map(Attr::Underline)),
        "s" => Some(value::flag(v).map(Attr::Strike)),
        "color" => with(|v| v.parse().ok().map(Attr::Color)),
        "size" => with(|v| {
            if v == "inherit" {
                Some(Attr::Size(None))
            } else {
                value::size(v).map(|s| Attr::Size(Some(s)))
            }
        }),
        "stroke" => with(|v| value::stroke(v).map(Attr::Stroke)),
        "shadow" => with(|v| value::shadow(v).map(Attr::Shadow)),
        "font" => with(|v| {
            if v == "inherit" {
                Some(Attr::Font(None))
            } else {
                value::font(v).map(|f| Attr::Font(Some(f)))
            }
        }),
        "speed" => with(|v| value::pace(v).map(Attr::Pace)),
        "instant" => Some(v.is_none().then_some(Attr::Pace(Pace::Instant))),
        "em" => Some(value::emphasis(tag).map(Attr::Emphasis)),
        name => {
            if let Some(kind) = ContinuousKind::from_key(name) {
                return Some(value::continuous(tag, kind).map(Attr::Continuous));
            }
            let entrance = match name {
                "fade" => EntranceKind::Fade,
                "pop" => EntranceKind::Pop,
                "slide" => EntranceKind::SlideUp,
                "noentrance" => EntranceKind::None,
                _ => return None,
            };
            Some(value::entrance(tag, entrance).map(Attr::Entrance))
        }
    }
}

/// `[vts hotkey=…]`, `[vts expr=… active= fade=]` or
/// `[vts param=… value= ms=]`.
fn vts_action(tag: &Tag) -> Option<VtsAction> {
    if let Some(hotkey) = tag.arg("hotkey") {
        return (!hotkey.is_empty()).then(|| VtsAction::Hotkey {
            hotkey: hotkey.to_owned(),
        });
    }
    if let Some(file) = tag.arg("expr") {
        let active = value::flag(tag.arg("active"))?;
        let fade = match tag.arg("fade") {
            Some(f) => Some(value::num(f).filter(|f| *f >= 0.0)?),
            None => None,
        };
        return (!file.is_empty()).then(|| VtsAction::Expression {
            file: file.to_owned(),
            active,
            fade,
        });
    }
    let name = tag.arg("param").filter(|n| !n.is_empty())?;
    let value = value::num(tag.arg("value")?)?;
    let ms = match tag.arg("ms") {
        Some(ms) => value::ms(ms)?,
        None => 0,
    };
    Some(VtsAction::Param {
        name: name.to_owned(),
        value,
        ms,
    })
}
