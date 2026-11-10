use super::*;
use crate::msg::WindowMsg;

/// What is wrong with an action's binding, worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyIssue {
    /// Another action has the same chord.
    Duplicate(Shortcut),
    /// Another program owns the chord.
    Conflict,
    /// The chord would swallow typing, belongs to the system, the editor
    /// or the send key, or cannot be registered.
    Unsafe(Refusal),
}

impl KeyIssue {
    fn badge_key(self) -> &'static str {
        match self {
            Self::Duplicate(_) => "hotkeys-badge-dup",
            Self::Conflict => "hotkeys-badge-conflict",
            Self::Unsafe(Refusal::Types) => "hotkeys-badge-unsafe",
            Self::Unsafe(_) => "hotkeys-badge-reserved",
        }
    }

    fn tone(self) -> StatusTone {
        match self {
            Self::Unsafe(_) => StatusTone::Warning,
            _ => StatusTone::Danger,
        }
    }

    fn color(self) -> SemanticColorRole {
        match self {
            Self::Unsafe(_) => SemanticColorRole::Warning,
            _ => SemanticColorRole::Danger,
        }
    }

    fn message(self, locale: Locale, s: &Settings, of: Shortcut) -> String {
        match self {
            Self::Duplicate(other) => I18N.tr_in(
                locale,
                "hotkeys-conflict-dup",
                &[("action", action_name(locale, s, other).into())],
            ),
            Self::Conflict => I18N.t_in(locale, "hotkeys-conflict-os"),
            Self::Unsafe(refusal) => I18N.t_in(locale, of.refusal_key(refusal)),
        }
    }
}

/// What an action is called: its label, or a phrase's own label.
pub fn action_name(locale: Locale, s: &Settings, action: Shortcut) -> String {
    match action {
        Shortcut::Global(GlobalAction::Phrase(i)) => s
            .assets
            .phrases
            .get(i)
            .map(|p| p.label.trim().to_owned())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| I18N.t_in(locale, "assets-phrases")),
        Shortcut::Global(GlobalAction::Profile(i)) => s
            .hotkeys
            .profiles
            .get(i)
            .map(|p| p.name.trim().to_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| I18N.t_in(locale, "general-profiles")),
        Shortcut::Global(GlobalAction::Character(i)) => s
            .general
            .characters
            .get(i)
            .map(|c| c.name.trim().to_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                I18N.tr_in(
                    locale,
                    "general-character-unnamed",
                    &[("n", (i as u64 + 1).into())],
                )
            }),
        _ => I18N.t_in(locale, action.label_key()),
    }
}

pub(super) fn key_issue(
    s: &Settings,
    states: &HashMap<GlobalAction, HotkeyState>,
    action: Shortcut,
) -> Option<KeyIssue> {
    let mine = action.get(s)?;
    if let Some(other) = Shortcut::all_in(s).find(|o| *o != action && o.get(s) == Some(mine)) {
        return Some(KeyIssue::Duplicate(other));
    }
    let state = match action {
        Shortcut::Global(a) => states.get(&a),
        Shortcut::Input(_) => None,
    };
    if let Some(refusal) = action.refusal(mine) {
        return Some(KeyIssue::Unsafe(refusal));
    }
    match state {
        Some(HotkeyState::Conflict) => Some(KeyIssue::Conflict),
        Some(HotkeyState::Invalid) => Some(KeyIssue::Unsafe(Refusal::Types)),
        _ => None,
    }
}

pub(super) use crate::ui::META_LABEL;

/// Most keys a chord has: Ctrl, Shift, Alt, Win and the key.
pub(super) const CHORD_KEYS: usize = 5;

/// The `i`th key of a chord as a keycap, "+" before all but the first. The
/// caps are fixed nodes that follow the binding: a settings row measures
/// its control once, so the control must not be rebuilt.
pub(super) fn keycap(
    i: usize,
    parts: impl Fn() -> Vec<String> + Send + Copy + 'static,
    danger: impl Fn() -> bool + Send + Copy + 'static,
) -> AnyView {
    let shown = move || parts().len() > i;
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center),
    )
    .visible(shown)
    .children((
        (i > 0).then(|| widget(crate::ui::kit::keycap::plus())),
        widget(crate::ui::kit::keycap::face())
            .border(move || {
                Some(if danger() {
                    SemanticColorRole::Danger
                } else {
                    SemanticColorRole::BorderSoft
                })
            })
            .children(widget(crate::ui::kit::keycap::label(String::new())).bind(
                move |t: &mut Text| {
                    let text = parts().get(i).cloned().unwrap_or_default();
                    if t.value != text {
                        t.value = text;
                    }
                },
            )),
    ))
    .into_any()
}

pub(super) fn hotkey_control(ui: Ui, action: Shortcut) -> AnyView {
    let recording = move || ui.recording.get() == Some(action);
    let bound = move || ui.settings.with(|s| action.get(s).is_some());
    let default = action.default_binding();
    let changed = {
        let default = default.clone();
        move || default.is_some() && ui.settings.with(|s| action.get(s) != default.as_ref())
    };
    let issue = move || {
        let states = ui.hotkeys.get();
        ui.settings.with(|s| key_issue(s, &states, action))
    };
    let parts = move || {
        ui.settings.with(|s| {
            action.get(s).map_or_else(Vec::new, |h| {
                h.label(META_LABEL).split('+').map(str::to_owned).collect()
            })
        })
    };
    let danger = move || issue().is_some();
    let badge = move || {
        let locale = ui.locale.get();
        if !bound() {
            return None;
        }
        match issue() {
            Some(i) => Some((I18N.t_in(locale, i.badge_key()), i.tone())),
            None => match action {
                Shortcut::Global(a) => ui
                    .hotkeys
                    .with(|h| matches!(h.get(&a), Some(HotkeyState::Registered)))
                    .then(|| (I18N.t_in(locale, "hotkeys-ok"), StatusTone::Success)),
                Shortcut::Input(_) => None,
            },
        }
    };
    let caps: Vec<AnyView> = (0..CHORD_KEYS).map(|i| keycap(i, parts, danger)).collect();
    let slot = widget(
        Stack::row(0.0)
            .height(LengthSpec::Px(crate::ui::kit::keycap::H))
            .padding_xy(space::LG, 0.0)
            .align(AlignSpec::Center)
            .border(SemanticColorRole::Border, 1.0)
            .radius(nana_ui_core::RadiusTier::Sm)
            .with_layout(|l| l.border_style = Some(nana_ui_core::BorderStyle::Dashed)),
    )
    .children(widget(
        Text::new(ui.now_t("hotkeys-click-to-set"))
            .style(Type::Small.style())
            .color(SemanticColorRole::Muted),
    ));
    // Recording is a live state: the highlight border, not the accent.
    let listening = widget(
        Stack::row(0.0)
            .min_width(LengthSpec::Px(field::SM))
            .height(LengthSpec::Px(crate::ui::kit::keycap::H))
            .justify(JustifySpec::Center)
            .align(AlignSpec::Center)
            .border(SemanticColorRole::Highlight, 1.0)
            .radius(nana_ui_core::RadiusTier::Sm),
    )
    .children(widget(
        Text::new(ui.now_t("hotkeys-press-keys"))
            .style(Type::Small.style())
            .color(SemanticColorRole::Text),
    ));
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center),
    )
    .children((
        widget(StatusBadge::new("", StatusTone::Neutral).compact(true))
            .visible(move || badge().is_some())
            .bind(move |b: &mut StatusBadge| {
                if let Some((text, tone)) = badge() {
                    let text: Arc<str> = text.into();
                    if b.label != text {
                        b.label = text;
                    }
                    b.tone = tone;
                }
            }),
        widget(
            Button::new("")
                .kind(ButtonKind::Ghost)
                .size(ControlSize::Small)
                .accessible_name(ui.now_t("hotkeys-record")),
        )
        .bind(move |b: &mut Button| {
            b.kind = if recording() {
                ButtonKind::Selected
            } else {
                ButtonKind::Ghost
            };
        })
        .on_cx(move |_, _: &Activate, cx| {
            let recording = ui.recording.get_untracked() == Some(action);
            cx.dispatch_program_all(Msg::RecordHotkey((!recording).then_some(action)))
        })
        .children((
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center),
            )
            .visible(move || bound() && !recording())
            .children(caps),
            slot.visible(move || !bound() && !recording()),
            listening.visible(recording),
        )),
        widget(
            IconButton::new(ti::X, ui.now_t("hotkeys-clear-binding"))
                .size(ControlSize::Small)
                .with_tooltip(ui.now_t("hotkeys-clear-binding")),
        )
        .visible(bound)
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(edit(move |s| action.set(s, None)))
        }),
        widget(
            IconButton::new(ti::ARROW_BACK_UP, ui.now_t("hotkeys-reset"))
                .size(ControlSize::Small)
                .with_tooltip(ui.now_t("hotkeys-reset")),
        )
        .visible(changed)
        .on_cx(move |_, _: &Activate, cx| {
            let default = default.clone();
            cx.dispatch_program_all(edit(move |s| action.bind(s, default)))
        }),
    ))
    .into_any()
}

/// The wrapping line under a hotkey row that says what is wrong.
pub(super) fn hotkey_issue_line(ui: Ui, action: Shortcut) -> AnyView {
    issue_line(ui, action, true)
}

/// [`hotkey_issue_line`] without the row's inset, under a compact recorder.
pub(super) fn hotkey_issue_caption(ui: Ui, action: Shortcut) -> AnyView {
    issue_line(ui, action, false)
}

fn issue_line(ui: Ui, action: Shortcut, inset_row: bool) -> AnyView {
    let issue = move || {
        let states = ui.hotkeys.get();
        ui.settings.with(|s| key_issue(s, &states, action))
    };
    widget(Stack::column(0.0).with_layout(move |l| {
        if inset_row {
            l.padding_left = Some(LengthSpec::Px(inset::CARD));
            l.padding_right = Some(LengthSpec::Px(inset::CARD));
            l.padding_bottom = Some(LengthSpec::Px(space::XL));
        }
    }))
    .visible(move || issue().is_some())
    .children(
        widget(Text::new(String::new()).style(Type::Caption.style())).bind(move |t: &mut Text| {
            if let Some(i) = issue() {
                let text = ui.settings.with(|s| i.message(ui.locale.get(), s, action));
                if t.value != text {
                    t.value = text;
                }
                t.style.foreground = Some(i.color());
            }
        }),
    )
    .into_any()
}

/// A shortcut's row: its name and recorder, then what is wrong with it.
fn shortcut_row(ui: Ui, label: String, action: Shortcut) -> Row {
    Row::inset(
        widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xs)).children((
            row_text(label, None, hotkey_control(ui, action)),
            hotkey_issue_line(ui, action),
        )),
    )
}

/// One row per quick phrase; renaming or adding one on the Assets page
/// rebuilds the list.
fn phrase_rows(ui: Ui) -> Row {
    Row::inset(dynamic(
        move || {
            ui.settings.with(|s| {
                s.assets
                    .phrases
                    .iter()
                    .map(|p| p.label.clone())
                    .collect::<Vec<_>>()
            })
        },
        move |labels| {
            if labels.is_empty() {
                return block(hint(ui.now_t("hotkeys-phrases-empty")));
            }
            let rows: Vec<Row> = (0..labels.len())
                .map(|i| {
                    let action = Shortcut::Global(GlobalAction::Phrase(i));
                    let name = ui
                        .settings
                        .with_untracked(|s| action_name(ui.locale.get_untracked(), s, action));
                    shortcut_row(ui, name, action)
                })
                .collect();
            Row::inset(widget(Stack::column(0.0)).children(divided(rows)))
        },
    ))
}

/// Switching characters: next and previous, then one row per character;
/// renaming or adding one in Settings › Characters rebuilds the list.
fn character_rows(ui: Ui) -> Row {
    Row::inset(dynamic(
        move || {
            ui.settings.with(|s| {
                s.general
                    .characters
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>()
            })
        },
        move |names| {
            let rows: Vec<Row> = CHARACTER_CYCLE
                .into_iter()
                .chain((0..names.len()).map(GlobalAction::Character))
                .map(|action| {
                    let action = Shortcut::Global(action);
                    let name = ui
                        .settings
                        .with_untracked(|s| action_name(ui.locale.get_untracked(), s, action));
                    shortcut_row(ui, name, action)
                })
                .collect();
            Row::inset(widget(Stack::column(0.0)).children(divided(rows)))
        },
    ))
}

/// The fixed actions listed with the characters' own keys, not with the
/// other global hotkeys.
const CHARACTER_CYCLE: [GlobalAction; 2] =
    [GlobalAction::NextCharacter, GlobalAction::PrevCharacter];

/// One row per saved profile; saving, renaming or deleting one in
/// Settings › General rebuilds the list.
fn profile_rows(ui: Ui) -> Row {
    Row::inset(dynamic(
        move || {
            ui.settings.with(|s| {
                s.hotkeys
                    .profiles
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>()
            })
        },
        move |names| {
            if names.is_empty() {
                return block(hint(ui.now_t("hotkeys-profiles-empty")));
            }
            let rows: Vec<Row> = (0..names.len())
                .map(|i| {
                    let action = Shortcut::Global(GlobalAction::Profile(i));
                    let name = ui
                        .settings
                        .with_untracked(|s| action_name(ui.locale.get_untracked(), s, action));
                    shortcut_row(ui, name, action)
                })
                .collect();
            Row::inset(widget(Stack::column(0.0)).children(divided(rows)))
        },
    ))
}

pub(super) fn hotkeys(ui: Ui) -> AnyView {
    let rows = |actions: Vec<Shortcut>| -> Vec<Row> {
        actions
            .into_iter()
            .map(|action| shortcut_row(ui, ui.now_t(action.label_key()), action))
            .collect()
    };
    let global = rows(
        GlobalAction::ALL
            .into_iter()
            .filter(|a| !CHARACTER_CYCLE.contains(a))
            .map(Shortcut::Global)
            .collect(),
    );
    let input = |chat: bool| {
        rows(
            InputKey::ALL
                .into_iter()
                .filter(|k| k.is_chat() == chat)
                .map(Shortcut::Input)
                .collect(),
        )
    };
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(ui, "hotkeys-global", global),
            section(ui, "hotkeys-local", input(false)),
            section(ui, "hotkeys-chat", input(true)),
            jump_section(
                ui,
                "hotkeys-phrases",
                "hotkeys-edit-phrases",
                Page::Assets,
                phrase_rows(ui),
            ),
            jump_section(
                ui,
                "hotkeys-characters",
                "hotkeys-edit-characters",
                Page::Characters,
                character_rows(ui),
            ),
            jump_section(
                ui,
                "hotkeys-profiles",
                "hotkeys-edit-profiles",
                Page::Profiles,
                profile_rows(ui),
            ),
        ))
        .into_any()
}

/// A group of hotkeys for things edited on another page: Edit on its title
/// opens that page.
fn jump_section(ui: Ui, title_key: &str, edit_key: &str, page: Page, rows: Row) -> AnyView {
    section_tools(
        ui,
        title_key,
        vec![tool(ui, ti::EDIT, edit_key, move || {
            Msg::Window(WindowMsg::SettingsPage(page))
        })],
        vec![(rows, None)],
    )
}
