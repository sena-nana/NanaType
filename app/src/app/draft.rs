//! Unified draft editor state, plain-text policy gate, and monotonic version
//! fence shared with host routing and speech dictation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use nanatype_markup::{parse, serialize, serialize_line};
use nanatype_model::{AssetRef, Document, Inline};

use super::EditorCommand;
use crate::msg::MarkupEdit;
use crate::ui::Ui;

/// Whether the current outgoing context permits inline images / atoms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DraftPolicy {
    #[default]
    Rich,
    /// Active Twitch chat sync or P2P session restricts drafts to plain text.
    PlainTextOnly,
}

impl DraftPolicy {
    pub fn from_sync(synchronizing: bool) -> Self {
        if synchronizing {
            Self::PlainTextOnly
        } else {
            Self::Rich
        }
    }

    pub fn is_plain_text_only(self) -> bool {
        matches!(self, Self::PlainTextOnly)
    }
}

/// Lock-free monotonic draft mutation & IME composition fence shared across the
/// host input router and speech dictation controller.
#[derive(Debug, Clone, Default)]
pub struct DraftFence {
    version: Arc<AtomicU64>,
    composing: Arc<AtomicBool>,
}

impl DraftFence {
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn bump_version(&self) -> u64 {
        self.version.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn is_composing(&self) -> bool {
        self.composing.load(Ordering::Acquire)
    }

    pub fn set_composing(&self, composing: bool) {
        self.composing.store(composing, Ordering::Release);
    }

    /// True only when no draft edit has occurred since `expected` and IME
    /// composition is inactive.
    pub fn allows_dictation(&self, expected_version: u64) -> bool {
        self.version() == expected_version && !self.is_composing()
    }
}

/// Owns pending editor bridge commands, completion state, clipboard cache,
/// plain-text policy gating, and the shared [`DraftFence`].
pub struct DraftController {
    pub policy: DraftPolicy,
    pub fence: DraftFence,
    pub rich_commands: Vec<EditorCommand>,
    pub focus_editor: bool,
    pub source_edit_undoable: bool,
    pub completer: crate::completion::Completer,
    clipboard: Option<arboard::Clipboard>,
}

impl DraftController {
    pub fn new(fence: DraftFence) -> Self {
        Self {
            policy: DraftPolicy::Rich,
            fence,
            rich_commands: Vec::new(),
            focus_editor: false,
            source_edit_undoable: false,
            completer: crate::completion::Completer::default(),
            clipboard: None,
        }
    }

    pub fn set_policy(&mut self, policy: DraftPolicy) {
        self.policy = policy;
    }

    pub fn is_plain_text_only(&self) -> bool {
        self.policy.is_plain_text_only()
    }

    /// Gate inserting a new image/atom (sticker, pasted image, image picker).
    pub fn allow_new_image(&self) -> bool {
        !self.policy.is_plain_text_only()
    }

    /// Gate a whole-document edit that might introduce additional outgoing images.
    pub fn allow_image_growth(&self, candidate: &Document, baseline: &Document) -> bool {
        !self.policy.is_plain_text_only()
            || nanatype_model::outgoing::image_count(candidate)
                <= nanatype_model::outgoing::image_count(baseline)
    }

    pub fn bump_version(&self) -> u64 {
        self.fence.bump_version()
    }

    pub fn version(&self) -> u64 {
        self.fence.version()
    }

    pub fn take_bridge_state(&mut self) -> (Vec<EditorCommand>, bool, bool) {
        (
            std::mem::take(&mut self.rich_commands),
            std::mem::take(&mut self.focus_editor),
            std::mem::take(&mut self.source_edit_undoable),
        )
    }

    pub fn clipboard_image(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        let clipboard = match &mut self.clipboard {
            Some(cb) => cb,
            None => {
                let cb = arboard::Clipboard::new().ok()?;
                self.clipboard.insert(cb)
            }
        };
        let image = clipboard.get_image().ok()?;
        Some((
            image.width as u32,
            image.height as u32,
            image.bytes.into_owned(),
        ))
    }
}

/// Pictures `doc` shows: atoms and the avatars of mentioned viewers.
pub(crate) fn doc_assets(doc: &Document) -> Vec<AssetRef> {
    doc.paragraphs
        .iter()
        .flat_map(|p| &p.inlines)
        .filter_map(|i| match i {
            Inline::Atom(a) => Some(a.asset.clone()),
            Inline::Mention(m) => m.user.avatar.clone(),
            _ => None,
        })
        .collect()
}

/// Synchronize an actual editor event before reactive views read the draft.
pub(crate) fn sync_rich_draft(ui: Ui, rich: nana_ui_core::RichText) -> Document {
    let look = crate::ui::dialogue::Base::editor(&ui.skin.get_untracked()).look();
    let doc = crate::ui::rich::from_rich(&rich, &look);
    ui.editor_value.set(rich);
    let meta = parse(&ui.source.get_untracked().text).meta;
    let text = serialize_line(&doc, &meta);
    let mut src = ui.source.get_untracked();
    if src.text != text {
        let len = text.len();
        src.text = text;
        src.selection = (len, len);
        src.revision += 1;
        ui.source.set(src);
    }
    ui.diagnostics.set(Arc::new(Vec::new()));
    ui.draft.set(Arc::new(doc.clone()));
    doc
}

pub(crate) fn apply_source_markup_guarded(
    src: &mut crate::ui::Source,
    edit: MarkupEdit,
    policy: DraftPolicy,
) -> bool {
    let mut candidate = src.clone();
    apply_source_markup(&mut candidate, edit);
    if policy.is_plain_text_only()
        && nanatype_model::outgoing::image_count(&parse(&candidate.text).doc)
            > nanatype_model::outgoing::image_count(&parse(&src.text).doc)
    {
        // Trigger the retained editor binding to restore its pre-edit value.
        src.revision += 1;
        return false;
    }
    *src = candidate;
    true
}

pub(crate) fn apply_source_markup(src: &mut crate::ui::Source, edit: MarkupEdit) {
    match edit {
        MarkupEdit::Typed { text, selection } => {
            src.text = text;
            src.selection = selection;
        }
        MarkupEdit::Wrap { open, close } => {
            let (a, b) = clamp_sel(&src.text, src.selection);
            src.text.insert_str(b, &close);
            src.text.insert_str(a, &open);
            let caret = if a == b {
                a + open.len()
            } else {
                b + open.len() + close.len()
            };
            src.selection = (caret, caret);
            src.revision += 1;
        }
        MarkupEdit::Insert(tag) => {
            let (a, b) = clamp_sel(&src.text, src.selection);
            src.text.replace_range(a..b, &tag);
            src.selection = (a + tag.len(), a + tag.len());
            src.revision += 1;
        }
        MarkupEdit::Align(align) => {
            let (a, b) = clamp_sel(&src.text, src.selection);
            let first = src.text[..a].bytes().filter(|v| *v == b'\n').count();
            let last_byte = if b > a { b - 1 } else { b };
            let last = src.text.as_bytes()[..last_byte]
                .iter()
                .filter(|v| **v == b'\n')
                .count();
            let mut parsed = parse(&src.text);
            for paragraph in parsed.doc.paragraphs.iter_mut().take(last + 1).skip(first) {
                paragraph.align = align;
            }
            src.text = serialize_line(&parsed.doc, &parsed.meta);
            let start = src
                .text
                .split_inclusive('\n')
                .take(first)
                .map(str::len)
                .sum::<usize>();
            let end = src
                .text
                .split_inclusive('\n')
                .take(last + 1)
                .map(str::len)
                .sum::<usize>()
                .min(src.text.len());
            src.selection = (start, end);
            src.revision += 1;
        }
        MarkupEdit::ClearFormat => {
            let (a, b) = clamp_sel(&src.text, src.selection);
            let (a, b) = if a == b { (0, src.text.len()) } else { (a, b) };
            let plain = strip_tags(&src.text[a..b]);
            src.text.replace_range(a..b, &plain);
            src.selection = (a, a + plain.len());
            if parse(&src.text)
                .doc
                .paragraphs
                .iter()
                .any(|p| p.align.is_some())
            {
                apply_source_markup(src, MarkupEdit::Align(None));
            } else {
                src.revision += 1;
            }
        }
    }
}

fn clamp_sel(text: &str, (a, b): (usize, usize)) -> (usize, usize) {
    let fix = |mut i: usize| {
        i = i.min(text.len());
        while i > 0 && !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (a, b) = (fix(a), fix(b));
    (a.min(b), a.max(b))
}

/// Remove `[tag]` and `[/tag]` but keep `[[` escapes and the text.
fn strip_tags(text: &str) -> String {
    let parsed = parse(text);
    let mut doc = parsed.doc;
    for paragraph in &mut doc.paragraphs {
        paragraph.align = None;
    }
    for run in doc.runs_mut() {
        run.style = Default::default();
        run.effects = Default::default();
        run.pace = Default::default();
    }
    serialize(&doc.normalized())
}

pub(crate) fn quote(v: &str) -> String {
    if v.chars()
        .any(|c| c.is_whitespace() || matches!(c, ']' | '"' | '[' | '\\'))
    {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_owned()
    }
}

impl super::App {
    /// Keep [`DraftController::policy`] synchronized with active plain-text transports
    /// (currently Twitch chat sending).
    pub(crate) fn sync_draft_policy(&mut self) {
        let policy = if self.ui.twitch.with_untracked(|t| t.sending) {
            DraftPolicy::PlainTextOnly
        } else {
            DraftPolicy::Rich
        };
        self.draft.set_policy(policy);
    }

    /// Replace the editor text. `program` marks it as not typed by the user.
    pub(super) fn set_source(&mut self, text: String, program: bool) {
        if !program
            && !self
                .draft
                .allow_image_growth(&parse(&text).doc, self.session.core.draft())
        {
            self.notice(self.ui.now_t("chat-images-unavailable"), true);
            return;
        }

        let mut src = self.ui.source.get_untracked();
        if src.text == text && !program {
            return;
        }
        let len = text.len();
        src.text = text;
        if program {
            src.revision += 1;
            src.selection = (len, len);
        }
        self.ui.source.set(src.clone());
        self.reparse(&src.text);
    }

    pub(super) fn reparse(&mut self, text: &str) {
        let parsed = parse(text);
        let locale = self.ui.locale.get_untracked();
        let diags: Vec<String> = parsed
            .diagnostics
            .iter()
            .filter(|d| !matches!(d.kind, nanatype_markup::DiagnosticKind::Unclosed(_)))
            .map(|d| {
                let pos = text[..d.range.start.min(text.len())].chars().count() + 1;
                let message = match &d.kind {
                    nanatype_markup::DiagnosticKind::UnknownTag(t) => format!("[{t}] ?"),
                    nanatype_markup::DiagnosticKind::BadValue { tag, value } => {
                        format!("[{tag}={value}] ?")
                    }
                    nanatype_markup::DiagnosticKind::UnmatchedClose(t) => format!("[/{t}] ?"),
                    other => format!("{other:?}"),
                };
                crate::ui::I18N.tr_in(
                    locale,
                    "editor-diagnostic",
                    &[("pos", pos.into()), ("message", message.into())],
                )
            })
            .collect();
        self.ui.diagnostics.set(Arc::new(diags));
        let doc = parsed.doc;
        self.ui.editor_value.set(self.editor_rich(&doc));
        self.ui.draft.set(Arc::new(doc.clone()));
        self.run(nanatype_core::Cmd::DraftMeta(parsed.meta));
        self.run(nanatype_core::Cmd::DraftChanged(doc));
        self.refresh_completion();
    }

    /// The editor's look: the dialogue text of the active skin.
    pub(super) fn editor_look(&self) -> crate::ui::rich::Look {
        self.appearance
            .editor_look(self.settings.assets.sticker_scale)
    }

    /// `doc` as the editor's rich text.
    pub(super) fn editor_rich(&self, doc: &Document) -> nana_ui_core::RichText {
        let urls = self.ui.asset_urls.get_untracked();
        self.appearance.editor_rich(
            doc,
            self.settings.assets.sticker_scale,
            &urls,
            self.ui.locale.get_untracked(),
        )
    }

    /// The user edited the rich text.
    pub(super) fn editor_changed(&mut self, rich: nana_ui_core::RichText) {
        let previous = self.session.core.draft().clone();
        let doc = sync_rich_draft(self.ui, rich);
        if !self.draft.allow_image_growth(&doc, &previous) {
            self.set_source(
                serialize_line(&previous, &self.session.core.draft_meta()),
                true,
            );
            self.notice(self.ui.now_t("chat-images-unavailable"), true);
            return;
        }
        self.run(nanatype_core::Cmd::DraftChanged(doc));
        self.refresh_atom_urls();
    }

    /// A toolbar action: an editor command, or a markup edit in source mode.
    pub(super) fn tool(&mut self, tool: crate::msg::Tool) {
        self.speech_user_edit();
        if !self.draft.allow_new_image()
            && matches!(tool, crate::msg::Tool::Insert(Inline::Atom(_)))
        {
            self.notice(self.ui.now_t("chat-images-unavailable"), true);
            return;
        }
        self.draft.focus_editor = true;
        use crate::msg::Tool;
        use nana_ui::runtime::{RichAttrMask, RichEditCommand};
        use nana_ui_core::{PaintColor, RichSpanStyle, RichTextShadow, RichTextStroke};

        if self.ui.source_mode.get_untracked() {
            if let Some(edit) = markup_for(&tool) {
                self.markup(edit);
            }
            return;
        }
        let attrs = self.ui.sel_attrs.get_untracked();
        let look = self.editor_look();
        let uniform = |mask: RichAttrMask| !attrs.mixed.contains(mask);
        let paint = |c: nanatype_model::Rgba8| PaintColor::srgb(crate::skin_theme::rgba(c));
        // Effects and pacing share one interned attribute per span.
        let (mut effects, mut pace) = match attrs.style.effect {
            Some(index) if uniform(RichAttrMask::EFFECT) => crate::ui::rich::attr_of(index),
            _ => Default::default(),
        };
        let command = match tool {
            Tool::Bold => {
                let on = uniform(RichAttrMask::WEIGHT)
                    && attrs.style.shape.weight.is_some_and(|w| w >= 600);
                RichEditCommand::SetAttrs(RichSpanStyle::new().weight(if on {
                    400.min(look.weight)
                } else {
                    700.max(look.weight + 200).min(900)
                }))
            }
            Tool::Italic => {
                let on = uniform(RichAttrMask::ITALIC) && attrs.style.shape.italic == Some(true);
                RichEditCommand::SetAttrs(RichSpanStyle::new().italic(!on))
            }
            Tool::Underline | Tool::Strike => {
                let mut d = attrs.style.paint.decoration.unwrap_or_default();
                if matches!(tool, Tool::Underline) {
                    d.underline = !(uniform(RichAttrMask::DECORATION) && d.underline);
                } else {
                    d.line_through = !(uniform(RichAttrMask::DECORATION) && d.line_through);
                }
                let mut s = RichSpanStyle::new();
                s.paint.decoration = Some(d);
                RichEditCommand::SetAttrs(s)
            }
            Tool::Color(c) => RichEditCommand::SetAttrs(RichSpanStyle::new().color(paint(c))),
            Tool::Size(Some(nanatype_model::Size::Em(em))) => {
                RichEditCommand::SetAttrs(RichSpanStyle::new().size_em(em))
            }
            Tool::Size(Some(nanatype_model::Size::Px(px))) => {
                RichEditCommand::SetAttrs(RichSpanStyle::new().size(px))
            }
            Tool::Size(None) => RichEditCommand::ClearAttrs(RichAttrMask::SIZE),
            Tool::Font(None) => RichEditCommand::ClearAttrs(RichAttrMask::FAMILY),
            Tool::Font(Some(font)) => RichEditCommand::SetAttrs(look.font_style(&font)),
            Tool::Align(align) => {
                RichEditCommand::SetParagraphAlignment(align.map(crate::ui::rich::align_to_rich))
            }
            Tool::Stroke(st) => {
                let (w, c) = st
                    .map(|s| (s.width_px, s.color))
                    .unwrap_or((0.0, look.color));
                RichEditCommand::SetAttrs(
                    RichSpanStyle::new().stroke(
                        RichTextStroke::new(w, paint(c))
                            .placement(nana_ui_core::TextStrokePlacement::Under),
                    ),
                )
            }
            Tool::Shadow(sh) => {
                let mut s = RichSpanStyle::new();
                s.paint.shadows = Some(match sh {
                    Some(sh) => Arc::from([RichTextShadow::new(
                        [sh.dx, sh.dy],
                        sh.blur,
                        paint(sh.color),
                    )]),
                    None => Arc::from([]),
                });
                RichEditCommand::SetAttrs(s)
            }
            Tool::Ruby(text) => {
                RichEditCommand::SetRuby((!text.trim().is_empty()).then(|| text.trim().to_owned()))
            }
            Tool::Effect(_) | Tool::Emphasis(_) | Tool::Entrance(..) | Tool::Pace(_) => {
                match tool {
                    Tool::Effect(effect) => effects.switch(effect),
                    Tool::Emphasis(em) => {
                        effects.emphasis = match effects.emphasis {
                            Some(on) if on == em => None,
                            _ => Some(em),
                        }
                    }
                    Tool::Entrance(kind, ms) => {
                        effects.entrance = Some(nanatype_model::Entrance {
                            kind,
                            ms: ms.unwrap_or(
                                self.appearance.skin.dialogue.reveal.entrance.ms.max(120),
                            ),
                        })
                    }
                    Tool::Pace(p) => pace = p,
                    _ => unreachable!(),
                }
                RichEditCommand::SetAttrs(
                    RichSpanStyle::new().effect(crate::ui::rich::attr_index_any(&effects, pace)),
                )
            }
            // Built the way the editor's text builds it: a tag for a marker,
            // a picture at its own aspect.
            Tool::Insert(inline) => {
                let doc = Document::from_inlines(vec![inline]);
                self.ensure_urls(doc_assets(&doc));
                match self.editor_rich(&doc).objects().first() {
                    Some((_, object)) => RichEditCommand::InsertObject(object.clone()),
                    None => return,
                }
            }
            Tool::ClearFormat => RichEditCommand::ClearFormatting,
        };
        self.draft.rich_commands.push(command.into());
    }

    pub(super) fn markup(&mut self, edit: MarkupEdit) {
        self.speech_user_edit();
        self.draft.source_edit_undoable |= !matches!(&edit, MarkupEdit::Typed { .. });
        let mut src = self.ui.source.get_untracked();
        let before = src.text.clone();
        if !apply_source_markup_guarded(&mut src, edit, self.draft.policy) {
            self.ui.source.set(src);
            self.notice(self.ui.now_t("chat-images-unavailable"), true);
            return;
        }
        self.ui.source.set(src.clone());
        if src.text != before {
            self.reparse(&src.text);
        }
    }

    pub(super) fn insert_asset(&mut self, asset: AssetRef) {
        if !self.draft.allow_new_image() {
            self.notice(self.ui.now_t("chat-images-unavailable"), true);
            return;
        }
        self.tool(crate::msg::Tool::Insert(Inline::Atom(
            nanatype_model::Atom {
                kind: nanatype_model::AtomKind::Image,
                asset,
                height_em: 2.0,
                alt: String::new(),
                effects: Default::default(),
            },
        )));
    }
}

/// The markup equivalent of a toolbar action (source mode).
pub(crate) fn markup_for(tool: &crate::msg::Tool) -> Option<MarkupEdit> {
    use crate::msg::Tool;
    let wrap = |open: String, close: &str| MarkupEdit::Wrap {
        open,
        close: close.to_owned(),
    };
    Some(match tool {
        Tool::Bold => wrap("[b]".into(), "[/b]"),
        Tool::Italic => wrap("[i]".into(), "[/i]"),
        Tool::Underline => wrap("[u]".into(), "[/u]"),
        Tool::Strike => wrap("[s]".into(), "[/s]"),
        Tool::Color(c) => wrap(format!("[color={c}]"), "[/color]"),
        Tool::Size(Some(nanatype_model::Size::Em(em))) => wrap(format!("[size={em}]"), "[/size]"),
        Tool::Size(Some(nanatype_model::Size::Px(px))) => wrap(format!("[size={px}px]"), "[/size]"),
        Tool::Size(None) => wrap("[size=inherit]".into(), "[/size]"),
        Tool::Font(font) => {
            let value = match font {
                None => "inherit".into(),
                Some(nanatype_model::FontSlot::Body) => "body".into(),
                Some(nanatype_model::FontSlot::Name) => "name".into(),
                Some(nanatype_model::FontSlot::Accent) => "accent".into(),
                Some(nanatype_model::FontSlot::Family(f)) => {
                    format!("\"{}\"", f.replace('\\', "\\\\").replace('\"', "\\\""))
                }
            };
            wrap(format!("[font={value}]"), "[/font]")
        }
        Tool::Align(align) => MarkupEdit::Align(*align),
        Tool::Stroke(Some(s)) => wrap(format!("[stroke={},{}]", s.color, s.width_px), "[/stroke]"),
        Tool::Shadow(Some(s)) => wrap(
            format!("[shadow={},{},{},{}]", s.color, s.dx, s.dy, s.blur),
            "[/shadow]",
        ),
        Tool::Ruby(r) => wrap(format!("[ruby={}]", quote(r)), "[/ruby]"),
        Tool::Effect(e) => {
            let mut open = format!("[{}", e.kind.key());
            if e.amp != 1.0 {
                open.push_str(&format!(" amp={}", e.amp));
            }
            if e.speed != 1.0 {
                open.push_str(&format!(" speed={}", e.speed));
            }
            open.push(']');
            wrap(open, &format!("[/{}]", e.kind.key()))
        }
        Tool::Emphasis(em) if *em == nanatype_model::Emphasis::default() => {
            wrap("[em]".into(), "[/em]")
        }
        Tool::Emphasis(em) => wrap(format!("[em={}]", em.ms), "[/em]"),
        Tool::Entrance(k, ms) => {
            let tag = match k {
                nanatype_model::EntranceKind::Fade => "fade",
                nanatype_model::EntranceKind::Pop => "pop",
                nanatype_model::EntranceKind::SlideUp => "slide",
                nanatype_model::EntranceKind::None => "noentrance",
            };
            let open = match ms {
                Some(ms) => format!("[{tag}={ms}]"),
                None => format!("[{tag}]"),
            };
            wrap(open, &format!("[/{tag}]"))
        }
        Tool::Pace(nanatype_model::Pace::Instant) => wrap("[instant]".into(), "[/instant]"),
        Tool::Pace(nanatype_model::Pace::Scale(s)) => wrap(format!("[speed={s}]"), "[/speed]"),
        Tool::Pace(nanatype_model::Pace::FixedMs(ms)) => {
            wrap(format!("[speed={ms}ms]"), "[/speed]")
        }
        Tool::Insert(inline) => {
            let doc = Document::from_inlines(vec![inline.clone()]);
            MarkupEdit::Insert(serialize(&doc))
        }
        Tool::ClearFormat => MarkupEdit::ClearFormat,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_clamp_to_char_boundaries() {
        assert_eq!(clamp_sel("你好", (1, 99)), (0, 6));
        assert_eq!(clamp_sel("abc", (2, 1)), (1, 2));
    }

    #[test]
    fn clearing_format_keeps_text_and_markers() {
        assert_eq!(
            strip_tags("[b]你[color=#ff0000]好[/color][/b][wait=300]"),
            "你好[wait=300]"
        );
    }

    #[test]
    fn quoting_only_when_needed() {
        assert_eq!(quote("cats/hi"), "cats/hi");
        assert_eq!(quote("file:C:/My Pics/a.png"), "\"file:C:/My Pics/a.png\"");
    }

    #[test]
    fn synchronized_source_edits_reject_images_without_changing_the_draft() {
        for tag in ["[img=blob:fixture]", "[sticker=cats/hello]", "[emote=wave]"] {
            for edit in [
                MarkupEdit::Insert(tag.into()),
                MarkupEdit::Typed {
                    text: tag.into(),
                    selection: (tag.len(), tag.len()),
                },
            ] {
                let mut source = crate::ui::Source {
                    text: "original".into(),
                    selection: (8, 8),
                    revision: 0,
                };
                assert!(
                    !apply_source_markup_guarded(&mut source, edit, DraftPolicy::PlainTextOnly),
                    "{tag}"
                );
                assert_eq!(source.text, "original");
                assert_eq!(source.selection, (8, 8));
                assert_eq!(source.revision, 1);
            }
        }
        let mut source = crate::ui::Source {
            text: "[emote=wave]".into(),
            ..Default::default()
        };
        assert!(apply_source_markup_guarded(
            &mut source,
            MarkupEdit::Typed {
                text: "text only".into(),
                selection: (9, 9)
            },
            DraftPolicy::PlainTextOnly
        ));
        assert_eq!(source.text, "text only");
    }

    #[test]
    fn source_alignment_targets_whole_paragraphs_and_preserves_meta_and_styles() {
        let mut src = crate::ui::Source {
            text: "[@auto=50][b]one[/b]\n[size=32px]two[/size]\n".into(),
            selection: (0, 25),
            revision: 0,
        };
        let boundary = src.text.find('\n').unwrap() + 1;
        src.selection = (1, boundary);
        let before = parse(&src.text);
        apply_source_markup(
            &mut src,
            MarkupEdit::Align(Some(nanatype_model::ParagraphAlign::Center)),
        );
        let after = parse(&src.text);
        assert_eq!(after.meta, before.meta);
        assert_eq!(
            after.doc.paragraphs[0].align,
            Some(nanatype_model::ParagraphAlign::Center)
        );
        assert_eq!(after.doc.paragraphs[1], before.doc.paragraphs[1]);
        apply_source_markup(&mut src, MarkupEdit::Align(None));
        assert_eq!(parse(&src.text).doc, before.doc);
    }
}
