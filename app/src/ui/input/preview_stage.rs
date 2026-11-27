use crate::msg::HostMsg;

use super::toolbar::toolbar;
use super::*;

/// The quoted viewer's avatar on the reply bar.
const REPLY_AVATAR: f32 = 18.0;

pub(super) fn status_content(ui: Ui) -> AnyView {
    enum Line {
        Error(String),
        Queued(String),
        Empty,
    }
    // Notices show as toasts above this line.
    let key = move || {
        let queued = ui.queued.get();
        let diag = ui.diagnostics.with(|d| d.first().cloned());
        (diag, queued, ui.locale.get())
    };
    let line = dynamic(key, move |k| {
        let (diag, queued, locale) = k.clone();
        let line = if let Some(d) = diag {
            Line::Error(d)
        } else if queued > 0 {
            Line::Queued(I18N.tr_in(locale, "status-queued-chip", &[("position", queued.into())]))
        } else {
            Line::Empty
        };
        match line {
            Line::Error(text) => widget(
                Text::new(text)
                    .style(Type::Small.style())
                    .truncating()
                    .color(SemanticColorRole::Danger),
            )
            .into_any(),
            // Live and attention states take the highlight, not the accent.
            Line::Queued(text) => crate::ui::kit::badge(text, Tone::Accent),
            Line::Empty => widget(Stack::row(0.0)).into_any(),
        }
    })
    // A chip or an error arrives on the `enter` token.
    .transition(super::super::motion::enter());
    widget(Stack::bar(0.0).align(AlignSpec::Center).with_layout(|l| {
        l.width = Some(LengthSpec::Px(0.0));
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_grow = Some(1.0);
    }))
    .key("input-status")
    .children((
        widget(Stack::row(0.0).with_layout(|l| {
            l.width = Some(LengthSpec::Px(0.0));
            l.flex_grow = Some(1.0);
            l.min_width = Some(LengthSpec::Px(0.0));
        }))
        .visible(move || ui.reply.with(Option::is_none))
        .children(line),
        widget(Stack::spacer())
            .visible(move || ui.reply.with(Option::is_some))
            .children(reply_bar(ui)),
    ))
    .into_any()
}

/// "Replying to": the quoted chat message the next line answers, with a
/// button that drops it.
fn reply_bar(ui: Ui) -> AnyView {
    dynamic(
        move || (ui.reply.get(), ui.chrome()),
        move |(reply, _)| {
            let Some(reply) = reply else {
                return widget(Stack::row(0.0)).into_any();
            };
            let mut label = Type::Overline.style();
            {
                let l = Arc::make_mut(&mut label.layout);
                l.white_space = nana_ui_core::WhiteSpaceSpec::Nowrap;
                l.flex_shrink = Some(1.0);
                l.min_width = Some(LengthSpec::Px(0.0));
                l.max_width = Some(LengthSpec::Percent(45.0));
            }
            // The accent, darkened where it would not read as text.
            label.foreground = Some(SemanticColorRole::AccentOnSoft);
            let mut body = Type::Caption.style();
            body.foreground = Some(SemanticColorRole::Muted);
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill),
            )
            .children((
                widget(
                    Text::new(I18N.tr_in(
                        ui.locale.get_untracked(),
                        "chat-replying",
                        &[("name", reply.user.name.as_str().into())],
                    ))
                    .style(label)
                    .truncating(),
                ),
                super::super::danmaku::asset_avatar(
                    *ui,
                    reply.user.avatar.as_ref().map(ToString::to_string),
                    &reply.user.name,
                    REPLY_AVATAR,
                    crate::ui::kit::theme::palette(ui.chrome())
                        .accent
                        .as_rgba_array(),
                ),
                widget(Stack::row(0.0).with_layout(|l| {
                    l.flex_grow = Some(1.0);
                    l.flex_shrink = Some(1.0);
                    l.min_width = Some(LengthSpec::Px(0.0));
                }))
                .children(widget(
                    Text::new(reply.text.clone()).style(body).truncating(),
                )),
                widget(
                    IconButton::new(ti::X, ui.now_t("chat-reply-cancel"))
                        .size(ControlSize::Small)
                        .with_tooltip(ui.now_t("chat-reply-cancel")),
                )
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(Msg::Chat(ChatMsg::ClearReply))
                }),
            ))
            .into_any()
        },
    )
    .into_any()
}

/// The draft inside the dialogue box, its events sent to the session.
fn editor_area(
    ui: Ui,
    editor: EntityRef<RichTextEditor>,
    source: EntityRef<TextArea>,
    inert: bool,
) -> AnyView {
    editor_view::editor_area(*ui, editor, source, inert, move |event| {
        Some(match event {
            EditorEvent::Context {
                source,
                x,
                y,
                keyboard,
            } => Msg::Host(HostMsg::EditorContext {
                source,
                x,
                y,
                keyboard,
            }),
            EditorEvent::Changed(rich) => {
                ui.draft_fence.with_untracked(|f| f.bump_version());
                Msg::Editor(EditorMsg::Changed(rich))
            }
            EditorEvent::Selection(selection) => Msg::Editor(EditorMsg::Selection(selection)),
            EditorEvent::Paste => Msg::Editor(EditorMsg::Paste),
            EditorEvent::Typed { text, selection } => {
                ui.draft_fence.with_untracked(|f| f.bump_version());
                Msg::Editor(EditorMsg::Markup(MarkupEdit::Typed { text, selection }))
            }
        })
    })
}

/// The input window. Returns the editor and the source field for key
/// handling, and the stack the host shows toasts in.
pub fn input_window(ui: Ui) -> WithRefs<impl IntoView, (EditorRef, SourceRef, ToastsRef)> {
    let editor = entity_ref::<RichTextEditor>();
    let source = entity_ref::<TextArea>();
    let toasts = entity_ref::<nana_ui::runtime::ToastStack>();
    let viewport = signal((0.0_f32, 0.0_f32));
    let is_preview = move || ui.settings.with(|s| s.general.input_skin_preview);
    let canvas_size = move || {
        if !is_preview() {
            return viewport.get();
        }
        let skin = ui.skin.get();
        let s = ui.settings.get();
        crate::windows::size_of(crate::windows::WindowKind::Display, &skin, &s)
    };
    // Fit only the preview presentation. The editor still lays out at the
    // stream canvas size, so resizing the sidebar cannot change line breaks.
    let preview_scale = move || {
        if !is_preview() {
            return 1.0;
        }
        let (w, _) = canvas_size();
        ((viewport.get().0 - 32.0).max(1.0) / w.max(1.0)).min(1.0)
    };
    let preview = widget(ScrollView::new(ScrollAxes::Both).with_layout(|l| {
        l.width = Some(LengthSpec::Fill);
        l.height = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_grow = Some(1.0);
        l.flex_shrink = Some(1.0);
    }))
    .key("input-preview-scroll")
    .bind(move |s: &mut ScrollView| {
        s.scrollbars = Some(if is_preview() {
            nana_ui_core::ScrollbarVisibility::Always
        } else {
            nana_ui_core::ScrollbarVisibility::Hidden
        });
    })
    .on(move |e: &ScrollViewportChanged| {
        let size = (e.width, e.height);
        if viewport.get_untracked() != size {
            viewport.set(size);
        }
    })
    .children(
        widget(
            Stack::column(0.0)
                .align(AlignSpec::Center)
                .justify(JustifySpec::Center),
        )
        .bind(move |s: &mut Stack| {
            let (w, h) = canvas_size();
            let (vw, vh) = viewport.get();
            let scale = preview_scale();
            let pad = if is_preview() { 32.0 } else { 0.0 };
            *s = s.clone().surface(if is_preview() {
                SemanticColorRole::Subtle
            } else {
                SemanticColorRole::Background
            });
            let l = s.layout_mut();
            l.width = Some(LengthSpec::Px(vw.max(w * scale + pad)));
            l.height = Some(LengthSpec::Px(vh.max(h * scale + pad)));
            l.flex_shrink = Some(0.0);
        })
        .children(
            widget(Stack::column(0.0).with_layout(|l| {
                l.position = PositionSpec::Relative;
                l.flex_shrink = Some(0.0);
                // The unscaled child must not enlarge the scrollable area.
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
            }))
            .key("input-preview-fit")
            .bind(move |s: &mut Stack| {
                let (w, h) = canvas_size();
                let scale = preview_scale();
                let l = s.layout_mut();
                l.width = Some(LengthSpec::Px(w * scale));
                l.height = Some(LengthSpec::Px(h * scale));
            })
            .children(
                widget(Stack::column(0.0).with_layout(|l| {
                    l.position = PositionSpec::Absolute;
                    l.offset_left = Some(LengthSpec::Px(0.0));
                    l.offset_top = Some(LengthSpec::Px(0.0));
                    l.flex_shrink = Some(0.0);
                }))
                .key("input-preview-canvas")
                .bind(move |s: &mut Stack| {
                    let (w, h) = canvas_size();
                    let l = s.layout_mut();
                    l.width = Some(LengthSpec::Px(w));
                    l.height = Some(LengthSpec::Px(h));
                    l.transforms.transform = is_preview()
                        .then(|| super::super::surface::affine(0.0, 0.0, 0.0, preview_scale()));
                    l.transforms.origin = Some(nana_ui_core::TransformOrigin {
                        x: LengthSpec::Px(0.0),
                        y: LengthSpec::Px(0.0),
                    });
                })
                .children(dialogue_box(
                    *ui,
                    Source::Draft,
                    Some(editor_area(ui, editor, source, false)),
                )),
            ),
        ),
    );
    let view = widget(
        Stack::fill_column(0.0)
            .surface(SemanticColorRole::Background)
            .with_layout(|l| {
                l.position = PositionSpec::Relative;
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
            }),
    )
    .key("input-window")
    .on(move |event: &SizeChanged| {
        if ui.input_width.get_untracked() != event.width {
            ui.input_width.set(event.width);
        }
        if ui.input_height.get_untracked() != event.height {
            ui.input_height.set(event.height);
        }
    })
    .children((
        workspace::split(
            ui,
            widget(Stack::fill_column(0.0).with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
            }))
            .children((
                toolbar(ui),
                crate::ui::kit::rule(),
                preview,
                super::super::plugins::mounted(ui, nanatype_plugins::Mount::Input),
            )),
        ),
        super::super::speech::status(ui),
        workspace::footer(ui),
        completion::popup(ui),
        super::super::feedback::toast_stack(toasts, crate::windows::INPUT_ACTIONS_H + 4.0, 2),
    ));
    with_refs(view, (editor, source, toasts))
}
