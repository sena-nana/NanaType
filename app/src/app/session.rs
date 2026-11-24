//! Playback session, scheduler, TTS, dictation, audio, archive, and live chat.

use std::collections::VecDeque;
use std::sync::Arc;

use nanatype_audio::AudioHandle;
use nanatype_core::Core;
use nanatype_theme::LoadedSoundPack;

use crate::chat::ChatMessage;
use crate::scheduler::Scheduler;
use crate::services::sounds::{self, Bank};

pub struct SessionController {
    pub core: Core,
    pub scheduler: Scheduler,
    pub speech: super::speech::Speech,
    pub tts: super::tts::Narrator<super::tts::Lazy>,
    pub audio: AudioHandle,
    pub bank: Bank,
    /// The chosen sound pack, layered over the skin's sounds.
    pub sound_pack: Option<LoadedSoundPack>,
    /// A chosen pack that is not installed (or does not load), already reported.
    pub sound_pack_missing: Option<String>,
    pub keys: sounds::Keys,
    pub(super) archive: super::archive::Archive,
    pub(super) live_chat: super::live::LiveChat,
    /// Live chat, oldest first, capped at the panel's size.
    pub chat: VecDeque<Arc<ChatMessage>>,
}

impl super::App {
    /// Take the chosen sound pack from the last scan; true when what plays
    /// changed. A pack that is missing or broken leaves the skin's sounds
    /// playing and is reported once; the setting stays for when it returns.
    pub(super) fn sync_sound_pack(&mut self) -> bool {
        let wanted = self
            .settings
            .sound
            .pack
            .as_deref()
            .filter(|id| !self.settings.disabled_packages.iter().any(|d| d == *id))
            .map(str::to_owned);
        let entry = wanted
            .as_deref()
            .and_then(|id| self.appearance.registry.sound_pack(id));
        let next = entry.and_then(|e| e.pack.clone());
        if let Some(id) = &wanted
            && next.is_none()
            && self.session.sound_pack_missing.as_ref() != Some(id)
        {
            let detail = entry
                .and_then(|e| e.errors.first())
                .map(|d| format!(": {d}"))
                .unwrap_or_default();
            let text = self
                .ui
                .tr("sound-pack-missing", &[("id", id.as_str().into())]);
            self.notice(format!("{text}{detail}"), true);
        }
        self.session.sound_pack_missing = wanted.filter(|_| next.is_none());
        let key =
            |p: &Option<LoadedSoundPack>| p.as_ref().map(|p| (p.id.clone(), p.sounds.clone()));
        let changed = key(&next) != key(&self.session.sound_pack);
        self.session.sound_pack = next;
        changed
    }

    pub(super) fn load_sounds(&self) {
        let skin = self.appearance.loaded.clone();
        let pack = self.session.sound_pack.clone();
        crate::plugins::background("nanatype-sounds", &self.ctx, move || {
            let (bank, errors) = sounds::load(&skin, pack.as_ref());
            crate::msg::Msg::Skin(crate::msg::SkinMsg::SoundsLoaded {
                skin: skin.skin.meta.id.clone(),
                pack: pack.map(|p| p.id),
                bank,
                errors,
            })
        });
    }

    pub(super) fn audio_levels(&self) {
        let s = &self.settings.sound;
        self.session.audio.set_master(s.master);
        self.session
            .audio
            .set_bus(nanatype_audio::Bus::Typing, s.typing_volume);
        self.session
            .audio
            .set_bus(nanatype_audio::Bus::Effects, s.effects_volume);
        self.session
            .audio
            .set_bus(nanatype_audio::Bus::Ui, s.ui_volume);
        self.session
            .audio
            .set_bus(nanatype_audio::Bus::Keys, s.keystroke_volume);
    }

    /// Motion is reduced when the setting or the system asks for it.
    pub fn sync_reduced_motion(&mut self) {
        let reduced =
            self.settings.playback.reduced_motion || self.ui.system_reduced_motion.get_untracked();
        if self.ui.reduced_motion.get_untracked() != reduced {
            self.ui.reduced_motion.set(reduced);
        }
        let fx = self.session.core.handle(
            nanatype_core::Cmd::SetReducedMotion(reduced),
            crate::clock::now_ms(),
        );
        self.apply_effects(fx);
    }

    pub fn run(&mut self, cmd: nanatype_core::Cmd) {
        self.run_command(cmd, false);
    }

    pub(super) fn run_command(&mut self, cmd: nanatype_core::Cmd, automatic: bool) {
        use nanatype_core::Cmd;
        if matches!(cmd, Cmd::Recall(_) | Cmd::SendMarkup(_)) {
            self.speech_user_edit();
        }
        if matches!(cmd, Cmd::Skip | Cmd::ClearDisplay) {
            self.session.tts.silence();
        }
        if self.peer_blocked(&cmd) {
            if matches!(cmd, Cmd::Send) {
                self.speech_user_edit();
            }
            return;
        }
        let is_send = matches!(cmd, Cmd::Send);
        let fx = match super::chat::handle_command(
            &mut self.session.core,
            cmd,
            self.draft.policy,
            crate::clock::now_ms(),
        ) {
            Ok(fx) => fx,
            Err(()) => {
                if is_send {
                    self.speech_user_edit();
                }
                self.notice(self.ui.now_t("chat-images-unavailable"), true);
                return;
            }
        };
        let sent = is_send && fx.replace_draft.as_ref().is_some_and(|d| d.doc.is_blank());
        self.apply_effects(fx);
        if is_send && !sent {
            self.speech_user_edit();
        }
        if sent {
            self.speech_sent(automatic);
            self.ui.chat_target.set(None);
            let count = self.session.core.queued();
            let feedback = if count > 0 {
                self.ui.tr("chat-queued", &[("count", count.into())])
            } else {
                format!("{} ✓", self.ui.now_t("status-sent"))
            };
            self.shell
                .toasts
                .push(crate::ui::feedback::ToastRequest::new(
                    feedback,
                    nana_ui::ToastTone::Success,
                    crate::ui::feedback::SENT_MS,
                    false,
                ));
        }
    }

    /// Click for a key pressed in the input window, when enabled.
    pub fn keystroke(&mut self, key: sounds::Keystroke) {
        if self.settings.sound.keystroke {
            let scheme = self.settings.sound.keystroke_scheme;
            self.session
                .audio
                .play(self.session.keys.voice(scheme, key));
        }
    }

    fn play_cue_now(&mut self, cue: &nanatype_playback::CueKind) {
        if let Some(v) = sounds::voice(cue, &self.appearance.skin.sounds) {
            self.session.audio.play(v);
        } else {
            self.run(nanatype_core::Cmd::CueFired(cue.clone()));
        }
    }

    pub(super) fn apply_effects(&mut self, fx: nanatype_core::Effects) {
        for epoch in &fx.cancel_epochs {
            self.session.audio.cancel(*epoch);
            self.session.scheduler.cancel(*epoch);
        }
        for wake in &fx.wakes {
            self.session.scheduler.wake(*wake);
        }
        for batch in &fx.cues {
            let mut audio = Vec::new();
            for cue in batch.cues.iter() {
                let at = batch.origin_ms + u64::from(cue.at_ms);
                match sounds::voice(&cue.kind, &self.appearance.skin.sounds) {
                    Some(voice) => audio.push(nanatype_audio::Scheduled {
                        at: crate::clock::instant(at),
                        voice,
                    }),
                    None => self
                        .session
                        .scheduler
                        .cue(batch.epoch, at, cue.kind.clone()),
                }
            }
            self.session.audio.schedule(batch.epoch, audio);
        }
        for cue in &fx.fire_now {
            self.play_cue_now(&cue.kind);
        }
        for sound in &fx.ui_sounds {
            let id = match sound {
                nanatype_core::UiSound::Open => sounds::OPEN,
                nanatype_core::UiSound::Advance => sounds::ADVANCE,
            };
            self.session.audio.play(nanatype_audio::Voice {
                sound: id.into(),
                bus: nanatype_audio::Bus::Ui,
                gain: 1.0,
                pitch: 1.0,
            });
        }
        for event in &fx.events {
            if let nanatype_core::Event::LineStarted(line) = event {
                self.tts_line(line);
            }
            self.twitch_submitted(event);
            self.peer_submitted(event);
            self.bridge_event(event);
        }
        if let Some(draft) = &fx.replace_draft {
            self.set_source(
                nanatype_markup::serialize_line(&draft.doc, &draft.meta),
                true,
            );
        }
        if fx.hide_input {
            self.shell.summoned = false;
        }
        if fx.display_changed {
            self.ui.display.set(Arc::new(self.shared_display()));
            self.ui.now.set(crate::clock::now_ms());
        }
        if fx.history_changed || fx.danmaku_changed {
            self.archive_rows();
        }
        if fx.history_changed && self.peer.handle.is_none() {
            self.ui.history.set(Arc::new(
                self.session.core.history().iter().cloned().collect(),
            ));
        }
        if fx.danmaku_changed {
            self.ui.danmaku.set(Arc::new(
                self.session.core.danmaku().iter().cloned().collect(),
            ));
        }
        let queued = self.session.core.queued();
        if self.ui.queued.get_untracked() != queued {
            self.ui.queued.set(queued);
        }
        if fx.reply_changed {
            self.ui
                .reply
                .set(self.session.core.pending_reply().cloned());
        }
        if fx.display_changed || fx.history_changed || fx.danmaku_changed {
            self.refresh_atom_urls();
        }
        if fx.display_changed || fx.history_changed {
            self.peer_publish();
            if self.peer.handle.is_some() {
                self.peer_render();
            }
        }
    }
}
