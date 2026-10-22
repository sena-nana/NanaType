use std::fmt;
use std::ops::Range;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use unicode_segmentation::UnicodeSegmentation;

use crate::{CharClass, ChatUser, Effects, Emphasis, Pace, Style, classify};

/// A line longer than this many reveal units is cut at normalization.
pub const MAX_UNITS: usize = 4096;

/// Where an atom's pixels live. The document itself never carries pixels.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AssetRef {
    /// Content-addressed blob in the session asset store (clipboard paste).
    Blob(String),
    /// A file inside the active skin package (`stickers/wave.png`).
    Skin(String),
    /// An entry of a user sticker library: folder name and file stem.
    Library { lib: String, name: String },
    /// An absolute file the user picked.
    File(String),
}

impl fmt::Display for AssetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blob(hash) => write!(f, "blob:{hash}"),
            Self::Skin(path) => write!(f, "skin:{path}"),
            Self::Library { lib, name } => write!(f, "lib:{lib}/{name}"),
            Self::File(path) => write!(f, "file:{path}"),
        }
    }
}

impl FromStr for AssetRef {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (scheme, rest) = value
            .split_once(':')
            .ok_or_else(|| format!("asset `{value}` has no scheme"))?;
        if rest.is_empty() {
            return Err(format!("asset `{value}` is empty"));
        }
        Ok(match scheme {
            "blob" => Self::Blob(rest.to_owned()),
            "skin" => Self::Skin(rest.to_owned()),
            "file" => Self::File(rest.to_owned()),
            "lib" => {
                let (lib, name) = rest
                    .rsplit_once('/')
                    .ok_or_else(|| format!("library asset `{value}` needs lib/name"))?;
                Self::Library {
                    lib: lib.to_owned(),
                    name: name.to_owned(),
                }
            }
            _ => return Err(format!("unknown asset scheme `{scheme}`")),
        })
    }
}

impl Serialize for AssetRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for AssetRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtomKind {
    Sticker,
    Emote,
    Image,
}

/// An inline picture. It counts as one reveal unit and wraps like a glyph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Atom {
    pub kind: AtomKind,
    pub asset: AssetRef,
    /// Height as a multiple of the line's body size.
    #[serde(default = "default_atom_height")]
    pub height_em: f32,
    /// Spoken/plain text form, used by copy, history export and screen readers.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alt: String,
    #[serde(default, skip_serializing_if = "Effects::is_none")]
    pub effects: Effects,
}

fn default_atom_height() -> f32 {
    1.0
}

/// A live-chat viewer named inline: drawn as a label with their avatar and
/// name. Like an atom it is one reveal unit and wraps as a whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mention {
    pub user: ChatUser,
    #[serde(default, skip_serializing_if = "Effects::is_none")]
    pub effects: Effects,
}

impl Mention {
    pub fn new(user: ChatUser) -> Self {
        Self {
            user,
            effects: Effects::default(),
        }
    }

    /// The plain-text form: `@name`.
    pub fn alt(&self) -> String {
        format!("@{}", self.user.name)
    }
}

/// A zero-width point event. Markers are chips in the editor and never paint
/// on the typewriter or in history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "marker", rename_all = "snake_case")]
pub enum Marker {
    /// Hold the reveal for `ms`.
    Wait { ms: u32 },
    /// Stop until the continue key; `auto_ms` continues on its own.
    Pause {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auto_ms: Option<u32>,
    },
    /// Wait for continue, then clear the box and keep typing on a fresh page.
    Page,
    /// Play a sound effect: a skin `se` key or an asset reference string.
    Sound {
        sound: String,
        #[serde(default = "unit_gain")]
        gain: f32,
    },
    /// Broadcast an expression (VTube Studio hotkey, OSC, WebSocket) and
    /// switch the portrait variant of the same name when the skin has one.
    Expression {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intensity: Option<f32>,
    },
    /// Switch the portrait/avatar variant without broadcasting.
    Portrait { variant: String },
    /// Shake the whole dialogue box.
    ScreenShake { amp: f32, ms: u32 },
    /// A VTube Studio request.
    Vts(VtsAction),
}

/// When VTube Studio expressions turned on by a line are turned off again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VtsReset {
    /// Expressions stay on until something turns them off.
    Off,
    /// When the line leaves the box.
    #[default]
    LineEnd,
    /// When the next line starts.
    NextLine,
}

impl VtsReset {
    pub const ALL: [Self; 3] = [Self::LineEnd, Self::NextLine, Self::Off];

    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::LineEnd => "line_end",
            Self::NextLine => "next_line",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.key() == key)
    }
}

/// What a `[vts ...]` marker asks VTube Studio to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VtsAction {
    /// Press a hotkey of the current model, by name or id.
    Hotkey { hotkey: String },
    /// Turn an expression file (`Smile.exp3.json`) on or off.
    Expression {
        file: String,
        active: bool,
        /// Fade time in seconds; VTube Studio's default when unset.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fade: Option<f32>,
    },
    /// Set a parameter to `value` and hold it for `ms` (`0`: set once).
    Param {
        name: String,
        value: f32,
        #[serde(default)]
        ms: u32,
    },
}

fn unit_gain() -> f32 {
    1.0
}

/// Styled text. `text` never contains `\n` after normalization; hard breaks
/// split paragraphs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    #[serde(default, skip_serializing_if = "Style::is_plain")]
    pub style: Style,
    #[serde(default, skip_serializing_if = "Effects::is_none")]
    pub effects: Effects,
    #[serde(default, skip_serializing_if = "Pace::is_inherit")]
    pub pace: Pace,
}

impl TextRun {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    pub fn styled(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
            ..Self::default()
        }
    }

    /// Same attributes, so the two runs may be merged.
    pub fn same_attrs(&self, other: &TextRun) -> bool {
        self.style == other.style && self.effects == other.effects && self.pace == other.pace
    }
}

/// Base text with a reading above it. The base reveals; the annotation
/// appears together with the first base character.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RubyGroup {
    pub base: TextRun,
    pub annotation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    Text(TextRun),
    Ruby(RubyGroup),
    Atom(Atom),
    Mention(Mention),
    Marker(Marker),
}

impl Inline {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextRun::plain(text))
    }

    /// An atom or a mention: one reveal unit with no text of its own.
    pub fn is_object(&self) -> bool {
        matches!(self, Self::Atom(_) | Self::Mention(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParagraphAlign {
    Start,
    Center,
    End,
}

impl ParagraphAlign {
    pub fn key(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "start" => Some(Self::Start),
            "center" => Some(Self::Center),
            "end" => Some(Self::End),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<ParagraphAlign>,
    #[serde(default)]
    pub inlines: Vec<Inline>,
}

impl Paragraph {
    pub fn new(inlines: Vec<Inline>) -> Self {
        Self {
            inlines,
            align: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    /// A grapheme of a text run (`ruby: false`) or of a ruby base (`ruby: true`).
    Grapheme {
        class: CharClass,
        ruby: bool,
    },
    Atom,
}

/// One reveal step: a grapheme cluster or an atom.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub para: usize,
    pub inline: usize,
    /// Byte range inside the run's (or ruby base's) text; empty for atoms.
    pub bytes: Range<usize>,
    pub kind: UnitKind,
    pub pace: Pace,
    pub emphasis: Option<Emphasis>,
}

impl Unit {
    pub fn class(&self) -> CharClass {
        match self.kind {
            UnitKind::Grapheme { class, .. } => class,
            UnitKind::Atom => CharClass::Atom,
        }
    }
}

/// A dialogue line's content.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(default)]
    pub paragraphs: Vec<Paragraph>,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    /// Unstyled text; `\n` starts a new paragraph.
    pub fn plain(text: &str) -> Self {
        let mut doc = Self {
            paragraphs: vec![Paragraph::new(vec![Inline::text(text)])],
        };
        doc.normalize();
        doc
    }

    pub fn from_inlines(inlines: Vec<Inline>) -> Self {
        let mut doc = Self {
            paragraphs: vec![Paragraph::new(inlines)],
        };
        doc.normalize();
        doc
    }

    /// No graphemes and no atoms. Markers alone do not make a line.
    pub fn is_blank(&self) -> bool {
        self.paragraphs.iter().all(|p| {
            p.inlines.iter().all(|inline| match inline {
                Inline::Text(run) => run.text.trim().is_empty(),
                Inline::Ruby(ruby) => ruby.base.text.trim().is_empty(),
                Inline::Atom(_) | Inline::Mention(_) => false,
                Inline::Marker(_) => true,
            })
        })
    }

    /// Plain text with atoms as their alt text and ruby as its base.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for (index, paragraph) in self.paragraphs.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            for inline in &paragraph.inlines {
                match inline {
                    Inline::Text(run) => out.push_str(&run.text),
                    Inline::Ruby(ruby) => out.push_str(&ruby.base.text),
                    Inline::Atom(atom) => out.push_str(&atom.alt),
                    Inline::Mention(mention) => out.push_str(&mention.alt()),
                    Inline::Marker(_) => {}
                }
            }
        }
        out
    }

    /// Reveal units in document order.
    pub fn units(&self) -> Vec<Unit> {
        let mut units = Vec::new();
        self.walk(|item| {
            if let Item::Unit(unit) = item {
                units.push(unit);
            }
        });
        units
    }

    pub fn unit_count(&self) -> usize {
        let mut count = 0;
        self.walk(|item| {
            if matches!(item, Item::Unit(_)) {
                count += 1;
            }
        });
        count
    }

    /// Markers with the number of units revealed before them.
    pub fn markers(&self) -> Vec<(usize, &Marker)> {
        let mut out = Vec::new();
        let mut seen = 0;
        for paragraph in &self.paragraphs {
            for inline in &paragraph.inlines {
                match inline {
                    Inline::Text(run) => seen += run.text.graphemes(true).count(),
                    Inline::Ruby(ruby) => seen += ruby.base.text.graphemes(true).count(),
                    Inline::Atom(_) | Inline::Mention(_) => seen += 1,
                    Inline::Marker(marker) => out.push((seen, marker)),
                }
            }
        }
        out
    }

    /// Visit units and markers in order. Unit ordinals are implied by order.
    pub fn walk(&self, mut visit: impl FnMut(Item<'_>)) {
        for (para, paragraph) in self.paragraphs.iter().enumerate() {
            for (inline_index, inline) in paragraph.inlines.iter().enumerate() {
                let text_units = |run: &TextRun, ruby: bool, visit: &mut dyn FnMut(Item<'_>)| {
                    for (start, cluster) in run.text.grapheme_indices(true) {
                        visit(Item::Unit(Unit {
                            para,
                            inline: inline_index,
                            bytes: start..start + cluster.len(),
                            kind: UnitKind::Grapheme {
                                class: classify(cluster),
                                ruby,
                            },
                            pace: run.pace,
                            emphasis: run.effects.emphasis,
                        }));
                    }
                };
                match inline {
                    Inline::Text(run) => text_units(run, false, &mut visit),
                    Inline::Ruby(ruby) => text_units(&ruby.base, true, &mut visit),
                    Inline::Atom(_) | Inline::Mention(_) => visit(Item::Unit(Unit {
                        para,
                        inline: inline_index,
                        bytes: 0..0,
                        kind: UnitKind::Atom,
                        pace: Pace::Inherit,
                        emphasis: None,
                    })),
                    Inline::Marker(marker) => visit(Item::Marker(marker)),
                }
            }
        }
    }

    /// The text of one unit (empty for atoms).
    pub fn unit_text(&self, unit: &Unit) -> &str {
        match self
            .paragraphs
            .get(unit.para)
            .and_then(|p| p.inlines.get(unit.inline))
        {
            Some(Inline::Text(run)) => run.text.get(unit.bytes.clone()).unwrap_or(""),
            Some(Inline::Ruby(ruby)) => ruby.base.text.get(unit.bytes.clone()).unwrap_or(""),
            _ => "",
        }
    }

    /// Canonical form: LF paragraphs, no empty runs, equal neighbours merged,
    /// at most [`MAX_UNITS`] units, never zero paragraphs. Idempotent.
    pub fn normalize(&mut self) {
        let mut paragraphs: Vec<Paragraph> = Vec::new();
        let mut current: Vec<Inline> = Vec::new();
        let mut budget = MAX_UNITS;

        fn push_run(current: &mut Vec<Inline>, run: TextRun) {
            if run.text.is_empty() {
                return;
            }
            if let Some(Inline::Text(last)) = current.last_mut()
                && last.same_attrs(&run)
            {
                last.text.push_str(&run.text);
                return;
            }
            current.push(Inline::Text(run));
        }

        fn take_graphemes(text: &str, budget: &mut usize) -> String {
            let mut out = String::new();
            for cluster in text.graphemes(true) {
                if *budget == 0 {
                    break;
                }
                *budget -= 1;
                out.push_str(cluster);
            }
            out
        }

        let source = std::mem::take(&mut self.paragraphs);
        let count = source.len();
        let mut align = None;
        for (index, paragraph) in source.into_iter().enumerate() {
            align = paragraph.align;
            for inline in paragraph.inlines {
                match inline {
                    Inline::Text(run) => {
                        let text = run.text.replace("\r\n", "\n").replace('\r', "\n");
                        let mut pieces = text.split('\n').peekable();
                        while let Some(piece) = pieces.next() {
                            let kept = take_graphemes(piece, &mut budget);
                            push_run(
                                &mut current,
                                TextRun {
                                    text: kept,
                                    ..run.clone()
                                },
                            );
                            if pieces.peek().is_some() {
                                paragraphs.push(Paragraph {
                                    inlines: std::mem::take(&mut current),
                                    align,
                                });
                            }
                        }
                    }
                    Inline::Ruby(mut ruby) => {
                        ruby.base.text = ruby.base.text.replace(['\r', '\n'], "");
                        ruby.annotation = ruby.annotation.replace(['\r', '\n'], "");
                        ruby.base.text = take_graphemes(&ruby.base.text, &mut budget);
                        if ruby.base.text.is_empty() {
                            continue;
                        }
                        if ruby.annotation.trim().is_empty() {
                            push_run(&mut current, ruby.base);
                        } else {
                            current.push(Inline::Ruby(ruby));
                        }
                    }
                    Inline::Atom(_) | Inline::Mention(_) => {
                        if budget == 0 {
                            continue;
                        }
                        budget -= 1;
                        current.push(inline);
                    }
                    Inline::Marker(marker) => current.push(Inline::Marker(marker)),
                }
            }
            if index + 1 < count {
                paragraphs.push(Paragraph {
                    inlines: std::mem::take(&mut current),
                    align,
                });
            }
        }
        paragraphs.push(Paragraph {
            inlines: current,
            align,
        });
        self.paragraphs = paragraphs;
    }

    pub fn normalized(mut self) -> Self {
        self.normalize();
        self
    }

    /// Every text run (including ruby bases), mutably.
    pub fn runs_mut(&mut self) -> impl Iterator<Item = &mut TextRun> {
        self.paragraphs.iter_mut().flat_map(|p| {
            p.inlines.iter_mut().filter_map(|inline| match inline {
                Inline::Text(run) => Some(run),
                Inline::Ruby(ruby) => Some(&mut ruby.base),
                _ => None,
            })
        })
    }

    /// A copy without markers: what history shows and what is copied out.
    pub fn without_markers(&self) -> Document {
        let mut doc = Document {
            paragraphs: self
                .paragraphs
                .iter()
                .map(|p| Paragraph {
                    align: p.align,
                    inlines: p
                        .inlines
                        .iter()
                        .filter(|inline| !matches!(inline, Inline::Marker(_)))
                        .cloned()
                        .collect(),
                })
                .collect(),
        };
        doc.normalize();
        doc
    }

    /// The first `count` units with the markers before them. Used to show the
    /// part of a line that is already revealed.
    pub fn prefix(&self, count: usize) -> Document {
        let mut left = count;
        let mut paragraphs = Vec::new();
        'outer: for paragraph in &self.paragraphs {
            let mut inlines = Vec::new();
            for inline in &paragraph.inlines {
                match inline {
                    Inline::Text(run) => {
                        let text: String = run.text.graphemes(true).take(left).collect();
                        left -= text.graphemes(true).count();
                        inlines.push(Inline::Text(TextRun {
                            text,
                            ..run.clone()
                        }));
                    }
                    Inline::Ruby(ruby) => {
                        let base: String = ruby.base.text.graphemes(true).take(left).collect();
                        left -= base.graphemes(true).count();
                        let mut ruby = ruby.clone();
                        ruby.base.text = base;
                        inlines.push(Inline::Ruby(ruby));
                    }
                    Inline::Atom(_) | Inline::Mention(_) if left > 0 => {
                        left -= 1;
                        inlines.push(inline.clone());
                    }
                    Inline::Atom(_) | Inline::Mention(_) => {}
                    Inline::Marker(marker) => inlines.push(Inline::Marker(marker.clone())),
                }
                if left == 0 {
                    paragraphs.push(Paragraph {
                        inlines,
                        align: paragraph.align,
                    });
                    break 'outer;
                }
            }
            paragraphs.push(Paragraph {
                inlines,
                align: paragraph.align,
            });
        }
        let mut doc = Document { paragraphs };
        doc.normalize();
        doc
    }

    /// Everything from unit `from` on: a page that starts mid-line. Leading
    /// paragraphs left without units are dropped, so a page does not open on
    /// a blank line; `suffix(n).unit_count() == unit_count() - n`.
    pub fn suffix(&self, from: usize) -> Document {
        if from == 0 {
            return self.clone();
        }
        let mut skip = from;
        let mut paragraphs: Vec<Paragraph> = Vec::new();
        for paragraph in &self.paragraphs {
            let mut inlines = Vec::new();
            let mut units = 0;
            for inline in &paragraph.inlines {
                let mut inline = inline.clone();
                match &mut inline {
                    Inline::Text(run) | Inline::Ruby(RubyGroup { base: run, .. }) => {
                        let cut = run
                            .text
                            .grapheme_indices(true)
                            .nth(skip)
                            .map_or(run.text.len(), |(i, _)| i);
                        skip -= run.text.drain(..cut).as_str().graphemes(true).count();
                        units += run.text.graphemes(true).count();
                    }
                    Inline::Atom(_) | Inline::Mention(_) if skip > 0 => {
                        skip -= 1;
                        continue;
                    }
                    Inline::Atom(_) | Inline::Mention(_) => units += 1,
                    Inline::Marker(_) if skip > 0 => continue,
                    Inline::Marker(_) => {}
                }
                inlines.push(inline);
            }
            if paragraphs.is_empty() && units == 0 {
                continue;
            }
            paragraphs.push(Paragraph {
                inlines,
                align: paragraph.align,
            });
        }
        let mut doc = Document { paragraphs };
        doc.normalize();
        doc
    }
}

/// What [`Document::walk`] visits.
#[derive(Debug)]
pub enum Item<'a> {
    Unit(Unit),
    Marker(&'a Marker),
}
