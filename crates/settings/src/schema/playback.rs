use nanatype_model::{AfterLine, EntranceKind};
use nanatype_playback::QueuePolicy;
use serde::{Deserialize, Serialize};

use super::{clamp_f32, clamp_ord, sanitize_voice_id};

/// Rates [`TtsSettings::rate`] keeps, as a multiple of the voice's own pace.
pub const TTS_RATE: (f32, f32) = (0.5, 2.0);

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

impl SpeechSettings {
    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        clamp_ord(&mut self.silence_seconds, 1, 10, &mut dirty);
        if self.language.as_ref().is_some_and(|v| {
            v.is_empty()
                || v.len() > 64
                || !v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) {
            self.language = None;
            dirty = true;
        }
        dirty
    }
}

/// Sent lines read aloud by the system's voices. A character's own voice
/// is [`crate::Character::voice`].
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

impl TtsSettings {
    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        clamp_f32(&mut self.rate, 1.0, TTS_RATE.0, TTS_RATE.1, &mut dirty);
        clamp_f32(&mut self.volume, 1.0, 0.0, 1.0, &mut dirty);
        sanitize_voice_id(&mut self.voice, &mut dirty);
        dirty
    }
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

impl PlaybackSettings {
    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        clamp_f32(&mut self.speed_scale, 1.0, 0.1, 5.0, &mut dirty);
        clamp_ord(&mut self.mirror_stagger_ms, 0, 500, &mut dirty);
        dirty
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

impl SoundSettings {
    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        for v in [
            &mut self.master,
            &mut self.typing_volume,
            &mut self.effects_volume,
            &mut self.ui_volume,
            &mut self.keystroke_volume,
        ] {
            clamp_f32(v, 1.0, 0.0, 1.0, &mut dirty);
        }
        dirty
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
    /// Show viewer chat in its own overlay window and output instead of
    /// mixing it into the history log.
    pub separate: bool,
    pub filter: ChatFilter,
}

impl Default for HistoryChat {
    fn default() -> Self {
        Self {
            separate: false,
            filter: ChatFilter {
                min_gift_coin: 1,
                ..ChatFilter::default()
            },
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

impl HistorySettings {
    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        clamp_ord(&mut self.max_items, 1, 5000, &mut dirty);
        clamp_ord(&mut self.visible_items, 1, self.max_items, &mut dirty);
        dirty
    }
}
