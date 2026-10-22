//! Settings schema v1. Every struct is `#[serde(default)]`, so a file written
//! by an older build loads with new fields at their defaults, and fields this
//! build does not know are kept in `unknown` and written back unchanged.

use std::collections::BTreeMap;

use nanatype_model::{AfterLine, EntranceKind, Overflow, Rgba8, VtsReset};
use nanatype_playback::QueuePolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SETTINGS_SCHEMA: u32 = 2;
pub const DEFAULT_SKIN: &str = "nanatype.halo";
/// Raised whenever scripts gain data or host interfaces: 1 granted events and
/// views, 2 adds declared data reads. Older approvals must be reviewed again.
pub const PLUGIN_CONSENT_POLICY: u32 = 2;
pub const MAX_ENABLED_PLUGINS: usize = 8;

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
        let key = self
            .key
            .strip_prefix("Key")
            .or_else(|| self.key.strip_prefix("Digit"))
            .unwrap_or(&self.key);
        let mut out = parts.join("+");
        if !out.is_empty() {
            out.push('+');
        }
        out.push_str(key);
        out
    }

    /// Function keys and media keys are fine alone; anything that types needs
    /// a modifier, or it would swallow normal typing system-wide.
    pub fn is_safe_global(&self) -> bool {
        if self.has_modifier() {
            return !self.key.is_empty();
        }
        let k = self.key.as_str();
        (k.len() >= 2
            && k.starts_with('F')
            && k[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)))
            || matches!(k, "Pause" | "ScrollLock" | "Insert")
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendKey {
    /// Enter sends, Shift+Enter breaks the line.
    #[default]
    Enter,
    /// Ctrl+Enter sends, Enter breaks the line.
    CtrlEnter,
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
    pub speaker_name: String,
    /// Avatar image for history rows (absolute path).
    pub avatar: Option<String>,
    pub input_presence: InputPresence,
    pub input_mode: InputMode,
    /// Show the draft inside the skin's stream canvas instead of a full editor.
    pub input_skin_preview: bool,
    pub send_key: SendKey,
    pub launch_at_login: bool,
    pub onboarding_done: bool,
    pub check_updates: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            locale: None,
            speaker_name: String::new(),
            avatar: None,
            input_presence: InputPresence::Resident,
            input_mode: InputMode::Compose,
            input_skin_preview: false,
            send_key: SendKey::Enter,
            launch_at_login: false,
            onboarding_done: false,
            check_updates: true,
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
}

impl SkinPersonalization {
    pub fn valid(&self) -> bool {
        self.display.valid() && self.history.valid()
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
}

impl SkinOverrides {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinSettings {
    pub advanced_effects: bool,
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
    /// Live chat (Bilibili) rows mixed into the list.
    pub chat: HistoryChat,
    /// Show the chat message a line answers above it.
    pub show_quote: bool,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    pub summon: Option<Hotkey>,
    #[serde(rename = "continue")]
    pub continue_: Option<Hotkey>,
    pub skip: Option<Hotkey>,
    pub clear: Option<Hotkey>,
    pub toggle_mode: Option<Hotkey>,
    pub toggle_overlays: Option<Hotkey>,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            summon: Some(Hotkey::bare("F8")),
            continue_: Some(Hotkey::bare("F9")),
            skip: None,
            clear: None,
            toggle_mode: None,
            toggle_overlays: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpoutOutput {
    pub enabled: bool,
    pub name: String,
    /// Fixed sender size; `None` follows the window.
    pub size: Option<[u32; 2]>,
    /// Straight alpha for receivers without a premultiplied mode.
    pub straight_alpha: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    pub display: SpoutOutput,
    pub peer_display: SpoutOutput,
    pub history: SpoutOutput,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            display: SpoutOutput {
                enabled: false,
                name: "NanaType Display".into(),
                size: None,
                straight_alpha: false,
            },
            peer_display: SpoutOutput {
                enabled: false,
                name: "NanaType Partner".into(),
                size: None,
                straight_alpha: false,
            },
            history: SpoutOutput {
                enabled: false,
                name: "NanaType History".into(),
                size: None,
                straight_alpha: false,
            },
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
pub struct BilibiliSettings {
    pub enabled: bool,
    /// Empty room selects the authenticated account's own room.
    pub sync_profile: bool,
    pub room: String,
    /// What the chat panel lists.
    pub panel: ChatFilter,
    /// Presentation of the operator's panel, independent of its message filter.
    pub panel_style: ChatPanelStyle,
    /// Quote the answered message on the dialogue box.
    pub quote_on_display: bool,
    /// The chat panel was open when the app last quit.
    pub panel_open: bool,
}

impl Default for BilibiliSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sync_profile: false,
            room: String::new(),
            panel: ChatFilter::default(),
            panel_style: ChatPanelStyle::default(),
            quote_on_display: true,
            panel_open: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DouyinSettings {
    pub enabled: bool,
    pub room: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct YoutubeSettings {
    pub enabled: bool,
    /// A video or channel address, an `@handle`, a channel id or a video id.
    pub target: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TwitchSettings {
    pub enabled: bool,
    /// A channel login or its twitch.tv address.
    pub channel: String,
}

/// Where third-party chat comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlivechatSource {
    /// A blivechat server at `host:port`.
    #[default]
    Server,
    /// The 彗星号 workshop plugin, through NanaType's own relay.
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

/// Third-party chat over the blivechat protocol. The key is free again: the
/// v1 blivechat settings moved to `bilibili` in the step to v2.
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
    /// The local relay the 彗星号 page connects to.
    pub comet_port: u16,
    /// What the 彗星号 page must present; it keeps other local pages from
    /// pushing chat. Made when 彗星号 is first chosen.
    pub comet_token: String,
}

pub const BLIVECHAT_PORT: u16 = 12450;
pub const COMET_RELAY_PORT: u16 = 12451;

impl Default for BlivechatSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            source: BlivechatSource::default(),
            host: "127.0.0.1".into(),
            port: BLIVECHAT_PORT,
            room_kind: BliveRoomKind::default(),
            room_id: String::new(),
            comet_port: COMET_RELAY_PORT,
            comet_token: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Integrations {
    pub vts: VtsSettings,
    pub osc: OscSettings,
    pub websocket: WsSettings,
    pub bilibili: BilibiliSettings,
    pub douyin: DouyinSettings,
    pub blivechat: BlivechatSettings,
    pub youtube: YoutubeSettings,
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

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetSettings {
    /// Folders whose images become sticker libraries (folder name = library).
    pub sticker_dirs: Vec<String>,
    pub phrases: Vec<QuickPhrase>,
    pub telegram: TelegramSettings,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Explicitly enabled functional packages. Pure skins never start a VM.
    pub enabled_plugins: Vec<String>,
    /// Approval binds explicit source trust to exact, reviewed package bytes.
    pub plugin_approvals: BTreeMap<String, PluginApproval>,
    pub schema: u32,
    pub general: General,
    pub playback: PlaybackSettings,
    pub sound: SoundSettings,
    pub skin: SkinSettings,
    pub overlays: Overlays,
    pub history: HistorySettings,
    pub hotkeys: HotkeySettings,
    pub output: OutputSettings,
    pub integrations: Integrations,
    pub assets: AssetSettings,
    pub update: UpdateSettings,
    /// Fields from a newer build, preserved verbatim.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SETTINGS_SCHEMA,
            enabled_plugins: Vec::new(),
            plugin_approvals: BTreeMap::new(),
            general: General::default(),
            playback: PlaybackSettings::default(),
            sound: SoundSettings::default(),
            skin: SkinSettings::default(),
            overlays: Overlays::default(),
            history: HistorySettings::default(),
            hotkeys: HotkeySettings::default(),
            output: OutputSettings::default(),
            integrations: Integrations::default(),
            assets: AssetSettings::default(),
            update: UpdateSettings::default(),
            unknown: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// Clamp values a hand-edited file could break. Returns whether anything changed.
    pub fn sanitize(&mut self) -> bool {
        let before = self.clone();
        let mut plugins = std::collections::BTreeSet::new();
        self.enabled_plugins.retain(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
                && id != "."
                && id != ".."
                && plugins.insert(id.clone())
        });
        self.enabled_plugins.truncate(MAX_ENABLED_PLUGINS);
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
        let h = &mut self.history;
        h.max_items = h.max_items.clamp(1, 5000);
        h.visible_items = h.visible_items.clamp(1, h.max_items);
        if let Some(size) = self.skin.overrides.text_size.as_mut() {
            *size = finite_or(*size, 28.0).clamp(8.0, 200.0);
        }
        for profile in self.skin.personalizations.values_mut() {
            profile.display.sanitize();
            profile.history.sanitize();
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
        for out in [
            &mut self.output.display,
            &mut self.output.peer_display,
            &mut self.output.history,
        ] {
            out.name = out.name.trim().chars().take(255).collect();
            if let Some([w, h]) = out.size.as_mut() {
                *w = (*w).clamp(16, 8192);
                *h = (*h).clamp(16, 8192);
            }
        }
        if self.skin.id.is_empty() {
            self.skin.id = DEFAULT_SKIN.into();
        }
        self.general.speaker_name = self.general.speaker_name.chars().take(64).collect();
        self.update.proxy = self.update.proxy.trim().chars().take(512).collect();
        self.assets.telegram.api_id = self.assets.telegram.api_id.max(0);
        self.assets.telegram.proxy = self
            .assets
            .telegram
            .proxy
            .trim()
            .chars()
            .take(2048)
            .collect();
        let d = &mut self.integrations.douyin;
        d.room = d.room.trim().chars().take(2048).collect();
        let y = &mut self.integrations.youtube;
        y.target = y.target.trim().chars().take(2048).collect();
        let t = &mut self.integrations.twitch;
        t.channel = t.channel.trim().chars().take(2048).collect();
        let b = &mut self.integrations.bilibili;
        b.room = b.room.trim().to_owned();
        let l = &mut self.integrations.blivechat;
        l.host = l.host.trim().chars().take(253).collect();
        if l.host.is_empty() {
            l.host = "127.0.0.1".into();
        }
        if l.port == 0 {
            l.port = BLIVECHAT_PORT;
        }
        if l.comet_port == 0 {
            l.comet_port = COMET_RELAY_PORT;
        }
        l.room_id = l.room_id.trim().chars().take(20).collect();
        l.comet_token.retain(|c| c.is_ascii_alphanumeric());
        l.comet_token.truncate(64);
        *self != before
    }
}

fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}
