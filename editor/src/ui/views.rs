//! Shared pieces of the editor's views, and the launcher. The editor itself
//! is laid out in [`super::shell`]; each workspace lives in [`super::modes`].
use crate::msg::{FileTarget, Msg};
use crate::session::{Action, EditorView};
use crate::ui::Ui;
use crate::ui::controls::{confirm, hint, label};
use crate::ui::kit::{self, Type, gap};
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, signal, watch_effect, widget};
use nana_ui::runtime::{
    Activate, Button, Select, SelectChanged, SelectOption, Stack, Text, TextChanged, ViewContext,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, LengthSpec};
use nanatype_theme::editor::ProjectKind;
use std::sync::Arc;

/// The least room the source editor keeps.
pub(crate) const SOURCE_MIN_H: f32 = 160.0;
/// A form's multi-line value, and a short note.
pub(crate) const FORM_TEXT_H: f32 = 72.0;
pub(crate) const NOTE_H: f32 = 56.0;

pub(crate) type Search = (Signal<String>, Signal<usize>);

/// A button handler that sends one editor action.
pub(crate) fn send(
    action: impl Fn() -> Action + Send + 'static,
) -> impl FnMut(&mut Button, &Activate, &mut ViewContext<'_, Button>) + Send + 'static {
    move |_, _, cx| cx.dispatch_program_all(Msg::PackageEditor(action()))
}

/// Update a field changed through a picker/template without resetting live typing.
pub(crate) fn sync_text(state: &mut nana_ui::runtime::TextInputState, value: String) {
    if state.value.as_str() != value {
        *state = nana_ui::runtime::TextInputState::new(value);
    }
}

/// A form value belongs to the session, including invalid text and hidden panels.
pub(crate) fn buffer<T>(ui: Ui, key: impl Into<String>, initial: T) -> Signal<T>
where
    T: Clone + PartialEq + serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
{
    let key = key.into();
    let view = view_of(ui);
    let session = view.as_ref().map_or(0, |v| v.session);
    let restored = view
        .and_then(|v| v.forms.get(&key).cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(|| initial.clone());
    let value = signal(restored.clone());
    let mut previous = restored;
    watch_effect(move || {
        let current = value.get();
        if current != previous {
            previous = current.clone();
            if let Some(post) = ui.post.get_untracked() {
                post(Action::Buffer {
                    session,
                    key: key.clone(),
                    value: if current == initial {
                        None
                    } else {
                        serde_json::to_value(current).ok()
                    },
                });
            }
        }
    });
    value
}

/// `YYYY-MM-DD HH:MM` in local time.
pub(crate) fn stamp(unix: u64) -> String {
    let t = unix as i64 + nanatype_frame::localtime::offset_seconds();
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        secs / 60 % 60
    )
}

/// Node keys are `/`-separated paths, so file paths inside them need escaping.
pub(crate) fn node_key(prefix: &str, path: &str) -> String {
    format!("{prefix}:{}", path.replace('/', "|"))
}

/// Keep a control at its natural width inside a stretching column.
pub(crate) fn natural(view: impl IntoView) -> AnyView {
    widget(Stack::row(0.)).children(view).into_any()
}

/// A text field up to `max` wide that shrinks with a narrow window.
pub(crate) fn input_box(max: f32, input: impl IntoView) -> AnyView {
    widget(
        Stack::row(0.)
            .width(LengthSpec::Px(max))
            .min_width(LengthSpec::Px(0.))
            .shrink(1.)
            // Wrapped rows do not shrink their items: cap at the line instead.
            .with_layout(|l| l.max_width = Some(LengthSpec::Percent(100.))),
    )
    .children(input)
    .into_any()
}

pub(crate) fn button(ui: Ui, key: &str) -> Button {
    Button::new(ui.now_t(key)).size(ControlSize::Small)
}

/// Wrapping, selectable-looking text that never widens its column.
pub(crate) fn readable(value: impl Into<String>) -> Text {
    let mut text = Text::new(value).style(Type::Caption.style());
    let layout = Arc::make_mut(&mut text.style.layout);
    layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    layout.white_space = nana_ui_core::WhiteSpaceSpec::PreWrap;
    text
}

pub(crate) fn kind_key(kind: ProjectKind) -> &'static str {
    match kind {
        ProjectKind::Skin => "pkgedit-kind-skin",
        ProjectKind::Resources => "pkgedit-kind-resources",
        ProjectKind::Lua => "pkgedit-kind-lua",
        ProjectKind::Sounds => "pkgedit-kind-sounds",
        ProjectKind::Mixed => "pkgedit-kind-mixed",
    }
}

pub(crate) fn view_of(ui: Ui) -> Option<Arc<EditorView>> {
    ui.package_editor.get_untracked()
}

// ---- launcher ----------------------------------------------------------------

/// Cards in the launcher's grids.
pub(crate) const CARD_W: f32 = 248.0;

/// A launcher card: a glyph, a title over a caption, and its actions.
pub(crate) fn card(
    key: String,
    glyph: nana_ui::runtime::Icon,
    title: String,
    caption: String,
    actions: impl IntoView,
) -> AnyView {
    widget(
        kit::panel()
            .width(LengthSpec::Px(CARD_W))
            .gap_step(gap::step::CONTROL),
    )
    .key(key)
    .children((
        widget(
            Stack::row(0.)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center),
        )
        .children((
            kit::icon_tile(glyph, kit::icon::LG, kit::Tone::Neutral),
            widget(kit::grow_column(gap::HAIR)).children((
                widget(Type::Strong.text(title).truncating()),
                widget(Type::Caption.text(caption).truncating()),
            )),
        )),
        widget(Stack::row(0.).gap_step(gap::step::TIGHT).wrap(true)).children(actions),
    ))
    .into_any()
}

/// A wrapping grid of launcher cards.
pub(crate) fn grid(cards: Vec<AnyView>) -> AnyView {
    widget(
        Stack::row(0.)
            .gap_step(gap::step::CONTROL)
            .wrap(true)
            .width(LengthSpec::Fill),
    )
    .children(cards)
    .into_any()
}

/// New and open, and any recovery snapshots waiting.
pub fn launcher(ui: Ui) -> AnyView {
    let kind = signal(ProjectKind::Mixed);
    let id = signal(String::new());
    let kinds = ProjectKind::ALL
        .iter()
        .map(|k| SelectOption::new(k.key(), ui.now_t(kind_key(*k))))
        .collect::<Vec<_>>();
    let open_button = |key: &str, glyph, target: FileTarget, widget_key: &'static str| {
        widget(
            Button::new(ui.now_t(key))
                .icon(glyph)
                .kind(ButtonKind::Ghost)
                .size(ControlSize::Small),
        )
        .key(widget_key)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::PickFile(target)))
        .into_any()
    };
    widget(kit::panel().gap_step(gap::step::CONTROL))
        .key("package-editor-launcher")
        .children((
            label(ui.now_t("pkgedit-title")),
            widget(readable(ui.now_t("pkgedit-intro"))),
            widget(
                Select::new(Some(ProjectKind::Mixed.key()))
                    .options(kinds)
                    .size(ControlSize::Small),
            )
            .key("package-editor-kind")
            .on(move |e: &SelectChanged| {
                if let Some(k) = ProjectKind::ALL.into_iter().find(|k| k.key() == &*e.value) {
                    kind.set(k);
                }
            }),
            widget(
                kit::text_input("")
                    .placeholder(ui.now_t("pkgedit-id-placeholder"))
                    .size(ControlSize::Small),
            )
            .key("package-editor-id")
            .on(move |e: &TextChanged| id.set(e.value.to_string())),
            natural(
                widget(button(ui, "pkgedit-new").kind(ButtonKind::Primary))
                    .key("package-editor-new")
                    .bind(move |b: &mut Button| {
                        b.disabled = id.with(|i| i.trim().is_empty());
                    })
                    .on_cx(send(move || Action::New {
                        kind: kind.get_untracked(),
                        id: id.get_untracked(),
                    })),
            ),
            kit::rule(),
            natural(open_button(
                "pkgedit-open-file",
                nana_ui::icons_tabler::PACKAGE,
                FileTarget::EditPackage,
                "editor-open-package",
            )),
            natural(open_button(
                "pkgedit-open-directory",
                nana_ui::icons_tabler::FOLDER_OPEN,
                FileTarget::OpenDirectory,
                "editor-open-directory",
            )),
            dynamic(
                move || ui.recoveries.get(),
                move |rows| {
                    widget(Stack::column(0.).gap_step(gap::step::CONTROL))
                        .key("editor-recoveries")
                        .children(
                            rows.iter()
                                .map(|r| {
                                    let id = r.id.clone();
                                    let discard = id.clone();
                                    widget(kit::panel().gap_step(gap::step::TIGHT))
                                        .children((
                                            widget(readable(format!(
                                                "{} · {}",
                                                ui.now_t("pkgedit-recovery"),
                                                r.package
                                            ))),
                                            widget(Stack::row(0.).gap_step(gap::step::TIGHT))
                                                .children((
                                                    widget(
                                                        button(ui, "pkgedit-recover")
                                                            .kind(ButtonKind::Primary),
                                                    )
                                                    .on_cx(send(move || {
                                                        Action::RestoreRecovery(id.clone())
                                                    })),
                                                    widget(button(ui, "pkgedit-discard")).on_cx(
                                                        send(move || {
                                                            Action::DiscardRecovery(discard.clone())
                                                        }),
                                                    ),
                                                )),
                                        ))
                                        .into_any()
                                })
                                .collect::<Vec<_>>(),
                        )
                        .into_any()
                },
            ),
        ))
        .into_any()
}

/// Saved drafts, each to open or discard.
pub fn drafts(ui: Ui) -> AnyView {
    dynamic(
        move || ui.package_drafts.get(),
        move |drafts| {
            if drafts.is_empty() {
                return hint(ui.now_t("pkgedit-drafts-none"));
            }
            grid(
                drafts
                    .iter()
                    .map(|draft| {
                        let open = draft.id.clone();
                        let discard = draft.id.clone();
                        card(
                            format!("package-draft:{}", draft.id),
                            nana_ui::icons_tabler::PENCIL,
                            draft.package.clone(),
                            stamp(draft.saved_at),
                            (
                                widget(button(ui, "pkgedit-open"))
                                    .on_cx(send(move || Action::OpenDraft(open.clone()))),
                                confirm(
                                    *ui,
                                    ui.now_t("pkgedit-discard-question"),
                                    ui.now_t("pkgedit-discard"),
                                    move |armed| {
                                        widget(
                                            button(ui, "pkgedit-discard").kind(ButtonKind::Ghost),
                                        )
                                        .on(move |_: &Activate| armed.set(true))
                                        .into_any()
                                    },
                                    move || {
                                        Msg::PackageEditor(Action::DiscardDraft(discard.clone()))
                                    },
                                ),
                            ),
                        )
                    })
                    .collect(),
            )
        },
    )
    .into_any()
}

/// The per-package editor entries on the packages page.
pub fn package_actions(ui: Ui, id: &str, builtin: bool) -> AnyView {
    let edit = id.to_owned();
    let copy = id.to_owned();
    widget(Stack::row(0.0).gap_step(gap::step::CONTROL).wrap(true))
        .children((
            (!builtin).then(|| {
                widget(button(ui, "pkgedit-edit"))
                    .key(format!("package-edit:{id}"))
                    .on_cx(send(move || Action::OpenInstalled(edit.clone())))
            }),
            widget(button(ui, "pkgedit-copy"))
                .key(format!("package-copy:{id}"))
                .on_cx(send(move || Action::CopyInstalled(copy.clone()))),
        ))
        .into_any()
}

pub use super::shell::editor;
