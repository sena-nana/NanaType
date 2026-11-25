//! OS shell state: paths, persistence, windows, quick bar, tray, hotkeys,
//! outputs, dialogs, toasts, and section-scoped settings diffing.

use std::collections::HashMap;

use nana_ui_platform::host::WindowCommand;
use nanatype_frame::locale_of;
use nanatype_settings::Settings;

use crate::msg::FileTarget;
use crate::output::OutputManager;
use crate::paths::Paths;
use crate::services::hotkeys::Hotkeys;
use crate::services::persist::Persist;
use crate::services::tray::Tray;
use crate::ui::feedback::ToastRequest;
pub use crate::windows::{QuickBarState, WindowManager};

pub struct ShellState {
    pub paths: Paths,
    pub readonly_settings: bool,
    pub persist: Persist,
    pub hotkeys: Hotkeys,
    pub tray: Option<Tray>,
    pub windows: WindowManager,
    pub quick: QuickBarState,
    pub summoned: bool,
    pub output: OutputManager,
    pub(super) backup: super::backup::Backup,
    pub next_notice: u64,
    pub dialogs: HashMap<u64, FileTarget>,
    pub next_dialog: u64,
    pub commands: Vec<WindowCommand>,
    pub toasts: Vec<ToastRequest>,
    pub tray_error: Option<(u64, String)>,
    pub quitting: bool,
    pub _critical: nanatype_scheduling::Critical,
}

/// Bitflag mask identifying which settings domains changed in a commit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingsSections(u32);

impl SettingsSections {
    pub const LOCALE: Self = Self(1 << 0);
    pub const SKIN: Self = Self(1 << 1);
    pub const SKIN_LOOK: Self = Self(1 << 2);
    pub const SKIN_PROFILE: Self = Self(1 << 3);
    pub const SKIN_WATCH: Self = Self(1 << 4);
    pub const EFFECTS_RETRY: Self = Self(1 << 5);
    pub const CHROME: Self = Self(1 << 6);
    pub const ZOOM: Self = Self(1 << 7);
    pub const ASSETS: Self = Self(1 << 8);
    pub const SOUND_DEVICE: Self = Self(1 << 9);
    pub const SOUND_PACK: Self = Self(1 << 10);
    pub const SOUND_LEVELS: Self = Self(1 << 11);
    pub const AUTOSTART: Self = Self(1 << 12);
    pub const PERFORMANCE: Self = Self(1 << 13);
    pub const HOTKEYS: Self = Self(1 << 14);
    pub const INTEGRATIONS: Self = Self(1 << 15);
    pub const OBS: Self = Self(1 << 16);
    pub const OUTPUT: Self = Self(1 << 17);
    pub const PLUGINS: Self = Self(1 << 18);
    pub const SPEAKER: Self = Self(1 << 19);
    pub const CHARACTERS: Self = Self(1 << 20);
    pub const SPEECH: Self = Self(1 << 21);
    pub const TTS_OFF: Self = Self(1 << 22);
    pub const CORE_CONFIG: Self = Self(1 << 23);
    pub const INPUT_PREVIEW: Self = Self(1 << 24);
    pub const REDUCED_MOTION: Self = Self(1 << 25);

    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn any(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn diff(prev: &Settings, next: &Settings) -> Self {
        let mut s = Self::default();
        let body_profile = |cfg: &Settings| {
            cfg.skin
                .personalizations
                .get(&next.skin.id)
                .cloned()
                .map(|mut p| {
                    p.display.size = None;
                    p.history.size = None;
                    p
                })
        };
        if locale_of(prev) != locale_of(next) {
            s.insert(Self::LOCALE);
        }
        if prev.skin.id != next.skin.id {
            s.insert(Self::SKIN);
        }
        if body_profile(prev) != body_profile(next) {
            s.insert(Self::SKIN_PROFILE);
            s.insert(Self::SKIN_LOOK);
        }
        if prev.skin.overrides != next.skin.overrides {
            s.insert(Self::SKIN_LOOK);
        }
        if prev.skin.hot_reload != next.skin.hot_reload {
            s.insert(Self::SKIN_WATCH);
        }
        if !prev.skin.advanced_effects && next.skin.advanced_effects {
            s.insert(Self::EFFECTS_RETRY);
        }
        if prev.accessibility != next.accessibility {
            s.insert(Self::CHROME);
        }
        if prev.accessibility.ui_scale != next.accessibility.ui_scale
            || prev.accessibility.ui_scale_follows_system
                != next.accessibility.ui_scale_follows_system
        {
            s.insert(Self::ZOOM);
        }
        if prev.assets.sticker_dirs != next.assets.sticker_dirs {
            s.insert(Self::ASSETS);
        }
        if prev.sound.device != next.sound.device {
            s.insert(Self::SOUND_DEVICE);
        }
        if prev.sound.pack != next.sound.pack {
            s.insert(Self::SOUND_PACK);
        }
        if prev.sound.master != next.sound.master
            || prev.sound.typing_volume != next.sound.typing_volume
            || prev.sound.effects_volume != next.sound.effects_volume
            || prev.sound.ui_volume != next.sound.ui_volume
            || prev.sound.keystroke_volume != next.sound.keystroke_volume
        {
            s.insert(Self::SOUND_LEVELS);
        }
        if prev.general.launch_at_login != next.general.launch_at_login {
            s.insert(Self::AUTOSTART);
        }
        if prev.performance != next.performance {
            s.insert(Self::PERFORMANCE);
        }
        if prev.general.input_skin_preview != next.general.input_skin_preview {
            s.insert(Self::INPUT_PREVIEW);
        }
        if prev.playback.reduced_motion != next.playback.reduced_motion {
            s.insert(Self::REDUCED_MOTION);
        }
        if prev.hotkeys != next.hotkeys
            || prev.general.characters != next.general.characters
            || prev.assets.phrases != next.assets.phrases
        {
            s.insert(Self::HOTKEYS);
        }
        if prev.integrations != next.integrations {
            s.insert(Self::INTEGRATIONS);
        }
        if prev.output.obs != next.output.obs
            || prev.output.display.obs_source != next.output.display.obs_source
            || prev.output.history.obs_source != next.output.history.obs_source
            || prev.output.danmaku.obs_source != next.output.danmaku.obs_source
        {
            s.insert(Self::OBS);
        }
        if prev.output != next.output || prev.history.chat.separate != next.history.chat.separate {
            s.insert(Self::OUTPUT);
        }
        if prev.enabled_plugins != next.enabled_plugins
            || prev.plugin_approvals != next.plugin_approvals
            || prev.disabled_packages != next.disabled_packages
        {
            s.insert(Self::PLUGINS);
        }
        if prev.general.active_character() != next.general.active_character() {
            s.insert(Self::SPEAKER);
        }
        if prev.general.characters != next.general.characters
            || prev.general.character != next.general.character
        {
            s.insert(Self::CHARACTERS);
        }
        if prev.tts.enabled && !next.tts.enabled {
            s.insert(Self::TTS_OFF);
        }
        if prev.speech != next.speech {
            s.insert(Self::SPEECH);
        }
        if prev.general != next.general
            || prev.playback != next.playback
            || prev.history != next.history
            || prev.tts != next.tts
            || prev.integrations.twitch != next.integrations.twitch
            || prev.skin.overrides != next.skin.overrides
        {
            s.insert(Self::CORE_CONFIG);
        }
        s
    }
}

impl std::ops::BitOr for SettingsSections {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}
