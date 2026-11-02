//! Translated subtitles: which lines are sent, where the answers go (the
//! display, the history, saved history, the bridge and partners' lanes),
//! and the providers' keys in the system's secret store. Failures show on
//! the settings badge and once as a notice; sending never waits on them.
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use nanatype_bridge::BridgeEvent;
use nanatype_core::Cmd;
use nanatype_model::{HistoryItem, Line, LineId, unix_millis};
use nanatype_settings::{TranslateProvider, TranslateSettings};
use nanatype_translate::{Config, Error, Outcome, Provider, Reply, Request, Translator};

use super::App;
use super::secret::LazySecret;
use crate::msg::{Msg, TranslateMsg};
use crate::ui::{EditShared, LinkState};

/// A newer line in a lane drops the ones still waiting in it.
const OWN: u8 = 0;
const PARTNER: u8 = 1;

/// Partner lines older than this when they first show (a room just
/// joined) are not translated.
const PARTNER_FRESH_MS: u64 = 60_000;
/// Partner translations kept for their lanes and rows.
const PARTNER_KEEP: usize = 256;

/// Where an answer goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Line(LineId),
    /// A history row by key (live chat).
    Row(u64),
    Partner(String, LineId),
    Test(String),
}

/// What the settings page shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// A key is kept in the secret store for OpenAI-compatible endpoints
    /// and for DeepL.
    pub openai_key: bool,
    pub deepl_key: bool,
    pub testing: bool,
    /// The last test: the sample and its translation, or why it failed.
    pub test: Option<Result<(String, String), String>>,
}

pub struct Translate {
    /// Started the first time translation is switched on.
    worker: Option<Translator>,
    post: Arc<dyn Fn(Msg) + Send + Sync>,
    openai_key: LazySecret,
    deepl_key: LazySecret,
    /// Requests sent, by id, with the text they asked about.
    pending: HashMap<u64, (Target, String)>,
    next: u64,
    /// The last failure, until something is translated again.
    failing: Option<Error>,
    /// Partner lines asked about.
    asked: HashSet<(String, LineId)>,
    /// Partner lines translated here, and when the answer came.
    partners: HashMap<(String, LineId), (Arc<str>, u64)>,
}

impl Translate {
    pub fn new(config_dir: &Path, post: impl Fn(Msg) + Send + Sync + 'static) -> Self {
        let secret = |name| {
            LazySecret::new(
                config_dir,
                name,
                &crate::diag::TRANSLATE_SECRET_FAILED,
                true,
            )
        };
        Self {
            worker: None,
            post: Arc::new(post),
            openai_key: secret("translate-openai"),
            deepl_key: secret("translate-deepl"),
            pending: HashMap::new(),
            next: 1,
            failing: None,
            asked: HashSet::new(),
            partners: HashMap::new(),
        }
    }

    fn key(&mut self, provider: TranslateProvider) -> &mut LazySecret {
        match provider {
            TranslateProvider::Openai => &mut self.openai_key,
            TranslateProvider::Deepl => &mut self.deepl_key,
        }
    }

    /// The provider `s` describes, reading its key once it is wanted.
    fn config(&mut self, s: &TranslateSettings) -> Option<Config> {
        if !s.enabled {
            return None;
        }
        let provider = match s.provider {
            TranslateProvider::Openai => Provider::Chat {
                base_url: s.base_url.clone(),
                model: s.model.clone(),
                key: self.openai_key.get(),
            },
            TranslateProvider::Deepl => Provider::DeepL {
                key: self.deepl_key.get().unwrap_or_default(),
            },
        };
        let proxy = s.proxy.trim();
        Some(Config {
            proxy: (!proxy.is_empty()).then(|| proxy.to_owned()),
            ..Config::new(provider)
        })
    }

    fn worker(&mut self) -> &mut Translator {
        self.worker.get_or_insert_with(|| {
            let post = self.post.clone();
            Translator::new(move |reply| post(Msg::Translate(TranslateMsg::Reply(reply))))
        })
    }

    /// Send `text` for `target`; `false` when nothing will be answered.
    fn ask(&mut self, target: Target, text: String, speaker: &str, s: &TranslateSettings) -> bool {
        let Some(worker) = self.worker.as_ref().filter(|w| w.configured()) else {
            return false;
        };
        let id = self.next;
        self.next += 1;
        let lane = match &target {
            Target::Line(_) => Some(OWN),
            Target::Partner(..) => Some(PARTNER),
            Target::Row(_) | Target::Test(_) => None,
        };
        // A cached answer comes back through `post`, after this returns.
        self.pending.insert(id, (target, text.clone()));
        let sent = worker.request(Request {
            id,
            text,
            speaker: speaker.to_owned(),
            target: s.target.clone(),
            lane,
        });
        if !sent {
            self.pending.remove(&id);
        }
        sent
    }

    /// A known translation of `line`, for its `line_start` event.
    fn cached(&self, s: &TranslateSettings, line: &Line) -> Option<String> {
        if !(s.enabled && s.own) {
            return None;
        }
        let text = nanatype_tts::speakable(&line.doc)?;
        self.worker
            .as_ref()?
            .cached(&text, &s.target)
            .filter(|t| t.trim() != text.trim())
    }
}

/// The sample line the test translates, in the interface's language.
fn sample(app: &App) -> String {
    app.ui.now_t("translate-sample")
}

impl App {
    /// Follow the settings: start, reconfigure or stop the worker.
    pub(super) fn translate_configure(&mut self) {
        let s = self.settings.translate.clone();
        let config = self.translate.config(&s);
        if config.is_some() || self.translate.worker.is_some() {
            self.translate.worker().configure(config);
        }
        if !s.enabled {
            self.translate.pending.clear();
            self.translate.failing = None;
        }
        let state = if s.enabled {
            LinkState::Connected(self.ui.now_t("translate-status-ready"))
        } else {
            LinkState::Off
        };
        if self.translate.failing.is_none() {
            self.edit_status(|st| st.translate = state);
        }
        let (openai, deepl) = (
            self.translate.openai_key.held(),
            self.translate.deepl_key.held(),
        );
        self.ui.translate.edit(|st| {
            st.openai_key = openai;
            st.deepl_key = deepl;
        });
    }

    /// The display started `line`: translate it, newest first.
    pub(super) fn translate_line(&mut self, line: &Line) {
        let s = &self.settings.translate;
        if !(s.enabled && s.own) {
            return;
        }
        let Some(text) = nanatype_tts::speakable(&line.doc) else {
            return;
        };
        let s = s.clone();
        self.translate
            .ask(Target::Line(line.id), text, &line.speaker.name, &s);
    }

    /// The known translation for a `line_start` event.
    pub(super) fn translate_cached(&self, line: &Line) -> Option<String> {
        self.translate.cached(&self.settings.translate, line)
    }

    /// A viewer's message joined the history as row `key`.
    pub(super) fn translate_chat(&mut self, key: u64, item: &HistoryItem) {
        let s = &self.settings.translate;
        if !(s.enabled && s.chat) {
            return;
        }
        let Some(text) = nanatype_tts::speakable(&item.doc) else {
            return;
        };
        let s = s.clone();
        self.translate.ask(Target::Row(key), text, &item.author, &s);
    }

    /// A partner's line showed. Their own translation, when they send one,
    /// is used as it is; otherwise it is translated here once.
    pub(crate) fn translate_partner(
        &mut self,
        owner: &str,
        line: LineId,
        item: &HistoryItem,
        theirs: bool,
    ) {
        let s = &self.settings.translate;
        if !(s.enabled && s.partners) || theirs {
            return;
        }
        if unix_millis().saturating_sub(item.at) > PARTNER_FRESH_MS
            || !self.translate.asked.insert((owner.to_owned(), line))
        {
            return;
        }
        let Some(text) = nanatype_tts::speakable(&item.doc) else {
            return;
        };
        let s = s.clone();
        self.translate.ask(
            Target::Partner(owner.to_owned(), line),
            text,
            &item.author,
            &s,
        );
    }

    /// The translation of a partner's line made here, and when it came.
    pub(crate) fn translate_partner_text(
        &self,
        owner: &str,
        line: LineId,
    ) -> Option<(Arc<str>, u64)> {
        self.translate
            .partners
            .get(&(owner.to_owned(), line))
            .cloned()
    }

    pub(super) fn translate_msg(&mut self, msg: TranslateMsg) {
        match msg {
            TranslateMsg::Reply(reply) => self.translate_reply(reply),
            TranslateMsg::Key(provider, key) => {
                if let Err(e) = self.translate.key(provider).set(&key) {
                    nanatype_diag::report!(crate::diag::TRANSLATE_SECRET_FAILED, op = 1u64; "{e}");
                    let text = self.ui.now_t("translate-key-not-stored");
                    self.notice(text, true);
                }
                let saved = self.translate.key(provider).held();
                self.ui.translate.edit(|st| match provider {
                    TranslateProvider::Openai => st.openai_key = saved,
                    TranslateProvider::Deepl => st.deepl_key = saved,
                });
                self.translate.failing = None;
                self.translate_configure();
            }
            TranslateMsg::Test => {
                let s = self.settings.translate.clone();
                // The test works before translation is switched on.
                let config = self.translate.config(&TranslateSettings {
                    enabled: true,
                    ..s.clone()
                });
                self.translate.worker().configure(config);
                let text = sample(self);
                self.ui.translate.edit(|st| {
                    st.testing = true;
                    st.test = None;
                });
                let speaker = self.settings.general.active_character().name;
                if !self
                    .translate
                    .ask(Target::Test(text.clone()), text, &speaker, &s)
                {
                    let reason = self.ui.now_t("translate-error-config");
                    self.ui.translate.edit(|st| {
                        st.testing = false;
                        st.test = Some(Err(reason));
                    });
                    if !s.enabled {
                        self.translate.worker().configure(None);
                    }
                }
            }
        }
    }

    fn translate_reply(&mut self, reply: Reply) {
        let Some((target, source)) = self.translate.pending.remove(&reply.id) else {
            return;
        };
        let test = matches!(target, Target::Test(_));
        if test && !self.settings.translate.enabled {
            self.translate.worker().configure(None);
        }
        let text = match reply.outcome {
            Outcome::Superseded => {
                if test {
                    self.ui.translate.edit(|st| st.testing = false);
                }
                return;
            }
            Outcome::Failed(e) => return self.translate_failed(target, e),
            Outcome::Done(text) => text,
        };
        if self.translate.failing.take().is_some() || !reply.cached {
            let ok = self.ui.now_t("translate-status-ok");
            self.edit_status(|st| st.translate = LinkState::Connected(ok));
        }
        // Already in the target language: nothing to add.
        let same = text.trim() == source.trim();
        let text: Arc<str> = text.into();
        match target {
            Target::Test(sample) => self.ui.translate.edit(|st| {
                st.testing = false;
                st.test = Some(Ok((sample, text.to_string())));
            }),
            _ if same => {}
            Target::Line(line) => {
                self.archive_translation(Some(line), None, &text);
                self.run(Cmd::Translated {
                    line,
                    text: text.clone(),
                });
                self.bridge_translation(line, &text);
            }
            Target::Row(key) => {
                self.archive_translation(None, Some(key), &text);
                self.run(Cmd::TranslatedRow { key, text });
            }
            Target::Partner(owner, line) => {
                let partners = &mut self.translate.partners;
                if partners.len() >= PARTNER_KEEP {
                    partners.clear();
                }
                partners.insert((owner, line), (text, crate::clock::now_ms()));
                self.peer_render();
            }
        }
    }

    fn translate_failed(&mut self, target: Target, error: Error) {
        let text = match error {
            Error::NoKey => self.ui.now_t("translate-error-no-key"),
            Error::Unauthorized => self.ui.now_t("translate-error-unauthorized"),
            Error::Limited => self.ui.now_t("translate-error-limited"),
            Error::Status(code) => self.ui.tr(
                "translate-error-status",
                &[("code", u64::from(code).into())],
            ),
            Error::Network => self.ui.now_t("translate-error-network"),
            Error::Timeout => self.ui.now_t("translate-error-timeout"),
            Error::Proxy => self.ui.now_t("translate-error-proxy"),
            Error::Parse | Error::Empty => self.ui.now_t("translate-error-answer"),
        };
        if matches!(target, Target::Test(_)) {
            self.ui.translate.edit(|st| {
                st.testing = false;
                st.test = Some(Err(text.clone()));
            });
        }
        if self.translate.failing.replace(error) != Some(error) {
            let status = match error {
                Error::Status(code) => u64::from(code),
                _ => 0,
            };
            nanatype_diag::event!(
                crate::diag::TRANSLATE_FAILED,
                kind = error.code(),
                status = status
            );
            // Once per kind of failure, not for every line.
            if !matches!(target, Target::Test(_)) {
                let notice = self
                    .ui
                    .tr("translate-failed", &[("reason", text.clone().into())]);
                self.notice(notice, false);
            }
        }
        self.edit_status(|st| st.translate = LinkState::Error(text));
    }

    /// Tell the bridge and plugins a line's translation.
    fn bridge_translation(&mut self, line: LineId, text: &str) {
        let out = BridgeEvent::Translation {
            id: line.0,
            text: text.to_owned(),
            target: self.settings.translate.target.clone(),
        };
        if self.plugins.has_running()
            && let Ok(event) = serde_json::to_value(&out)
        {
            self.plugins.broadcast(event, self.ui);
        }
        self.bridge.emit(out);
    }
}
