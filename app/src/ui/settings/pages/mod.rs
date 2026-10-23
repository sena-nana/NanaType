//! The settings pages, one module per page; [`page`] picks one.

/// A [`row`] with a [`switch`] bound to the `bool` at `$field` of the
/// settings (`toggle_row!(ui, "history-name", history.show_name)`). Its hint
/// is the `{key}-hint` message, when there is one.
macro_rules! toggle_row {
    ($ui:expr, $key:literal, $($field:ident).+) => {
        row(
            $ui,
            $key,
            hint_key($key).as_deref(),
            switch($ui, |s| s.$($field).+, |s, v| s.$($field).+ = v),
        )
    };
}

mod about;
mod assets;
mod general;
mod history;
mod hotkeys;
mod integrations;
mod onboarding;
mod output;
mod platforms;
mod playback;
mod skin_grid;
mod skins;
mod windows;
mod workshop;

pub use self::about::font_licence_panel;

use self::about::*;
use self::assets::*;
use self::general::*;
use self::history::*;
use self::hotkeys::*;
use self::integrations::*;
use self::onboarding::*;
use self::output::*;
use self::platforms::*;
use self::playback::*;
use self::skins::*;
use self::windows::*;

use std::collections::HashMap;
use std::sync::Arc;

use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, signal, widget};
use nana_ui::runtime::{
    ActionMenu, ActionMenuItem, Activate, Button, Chip, IconButton, IconGlyph, ListItem,
    PopoverClosed, PopoverToggled, ScrollAxes, ScrollView, Select, SelectChanged, SelectOption,
    Stack, StatusBadge, Switch, Text, TextChanged, ToggleChanged,
};
use nana_ui::theme::space;
use nana_ui::{ButtonKind, ControlSize, StatusTone};
use nana_ui_core::{
    AlignSpec, BackgroundImage, JustifySpec, LengthSpec, OverflowSpec, PositionSpec, RadiusTier,
    SemanticColorMix, SemanticColorRole,
};
use nanatype_i18n::Locale;
use nanatype_model::{AfterLine, EntranceKind, Overflow, VtsReset};
use nanatype_playback::QueuePolicy;
use nanatype_settings::{
    CaptureBackground, ChromeMode, HideLatest, HistorySync, InputKey, InputMode, InputPresence,
    KeystrokeScheme, Lock, QuickPhrase, Settings,
};

use super::Page;
use super::controls::*;
use crate::msg::{FileTarget, GlobalAction, Msg, Shortcut};
use crate::services::hotkeys::HotkeyState;
use crate::ui::{I18N, LinkState, SkinRow, Ui};
use crate::windows::WindowKind;

pub fn page(ui: Ui, page: Page) -> AnyView {
    let body = match page {
        Page::General => general(ui),
        Page::Playback => playback(ui),
        Page::Skins => skins(ui),
        Page::Packages => crate::ui::plugins::settings(ui),
        Page::Windows => windows(ui),
        Page::History => history(ui),
        Page::Hotkeys => hotkeys(ui),
        Page::Output => output(ui),
        Page::Platforms => platforms(ui),
        Page::Integrations => integrations(ui),
        Page::Assets => assets(ui),
        Page::About => about(ui),
    };
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children(body)
        .into_any()
}

/// The open page's page-wide actions, shown in its header.
pub fn actions(ui: Ui, page: Page) -> Option<AnyView> {
    match page {
        Page::Skins if crate::distribution::STEAM => Some(
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .align(AlignSpec::Center),
            )
            .children((
                workshop::browse_button(ui, crate::workshop::Tag::Skins),
                primary(ui, "skins-import", || Msg::PickFile(FileTarget::ImportSkin)),
            ))
            .into_any(),
        ),
        Page::Skins => Some(primary(ui, "skins-import", || {
            Msg::PickFile(FileTarget::ImportSkin)
        })),
        Page::Packages => Some(crate::ui::plugins::actions(ui)),
        Page::Platforms => Some(button(ui, "chat-open-panel", || {
            Msg::ShowWindow(WindowKind::Chat)
        })),
        Page::Output => Some(button(ui, "output-sample", || Msg::PlaySample)),
        Page::History => Some(
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center),
            )
            .children((
                history::page_link(ui, Page::Windows),
                history::page_link(ui, Page::Output),
            ))
            .into_any(),
        ),
        _ => None,
    }
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
            .width(LengthSpec::Px(field::MD))
            .with_layout(|l| {
                l.flex_shrink = Some(0.0);
            }),
    )
    .children(dynamic(
        move || crate::ui::sources::profile_bound(ui),
        move |bound| {
            if *bound {
                dynamic(
                    move || ui.speaker.get().name,
                    move |name| {
                        widget(
                            Text::new(name.clone())
                                .style(Type::Body.style())
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
                    field::MD,
                    |s| s.general.speaker_name.clone(),
                    |s, v| s.general.speaker_name = v,
                )
            }
        },
    ))
    .into_any()
}

fn mirror(ui: Ui) -> bool {
    ui.settings
        .with(|s| matches!(s.general.input_mode, InputMode::Mirror))
}

/// `{key}-hint`, when that message exists.
fn hint_key(key: &str) -> Option<String> {
    let hint = format!("{key}-hint");
    I18N.has(&hint).then_some(hint)
}
