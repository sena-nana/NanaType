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
mod peer;
mod platforms;
mod playback;
mod profiles;
mod skin_grid;
mod skins;
mod speech;
mod windows;
mod workshop;

pub use self::about::font_licence_panel;
pub use self::hotkeys::action_name;
pub use self::profiles::import_dialog;

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
use self::profiles::*;
use self::skins::*;
use self::windows::*;

use std::collections::HashMap;
use std::sync::Arc;

use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, signal, widget};
use nana_ui::runtime::{
    ActionMenu, ActionMenuItem, Activate, Button, Chip, ColorChanged, ColorField, IconButton,
    IconGlyph, ListItem, PopoverClosed, PopoverToggled, ScrollAxes, ScrollView, Select,
    SelectChanged, SelectOption, Stack, StatusBadge, Switch, Text, TextChanged, ToggleChanged,
};
use nana_ui::theme::space;
use nana_ui::{ButtonKind, ControlSize, StatusTone};
use nana_ui_core::{
    AlignSpec, BackgroundImage, JustifySpec, LengthSpec, OverflowSpec, PositionSpec, RadiusTier,
    SemanticColorMix, SemanticColorRole,
};
use nanatype_i18n::Locale;
use nanatype_model::{AfterLine, EntranceKind, Overflow, Rgba8, VtsReset};
use nanatype_playback::QueuePolicy;
use nanatype_settings::{
    CaptureBackground, ChromeMode, ContrastMode, HideLatest, HistorySync, InputKey, InputMode,
    InputPresence, KeystrokeScheme, Lock, QuickPhrase, Refusal, Settings,
};

use super::Page;
use super::controls::*;
use crate::msg::{ChatMsg, FileTarget, GlobalAction, Msg, Shortcut};
use crate::services::hotkeys::HotkeyState;
use crate::ui::{I18N, LinkState, SkinRow, Ui};
use crate::windows::WindowKind;

pub fn page(ui: Ui, page: Page) -> AnyView {
    let body = match page {
        Page::General => general(ui),
        Page::Profiles => profiles_page(ui),
        Page::Input => input_page(ui),
        Page::Characters => characters_page(ui),
        Page::Assets => assets(ui),
        Page::Speech => speech::settings(ui),
        Page::Hotkeys => hotkeys(ui),
        Page::Skins => skins(ui),
        Page::Playback => playback(ui),
        Page::Windows => windows(ui),
        Page::History => history(ui),
        Page::Platforms => platforms(ui),
        Page::Output => output(ui),
        Page::Integrations => integrations(ui),
        Page::Peer => peer::peer(ui),
        Page::Workshop => workshop::workshop(ui),
        Page::Packages => crate::ui::plugins::settings(ui),
        Page::About => about(ui),
    };
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children(body)
        .into_any()
}

/// The open page's page-wide actions, shown in its header.
pub fn actions(ui: Ui, page: Page) -> Option<AnyView> {
    match page {
        Page::Skins => Some(crate::ui::kit::hstack((
            crate::distribution::STEAM
                .then(|| workshop::browse_button(ui, crate::workshop::Tag::Skins)),
            tool(ui, ti::DOWNLOAD, "skins-import", || {
                Msg::PickFile(FileTarget::ImportSkin)
            }),
        ))),
        Page::Workshop => crate::distribution::STEAM
            .then(|| workshop::browse_button(ui, crate::workshop::Tag::Skins)),
        Page::Packages => Some(crate::ui::plugins::actions(ui)),
        Page::Platforms => Some(tool(ui, ti::MESSAGES, "chat-open-panel", || {
            Msg::Chat(ChatMsg::Show)
        })),
        Page::Output => Some(tool(ui, ti::PLAYER_PLAY, "output-sample", || {
            Msg::PlaySample
        })),
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

/// The active character's name field, bound to the setting.
fn speaker_field(ui: Ui) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(field::MD))
            .with_layout(|l| {
                l.flex_shrink = Some(0.0);
            }),
    )
    .children(text(
        ui,
        String::new(),
        field::MD,
        |s| s.general.active_character().name,
        |s, v| {
            let i = s.general.character;
            if let Some(c) = s.general.characters.get_mut(i) {
                c.name = v;
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
