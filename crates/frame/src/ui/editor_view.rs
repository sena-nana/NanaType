//! The draft inside the dialogue box: the rich editor or the markup source,
//! drawn in the skin's text style. The input window wires its events to the
//! session; the skin preview shows it inert.

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, EntityRef, IntoView, entity_ref, widget};
use nana_ui::runtime::{RichTextEditor, RichTextEditorEvent, Stack, Text, TextArea, TextChanged};
use nana_ui_core::{LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec, SemanticColorRole};
use nanatype_i18n::{Arg, Locale};
use nanatype_model::{Marker, VtsAction};

use super::dialogue::Base;
use super::{I18N, Stage, kit};

/// What the user did in the draft, for the product to turn into a message.
#[derive(Debug, Clone)]
pub enum EditorEvent {
    /// A context menu was asked for over the rich editor or (`source`) the
    /// markup source.
    Context {
        source: bool,
        x: f32,
        y: f32,
        keyboard: bool,
    },
    Changed(nana_ui_core::RichText),
    /// The rich editor's selection moved (byte offsets).
    Selection(std::ops::Range<usize>),
    Paste,
    /// The markup source was typed in.
    Typed {
        text: String,
        selection: (usize, usize),
    },
}

pub fn marker_label(m: &Marker, locale: Locale) -> String {
    let t = |k: &str| I18N.t_in(locale, k);
    let tr = |k: &str, name: &str, v: Arg<'_>| I18N.tr_in(locale, k, &[(name, v)]);
    match m {
        Marker::Wait { ms } => tr("marker-wait", "ms", (*ms).into()),
        Marker::Pause { auto_ms: None } => t("marker-pause"),
        Marker::Pause { auto_ms: Some(ms) } => tr("marker-pause-auto", "ms", (*ms).into()),
        Marker::Page => t("marker-page"),
        Marker::Sound { sound, .. } => tr("marker-sound", "name", sound.as_str().into()),
        Marker::Expression { name, .. } => tr("marker-expression", "name", name.as_str().into()),
        Marker::Portrait { variant } => tr("marker-portrait", "name", variant.as_str().into()),
        Marker::ScreenShake { .. } => t("marker-shake"),
        Marker::Vts(action) => {
            let name = match action {
                VtsAction::Hotkey { hotkey } => hotkey,
                VtsAction::Expression { file, .. } => file,
                VtsAction::Param { name, .. } => name,
            };
            tr("marker-vts", "name", name.as_str().into())
        }
    }
}

/// The input editor as a still sample for a skin preview: it takes neither
/// focus nor input, so it can never edit or send a draft.
pub fn preview_editor(ui: Stage) -> AnyView {
    editor_area(ui, entity_ref(), entity_ref(), true, |_| None::<()>)
}

/// The draft inside the dialogue box: the rich editor, or the markup source.
/// Both are built once and shown by `source_mode`, so the editor keeps its
/// selection and undo history across toggles. `inert` makes both read-only.
pub fn editor_area<M: Send + 'static>(
    ui: Stage,
    editor: EntityRef<RichTextEditor>,
    source: EntityRef<TextArea>,
    inert: bool,
    send: impl Fn(EditorEvent) -> Option<M> + Clone + Send + Sync + 'static,
) -> AnyView {
    let text_base = move || {
        // Keep the rich document's skin-relative sizes and font roles intact;
        // plain mode changes only inherited presentation, never stored spans.
        let mut base = Base::editor(&ui.skin.get());
        if !ui.settings.with(|s| s.general.input_skin_preview) {
            base.shadow = None;
            base.stroke = None;
            base.weight = 400;
            base.letter_spacing = 0.0;
            base.align = nanatype_theme::TextAlign::Start;
            base.family = Some(crate::skin_theme::NANA_UI_FAMILIES[0].into());
        }
        base
    };
    let style = move || {
        let mut st = text_base().node_style();
        let preview = ui.settings.with(|s| s.general.input_skin_preview);
        if !preview {
            st.foreground = Some(SemanticColorRole::Text);
        }
        let l = Arc::make_mut(&mut st.layout);
        if !preview {
            l.color = None;
        }
        l.width = Some(LengthSpec::Fill);
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_grow = Some(1.0);
        // Held to the text area, so a long draft scrolls inside the editor
        // (which keeps the caret in view) instead of running off the box.
        l.flex_shrink = Some(1.0);
        l.min_height = Some(LengthSpec::Px(0.0));
        l.overflow_x = OverflowSpec::Hidden;
        l.overflow_y = OverflowSpec::Hidden;
        l.cursor = Some(nana_ui_core::CursorSpec::Text);
        st
    };
    // The placeholder is chrome, not a line: the theme's muted ink with none
    // of the skin's outline or shadow, so it cannot pass for typed text.
    let placeholder_style = move || {
        let mut base = text_base();
        base.shadow = None;
        base.stroke = None;
        let mut st = base.node_style();
        let l = Arc::make_mut(&mut st.layout);
        l.position = PositionSpec::Absolute;
        l.offset_left = Some(LengthSpec::Px(0.0));
        l.offset_right = Some(LengthSpec::Px(0.0));
        l.offset_top = Some(LengthSpec::Px(0.0));
        l.color = None;
        l.pointer_events = Some(PointerEventsSpec::None);
        st.foreground = Some(SemanticColorRole::Muted);
        st
    };
    widget(Stack::fill_column(0.0).with_layout(|l| {
        l.position = PositionSpec::Relative;
        l.flex_grow = Some(1.0);
        l.min_height = Some(LengthSpec::Px(0.0));
    }))
    .children((
        widget(Stack::fill_column(0.0).with_layout(|l| {
            l.flex_grow = Some(1.0);
            l.min_height = Some(LengthSpec::Px(0.0));
        }))
        .visible(move || !ui.source_mode.get())
        .children((
            widget(RichTextEditor::new(ui.editor_value.get_untracked()).disabled(inert))
                .entity_ref(editor)
                .on_cx({
                    let send = send.clone();
                    move |_, e: &nana_ui::runtime::SecondaryPress, cx| {
                        if let Some(m) = send(EditorEvent::Context {
                            source: false,
                            x: e.x,
                            y: e.y,
                            keyboard: e.keyboard,
                        }) {
                            cx.dispatch_program_all(m);
                        }
                    }
                })
                .bind(move |e: &mut RichTextEditor| {
                    // Rehydrate image objects in place. NanaUI preserves this
                    // editor's selection and undo stack when its value updates.
                    let urls = ui.asset_urls.get();
                    let look = Base::editor(&ui.skin.get()).look();
                    let value = super::rich::to_rich(
                        &ui.draft.get(),
                        &super::rich::Env {
                            look: &look,
                            urls: &urls,
                            editor: true,
                            locale: ui.locale.get(),
                        },
                    );
                    if e.value != value {
                        if e.value.text() != value.text() {
                            // Binding directly to `value` bypasses NanaUI's
                            // reconciliation: a cleared draft would retain an
                            // out-of-bounds caret and crash the next @ insertion.
                            let mut next = e.clone();
                            next.value = value;
                            nana_ui::runtime::ComponentView::reconcile(e, next);
                        } else {
                            // Image/style-only updates preserve IME preedit,
                            // typing attributes, selection and undo history.
                            e.value = value;
                        }
                    }
                    let st = style();
                    if e.style != st {
                        e.style = st;
                    }
                })
                .on_cx({
                    let send = send.clone();
                    move |_, e: &RichTextEditorEvent, cx| {
                        let event = match e {
                            RichTextEditorEvent::Changed(rich)
                                if !ui.editor_transaction.get_untracked() =>
                            {
                                EditorEvent::Changed(rich.clone())
                            }
                            RichTextEditorEvent::SelectionChanged { selection, attrs } => {
                                if ui.sel_attrs.with_untracked(|a| a != attrs) {
                                    ui.sel_attrs.set(attrs.clone());
                                }
                                EditorEvent::Selection(selection.clone())
                            }
                            RichTextEditorEvent::PasteRequested => EditorEvent::Paste,
                            _ => return,
                        };
                        if let Some(m) = send(event) {
                            cx.dispatch_program_all(m);
                        }
                    }
                }),
            widget(Text::new(ui.now_t("editor-placeholder")))
                .bind(move |t: &mut Text| {
                    let st = placeholder_style();
                    if t.style != st {
                        t.style = st;
                    }
                    let value = I18N.t_in(ui.locale.get(), "editor-placeholder");
                    if t.value != value {
                        t.value = value;
                    }
                })
                // IME preedit does not enter the draft until committed. Hide
                // the empty hint while composing so it cannot overlap preedit.
                .key("input-empty-hint")
                .visible(move || !ui.composing.get() && ui.draft.with(|d| d.is_blank())),
        )),
        widget(Stack::fill_column(0.0).with_layout(|l| {
            l.flex_grow = Some(1.0);
            l.min_height = Some(LengthSpec::Px(0.0));
        }))
        .visible(move || ui.source_mode.get())
        .children(
            widget({
                let mut area =
                    kit::text_area("").placeholder(ui.now_t("editor-source-placeholder"));
                area.disabled = inert;
                let l = Arc::make_mut(&mut area.style.layout);
                l.width = Some(LengthSpec::Fill);
                l.height = Some(LengthSpec::Percent(100.0));
                // Markup source reads in the mono role.
                super::mono(l);
                area
            })
            .entity_ref(source)
            .on_cx({
                let send = send.clone();
                move |_, e: &nana_ui::runtime::SecondaryPress, cx| {
                    if let Some(m) = send(EditorEvent::Context {
                        source: true,
                        x: e.x,
                        y: e.y,
                        keyboard: e.keyboard,
                    }) {
                        cx.dispatch_program_all(m);
                    }
                }
            })
            .bind(move |a: &mut TextArea| {
                let completions = ui.source_completions.get();
                if a.completions != completions {
                    a.completions = completions;
                }
                let source = ui.source.get();
                if a.state.value.as_str() != source.text {
                    a.state.replace_value(source.text);
                    a.state.selection = nana_ui::runtime::TextSelection::new(
                        source.selection.0,
                        source.selection.1,
                    );
                }
            })
            .on_cx(move |_, e: &TextChanged, cx| {
                if let Some(m) = send(EditorEvent::Typed {
                    text: e.value.to_string(),
                    selection: (e.selection.anchor, e.selection.focus),
                }) {
                    cx.dispatch_program_all(m);
                }
            }),
        ),
    ))
    .into_any()
}
