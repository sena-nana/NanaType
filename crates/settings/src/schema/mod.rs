//! Settings schema v10. Every struct is `#[serde(default)]`, so a file written
//! by an older build loads with new fields at their defaults, and fields this
//! build does not know are kept in `unknown` and written back unchanged.

mod general;
mod hotkeys;
mod integrations;
mod playback;
mod skin;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use general::{
    AccessibilitySettings, AssetSettings, Character, ChromeMode, ContrastMode, DiagnosticsSettings,
    General, InputMode, InputPresence, MAX_CHARACTERS, ONBOARDING_STEPS, PerformanceSettings,
    QuickPhrase, STICKER_SCALE, StreamRoute, TelegramSettings, UI_SCALE, UpdateSettings,
};
pub use hotkeys::{Hotkey, HotkeySettings, InputKey, InputKeys, ProfileHotkey, Refusal};
pub use integrations::{
    BLIVECHAT_PORT, BilibiliLink, BilibiliSettings, BilibiliSource, BliveRoomKind,
    BlivechatSettings, COMET_PORT, ChatPanelStyle, ChatSettings, ChatSource, CometRooms,
    CometSettings, Integrations, LAPLACE_URL, LaplaceSettings, NANALIVE_PORT, NanaLiveSettings,
    OscSettings, TwitchSettings, VtsSettings, WsSettings,
};
pub use playback::{
    ChatFilter, HideLatest, HistoryArchive, HistoryChat, HistorySettings, HistorySync,
    KeystrokeScheme, PlaybackSettings, SoundSettings, SpeechBackend, SpeechMode, SpeechSettings,
    TTS_RATE, TtsSettings,
};
pub use skin::{
    CaptureBackground, DEFAULT_SKIN, Lock, MAX_SKIN_PARAMS, OUTPUT_MAX_LONG, OUTPUT_MAX_SHORT,
    OUTPUT_MIN, ObsLink, OutputSettings, OverlaySettings, Overlays, PARTNER_SEATS,
    SKIN_FRAME_RATES, SKIN_PART_SCALE, SkinOverrides, SkinPersonalization, SkinSettings,
    SpoutOutput, SurfacePersonalization, fit_output_size, partner_output, partner_output_name,
};

pub const SETTINGS_SCHEMA: u32 = 11;
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
    fn sanitize_hotkeys(&mut self) -> bool {
        let mut dirty = false;
        let h = &mut self.hotkeys;
        for slot in h.global_slots_mut() {
            if slot.as_ref().is_some_and(Hotkey::is_send_chord) {
                *slot = None;
                dirty = true;
            }
        }
        for phrase in &mut self.assets.phrases {
            if phrase.hotkey.as_ref().is_some_and(Hotkey::is_send_chord) {
                phrase.hotkey = None;
                dirty = true;
            }
        }
        for character in &mut self.general.characters {
            if character.hotkey.as_ref().is_some_and(Hotkey::is_send_chord) {
                character.hotkey = None;
                dirty = true;
            }
        }
        for profile in &mut h.profiles {
            if profile.hotkey.as_ref().is_some_and(Hotkey::is_send_chord) {
                profile.hotkey = None;
                dirty = true;
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
            dirty = true;
        }
        dirty
    }

    /// Clamp values a hand-edited file could break. Returns whether anything changed.
    pub fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        dirty |= self.integrations.sanitize();
        dirty |= self.speech.sanitize();
        dirty |= self.tts.sanitize();
        dirty |= self.sanitize_packages();
        dirty |= self.playback.sanitize();
        dirty |= self.sound.sanitize();
        dirty |= self.accessibility.sanitize();
        dirty |= self.history.sanitize();
        dirty |= self.skin.sanitize();
        dirty |= self.overlays.sanitize();
        dirty |= self.output.sanitize();
        dirty |= self.sanitize_hotkeys();
        dirty |= self.general.sanitize();
        dirty |= self.update.sanitize();
        dirty |= self.assets.sanitize();
        dirty
    }

    fn sanitize_packages(&mut self) -> bool {
        let mut dirty = false;
        let package_id = |id: &str| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
                && id != "."
                && id != ".."
        };
        let mut plugins = BTreeSet::new();
        let before_enabled = self.enabled_plugins.len();
        self.enabled_plugins
            .retain(|id| package_id(id) && plugins.insert(id.clone()));
        if self.enabled_plugins.len() > MAX_ENABLED_PLUGINS {
            self.enabled_plugins.truncate(MAX_ENABLED_PLUGINS);
        }
        dirty |= self.enabled_plugins.len() != before_enabled;

        let mut disabled = BTreeSet::new();
        let before_disabled = self.disabled_packages.len();
        self.disabled_packages
            .retain(|id| package_id(id) && disabled.insert(id.clone()));
        if self.disabled_packages.len() > MAX_DISABLED_PACKAGES {
            self.disabled_packages.truncate(MAX_DISABLED_PACKAGES);
        }
        dirty |= self.disabled_packages.len() != before_disabled;

        // A package that is off runs nothing.
        let before_active = self.enabled_plugins.len();
        self.enabled_plugins
            .retain(|id| !self.disabled_packages.contains(id));
        dirty |= self.enabled_plugins.len() != before_active;

        let before_approvals = self.plugin_approvals.len();
        self.plugin_approvals.retain(|id, approval| {
            self.enabled_plugins.contains(id)
                && approval.is_current()
                && approval.digest.len() == 64
                && approval.digest.bytes().all(|b| b.is_ascii_hexdigit())
        });
        dirty |= self.plugin_approvals.len() != before_approvals;
        dirty
    }
}

pub(crate) fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}

pub(crate) fn set_if<T: PartialEq>(slot: &mut T, next: T, dirty: &mut bool) {
    if *slot != next {
        *slot = next;
        *dirty = true;
    }
}

pub(crate) fn clamp_f32(slot: &mut f32, fallback: f32, lo: f32, hi: f32, dirty: &mut bool) {
    let next = finite_or(*slot, fallback).clamp(lo, hi);
    if !slot.is_finite() || *slot != next {
        *slot = next;
        *dirty = true;
    }
}

pub(crate) fn clamp_f32_opt(
    slot: &mut Option<f32>,
    fallback: f32,
    lo: f32,
    hi: f32,
    dirty: &mut bool,
) {
    if let Some(v) = slot.as_mut() {
        clamp_f32(v, fallback, lo, hi, dirty);
    }
}

pub(crate) fn clamp_ord<T: Ord + Copy>(slot: &mut T, lo: T, hi: T, dirty: &mut bool) {
    let next = (*slot).clamp(lo, hi);
    set_if(slot, next, dirty);
}

pub(crate) fn trim_take(slot: &mut String, max_chars: usize, dirty: &mut bool) {
    let next: String = slot.trim().chars().take(max_chars).collect();
    if *slot != next {
        *slot = next;
        *dirty = true;
    }
}

pub(crate) fn sanitize_voice_id(v: &mut Option<String>, dirty: &mut bool) {
    if v.as_ref().is_some_and(|s| s.is_empty() || s.len() > 512) {
        *v = None;
        *dirty = true;
    }
}
