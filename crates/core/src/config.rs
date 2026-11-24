use std::collections::BTreeSet;

use nanatype_model::{Document, Entrance, EntranceKind, LineMeta, Overflow, Speaker};
use nanatype_playback::{ExitStyle, PacingParams, PlaybackProfile, TypingSoundRule};
use nanatype_settings::{
    Character, HistorySync, InputMode, InputPresence, PlaybackSettings, Settings,
};
use nanatype_theme::Skin;

/// Click variants the host provides when a skin has no typing sounds.
pub const DEFAULT_TYPING_VARIANTS: u8 = 3;

/// Narrow projection of [`Settings`] containing only the fields [`crate::Core`] observes.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreConfig {
    pub input_mode: InputMode,
    pub input_presence: InputPresence,
    pub characters: Vec<Character>,
    pub active_character: usize,
    pub playback: PlaybackSettings,
    pub typing_sound: bool,
    pub advanced_effects: bool,
    pub history_cap: usize,
    pub history_sync: HistorySync,
    pub separate_danmaku: bool,
}

impl From<&Settings> for CoreConfig {
    fn from(s: &Settings) -> Self {
        Self {
            input_mode: s.general.input_mode,
            input_presence: s.general.input_presence,
            characters: s.general.characters.clone(),
            active_character: s.general.character,
            playback: s.playback.clone(),
            typing_sound: s.sound.typing,
            advanced_effects: s.skin.advanced_effects,
            history_cap: s.history.max_items.max(1) as usize,
            history_sync: s.history.sync,
            separate_danmaku: s.history.chat.separate,
        }
    }
}

impl CoreConfig {
    pub fn active_character(&self) -> Character {
        self.characters
            .get(self.active_character)
            .or(self.characters.first())
            .cloned()
            .unwrap_or_default()
    }

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

    pub(crate) fn active_speaker(&self, portrait: Option<String>) -> Speaker {
        Speaker {
            portrait,
            ..self.active_character().speaker()
        }
    }

    pub(crate) fn speaker_for(
        &self,
        meta: &LineMeta,
        override_speaker: Option<&Speaker>,
        portrait: Option<String>,
    ) -> Speaker {
        if let Some(character) = meta
            .speaker
            .as_deref()
            .and_then(|k| self.character_named(k))
        {
            return Speaker {
                portrait,
                ..character.speaker()
            };
        }
        if let Some(speaker) = override_speaker {
            let mut speaker = speaker.clone();
            speaker.portrait = portrait;
            return speaker;
        }
        self.active_speaker(portrait)
    }

    pub(crate) fn pacing(&self, skin: &SkinCapabilities) -> PacingParams {
        PacingParams {
            punctuation_pauses: self.playback.punctuation_pauses,
            speed_scale: self.playback.speed_scale,
            typing: self.playback.typing,
            ..skin.pacing
        }
    }

    pub(crate) fn sound_rule(&self, skin: &SkinCapabilities) -> TypingSoundRule {
        TypingSoundRule {
            enabled: self.typing_sound,
            ..skin.typing_sound
        }
    }

    pub(crate) fn entrance_for(&self, doc: &Document, skin: &SkinCapabilities) -> Entrance {
        if let Some(entrance) = doc.entrance() {
            return entrance;
        }
        let kind = self.playback.entrance.unwrap_or(skin.default_entrance.kind);
        Entrance {
            kind,
            ms: if kind == EntranceKind::None {
                0
            } else {
                skin.default_entrance.ms.max(1)
            },
        }
    }

    pub(crate) fn transitions_enabled(&self, system_reduced: bool) -> bool {
        self.advanced_effects && !self.playback.reduced_motion && !system_reduced
    }

    pub(crate) fn intro_ms(
        &self,
        doc: &Document,
        entrance: Entrance,
        skin: &SkinCapabilities,
        system_reduced: bool,
    ) -> u32 {
        if self.playback.reduced_motion || system_reduced {
            return 0;
        }
        let mut ms = u32::from(entrance.ms);
        if self.transitions_enabled(system_reduced)
            && self.playback.entrance.is_none()
            && doc.entrance().is_none()
            && let Some(scramble_ms) = skin.scramble_intro_ms
        {
            // Substitution, then the glyph's brief flash (text_scramble.rs).
            ms = ms.max(u32::from(scramble_ms) + 45);
        }
        ms
    }

    pub(crate) fn exit_style(&self, skin: &SkinCapabilities, system_reduced: bool) -> ExitStyle {
        if self.transitions_enabled(system_reduced)
            && let Some(ms) = skin.exit_ms
            && ms > 0
        {
            ExitStyle::Presentation { ms }
        } else {
            ExitStyle::Instant
        }
    }

    pub(crate) fn playback_profile(
        &self,
        meta: &LineMeta,
        doc: &Document,
        entrance: Entrance,
        skin: &SkinCapabilities,
        system_reduced: bool,
    ) -> PlaybackProfile {
        PlaybackProfile {
            intro_ms: self.intro_ms(doc, entrance, skin, system_reduced),
            exit: self.exit_style(skin, system_reduced),
            after: meta.after.unwrap_or(self.playback.after),
            auto_advance_ms: meta.auto_advance_ms,
        }
    }
}

/// Narrow projection of [`Skin`] containing only the capabilities [`crate::Core`] observes.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinCapabilities {
    pub pacing: PacingParams,
    pub typing_sound: TypingSoundRule,
    pub has_open_sound: bool,
    pub has_advance_sound: bool,
    pub portrait_variants: BTreeSet<String>,
    pub overflow: Overflow,
    pub overflow_auto_ms: Option<u32>,
    pub default_entrance: Entrance,
    pub exit_ms: Option<u16>,
    pub scramble_intro_ms: Option<u16>,
}

impl From<&Skin> for SkinCapabilities {
    fn from(skin: &Skin) -> Self {
        let mut typing_sound = skin.sounds.typing.rule(true);
        if skin.sounds.typing.files.is_empty() {
            typing_sound.variants = DEFAULT_TYPING_VARIANTS;
        }
        let r = &skin.dialogue.reveal;
        let exit_ms = r.scramble.as_ref().map(|s| s.exit_ms).or_else(|| {
            r.exit
                .as_ref()
                .filter(|e| e.ms > 0)
                .filter(|e| {
                    e.material
                        .as_ref()
                        .is_some_and(|m| skin.material_programs.contains_key(m))
                })
                .map(|e| e.ms)
        });
        Self {
            pacing: skin.pacing(1.0, true, true),
            typing_sound,
            has_open_sound: skin.sounds.open.is_some(),
            has_advance_sound: skin.sounds.advance.is_some(),
            portrait_variants: skin.dialogue.portrait.variants.keys().cloned().collect(),
            overflow: skin.dialogue.overflow,
            overflow_auto_ms: skin.dialogue.overflow_auto_ms,
            default_entrance: Entrance {
                kind: r.entrance.kind,
                ms: r.entrance.ms,
            },
            exit_ms,
            scramble_intro_ms: r.scramble.as_ref().map(|s| s.entrance_ms),
        }
    }
}
