use std::collections::HashMap;
use std::sync::Arc;

use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, signal, widget};
use nana_ui::runtime::{
    ActionMenu, ActionMenuItem, Activate, Button, Chip, IconButton, IconGlyph, ListItem,
    PopoverClosed, PopoverToggled, ScrollAxes, ScrollView, Select, SelectChanged, SelectOption,
    SizeChanged, Stack, StatusBadge, Switch, Text, TextChanged, TextInput, ToggleChanged,
};
use nana_ui::theme::{space, type_scale};
use nana_ui::{ButtonKind, ControlSize, StatusTone};
use nana_ui_core::{
    AlignSpec, BackgroundImage, JustifySpec, LengthSpec, OverflowSpec, PointerEventsSpec,
    PositionSpec, SemanticColorRole, TransformOrigin,
};
use nanatype_i18n::Locale;
use nanatype_model::{AfterLine, EntranceKind, Overflow, VtsReset};
use nanatype_playback::QueuePolicy;
use nanatype_settings::{
    CaptureBackground, ChromeMode, HideLatest, HistorySync, InputMode, InputPresence, Lock,
    QuickPhrase, SendKey, Settings,
};

use super::Page;
use super::controls::*;
use crate::msg::{FileTarget, GlobalAction, Msg};
use crate::services::hotkeys::HotkeyState;
use crate::ui::dialogue::{self, BoxCtx, Source};
use crate::ui::{I18N, LinkState, SkinRow, Ui};
use crate::windows::WindowKind;

pub fn page(ui: Ui, page: Page) -> AnyView {
    let body = match page {
        Page::Bilibili => super::super::bilibili::settings_page(ui),
        Page::Douyin => super::super::douyin::settings_page(ui),
        Page::General => general(ui),
        Page::Playback => playback(ui),
        Page::Skins => skins(ui),
        Page::Windows => windows(ui),
        Page::History => history(ui),
        Page::Hotkeys => hotkeys(ui),
        Page::Output => output(ui),
        Page::Integrations => integrations(ui),
        Page::Assets => assets(ui),
        Page::About => about(ui),
    };
    widget(Stack::column(SECTION_GAP))
        .children((
            widget(Stack::column(space::XS)).children((
                heading(ui, page.label_key()),
                widget(
                    Text::new(ui.now_t(page.subtitle_key()))
                        .style(text_style(type_scale::BODY, type_scale::REGULAR))
                        .color(SemanticColorRole::Muted),
                ),
            )),
            body,
        ))
        .into_any()
}

fn button(ui: Ui, key: &str, msg: impl Fn() -> Msg + Send + 'static) -> AnyView {
    widget(Button::new(ui.now_t(key)).size(ControlSize::Small))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
        .into_any()
}

fn primary(ui: Ui, key: &str, msg: impl Fn() -> Msg + Send + 'static) -> AnyView {
    widget(
        Button::new(ui.now_t(key))
            .kind(ButtonKind::Primary)
            .size(ControlSize::Small),
    )
    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
    .into_any()
}

/// "Follow system" and every language by its own name.
fn language_options(ui: Ui) -> Vec<(String, String)> {
    let mut langs = vec![("system".to_owned(), ui.now_t("general-language-system"))];
    langs.extend(
        Locale::ALL
            .iter()
            .map(|l| (l.tag().to_owned(), l.native_name().to_owned())),
    );
    langs
}

/// The language select, bound to the setting.
fn language_select(ui: Ui) -> AnyView {
    select(
        ui,
        language_options(ui),
        |s| s.general.locale.clone().unwrap_or_else(|| "system".into()),
        |s, v| s.general.locale = (v != "system").then(|| v.to_owned()),
    )
}

/// The speaker-name field, bound to the setting.
fn speaker_field(ui: Ui) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(220.0))
            .with_layout(|l| {
                l.flex_shrink = Some(0.0);
            }),
    )
    .children(dynamic(
        move || {
            ui.settings.with(|s| s.integrations.bilibili.sync_profile)
                && matches!(
                    ui.bilibili.get().account,
                    nanatype_bilibili::AccountState::Ready(_)
                )
        },
        move |bound| {
            if *bound {
                dynamic(
                    move || ui.speaker.get().name,
                    move |name| {
                        widget(
                            Text::new(name.clone())
                                .style(text_style(type_scale::BODY, type_scale::REGULAR))
                                .truncating(),
                        )
                        .into_any()
                    },
                )
                .into_any()
            } else {
                text(
                    ui,
                    String::new(),
                    220.0,
                    |s| s.general.speaker_name.clone(),
                    |s, v| s.general.speaker_name = v,
                )
            }
        },
    ))
    .into_any()
}

fn general(ui: Ui) -> AnyView {
    let avatar_row = {
        let has = ui.settings.get_untracked().general.avatar.is_some();
        widget(Stack::row(8.0))
            .visible(move || {
                !(ui.settings.with(|s| s.integrations.bilibili.sync_profile)
                    && matches!(
                        ui.bilibili.get().account,
                        nanatype_bilibili::AccountState::Ready(_)
                    ))
            })
            .children((
                button(ui, "general-avatar-pick", || {
                    Msg::PickFile(FileTarget::Avatar)
                }),
                has.then(|| {
                    button(ui, "general-avatar-clear", || {
                        edit(|s| s.general.avatar = None)
                    })
                }),
            ))
            .into_any()
    };
    let guide = (!ui.settings.get_untracked().general.onboarding_done).then(|| onboarding(ui));
    widget(Stack::column(SECTION_GAP))
        .children((
            guide,
            section(
                ui,
                "",
                vec![
                    row(ui, "general-language", None, language_select(ui)),
                    row(
                        ui,
                        "general-appearance",
                        None,
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("system", "general-appearance-system"),
                                    ("light", "general-appearance-light"),
                                    ("dark", "general-appearance-dark"),
                                ],
                            ),
                            |s| match s.skin.chrome {
                                ChromeMode::System => "system".into(),
                                ChromeMode::Light => "light".into(),
                                ChromeMode::Dark => "dark".into(),
                            },
                            |s, v| {
                                s.skin.chrome = match v {
                                    "light" => ChromeMode::Light,
                                    "dark" => ChromeMode::Dark,
                                    _ => ChromeMode::System,
                                }
                            },
                        ),
                    ),
                    row(
                        ui,
                        "general-speaker",
                        Some("general-speaker-hint"),
                        speaker_field(ui),
                    ),
                    row(ui, "general-avatar", None, avatar_row),
                    super::super::bilibili::identity_hint(ui),
                ],
            ),
            section_with(
                ui,
                "general-input",
                vec![
                    (
                        row(
                            ui,
                            "general-presence",
                            Some("general-presence-hint"),
                            segmented(
                                ui,
                                select_keys(
                                    ui,
                                    &[
                                        ("resident", "general-presence-resident"),
                                        ("summon", "general-presence-summon"),
                                    ],
                                ),
                                |s| match s.general.input_presence {
                                    InputPresence::Resident => "resident".into(),
                                    InputPresence::Summon => "summon".into(),
                                },
                                |s, v| {
                                    s.general.input_presence = if v == "summon" {
                                        InputPresence::Summon
                                    } else {
                                        InputPresence::Resident
                                    }
                                },
                            ),
                        ),
                        None,
                    ),
                    (
                        row_live_hint(
                            ui,
                            "general-mode",
                            move || {
                                Some(I18N.t_in(
                                    ui.locale.get(),
                                    if mirror(ui) {
                                        "mode-mirror-hint"
                                    } else {
                                        "mode-compose-hint"
                                    },
                                ))
                            },
                            segmented(
                                ui,
                                select_keys(
                                    ui,
                                    &[("compose", "mode-compose"), ("mirror", "mode-mirror")],
                                ),
                                |s| match s.general.input_mode {
                                    InputMode::Compose => "compose".into(),
                                    InputMode::Mirror => "mirror".into(),
                                },
                                |s, v| {
                                    s.general.input_mode = if v == "mirror" {
                                        InputMode::Mirror
                                    } else {
                                        InputMode::Compose
                                    }
                                },
                            ),
                        ),
                        None,
                    ),
                    shown_if(
                        row(
                            ui,
                            "playback-stagger",
                            None,
                            slider(
                                ui,
                                (0.0, 120.0, 5.0),
                                "ms",
                                |s| f64::from(s.playback.mirror_stagger_ms),
                                |s, v| s.playback.mirror_stagger_ms = v as u32,
                            ),
                        ),
                        move || mirror(ui),
                    ),
                    (
                        row(
                            ui,
                            "general-send-key",
                            Some("general-send-key-hint"),
                            select(
                                ui,
                                select_keys(
                                    ui,
                                    &[
                                        ("enter", "general-send-key-enter"),
                                        ("ctrl", "general-send-key-ctrl-enter"),
                                    ],
                                ),
                                |s| match s.general.send_key {
                                    SendKey::Enter => "enter".into(),
                                    SendKey::CtrlEnter => "ctrl".into(),
                                },
                                |s, v| {
                                    s.general.send_key = if v == "ctrl" {
                                        SendKey::CtrlEnter
                                    } else {
                                        SendKey::Enter
                                    }
                                },
                            ),
                        ),
                        None,
                    ),
                    (
                        row(
                            ui,
                            "general-autostart",
                            None,
                            switch(
                                ui,
                                |s| s.general.launch_at_login,
                                |s, v| s.general.launch_at_login = v,
                            ),
                        ),
                        None,
                    ),
                ],
            ),
        ))
        .into_any()
}

/// How many steps the first-run guide has.
const ONBOARD_STEPS: u8 = 4;

/// The first-run guide: one card marked by a 3px accent bar, four steps
/// (language and name, a skin with its live preview, the input mode, the
/// hotkeys and OBS) with Back / Next and a step indicator. Skip or the last
/// step's "Get started" closes it for good; About opens it again.
fn onboarding(ui: Ui) -> AnyView {
    let step = ui.onboard_step;
    let finish = move || {
        step.set(0);
        edit(|s| s.general.onboarding_done = true)
    };
    let header = widget(
        Stack::row(space::MD)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .padding_xy(INSET, 0.0),
    )
    .children((
        widget(
            IconGlyph::new(ti::SPARKLES)
                .size(16.0)
                .role(SemanticColorRole::Accent),
        ),
        widget(
            Text::new(ui.now_t("onboard-welcome"))
                .style(text_style(type_scale::HEADING, type_scale::SEMIBOLD)),
        ),
        widget(Stack::row(0.0).grow(1.0)),
        step_indicator(ui),
        widget(
            Button::new(ui.now_t("onboard-skip"))
                .kind(ButtonKind::Text)
                .size(ControlSize::Small),
        )
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(finish())),
    ));
    // Each step rises in on the `enter` token.
    let body = dynamic(move || step.get(), move |n| onboard_step(ui, *n))
        .transition(crate::ui::motion::enter());
    let footer = widget(
        Stack::row(space::MD)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .padding_xy(INSET, 0.0),
    )
    .children((
        widget(
            Button::new(ui.now_t("onboard-back"))
                .icon(ti::CHEVRON_LEFT)
                .size(ControlSize::Small),
        )
        .visible(move || step.get() > 0)
        .on(move |_: &Activate| step.set(step.get_untracked().saturating_sub(1))),
        widget(Stack::row(0.0).grow(1.0)),
        // Next, and on the last step "Get started".
        widget(crate::skin_theme::cta(
            Button::new(ui.now_t("onboard-next"))
                .kind(ButtonKind::Primary)
                .size(ControlSize::Small),
        ))
        .bind(move |b: &mut Button| {
            let last = step.get() + 1 >= ONBOARD_STEPS;
            let label = ui.now_t(if last { "onboard-done" } else { "onboard-next" });
            if b.label != label {
                b.label = label;
            }
        })
        .on_cx(move |_, _: &Activate, cx| {
            crate::ui::motion::press(cx, ui.reduced_motion.get_untracked());
            let n = step.get_untracked();
            if n + 1 >= ONBOARD_STEPS {
                cx.dispatch_program_all(finish());
            } else {
                step.set(n + 1);
            }
        }),
    ));
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Stretch)
            .surface(SemanticColorRole::Surface)
            .radius(nana_ui_core::RadiusTier::Lg)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .with_layout(|l| {
                l.position = PositionSpec::Relative;
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
            }),
    )
    .visible(move || !ui.settings.with(|s| s.general.onboarding_done))
    .children((
        widget(
            Stack::column(0.0)
                .width(LengthSpec::Px(3.0))
                .surface(SemanticColorRole::Accent)
                .with_layout(accent_bar_layout),
        ),
        widget(
            Stack::column(space::MD)
                .grow(1.0)
                .shrink(1.0)
                .padding_xy(0.0, BLOCK_PAD_Y),
        )
        .children((header, body, divider(), footer)),
    ))
    .into_any()
}

/// "Step 2 of 4" and a dot per step: the current one a wide accent pill,
/// the ones done accent dots, the ones ahead hairline dots.
fn step_indicator(ui: Ui) -> AnyView {
    let step = ui.onboard_step;
    let label = move || {
        I18N.tr_in(
            ui.locale.get_untracked(),
            "onboard-step",
            &[
                ("step", u32::from(step.get() + 1).into()),
                ("total", u32::from(ONBOARD_STEPS).into()),
            ],
        )
    };
    let dots: Vec<AnyView> = (0..ONBOARD_STEPS)
        .map(|i| {
            widget(
                Stack::row(0.0)
                    .height(LengthSpec::Px(space::SM))
                    .radius_px(3.0),
            )
            .bind(move |s: &mut Stack| {
                let n = step.get();
                let role = if i <= n {
                    SemanticColorRole::Accent
                } else {
                    SemanticColorRole::BorderStrong
                };
                let taken = std::mem::replace(s, Stack::row(0.0));
                *s = taken
                    .width(LengthSpec::Px(if i == n { space::XXXL } else { space::SM }))
                    .surface(role);
            })
            .into_any()
        })
        .collect();
    widget(Stack::row(space::MD).align(AlignSpec::Center))
        .children((
            widget(
                Text::new(label())
                    .style(text_style(type_scale::META, type_scale::REGULAR))
                    .color(SemanticColorRole::Muted),
            )
            .bind(move |t: &mut Text| {
                let text = label();
                if t.value != text {
                    t.value = text;
                }
            }),
            widget(Stack::row(space::XS).align(AlignSpec::Center)).children(dots),
        ))
        .into_any()
}

/// One step's body, under the guide's header. Its rows carry no dividers of
/// their own: the footer's line is the step's only one, so none stacks on
/// it.
fn onboard_step(ui: Ui, n: u8) -> AnyView {
    let lead = |key: &str| {
        widget(Stack::column(0.0).padding_xy(INSET, 0.0))
            .children(hint_13(ui.now_t(key)))
            .into_any()
    };
    let body: Vec<AnyView> = match n {
        0 => vec![
            lead("onboard-welcome-body"),
            row(ui, "onboard-language", None, language_select(ui)),
            row(
                ui,
                "onboard-speaker",
                Some("general-speaker-hint"),
                speaker_field(ui),
            ),
        ],
        1 => vec![
            lead("onboard-skin-hint"),
            row(ui, "onboard-skin", None, skin_select(ui)),
            // The live preview of the skin just chosen.
            skin_preview(ui, signal(None)),
        ],
        2 => vec![
            lead("onboard-mode-body"),
            widget(
                Stack::row(space::XL)
                    .align(AlignSpec::Stretch)
                    .width(LengthSpec::Fill)
                    .padding_xy(INSET, 0.0),
            )
            .children((
                mode_card(ui, InputMode::Compose),
                mode_card(ui, InputMode::Mirror),
            ))
            .into_any(),
        ],
        _ => vec![
            widget(Stack::column(space::LG).padding_xy(INSET, 0.0))
                .children((subheading(ui, "onboard-hotkeys"), hotkeys_sentence(ui)))
                .into_any(),
            row(
                ui,
                "onboard-obs",
                Some("onboard-obs-hint"),
                button(ui, "onboard-obs", || Msg::SettingsPage(Page::Output)),
            ),
            widget(Stack::column(0.0).padding_xy(INSET, 0.0))
                .children(hint(ui.now_t("onboard-reopen")))
                .into_any(),
        ],
    };
    widget(Stack::column(0.0)).children(body).into_any()
}

/// Every skin that loads, by name, bound to the active skin: choosing one
/// applies it, and the preview below follows.
fn skin_select(ui: Ui) -> AnyView {
    let options: Vec<(String, String)> = ui
        .skins
        .get_untracked()
        .iter()
        .filter(|r| r.error.is_none())
        .map(|r| (r.id.clone(), r.name.clone()))
        .collect();
    select(
        ui,
        options,
        |s| s.skin.id.clone(),
        |s, v| s.skin.id = v.to_owned(),
    )
}

/// A picture of an input mode: the input window's text on the left, the
/// dialogue box on the right, and what carries one to the other.
fn mode_diagram(mode: InputMode) -> AnyView {
    let bar = |w: f32, role: SemanticColorRole| {
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(w))
                .height(LengthSpec::Px(4.0))
                .radius_px(2.0)
                .surface(role),
        )
        .into_any()
    };
    let caret = || {
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(2.0))
                .height(LengthSpec::Px(12.0))
                .surface(SemanticColorRole::Accent),
        )
        .into_any()
    };
    // A small window: its lines, the last ending at a caret when typing.
    let window = |w: f32, lines: Vec<f32>, typing: bool, accent_edge: bool| {
        let last = lines.len().saturating_sub(1);
        let rows: Vec<AnyView> = lines
            .into_iter()
            .enumerate()
            .map(|(i, lw)| {
                widget(Stack::row(space::XXS).align(AlignSpec::Center))
                    .children((
                        bar(lw, SemanticColorRole::BorderStrong),
                        (typing && i == last).then(caret),
                    ))
                    .into_any()
            })
            .collect();
        let frame = Stack::column(space::SM)
            .width(LengthSpec::Px(w))
            .padding_xy(space::MD, space::MD)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(nana_ui_core::RadiusTier::Sm)
            .with_layout(|l| {
                l.position = PositionSpec::Relative;
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
            });
        widget(frame)
            .children((
                accent_edge.then(|| {
                    widget(
                        Stack::column(0.0)
                            .width(LengthSpec::Px(2.0))
                            .surface(SemanticColorRole::Accent)
                            .with_layout(accent_bar_layout),
                    )
                }),
                rows,
            ))
            .into_any()
    };
    let (link, shown, typing) = match mode {
        // Written in full, then sent: the box shows the whole line.
        InputMode::Compose => (ti::SEND, vec![40.0, 26.0], false),
        // Followed live: the box shows what is typed so far.
        InputMode::Mirror => (ti::ARROW_NARROW_RIGHT, vec![40.0, 14.0], true),
    };
    widget(
        Stack::row(space::MD)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Fill)
            .surface(SemanticColorRole::Subtle)
            .radius(nana_ui_core::RadiusTier::Md),
    )
    .children((
        window(60.0, vec![40.0, 14.0], true, false),
        widget(IconGlyph::new(link).size(16.0).role(match mode {
            InputMode::Compose => SemanticColorRole::Accent,
            InputMode::Mirror => SemanticColorRole::Highlight,
        })),
        window(76.0, shown, typing, true),
    ))
    .into_any()
}

/// One input mode as a card to pick: its picture, its name and what it
/// does. The chosen one takes the selected fill and an accent outline.
fn mode_card(ui: Ui, mode: InputMode) -> AnyView {
    let (name, about) = match mode {
        InputMode::Compose => ("mode-compose", "mode-compose-hint"),
        InputMode::Mirror => ("mode-mirror", "mode-mirror-hint"),
    };
    let chosen = move || ui.settings.with(|s| s.general.input_mode == mode);
    let item = widget(
        ListItem::new(ui.now_t(name))
            .style({
                let mut st = ListItem::new("").style;
                let l = Arc::make_mut(&mut st.layout);
                l.width = Some(LengthSpec::Fill);
                st.control_padding_x = None;
                l.padding_left = Some(LengthSpec::Px(space::LG));
                l.padding_right = Some(LengthSpec::Px(space::LG));
                l.padding_top = Some(LengthSpec::Px(space::LG));
                l.padding_bottom = Some(LengthSpec::Px(space::LG));
                st
            })
            .auto_height(true),
    )
    .bind(move |i: &mut ListItem| i.selected = chosen())
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(edit(move |s| s.general.input_mode = mode));
    })
    .content(
        widget(Stack::column(space::MD).width(LengthSpec::Fill)).children((
            mode_diagram(mode),
            widget(
                Text::new(ui.now_t(name)).style(text_style(type_scale::BODY, type_scale::SEMIBOLD)),
            ),
            hint(ui.now_t(about)),
        )),
    );
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Px(0.0))
            .grow(1.0)
            .radius(nana_ui_core::RadiusTier::Md),
    )
    .bind(move |s: &mut Stack| {
        let (role, w) = if chosen() {
            (SemanticColorRole::Accent, 2.0)
        } else {
            (SemanticColorRole::BorderSoft, 1.0)
        };
        let taken = std::mem::replace(s, Stack::row(0.0));
        *s = taken.outline(role, w);
    })
    .children(item)
    .into_any()
}

/// "Press [F8] to summon the input and [F9] to continue": the sentence
/// around the hotkeys as keycaps, in the locale's own word order.
fn hotkeys_sentence(ui: Ui) -> AnyView {
    let s = ui.settings.get_untracked();
    let chord = |h: Option<&nanatype_settings::Hotkey>| match h {
        Some(h) => crate::ui::keycap::chord(&h.label(META_LABEL)),
        None => crate::ui::keycap::keycap("—"),
    };
    let text = I18N.tr_in(
        ui.locale.get_untracked(),
        "onboard-hotkeys-body",
        &[("summon", "\u{1}".into()), ("continue", "\u{2}".into())],
    );
    let mut parts: Vec<AnyView> = Vec::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find(['\u{1}', '\u{2}']) {
        let (before, after) = rest.split_at(at);
        if !before.trim().is_empty() {
            parts.push(hint_13(before.trim().to_owned()));
        }
        parts.push(if after.starts_with('\u{1}') {
            chord(s.hotkeys.summon.as_ref())
        } else {
            chord(s.hotkeys.continue_.as_ref())
        });
        rest = &after[1..];
    }
    if !rest.trim().is_empty() {
        parts.push(hint_13(rest.trim().to_owned()));
    }
    widget(Stack::row(space::SM).align(AlignSpec::Center).wrap(true))
        .children(parts)
        .into_any()
}

/// A 3px bar pinned down the left edge of a relatively positioned parent.
fn accent_bar_layout(l: &mut nana_ui_core::LayoutStyle) {
    l.position = PositionSpec::Absolute;
    l.offset_left = Some(LengthSpec::Px(0.0));
    l.offset_top = Some(LengthSpec::Px(0.0));
    l.offset_bottom = Some(LengthSpec::Px(0.0));
}

/// Body-size muted text.
fn hint_13(text: String) -> AnyView {
    widget(
        Text::new(text)
            .style(text_style(type_scale::BODY, type_scale::REGULAR))
            .color(SemanticColorRole::Muted),
    )
    .into_any()
}

fn mirror(ui: Ui) -> bool {
    ui.settings
        .with(|s| matches!(s.general.input_mode, InputMode::Mirror))
}

/// Reads 0 as "Off".
fn off_at_zero(locale: Locale, v: f64) -> Option<String> {
    (v <= 0.0).then(|| I18N.t_in(locale, "playback-auto-off"))
}

/// Reads 0 as "Skin default".
fn skin_at_zero(locale: Locale, v: f64) -> Option<String> {
    (v <= 0.0).then(|| I18N.t_in(locale, "common-skin-default"))
}

fn after_key(a: &AfterLine) -> String {
    match a {
        AfterLine::Hold => "hold".into(),
        AfterLine::Fade { .. } => "fade".into(),
        AfterLine::Clear { .. } => "clear".into(),
    }
}

fn after_delay(a: &AfterLine) -> f64 {
    match a {
        AfterLine::Fade { delay_ms, .. } | AfterLine::Clear { delay_ms } => {
            f64::from(*delay_ms) / 1000.0
        }
        AfterLine::Hold => 3.0,
    }
}

fn playback(ui: Ui) -> AnyView {
    let entrance_opts = select_keys(
        ui,
        &[
            ("skin", "tb-entrance-skin"),
            ("none", "tb-entrance-none"),
            ("fade", "tb-entrance-fade"),
            ("pop", "tb-entrance-pop"),
            ("slide", "tb-entrance-slide"),
        ],
    );
    let mut devices = vec![("default".to_owned(), ui.now_t("playback-device-default"))];
    devices.extend(
        ui.status
            .get_untracked()
            .audio_devices
            .iter()
            .map(|d| (d.clone(), d.clone())),
    );
    let holds = move || {
        ui.settings
            .with(|s| matches!(s.playback.after, AfterLine::Hold))
    };
    widget(Stack::column(SECTION_GAP))
        .children((
            section(
                ui,
                "",
                vec![
                    row(
                        ui,
                        "playback-typing",
                        Some("playback-typing-hint"),
                        switch(ui, |s| s.playback.typing, |s, v| s.playback.typing = v),
                    ),
                    row(
                        ui,
                        "playback-speed",
                        None,
                        slider(
                            ui,
                            (0.25, 3.0, 0.05),
                            "×",
                            |s| f64::from(1.0 / s.playback.speed_scale.max(0.01)),
                            |s, v| s.playback.speed_scale = (1.0 / v.max(0.01)) as f32,
                        ),
                    ),
                    row(
                        ui,
                        "playback-punct",
                        Some("playback-punct-hint"),
                        switch(
                            ui,
                            |s| s.playback.punctuation_pauses,
                            |s, v| s.playback.punctuation_pauses = v,
                        ),
                    ),
                    row(
                        ui,
                        "playback-entrance",
                        None,
                        select(
                            ui,
                            entrance_opts,
                            |s| {
                                s.playback
                                    .entrance
                                    .map(|e| e.key().to_owned())
                                    .unwrap_or_else(|| "skin".into())
                            },
                            |s, v| s.playback.entrance = EntranceKind::from_key(v),
                        ),
                    ),
                ],
            ),
            section_with(
                ui,
                "playback-flow",
                vec![
                    (
                        row(
                            ui,
                            "playback-after",
                            None,
                            segmented(
                                ui,
                                select_keys(
                                    ui,
                                    &[
                                        ("hold", "playback-after-hold"),
                                        ("fade", "playback-after-fade"),
                                        ("clear", "playback-after-clear"),
                                    ],
                                ),
                                |s| after_key(&s.playback.after),
                                |s, v| {
                                    let delay = (after_delay(&s.playback.after) * 1000.0) as u32;
                                    s.playback.after = match v {
                                        "fade" => AfterLine::Fade {
                                            delay_ms: delay,
                                            per_unit_ms: 40,
                                        },
                                        "clear" => AfterLine::Clear { delay_ms: delay },
                                        _ => AfterLine::Hold,
                                    }
                                },
                            ),
                        ),
                        None,
                    ),
                    shown_if(
                        row(
                            ui,
                            "playback-after-delay",
                            None,
                            slider(
                                ui,
                                (0.5, 30.0, 0.5),
                                "s",
                                |s| after_delay(&s.playback.after),
                                |s, v| {
                                    let ms = (v * 1000.0) as u32;
                                    match &mut s.playback.after {
                                        AfterLine::Fade { delay_ms, .. }
                                        | AfterLine::Clear { delay_ms } => *delay_ms = ms,
                                        AfterLine::Hold => {}
                                    }
                                },
                            ),
                        ),
                        move || !holds(),
                    ),
                    (
                        row(
                            ui,
                            "playback-auto",
                            None,
                            slider_with(
                                ui,
                                (0.0, 10.0, 0.5),
                                "s",
                                |locale, v| {
                                    Some(if v <= 0.0 {
                                        I18N.t_in(locale, "playback-auto-off")
                                    } else {
                                        I18N.tr_in(
                                            locale,
                                            "playback-auto-value",
                                            &[("seconds", trim_num(v, 0.5).into())],
                                        )
                                    })
                                },
                                |s| {
                                    s.playback
                                        .auto_advance_ms
                                        .map(|v| f64::from(v) / 1000.0)
                                        .unwrap_or(0.0)
                                },
                                |s, v| {
                                    s.playback.auto_advance_ms =
                                        (v > 0.0).then_some((v * 1000.0) as u32)
                                },
                            ),
                        ),
                        None,
                    ),
                    (
                        // Short option names; the hint spells out the chosen one.
                        row_live_hint(
                            ui,
                            "playback-queue",
                            move || {
                                let key = ui.settings.with(|s| match s.playback.queue {
                                    QueuePolicy::Replace => "playback-queue-replace",
                                    QueuePolicy::FinishThenNext => "playback-queue-finish",
                                    QueuePolicy::Queue => "playback-queue-queue",
                                });
                                Some(I18N.t_in(ui.locale.get(), key))
                            },
                            segmented(
                                ui,
                                select_keys(
                                    ui,
                                    &[
                                        ("replace", "playback-queue-replace-short"),
                                        ("finish", "playback-queue-finish-short"),
                                        ("queue", "playback-queue-queue-short"),
                                    ],
                                ),
                                |s| match s.playback.queue {
                                    QueuePolicy::Replace => "replace".into(),
                                    QueuePolicy::FinishThenNext => "finish".into(),
                                    QueuePolicy::Queue => "queue".into(),
                                },
                                |s, v| {
                                    s.playback.queue = match v {
                                        "finish" => QueuePolicy::FinishThenNext,
                                        "queue" => QueuePolicy::Queue,
                                        _ => QueuePolicy::Replace,
                                    }
                                },
                            ),
                        ),
                        None,
                    ),
                    (
                        // The hint says what the system asks for too: either
                        // one reduces motion.
                        row_live_hint(
                            ui,
                            "playback-reduced",
                            move || {
                                let locale = ui.locale.get();
                                let system = if ui.system_reduced_motion.get() {
                                    "playback-reduced-system-on"
                                } else {
                                    "playback-reduced-system-off"
                                };
                                Some(format!(
                                    "{} · {}",
                                    I18N.t_in(locale, "playback-reduced-hint"),
                                    I18N.t_in(locale, system)
                                ))
                            },
                            switch(
                                ui,
                                |s| s.playback.reduced_motion,
                                |s, v| s.playback.reduced_motion = v,
                            ),
                        ),
                        None,
                    ),
                ],
            ),
            section(
                ui,
                "playback-audio",
                vec![
                    row(
                        ui,
                        "playback-sound",
                        None,
                        switch(ui, |s| s.sound.typing, |s, v| s.sound.typing = v),
                    ),
                    row(
                        ui,
                        "playback-volume",
                        None,
                        percent(
                            ui,
                            |s| f64::from(s.sound.master),
                            |s, v| s.sound.master = v as f32,
                        ),
                    ),
                    row(
                        ui,
                        "playback-volume-typing",
                        None,
                        percent(
                            ui,
                            |s| f64::from(s.sound.typing_volume),
                            |s, v| s.sound.typing_volume = v as f32,
                        ),
                    ),
                    row(
                        ui,
                        "playback-volume-effects",
                        None,
                        percent(
                            ui,
                            |s| f64::from(s.sound.effects_volume),
                            |s, v| s.sound.effects_volume = v as f32,
                        ),
                    ),
                    row(
                        ui,
                        "playback-device",
                        None,
                        select(
                            ui,
                            devices,
                            |s| s.sound.device.clone().unwrap_or_else(|| "default".into()),
                            |s, v| s.sound.device = (v != "default").then(|| v.to_owned()),
                        ),
                    ),
                ],
            ),
        ))
        .into_any()
}

fn skins(ui: Ui) -> AnyView {
    let filter: Signal<Option<String>> = signal(None);
    let previewing: Signal<Option<String>> = signal(None);
    // Cards with thumbnails by default; the list stays for skins whose card
    // cannot say enough (an author's broken skin, a long description).
    let grid = ui.skins_grid;
    let list = dynamic(
        move || {
            (
                ui.skins.with(|s| Arc::as_ptr(s) as usize),
                ui.settings.with(|s| s.skin.id.clone()),
                filter.get(),
                grid.get(),
                ui.chrome_dark.get(),
            )
        },
        move |(_, active, shown, grid, _)| {
            let locale = ui.locale.get_untracked();
            let skins = ui.skins.get_untracked();
            let rows = skins
                .iter()
                .filter(|skin| matches_filter(skin, shown.as_deref()));
            if *grid {
                let cards: Vec<AnyView> = rows
                    .map(|skin| skin_card(ui, skin, skin.id == *active, previewing, locale))
                    .collect();
                widget(
                    Stack::row(GRID_GAP)
                        .wrap(true)
                        .padding_xy(INSET, 0.0)
                        .with_layout(|l| {
                            l.row_gap = Some(LengthSpec::Px(GRID_GAP));
                            l.padding_bottom = Some(LengthSpec::Px(INSET));
                        }),
                )
                .children(cards)
                .into_any()
            } else {
                let rows: Vec<AnyView> = rows
                    .map(|skin| skin_list_row(ui, skin, skin.id == *active, previewing, locale))
                    .collect();
                widget(Stack::column(0.0)).children(rows).into_any()
            }
        },
    );
    let fallback = ui.skin.get_untracked();
    widget(Stack::column(SECTION_GAP))
        .children((
            widget(Stack::row(space::MD)).children(primary(ui, "skins-import", || {
                Msg::PickFile(FileTarget::ImportSkin)
            })),
            section(ui, "skins-preview", vec![skin_preview(ui, previewing)]),
            section(
                ui,
                "skins-library",
                vec![
                    widget(Stack::column(0.0).padding_xy(INSET, space::LG))
                        .children(skin_filters(ui, filter, grid))
                        .into_any(),
                    list.into_any(),
                ],
            ),
            section(
                ui,
                "skins-overrides",
                vec![
                    row(
                        ui,
                        "skins-override-text",
                        None,
                        color(
                            ui,
                            fallback.dialogue.text.color,
                            |s| s.skin.overrides.text_color,
                            |s, v| s.skin.overrides.text_color = v,
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-name",
                        None,
                        color(
                            ui,
                            fallback.dialogue.nameplate.color,
                            |s| s.skin.overrides.name_color,
                            |s, v| s.skin.overrides.name_color = v,
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-box",
                        None,
                        color(
                            ui,
                            fallback.dialogue.box_.color,
                            |s| s.skin.overrides.box_color,
                            |s, v| s.skin.overrides.box_color = v,
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-accent",
                        None,
                        color(
                            ui,
                            fallback.chrome.accent,
                            |s| s.skin.overrides.accent,
                            |s, v| s.skin.overrides.accent = v,
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-size",
                        None,
                        // The step left of 12 px is "unset": the skin's own size.
                        slider_with(
                            ui,
                            (11.0, 72.0, 1.0),
                            "px",
                            |locale, v| {
                                (v < 12.0).then(|| I18N.t_in(locale, "common-skin-default"))
                            },
                            |s| f64::from(s.skin.overrides.text_size.unwrap_or(11.0)),
                            |s, v| s.skin.overrides.text_size = (v >= 12.0).then_some(v as f32),
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-overflow",
                        Some("skins-override-overflow-hint"),
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("skin", "skins-override-overflow-skin"),
                                    ("clip", "skins-override-overflow-clip"),
                                    ("scroll", "skins-override-overflow-scroll"),
                                    ("page", "skins-override-overflow-page"),
                                ],
                            ),
                            |s| s.skin.overrides.overflow.map_or("skin", |o| o.key()).into(),
                            |s, v| s.skin.overrides.overflow = Overflow::from_key(v),
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-page-auto",
                        Some("skins-override-page-auto-hint"),
                        slider_with(
                            ui,
                            (0.0, 10.0, 0.5),
                            "s",
                            skin_at_zero,
                            |s| f64::from(s.skin.overrides.overflow_auto_ms.unwrap_or(0)) / 1000.0,
                            |s, v| {
                                s.skin.overrides.overflow_auto_ms =
                                    (v > 0.0).then_some((v * 1000.0) as u32)
                            },
                        ),
                    ),
                    row(
                        ui,
                        "skins-override-reset",
                        None,
                        button(ui, "common-reset", || {
                            edit(|s| s.skin.overrides = Default::default())
                        }),
                    ),
                ],
            ),
            section(
                ui,
                "skins-interface",
                vec![
                    row(
                        ui,
                        "skins-advanced-effects",
                        Some("skins-advanced-effects-hint"),
                        switch(
                            ui,
                            |s| s.skin.advanced_effects,
                            |s, v| s.skin.advanced_effects = v,
                        ),
                    ),
                    row(
                        ui,
                        "skins-hot-reload",
                        Some("skins-hot-reload-hint"),
                        switch(ui, |s| s.skin.hot_reload, |s, v| s.skin.hot_reload = v),
                    ),
                ],
            ),
        ))
        .into_any()
}

/// A labelled menu entry that closes its menu and sends `msg()`.
fn menu_item(
    ui: Ui,
    open: Signal<bool>,
    key: &str,
    icon: nana_ui::runtime::Icon,
    msg: impl Fn() -> Msg + Send + 'static,
) -> AnyView {
    widget(ActionMenuItem::new(ui.now_t(key)).leading(icon))
        .on_cx(move |_, _: &Activate, cx| {
            open.set(false);
            cx.dispatch_program_all(msg());
        })
        .into_any()
}

/// Space between skin cards.
const GRID_GAP: f32 = space::XL;
/// Height of a card's thumbnail well.
const THUMB_H: f32 = 76.0;

/// Whether skin `id` is the one previewed (the active one until another is
/// picked), tracked.
fn previewed(
    ui: Ui,
    previewing: Signal<Option<String>>,
    id: String,
) -> impl Fn() -> bool + Clone + Send + 'static {
    move || {
        previewing.with(|p| match p {
            Some(p) => *p == id,
            None => ui.settings.with(|s| s.skin.id == id),
        })
    }
}

/// The solid Apply button that makes skin `id` the active one, shown while
/// `shown` holds.
fn apply_button(ui: Ui, id: String, shown: impl Fn() -> bool + Send + 'static) -> AnyView {
    widget(crate::skin_theme::cta(
        Button::new(ui.now_t("common-apply"))
            .kind(ButtonKind::Primary)
            .size(ControlSize::Small),
    ))
    .visible(shown)
    .on_cx(move |_, _: &Activate, cx| {
        let id = id.clone();
        cx.dispatch_program_all(edit(move |s| s.skin.id = id));
    })
    .into_any()
}

/// A skin's "⋯" menu: duplicate, export, open folder and, for a skin the
/// user added, delete (asked before it acts).
fn skin_actions(ui: Ui, skin: &SkinRow, locale: Locale) -> AnyView {
    let (eid, did, rid) = (skin.id.clone(), skin.id.clone(), skin.id.clone());
    let folder = skin.folder.clone();
    let menu_open = signal(false);
    let menu = move |armed: Option<Signal<bool>>| {
        let mut items = vec![
            menu_item(ui, menu_open, "skins-duplicate", ti::COPY, {
                let id = did.clone();
                move || Msg::DuplicateSkin(id.clone())
            }),
            menu_item(ui, menu_open, "skins-export", ti::FILE_EXPORT, {
                let id = eid.clone();
                move || Msg::ExportSkin(id.clone())
            }),
        ];
        if let Some(f) = folder.clone() {
            items.push(menu_item(
                ui,
                menu_open,
                "skins-open-folder",
                ti::FOLDER_OPEN,
                move || Msg::OpenFolder(f.clone()),
            ));
        }
        if let Some(armed) = armed {
            items.push(
                widget(
                    ActionMenuItem::new(ui.now_t("skins-delete"))
                        .leading(ti::TRASH)
                        .danger(true),
                )
                .on(move |_: &Activate| {
                    menu_open.set(false);
                    armed.set(true);
                })
                .into_any(),
            );
        }
        widget(
            ActionMenu::new()
                .trigger_icon(ti::DOTS, ui.now_t("skins-more"))
                .bare_trigger(true),
        )
        .bind(move |m: &mut ActionMenu| m.popover.open = menu_open.get())
        .on(move |e: &PopoverToggled| menu_open.set(e.open))
        .on(move |_: &PopoverClosed| menu_open.set(false))
        .children(items)
        .into_any()
    };
    if skin.builtin {
        menu(None)
    } else {
        confirm(
            ui,
            I18N.tr_in(
                locale,
                "skins-delete-confirm",
                &[("name", skin.name.as_str().into())],
            ),
            ui.now_t("skins-delete"),
            move |armed| menu(Some(armed)),
            move || Msg::DeleteSkin(rid.clone()),
        )
    }
}

/// One skin in the list view: swatches, name, badges and its detail line.
fn skin_list_row(
    ui: Ui,
    skin: &SkinRow,
    selected: bool,
    previewing: Signal<Option<String>>,
    locale: Locale,
) -> AnyView {
    let swatch = |color: [f32; 4]| {
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(14.0))
                .height(LengthSpec::Px(14.0))
                .outline(SemanticColorRole::BorderStrong, 1.0)
                .with_layout(move |l| {
                    l.background = Some(color);
                    // A dot: the pill radius.
                    l.border_radius = Some(7.0);
                }),
        )
        .into_any()
    };
    let detail = match &skin.error {
        Some(e) => format!("{} · {e}", ui.now_t("skins-broken")),
        None if !skin.description.is_empty() && skin.builtin => skin.description.clone(),
        None if skin.builtin => ui.now_t("skins-builtin"),
        None => format!(
            "{} · {}",
            I18N.tr_in(
                locale,
                "skins-by",
                &[("author", skin.author.as_str().into())]
            ),
            I18N.tr_in(
                locale,
                "skins-version",
                &[("version", skin.version.as_str().into())]
            )
        ),
    };
    // The "Fallback font" badge sits with the tags; the detail line only
    // names the missing family, so it stays one line in every language.
    let fallback = (skin.error.is_none() && skin.fallback_font).then(|| {
        widget(Stack::row(0.0).with_layout(|l| l.flex_shrink = Some(0.0))).children(widget(
            StatusBadge::new(
                I18N.t_in(locale, "skins-fallback-font"),
                StatusTone::Warning,
            )
            .compact(true),
        ))
    });
    let detail = match missing_family(skin).filter(|_| fallback.is_some()) {
        Some(family) => format!(
            "{detail} · {}",
            I18N.tr_in(
                locale,
                "skins-fallback-font-missing",
                &[("family", family.into())]
            )
        ),
        None => detail,
    };
    let usable = skin.error.is_none();
    let previewed = previewed(ui, previewing, skin.id.clone());
    let static_materials = skin.skin.as_ref().is_some_and(|s| !s.materials.is_empty());
    let pick_id = skin.id.clone();
    let trailing = widget(Stack::row(8.0).align(AlignSpec::Center)).children((
        selected.then(|| {
            widget(StatusBadge::new(ui.now_t("skins-active"), StatusTone::Success).compact(true))
        }),
        (!selected && usable).then(|| apply_button(ui, skin.id.clone(), previewed.clone())),
        skin_actions(ui, skin, locale),
    ));
    // The current skin keeps a 3px accent bar; the row being previewed
    // takes the selected fill.
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .with_layout(|l| l.position = PositionSpec::Relative),
    )
    .children((
        widget(
            ListItem::new(skin.name.clone())
                .style({
                    let mut st = ListItem::new("").style;
                    let l = Arc::make_mut(&mut st.layout);
                    l.flex_grow = Some(1.0);
                    l.flex_shrink = Some(1.0);
                    l.min_width = Some(LengthSpec::Px(0.0));
                    l.padding_top = Some(LengthSpec::Px(space::MD));
                    l.padding_bottom = Some(LengthSpec::Px(space::MD));
                    st
                })
                .auto_height(true),
        )
        .bind(move |i: &mut ListItem| i.selected = previewed())
        .on_cx(move |_, _: &Activate, cx| {
            // A skin that does not load has nothing to show.
            if usable {
                previewing.set(Some(pick_id.clone()));
                if ui.shader_preview.get_untracked() || static_materials {
                    cx.dispatch_program_all(Msg::PrepareSkinPreview(pick_id.clone()));
                }
            }
        })
        .leading(widget(Stack::row(4.0)).children((
            swatch(skin.box_color),
            swatch(skin.text_color),
            swatch(skin.accent),
        )))
        .content(
            widget(
                Stack::column(2.0)
                    .grow(1.0)
                    .shrink(1.0)
                    .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
            )
            .children((
                // A long name is cut before the tags and the badge are.
                widget(
                    Stack::row(6.0)
                        .align(AlignSpec::Center)
                        .width(LengthSpec::Fill)
                        .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
                )
                .children((
                    widget(Text::new(skin.name.clone()).style({
                        let mut st = one_line(13.0, 500);
                        Arc::make_mut(&mut st.layout).flex_grow = Some(0.0);
                        st
                    })),
                    badges(locale, skin)
                        .into_iter()
                        .map(|b| {
                            widget(Stack::row(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
                                .children(b)
                                .into_any()
                        })
                        .collect::<Vec<_>>(),
                    fallback,
                )),
                hint(detail),
            )),
        )
        .trailing(trailing),
        widget({
            let bar = Stack::column(0.0)
                .width(LengthSpec::Px(3.0))
                .with_layout(accent_bar_layout);
            if selected {
                bar.surface(SemanticColorRole::Accent)
            } else {
                bar
            }
        }),
    ))
    .into_any()
}

/// Text on one line, cut with an ellipsis where it does not fit.
fn one_line(size: f32, weight: u16) -> nana_ui::runtime::NodeStyle {
    let mut st = text_style(size, weight);
    let l = Arc::make_mut(&mut st.layout);
    l.white_space_nowrap = true;
    l.text_overflow_ellipsis = true;
    l.min_width = Some(LengthSpec::Px(0.0));
    l.flex_grow = Some(1.0);
    l.flex_shrink = Some(1.0);
    st
}

/// The body family a skin asks for first, which this system lacks.
fn missing_family(row: &SkinRow) -> Option<&str> {
    row.skin
        .as_ref()?
        .fonts
        .body
        .families
        .first()
        .map(String::as_str)
}

/// "Fusion Pixel 12px is not installed; showing a fallback font", for a
/// skin whose own body font cannot be used here.
fn fallback_font_note(locale: Locale, row: &SkinRow) -> Option<String> {
    if !row.fallback_font || row.error.is_some() {
        return None;
    }
    let family = missing_family(row)?;
    Some(I18N.tr_in(
        locale,
        "skins-fallback-font-detail",
        &[("family", family.into())],
    ))
}

/// "Light · Classic": the skin's tone and tags on one line.
fn tag_line(locale: Locale, row: &SkinRow) -> String {
    let tone = if row.dark { "dark" } else { "light" };
    std::iter::once(tone)
        .chain(row.tags.iter().map(String::as_str))
        .map(|t| tag_label(locale, t))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// One skin in the grid: a thumbnail of its box over its name and tags.
/// The previewed card takes the selected fill; the current skin carries a
/// "Current" badge in its name row and a 3px accent bar.
fn skin_card(
    ui: Ui,
    skin: &SkinRow,
    current: bool,
    previewing: Signal<Option<String>>,
    locale: Locale,
) -> AnyView {
    let usable = skin.error.is_none();
    let previewed = previewed(ui, previewing, skin.id.clone());
    let dark = ui.chrome_dark.get_untracked();
    let backdrop = BackgroundImage::url(checker_url(dark));
    let well = widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Px(THUMB_H))
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .radius(nana_ui_core::RadiusTier::Sm)
            .with_layout(move |l| {
                l.position = PositionSpec::Relative;
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
                l.paint.background_image = Some(backdrop);
            }),
    )
    .children(super::skin_grid::miniature(&super::skin_grid::thumb(skin)));
    let (tags, tone) = match &skin.error {
        Some(_) => (I18N.t_in(locale, "skins-broken"), SemanticColorRole::Danger),
        None => (tag_line(locale, skin), SemanticColorRole::Muted),
    };
    // "· Fallback font" never shrinks: when the line is too long for the
    // card (English, Japanese), the tags are cut, not the hint.
    let font_note = (skin.error.is_none() && skin.fallback_font).then(|| {
        let mut st = text_style(12.0, 400);
        let l = Arc::make_mut(&mut st.layout);
        l.white_space_nowrap = true;
        l.flex_shrink = Some(0.0);
        widget(
            Text::new(format!(" · {}", I18N.t_in(locale, "skins-fallback-font")))
                .style(st)
                .color(SemanticColorRole::Muted),
        )
    });
    let tags_row = widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
    )
    .children((
        widget(
            Text::new(tags)
                .style({
                    // Shrinks with an ellipsis but does not grow, so the
                    // hint follows the tags rather than the card's edge.
                    let mut st = one_line(12.0, 400);
                    Arc::make_mut(&mut st.layout).flex_grow = Some(0.0);
                    st
                })
                .color(tone),
        ),
        font_note,
    ));
    let name_row = widget(
        Stack::row(6.0)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(Text::new(skin.name.clone()).style(one_line(13.0, 500))),
        // The current skin's badge takes the Apply button's place, off the
        // thumbnail, so no box is covered.
        current.then(|| {
            widget(Stack::row(0.0).with_layout(|l| l.flex_shrink = Some(0.0))).children(widget(
                StatusBadge::new(ui.now_t("skins-active"), StatusTone::Success).compact(true),
            ))
        }),
        (!current && usable).then(|| apply_button(ui, skin.id.clone(), previewed.clone())),
        skin_actions(ui, skin, locale),
    ));
    let static_materials = skin.skin.as_ref().is_some_and(|s| !s.materials.is_empty());
    let pick_id = skin.id.clone();
    let card = widget(
        ListItem::new(skin.name.clone())
            .style({
                let mut st = ListItem::new("")
                    .style
                    .outline(SemanticColorRole::BorderSoft, 1.0)
                    .radius(nana_ui_core::RadiusTier::Md);
                st.control_padding_x = None;
                let l = Arc::make_mut(&mut st.layout);
                l.width = Some(LengthSpec::Fill);
                l.padding_left = Some(LengthSpec::Px(space::MD));
                l.padding_right = Some(LengthSpec::Px(space::MD));
                l.padding_top = Some(LengthSpec::Px(space::MD));
                l.padding_bottom = Some(LengthSpec::Px(space::MD));
                st
            })
            .auto_height(true),
    )
    .bind(move |i: &mut ListItem| i.selected = previewed())
    .on_cx(move |_, _: &Activate, cx| {
        if usable {
            previewing.set(Some(pick_id.clone()));
            if ui.shader_preview.get_untracked() || static_materials {
                cx.dispatch_program_all(Msg::PrepareSkinPreview(pick_id.clone()));
            }
        }
    })
    .content(
        widget(
            Stack::column(8.0)
                .grow(1.0)
                .shrink(1.0)
                .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
        )
        .children((
            well,
            widget(Stack::column(space::XXS).padding_xy(space::XXS, 0.0))
                .children((name_row, tags_row)),
        )),
    );
    widget(Stack::column(0.0).with_layout(|l| {
        l.position = PositionSpec::Relative;
        // Three to a row; the half pixel keeps rounding from wrapping one.
        l.width = Some(crate::ui::surface::calc(
            100.0 / 3.0,
            -GRID_GAP * 2.0 / 3.0 - 0.5,
        ));
    }))
    .children((
        card,
        current.then(|| {
            widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(3.0))
                    .surface(SemanticColorRole::Accent)
                    .radius_px(2.0)
                    .with_layout(|l| {
                        l.position = PositionSpec::Absolute;
                        l.offset_left = Some(LengthSpec::Px(0.0));
                        l.offset_top = Some(LengthSpec::Px(14.0));
                        l.offset_bottom = Some(LengthSpec::Px(14.0));
                    }),
            )
        }),
    ))
    .into_any()
}

/// Grid or list: a two-state toggle of two icons in one outlined pill, the
/// current view selected. Fixed width, so it never wraps.
fn view_switch(ui: Ui, grid: Signal<bool>) -> AnyView {
    let item = |icon: nana_ui::runtime::Icon, key: &str, as_grid: bool| {
        widget(
            IconButton::new(icon, ui.now_t(key))
                .size(ControlSize::Small)
                .with_tooltip(ui.now_t(key)),
        )
        .bind(move |b: &mut IconButton| b.selected = grid.get() == as_grid)
        .on(move |_: &Activate| grid.set(as_grid))
        .into_any()
    };
    widget(
        Stack::row(space::XXS)
            .align(AlignSpec::Center)
            .padding_xy(space::XXS, space::XXS)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(nana_ui_core::RadiusTier::Md)
            .with_layout(|l| {
                l.flex_shrink = Some(0.0);
            }),
    )
    .children((
        item(ti::LAYOUT_GRID, "skins-view-grid", true),
        item(ti::LIST, "skins-view-list", false),
    ))
    .into_any()
}

/// Checkerboard showing around the previewed box.
const PREVIEW_INSET: f32 = INSET;

/// A 10px transparency checkerboard tile as a data URL, in light or dark
/// chrome tones.
fn checker_url(dark: bool) -> String {
    fn tile(a: [u8; 3], b: [u8; 3]) -> String {
        let img = image::RgbImage::from_fn(20, 20, |x, y| {
            image::Rgb(if (x / 10 + y / 10) % 2 == 0 { a } else { b })
        });
        let mut png = Vec::new();
        let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
        format!("data:image/png;base64,{}", base64(&png))
    }
    static LIGHT: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| tile([0xe9, 0xec, 0xf1], [0xf6, 0xf7, 0xf9]));
    static DARK: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| tile([0x2a, 0x2f, 0x3a], [0x32, 0x38, 0x46]));
    if dark { DARK.clone() } else { LIGHT.clone() }
}

/// `light`, `dark` or a tag from [`nanatype_theme::SKIN_TAGS`], localized.
fn tag_label(locale: Locale, tag: &str) -> String {
    I18N.t_in(locale, &format!("skins-tag-{tag}"))
}

/// The light/dark badge and the tag badges of a skin that loads.
fn badges(locale: Locale, row: &SkinRow) -> Vec<AnyView> {
    if row.error.is_some() {
        return Vec::new();
    }
    let tone = if row.dark { "dark" } else { "light" };
    std::iter::once(tone)
        .chain(row.tags.iter().map(String::as_str))
        .map(|t| {
            widget(StatusBadge::new(tag_label(locale, t), StatusTone::Neutral).compact(true))
                .into_any()
        })
        .collect()
}

/// Whether `row` passes the picker's filter (`light`, `dark` or a tag).
fn matches_filter(row: &SkinRow, filter: Option<&str>) -> bool {
    match filter {
        None => true,
        Some("light") => !row.dark,
        Some("dark") => row.dark,
        Some(tag) => row.tags.iter().any(|t| t == tag),
    }
}

/// Chips that narrow the list: all, light, dark, then every tag in use;
/// then the grid/list switch.
fn skin_filters(ui: Ui, filter: Signal<Option<String>>, grid: Signal<bool>) -> AnyView {
    dynamic(
        move || {
            (
                filter.get(),
                ui.skins.with(|s| Arc::as_ptr(s) as usize),
                ui.locale.get(),
            )
        },
        move |(current, _, locale)| {
            let rows = ui.skins.get_untracked();
            let mut keys: Vec<Option<String>> =
                vec![None, Some("light".into()), Some("dark".into())];
            for tag in nanatype_theme::SKIN_TAGS {
                if rows.iter().any(|r| r.tags.iter().any(|t| t == tag)) {
                    keys.push(Some((*tag).to_owned()));
                }
            }
            let chips: Vec<AnyView> = keys
                .into_iter()
                .map(|key| {
                    let text = match &key {
                        None => I18N.t_in(*locale, "skins-filter-all"),
                        Some(tag) => tag_label(*locale, tag),
                    };
                    let selected = *current == key;
                    widget(Chip::new(text).selected(selected).size(ControlSize::Small))
                        .on_cx(move |_, _: &Activate, _| filter.set(key.clone()))
                        .into_any()
                })
                .collect();
            // The chips wrap among themselves; the grid/list toggle keeps
            // the top right of the row, beside the first line, in every
            // language.
            widget(
                Stack::row(12.0)
                    .align(AlignSpec::Start)
                    .width(LengthSpec::Fill),
            )
            .children((
                widget(
                    Stack::row(6.0)
                        .wrap(true)
                        .align(AlignSpec::Center)
                        .grow(1.0)
                        .shrink(1.0)
                        .with_layout(|l| {
                            l.min_width = Some(LengthSpec::Px(0.0));
                            l.row_gap = Some(LengthSpec::Px(6.0));
                        }),
                )
                .children(chips),
                view_switch(ui, grid),
            ))
            .into_any()
        },
    )
    .into_any()
}

/// A live sample of the previewed skin (the active one by default) with the
/// user's personalisation, scaled to fit the page.
fn skin_preview(ui: Ui, previewing: Signal<Option<String>>) -> AnyView {
    let available_width = signal(0.0_f32);
    let key = move || {
        // The overrides and the active skin both show up as a new `ui.skin`.
        (
            previewing.get(),
            ui.chrome_dark.get(),
            ui.settings.with(|s| s.skin.id.clone()),
            ui.skins.with(|s| Arc::as_ptr(s) as usize),
            ui.skin.with(|s| Arc::as_ptr(s) as usize),
            ui.locale.get(),
            available_width.get(),
            (ui.shader_preview.get(), crate::ui::materials::enabled(ui)),
        )
    };
    let content = dynamic(key, move |(want, _, active, _, _, locale, width, _)| {
        let locale = *locale;
        let id = want.clone().unwrap_or_else(|| active.clone());
        let rows = ui.skins.get_untracked();
        let row = rows.iter().find(|r| r.id == id).cloned();
        let other = row
            .as_ref()
            .filter(|r| r.id != *active)
            .and_then(|r| r.skin.clone().map(|s| (r.clone(), s)));
        let is_active = other.is_none();
        let (skin, urls) = match &other {
            None => (ui.skin.get_untracked(), ui.asset_urls.get_untracked()),
            Some((row, base)) => {
                let skin = ui.settings.with_untracked(|s| {
                    crate::skin_theme::prepare(
                        base,
                        &s.skin.overrides,
                        &Default::default(),
                        locale.tag(),
                    )
                });
                (Arc::new(skin), row.image_urls.clone())
            }
        };
        let d = &skin.dialogue;
        let (w, h) = (
            d.region[0] + d.overscan * 2.0,
            d.region[1] + d.overscan * 2.0,
        );
        let scale = ((width - 2.0 * PREVIEW_INSET).max(0.0) / w).min(1.0);
        let ctx = BoxCtx {
            skin: skin.clone(),
            urls,
            source: Source::Preview,
            preview_name: I18N.t_in(locale, "skins-preview-name"),
        };
        let doc = nanatype_markup::parse(&I18N.t_in(locale, "skins-preview-line")).doc;
        let sample = dialogue::preview_box(ui, ctx, &doc);
        // A transparency checkerboard in the chrome's tone, so boxless and
        // translucent skins read as see-through.
        let dark = ui.chrome_dark.get_untracked();
        let backdrop = BackgroundImage::url(checker_url(dark));
        let stage = widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(w * scale))
                .height(LengthSpec::Px(h * scale))
                .with_layout(move |l| {
                    l.position = PositionSpec::Relative;
                    l.overflow_x = OverflowSpec::Hidden;
                    l.overflow_y = OverflowSpec::Hidden;
                }),
        )
        .key("skin-preview-box")
        .children(
            widget(
                Stack::fill_column(0.0)
                    .width(LengthSpec::Px(w))
                    .height(LengthSpec::Px(h))
                    .with_layout(move |l| {
                        l.position = PositionSpec::Absolute;
                        l.offset_left = Some(LengthSpec::Px(0.0));
                        l.offset_top = Some(LengthSpec::Px(0.0));
                        l.pointer_events = Some(PointerEventsSpec::None);
                        l.transform = Some(crate::ui::surface::affine(0.0, 0.0, 0.0, scale));
                        l.transform_origin = Some(TransformOrigin {
                            x: LengthSpec::Px(0.0),
                            y: LengthSpec::Px(0.0),
                        });
                    }),
            )
            .children(sample),
        );
        let stage = widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(w * scale + 2.0 * PREVIEW_INSET))
                .padding_xy(PREVIEW_INSET, PREVIEW_INSET)
                .radius(nana_ui_core::RadiusTier::Md)
                .with_layout(move |l| {
                    l.overflow_x = OverflowSpec::Hidden;
                    l.overflow_y = OverflowSpec::Hidden;
                    l.paint.background_image = Some(backdrop);
                }),
        )
        .children(stage);
        let (name, description, badges) = match &row {
            Some(r) => (r.name.clone(), r.description.clone(), badges(locale, r)),
            None => (skin.display_name(locale.tag()), String::new(), Vec::new()),
        };
        let font_note = row.as_ref().and_then(|r| fallback_font_note(locale, r));
        let apply_id = id.clone();
        let play_id = id.clone();
        let caption = widget(
            Stack::bar(12.0)
                .align(AlignSpec::Center)
                .width(LengthSpec::Px(w * scale + 2.0 * PREVIEW_INSET)),
        )
        .children((
            (!skin.dialogue.shader_effects.is_empty()
                || skin.dialogue.decor.iter().any(|d| d.shader.is_some()))
            .then(|| {
                widget(
                    Button::new(I18N.t_in(
                        locale,
                        if ui.shader_preview.get_untracked() {
                            "skins-stop-effects"
                        } else {
                            "skins-play-effects"
                        },
                    ))
                    .size(ControlSize::Small),
                )
                .on_cx(move |_, _: &Activate, cx| {
                    if ui.shader_preview.get_untracked() {
                        ui.shader_preview.set(false);
                    } else {
                        cx.dispatch_program_all(Msg::PrepareSkinPreview(play_id.clone()));
                    }
                })
            }),
            widget(Stack::column(4.0).grow(1.0).shrink(1.0).with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
            }))
            .children((
                widget(Stack::row(6.0).wrap(true).align(AlignSpec::Center))
                    .children((label(name), badges)),
                (!description.is_empty()).then(|| hint(description)),
                font_note.map(hint),
            )),
            (!is_active).then(|| {
                widget(crate::skin_theme::cta(
                    Button::new(I18N.t_in(locale, "common-apply"))
                        .kind(ButtonKind::Primary)
                        .size(ControlSize::Small),
                ))
                .key("apply-preview")
                .on_cx(move |_, _: &Activate, cx| {
                    let id = apply_id.clone();
                    cx.dispatch_program_all(edit(move |s| s.skin.id = id));
                })
            }),
        ));
        widget(Stack::column(space::LG))
            .children((stage, caption))
            .into_any()
    });
    // Measure a stable, page-width container, not the scaled sample itself.
    // Otherwise the preview's intrinsic width can prevent it from shrinking.
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .padding_xy(INSET, space::XL)
            .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
    )
    .key("skin-preview")
    .on(move |event: &SizeChanged| {
        let width = (event.width - 2.0 * INSET).max(0.0);
        if available_width.get_untracked() != width {
            available_width.set(width);
        }
    })
    .children(content)
    .into_any()
}

/// The overlay windows, in the matrix's column order.
const OVERLAYS: [WindowKind; 4] = [
    WindowKind::Display,
    WindowKind::PeerDisplay,
    WindowKind::History,
    WindowKind::Input,
];
/// Width of the matrix's row labels.
const MATRIX_LABEL_W: f32 = 150.0;
/// Space between the matrix's columns.
const MATRIX_GAP: f32 = space::MD;

/// A matrix line: a label, then one cell per overlay window.
fn matrix_row(label: AnyView, cells: [AnyView; 4]) -> AnyView {
    let cells = cells.map(|cell| {
        widget(
            // A quarter of what the label and the gaps leave, so the columns
            // line up whatever their cells hold.
            Stack::row(0.0).align(AlignSpec::Center).with_layout(|l| {
                l.width = Some(crate::ui::surface::calc(
                    100.0 / 4.0,
                    -(MATRIX_LABEL_W + 4.0 * MATRIX_GAP) / 4.0 - 0.5,
                ));
                l.flex_shrink = Some(0.0);
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
        )
        .children(cell)
        .into_any()
    });
    widget(
        Stack::row(MATRIX_GAP)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .padding_xy(INSET, ROW_PAD_Y),
    )
    .children((
        widget(
            Stack::row(4.0)
                .align(AlignSpec::Center)
                .width(LengthSpec::Px(MATRIX_LABEL_W))
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .children(label),
        cells.into_iter().collect::<Vec<_>>(),
    ))
    .into_any()
}

/// A row label, with an info mark whose tooltip says what the options do.
fn matrix_label(ui: Ui, key: &str, tip: Option<&str>) -> AnyView {
    widget(Stack::row(2.0).align(AlignSpec::Center))
        .children((
            widget(Text::new(ui.now_t(key)).style(text_style(13.0, 500))),
            tip.map(|tip| {
                widget(
                    IconButton::new(ti::INFO_CIRCLE, ui.now_t(tip))
                        .size(ControlSize::Small)
                        .kind(ButtonKind::Ghost)
                        .with_tooltip(ui.now_t(tip)),
                )
            }),
        ))
        .into_any()
}

/// "—": the setting does not apply to this window.
fn not_applicable() -> AnyView {
    widget(
        Text::new("—")
            .style(text_style(13.0, 400))
            .color(SemanticColorRole::Faint),
    )
    .into_any()
}

/// A switch bound to one field of `kind`'s overlay settings.
fn overlay_switch(
    ui: Ui,
    kind: WindowKind,
    get: fn(&nanatype_settings::OverlaySettings) -> bool,
    set: fn(&mut nanatype_settings::OverlaySettings, bool),
) -> AnyView {
    let read = move |s: &Settings| kind.overlay(s).is_some_and(get);
    widget(Switch::new("", read(&ui.settings.get_untracked())))
        .bind(move |s: &mut Switch| s.checked = ui.settings.with(|x| read(x)))
        .on_cx(move |_, e: &ToggleChanged, cx| {
            let v = e.checked;
            cx.dispatch_program_all(edit(move |s| {
                if let Some(o) = kind.overlay_mut(s) {
                    set(o, v);
                }
            }));
        })
        .into_any()
}

/// A select filling its cell, bound to one field of `kind`'s overlay
/// settings; options are `(value, message key)`.
fn overlay_select(
    ui: Ui,
    kind: WindowKind,
    options: &[(&'static str, &'static str)],
    get: fn(&nanatype_settings::OverlaySettings) -> &'static str,
    set: fn(&mut nanatype_settings::OverlaySettings, &str),
) -> AnyView {
    let read = move |s: &Settings| kind.overlay(s).map_or("", get);
    let opts: Vec<SelectOption> = select_keys(ui, options)
        .into_iter()
        .map(|(v, l)| SelectOption::new(v, l))
        .collect();
    widget(Stack::row(0.0).width(LengthSpec::Fill))
        .children(
            widget(
                Select::new(Some(read(&ui.settings.get_untracked()).to_owned()))
                    .options(opts)
                    .size(ControlSize::Small),
            )
            .bind(move |s: &mut Select| {
                let v: Arc<str> = ui.settings.with(|x| read(x)).into();
                if s.value.as_ref() != Some(&v) {
                    s.value = Some(v);
                }
            })
            .on_cx(move |_, e: &SelectChanged, cx| {
                let v = e.value.to_string();
                cx.dispatch_program_all(edit(move |s| {
                    if let Some(o) = kind.overlay_mut(s) {
                        set(o, &v);
                    }
                }));
            }),
        )
        .into_any()
}

/// The overlay windows side by side: one line per setting, one
/// column per window, "—" where a setting does not apply.
fn windows(ui: Ui) -> AnyView {
    let header = matrix_row(
        widget(Stack::row(0.0)).into_any(),
        OVERLAYS.map(|kind| {
            let key = match kind {
                WindowKind::Display => "windows-display",
                WindowKind::PeerDisplay => "windows-peer-display",
                WindowKind::History => "windows-history",
                _ => "windows-input",
            };
            widget(
                Text::new(ui.now_t(key))
                    .style(one_line(12.0, 600))
                    .color(SemanticColorRole::Muted),
            )
            .into_any()
        }),
    );
    let show = matrix_row(
        matrix_label(ui, "windows-visible", None),
        OVERLAYS.map(|kind| overlay_switch(ui, kind, |o| o.visible, |o, v| o.visible = v)),
    );
    let lock = matrix_row(
        matrix_label(ui, "windows-lock", Some("windows-lock-tip")),
        OVERLAYS.map(|kind| {
            if kind == WindowKind::Input {
                return not_applicable();
            }
            overlay_select(
                ui,
                kind,
                &[
                    ("interactive", "windows-lock-interactive"),
                    ("locked", "windows-lock-locked-short"),
                ],
                |o| match o.lock {
                    Lock::Locked => "locked",
                    Lock::Interactive => "interactive",
                },
                |o, v| {
                    o.lock = if v == "locked" {
                        Lock::Locked
                    } else {
                        Lock::Interactive
                    }
                },
            )
        }),
    );
    let background = matrix_row(
        matrix_label(ui, "windows-background", Some("windows-background-tip")),
        OVERLAYS.map(|kind| {
            if kind == WindowKind::Input {
                return not_applicable();
            }
            overlay_select(
                ui,
                kind,
                &[
                    ("transparent", "windows-background-transparent"),
                    ("chroma", "windows-background-chroma"),
                ],
                |o| match o.background {
                    CaptureBackground::Transparent => "transparent",
                    CaptureBackground::Chroma { .. } => "chroma",
                },
                |o, v| {
                    o.background = if v == "chroma" {
                        CaptureBackground::Chroma {
                            color: nanatype_model::Rgba8::rgb(0, 255, 0),
                        }
                    } else {
                        CaptureBackground::Transparent
                    }
                },
            )
        }),
    );
    let on_top = matrix_row(
        matrix_label(ui, "windows-on-top", None),
        OVERLAYS
            .map(|kind| overlay_switch(ui, kind, |o| o.always_on_top, |o, v| o.always_on_top = v)),
    );
    // The size the window was dragged to ("Follow skin" until it is), and
    // the reset that puts it back where and as big as the skin says.
    let size = matrix_row(
        matrix_label(ui, "windows-size", Some("windows-reset-tip")),
        OVERLAYS.map(|kind| {
            let text = move || {
                let size = ui.settings.with(|s| kind.overlay(s).and_then(|o| o.size));
                match size {
                    Some([w, h]) => format!("{}×{}", w.round(), h.round()),
                    None => I18N.t_in(ui.locale.get(), "windows-size-skin"),
                }
            };
            widget(
                Stack::row(4.0)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill),
            )
            .children((
                widget(
                    Text::new(text())
                        .style(one_line(12.0, 500))
                        .color(SemanticColorRole::Muted),
                )
                .bind(move |t: &mut Text| {
                    let s = text();
                    if t.value != s {
                        t.value = s;
                    }
                }),
                widget(
                    IconButton::new(ti::RESTORE, ui.now_t("windows-reset"))
                        .size(ControlSize::Small)
                        .with_tooltip(ui.now_t("windows-reset")),
                )
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(Msg::ResetWindow(kind));
                }),
            ))
            .into_any()
        }),
    );
    widget(Stack::column(SECTION_GAP))
        .children((
            section(ui, "", vec![header, show, lock, background, on_top, size]),
            widget(Stack::column(0.0).padding_xy(INSET, space::MD)).children((
                hint(ui.now_t("windows-layout-hint")),
                hint(ui.now_t("windows-input-na")),
            )),
        ))
        .into_any()
}

fn history(ui: Ui) -> AnyView {
    widget(Stack::column(SECTION_GAP))
        .children((
            section(
                ui,
                "",
                vec![
                    row(
                        ui,
                        "history-sync",
                        None,
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("submit", "history-sync-submit"),
                                    ("reveal", "history-sync-reveal"),
                                    ("complete", "history-sync-complete"),
                                ],
                            ),
                            |s| match s.history.sync {
                                HistorySync::OnSubmit => "submit".into(),
                                HistorySync::Reveal => "reveal".into(),
                                HistorySync::OnComplete => "complete".into(),
                            },
                            |s, v| {
                                s.history.sync = match v {
                                    "submit" => HistorySync::OnSubmit,
                                    "complete" => HistorySync::OnComplete,
                                    _ => HistorySync::Reveal,
                                }
                            },
                        ),
                    ),
                    row(
                        ui,
                        "history-hide-latest",
                        Some("history-hide-latest-hint"),
                        select(
                            ui,
                            select_keys(
                                ui,
                                &[
                                    ("skin", "history-hide-latest-skin"),
                                    ("always", "history-hide-latest-always"),
                                    ("never", "history-hide-latest-never"),
                                ],
                            ),
                            |s| match s.history.hide_latest {
                                HideLatest::Skin => "skin".into(),
                                HideLatest::Always => "always".into(),
                                HideLatest::Never => "never".into(),
                            },
                            |s, v| {
                                s.history.hide_latest = match v {
                                    "always" => HideLatest::Always,
                                    "never" => HideLatest::Never,
                                    _ => HideLatest::Skin,
                                }
                            },
                        ),
                    ),
                    row(
                        ui,
                        "history-visible",
                        None,
                        number(
                            ui,
                            (1.0, 200.0),
                            |s| f64::from(s.history.visible_items),
                            |s, v| s.history.visible_items = v as u32,
                        ),
                    ),
                    row(
                        ui,
                        "history-max",
                        None,
                        number(
                            ui,
                            (1.0, 5000.0),
                            |s| f64::from(s.history.max_items),
                            |s, v| s.history.max_items = v as u32,
                        ),
                    ),
                    row(
                        ui,
                        "history-fade",
                        None,
                        slider_with(
                            ui,
                            (0.0, 300.0, 5.0),
                            "s",
                            off_at_zero,
                            |s| f64::from(s.history.fade_after_s.unwrap_or(0)),
                            |s, v| s.history.fade_after_s = (v > 0.0).then_some(v as u32),
                        ),
                    ),
                    row(
                        ui,
                        "history-avatar",
                        None,
                        switch(
                            ui,
                            |s| s.history.show_avatar,
                            |s, v| s.history.show_avatar = v,
                        ),
                    ),
                    row(
                        ui,
                        "history-name",
                        None,
                        switch(ui, |s| s.history.show_name, |s, v| s.history.show_name = v),
                    ),
                    row(
                        ui,
                        "history-time",
                        None,
                        switch(ui, |s| s.history.show_time, |s, v| s.history.show_time = v),
                    ),
                    row(
                        ui,
                        "history-session",
                        None,
                        confirm(
                            ui,
                            ui.now_t("history-clear-confirm"),
                            ui.now_t("history-clear"),
                            |armed| {
                                widget(
                                    Button::new(ui.now_t("history-clear")).size(ControlSize::Small),
                                )
                                .on(move |_: &Activate| armed.set(true))
                                .into_any()
                            },
                            || Msg::Core(nanatype_core::Cmd::ClearHistory),
                        ),
                    ),
                ],
            ),
            section(ui, "history-chat", {
                let mut rows = vec![
                    row(
                        ui,
                        "history-chat-show",
                        Some("history-chat-show-hint"),
                        switch(
                            ui,
                            |s| s.history.chat.enabled,
                            |s, v| s.history.chat.enabled = v,
                        ),
                    ),
                    row(
                        ui,
                        "history-show-quote",
                        Some("history-show-quote-hint"),
                        switch(
                            ui,
                            |s| s.history.show_quote,
                            |s, v| s.history.show_quote = v,
                        ),
                    ),
                ];
                rows.extend([
                    row(
                        ui,
                        "chat-filter-text",
                        None,
                        switch(
                            ui,
                            |s| s.history.chat.filter.text,
                            |s, v| s.history.chat.filter.text = v,
                        ),
                    ),
                    row(
                        ui,
                        "chat-filter-super-chat",
                        None,
                        switch(
                            ui,
                            |s| s.history.chat.filter.super_chat,
                            |s, v| s.history.chat.filter.super_chat = v,
                        ),
                    ),
                    row(
                        ui,
                        "chat-filter-gift",
                        None,
                        switch(
                            ui,
                            |s| s.history.chat.filter.gift,
                            |s, v| s.history.chat.filter.gift = v,
                        ),
                    ),
                    row(
                        ui,
                        "chat-filter-member",
                        None,
                        switch(
                            ui,
                            |s| s.history.chat.filter.member,
                            |s, v| s.history.chat.filter.member = v,
                        ),
                    ),
                    row(
                        ui,
                        "chat-min-gift",
                        Some("chat-min-gift-hint"),
                        number(
                            ui,
                            (0.0, 10000.0),
                            |s| f64::from(s.history.chat.filter.min_gift_coin) / 1000.0,
                            |s, v| {
                                s.history.chat.filter.min_gift_coin = (v * 1000.0).round() as u32
                            },
                        ),
                    ),
                ]);
                rows
            }),
            // Where the window itself and its capture are set.
            section(
                ui,
                "",
                vec![row(
                    ui,
                    "history-window-link",
                    Some("history-window-link-hint"),
                    widget(Stack::row(4.0).align(AlignSpec::Center))
                        .children((page_link(ui, Page::Windows), page_link(ui, Page::Output)))
                        .into_any(),
                )],
            ),
        ))
        .into_any()
}

/// "Windows →": a quiet button that opens another settings page.
fn page_link(ui: Ui, page: Page) -> AnyView {
    widget(
        Button::new(format!("{} →", ui.now_t(page.label_key())))
            .kind(ButtonKind::Ghost)
            .size(ControlSize::Small),
    )
    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::SettingsPage(page)))
    .into_any()
}

/// What is wrong with an action's binding, worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyIssue {
    /// Another action has the same chord.
    Duplicate(GlobalAction),
    /// Another program owns the chord.
    Conflict,
    /// The chord would swallow typing, or cannot be registered.
    Unsafe,
}

impl KeyIssue {
    fn badge_key(self) -> &'static str {
        match self {
            Self::Duplicate(_) => "hotkeys-badge-dup",
            Self::Conflict => "hotkeys-badge-conflict",
            Self::Unsafe => "hotkeys-badge-unsafe",
        }
    }

    fn tone(self) -> StatusTone {
        match self {
            Self::Unsafe => StatusTone::Warning,
            _ => StatusTone::Danger,
        }
    }

    fn color(self) -> SemanticColorRole {
        match self {
            Self::Unsafe => SemanticColorRole::Warning,
            _ => SemanticColorRole::Danger,
        }
    }

    fn message(self, locale: Locale, s: &Settings) -> String {
        match self {
            Self::Duplicate(other) => I18N.tr_in(
                locale,
                "hotkeys-conflict-dup",
                &[("action", action_name(locale, s, other).into())],
            ),
            Self::Conflict => I18N.t_in(locale, "hotkeys-conflict-os"),
            Self::Unsafe => I18N.t_in(locale, "hotkeys-unsafe"),
        }
    }
}

/// What an action is called: its label, or a phrase's own label.
fn action_name(locale: Locale, s: &Settings, action: GlobalAction) -> String {
    match action {
        GlobalAction::Phrase(i) => s
            .assets
            .phrases
            .get(i)
            .map(|p| p.label.trim().to_owned())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| I18N.t_in(locale, "assets-phrases")),
        _ => I18N.t_in(locale, action.label_key()),
    }
}

fn key_issue(
    s: &Settings,
    states: &HashMap<GlobalAction, HotkeyState>,
    action: GlobalAction,
) -> Option<KeyIssue> {
    let mine = action.get(s)?;
    if let Some(other) = GlobalAction::all_in(s).find(|o| *o != action && o.get(s) == Some(mine)) {
        return Some(KeyIssue::Duplicate(other));
    }
    match states.get(&action) {
        Some(HotkeyState::Conflict) => Some(KeyIssue::Conflict),
        Some(HotkeyState::Invalid) => Some(KeyIssue::Unsafe),
        _ if !mine.is_safe_global() => Some(KeyIssue::Unsafe),
        _ => None,
    }
}

const META_LABEL: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Win"
};

/// Most keys a chord has: Ctrl, Shift, Alt, Win and the key.
const CHORD_KEYS: usize = 5;

/// The `i`th key of a chord as a keycap, "+" before all but the first. The
/// caps are fixed nodes that follow the binding: a settings row measures
/// its control once, so the control must not be rebuilt.
fn keycap(
    i: usize,
    parts: impl Fn() -> Vec<String> + Send + Copy + 'static,
    danger: impl Fn() -> bool + Send + Copy + 'static,
) -> AnyView {
    let shown = move || parts().len() > i;
    widget(Stack::row(4.0).align(AlignSpec::Center))
        .visible(shown)
        .children((
            (i > 0).then(|| widget(crate::ui::keycap::plus())),
            widget(crate::ui::keycap::face())
                .border(move || {
                    Some(if danger() {
                        SemanticColorRole::Danger
                    } else {
                        SemanticColorRole::BorderSoft
                    })
                })
                .children(widget(crate::ui::keycap::label(String::new())).bind(
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

fn hotkey_control(ui: Ui, action: GlobalAction) -> AnyView {
    let recording = move || ui.recording.get() == Some(action);
    let bound = move || ui.settings.with(|s| action.get(s).is_some());
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
            None => ui
                .hotkeys
                .with(|h| matches!(h.get(&action), Some(HotkeyState::Registered)))
                .then(|| (I18N.t_in(locale, "hotkeys-ok"), StatusTone::Success)),
        }
    };
    let caps: Vec<AnyView> = (0..CHORD_KEYS).map(|i| keycap(i, parts, danger)).collect();
    let slot = widget(
        Stack::row(0.0)
            .height(LengthSpec::Px(24.0))
            .padding_xy(space::LG, 0.0)
            .align(AlignSpec::Center)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(nana_ui_core::RadiusTier::Sm)
            .with_layout(|l| l.border_style = Some(nana_ui_core::BorderStyle::Dashed)),
    )
    .children(widget(
        Text::new(ui.now_t("hotkeys-click-to-set"))
            .style(text_style(12.0, 500))
            .color(SemanticColorRole::Muted),
    ));
    // Recording is a live state: the highlight outline, not the accent.
    let listening = widget(
        Stack::row(0.0)
            .min_width(LengthSpec::Px(160.0))
            .height(LengthSpec::Px(24.0))
            .justify(JustifySpec::Center)
            .align(AlignSpec::Center)
            .outline(SemanticColorRole::Highlight, 1.0)
            .radius(nana_ui_core::RadiusTier::Sm),
    )
    .children(widget(
        Text::new(ui.now_t("hotkeys-press-keys"))
            .style(text_style(12.0, 500))
            .color(SemanticColorRole::Text),
    ));
    widget(Stack::row(8.0).align(AlignSpec::Center))
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
                widget(Stack::row(4.0).align(AlignSpec::Center))
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
        ))
        .into_any()
}

/// The wrapping line under a hotkey row that says what is wrong.
fn hotkey_issue_line(ui: Ui, action: GlobalAction) -> AnyView {
    let issue = move || {
        let states = ui.hotkeys.get();
        ui.settings.with(|s| key_issue(s, &states, action))
    };
    widget(Stack::column(0.0).with_layout(|l| {
        l.padding_left = Some(LengthSpec::Px(INSET));
        l.padding_right = Some(LengthSpec::Px(INSET));
        l.padding_bottom = Some(LengthSpec::Px(space::XL));
    }))
    .visible(move || issue().is_some())
    .children(
        widget(Text::new(String::new()).style(text_style(12.0, 400))).bind(move |t: &mut Text| {
            if let Some(i) = issue() {
                let text = ui.settings.with(|s| i.message(ui.locale.get(), s));
                if t.value != text {
                    t.value = text;
                }
                t.style.foreground = Some(i.color());
            }
        }),
    )
    .into_any()
}

fn hotkeys(ui: Ui) -> AnyView {
    let rows: Vec<AnyView> = GlobalAction::ALL
        .into_iter()
        .map(|action| {
            widget(Stack::column(space::XS))
                .children((
                    row(ui, action.label_key(), None, hotkey_control(ui, action)),
                    hotkey_issue_line(ui, action),
                ))
                .into_any()
        })
        .collect();
    widget(Stack::column(SECTION_GAP))
        .children((
            section(ui, "", rows),
            section(
                ui,
                "hotkeys-local",
                vec![block(hint(ui.now_t("hotkeys-local-hint")))],
            ),
        ))
        .into_any()
}

/// A numbered step: a 20px numbered circle, then what to do.
fn step(n: u8, body: impl IntoView) -> AnyView {
    widget(Stack::row(10.0).align(AlignSpec::Center))
        .children((
            widget(
                Stack::row(0.0)
                    .width(LengthSpec::Px(20.0))
                    .height(LengthSpec::Px(20.0))
                    .align(AlignSpec::Center)
                    .justify(JustifySpec::Center)
                    .surface(SemanticColorRole::Selected)
                    .radius_px(10.0)
                    .with_layout(|l| l.flex_shrink = Some(0.0)),
            )
            .children(widget(
                Text::new(n.to_string())
                    .style(text_style(12.0, 700))
                    .color(SemanticColorRole::Text),
            )),
            widget(
                Stack::row(8.0)
                    .align(AlignSpec::Center)
                    .wrap(true)
                    .grow(1.0)
                    .shrink(1.0),
            )
            .children(body),
        ))
        .into_any()
}

/// Getting the display into OBS: enable output before selecting its live sender
/// name, other capture methods folded under one line, and a sample line.
fn obs_guide(ui: Ui) -> AnyView {
    let body = |key: &str| {
        widget(
            Text::new(ui.now_t(key))
                .style(text_style(13.0, 400))
                .color(SemanticColorRole::Text),
        )
        .into_any()
    };
    let sender = widget(
        Stack::row(4.0)
            .align(AlignSpec::Center)
            .padding_xy(space::MD, space::XXS)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius_px(6.0),
    )
    .children((
        widget(Text::new(String::new()).style(mono_style(12.0, 500))).bind(move |t: &mut Text| {
            let name = ui.settings.with(|s| s.output.display.name.clone());
            if t.value != name {
                t.value = name;
            }
        }),
        widget(
            IconButton::new(ti::COPY, ui.now_t("output-spout-copy"))
                .size(ControlSize::Small)
                .with_tooltip(ui.now_t("output-spout-copy")),
        )
        .on_cx(move |_, _: &Activate, cx| {
            let name = ui
                .settings
                .with_untracked(|s| s.output.display.name.clone());
            cx.dispatch_program_all(Msg::CopyText(name))
        }),
    ));
    let enable = || {
        step(
            1,
            (
                body("output-obs-step-enable"),
                widget(crate::skin_theme::cta(
                    Button::new(ui.now_t("output-obs-enable"))
                        .kind(ButtonKind::Primary)
                        .size(ControlSize::Small),
                ))
                .bind(move |b: &mut Button| {
                    let enabled = ui.settings.with(|s| s.output.display.enabled);
                    b.label = ui.now_t(if enabled {
                        "output-obs-enabled"
                    } else {
                        "output-obs-enable"
                    });
                    b.disabled = enabled;
                })
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(edit(|s| s.output.display.enabled = true));
                }),
                link_badge(ui, |s| &s.spout_display),
            ),
        )
    };
    let steps: Vec<AnyView> = if cfg!(windows) {
        vec![
            enable(),
            step(2, body("output-obs-step-plugin")),
            step(3, (body("output-obs-step-source"), sender)),
            step(4, body("output-obs-step-alpha")),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            enable(),
            step(2, body("output-obs-step-syphon")),
            step(3, (body("output-obs-step-syphon-source"), sender)),
            step(4, body("output-obs-step-alpha")),
        ]
    } else {
        let _ = sender;
        vec![hint(ui.now_t("output-spout-unsupported"))]
    };
    let other_open = signal(!(cfg!(windows) || cfg!(target_os = "macos")));
    let other = widget(Stack::column(6.0).align(AlignSpec::Start)).children((
        widget(
            Button::new(ui.now_t("output-obs-other"))
                .kind(ButtonKind::Text)
                .size(ControlSize::Small)
                .icon(ti::CHEVRON_RIGHT),
        )
        .bind(move |b: &mut Button| {
            let icon = if other_open.get() {
                ti::CHEVRON_DOWN
            } else {
                ti::CHEVRON_RIGHT
            };
            if b.icon != Some(icon) {
                b.icon = Some(icon);
            }
        })
        .on(move |_: &Activate| other_open.set(!other_open.get_untracked())),
        widget(Stack::column(0.0).with_layout(|l| {
            l.padding_left = Some(LengthSpec::Px(space::MD));
        }))
        .visible(move || other_open.get())
        .children(hint(ui.now_t(if cfg!(target_os = "macos") {
            "output-obs-mac"
        } else {
            "output-obs-window"
        }))),
    ));
    widget(Stack::column(space::LG).padding_xy(INSET, BLOCK_PAD_Y))
        .children((
            steps,
            other,
            widget(Stack::row(0.0)).children(primary(ui, "output-sample", || Msg::PlaySample)),
        ))
        .into_any()
}

fn output(ui: Ui) -> AnyView {
    let spout = |kind: WindowKind, title: &'static str| {
        let link: fn(&crate::ui::Status) -> &LinkState = match kind {
            WindowKind::Display => |s| &s.spout_display,
            WindowKind::PeerDisplay => |s| &s.spout_peer_display,
            _ => |s| &s.spout_history,
        };
        let read: fn(&Settings) -> &nanatype_settings::SpoutOutput = match kind {
            WindowKind::Display => |s| &s.output.display,
            WindowKind::PeerDisplay => |s| &s.output.peer_display,
            _ => |s| &s.output.history,
        };
        let write: fn(&mut Settings) -> &mut nanatype_settings::SpoutOutput = match kind {
            WindowKind::Display => |s| &mut s.output.display,
            WindowKind::PeerDisplay => |s| &mut s.output.peer_display,
            _ => |s| &mut s.output.history,
        };
        section(
            ui,
            title,
            vec![
                row(
                    ui,
                    if cfg!(target_os = "macos") {
                        "output-syphon"
                    } else {
                        "output-spout"
                    },
                    (kind == WindowKind::PeerDisplay).then_some("output-peer-hint"),
                    widget(Stack::row(10.0).align(AlignSpec::Center))
                        .children((
                            link_badge(ui, link),
                            switch(
                                ui,
                                move |s| read(s).enabled,
                                move |s, v| write(s).enabled = v,
                            ),
                        ))
                        .into_any(),
                ),
                when_on(
                    ui,
                    move |s| read(s).enabled,
                    vec![
                        row(
                            ui,
                            "output-spout-name",
                            None,
                            widget(Stack::row(6.0))
                                .children((
                                    text(
                                        ui,
                                        String::new(),
                                        220.0,
                                        move |s| read(s).name.clone(),
                                        move |s, v| write(s).name = v,
                                    ),
                                    widget(
                                        IconButton::new(ti::COPY, ui.now_t("output-spout-copy"))
                                            .size(ControlSize::Small)
                                            .with_tooltip(ui.now_t("output-spout-copy")),
                                    )
                                    .on_cx(
                                        move |_, _: &Activate, cx| {
                                            // Read when clicked: the name may have been edited since the
                                            // page was built.
                                            let current =
                                                read(&ui.settings.get_untracked()).name.clone();
                                            cx.dispatch_program_all(Msg::CopyText(current))
                                        },
                                    ),
                                ))
                                .into_any(),
                        ),
                        row(
                            ui,
                            "output-spout-size",
                            None,
                            select(
                                ui,
                                vec![
                                    ("window".into(), ui.now_t("output-spout-size-window")),
                                    ("1280x720".into(), "1280 × 720".into()),
                                    ("1920x1080".into(), "1920 × 1080".into()),
                                    ("2560x1440".into(), "2560 × 1440".into()),
                                    ("3840x2160".into(), "3840 × 2160".into()),
                                ],
                                move |s| {
                                    read(s)
                                        .size
                                        .map(|[w, h]| format!("{w}x{h}"))
                                        .unwrap_or_else(|| "window".into())
                                },
                                move |s, v| {
                                    let size = v.split_once('x').and_then(|(w, h)| {
                                        Some([w.parse().ok()?, h.parse().ok()?])
                                    });
                                    write(s).size = size;
                                },
                            ),
                        ),
                        row(
                            ui,
                            "output-spout-straight",
                            Some("output-spout-straight-hint"),
                            switch(
                                ui,
                                move |s| read(s).straight_alpha,
                                move |s, v| write(s).straight_alpha = v,
                            ),
                        ),
                    ],
                    "output-off-hint",
                ),
            ],
        )
    };
    widget(Stack::column(SECTION_GAP))
        .children((
            section(ui, "output-obs-title", vec![obs_guide(ui)]),
            spout(WindowKind::Display, "output-display"),
            spout(WindowKind::PeerDisplay, "output-peer-display"),
            spout(WindowKind::History, "output-history"),
        ))
        .into_any()
}

/// Width of the hotkey column of the VTS mapping table.
const VTS_SELECT_W: f32 = 240.0;

/// A marker as typed in a line: monospace on a quiet chip.
fn marker_chip(text: String) -> AnyView {
    widget(
        Stack::row(0.0)
            .padding_xy(space::MD, space::XXS)
            .align(AlignSpec::Center)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius_px(6.0)
            .with_layout(|l| l.flex_shrink = Some(1.0)),
    )
    .children(widget(
        Text::new(text)
            .style(mono_style(12.0, 500))
            .color(SemanticColorRole::Text),
    ))
    .into_any()
}

/// One marker's row: its chip, then the VTS hotkey it presses ("Auto" lets
/// the bridge match the hotkey of the same name).
fn vts_map_row(ui: Ui, key: String, options: Vec<SelectOption>) -> AnyView {
    let current = {
        let key = key.clone();
        move || {
            ui.settings
                .with(|s| s.integrations.vts.expressions.get(&key).cloned())
                .unwrap_or_default()
        }
    };
    let start = current();
    let set_key = key.clone();
    widget(
        Stack::bar(space::XL)
            .padding_xy(INSET, ROW_PAD_Y)
            .align(AlignSpec::Center),
    )
    .children((
        widget(
            Stack::row(space::SM)
                .align(AlignSpec::Center)
                .grow(1.0)
                .shrink(1.0),
        )
        .children((
            marker_chip(format!("[expr={key}]")),
            widget(IconGlyph::new(ti::ARROW_RIGHT).size(14.0).decorative()),
        )),
        widget(Stack::row(0.0).width(LengthSpec::Px(VTS_SELECT_W))).children(
            widget(
                Select::new(Some(start))
                    .options(options)
                    .size(ControlSize::Small),
            )
            .bind(move |s: &mut Select| {
                let v: Arc<str> = current().into();
                if s.value.as_ref() != Some(&v) {
                    s.value = Some(v);
                }
            })
            .on_cx(move |_, e: &SelectChanged, cx| {
                let (k, id) = (set_key.clone(), e.value.to_string());
                cx.dispatch_program_all(edit(move |s| {
                    let map = &mut s.integrations.vts.expressions;
                    if id.is_empty() {
                        map.remove(&k);
                    } else {
                        map.insert(k, id);
                    }
                }))
            }),
        ),
    ))
    .into_any()
}

/// Expression markers mapped to VTube Studio hotkeys: access first, then a
/// two-column table of every marker the model's hotkeys (or a saved
/// mapping) give.
fn vts_mapping(ui: Ui) -> AnyView {
    let authorized = move || {
        ui.settings.with(|s| s.integrations.vts.token.is_some())
            && ui.status.with(|s| s.vts != LinkState::NeedsAuth)
    };
    let header = widget(
        Stack::bar(space::XL)
            .padding_xy(INSET, space::XL)
            .align(AlignSpec::Center),
    )
    .children((
        widget(Stack::column(2.0).grow(1.0).shrink(1.0)).children((
            subheading(ui, "int-vts-map"),
            widget(Stack::column(0.0))
                .visible(move || !authorized())
                .children(hint(ui.now_t("int-vts-authorize-hint"))),
        )),
        widget(Stack::row(6.0).align(AlignSpec::Center)).children((
            widget(
                StatusBadge::new(ui.now_t("int-vts-authorized"), StatusTone::Success).compact(true),
            )
            .visible(authorized),
            // With nothing listed, the empty state below carries the button.
            widget(Stack::row(0.0))
                .visible(move || {
                    !authorized()
                        && (ui.status.with(|s| !s.vts_hotkeys.is_empty())
                            || ui
                                .settings
                                .with(|s| !s.integrations.vts.expressions.is_empty()))
                })
                .children(button(ui, "int-vts-authorize", || Msg::VtsAuthorize)),
            widget(
                IconButton::new(ti::REFRESH, ui.now_t("int-vts-refresh"))
                    .size(ControlSize::Small)
                    .with_tooltip(ui.now_t("int-vts-refresh")),
            )
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::VtsRefresh)),
        )),
    ));
    let table = dynamic(
        move || {
            let (hotkeys, connected) = ui.status.with(|s| {
                (
                    s.vts_hotkeys.clone(),
                    matches!(s.vts, LinkState::Connected(_)),
                )
            });
            let mapped = ui.settings.with(|s| {
                s.integrations
                    .vts
                    .expressions
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
            });
            (hotkeys, connected, mapped)
        },
        move |(hotkeys, connected, mapped)| {
            let mut markers: Vec<String> = Vec::new();
            let names = hotkeys.iter().map(|(_, n)| nanatype_bridge::mapping_key(n));
            for key in names.chain(mapped.iter().map(|(k, _)| k.clone())) {
                if !key.is_empty() && !markers.contains(&key) {
                    markers.push(key);
                }
            }
            if markers.is_empty() {
                return if *connected {
                    crate::ui::feedback::empty_state(
                        ui.now_t("empty-vts-hotkeys"),
                        Some((ui.now_t("int-vts-refresh"), Box::new(|| Msg::VtsRefresh))),
                    )
                } else {
                    crate::ui::feedback::empty_state(
                        ui.now_t("int-vts-map-empty"),
                        Some((
                            ui.now_t("int-vts-authorize"),
                            Box::new(|| Msg::VtsAuthorize),
                        )),
                    )
                };
            }
            let mut options = vec![SelectOption::new("", ui.now_t("int-vts-map-auto"))];
            options.extend(
                hotkeys
                    .iter()
                    .map(|(id, name)| SelectOption::new(id.clone(), name.clone())),
            );
            for (_, id) in mapped {
                if !hotkeys.iter().any(|(h, _)| h == id) {
                    options.push(SelectOption::new(id.clone(), id.clone()));
                }
            }
            let caption = |key: &str| {
                widget(
                    Text::new(ui.now_t(key))
                        .style(text_style(12.0, 600))
                        .color(SemanticColorRole::Muted),
                )
            };
            let head = widget(
                Stack::bar(space::XL)
                    .padding_xy(INSET, 0.0)
                    .align(AlignSpec::Center)
                    .with_layout(|l| l.padding_bottom = Some(LengthSpec::Px(space::XS))),
            )
            .children((
                widget(Stack::row(0.0).grow(1.0)).children(caption("int-vts-map-expression")),
                widget(Stack::row(0.0).width(LengthSpec::Px(VTS_SELECT_W)))
                    .children(caption("int-vts-map-hotkey")),
            ));
            let rows: Vec<AnyView> = markers
                .into_iter()
                .map(|key| vts_map_row(ui, key, options.clone()))
                .collect();
            widget(Stack::column(0.0).with_layout(|l| {
                l.padding_bottom = Some(LengthSpec::Px(space::SM));
            }))
            .children((head, divided(rows)))
            .into_any()
        },
    );
    widget(Stack::column(0.0))
        .children((header, table))
        .into_any()
}

fn integrations(ui: Ui) -> AnyView {
    widget(Stack::column(SECTION_GAP))
        .children((
            section(
                ui,
                "int-vts",
                vec![
                    row(
                        ui,
                        "int-vts",
                        Some("int-vts-hint"),
                        widget(Stack::row(10.0).align(AlignSpec::Center))
                            .children((
                                link_badge(ui, |s| &s.vts),
                                switch(
                                    ui,
                                    |s| s.integrations.vts.enabled,
                                    |s, v| s.integrations.vts.enabled = v,
                                ),
                            ))
                            .into_any(),
                    ),
                    when_on(
                        ui,
                        |s| s.integrations.vts.enabled,
                        vec![
                            row(
                                ui,
                                "int-vts-port",
                                None,
                                port(
                                    ui,
                                    |s| s.integrations.vts.port,
                                    |s, v| s.integrations.vts.port = v,
                                ),
                            ),
                            vts_mapping(ui),
                            row(
                                ui,
                                "int-vts-lipsync",
                                Some("int-vts-lipsync-hint"),
                                switch(
                                    ui,
                                    |s| s.integrations.vts.lip_sync,
                                    |s, v| s.integrations.vts.lip_sync = v,
                                ),
                            ),
                            row(
                                ui,
                                "int-vts-mouth-param",
                                Some("int-vts-mouth-param-hint"),
                                text(
                                    ui,
                                    "MouthOpen".into(),
                                    180.0,
                                    |s| s.integrations.vts.mouth_param.clone(),
                                    |s, v| s.integrations.vts.mouth_param = v,
                                ),
                            ),
                            row(
                                ui,
                                "int-vts-mouth-amp",
                                None,
                                slider(
                                    ui,
                                    (0.1, 1.0, 0.05),
                                    "",
                                    |s| f64::from(s.integrations.vts.mouth_amp),
                                    |s, v| s.integrations.vts.mouth_amp = v as f32,
                                ),
                            ),
                            row(
                                ui,
                                "int-vts-reset",
                                Some("int-vts-reset-hint"),
                                select(
                                    ui,
                                    select_keys(
                                        ui,
                                        &[
                                            ("line_end", "int-vts-reset-line-end"),
                                            ("next_line", "int-vts-reset-next-line"),
                                            ("off", "int-vts-reset-off"),
                                        ],
                                    ),
                                    |s| s.integrations.vts.reset_expressions.key().into(),
                                    |s, v| {
                                        s.integrations.vts.reset_expressions =
                                            VtsReset::from_key(v).unwrap_or_default()
                                    },
                                ),
                            ),
                        ],
                        "int-off-hint",
                    ),
                ],
            ),
            section(
                ui,
                "int-osc",
                vec![
                    row(
                        ui,
                        "int-osc",
                        Some("int-osc-hint"),
                        widget(Stack::row(10.0).align(AlignSpec::Center))
                            .children((
                                link_badge(ui, |s| &s.osc),
                                switch(
                                    ui,
                                    |s| s.integrations.osc.enabled,
                                    |s, v| s.integrations.osc.enabled = v,
                                ),
                            ))
                            .into_any(),
                    ),
                    when_on(
                        ui,
                        |s| s.integrations.osc.enabled,
                        vec![
                            row(
                                ui,
                                "int-osc-host",
                                None,
                                code_text(
                                    ui,
                                    "127.0.0.1".into(),
                                    180.0,
                                    |s| s.integrations.osc.host.clone(),
                                    |s, v| s.integrations.osc.host = v,
                                ),
                            ),
                            row(
                                ui,
                                "int-osc-port",
                                None,
                                port(
                                    ui,
                                    |s| s.integrations.osc.port,
                                    |s, v| s.integrations.osc.port = v,
                                ),
                            ),
                            row(
                                ui,
                                "int-osc-prefix",
                                None,
                                code_text(
                                    ui,
                                    "/nanatype".into(),
                                    180.0,
                                    |s| s.integrations.osc.prefix.clone(),
                                    |s, v| s.integrations.osc.prefix = v,
                                ),
                            ),
                        ],
                        "int-off-hint",
                    ),
                ],
            ),
            section(
                ui,
                "int-ws",
                vec![
                    row(
                        ui,
                        "int-ws",
                        Some("int-ws-hint"),
                        widget(Stack::row(10.0).align(AlignSpec::Center))
                            .children((
                                link_badge(ui, |s| &s.ws),
                                switch(
                                    ui,
                                    |s| s.integrations.websocket.enabled,
                                    |s, v| s.integrations.websocket.enabled = v,
                                ),
                            ))
                            .into_any(),
                    ),
                    when_on(
                        ui,
                        |s| s.integrations.websocket.enabled,
                        vec![row(
                            ui,
                            "int-ws-port",
                            None,
                            port(
                                ui,
                                |s| s.integrations.websocket.port,
                                |s, v| {
                                    // Below 1024 needs admin rights; keep the last good port.
                                    if v >= 1024 {
                                        s.integrations.websocket.port = v;
                                    }
                                },
                            ),
                        )],
                        "int-off-hint",
                    ),
                ],
            ),
        ))
        .into_any()
}

/// A trash button that arms a [`confirm`].
fn remove_button(ui: Ui, armed: Signal<bool>) -> AnyView {
    widget(
        IconButton::new(ti::TRASH, ui.now_t("assets-remove"))
            .size(ControlSize::Small)
            .with_tooltip(ui.now_t("assets-remove")),
    )
    .on(move |_: &Activate| armed.set(true))
    .into_any()
}

fn assets(ui: Ui) -> AnyView {
    let dirs = dynamic(
        move || (ui.sticker_packs.get(), ui.locale.get()),
        move |(packs, _)| {
            use crate::stickers::Phase;
            let rows: Vec<AnyView> = packs
                .iter()
                .map(|pack| {
                    let title = pack
                        .preset
                        .map(|p| ui.now_t(p.title_key()))
                        .unwrap_or_else(|| pack.title.clone());
                    let status = match &pack.phase {
                        Phase::NotDownloaded => ui.now_t("stickers-not-downloaded"),
                        Phase::Downloading(done, total) => ui.tr(
                            "stickers-downloading",
                            &[
                                ("done", (*done as u64).into()),
                                ("total", (*total as u64).into()),
                            ],
                        ),
                        Phase::Processing(done, total) => ui.tr(
                            "stickers-processing",
                            &[
                                ("done", (*done as u64).into()),
                                ("total", (*total as u64).into()),
                            ],
                        ),
                        Phase::Ready => {
                            ui.tr("stickers-ready", &[("count", (pack.count as u64).into())])
                        }
                        Phase::Failed(error) => {
                            ui.tr("stickers-failed", &[("message", error.clone().into())])
                        }
                    };
                    let details = [
                        status,
                        pack.author.clone(),
                        pack.license.clone(),
                        if pack.preset.is_none() {
                            pack.root.display().to_string()
                        } else {
                            String::new()
                        },
                    ]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                    let mut actions = Vec::new();
                    if pack
                        .repository
                        .as_deref()
                        .is_some_and(|url| url.starts_with("https://"))
                    {
                        let root = pack.root.clone();
                        let label = if pack
                            .repository
                            .as_deref()
                            .is_some_and(|u| u.starts_with("https://t.me/"))
                        {
                            "telegram-source"
                        } else {
                            "stickers-repository"
                        };
                        actions.push(button(ui, label, move || {
                            Msg::StickerRepository(root.clone())
                        }));
                    }
                    if !pack.phase.busy() && pack.phase != Phase::Ready {
                        let root = pack.root.clone();
                        let preset = pack.preset;
                        actions.push(button(
                            ui,
                            if pack.phase == Phase::NotDownloaded {
                                "stickers-download"
                            } else {
                                "stickers-retry"
                            },
                            move || {
                                preset
                                    .map(Msg::StickerDownload)
                                    .unwrap_or_else(|| Msg::StickerRetry(root.clone()))
                            },
                        ));
                    }
                    if pack.phase != Phase::NotDownloaded {
                        let root = pack.root.clone();
                        if pack.phase.busy() {
                            actions.push(button(ui, "common-cancel", move || {
                                Msg::StickerRemove(root.clone())
                            }));
                        } else {
                            actions.push(confirm(
                                ui,
                                ui.now_t("assets-remove-folder-confirm"),
                                ui.now_t("assets-remove"),
                                |armed| remove_button(ui, armed),
                                move || Msg::StickerRemove(root.clone()),
                            ));
                        }
                    }
                    row_text(
                        title,
                        Some(details),
                        widget(Stack::row(space::SM).wrap(true))
                            .children(actions)
                            .into_any(),
                    )
                })
                .collect();
            widget(Stack::column(0.0)).children(rows).into_any()
        },
    );
    let phrases = dynamic(
        move || ui.settings.with(|s| s.assets.phrases.len()),
        move |n| {
            if *n == 0 {
                return crate::ui::feedback::empty_state(ui.now_t("empty-phrases"), None);
            }
            let rows: Vec<AnyView> = (0..*n)
                .map(|i| {
                    let p = ui.settings.with_untracked(|s| s.assets.phrases[i].clone());
                    let action = GlobalAction::Phrase(i);
                    // Two lines: name, hotkey, send and remove; then the
                    // content across the full width.
                    let first = widget(
                        Stack::row(10.0)
                            .align(AlignSpec::Center)
                            .width(LengthSpec::Fill),
                    )
                    .children((
                        widget(Stack::row(0.0).width(LengthSpec::Px(200.0))).children(
                            widget(
                                TextInput::new(p.label.clone())
                                    .size(ControlSize::Small)
                                    .placeholder(ui.now_t("assets-phrase-label")),
                            )
                            .on_cx(move |_, e: &TextChanged, cx| {
                                let v = e.value.to_string();
                                cx.dispatch_program_all(edit(move |s| {
                                    if let Some(p) = s.assets.phrases.get_mut(i) {
                                        p.label = v;
                                    }
                                }))
                            }),
                        ),
                        hotkey_control(ui, action),
                        widget(Stack::row(0.0).grow(1.0)),
                        widget(
                            Text::new(ui.now_t("assets-phrase-send"))
                                .style(text_style(12.0, 500))
                                .color(SemanticColorRole::Muted),
                        ),
                        widget(Switch::new("", p.send))
                            .bind(move |w: &mut Switch| {
                                w.checked = ui
                                    .settings
                                    .with(|s| s.assets.phrases.get(i).is_some_and(|p| p.send))
                            })
                            .on_cx(move |_, e: &ToggleChanged, cx| {
                                let v = e.checked;
                                cx.dispatch_program_all(edit(move |s| {
                                    if let Some(p) = s.assets.phrases.get_mut(i) {
                                        p.send = v;
                                    }
                                }))
                            }),
                        confirm(
                            ui,
                            ui.now_t("assets-phrase-remove-confirm"),
                            ui.now_t("assets-remove"),
                            |armed| remove_button(ui, armed),
                            move || {
                                edit(move |s| {
                                    if i < s.assets.phrases.len() {
                                        s.assets.phrases.remove(i);
                                    }
                                })
                            },
                        ),
                    ));
                    // Markup source, in the mono role.
                    let content = widget(
                        Stack::row(0.0)
                            .width(LengthSpec::Fill)
                            .with_layout(crate::ui::mono),
                    )
                    .children(
                        widget(
                            TextInput::new(p.markup.clone())
                                .size(ControlSize::Small)
                                .placeholder(ui.now_t("assets-phrase-markup")),
                        )
                        .on_cx(move |_, e: &TextChanged, cx| {
                            let v = e.value.to_string();
                            cx.dispatch_program_all(edit(move |s| {
                                if let Some(p) = s.assets.phrases.get_mut(i) {
                                    p.markup = v;
                                }
                            }))
                        }),
                    );
                    widget(Stack::column(space::XS))
                        .children((
                            widget(Stack::column(space::MD).padding_xy(INSET, space::XL))
                                .children((first, content)),
                            hotkey_issue_line(ui, action),
                        ))
                        .into_any()
                })
                .collect();
            widget(Stack::column(0.0))
                .children(divided(rows))
                .into_any()
        },
    );
    widget(Stack::column(SECTION_GAP))
        .children((
            section(
                ui,
                "assets-stickers",
                vec![
                    widget(Stack::column(0.0).padding_xy(INSET, space::LG))
                        .children(hint(ui.now_t("assets-stickers-hint")))
                        .into_any(),
                    dirs.into_any(),
                    widget(Stack::row(0.0).padding_xy(INSET, space::LG))
                        .children(button(ui, "assets-add-folder", || {
                            Msg::PickFile(FileTarget::StickerFolder)
                        }))
                        .into_any(),
                ],
            ),
            super::telegram::panel(ui),
            section(
                ui,
                "assets-phrases",
                vec![
                    phrases.into_any(),
                    widget(Stack::row(0.0).padding_xy(INSET, space::LG))
                        .children(button(ui, "assets-phrase-add", || {
                            edit(|s| {
                                s.assets.phrases.push(QuickPhrase {
                                    label: String::new(),
                                    markup: String::new(),
                                    send: false,
                                    hotkey: None,
                                })
                            })
                        }))
                        .into_any(),
                ],
            ),
        ))
        .into_any()
}

fn about(ui: Ui) -> AnyView {
    let locale = ui.locale.get_untracked();
    let build = I18N.tr_in(
        locale,
        "about-build",
        &[("build", env!("NANATYPE_BUILD_ID").into())],
    );
    let paths = crate::paths::Paths::resolve();
    let (logs, data) = (paths.logs.clone(), paths.data.clone());
    let icon = widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(48.0))
            .height(LengthSpec::Px(48.0))
            .with_layout(|l| {
                l.flex_shrink = Some(0.0);
                l.paint.background_image = Some(BackgroundImage::url_with_fit(
                    format!("data:image/png;base64,{}", ICON_PNG_B64.as_str()),
                    nana_ui_core::BackgroundImageFit::Contain,
                ));
            }),
    );
    let version_chip = widget(
        Stack::row(0.0)
            .padding_xy(space::MD, space::XXS)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius_px(999.0),
    )
    .children(widget(
        Text::new(format!("v{}", env!("CARGO_PKG_VERSION")))
            .style(mono_style(type_scale::META, type_scale::MEDIUM))
            .color(SemanticColorRole::Muted),
    ));
    let header = widget(
        Stack::row(space::XXL)
            .padding_xy(INSET, INSET)
            .align(AlignSpec::Center),
    )
    .children((
        icon,
        widget(Stack::column(space::XS).grow(1.0).shrink(1.0)).children((
            widget(Stack::row(space::MD).align(AlignSpec::Center)).children((
                widget(
                    Text::new("NanaType").style(text_style(type_scale::TITLE, type_scale::BOLD)),
                ),
                version_chip,
            )),
            hint_13(ui.now_t("app-tagline")),
            hint(build),
        )),
    ));
    let link = |key: &str, icon: nana_ui::runtime::Icon, msg: Box<dyn Fn() -> Msg + Send>| {
        let mut button = Button::new(ui.now_t(key))
            .kind(ButtonKind::Ghost)
            .size(ControlSize::Small)
            .icon(icon);
        button.style.control_padding_x = None;
        widget(button)
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(msg()))
            .into_any()
    };
    let links = widget(
        Stack::row(space::SM)
            .padding_xy(INSET, space::LG)
            .wrap(true),
    )
    .children(vec![
        link(
            "about-logs",
            ti::FILE_TEXT,
            Box::new(move || Msg::OpenFolder(logs.clone())),
        ),
        link(
            "about-data",
            ti::FOLDER_OPEN,
            Box::new(move || Msg::OpenFolder(data.clone())),
        ),
        link(
            "about-homepage",
            ti::EXTERNAL_LINK,
            Box::new(|| Msg::OpenUrl(HOMEPAGE)),
        ),
        // The first-run guide again, from its first step, on General.
        {
            let mut guide = Button::new(ui.now_t("about-setup-guide"))
                .kind(ButtonKind::Ghost)
                .size(ControlSize::Small)
                .icon(ti::SPARKLES);
            guide.style.control_padding_x = None;
            widget(guide)
        }
        .on_cx(move |_, _: &Activate, cx| {
            ui.onboard_step.set(0);
            cx.dispatch_program_all(edit(|s| s.general.onboarding_done = false));
            cx.dispatch_program_all(Msg::SettingsPage(Page::General));
        })
        .into_any(),
    ]);
    let mut rows = vec![(header.into_any(), None), (links.into_any(), None)];
    rows.extend(interface_licences(ui));
    rows.extend(design_sources(ui));
    rows.extend(font_licences(ui));
    widget(Stack::column(SECTION_GAP))
        .children(section_with(ui, "", rows))
        .into_any()
}

fn design_sources(ui: Ui) -> Vec<(AnyView, Shown)> {
    let link = |key: &str, url: &'static str| {
        widget(
            Button::new(ui.now_t(key))
                .kind(ButtonKind::Ghost)
                .size(ControlSize::Small)
                .icon(ti::EXTERNAL_LINK),
        )
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::OpenUrl(url)))
    };
    vec![(
        widget(Stack::column(space::SM).padding_xy(INSET, space::LG))
            .children((
                widget(Text::new(ui.now_t("about-design")).style(text_style(13.0, 600))),
                widget(Text::new("Echo-Live · Sheep-realms").style(text_style(13.0, 600))),
                hint(ui.now_t("about-design-echo")),
                widget(Stack::row(space::SM).wrap(true)).children((
                    link(
                        "about-source-repository",
                        "https://github.com/sheep-realms/Echo-Live",
                    ),
                    link(
                        "about-design-upstream-license",
                        "https://github.com/sheep-realms/Echo-Live/blob/master/copyright.md",
                    ),
                )),
            ))
            .into_any(),
        None,
    )]
}

/// Sources stay visible; each licence is embedded for offline reading.
fn interface_licences(ui: Ui) -> Vec<(AnyView, Shown)> {
    let mut rows = vec![(
        widget(Stack::column(space::SM).padding_xy(INSET, space::LG))
            .children((
                widget(Text::new(ui.now_t("about-interfaces")).style(text_style(13.0, 600))),
                hint(ui.now_t("about-interfaces-hint")),
            ))
            .into_any(),
        None,
    )];
    let entries = [
        (
            "NanaType · MIT AND CC-BY-NC-4.0",
            "about-license-project",
            HOMEPAGE,
            None,
            concat!(
                include_str!("../../../../LICENSE"),
                "\n",
                include_str!("../../../../LICENSE-MIT")
            ),
        ),
        (
            "SocialSisterYi/bilibili-API-collect · CC BY-NC 4.0",
            "about-source-api-collect",
            "https://github.com/SocialSisterYi/bilibili-API-collect",
            Some("https://github.com/bilibili-plugins/bilibili-api-collect"),
            include_str!("../../../../crates/bilibili/LICENSE-API-COLLECT"),
        ),
        (
            "jwwsjlm/douyinLive · MIT",
            "about-source-douyin",
            "https://github.com/jwwsjlm/douyinLive/tree/0e506f3cf935beede946296602af2e52d57ecea2",
            None,
            include_str!("../../../../crates/douyin/LICENSE-DOUYINLIVE"),
        ),
    ];
    for (name, description, url, mirror, licence) in entries {
        let open = signal(false);
        let source_link = |key: &str, url: &'static str| {
            widget(
                Button::new(ui.now_t(key))
                    .kind(ButtonKind::Ghost)
                    .size(ControlSize::Small)
                    .icon(ti::EXTERNAL_LINK),
            )
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::OpenUrl(url)))
            .into_any()
        };
        let mut buttons = vec![source_link("about-source-repository", url)];
        if let Some(mirror) = mirror {
            buttons.push(source_link("about-source-mirror", mirror));
        }
        buttons.push(
            widget(Button::new(ui.now_t("about-fonts-show")).size(ControlSize::Small))
                .visible(move || !open.get())
                .on(move |_: &Activate| open.set(true))
                .into_any(),
        );
        buttons.push(
            widget(Button::new(ui.now_t("about-fonts-hide")).size(ControlSize::Small))
                .visible(move || open.get())
                .on(move |_: &Activate| open.set(false))
                .into_any(),
        );
        rows.push((
            widget(Stack::column(space::SM).padding_xy(INSET, space::LG))
                .children((
                    widget(Text::new(name).style(text_style(13.0, 600))),
                    hint(ui.now_t(description)),
                    widget(Stack::row(space::SM).wrap(true)).children(buttons),
                ))
                .into_any(),
            None,
        ));
        rows.push(shown_if(
            widget(Stack::column(0.0).padding_xy(INSET, space::LG))
                .children(
                    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                        l.height = Some(LengthSpec::Px(260.0));
                    }))
                    .children(widget(Stack::column(0.0)).children(licence_lines(licence))),
                )
                .into_any(),
            move || open.get(),
        ));
    }
    rows
}

/// The "Open-source fonts" row and, below it while open, a scrolling panel
/// with each font this build embeds: its name, author, licence and the
/// licence's full text (OFL 1.1 asks that it travel with the font). Nothing
/// when the build embeds no fonts.
fn font_licences(ui: Ui) -> Vec<(AnyView, Shown)> {
    let fonts = nanatype_theme::BuiltinFonts::embedded().fonts();
    if fonts.is_empty() {
        return Vec::new();
    }
    let open = signal(false);
    let toggle = |key: &str, shown: bool| {
        widget(Button::new(ui.now_t(key)).size(ControlSize::Small))
            .visible(move || open.get() == shown)
            .on(move |_: &Activate| open.set(!shown))
            .into_any()
    };
    let row_view = row(
        ui,
        "about-fonts",
        Some("about-fonts-hint"),
        widget(Stack::row(0.0))
            .children((
                toggle("about-fonts-show", false),
                toggle("about-fonts-hide", true),
            ))
            .into_any(),
    );
    vec![
        (row_view, None),
        shown_if(font_licence_panel(ui), move || open.get()),
    ]
}

/// Each font this build embeds, its author, version and licence, and the
/// licence's full text, in a scrolling panel. Shown under About's
/// "Open-source fonts" row.
pub fn font_licence_panel(ui: Ui) -> AnyView {
    let fonts = nanatype_theme::BuiltinFonts::embedded().fonts();
    let locale = ui.locale.get_untracked();
    let entries: Vec<AnyView> = fonts
        .iter()
        .map(|font| {
            let name = font.family().unwrap_or_else(|| font.path.to_owned());
            let mut facts = Vec::new();
            if let Some(author) = font.author() {
                facts.push(I18N.tr_in(locale, "about-fonts-author", &[("author", author.into())]));
            }
            if let Some(version) = font.version() {
                facts.push(I18N.tr_in(
                    locale,
                    "about-fonts-version",
                    &[("version", version.into())],
                ));
            }
            facts.push(match font.license_name() {
                Some(l) => l.to_owned(),
                None => I18N.t_in(locale, "about-fonts-license-other"),
            });
            widget(Stack::column(4.0))
                .children((
                    widget(Text::new(name).style(text_style(13.0, 600))),
                    hint(facts.join(" · ")),
                    widget(Stack::column(0.0).with_layout(|l| {
                        l.padding_top = Some(LengthSpec::Px(space::SM));
                    }))
                    .children(licence_lines(font.license)),
                ))
                .into_any()
        })
        .collect();
    widget(Stack::column(0.0).padding_xy(INSET, space::LG))
        .children(
            widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                l.height = Some(LengthSpec::Px(320.0));
            }))
            .children(
                widget(Stack::column(space::XXXL).with_layout(|l| {
                    l.padding_right = Some(LengthSpec::Px(space::XL));
                }))
                .children(entries),
            ),
        )
        .into_any()
}

/// A licence's text as it is laid out upstream: one text per line (a text
/// folds its line breaks), a blank line as a gap.
fn licence_lines(text: &str) -> Vec<AnyView> {
    text.trim_end()
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                widget(Stack::column(0.0).height(LengthSpec::Px(8.0))).into_any()
            } else {
                widget(
                    Text::new(line.to_owned())
                        .style(mono_style(11.0, 400))
                        .color(SemanticColorRole::Muted),
                )
                .into_any()
            }
        })
        .collect()
}

/// Where the project lives.
const HOMEPAGE: &str = "https://github.com/sena-nana/NanaType";
