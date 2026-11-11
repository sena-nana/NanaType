//! Settings schema v6. Every struct is `#[serde(default)]`, so a file written
//! by an older build loads with new fields at their defaults, and fields this
//! build does not know are kept in `unknown` and written back unchanged.

use std::collections::BTreeMap;

use nanatype_model::{
    AfterLine, EntranceKind, Overflow, ParamValue, Rgba8, Speaker, VtsReset, valid_param_id,
};
use nanatype_playback::QueuePolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SETTINGS_SCHEMA: u32 = 7;
/// Most characters a profile keeps.
pub const MAX_CHARACTERS: usize = 32;
/// How many steps the first-run guide has.
pub const ONBOARDING_STEPS: u8 = 4;
pub const DEFAULT_SKIN: &str = "nanatype.halo";
/// Skin loop rates the settings offer; the first is the default.
pub const SKIN_FRAME_RATES: [u32; 2] = [30, 60];
/// Raised whenever scripts gain data or host interfaces: 1 granted events and
/// views, 2 adds declared data reads. Older approvals must be reviewed again.
pub const PLUGIN_CONSENT_POLICY: u32 = 2;
pub const MAX_ENABLED_PLUGINS: usize = 8;
/// Packages that can be switched off at once.
pub const MAX_DISABLED_PACKAGES: usize = 512;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginApproval {
    pub digest: String,
    pub trusted_source: bool,
    pub policy: u32,
}

impl PluginApproval {
    pub fn is_current(&self) -> bool {
        self.policy == PLUGIN_CONSENT_POLICY && self.trusted_source
    }
    pub fn matches(&self, digest: &str) -> bool {
        self.is_current() && self.digest == digest
    }
}

/// A key chord. `key` is a W3C `KeyboardEvent.code` name (`F8`, `KeyN`, `Digit1`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hotkey {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    /// Windows key / Command.
    #[serde(default, alias = "win")]
    pub meta: bool,
    pub key: String,
}

impl Hotkey {
    pub fn bare(key: impl Into<String>) -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            key: key.into(),
        }
    }

    pub fn ctrl(key: impl Into<String>) -> Self {
        Self {
            ctrl: true,
            ..Self::bare(key)
        }
    }

    pub fn shift(key: impl Into<String>) -> Self {
        Self {
            shift: true,
            ..Self::bare(key)
        }
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.shift || self.alt || self.meta
    }

    /// `Ctrl+Shift+F8`. `meta_label` is "Win" or "⌘".
    pub fn label(&self, meta_label: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.meta {
            parts.push(meta_label);
        }
        let key = match self.key.as_str() {
            "ArrowUp" => "↑",
            "ArrowDown" => "↓",
            "ArrowLeft" => "←",
            "ArrowRight" => "→",
            "Escape" => "Esc",
            k => k
                .strip_prefix("Key")
                .or_else(|| k.strip_prefix("Digit"))
                .unwrap_or(k),
        };
        let mut out = parts.join("+");
        if !out.is_empty() {
            out.push('+');
        }
        out.push_str(key);
        out
    }

    /// Function keys and a few lock keys are fine alone: they type nothing.
    fn types_nothing(&self) -> bool {
        let k = self.key.as_str();
        (k.len() >= 2
            && k.starts_with('F')
            && k[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)))
            || matches!(k, "Pause" | "ScrollLock" | "Insert")
    }

    /// Enter, Ctrl+Enter and ⌘+Enter: the send key's choices, which no other
    /// action may take.
    pub fn is_send_chord(&self) -> bool {
        matches!(self.key.as_str(), "Enter" | "NumpadEnter")
            && !self.shift
            && !self.alt
            && !(self.ctrl && self.meta)
    }

    /// Chords every editor or the system already means something by:
    /// clipboard and undo, select all, closing and switching windows, and
    /// the input method switch.
    fn is_system_chord(&self) -> bool {
        let command = self.ctrl != self.meta;
        let k = self.key.as_str();
        (command && !self.alt && matches!(k, "KeyA" | "KeyC" | "KeyV" | "KeyX" | "KeyZ" | "KeyY"))
            || (self.alt && !self.ctrl && !self.meta && matches!(k, "F4" | "Tab" | "Space"))
            || (self.meta
                && !self.ctrl
                && !self.alt
                && matches!(k, "Tab" | "KeyQ" | "KeyW" | "KeyH" | "KeyM"))
            || (command && !self.alt && !self.shift && k == "Space")
    }

    /// Keys that move the caret, select or delete. Some modifier does each
    /// of these in some editor, so inside the input window they stay the
    /// editor's; system-wide only a chord with two of Ctrl, Alt and ⌘ is
    /// clear of them.
    fn edits_text(&self) -> bool {
        matches!(
            self.key.as_str(),
            "ArrowLeft" | "ArrowRight" | "Home" | "End" | "Backspace" | "Delete"
        )
    }

    /// Why this chord may not be a system-wide hotkey. A key that types
    /// needs Ctrl, Alt or ⌘ (Shift alone still types). `send` is never true
    /// for a hotkey today; it keeps the Enter rule in one place.
    pub fn refusal_global(&self, send: bool) -> Option<Refusal> {
        if self.key.is_empty() {
            return Some(Refusal::Types);
        }
        if self.is_send_chord() && !send {
            return Some(Refusal::SendOnly);
        }
        if !(self.ctrl || self.alt || self.meta || self.types_nothing()) {
            return Some(Refusal::Types);
        }
        let strong = [self.ctrl, self.alt, self.meta]
            .into_iter()
            .filter(|m| *m)
            .count();
        if self.is_system_chord() || (self.edits_text() && strong < 2) || self.key == "Tab" {
            return Some(Refusal::Reserved);
        }
        None
    }

    /// Why this chord may not be one of the input window's keys. `send` is
    /// whether it is for the send key, the only one that may take Enter or
    /// Ctrl+Enter.
    pub fn refusal_local(&self, send: bool) -> Option<Refusal> {
        if self.key.is_empty() {
            return Some(Refusal::Types);
        }
        if self.is_send_chord() && !send {
            return Some(Refusal::SendOnly);
        }
        if !self.is_safe_local() {
            return Some(Refusal::Types);
        }
        // Tab and Shift+Tab move focus: the keyboard's way around the window.
        let tab = self.key == "Tab" && !self.ctrl && !self.alt && !self.meta;
        if self.is_system_chord() || self.edits_text() || tab {
            return Some(Refusal::Reserved);
        }
        None
    }

    /// Function keys and media keys are fine alone; anything that types needs
    /// a modifier other than Shift, or it would swallow normal typing
    /// system-wide.
    pub fn is_safe_global(&self) -> bool {
        self.refusal_global(false).is_none()
    }

    /// Inside the input window a bare key is fine as long as it does not type
    /// text: Enter, Esc, arrows, paging and the function keys.
    pub fn is_safe_local(&self) -> bool {
        if self.key.is_empty() {
            return false;
        }
        self.has_modifier()
            || self.types_nothing()
            || matches!(
                self.key.as_str(),
                "Enter"
                    | "NumpadEnter"
                    | "Escape"
                    | "Tab"
                    | "ArrowUp"
                    | "ArrowDown"
                    | "ArrowLeft"
                    | "ArrowRight"
                    | "Home"
                    | "End"
                    | "PageUp"
                    | "PageDown"
                    | "Delete"
            )
    }
}

/// Why a chord cannot be bound to an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// It types text (or is no key at all).
    Types,
    /// The system or the editor already uses it.
    Reserved,
    /// Enter and Ctrl+Enter belong to the send key.
    SendOnly,
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
pub enum SpeechBackend {
    #[default]
    Auto,
    System,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechMode {
    #[default]
    Manual,
    Auto,
}

/// Preferences only: microphone activation is deliberately never persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeechSettings {
    pub backend: SpeechBackend,
    pub mode: SpeechMode,
    /// None follows the default input device.
    pub device: Option<String>,
    /// None follows the interface language; "auto" uses local language detection.
    pub language: Option<String>,
    pub silence_seconds: u32,
}

impl Default for SpeechSettings {
    fn default() -> Self {
        Self {
            backend: SpeechBackend::Auto,
            mode: SpeechMode::Manual,
            device: None,
            language: None,
            silence_seconds: 3,
        }
    }
}

/// Rates [`TtsSettings::rate`] keeps, as a multiple of the voice's own pace.
pub const TTS_RATE: (f32, f32) = (0.5, 2.0);

/// Sent lines read aloud by the system's voices. A character's own voice
/// is [`Character::voice`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsSettings {
    pub enabled: bool,
    /// For characters without a voice, chat and partners; `None` is the
    /// system default.
    pub voice: Option<String>,
    /// A multiple of the voice's own pace.
    pub rate: f32,
    pub volume: f32,
    /// Also read viewers' messages that reach the history.
    pub chat: bool,
    /// Also read co-op partners' lines.
    pub partners: bool,
    /// The typewriter types each word as the voice says it (by the voice's
    /// word boundaries), and on its own when the voice gives none.
    pub pace_typing: bool,
}

impl Default for TtsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            voice: None,
            rate: 1.0,
            volume: 1.0,
            chat: false,
            partners: false,
            pace_typing: false,
        }
    }
}

/// Where translations come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslateProvider {
    /// An OpenAI-compatible chat completions endpoint (OpenAI, DeepSeek, a
    /// local Ollama or LM Studio).
    #[default]
    Openai,
    Deepl,
}

impl TranslateProvider {
    pub fn key(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Deepl => "deepl",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "openai" => Some(Self::Openai),
            "deepl" => Some(Self::Deepl),
            _ => None,
        }
    }
}

/// Translated subtitles for sent lines. The API keys live in the system's
/// secret store, never here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslateSettings {
    pub enabled: bool,
    pub provider: TranslateProvider,
    /// The OpenAI-compatible endpoint's base URL, up to `/v1`.
    pub base_url: String,
    pub model: String,
    /// A BCP 47 code (`en`, `ja`, `zh-Hant`); the source is detected.
    pub target: String,
    /// Which lines are translated: the streamer's, co-op partners' (when
    /// they send none of their own) and viewer chat in the history.
    pub own: bool,
    pub partners: bool,
    pub chat: bool,
    /// Where translations show: under the line on the display, and under
    /// rows in the history window. Saved history keeps them either way.
    pub display: bool,
    pub history: bool,
    /// A proxy for the provider; empty uses the system's.
    pub proxy: String,
}

impl Default for TranslateSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: TranslateProvider::default(),
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            target: "en".into(),
            own: true,
            partners: false,
            chat: false,
            display: true,
            history: true,
            proxy: String::new(),
        }
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

/// The recordings played for keys pressed in the input window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeystrokeScheme {
    #[default]
    Mechanical,
    Bubble,
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
    /// [`TtsSettings::voice`].
    pub voice: Option<String>,
}

impl Character {
    /// Who a line said now by this character is from.
    pub fn speaker(&self) -> Speaker {
        Speaker {
            name: self.name.clone(),
            avatar: self.avatar.clone(),
            portrait: None,
            color: self.color,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackSettings {
    /// Multiplies every reveal delay; `0.5` is twice as fast.
    pub speed_scale: f32,
    pub punctuation_pauses: bool,
    /// Off shows each line at once.
    pub typing: bool,
    /// `None` uses the skin's entrance.
    pub entrance: Option<EntranceKind>,
    pub after: AfterLine,
    pub auto_advance_ms: Option<u32>,
    pub queue: QueuePolicy,
    /// Disables per-character motion (accessibility).
    pub reduced_motion: bool,
    /// Live mirror: delay between characters of a paste or IME commit.
    pub mirror_stagger_ms: u32,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            speed_scale: 1.0,
            punctuation_pauses: true,
            typing: true,
            entrance: None,
            after: AfterLine::Hold,
            auto_advance_ms: None,
            queue: QueuePolicy::Replace,
            reduced_motion: false,
            mirror_stagger_ms: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSettings {
    pub master: f32,
    pub typing: bool,
    pub typing_volume: f32,
    pub effects_volume: f32,
    pub ui_volume: f32,
    /// Click on every key pressed in the input window.
    pub keystroke: bool,
    pub keystroke_scheme: KeystrokeScheme,
    pub keystroke_volume: f32,
    /// Output device name; `None` is the system default.
    pub device: Option<String>,
    /// Installed sound pack layered over the skin's sounds; `None` plays the
    /// skin's own. Kept when the pack is missing, so a pack this build cannot
    /// see (a Workshop item, say) comes back with the build that can.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<String>,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            master: 0.8,
            typing: true,
            typing_volume: 0.7,
            effects_volume: 0.9,
            ui_volume: 0.5,
            keystroke: false,
            keystroke_scheme: KeystrokeScheme::Mechanical,
            keystroke_volume: 0.6,
            device: None,
            pack: None,
        }
    }
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

/// The smallest and largest UI scale, in percent, and its step.
pub const UI_SCALE: (u32, u32, u32) = (80, 200, 10);

/// How the application's own windows read: settings, the input window and
/// the editor. Nothing here reaches the display, partner or history output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessibilitySettings {
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
}

/// User tweaks applied on top of the active skin.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfacePersonalization {
    pub font_family: Option<String>,
    pub text_size: Option<f32>,
    pub line_height: Option<f32>,
    pub text_color: Option<Rgba8>,
    /// Native client size, including the skin's overscan.
    pub size: Option<[f32; 2]>,
}

impl SurfacePersonalization {
    pub fn valid(&self) -> bool {
        self.text_size
            .is_none_or(|v| v.is_finite() && (8.0..=200.0).contains(&v))
            && self
                .line_height
                .is_none_or(|v| v.is_finite() && (1.0..=3.0).contains(&v))
            && self.size.is_none_or(|[w, h]| {
                w.is_finite()
                    && h.is_finite()
                    && (160.0..=8192.0).contains(&w)
                    && (96.0..=8192.0).contains(&h)
            })
            && self
                .font_family
                .as_ref()
                .is_none_or(|v| !v.trim().is_empty() && v.len() <= 256)
    }

    fn sanitize(&mut self) {
        if let Some(v) = &mut self.text_size {
            *v = finite_or(*v, 28.0).clamp(8.0, 200.0);
        }
        if let Some(v) = &mut self.line_height {
            *v = finite_or(*v, 1.4).clamp(1.0, 3.0);
        }
        if let Some([w, h]) = &mut self.size {
            *w = finite_or(*w, 760.0).clamp(160.0, 8192.0);
            *h = finite_or(*h, 200.0).clamp(96.0, 8192.0);
        }
        self.font_family = self.font_family.take().and_then(|v| {
            let mut v: String = v.trim().chars().take(256).collect();
            while v.len() > 256 {
                v.pop();
            }
            (!v.is_empty()).then_some(v)
        });
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinPersonalization {
    pub display: SurfacePersonalization,
    pub history: SurfacePersonalization,
    /// Values for the skin's declared params that differ from its defaults.
    /// The skin's schema checks them when applied; a value it refuses falls
    /// back to the default, so this map never needs a migration.
    #[serde(
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "lenient_params"
    )]
    pub params: BTreeMap<String, ParamValue>,
}

/// Most params a skin can declare, and so most values one profile keeps.
pub const MAX_SKIN_PARAMS: usize = 64;
const MAX_PARAM_TEXT_BYTES: usize = 256;

fn param_entry_valid(id: &str, value: &ParamValue) -> bool {
    valid_param_id(id)
        && match value {
            ParamValue::Number(v) => v.is_finite(),
            ParamValue::Bool(_) => true,
            ParamValue::Text(t) => t.len() <= MAX_PARAM_TEXT_BYTES,
        }
}

/// Keep the well-formed entries and drop the rest instead of failing: a bad
/// param value must not make the whole settings file unreadable.
fn lenient_params<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, ParamValue>, D::Error> {
    // Any JSON value parses; only then is its shape inspected, so a stray
    // array or string here never fails the settings file.
    let Value::Object(raw) = Value::deserialize(deserializer)? else {
        return Ok(BTreeMap::new());
    };
    Ok(raw
        .into_iter()
        .filter_map(|(id, v)| {
            let value = match v {
                Value::Bool(b) => ParamValue::Bool(b),
                Value::Number(n) => ParamValue::Number(n.as_f64()?),
                Value::String(s) => ParamValue::Text(s),
                _ => return None,
            };
            param_entry_valid(&id, &value).then_some((id, value))
        })
        .take(MAX_SKIN_PARAMS)
        .collect())
}

impl SkinPersonalization {
    pub fn valid(&self) -> bool {
        self.display.valid()
            && self.history.valid()
            && self.params.len() <= MAX_SKIN_PARAMS
            && self.params.iter().all(|(id, v)| param_entry_valid(id, v))
    }

    fn sanitize(&mut self) {
        self.display.sanitize();
        self.history.sanitize();
        self.params.retain(|id, v| param_entry_valid(id, v));
        while self.params.len() > MAX_SKIN_PARAMS {
            self.params.pop_last();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinOverrides {
    pub text_color: Option<Rgba8>,
    pub name_color: Option<Rgba8>,
    pub box_color: Option<Rgba8>,
    pub accent: Option<Rgba8>,
    pub text_size: Option<f32>,
    pub region: Option<[f32; 2]>,
    pub overflow: Option<Overflow>,
    /// In page mode, turn a full page on its own after this many ms.
    pub overflow_auto_ms: Option<u32>,
    /// `Some(false)` hides the dialogue name plate; `None` leaves it to the skin.
    pub name_visible: Option<bool>,
    /// The name plate's size as a multiple of the skin's ([`SKIN_PART_SCALE`]).
    pub name_scale: Option<f32>,
    /// The dialogue box's ornaments' size as a multiple of the skin's
    /// ([`SKIN_PART_SCALE`]).
    pub decor_scale: Option<f32>,
}

/// The smallest and largest [`SkinOverrides::name_scale`] and
/// [`SkinOverrides::decor_scale`], and their slider's step.
pub const SKIN_PART_SCALE: (f64, f64, f64) = (0.25, 4.0, 0.05);

impl SkinOverrides {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinSettings {
    pub advanced_effects: bool,
    /// Skin backgrounds move. Off, each is drawn once at rest and that frame
    /// is reused.
    pub motion: bool,
    /// Frames per second for the display window's skin animation: 30 or 60.
    /// Typing and other glyph motion are not held to it.
    pub frame_rate: u32,
    pub id: String,
    pub overrides: SkinOverrides,
    /// An entry replaces legacy body overrides; None fields mean skin defaults.
    pub personalizations: BTreeMap<String, SkinPersonalization>,
    /// Independent application appearance; path retained for saved-settings compatibility.
    pub chrome: ChromeMode,
    /// Reload folder skins when their files change (for skin authors).
    pub hot_reload: bool,
}

impl Default for SkinSettings {
    fn default() -> Self {
        Self {
            advanced_effects: true,
            motion: true,
            frame_rate: SKIN_FRAME_RATES[0],
            id: DEFAULT_SKIN.into(),
            overrides: SkinOverrides::default(),
            personalizations: BTreeMap::new(),
            chrome: ChromeMode::System,
            hot_reload: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lock {
    /// Clicks pass through the whole window.
    Locked,
    /// Drag anywhere to move; use the wheel to scale the window.
    #[default]
    #[serde(alias = "edit_layout")]
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaptureBackground {
    #[default]
    Transparent,
    /// A solid key colour for capture tools without alpha.
    Chroma { color: Rgba8 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub visible: bool,
    pub lock: Lock,
    pub always_on_top: bool,
    pub background: CaptureBackground,
    /// Logical size override; `None` uses the skin's size.
    pub size: Option<[f32; 2]>,
    /// Write the dialogue top to bottom. The skin's portrait layout still
    /// follows the window's shape.
    pub vertical_text: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            visible: true,
            lock: Lock::Interactive,
            always_on_top: true,
            background: CaptureBackground::Transparent,
            size: None,
            vertical_text: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Overlays {
    pub input: OverlaySettings,
    pub display: OverlaySettings,
    pub peer_display: OverlaySettings,
    pub history: OverlaySettings,
}

impl Overlays {
    /// Lock the display, partner and history overlays (clicks pass through
    /// them), or unlock them all when the display is locked.
    pub fn toggle_lock(&mut self) {
        let lock = if self.display.lock == Lock::Locked {
            Lock::Interactive
        } else {
            Lock::Locked
        };
        self.display.lock = lock;
        self.peer_display.lock = lock;
        self.history.lock = lock;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistorySync {
    /// The row appears when the line is sent.
    OnSubmit,
    /// The row types along with the display.
    #[default]
    Reveal,
    /// The row appears when the line is fully revealed.
    OnComplete,
}

/// Whether the history leaves out the line still on the dialogue box, until
/// the box clears or the next line replaces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HideLatest {
    /// As the skin says (`history.hide_latest`).
    #[default]
    Skin,
    Always,
    Never,
}

impl HideLatest {
    pub fn resolve(self, skin: bool) -> bool {
        match self {
            Self::Skin => skin,
            Self::Always => true,
            Self::Never => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    /// Rows kept in memory.
    pub max_items: u32,
    /// Rows drawn; older ones scroll away.
    pub visible_items: u32,
    /// Rows fade out after this many seconds; `None` keeps them.
    pub fade_after_s: Option<u32>,
    pub show_avatar: bool,
    pub show_name: bool,
    pub show_time: bool,
    pub sync: HistorySync,
    pub hide_latest: HideLatest,
    /// Live chat rows mixed into the list.
    pub chat: HistoryChat,
    /// Show the chat message a line answers above it.
    pub show_quote: bool,
    /// Sent lines saved to disk.
    pub archive: HistoryArchive,
}

/// Saved history: one JSON Lines file per session under the data folder.
/// Clearing the on-screen history never touches these files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryArchive {
    /// Save the streamer's own lines and a co-op partner's.
    pub enabled: bool,
    /// Also save viewer chat that joins the history (off: it is other
    /// people's words).
    pub chat: bool,
    /// Sessions older than this many days go at startup; 0 keeps them all.
    pub keep_days: u32,
}

impl Default for HistoryArchive {
    fn default() -> Self {
        Self {
            enabled: true,
            chat: false,
            keep_days: 30,
        }
    }
}

/// Which live-chat messages a list shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatFilter {
    pub text: bool,
    pub super_chat: bool,
    pub gift: bool,
    pub member: bool,
    /// Gifts worth less than this many gold coins (1000 = ¥1) are left out;
    /// free gifts are worth nothing.
    pub min_gift_coin: u32,
}

impl Default for ChatFilter {
    fn default() -> Self {
        Self {
            text: true,
            super_chat: true,
            gift: true,
            member: true,
            min_gift_coin: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryChat {
    pub enabled: bool,
    pub filter: ChatFilter,
}

impl Default for HistoryChat {
    fn default() -> Self {
        Self {
            enabled: false,
            filter: ChatFilter {
                min_gift_coin: 1,
                ..ChatFilter::default()
            },
        }
    }
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            max_items: 200,
            visible_items: 30,
            fade_after_s: None,
            show_avatar: true,
            show_name: true,
            show_time: false,
            sync: HistorySync::Reveal,
            hide_latest: HideLatest::Skin,
            chat: HistoryChat::default(),
            show_quote: true,
            archive: HistoryArchive::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    /// Bring the input window forward.
    pub summon: Option<Hotkey>,
    /// Open the one-line quick input bar.
    pub quick_input: Option<Hotkey>,
    #[serde(rename = "continue")]
    pub continue_: Option<Hotkey>,
    pub skip: Option<Hotkey>,
    pub clear: Option<Hotkey>,
    pub toggle_mode: Option<Hotkey>,
    pub toggle_overlays: Option<Hotkey>,
    /// Bring the input window forward with live chat in view.
    pub show_chat: Option<Hotkey>,
    /// Bring the input window forward replying to the newest chat message.
    pub reply_latest: Option<Hotkey>,
    /// Bring the input window forward with the quick phrases open.
    pub quick_phrases: Option<Hotkey>,
    /// Lock or unlock the overlays (click-through).
    pub toggle_lock: Option<Hotkey>,
    /// Show the last sent line on the display again.
    pub replay: Option<Hotkey>,
    pub toggle_history: Option<Hotkey>,
    pub next_character: Option<Hotkey>,
    pub prev_character: Option<Hotkey>,
    /// Switch to the next profile in the list.
    pub next_profile: Option<Hotkey>,
    /// Switch to one profile, by its id. Global like every hotkey here (a
    /// profile never brings or takes these); one entry per saved profile.
    pub profiles: Vec<ProfileHotkey>,
    /// Keys handled by the input window itself, not registered system-wide.
    pub input: InputKeys,
}

/// The hotkey that switches to a saved profile.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileHotkey {
    /// The profile's file stem (profiles/<id>.json).
    pub id: String,
    /// Its name as last seen, for the Hotkeys page.
    pub name: String,
    pub hotkey: Option<Hotkey>,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            summon: Some(Hotkey::bare("F8")),
            quick_input: None,
            continue_: Some(Hotkey::bare("F9")),
            skip: None,
            clear: None,
            toggle_mode: None,
            toggle_overlays: None,
            show_chat: None,
            reply_latest: None,
            quick_phrases: None,
            toggle_lock: None,
            replay: None,
            toggle_history: None,
            next_character: None,
            prev_character: None,
            next_profile: None,
            profiles: Vec::new(),
            input: InputKeys::default(),
        }
    }
}

/// The input window's own keys. `None` leaves the key to the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputKeys {
    pub send: Option<Hotkey>,
    pub newline: Option<Hotkey>,
    pub recall_prev: Option<Hotkey>,
    pub recall_next: Option<Hotkey>,
    pub hide: Option<Hotkey>,
    pub bold: Option<Hotkey>,
    pub italic: Option<Hotkey>,
    pub underline: Option<Hotkey>,
    pub chat_prev: Option<Hotkey>,
    pub chat_next: Option<Hotkey>,
    pub reply: Option<Hotkey>,
    pub mention: Option<Hotkey>,
    pub cancel_reply: Option<Hotkey>,
    pub toggle_chat: Option<Hotkey>,
    pub phrases: Option<Hotkey>,
    pub stickers: Option<Hotkey>,
}

impl Default for InputKeys {
    fn default() -> Self {
        let mut keys = Self {
            send: None,
            newline: None,
            recall_prev: None,
            recall_next: None,
            hide: None,
            bold: None,
            italic: None,
            underline: None,
            chat_prev: None,
            chat_next: None,
            reply: None,
            mention: None,
            cancel_reply: None,
            toggle_chat: None,
            phrases: None,
            stickers: None,
        };
        for key in InputKey::ALL {
            *key.slot(&mut keys) = key.default_binding();
        }
        keys
    }
}

impl InputKeys {
    /// The action bound to `chord`, if any.
    pub fn matching(&self, chord: &Hotkey) -> Option<InputKey> {
        InputKey::ALL
            .into_iter()
            .find(|k| k.get(self) == Some(chord))
    }
}

/// An action of the input window's own keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputKey {
    Send,
    Newline,
    RecallPrev,
    RecallNext,
    Hide,
    Bold,
    Italic,
    Underline,
    /// Select the chat message above the selected one (the newest first).
    ChatPrev,
    /// Select the one below; past the newest, select none.
    ChatNext,
    /// Reply to the selected chat message, or the newest.
    Reply,
    /// Mention the selected message's author, or the newest's.
    Mention,
    CancelReply,
    /// Show live chat in the sidebar, or put it away.
    ToggleChat,
    /// Open the sidebar's quick phrases, or fold them away.
    Phrases,
    Stickers,
}

impl InputKey {
    pub const ALL: [Self; 16] = [
        Self::Send,
        Self::Newline,
        Self::RecallPrev,
        Self::RecallNext,
        Self::Hide,
        Self::Bold,
        Self::Italic,
        Self::Underline,
        Self::ChatPrev,
        Self::ChatNext,
        Self::Reply,
        Self::Mention,
        Self::CancelReply,
        Self::ToggleChat,
        Self::Phrases,
        Self::Stickers,
    ];

    /// Why `chord` may not be this key's.
    pub fn refusal(self, chord: &Hotkey) -> Option<Refusal> {
        chord.refusal_local(self == Self::Send)
    }

    /// Whether this key acts on live chat (Settings › Hotkeys lists these
    /// apart).
    pub fn is_chat(self) -> bool {
        matches!(
            self,
            Self::ChatPrev
                | Self::ChatNext
                | Self::Reply
                | Self::Mention
                | Self::CancelReply
                | Self::ToggleChat
        )
    }

    pub fn default_binding(self) -> Option<Hotkey> {
        let alt = |key: &str| Hotkey {
            alt: true,
            ..Hotkey::bare(key)
        };
        Some(match self {
            Self::Send => Hotkey::bare("Enter"),
            Self::Newline => Hotkey::shift("Enter"),
            Self::RecallPrev => Hotkey::ctrl("ArrowUp"),
            Self::RecallNext => Hotkey::ctrl("ArrowDown"),
            Self::Hide => Hotkey::bare("Escape"),
            Self::Bold => Hotkey::ctrl("KeyB"),
            Self::Italic => Hotkey::ctrl("KeyI"),
            Self::Underline => Hotkey::ctrl("KeyU"),
            Self::ChatPrev => alt("ArrowUp"),
            Self::ChatNext => alt("ArrowDown"),
            Self::Reply => alt("KeyR"),
            Self::Mention => alt("KeyM"),
            Self::CancelReply | Self::ToggleChat | Self::Phrases | Self::Stickers => return None,
        })
    }

    pub fn get(self, keys: &InputKeys) -> Option<&Hotkey> {
        match self {
            Self::Send => keys.send.as_ref(),
            Self::Newline => keys.newline.as_ref(),
            Self::RecallPrev => keys.recall_prev.as_ref(),
            Self::RecallNext => keys.recall_next.as_ref(),
            Self::Hide => keys.hide.as_ref(),
            Self::Bold => keys.bold.as_ref(),
            Self::Italic => keys.italic.as_ref(),
            Self::Underline => keys.underline.as_ref(),
            Self::ChatPrev => keys.chat_prev.as_ref(),
            Self::ChatNext => keys.chat_next.as_ref(),
            Self::Reply => keys.reply.as_ref(),
            Self::Mention => keys.mention.as_ref(),
            Self::CancelReply => keys.cancel_reply.as_ref(),
            Self::ToggleChat => keys.toggle_chat.as_ref(),
            Self::Phrases => keys.phrases.as_ref(),
            Self::Stickers => keys.stickers.as_ref(),
        }
    }

    pub fn slot(self, keys: &mut InputKeys) -> &mut Option<Hotkey> {
        match self {
            Self::Send => &mut keys.send,
            Self::Newline => &mut keys.newline,
            Self::RecallPrev => &mut keys.recall_prev,
            Self::RecallNext => &mut keys.recall_next,
            Self::Hide => &mut keys.hide,
            Self::Bold => &mut keys.bold,
            Self::Italic => &mut keys.italic,
            Self::Underline => &mut keys.underline,
            Self::ChatPrev => &mut keys.chat_prev,
            Self::ChatNext => &mut keys.chat_next,
            Self::Reply => &mut keys.reply,
            Self::Mention => &mut keys.mention,
            Self::CancelReply => &mut keys.cancel_reply,
            Self::ToggleChat => &mut keys.toggle_chat,
            Self::Phrases => &mut keys.phrases,
            Self::Stickers => &mut keys.stickers,
        }
    }
}

/// An output's longest side: no output is larger than 1080p, either way round.
pub const OUTPUT_MAX_LONG: u32 = 1920;
/// An output's shortest side.
pub const OUTPUT_MAX_SHORT: u32 = 1080;
/// The smallest side an output has.
pub const OUTPUT_MIN: u32 = 16;

/// `width × height` scaled down, keeping its aspect, until it fits 1080p
/// either way round, in whole pixels of at least [`OUTPUT_MIN`]. `None` for
/// a size that is not a size (under half a pixel, not finite).
pub fn fit_output_size(width: f64, height: f64) -> Option<[u32; 2]> {
    if !(width.is_finite() && height.is_finite() && width >= 0.5 && height >= 0.5) {
        return None;
    }
    let (long, short) = (width.max(height), width.min(height));
    let scale = (f64::from(OUTPUT_MAX_LONG) / long)
        .min(f64::from(OUTPUT_MAX_SHORT) / short)
        .min(1.0);
    let side = |v: f64| ((v * scale).round() as u32).clamp(OUTPUT_MIN, OUTPUT_MAX_LONG);
    Some([side(width), side(height)])
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpoutOutput {
    pub enabled: bool,
    pub name: String,
    /// Fixed sender size; `None` follows the window. Also what is sent while
    /// the OBS source below cannot be read.
    pub size: Option<[u32; 2]>,
    /// Straight alpha for receivers without a premultiplied mode.
    pub straight_alpha: bool,
    /// The OBS source (in the program scene) whose bounding box sets the
    /// size while OBS is linked; empty for none.
    pub obs_source: String,
    /// The size that box gives now, already fitted to 1080p. Runtime only:
    /// never saved, and `None` whenever OBS cannot say.
    #[serde(skip)]
    pub obs_size: Option<[u32; 2]>,
}

impl SpoutOutput {
    /// The size to send: OBS's bounding box when known, else the fixed
    /// size; `None` follows the window.
    pub fn extent(&self) -> Option<[u32; 2]> {
        self.obs_size.or(self.size)
    }
}

/// The obs-websocket server (OBS › Tools › WebSocket Server Settings). Its
/// password is kept in the system's secret store, never here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObsLink {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
}

impl Default for ObsLink {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".into(),
            port: 4455,
        }
    }
}

/// Partner seats in a co-op room: everyone but the local person, in seat
/// order (the host, then guests as they joined).
pub const PARTNER_SEATS: usize = 7;

/// Seat `index`'s default sender name, counted from one.
pub fn partner_output_name(index: usize) -> String {
    format!("NanaType Partner {}", index + 1)
}

/// Seat `index`'s sender as it starts out: off, under its own name.
pub fn partner_output(index: usize) -> SpoutOutput {
    SpoutOutput {
        enabled: false,
        name: partner_output_name(index),
        ..SpoutOutput::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    pub display: SpoutOutput,
    /// One sender per partner seat, remembered by seat: whoever sits there
    /// goes out under that seat's name. Always [`PARTNER_SEATS`] long.
    pub partners: Vec<SpoutOutput>,
    pub history: SpoutOutput,
    pub obs: ObsLink,
    /// The single partner sender of earlier versions, read once into the
    /// first seat by [`Settings::sanitize`].
    #[serde(skip_serializing)]
    pub peer_display: Option<SpoutOutput>,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            display: SpoutOutput {
                enabled: false,
                name: "NanaType Display".into(),
                ..SpoutOutput::default()
            },
            partners: (0..PARTNER_SEATS).map(partner_output).collect(),
            peer_display: None,
            history: SpoutOutput {
                enabled: false,
                name: "NanaType History".into(),
                ..SpoutOutput::default()
            },
            obs: ObsLink::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VtsSettings {
    pub enabled: bool,
    pub port: u16,
    /// Plugin token granted by VTube Studio.
    pub token: Option<String>,
    /// Expression name → VTube Studio hotkey id.
    pub expressions: BTreeMap<String, String>,
    /// Move the model's mouth while the speaker is talking.
    pub lip_sync: bool,
    /// The parameter lip sync drives.
    pub mouth_param: String,
    /// How far the mouth opens (the parameter's peak value).
    pub mouth_amp: f32,
    /// When expressions turned on by a line are turned off again.
    pub reset_expressions: VtsReset,
}

impl Default for VtsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8001,
            token: None,
            expressions: BTreeMap::new(),
            lip_sync: true,
            mouth_param: "MouthOpen".into(),
            mouth_amp: 1.0,
            reset_expressions: VtsReset::LineEnd,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OscSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub prefix: String,
}

impl Default for OscSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".into(),
            port: 9000,
            prefix: "/nanatype".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WsSettings {
    pub enabled: bool,
    pub port: u16,
}

impl Default for WsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 41890,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPanelStyle {
    #[default]
    Compact,
    History,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatSettings {
    /// What the chat panel lists.
    pub panel: ChatFilter,
    /// Presentation of the operator's panel, independent of its message filter.
    pub panel_style: ChatPanelStyle,
    /// Quote the answered message on the dialogue box.
    pub quote_on_display: bool,
    /// The old chat window was open when the app last quit. Read once to pin
    /// chat in the input sidebar on first run of the sidebar; no longer written.
    pub panel_open: bool,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            panel: ChatFilter::default(),
            panel_style: ChatPanelStyle::default(),
            quote_on_display: true,
            panel_open: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TwitchSettings {
    pub enabled: bool,
    /// A channel login or its twitch.tv address.
    pub channel: String,
}

/// Where blivechat-protocol chat comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlivechatSource {
    /// A blivechat server at `host:port`.
    #[default]
    Server,
    /// The 彗星号 client's own feed on `127.0.0.1:comet_port`.
    Comet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BliveRoomKind {
    /// The streamer's 身份码, kept in the system's secret store.
    #[default]
    AuthCode,
    RoomId,
}

/// Chat over the blivechat protocol: a blivechat server or the 彗星号 client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlivechatSettings {
    pub enabled: bool,
    pub source: BlivechatSource,
    pub host: String,
    pub port: u16,
    pub room_kind: BliveRoomKind,
    /// The room number when `room_kind` is `RoomId`. An auth code is never
    /// written here.
    pub room_id: String,
    /// The port the 彗星号 client serves its feed on.
    pub comet_port: u16,
}

/// Chat from a LAPLACE Event Bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaplaceSettings {
    pub enabled: bool,
    /// The bridge's link without its token (`ws://` or `wss://`), as read
    /// from what the user pasted. The token is kept in the system's secret
    /// store.
    pub url: String,
}

impl Default for LaplaceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            url: LAPLACE_URL.into(),
        }
    }
}

pub const BLIVECHAT_PORT: u16 = 12450;
pub const COMET_PORT: u16 = 19400;
/// NanaType's own 彗星号 relay listened here before it read the client.
const LEGACY_COMET_RELAY_PORT: u16 = 12451;
pub const LAPLACE_URL: &str = "ws://127.0.0.1:9696/";

impl Default for BlivechatSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            source: BlivechatSource::default(),
            host: "127.0.0.1".into(),
            port: BLIVECHAT_PORT,
            room_kind: BliveRoomKind::default(),
            room_id: String::new(),
            comet_port: COMET_PORT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Integrations {
    pub vts: VtsSettings,
    pub osc: OscSettings,
    pub websocket: WsSettings,
    pub chat: ChatSettings,
    pub blivechat: BlivechatSettings,
    pub laplace: LaplaceSettings,
    pub twitch: TwitchSettings,
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

/// Heights [`AssetSettings::sticker_scale`] keeps, in line heights.
pub const STICKER_SCALE: (f32, f32) = (0.8, 2.0);

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramSettings {
    pub api_id: i32,
    /// SOCKS5 URL. Login codes, API hashes and account passwords are never
    /// settings; a proxy's own `user:password@` part is, so the hint tells
    /// users to prefer an unauthenticated local proxy.
    pub proxy: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Explicitly enabled functional packages. Pure skins never start a VM.
    pub enabled_plugins: Vec<String>,
    /// Packages switched off: their skins and sound packs are not offered
    /// and their scripts do not run. Other packages may still depend on them.
    pub disabled_packages: Vec<String>,
    /// Approval binds explicit source trust to exact, reviewed package bytes.
    pub plugin_approvals: BTreeMap<String, PluginApproval>,
    pub schema: u32,
    pub general: General,
    pub playback: PlaybackSettings,
    pub sound: SoundSettings,
    pub speech: SpeechSettings,
    pub tts: TtsSettings,
    pub translate: TranslateSettings,
    pub skin: SkinSettings,
    pub overlays: Overlays,
    pub history: HistorySettings,
    pub hotkeys: HotkeySettings,
    pub output: OutputSettings,
    pub integrations: Integrations,
    pub assets: AssetSettings,
    pub update: UpdateSettings,
    pub diagnostics: DiagnosticsSettings,
    pub performance: PerformanceSettings,
    pub accessibility: AccessibilitySettings,
    /// Fields from a newer build, preserved verbatim.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SETTINGS_SCHEMA,
            enabled_plugins: Vec::new(),
            disabled_packages: Vec::new(),
            plugin_approvals: BTreeMap::new(),
            general: General::default(),
            playback: PlaybackSettings::default(),
            sound: SoundSettings::default(),
            speech: SpeechSettings::default(),
            tts: TtsSettings::default(),
            translate: TranslateSettings::default(),
            skin: SkinSettings::default(),
            overlays: Overlays::default(),
            history: HistorySettings::default(),
            hotkeys: HotkeySettings::default(),
            output: OutputSettings::default(),
            integrations: Integrations::default(),
            assets: AssetSettings::default(),
            update: UpdateSettings::default(),
            diagnostics: DiagnosticsSettings::default(),
            performance: PerformanceSettings::default(),
            accessibility: AccessibilitySettings::default(),
            unknown: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// Only the send key may hold Enter or Ctrl+Enter. Another key that has
    /// one (the newline key of the old Ctrl+Enter-sends layout, a
    /// hand-edited file) goes back to its default, or to none when that is
    /// taken: plain Enter breaks the line in the editor anyway.
    fn sanitize_hotkeys(&mut self) {
        let h = &mut self.hotkeys;
        for slot in [
            &mut h.summon,
            &mut h.quick_input,
            &mut h.continue_,
            &mut h.skip,
            &mut h.clear,
            &mut h.toggle_mode,
            &mut h.toggle_overlays,
            &mut h.show_chat,
            &mut h.reply_latest,
            &mut h.quick_phrases,
            &mut h.toggle_lock,
            &mut h.replay,
            &mut h.toggle_history,
            &mut h.next_character,
            &mut h.prev_character,
            &mut h.next_profile,
        ] {
            if slot.as_ref().is_some_and(Hotkey::is_send_chord) {
                *slot = None;
            }
        }
        for phrase in &mut self.assets.phrases {
            if phrase.hotkey.as_ref().is_some_and(Hotkey::is_send_chord) {
                phrase.hotkey = None;
            }
        }
        for character in &mut self.general.characters {
            if character.hotkey.as_ref().is_some_and(Hotkey::is_send_chord) {
                character.hotkey = None;
            }
        }
        for profile in &mut h.profiles {
            if profile.hotkey.as_ref().is_some_and(Hotkey::is_send_chord) {
                profile.hotkey = None;
            }
        }
        let keys = &mut h.input;
        for key in InputKey::ALL {
            if key == InputKey::Send || !key.get(keys).is_some_and(Hotkey::is_send_chord) {
                continue;
            }
            let fallback = key
                .default_binding()
                .filter(|d| InputKey::ALL.into_iter().all(|k| k.get(keys) != Some(d)));
            *key.slot(keys) = fallback;
        }
    }

    /// Clamp values a hand-edited file could break. Returns whether anything changed.
    pub fn sanitize(&mut self) -> bool {
        let before = self.clone();
        self.speech.silence_seconds = self.speech.silence_seconds.clamp(1, 10);
        if self.speech.language.as_ref().is_some_and(|v| {
            v.is_empty()
                || v.len() > 64
                || !v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) {
            self.speech.language = None;
        }
        let t = &mut self.tts;
        t.rate = finite_or(t.rate, 1.0).clamp(TTS_RATE.0, TTS_RATE.1);
        t.volume = finite_or(t.volume, 1.0).clamp(0.0, 1.0);
        let voice_id = |v: &mut Option<String>| {
            if v.as_ref().is_some_and(|v| v.is_empty() || v.len() > 512) {
                *v = None;
            }
        };
        voice_id(&mut t.voice);
        for character in &mut self.general.characters {
            voice_id(&mut character.voice);
        }
        let tr = &mut self.translate;
        let target = tr.target.trim();
        if target.is_empty()
            || target.len() > 16
            || !target
                .split('-')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            tr.target = TranslateSettings::default().target;
        } else if target.len() != tr.target.len() {
            tr.target = target.to_owned();
        }
        for (field, limit) in [
            (&mut tr.base_url, 2048),
            (&mut tr.model, 256),
            (&mut tr.proxy, 2048),
        ] {
            if field.len() > limit {
                field.clear();
            }
        }
        let package_id = |id: &str| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
                && id != "."
                && id != ".."
        };
        let mut plugins = std::collections::BTreeSet::new();
        self.enabled_plugins
            .retain(|id| package_id(id) && plugins.insert(id.clone()));
        self.enabled_plugins.truncate(MAX_ENABLED_PLUGINS);
        let mut disabled = std::collections::BTreeSet::new();
        self.disabled_packages
            .retain(|id| package_id(id) && disabled.insert(id.clone()));
        self.disabled_packages.truncate(MAX_DISABLED_PACKAGES);
        // A package that is off runs nothing.
        self.enabled_plugins
            .retain(|id| !self.disabled_packages.contains(id));
        self.plugin_approvals.retain(|id, approval| {
            self.enabled_plugins.contains(id)
                && approval.is_current()
                && approval.digest.len() == 64
                && approval.digest.bytes().all(|b| b.is_ascii_hexdigit())
        });
        let p = &mut self.playback;
        p.speed_scale = finite_or(p.speed_scale, 1.0).clamp(0.1, 5.0);
        p.mirror_stagger_ms = p.mirror_stagger_ms.min(500);
        let s = &mut self.sound;
        for v in [
            &mut s.master,
            &mut s.typing_volume,
            &mut s.effects_volume,
            &mut s.ui_volume,
            &mut s.keystroke_volume,
        ] {
            *v = finite_or(*v, 1.0).clamp(0.0, 1.0);
        }
        let (lo, hi, step) = UI_SCALE;
        let a = &mut self.accessibility;
        a.ui_scale = (a.ui_scale.clamp(lo, hi) + step / 2) / step * step;
        let h = &mut self.history;
        h.max_items = h.max_items.clamp(1, 5000);
        h.visible_items = h.visible_items.clamp(1, h.max_items);
        if !SKIN_FRAME_RATES.contains(&self.skin.frame_rate) {
            self.skin.frame_rate = SKIN_FRAME_RATES[0];
        }
        if let Some(size) = self.skin.overrides.text_size.as_mut() {
            *size = finite_or(*size, 28.0).clamp(8.0, 200.0);
        }
        for scale in [
            &mut self.skin.overrides.name_scale,
            &mut self.skin.overrides.decor_scale,
        ] {
            *scale = scale
                .map(|v| {
                    finite_or(v, 1.0).clamp(SKIN_PART_SCALE.0 as f32, SKIN_PART_SCALE.1 as f32)
                })
                .filter(|v| (v - 1.0).abs() > 1e-3);
        }
        for profile in self.skin.personalizations.values_mut() {
            profile.sanitize();
        }
        for o in [
            &mut self.overlays.input,
            &mut self.overlays.display,
            &mut self.overlays.peer_display,
            &mut self.overlays.history,
        ] {
            if let Some([w, h]) = o.size.as_mut() {
                *w = finite_or(*w, 760.0).clamp(160.0, 8192.0);
                *h = finite_or(*h, 200.0).clamp(60.0, 8192.0);
            }
        }
        let o = &mut self.output;
        o.partners.truncate(PARTNER_SEATS);
        while o.partners.len() < PARTNER_SEATS {
            o.partners.push(partner_output(o.partners.len()));
        }
        if let Some(legacy) = o.peer_display.take() {
            o.partners[0] = legacy;
        }
        for out in [&mut o.display, &mut o.history]
            .into_iter()
            .chain(o.partners.iter_mut())
        {
            out.name = out.name.trim().chars().take(255).collect();
            out.obs_source = out.obs_source.chars().take(255).collect();
            if let Some([w, h]) = out.size {
                out.size = fit_output_size(f64::from(w.max(1)), f64::from(h.max(1)));
            }
        }
        let obs = &mut self.output.obs;
        obs.host = obs.host.trim().chars().take(255).collect();
        if obs.port == 0 {
            obs.port = ObsLink::default().port;
        }
        if self.skin.id.is_empty() {
            self.skin.id = DEFAULT_SKIN.into();
        }
        self.sanitize_hotkeys();
        let g = &mut self.general;
        g.quick_opacity = finite_or(g.quick_opacity, 1.0).clamp(0.0, 1.0);
        if g.onboarding_step >= ONBOARDING_STEPS {
            g.onboarding_step = 0;
        }
        g.characters.truncate(MAX_CHARACTERS);
        if g.characters.is_empty() {
            g.characters.push(Character::default());
        }
        for character in &mut g.characters {
            character.name = character.name.chars().take(64).collect();
        }
        if g.character >= g.characters.len() {
            g.character = 0;
        }
        self.update.proxy = self.update.proxy.trim().chars().take(512).collect();
        self.assets.sticker_scale =
            finite_or(self.assets.sticker_scale, 1.0).clamp(STICKER_SCALE.0, STICKER_SCALE.1);
        self.assets.telegram.api_id = self.assets.telegram.api_id.max(0);
        self.assets.telegram.proxy = self
            .assets
            .telegram
            .proxy
            .trim()
            .chars()
            .take(2048)
            .collect();
        let t = &mut self.integrations.twitch;
        t.channel = t.channel.trim().chars().take(2048).collect();
        let l = &mut self.integrations.blivechat;
        l.host = l.host.trim().chars().take(253).collect();
        if l.host.is_empty() {
            l.host = "127.0.0.1".into();
        }
        if l.port == 0 {
            l.port = BLIVECHAT_PORT;
        }
        if l.comet_port == 0 || l.comet_port == LEGACY_COMET_RELAY_PORT {
            l.comet_port = COMET_PORT;
        }
        l.room_id = l.room_id.trim().chars().take(20).collect();
        let l = &mut self.integrations.laplace;
        l.url = l.url.trim().chars().take(2048).collect();
        if l.url.is_empty() {
            l.url = LAPLACE_URL.into();
        }
        *self != before
    }
}

fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}
