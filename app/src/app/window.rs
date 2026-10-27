use super::App;
use crate::msg::Msg;
use crate::windows::WindowKind;
use nana_ui_platform::host::WindowCommand;
use nanatype_settings::InputPresence;

impl App {
    /// Window visibility, settings navigation and displays.
    pub(super) fn window_msg(&mut self, msg: Msg) {
        match msg {
            Msg::OverlayAction(kind, action) => {
                // Native menus are snapshots. Recheck before applying an action
                // if the window, connection or settings changed while tracking.
                if crate::services::overlay_menu::Snapshot::new(kind, self.ui)
                    .is_some_and(|menu| menu.enabled(&action))
                {
                    self.update(action.message(kind));
                }
            }
            Msg::ToggleWindow(kind) => match kind {
                WindowKind::Input
                    if self.settings.general.input_presence == InputPresence::Summon =>
                {
                    self.summoned = !self.summoned;
                }
                WindowKind::Settings => self.open_settings(),
                kind => self.edit(|s| {
                    if let Some(o) = kind.overlay_mut(s) {
                        o.visible = !o.visible;
                    }
                }),
            },
            Msg::ShowWindow(kind) => match kind {
                WindowKind::Input => {
                    self.focus_editor = true;
                    self.set_input_visibility(true);
                    self.commands
                        .push(WindowCommand::Focus(WindowKind::Input.id()));
                }
                WindowKind::Settings => self.open_settings(),
                kind => self.edit(|s| {
                    if let Some(o) = kind.overlay_mut(s) {
                        o.visible = true;
                    }
                }),
            },
            Msg::HideInput => self.set_input_visibility(false),
            Msg::ResetWindow(kind) => {
                self.windows.forget(kind);
                self.edit(|s| kind.clear_saved_size(s));
            }
            Msg::OpenSettings => self.open_settings(),
            Msg::SettingsPlatform(platform) => {
                self.ui.platform_tab.set_if_changed(platform);
                self.update(Msg::SettingsPage(crate::ui::settings::Page::Platforms));
            }
            Msg::SettingsPage(page) => {
                self.ui.page.set(page);
                self.open_settings();
            }
            Msg::SecondInstance => {
                self.update(Msg::ShowWindow(WindowKind::Input));
                self.open_settings();
            }
            Msg::Displays(monitors) => {
                self.windows.monitors = monitors;
            }
            _ => unreachable!("not a window message"),
        }
    }
}
