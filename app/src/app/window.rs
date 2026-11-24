use super::App;
use super::shell::QuickBarState;
use crate::msg::WindowMsg;
use crate::windows::WindowKind;
use nana_ui_platform::host::WindowCommand;
use nanatype_settings::InputPresence;

impl App {
    /// Window visibility, settings navigation and displays.
    pub(super) fn window_msg(&mut self, msg: WindowMsg) {
        match msg {
            WindowMsg::OverlayAction(kind, action) => {
                // Native menus are snapshots. Recheck before applying an action
                // if the window, connection or settings changed while tracking.
                if crate::services::overlay_menu::Snapshot::new(kind, self.ui)
                    .is_some_and(|menu| menu.enabled(&action))
                    && let Ok(app_msg) = action.message(kind).into_domain()
                {
                    self.update(app_msg);
                }
            }
            WindowMsg::Toggle(kind) => match kind {
                WindowKind::Input
                    if self.settings.general.input_presence == InputPresence::Summon =>
                {
                    self.shell.summoned = !self.shell.summoned;
                }
                WindowKind::Settings => self.open_settings(),
                WindowKind::PeerDisplay(_) if self.peer.handle.is_none() => {}
                WindowKind::Danmaku if !self.settings.history.chat.separate => {}
                kind => self.edit(|s| {
                    if let Some(o) = kind.overlay_mut(s) {
                        o.visible = !o.visible;
                    }
                }),
            },
            WindowMsg::Show(kind) => match kind {
                WindowKind::Input => self.show_input_window(),
                WindowKind::Settings => self.open_settings(),
                kind => self.edit(|s| {
                    if let Some(o) = kind.overlay_mut(s) {
                        o.visible = true;
                    }
                }),
            },
            WindowMsg::HideInput => self.set_input_visibility(false),
            WindowMsg::Reset(kind) => {
                self.shell.windows.forget(kind);
                self.edit(|s| kind.clear_saved_size(s));
            }
            WindowMsg::OpenSettings => self.open_settings(),
            WindowMsg::SettingsPlatform(platform) => {
                self.ui.platform_tab.set_if_changed(platform);
                self.open_settings_page(crate::ui::settings::Page::Platforms);
            }
            WindowMsg::SettingsPage(page) => {
                self.open_settings_page(page);
            }
            WindowMsg::SecondInstance => {
                self.show_input_window();
                self.open_settings();
            }
            WindowMsg::Displays(monitors) => {
                self.shell.windows.monitors = monitors;
            }
        }
    }

    pub(super) fn show_input_window(&mut self) {
        self.draft.focus_editor = true;
        self.set_input_visibility(true);
        self.shell
            .commands
            .push(WindowCommand::Focus(WindowKind::Input.id()));
    }

    pub(super) fn open_settings_page(&mut self, page: crate::ui::settings::Page) {
        self.ui.page.set(page);
        if page == crate::ui::settings::Page::History {
            self.archive_page_shown();
        }
        self.open_settings();
    }

    pub(super) fn quick_bar_open(&self) -> bool {
        matches!(self.shell.quick, QuickBarState::Visible { .. })
    }
}
