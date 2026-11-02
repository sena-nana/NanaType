//! Application-owned dictation policy. Both timer and OS callbacks cross this boundary.
use super::App;
use crate::msg::{Msg, SpeechMsg};
use nanatype_speech::{
    Backend, Event, Inventory, Service,
    controller::{Controller, Phase},
    model::{self, ModelState},
};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub phase: Phase,
    pub partial: String,
    pub remaining: u32,
    pub backend: Option<Backend>,
    pub error: Option<nanatype_speech::Error>,
    pub model: ModelState,
    pub inventory: Inventory,
}
impl State {
    pub fn status(&self, ui: crate::ui::Ui) -> String {
        if let Some(error) = &self.error {
            let label = ui.now_t(error.key);
            return if error.detail.is_empty() {
                label
            } else {
                format!("{label}: {}", error.detail)
            };
        }
        let status = match self.phase {
            Phase::Off => ui.now_t("speech-off"),
            Phase::Starting => ui.now_t("speech-starting"),
            Phase::Listening => ui.now_t("speech-listening"),
            Phase::Recognizing => ui.now_t("speech-recognizing"),
            Phase::Countdown => ui.tr("speech-countdown", &[("seconds", self.remaining.into())]),
            Phase::Paused => ui.now_t("speech-paused"),
            Phase::Error => ui.now_t("speech-system-error"),
        };
        if let Some(backend) = self.backend {
            format!(
                "{status} · {}",
                ui.now_t(match backend {
                    Backend::System => "speech-backend-system",
                    Backend::Local => "speech-backend-local",
                })
            )
        } else {
            status
        }
    }
}
pub struct Speech {
    pub controller: Controller,
    pub service: Service,
    pub(super) state: State,
    model_cancel: Option<Arc<AtomicBool>>,
}
impl Speech {
    pub fn new(directory: PathBuf, post: impl Fn(Msg) + Send + Sync + 'static) -> Self {
        let state = State {
            model: if model::path(&directory).is_file() {
                ModelState::Ready
            } else {
                ModelState::Missing
            },
            ..Default::default()
        };
        Self {
            controller: Controller::default(),
            service: Service::new(directory, move |g, e| {
                post(Msg::Speech(SpeechMsg::Event(g, e)))
            }),
            state,
            model_cancel: None,
        }
    }
}
impl Drop for Speech {
    fn drop(&mut self) {
        self.service.stop();
        if let Some(cancel) = &self.model_cancel {
            cancel.store(true, Ordering::Release);
        }
    }
}
impl App {
    pub(crate) fn publish_speech(&mut self) {
        let c = &self.speech.controller;
        self.speech.state.phase = c.phase;
        self.speech.state.partial.clone_from(&c.partial);
        self.speech.state.remaining = c.remaining;
        if self.ui.speech.with_untracked(|s| **s != self.speech.state) {
            self.ui.speech.set(Arc::new(self.speech.state.clone()));
            self.sync_tray();
        }
    }
    fn start_speech(&mut self) {
        if self.ui.composing.get_untracked() {
            self.speech.state.error = Some(nanatype_speech::Error::key("speech-finish-edit"));
            self.publish_speech();
            return;
        }
        let generation = self.speech.controller.start(!self.core.draft().is_blank());
        self.ui.speech_edit_pending.set(false);
        self.speech.state.error = None;
        self.speech.state.backend = None;
        let language = self
            .settings
            .speech
            .language
            .clone()
            .unwrap_or_else(|| self.ui.locale.get_untracked().tag().to_string());
        self.speech
            .service
            .start(self.settings.speech.clone(), language, generation);
        self.schedule_speech_tick(generation);
        self.publish_speech();
    }
    fn schedule_speech_tick(&self, generation: u64) {
        self.scheduler.post_after(
            Duration::from_millis(250),
            Msg::Speech(SpeechMsg::Tick(generation, self.speech.controller.revision)),
        );
    }
    /// Called synchronously by input routing as well as semantic edit messages.
    pub(crate) fn speech_user_edit(&mut self) {
        if self.speech.controller.active()
            && (self.speech.controller.running() || !self.speech.service.stopped())
        {
            self.speech.service.stop();
            self.speech.controller.pause();
            self.speech.state.error = None;
            self.publish_speech();
        }
        self.ui.speech_edit_pending.set(false);
    }
    pub(crate) fn speech_sent(&mut self, automatic: bool) {
        if !self.speech.controller.active() {
            return;
        }
        // Auto-send already established that the preceding utterances were final.
        // Any audio arriving while Core submits belongs to the next draft.
        let pending = if automatic {
            0
        } else {
            self.speech.service.activity().pending
        };
        if !self.speech.service.stopped()
            && !self.ui.speech_edit_pending.get_untracked()
            && self.speech.controller.continue_after_send(pending)
        {
            self.schedule_speech_tick(self.speech.controller.generation);
            self.publish_speech();
        } else {
            self.start_speech();
        }
    }
    pub(crate) fn speech_inventory(&self) {
        crate::plugins::background("nanatype-speech-devices", &self.ctx, || {
            Msg::Speech(SpeechMsg::Inventory(nanatype_speech::inventory()))
        });
    }
    pub(crate) fn install_speech_model(&mut self, source: Option<PathBuf>) {
        // A cancelled operation keeps this slot until it finishes removing its staging file.
        if self.speech.model_cancel.is_some() {
            return;
        }
        self.speech_user_edit();
        let cancel = Arc::new(AtomicBool::new(false));
        self.speech.model_cancel = Some(cancel.clone());
        self.speech.state.model = ModelState::Working(0);
        let directory = self.speech.service.directory().to_owned();
        let progress = self.ctx.clone();
        if let Err(e) =
            crate::plugins::try_background("nanatype-speech-model", &self.ctx, move || {
                let result = model::install(&directory, source.as_deref(), &cancel, |percent| {
                    progress.dispatch(Msg::Speech(SpeechMsg::ModelProgress(percent)));
                });
                Msg::Speech(SpeechMsg::ModelDone(result))
            })
        {
            self.ctx
                .dispatch(Msg::Speech(SpeechMsg::ModelDone(Err(e.to_string()))));
        }
        self.publish_speech();
    }
    pub(super) fn speech_msg(&mut self, msg: SpeechMsg) {
        match msg {
            SpeechMsg::Toggle => {
                if self.speech.controller.active() {
                    self.speech.service.stop();
                    self.speech.controller.stop();
                    self.speech.state.error = None;
                } else {
                    self.start_speech();
                }
            }
            SpeechMsg::Resume => self.start_speech(),
            SpeechMsg::Edit => self.speech_user_edit(),
            SpeechMsg::Refresh => self.speech_inventory(),
            SpeechMsg::Inventory(inventory) => self.speech.state.inventory = inventory,
            SpeechMsg::Event(generation, event) => {
                if generation != self.speech.controller.generation
                    || !self.speech.controller.running()
                {
                    return;
                }
                // An editor event has already arrived but its program message is still queued.
                if self.ui.speech_edit_pending.get_untracked() || self.ui.composing.get_untracked()
                {
                    self.speech_user_edit();
                    return;
                }
                match event {
                    Event::Ready(backend) => {
                        self.speech.state.backend = Some(backend);
                        self.speech.controller.phase = Phase::Listening;
                    }
                    Event::Pending(id) => self.speech.controller.pending(id),
                    Event::Partial(id, text) => {
                        let _ = id;
                        self.speech.controller.partial = text;
                    }
                    Event::Final(id, text) => {
                        for text in self.speech.controller.final_text(id, text) {
                            let doc = append_plain(self.core.draft(), &text);
                            self.set_source(
                                nanatype_markup::serialize_line(&doc, &self.core.draft_meta()),
                                true,
                            );
                        }
                    }
                    Event::Error(error) => {
                        if error.key == "speech-model-invalid" {
                            self.speech.state.model =
                                ModelState::Error("speech-model-invalid".into());
                        }
                        self.speech.service.stop();
                        self.speech.controller.fail();
                        self.speech.state.error = Some(error);
                    }
                }
            }
            SpeechMsg::Tick(generation, revision) => {
                if generation != self.speech.controller.generation
                    || revision != self.speech.controller.revision
                    || !self.speech.controller.running()
                {
                    return;
                }
                if self.ui.speech_edit_pending.get_untracked() || self.ui.composing.get_untracked()
                {
                    self.speech_user_edit();
                    return;
                }
                // A worker failure invalidates auto-send before its error message reaches UI.
                if self.speech.service.stopped() {
                    return;
                }
                let send = self.speech.controller.tick(
                    self.settings.speech.mode,
                    self.settings.speech.silence_seconds,
                    Instant::now(),
                    self.speech.service.activity(),
                );
                if send {
                    // run() performs all existing synchronization/peer validation. A successful
                    // send advances the draft revision while keeping idle capture alive.
                    self.run_command(nanatype_core::Cmd::Send, true);
                    if self.speech.controller.generation == generation
                        && self.speech.controller.revision == revision
                    {
                        self.speech.service.stop();
                        self.speech.controller.pause();
                    }
                } else {
                    self.schedule_speech_tick(generation);
                }
            }
            SpeechMsg::ModelDownload => self.install_speech_model(None),
            SpeechMsg::ModelCancel => {
                if let Some(cancel) = &self.speech.model_cancel {
                    cancel.store(true, Ordering::Release);
                }
            }
            SpeechMsg::ModelDelete => {
                if self.speech.model_cancel.is_some() {
                    return;
                }
                self.speech_user_edit();
                self.speech.state.model =
                    match std::fs::remove_file(self.speech.service.model_path()) {
                        Ok(()) => ModelState::Missing,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ModelState::Missing,
                        Err(e) => ModelState::Error(e.to_string()),
                    };
            }
            SpeechMsg::ModelProgress(percent) => {
                self.speech.state.model = ModelState::Working(percent);
            }
            SpeechMsg::ModelDone(result) => {
                self.speech.model_cancel = None;
                self.speech.state.model = match result {
                    Ok(()) => ModelState::Ready,
                    Err(e) if e == "speech-cancelled" => {
                        if self.speech.service.model_path().is_file() {
                            ModelState::Ready
                        } else {
                            ModelState::Missing
                        }
                    }
                    Err(e) => ModelState::Error(e),
                };
            }
        }
        self.publish_speech();
    }
}

/// Append document nodes, never parse the recognizer's text as markup.
fn append_plain(draft: &nanatype_model::Document, text: &str) -> nanatype_model::Document {
    let mut doc = draft.clone();
    let text = text.trim();
    if text.is_empty() {
        return doc;
    }
    let previous = doc.plain_text();
    let spacing = previous
        .chars()
        .last()
        .zip(text.chars().next())
        .is_some_and(|(a, b)| a.is_ascii_alphanumeric() && b.is_ascii_alphanumeric());
    let fragment =
        nanatype_model::Document::plain(&format!("{}{text}", if spacing { " " } else { "" }));
    let mut paragraphs = fragment.paragraphs.into_iter();
    if let Some(first) = paragraphs.next() {
        if let Some(last) = doc.paragraphs.last_mut() {
            last.inlines.extend(first.inlines);
        } else {
            doc.paragraphs.push(first);
        }
    }
    doc.paragraphs.extend(paragraphs);
    doc.normalize();
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dictated_markup_is_literal_and_existing_styles_survive() {
        let draft = nanatype_markup::parse("[b]hello[/b]").doc;
        let doc = append_plain(&draft, "[pause][img=secret]");
        assert_eq!(doc.plain_text(), "hello[pause][img=secret]");
        let source = nanatype_markup::serialize(&doc);
        assert!(source.contains("[b]hello[/b]"));
        assert_eq!(nanatype_markup::parse(&source).doc, doc);
    }
    #[test]
    fn multilingual_append_and_newlines_preserve_text() {
        assert_eq!(
            append_plain(&nanatype_model::Document::plain("hello"), "world").plain_text(),
            "hello world"
        );
        assert_eq!(
            append_plain(&nanatype_model::Document::plain("你好"), "世界\nこんにちは").plain_text(),
            "你好世界\nこんにちは"
        );
    }
}
