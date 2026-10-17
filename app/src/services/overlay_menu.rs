//! Native menus stay on the window thread, outside broadcast GPU scenes.
use nana_ui::{RuntimeProgramContext, WindowHandle};
use nanatype_core::Cmd;
use nanatype_i18n::Locale;
use nanatype_settings::{InputMode, Lock, Settings};
use std::sync::Arc;
use tray_icon::menu::{CheckMenuItem, ContextMenu, MenuItem, PredefinedMenuItem, Submenu};

use crate::{
    msg::Msg,
    ui::{I18N, SkinRow, Ui},
    windows::WindowKind,
};

const PREFIX: &str = "nt.overlay:";
const WINDOWS: [WindowKind; 3] = [
    WindowKind::Display,
    WindowKind::PeerDisplay,
    WindowKind::History,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Skin(String),
    AdjustSkin,
    OnTop,
    Lock,
    Reset,
    Close,
    Continue,
    Skip,
    ClearDisplay,
    Avatar,
    Name,
    Time,
    ClearHistory,
}

impl Action {
    const FIXED: [Self; 12] = [
        Self::AdjustSkin,
        Self::OnTop,
        Self::Lock,
        Self::Reset,
        Self::Close,
        Self::Continue,
        Self::Skip,
        Self::ClearDisplay,
        Self::Avatar,
        Self::Name,
        Self::Time,
        Self::ClearHistory,
    ];

    fn key(&self) -> &str {
        match self {
            Self::Skin(id) => id,
            Self::AdjustSkin => "adjust-skin",
            Self::OnTop => "on-top",
            Self::Lock => "lock",
            Self::Reset => "reset",
            Self::Close => "close",
            Self::Continue => "continue",
            Self::Skip => "skip",
            Self::ClearDisplay => "clear-display",
            Self::Avatar => "avatar",
            Self::Name => "name",
            Self::Time => "time",
            Self::ClearHistory => "clear-history",
        }
    }

    fn id(&self, kind: WindowKind) -> String {
        let skin = if matches!(self, Self::Skin(_)) {
            "skin:"
        } else {
            ""
        };
        format!("{PREFIX}{}:{skin}{}", kind.key(), self.key())
    }

    pub(crate) fn message(self, kind: WindowKind) -> Msg {
        match self {
            Self::Skin(id) => Msg::Edit(Box::new(move |s| s.skin.id = id)),
            Self::AdjustSkin => Msg::SettingsPage(crate::ui::settings::Page::Skins),
            Self::OnTop => Msg::Edit(Box::new(move |s| {
                let o = kind.overlay_mut(s).expect("broadcast overlay");
                o.always_on_top = !o.always_on_top;
            })),
            Self::Lock => Msg::Edit(Box::new(move |s| {
                kind.overlay_mut(s).expect("broadcast overlay").lock = Lock::Locked;
            })),
            Self::Reset => Msg::ResetWindow(kind),
            Self::Close => Msg::Edit(Box::new(move |s| {
                kind.overlay_mut(s).expect("broadcast overlay").visible = false
            })),
            Self::Continue => Msg::Core(Cmd::Continue),
            Self::Skip => Msg::Core(Cmd::Skip),
            Self::ClearDisplay => Msg::Core(Cmd::ClearDisplay),
            Self::Avatar => Msg::Edit(Box::new(|s| s.history.show_avatar = !s.history.show_avatar)),
            Self::Name => Msg::Edit(Box::new(|s| s.history.show_name = !s.history.show_name)),
            Self::Time => Msg::Edit(Box::new(|s| s.history.show_time = !s.history.show_time)),
            Self::ClearHistory => Msg::Core(Cmd::ClearHistory),
        }
    }
}

/// Decode only this menu's namespace; skin IDs may themselves contain colons.
pub(crate) fn message(id: &str) -> Option<Msg> {
    let (window, key) = id.strip_prefix(PREFIX)?.split_once(':')?;
    let kind = WINDOWS.into_iter().find(|kind| kind.key() == window)?;
    let action = match key.strip_prefix("skin:") {
        Some("") => return None,
        Some(id) => Action::Skin(id.to_owned()),
        None => Action::FIXED
            .into_iter()
            .find(|action| action.key() == key)?,
    };
    Some(Msg::OverlayAction(kind, action))
}

/// Owned state crosses to the native thread. App rechecks these availability
/// rules before executing a selection from a menu that may have become stale.
pub(crate) struct Snapshot {
    kind: WindowKind,
    settings: Arc<Settings>,
    skins: Arc<Vec<SkinRow>>,
    locale: Locale,
    peer_active: bool,
    connected: bool,
    history_empty: bool,
}

impl Snapshot {
    pub(crate) fn new(kind: WindowKind, ui: Ui) -> Option<Self> {
        let settings = ui.settings.get_untracked();
        let overlay = kind.overlay(&settings)?;
        if !WINDOWS.contains(&kind) || !overlay.visible || overlay.lock == Lock::Locked {
            return None;
        }
        let peer = ui.peer.get_untracked();
        Some(Self {
            kind,
            settings,
            skins: ui.skins.get_untracked(),
            locale: ui.locale.get_untracked(),
            peer_active: peer.active,
            connected: !peer.active
                || matches!(peer.status, nanatype_p2p::Status::Connected { .. }),
            history_empty: ui.history.get_untracked().is_empty(),
        })
    }

    pub(crate) fn enabled(&self, action: &Action) -> bool {
        match action {
            Action::Skin(id) => self
                .skins
                .iter()
                .any(|skin| skin.id == *id && skin.error.is_none()),
            Action::Continue | Action::Skip => {
                self.kind == WindowKind::Display
                    && self.connected
                    && self.settings.general.input_mode == InputMode::Compose
            }
            Action::ClearDisplay => self.kind == WindowKind::Display && self.connected,
            Action::Avatar | Action::Name | Action::Time => self.kind == WindowKind::History,
            Action::ClearHistory => {
                self.kind == WindowKind::History && !self.peer_active && !self.history_empty
            }
            _ => true,
        }
    }

    fn text(&self, key: &str) -> String {
        I18N.t_in(self.locale, key)
    }

    fn item(
        &self,
        menu: &Submenu,
        action: Action,
        key: &str,
        checked: Option<bool>,
    ) -> Result<(), tray_icon::menu::Error> {
        let id = action.id(self.kind);
        let enabled = self.enabled(&action);
        let label = self.text(key);
        match checked {
            Some(checked) => {
                menu.append(&CheckMenuItem::with_id(id, label, enabled, checked, None))
            }
            None => menu.append(&MenuItem::with_id(id, label, enabled, None)),
        }
    }

    fn native(&self) -> Result<Submenu, tray_icon::menu::Error> {
        // A Submenu is a context menu on both Windows and macOS.
        let menu = Submenu::new("", true);
        if self.kind == WindowKind::Display {
            for (action, key) in [
                (Action::Continue, "overlay-menu-continue"),
                (Action::Skip, "overlay-menu-skip"),
                (Action::ClearDisplay, "overlay-menu-clear-display"),
            ] {
                self.item(&menu, action, key, None)?;
            }
            menu.append(&PredefinedMenuItem::separator())?;
        } else if self.kind == WindowKind::History {
            let h = &self.settings.history;
            for (action, key, checked) in [
                (Action::Avatar, "history-avatar", h.show_avatar),
                (Action::Name, "history-name", h.show_name),
                (Action::Time, "history-time", h.show_time),
            ] {
                self.item(&menu, action, key, Some(checked))?;
            }
            let key = if self.peer_active {
                "overlay-menu-clear-history-peer"
            } else {
                "history-clear"
            };
            let clear = Submenu::new(self.text(key), self.enabled(&Action::ClearHistory));
            self.item(
                &clear,
                Action::ClearHistory,
                "overlay-menu-confirm-clear",
                None,
            )?;
            menu.append(&clear)?;
            menu.append(&PredefinedMenuItem::separator())?;
        }
        let skins = Submenu::new(self.text("overlay-menu-skin"), !self.skins.is_empty());
        for skin in self.skins.iter() {
            let action = Action::Skin(skin.id.clone());
            skins.append(&CheckMenuItem::with_id(
                action.id(self.kind),
                skin.name.replace('&', "&&"),
                self.enabled(&action),
                skin.id == self.settings.skin.id,
                None,
            ))?;
        }
        menu.append(&skins)?;
        self.item(&menu, Action::AdjustSkin, "overlay-menu-adjust-skin", None)?;
        menu.append(&PredefinedMenuItem::separator())?;
        self.item(
            &menu,
            Action::OnTop,
            "windows-on-top",
            Some(
                self.kind
                    .overlay(&self.settings)
                    .expect("broadcast overlay")
                    .always_on_top,
            ),
        )?;
        self.item(&menu, Action::Lock, "overlay-menu-lock", None)?;
        self.item(&menu, Action::Reset, "overlay-menu-reset", None)?;
        menu.append(&PredefinedMenuItem::separator())?;
        self.item(&menu, Action::Close, "overlay-menu-close", None)?;
        Ok(menu)
    }
}

pub(crate) fn show(
    snapshot: Snapshot,
    handle: &WindowHandle,
    position: (f32, f32),
    post: RuntimeProgramContext<Msg>,
) {
    // The callback owns the menu for exactly the duration of native tracking.
    // Do not wait here: callbacks execute on the window's owning thread.
    drop(handle.with_native_handle(move |handle| {
        let menu = match snapshot.native() {
            Ok(menu) => menu,
            Err(error) => {
                post.dispatch(Msg::Notice {
                    text: error.to_string(),
                    error: true,
                });
                return;
            }
        };
        let position = Some(
            tray_icon::menu::dpi::LogicalPosition::new(
                f64::from(position.0),
                f64::from(position.1),
            )
            .into(),
        );
        match handle.as_raw() {
            #[cfg(windows)]
            raw_window_handle::RawWindowHandle::Win32(window) => {
                // SAFETY: the service borrows a live HWND on its owning thread.
                unsafe {
                    menu.show_context_menu_for_hwnd(window.hwnd.get(), position);
                }
            }
            #[cfg(target_os = "macos")]
            raw_window_handle::RawWindowHandle::AppKit(window) => {
                // SAFETY: the service borrows a live content NSView on the main thread.
                unsafe {
                    menu.show_context_menu_for_nsview(window.ns_view.as_ptr(), position);
                }
            }
            _ => {}
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Ui {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let skin = registry.load(nanatype_theme::DEFAULT_SKIN).unwrap().skin;
        let mut core = nanatype_core::Core::new(Settings::default(), skin.clone());
        core.handle(Cmd::SendMarkup("History".into()), 0);
        let ui = Ui::new(Arc::new(Settings::default()), skin, Locale::ZhCn);
        ui.history
            .set(Arc::new(core.history().iter().cloned().collect()));
        let mut skins = crate::ui::skin_rows(&registry, Locale::ZhCn);
        let mut failed = skins[0].clone();
        failed.id = "broken:skin".into();
        failed.name = "Broken & skin".into();
        failed.error = Some("Invalid skin".into());
        skins.push(failed);
        ui.skins.set(Arc::new(skins));
        ui
    }

    #[test]
    fn availability_rechecks_window_mode_connection_history_and_skins() {
        let ui = fixture();
        let enabled = |kind, action| Snapshot::new(kind, ui).is_some_and(|s| s.enabled(&action));
        for kind in WindowKind::ALL {
            assert_eq!(enabled(kind, Action::Close), WINDOWS.contains(&kind));
            assert_eq!(enabled(kind, Action::Continue), kind == WindowKind::Display);
            assert_eq!(enabled(kind, Action::Avatar), kind == WindowKind::History);
            assert_eq!(
                enabled(kind, Action::ClearHistory),
                kind == WindowKind::History
            );
        }
        assert!(!enabled(
            WindowKind::Display,
            Action::Skin("broken:skin".into())
        ));
        assert!(!enabled(
            WindowKind::Display,
            Action::Skin("removed".into())
        ));
        ui.settings
            .update(|s| Arc::make_mut(s).general.input_mode = InputMode::Mirror);
        assert!(!enabled(WindowKind::Display, Action::Continue));
        assert!(!enabled(WindowKind::Display, Action::Skip));
        assert!(enabled(WindowKind::Display, Action::ClearDisplay));
        ui.peer.update(|p| Arc::make_mut(p).active = true);
        assert!(!enabled(WindowKind::Display, Action::ClearDisplay));
        assert!(!enabled(WindowKind::History, Action::ClearHistory));
        assert!(enabled(WindowKind::History, Action::Avatar));
        ui.peer.update(|p| Arc::make_mut(p).active = false);
        ui.history.set(Arc::new(Vec::new()));
        assert!(!enabled(WindowKind::History, Action::ClearHistory));
        for kind in WINDOWS {
            ui.settings
                .update(|s| kind.overlay_mut(Arc::make_mut(s)).unwrap().lock = Lock::Locked);
            assert!(!enabled(kind, Action::Close));
            ui.settings.update(|s| {
                let o = kind.overlay_mut(Arc::make_mut(s)).unwrap();
                o.lock = Lock::Interactive;
                o.visible = false;
            });
            assert!(!enabled(kind, Action::Close));
        }
    }

    #[test]
    fn routing_preserves_window_targets_and_namespaced_skin_ids() {
        for kind in WINDOWS {
            for action in Action::FIXED
                .into_iter()
                .chain([Action::Skin("custom:skin".into())])
            {
                assert!(
                    matches!(message(&action.id(kind)), Some(Msg::OverlayAction(target, decoded)) if target == kind && decoded == action)
                );
            }
            let mut settings = Settings::default();
            for action in [Action::OnTop, Action::Lock, Action::Close] {
                let Msg::Edit(edit) = action.message(kind) else {
                    panic!("settings edit")
                };
                edit(&mut settings);
            }
            for other in WINDOWS {
                let o = other.overlay(&settings).unwrap();
                assert_eq!(o.visible, kind != other);
                assert_eq!(o.always_on_top, kind != other);
                assert_eq!(o.lock == Lock::Locked, kind == other);
            }
        }
        for id in [
            "nt.display",
            "nt.skin:nanatype.halo",
            "nt.overlay:input:close",
            "nt.overlay:history:unknown",
            "nt.overlay:display:skin:",
        ] {
            assert!(message(id).is_none(), "{id}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn native_menus_localize_checkmarks_skins_and_clear_confirmation() {
        use tray_icon::menu::MenuItemKind;
        let ui = fixture();
        for locale in Locale::ALL {
            ui.locale.set(locale);
            for kind in WINDOWS {
                let menu = Snapshot::new(kind, ui).unwrap().native().unwrap();
                let items = menu.items();
                assert!(items.iter().any(|i| matches!(i, MenuItemKind::MenuItem(i) if i.text() == ui.now_t("overlay-menu-close"))));
                assert!(items.iter().any(|i| matches!(i, MenuItemKind::Check(i) if i.id().0 == Action::OnTop.id(kind) && i.is_checked())));
                let leaves: Vec<_> = items
                    .iter()
                    .filter_map(|i| i.as_submenu())
                    .flat_map(|s| s.items())
                    .collect();
                assert!(leaves.iter().any(|i| matches!(i, MenuItemKind::Check(i) if i.id().0 == Action::Skin(ui.settings.get_untracked().skin.id.clone()).id(kind) && i.is_checked())));
                assert!(leaves.iter().any(|i| matches!(i, MenuItemKind::Check(i) if i.id().0.ends_with("skin:broken:skin") && !i.is_enabled() && i.text() == "Broken && skin")));
                assert_eq!(leaves.iter().any(|i| matches!(i, MenuItemKind::MenuItem(i) if i.id().0 == Action::ClearHistory.id(kind) && i.text() == ui.now_t("overlay-menu-confirm-clear"))), kind == WindowKind::History);
                assert!(!items.iter().any(|i| matches!(i, MenuItemKind::MenuItem(i) if i.id().0 == Action::ClearHistory.id(kind))));
            }
        }
    }
}
