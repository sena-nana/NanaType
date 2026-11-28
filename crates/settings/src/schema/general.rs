use nanatype_model::Rgba8;
use serde::{Deserialize, Serialize};

use super::{Hotkey, clamp_f32, clamp_ord, set_if, trim_take};

/// Most characters a profile keeps.
pub const MAX_CHARACTERS: usize = 32;
/// How many steps the first-run guide has.
pub const ONBOARDING_STEPS: u8 = 5;
/// The smallest and largest UI scale, in percent, and its step.
pub const UI_SCALE: (u32, u32, u32) = (80, 200, 10);
/// Heights [`AssetSettings::sticker_scale`] keeps, in line heights.
pub const STICKER_SCALE: (f32, f32) = (0.8, 2.0);

/// How the first-run guide was told the display reaches the stream: which
/// streaming app, and how it takes the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamRoute {
    /// OBS, over Spout2 (Syphon on macOS).
    ObsShared,
    /// OBS, capturing the display window.
    ObsWindow,
    /// Another streaming app, capturing the display window.
    OtherWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputPresence {
    /// The input window stays open.
    #[default]
    Resident,
    /// Hidden until the summon hotkey; sending or Esc hides it again.
    Summon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    /// Compose, then send to play with the typewriter.
    #[default]
    Compose,
    /// The display mirrors the draft as it is typed.
    Mirror,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromeMode {
    /// Application chrome follows the OS, independently of the skin.
    #[default]
    #[serde(alias = "skin")]
    System,
    Light,
    Dark,
}

/// High contrast for the application's own windows (never the skins).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContrastMode {
    /// On while the system asks for high contrast (where it reports it).
    #[default]
    System,
    Off,
    On,
}

/// How the application's own windows read: settings, the input window and
/// the editor. Nothing here reaches the display, partner or history output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessibilitySettings {
    pub chrome: ChromeMode,
    pub contrast: ContrastMode,
    /// Everything the windows draw, in percent of its size, on top of the
    /// system's display scale ([`UI_SCALE`]).
    pub ui_scale: u32,
    /// Take the system's text size instead of [`Self::ui_scale`], where the
    /// system has one (Windows).
    pub ui_scale_follows_system: bool,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            chrome: ChromeMode::System,
            contrast: ContrastMode::System,
            ui_scale: 100,
            ui_scale_follows_system: false,
        }
    }
}

impl AccessibilitySettings {
    /// [`Self::ui_scale`] as a factor.
    pub fn scale(&self) -> f32 {
        self.ui_scale as f32 / 100.0
    }

    /// The zoom of the app's own windows, given the system's text size
    /// (`None` where it has none).
    pub fn zoom(&self, system_text: Option<f32>) -> f32 {
        match system_text {
            Some(text) if self.ui_scale_follows_system => text,
            _ => self.scale(),
        }
    }

    /// Whether the chrome paints in high contrast, given the system's flag.
    pub fn high_contrast(&self, system: bool) -> bool {
        match self.contrast {
            ContrastMode::System => system,
            ContrastMode::Off => false,
            ContrastMode::On => true,
        }
    }

    pub(crate) fn sanitize(&mut self) -> bool {
        let (lo, hi, step) = UI_SCALE;
        let next = (self.ui_scale.clamp(lo, hi) + step / 2) / step * step;
        let mut dirty = false;
        set_if(&mut self.ui_scale, next, &mut dirty);
        dirty
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// An extra file proxy, tried first; the built-in ones are always tried.
    pub proxy: String,
    /// A release the user chose not to be reminded of.
    pub skipped_version: Option<String>,
    /// The last automatic check, in Unix seconds.
    pub last_check: Option<u64>,
}

impl UpdateSettings {
    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        trim_take(&mut self.proxy, 512, &mut dirty);
        dirty
    }
}

/// Logging (docs/logging.md). Read once at launch, before the window opens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiagnosticsSettings {
    /// Keep every recorded event in the session log, not only warnings.
    pub verbose: bool,
    /// The newest crash snapshot already pointed out, in Unix seconds.
    pub crash_seen: u64,
}

/// GPU choice and scheduling preferences while a game or encoder competes
/// (Windows only).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceSettings {
    /// Raise the process's GPU scheduling priority.
    pub gpu_high: bool,
    /// Raise the CPU priority of the threads a frame waits on.
    pub cpu_high: bool,
    /// Keep those threads on the performance cores.
    pub cpu_performance_cores: bool,
    /// The GPU to render on, by the name the renderer reports; `None` lets
    /// the renderer choose. Read once, at startup.
    pub gpu_adapter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    /// `None` follows the system language.
    pub locale: Option<String>,
    /// Who the lines are said by. Never empty: the last one stays.
    pub characters: Vec<Character>,
    /// Index of the character new lines are said by.
    pub character: usize,
    pub input_presence: InputPresence,
    pub input_mode: InputMode,
    /// Show the draft inside the skin's stream canvas instead of a full editor.
    pub input_skin_preview: bool,
    /// Opacity of the summon hotkey's one-line quick input bar.
    pub quick_opacity: f32,
    pub launch_at_login: bool,
    pub onboarding_done: bool,
    /// The first-run guide's step (0-based) when it was last left, so a
    /// guide closed halfway opens there again.
    pub onboarding_step: u8,
    /// What the guide's last step was told about the stream; `None` until
    /// it is answered.
    pub onboarding_route: Option<StreamRoute>,
    pub check_updates: bool,
    /// The profile last switched to (profiles/<id>.json); its sections are
    /// saved back into it before another is switched to.
    pub profile: Option<String>,
}

impl Default for General {
    fn default() -> Self {
        Self {
            locale: None,
            characters: vec![Character::default()],
            character: 0,
            input_presence: InputPresence::Resident,
            input_mode: InputMode::Compose,
            input_skin_preview: false,
            quick_opacity: 1.0,
            launch_at_login: false,
            onboarding_done: false,
            onboarding_step: 0,
            onboarding_route: None,
            check_updates: true,
            profile: None,
        }
    }
}

impl General {
    /// The character new lines are said by.
    pub fn active_character(&self) -> Character {
        self.characters
            .get(self.character)
            .or(self.characters.first())
            .cloned()
            .unwrap_or_default()
    }

    /// The character a line's `speaker` names: by name (exactly, then
    /// ignoring case), else by its number in the list from 1.
    pub fn character_named(&self, key: &str) -> Option<&Character> {
        let key = key.trim();
        self.characters
            .iter()
            .find(|c| c.name.trim() == key)
            .or_else(|| {
                let lower = key.to_lowercase();
                self.characters
                    .iter()
                    .find(|c| c.name.trim().to_lowercase() == lower)
            })
            .or_else(|| {
                let n: usize = key.strip_prefix('#').unwrap_or(key).parse().ok()?;
                self.characters.get(n.checked_sub(1)?)
            })
    }

    /// Make character `i` the active one; out of range does nothing.
    pub fn select_character(&mut self, i: usize) {
        if i < self.characters.len() {
            self.character = i;
        }
    }

    /// Step the active character by `step`, wrapping around the list.
    pub fn cycle_character(&mut self, step: i32) {
        let n = self.characters.len() as i64;
        if n > 0 {
            self.character = (self.character as i64 + i64::from(step)).rem_euclid(n) as usize;
        }
    }

    /// Remove character `i`, keeping the last one and the active choice.
    pub fn remove_character(&mut self, i: usize) {
        if self.characters.len() <= 1 || i >= self.characters.len() {
            return;
        }
        self.characters.remove(i);
        if self.character > i || self.character >= self.characters.len() {
            self.character = self.character.saturating_sub(1);
        }
    }

    /// Move character `i` by `step` places; the active one moves with it.
    pub fn move_character(&mut self, i: usize, step: i32) {
        let n = self.characters.len();
        let j = i as i64 + i64::from(step);
        if i >= n || j < 0 || j as usize >= n {
            return;
        }
        let j = j as usize;
        self.characters.swap(i, j);
        if self.character == i {
            self.character = j;
        } else if self.character == j {
            self.character = i;
        }
    }

    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        clamp_f32(&mut self.quick_opacity, 1.0, 0.0, 1.0, &mut dirty);
        if self.onboarding_step >= ONBOARDING_STEPS {
            self.onboarding_step = 0;
            dirty = true;
        }
        if self.characters.len() > MAX_CHARACTERS {
            self.characters.truncate(MAX_CHARACTERS);
            dirty = true;
        }
        if self.characters.is_empty() {
            self.characters.push(Character::default());
            dirty = true;
        }
        for character in &mut self.characters {
            let trimmed: String = character.name.chars().take(64).collect();
            if character.name != trimmed {
                character.name = trimmed;
                dirty = true;
            }
            super::sanitize_voice_id(&mut character.voice, &mut dirty);
        }
        if self.character >= self.characters.len() {
            self.character = 0;
            dirty = true;
        }
        dirty
    }
}

/// A persona lines can be said by: the name on the plate and in the
/// history, with its own picture and name colour.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Character {
    pub name: String,
    /// Avatar image for history rows (absolute path).
    pub avatar: Option<String>,
    /// Colour of the name on the name plate; `None` keeps the skin's.
    pub color: Option<Rgba8>,
    /// System-wide key that switches to this character.
    pub hotkey: Option<Hotkey>,
    /// The system voice its lines are read aloud with; `None` uses
    /// [`crate::TtsSettings::voice`].
    pub voice: Option<String>,
}

impl Character {
    /// Who a line said now by this character is from.
    pub fn speaker(&self) -> nanatype_model::Speaker {
        nanatype_model::Speaker {
            name: self.name.clone(),
            avatar: self.avatar.clone(),
            portrait: None,
            color: self.color,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickPhrase {
    pub label: String,
    /// Markup inserted into the editor (or sent, when `send` is set).
    pub markup: String,
    #[serde(default)]
    pub send: bool,
    #[serde(default)]
    pub hotkey: Option<Hotkey>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetSettings {
    /// Folders whose images become sticker libraries (folder name = library).
    pub sticker_dirs: Vec<String>,
    /// How tall stickers are drawn, as a multiple of the text's line height
    /// (within [`STICKER_SCALE`]).
    pub sticker_scale: f32,
    pub phrases: Vec<QuickPhrase>,
    pub telegram: TelegramSettings,
}

impl Default for AssetSettings {
    fn default() -> Self {
        Self {
            sticker_dirs: Vec::new(),
            sticker_scale: 1.0,
            phrases: Vec::new(),
            telegram: TelegramSettings::default(),
        }
    }
}

impl AssetSettings {
    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        clamp_f32(
            &mut self.sticker_scale,
            1.0,
            STICKER_SCALE.0,
            STICKER_SCALE.1,
            &mut dirty,
        );
        clamp_ord(&mut self.telegram.api_id, 0, i32::MAX, &mut dirty);
        trim_take(&mut self.telegram.proxy, 2048, &mut dirty);
        dirty
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramSettings {
    pub api_id: i32,
    /// SOCKS5 URL. Login codes, API hashes and account passwords are never
    /// settings; a proxy's own `user:password@` part is, so the hint tells
    /// users to prefer an unauthenticated local proxy.
    pub proxy: String,
}
