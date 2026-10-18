use std::collections::VecDeque;
use std::sync::Arc;

use nanatype_model::{
    ChatRef, Document, Entrance, EntranceKind, HistoryItem, Line, LineId, LineMeta, Overflow,
    SourceId, SourceKind, Speaker, VtsAction, unix_millis,
};
use nanatype_playback::{
    Cue, CueKind, LineQueue, PacingParams, Phase, Player, PlayerEffect, QueueDecision, Timeline,
    TypingSoundRule, WakeReason, compile, mirror_delta, mirror_offsets,
};
use nanatype_settings::{HistorySync, InputMode, InputPresence, Settings};
use nanatype_theme::Skin;

use crate::Millis;
use crate::view::{DisplayView, HistoryRow, Reveal, SegmentReveal, ShakeView};

/// Lines kept for Ctrl+↑ recall.
const RECALL_CAP: usize = 100;

/// Click variants the host provides when a skin has no typing sounds.
pub const DEFAULT_TYPING_VARIANTS: u8 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// The editor's content changed.
    DraftChanged(Document),
    /// The markup source's `[@...]` header, sent along with the next draft.
    DraftMeta(LineMeta),
    /// The box is full at these units of `line` (page overflow): turn pages
    /// there. Measured by the host from the laid out text.
    PageBreaks {
        line: LineId,
        breaks: Vec<u32>,
    },
    /// Send the draft.
    Send,
    /// Send markup directly (quick phrase), leaving the draft alone.
    SendMarkup(String),
    /// The continue key: finish the current segment, pass a pause, or move
    /// to the next queued line.
    Continue,
    /// Reveal the whole line at once.
    Skip,
    ClearDisplay,
    /// Step through sent lines: `-1` older, `+1` newer.
    Recall(i32),
    /// A scheduled wake arrived.
    Wake {
        epoch: u64,
        reason: WakeReason,
    },
    /// A timed cue reached its moment (portrait, shake, expression, speaking).
    CueFired(CueKind),
    SetSettings(Box<Settings>),
    /// Effective accessibility preference, including the operating system.
    SetReducedMotion(bool),
    SetSkin(Arc<Skin>),
    SetSpeaker(Option<Speaker>),
    RemoveHistoryRows(Vec<u64>),
    /// A row from another message source (live chat).
    External(HistoryItem),
    ClearHistory,
    /// Answer a chat message with the next line sent from the editor;
    /// `None` drops the pending reply.
    SetReply(Option<ChatRef>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wake {
    pub epoch: u64,
    pub at_ms: Millis,
    pub reason: WakeReason,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CueBatch {
    pub epoch: u64,
    pub origin_ms: Millis,
    pub cues: Arc<[Cue]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSound {
    Open,
    Advance,
}

/// Things other programs may want to know (bridge, status bar).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Accepted from the local editor or a quick phrase, even if queued.
    LineSubmitted(Line),
    LineStarted(Line),
    LineRevealed(LineId),
    LineFinished(LineId),
    Expression {
        name: String,
        intensity: Option<f32>,
    },
    Portrait(String),
    Speaking(bool),
    Shake {
        amp: f32,
        ms: u32,
    },
    Vts(VtsAction),
    Queued {
        position: usize,
    },
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Effects {
    pub wakes: Vec<Wake>,
    /// Cues to run at `origin_ms + cue.at_ms`.
    pub cues: Vec<CueBatch>,
    /// Cues to run immediately (kept through a skip).
    pub fire_now: Vec<Cue>,
    /// Scheduled cues of these epochs must not run any more.
    pub cancel_epochs: Vec<u64>,
    pub events: Vec<Event>,
    pub ui_sounds: Vec<UiSound>,
    /// Replace the editor content.
    pub replace_draft: Option<Document>,
    pub hide_input: bool,
    pub display_changed: bool,
    pub history_changed: bool,
    /// The pending reply changed (set, dropped, or used by a send).
    pub reply_changed: bool,
}

struct Active {
    line: Line,
    player: Player,
    /// Player generation; makes epochs unique across lines.
    generation: u64,
}

#[derive(Clone, Default)]
struct Draft {
    doc: Document,
    meta: LineMeta,
}

enum AfterExit {
    Clear,
    Line(Line),
    Advance,
    Wake { epoch: u64, reason: WakeReason },
    Finish(LineId),
}
struct Retiring {
    epoch: u64,
    next: AfterExit,
}

pub struct Core {
    retiring: Option<Retiring>,
    retirement_serial: u64,
    bypass_exit: bool,
    reduced_motion: bool,
    settings: Settings,
    skin: Arc<Skin>,
    pacing: PacingParams,
    sound_rule: TypingSoundRule,
    draft: Document,
    active: Option<Active>,
    generation: u64,
    /// Epoch of the last scheduled cue batch (for cancellation).
    cue_epoch: Option<u64>,
    queue: LineQueue,
    display: DisplayView,
    history: VecDeque<HistoryRow>,
    next_row: u64,
    sent: VecDeque<Draft>,
    recall: Option<usize>,
    /// Draft stashed when recall started.
    stash: Option<Draft>,
    /// Mirror mode: the committed line stays until typing starts again.
    mirror_committed: bool,
    /// The last speaking cue fired was a start (avatars' mouths are moving).
    speaking: bool,
    /// Line meta typed in the markup source, for the next send.
    draft_meta: LineMeta,
    /// The chat message the next sent line answers.
    reply: Option<ChatRef>,
    speaker_override: Option<Speaker>,
}

fn global(generation: u64, epoch: u64) -> u64 {
    (generation << 32) | (epoch & 0xFFFF_FFFF)
}

impl Core {
    pub fn new(settings: Settings, skin: Arc<Skin>) -> Self {
        let mut core = Self {
            retiring: None,
            retirement_serial: 0,
            bypass_exit: false,
            reduced_motion: false,
            pacing: PacingParams::default(),
            sound_rule: TypingSoundRule::default(),
            queue: LineQueue::new(settings.playback.queue),
            display: DisplayView::empty(Entrance {
                kind: EntranceKind::None,
                ms: 0,
            }),
            settings,
            skin,
            draft: Document::new(),
            active: None,
            generation: 0,
            cue_epoch: None,
            history: VecDeque::new(),
            next_row: 1,
            sent: VecDeque::new(),
            recall: None,
            stash: None,
            mirror_committed: false,
            speaking: false,
            draft_meta: LineMeta::default(),
            reply: None,
            speaker_override: None,
        };
        core.refresh_params();
        core.display.entrance = core.entrance();
        core
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn skin(&self) -> &Arc<Skin> {
        &self.skin
    }

    pub fn draft(&self) -> &Document {
        &self.draft
    }

    pub fn draft_meta(&self) -> LineMeta {
        self.draft_meta
    }

    pub fn display(&self) -> &DisplayView {
        &self.display
    }

    pub fn history(&self) -> &VecDeque<HistoryRow> {
        &self.history
    }

    /// The chat message the next sent line answers.
    pub fn pending_reply(&self) -> Option<&ChatRef> {
        self.reply.as_ref()
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    pub fn phase(&self) -> Option<Phase> {
        if self.retiring.is_some() {
            return Some(Phase::Fading);
        }
        self.active.as_ref().map(|a| a.player.phase())
    }

    pub fn mode(&self) -> InputMode {
        self.settings.general.input_mode
    }

    fn refresh_params(&mut self) {
        let p = &self.settings.playback;
        self.pacing = self
            .skin
            .pacing(p.speed_scale, p.punctuation_pauses, p.typing);
        self.sound_rule = self.skin.sounds.typing.rule(self.settings.sound.typing);
        if self.skin.sounds.typing.files.is_empty() {
            // The host synthesizes a default set of clicks.
            self.sound_rule.variants = DEFAULT_TYPING_VARIANTS;
        }
        self.queue.policy = p.queue;
    }

    fn entrance(&self) -> Entrance {
        let skin = &self.skin.dialogue.reveal.entrance;
        let kind = self.settings.playback.entrance.unwrap_or(skin.kind);
        Entrance {
            kind,
            ms: if kind == EntranceKind::None {
                0
            } else {
                skin.ms.max(1)
            },
        }
    }

    fn speaker(&self) -> Speaker {
        if let Some(speaker) = &self.speaker_override {
            let mut speaker = speaker.clone();
            speaker.portrait = self.display.portrait.clone();
            return speaker;
        }
        Speaker {
            name: self.settings.general.speaker_name.clone(),
            avatar: self.settings.general.avatar.clone(),
            portrait: self.display.portrait.clone(),
        }
    }

    fn touch_display(&mut self, fx: &mut Effects) {
        self.display.revision += 1;
        fx.display_changed = true;
    }

    pub fn handle(&mut self, cmd: Cmd, now: Millis) -> Effects {
        let mut fx = Effects::default();
        match cmd {
            Cmd::DraftChanged(doc) => self.draft_changed(doc, now, &mut fx),
            Cmd::DraftMeta(meta) => self.draft_meta = meta,
            Cmd::PageBreaks { line, breaks } => self.page_breaks(line, &breaks, now, &mut fx),
            Cmd::Send => self.send(now, &mut fx),
            Cmd::SendMarkup(markup) => {
                let parsed = nanatype_markup::parse(&markup);
                if !parsed.doc.is_blank() {
                    let mut line = Line::new(parsed.doc, self.speaker());
                    line.meta = parsed.meta;
                    self.submit(line, now, &mut fx);
                }
            }
            Cmd::Continue | Cmd::Skip if self.retiring.is_some() => self.finish_exit(now, &mut fx),
            Cmd::Continue => self.continue_key(now, &mut fx),
            Cmd::Skip => {
                if let Some(active) = self.active.as_mut()
                    && matches!(active.player.phase(), Phase::Revealing | Phase::Gated)
                {
                    let effects = active.player.finish_reveal(now);
                    self.cancel_cues(&mut fx);
                    self.apply_player(effects, now, &mut fx);
                    self.display.entrance = Entrance {
                        kind: EntranceKind::None,
                        ms: 0,
                    };
                    self.touch_display(&mut fx);
                }
            }
            Cmd::ClearDisplay => self.clear_display(now, &mut fx),
            Cmd::Recall(dir) => self.recall(dir, now, &mut fx),
            Cmd::Wake {
                epoch,
                reason: WakeReason::PresentationDone,
            } => {
                if self.retiring.as_ref().is_some_and(|r| r.epoch == epoch) {
                    self.finish_exit(now, &mut fx);
                }
            }
            Cmd::Wake { epoch, reason } => {
                if self.retiring.is_some() {
                    return fx;
                }
                if reason == WakeReason::SegmentEnd
                    && self.transitions_enabled()
                    && self.settings.playback.entrance.is_none()
                    && self
                        .skin
                        .dialogue
                        .reveal
                        .entrance
                        .material
                        .as_ref()
                        .is_some_and(|m| self.skin.material_programs.contains_key(m))
                    && self.active.as_ref().is_some_and(|a| {
                        a.generation == epoch >> 32 && a.player.epoch() == epoch & 0xffff_ffff
                    })
                {
                    let end = self
                        .display
                        .reveal
                        .hidden_from
                        .checked_sub(1)
                        .and_then(|u| self.display.reveal.unit_at(u))
                        .map(|at| at.saturating_add(u64::from(self.display.entrance.ms)));
                    if let Some(at_ms) = end.filter(|end| now < *end) {
                        fx.wakes.push(Wake {
                            epoch,
                            reason,
                            at_ms,
                        });
                        return fx;
                    }
                }
                if reason == WakeReason::AutoContinue
                    && self.active.as_ref().is_some_and(|a| {
                        a.generation == epoch >> 32
                            && a.player.epoch() == epoch & 0xffff_ffff
                            && a.player.next_clears_page()
                    })
                    && self.begin_exit(AfterExit::Wake { epoch, reason }, now, &mut fx)
                {
                    return fx;
                }
                let Some(active) = self.active.as_mut() else {
                    return fx;
                };
                if epoch >> 32 != active.generation {
                    return fx;
                }
                let effects = active.player.on_wake(epoch & 0xFFFF_FFFF, reason, now);
                self.apply_player(effects, now, &mut fx);
            }
            Cmd::CueFired(kind) => self.cue_fired(kind, now, &mut fx),
            Cmd::SetSettings(settings) => {
                self.set_settings(*settings, now, &mut fx);
                if self.retiring.is_some() && !self.transitions_enabled() {
                    self.finish_exit(now, &mut fx);
                }
            }
            Cmd::SetReducedMotion(reduced) => {
                if self.reduced_motion != reduced {
                    self.touch_display(&mut fx);
                }
                self.reduced_motion = reduced;
                if reduced && self.retiring.is_some() {
                    self.finish_exit(now, &mut fx);
                }
            }
            Cmd::SetSpeaker(speaker) => {
                self.speaker_override = speaker;
                if self.mode() == InputMode::Mirror
                    && !self.mirror_committed
                    && self.display.line.is_none()
                {
                    self.display.speaker = self.speaker();
                    self.touch_display(&mut fx);
                }
            }
            Cmd::RemoveHistoryRows(keys) => {
                self.history
                    .retain(|row| row.item.source.is_local() || !keys.contains(&row.key));
                fx.history_changed = true;
            }
            Cmd::SetSkin(skin) => {
                self.skin = skin;
                self.refresh_params();
                self.display.entrance = self.entrance();
                if let Some(p) = &self.display.portrait
                    && !self.skin.dialogue.portrait.variants.contains_key(p)
                {
                    self.display.portrait = None;
                }
                self.touch_display(&mut fx);
                if self.retiring.is_some() {
                    self.finish_exit(now, &mut fx);
                }
            }
            Cmd::External(item) => {
                let doc = Arc::new(item.doc.without_markers());
                self.push_row(item, doc, None, &mut fx);
            }
            Cmd::ClearHistory => {
                self.history.clear();
                fx.history_changed = true;
            }
            Cmd::SetReply(reply) => {
                if reply != self.reply {
                    self.reply = reply;
                    fx.reply_changed = true;
                    if self.mode() == InputMode::Mirror
                        && self.display.line.is_none()
                        && !self.mirror_committed
                    {
                        self.display.reply = self.reply.clone();
                        self.touch_display(&mut fx);
                    }
                }
            }
        }
        fx
    }

    fn set_settings(&mut self, settings: Settings, now: Millis, fx: &mut Effects) {
        let mode_changed = settings.general.input_mode != self.settings.general.input_mode;
        let cap = settings.history.max_items as usize;
        self.settings = settings;
        self.refresh_params();
        self.display.entrance = self.entrance();
        while self.history.len() > cap {
            self.history.pop_front();
            fx.history_changed = true;
        }
        if mode_changed {
            self.retiring = None;
            self.display.text_exit = None;
            self.queue.clear();
            self.stop_player(fx);
            self.mirror_committed = false;
            if self.mode() == InputMode::Mirror {
                let draft = self.draft.clone();
                self.show_mirror(draft, &Document::new(), now, fx);
            } else {
                self.blank_display(fx);
            }
        }
        self.touch_display(fx);
    }

    // ---- drafting -------------------------------------------------------

    fn draft_changed(&mut self, doc: Document, now: Millis, fx: &mut Effects) {
        let doc = doc.normalized();
        if doc == self.draft {
            return;
        }
        let previous = std::mem::replace(&mut self.draft, doc.clone());
        if self.recall.is_some() && Some(&doc) != self.recalled_doc() {
            // Typing after a recall keeps the recalled text as a new draft.
            self.recall = None;
            self.stash = None;
        }
        if self.mode() != InputMode::Mirror {
            return;
        }
        if self.mirror_committed {
            if doc.is_blank() {
                return;
            }
            self.mirror_committed = false;
            self.stop_player(fx);
            self.show_mirror(doc, &Document::new(), now, fx);
        } else {
            self.show_mirror(doc, &previous, now, fx);
        }
    }

    /// Mirror the draft: unchanged units stay, appended units ripple in.
    fn show_mirror(&mut self, doc: Document, previous: &Document, now: Millis, fx: &mut Effects) {
        self.retiring = None;
        self.display.text_exit = None;
        let units = doc.unit_count() as u32;
        let prev_shown = if self.display.line.is_none() && !self.display.empty {
            previous
        } else {
            &Document::new()
        };
        let delta = mirror_delta(prev_shown, &doc);
        let mirror_times = self.skin.dialogue.reveal.scramble.as_ref().map(|_| {
            let matches = nanatype_playback::mirror_matches(prev_shown, &doc);
            let added = matches.iter().filter(|m| m.is_none()).count() as u32;
            let mut offsets =
                mirror_offsets(added, self.settings.playback.mirror_stagger_ms, 240).into_iter();
            matches
                .into_iter()
                .map(|m| {
                    m.map(|i| {
                        self.display
                            .mirror_reveal_ms
                            .as_ref()
                            .and_then(|t| t.get(i))
                            .copied()
                            .unwrap_or(0)
                    })
                    .unwrap_or_else(|| now.max(1) + u64::from(offsets.next().unwrap_or(0)))
                })
                .collect::<Arc<[u64]>>()
        });
        let offsets = mirror_offsets(
            delta.appended,
            self.settings.playback.mirror_stagger_ms,
            240,
        );
        let reveal = if delta.appended == 0 {
            Reveal::full(units)
        } else {
            Reveal {
                visible_from: 0,
                shown: delta.common_prefix,
                segment: Some(SegmentReveal {
                    origin_ms: now,
                    start: delta.common_prefix,
                    reveal_ms: offsets.iter().copied().collect(),
                }),
                hidden_from: units,
            }
        };
        if self.sound_rule.enabled && delta.appended > 0 {
            let doc_units = doc.units();
            let mut cues = Vec::new();
            let mut last: Option<u32> = None;
            for (i, at) in offsets.iter().enumerate() {
                let unit = delta.common_prefix + i as u32;
                let class = doc_units[unit as usize].class();
                let skip = matches!(class, nanatype_model::CharClass::Space)
                    || (self.sound_rule.skip_punct
                        && matches!(class, nanatype_model::CharClass::Punct(_)));
                let spaced =
                    last.is_none_or(|t| at - t >= u32::from(self.sound_rule.min_interval_ms));
                if !skip && spaced {
                    cues.push(Cue {
                        at_ms: *at,
                        kind: CueKind::TypeSound {
                            unit,
                            variant: (unit % u32::from(self.sound_rule.variants.max(1))) as u8,
                            seed: unit.wrapping_mul(0x9E37_79B9),
                        },
                    });
                    last = Some(*at);
                }
            }
            if !cues.is_empty() {
                fx.cues.push(CueBatch {
                    epoch: 0,
                    origin_ms: now,
                    cues: cues.into(),
                });
            }
        }
        self.display.line = None;
        self.display.empty = doc.is_blank();
        self.display.doc = Arc::new(doc);
        self.display.speaker = self.speaker();
        self.display.reply = self.reply.clone();
        self.display.reveal = reveal;
        self.display.mirror_reveal_ms = mirror_times;
        self.display.fade = None;
        self.display.continue_indicator = false;
        self.touch_display(fx);
    }

    fn recalled_doc(&self) -> Option<&Document> {
        self.recall
            .and_then(|i| self.sent.get(i))
            .map(|draft| &draft.doc)
    }

    fn recall(&mut self, dir: i32, now: Millis, fx: &mut Effects) {
        if self.sent.is_empty() {
            return;
        }
        let last = self.sent.len() - 1;
        let next = match (self.recall, dir.signum()) {
            (None, -1) => Some(last),
            (None, _) => return,
            (Some(i), -1) => Some(i.saturating_sub(1)),
            (Some(i), _) if i >= last => None,
            (Some(i), _) => Some(i + 1),
        };
        if self.recall.is_none() {
            self.stash = Some(Draft {
                doc: self.draft.clone(),
                meta: self.draft_meta,
            });
        }
        self.recall = next;
        let draft = match next {
            Some(i) => self.sent[i].clone(),
            None => self.stash.take().unwrap_or_default(),
        };
        let previous = std::mem::replace(&mut self.draft, draft.doc.clone());
        self.draft_meta = draft.meta;
        if self.mode() == InputMode::Mirror {
            self.mirror_committed = false;
            self.stop_player(fx);
            self.show_mirror(draft.doc.clone(), &previous, now, fx);
        }
        fx.replace_draft = Some(draft.doc);
    }

    // ---- sending --------------------------------------------------------

    fn send(&mut self, now: Millis, fx: &mut Effects) {
        let doc = self.draft.clone();
        if doc.is_blank() {
            return;
        }
        self.sent.push_back(Draft {
            doc: doc.clone(),
            meta: self.draft_meta,
        });
        while self.sent.len() > RECALL_CAP {
            self.sent.pop_front();
        }
        self.recall = None;
        self.stash = None;
        self.draft = Document::new();
        fx.replace_draft = Some(Document::new());
        if self.settings.general.input_presence == InputPresence::Summon {
            fx.hide_input = true;
        }
        let mut line = Line::new(doc, self.speaker());
        line.meta = std::mem::take(&mut self.draft_meta);
        line.reply = self.reply.take().map(Box::new);
        fx.reply_changed |= line.reply.is_some();
        if self.mode() == InputMode::Mirror {
            self.commit_mirror(line, now, fx);
        } else {
            self.submit(line, now, fx);
        }
    }

    /// Mirror mode send: the line is already on screen; record it and run the
    /// after-line behaviour.
    fn commit_mirror(&mut self, line: Line, now: Millis, fx: &mut Effects) {
        fx.events.push(Event::LineSubmitted(line.clone()));
        self.mirror_committed = true;
        let doc = Arc::new(line.doc.without_markers());
        let item = self.local_item(&line);
        self.push_row(item, doc, None, fx);
        fx.events.push(Event::LineStarted(line.clone()));
        let instant = PacingParams {
            typing: false,
            ..self.pacing
        };
        let timeline = Arc::new(compile(
            &line.doc,
            &instant,
            &TypingSoundRule {
                enabled: false,
                ..self.sound_rule
            },
        ));
        self.start_player(line, timeline, now, fx, false);
    }

    fn submit(&mut self, line: Line, now: Millis, fx: &mut Effects) {
        fx.events.push(Event::LineSubmitted(line.clone()));
        let busy = self.retiring.is_some()
            || self
                .active
                .as_ref()
                .is_some_and(|a| matches!(a.player.phase(), Phase::Revealing | Phase::Gated));
        match self.queue.submit(line, busy) {
            QueueDecision::StartNow(line) => self.start_line(line, now, fx),
            QueueDecision::FinishCurrentThenStart(line) => {
                if let Some(active) = self.active.as_mut() {
                    let effects = active.player.finish_reveal(now);
                    self.cancel_cues(fx);
                    self.apply_player(effects, now, fx);
                }
                self.start_line(line, now, fx);
            }
            QueueDecision::Enqueued { position } => fx.events.push(Event::Queued { position }),
        }
    }

    fn local_item(&self, line: &Line) -> HistoryItem {
        HistoryItem {
            source: SourceId::local(),
            kind: SourceKind::Typewriter,
            line: Some(line.id),
            author: line.speaker.name.clone(),
            avatar: line.speaker.avatar.clone(),
            doc: line.doc.clone(),
            at: unix_millis(),
            uid: None,
            reply: line.reply.clone(),
        }
    }

    fn start_line(&mut self, mut line: Line, now: Millis, fx: &mut Effects) {
        if let Some(retiring) = &mut self.retiring {
            retiring.next = AfterExit::Line(line);
            return;
        }
        if self.begin_exit(AfterExit::Line(line.clone()), now, fx) {
            return;
        }
        if line.meta == LineMeta::default() {
            line.meta.auto_advance_ms = self.settings.playback.auto_advance_ms;
        }
        let timeline = Arc::new(compile(&line.doc, &self.pacing, &self.sound_rule));
        let sync = self.settings.history.sync;
        if sync != HistorySync::OnComplete {
            let doc = Arc::new(line.doc.without_markers());
            let item = self.local_item(&line);
            let reveal = (sync == HistorySync::Reveal).then_some(Reveal {
                visible_from: 0,
                shown: 0,
                segment: None,
                hidden_from: 0,
            });
            self.push_row(item, doc, reveal, fx);
        }
        fx.events.push(Event::LineStarted(line.clone()));
        if self.skin.sounds.open.is_some() {
            fx.ui_sounds.push(UiSound::Open);
        }
        self.start_player(line, timeline, now, fx, true);
    }

    fn start_player(
        &mut self,
        line: Line,
        timeline: Arc<Timeline>,
        now: Millis,
        fx: &mut Effects,
        animate: bool,
    ) {
        self.stop_player(fx);
        self.generation += 1;
        let after = line.meta.after.unwrap_or(self.settings.playback.after);
        let auto = line.meta.auto_advance_ms;
        let units = timeline.unit_count;
        let (player, effects) = Player::start(line.id, timeline, after, auto, now);
        self.display.line = Some(line.id);
        self.display.mirror_reveal_ms = None;
        self.display.doc = Arc::new(line.doc.clone());
        self.display.speaker = line.speaker.clone();
        self.display.reply = line.reply.as_deref().cloned();
        self.display.fade = None;
        self.display.text_exit = None;
        self.display.shake = None;
        self.display.empty = false;
        self.display.reveal = if animate {
            Reveal {
                visible_from: 0,
                shown: 0,
                segment: None,
                hidden_from: 0,
            }
        } else {
            Reveal::full(units)
        };
        self.active = Some(Active {
            line,
            player,
            generation: self.generation,
        });
        self.apply_player(effects, now, fx);
        if !animate {
            self.display.reveal = Reveal::full(units);
        }
    }

    fn cancel_cues(&mut self, fx: &mut Effects) {
        if let Some(epoch) = self.cue_epoch.take() {
            fx.cancel_epochs.push(epoch);
        }
        // A cancelled stop would leave the mouth moving.
        self.set_speaking(false, fx);
    }

    fn page_breaks(&mut self, line: LineId, breaks: &[u32], now: Millis, fx: &mut Effects) {
        if self.retiring.is_some() {
            return;
        }
        if self.skin.dialogue.overflow != Overflow::Page || self.mode() == InputMode::Mirror {
            return;
        }
        let auto_ms = self.skin.dialogue.overflow_auto_ms;
        let Some(active) = self.active.as_mut().filter(|a| a.line.id == line) else {
            return;
        };
        let (touched, effects) = active.player.paginate(breaks, auto_ms, now);
        if touched && let Some(epoch) = self.cue_epoch.take() {
            // Rescheduled from the same origin: talking carries on.
            fx.cancel_epochs.push(epoch);
        }
        self.apply_player(effects, now, fx);
    }

    fn set_speaking(&mut self, on: bool, fx: &mut Effects) {
        if self.speaking != on {
            self.speaking = on;
            fx.events.push(Event::Speaking(on));
        }
    }

    fn stop_player(&mut self, fx: &mut Effects) {
        if let Some(active) = self.active.take() {
            self.cancel_cues(fx);
            self.complete_row(active.line.id, fx);
            if self.display.continue_indicator {
                self.display.continue_indicator = false;
            }
        }
    }

    fn blank_display(&mut self, fx: &mut Effects) {
        let entrance = self.display.entrance;
        let revision = self.display.revision;
        let portrait = self.display.portrait.take();
        self.display = DisplayView::empty(entrance);
        self.display.revision = revision;
        self.display.portrait = portrait;
        self.touch_display(fx);
    }

    fn transitions_enabled(&self) -> bool {
        self.settings.skin.advanced_effects
            && !self.settings.playback.reduced_motion
            && !self.reduced_motion
    }
    fn begin_exit(&mut self, next: AfterExit, now: Millis, fx: &mut Effects) -> bool {
        if self.bypass_exit
            || self.display.empty
            || self.display.doc.is_blank()
            || !self.transitions_enabled()
        {
            return false;
        }
        let Some(ms) = self
            .skin
            .dialogue
            .reveal
            .scramble
            .as_ref()
            .map(|s| s.exit_ms)
            .or_else(|| {
                self.skin
                    .dialogue
                    .reveal
                    .exit
                    .as_ref()
                    .filter(|e| e.ms > 0)
                    .filter(|e| {
                        e.material
                            .as_ref()
                            .is_some_and(|m| self.skin.material_programs.contains_key(m))
                    })
                    .map(|e| e.ms)
            })
        else {
            return false;
        };
        self.cancel_cues(fx);
        self.display.text_exit = Some(crate::view::TextExit { at_ms: now, ms });
        self.display.fade = None;
        self.display.continue_indicator = false;
        self.retirement_serial += 1;
        let epoch = (1u64 << 63) | self.retirement_serial;
        self.retiring = Some(Retiring { epoch, next });
        fx.wakes.push(Wake {
            epoch,
            at_ms: now + u64::from(ms),
            reason: WakeReason::PresentationDone,
        });
        self.touch_display(fx);
        true
    }
    fn finish_exit(&mut self, now: Millis, fx: &mut Effects) {
        let Some(retiring) = self.retiring.take() else {
            return;
        };
        self.bypass_exit = true;
        self.display.text_exit = None;
        match retiring.next {
            AfterExit::Clear => self.clear_display(now, fx),
            AfterExit::Line(line) => self.start_line(line, now, fx),
            AfterExit::Advance => self.continue_key(now, fx),
            AfterExit::Wake { epoch, reason } => {
                if let Some(a) = self.active.as_mut().filter(|a| a.generation == epoch >> 32) {
                    let effects = a.player.on_wake(epoch & 0xffff_ffff, reason, now);
                    self.apply_player(effects, now, fx);
                }
            }
            AfterExit::Finish(id) => {
                self.apply_player(vec![PlayerEffect::LineFinished(id)], now, fx)
            }
        }
        self.bypass_exit = false;
        self.touch_display(fx);
    }
    fn clear_display(&mut self, now: Millis, fx: &mut Effects) {
        self.queue.clear();
        if let Some(r) = self.retiring.as_mut() {
            r.next = AfterExit::Clear;
            return;
        }
        if self.begin_exit(AfterExit::Clear, now, fx) {
            return;
        }
        self.stop_player(fx);
        self.mirror_committed = false;
        self.blank_display(fx);
    }

    fn continue_key(&mut self, now: Millis, fx: &mut Effects) {
        if self
            .active
            .as_ref()
            .is_some_and(|a| a.player.next_clears_page())
            && self.begin_exit(AfterExit::Advance, now, fx)
        {
            return;
        }
        let Some(active) = self.active.as_mut() else {
            return;
        };
        match active.player.phase() {
            Phase::Revealing => {
                let (_, effects) = active.player.advance(now);
                self.cancel_cues(fx);
                self.apply_player(effects, now, fx);
                self.display.entrance = Entrance {
                    kind: EntranceKind::None,
                    ms: 0,
                };
                self.touch_display(fx);
            }
            Phase::Gated => {
                let (_, effects) = active.player.advance(now);
                if self.skin.sounds.advance.is_some() {
                    fx.ui_sounds.push(UiSound::Advance);
                }
                self.apply_player(effects, now, fx);
            }
            Phase::Done | Phase::Fading | Phase::Finished => {
                if let Some(next) = self.queue.pop_next() {
                    if self.skin.sounds.advance.is_some() {
                        fx.ui_sounds.push(UiSound::Advance);
                    }
                    self.start_line(next, now, fx);
                }
            }
        }
    }

    // ---- player effects -------------------------------------------------

    fn apply_player(&mut self, effects: Vec<PlayerEffect>, now: Millis, fx: &mut Effects) {
        let Some(generation) = self.active.as_ref().map(|a| a.generation) else {
            return;
        };
        for effect in effects {
            match effect {
                PlayerEffect::ShowSegment {
                    seg,
                    origin_ms,
                    reveal_ms,
                    ..
                } => {
                    let tl = self
                        .active
                        .as_ref()
                        .expect("active")
                        .player
                        .timeline()
                        .clone();
                    let s = &tl.segments[seg];
                    let reveal = Reveal {
                        visible_from: s.visible_from,
                        shown: s.units.start,
                        segment: Some(SegmentReveal {
                            origin_ms,
                            start: s.units.start,
                            reveal_ms,
                        }),
                        hidden_from: s.units.end,
                    };
                    self.display.reveal = reveal.clone();
                    self.display.entrance = self.entrance();
                    self.display.fade = None;
                    self.follow_row(reveal, fx);
                    self.touch_display(fx);
                }
                PlayerEffect::ScheduleCues {
                    epoch,
                    origin_ms,
                    cues,
                } => {
                    let epoch = global(generation, epoch);
                    self.cue_epoch = Some(epoch);
                    fx.cues.push(CueBatch {
                        epoch,
                        origin_ms,
                        cues,
                    });
                }
                PlayerEffect::FireNow { cues } => fx.fire_now.extend(cues),
                PlayerEffect::WakeAt {
                    epoch,
                    at_ms,
                    reason,
                } => fx.wakes.push(Wake {
                    epoch: global(generation, epoch),
                    at_ms,
                    reason,
                }),
                PlayerEffect::ContinueIndicator(on) => {
                    if self.display.continue_indicator != on {
                        self.display.continue_indicator = on;
                        self.touch_display(fx);
                    }
                }
                PlayerEffect::FadeOut { at_ms, ms } => {
                    if self.begin_exit(AfterExit::Finish(self.display.line.unwrap()), now, fx) {
                        return;
                    }
                    self.display.fade = Some((at_ms, ms));
                    self.touch_display(fx);
                }
                PlayerEffect::LineRevealed(id) => {
                    if self.settings.history.sync == HistorySync::OnComplete
                        && let Some(active) = &self.active
                    {
                        let line = active.line.clone();
                        let item = self.local_item(&line);
                        self.push_row(item, Arc::new(line.doc.without_markers()), None, fx);
                    }
                    self.complete_row(id, fx);
                    fx.events.push(Event::LineRevealed(id));
                }
                PlayerEffect::LineFinished(id) => {
                    if self.begin_exit(AfterExit::Finish(id), now, fx) {
                        return;
                    }
                    self.set_speaking(false, fx);
                    fx.events.push(Event::LineFinished(id));
                    self.active = None;
                    self.cue_epoch = None;
                    self.mirror_committed = false;
                    self.blank_display(fx);
                    if self.queue.policy == nanatype_playback::QueuePolicy::Queue
                        && let Some(next) = self.queue.pop_next()
                    {
                        self.start_line(next, now, fx);
                    }
                    return;
                }
                PlayerEffect::AutoAdvance(_) => {
                    if let Some(next) = self.queue.pop_next() {
                        self.start_line(next, now, fx);
                        return;
                    }
                }
            }
        }
    }

    fn cue_fired(&mut self, kind: CueKind, now: Millis, fx: &mut Effects) {
        match kind {
            CueKind::Expression { name, intensity } => {
                if self.skin.dialogue.portrait.variants.contains_key(&name) {
                    self.display.portrait = Some(name.clone());
                    self.touch_display(fx);
                }
                fx.events.push(Event::Expression { name, intensity });
            }
            CueKind::Portrait { variant } => {
                self.display.portrait = Some(variant.clone()).filter(|v| v != "default");
                self.touch_display(fx);
                fx.events.push(Event::Portrait(variant));
            }
            CueKind::Shake { amp, ms } => {
                self.display.shake = Some(ShakeView {
                    at_ms: now,
                    amp,
                    ms,
                });
                self.touch_display(fx);
                fx.events.push(Event::Shake { amp, ms });
            }
            CueKind::SpeakingStart => self.set_speaking(true, fx),
            CueKind::SpeakingStop => self.set_speaking(false, fx),
            CueKind::Vts(action) => fx.events.push(Event::Vts(action)),
            CueKind::TypeSound { .. } | CueKind::Sound { .. } => {}
        }
    }

    // ---- history --------------------------------------------------------

    fn push_row(
        &mut self,
        item: HistoryItem,
        doc: Arc<nanatype_model::Document>,
        reveal: Option<Reveal>,
        fx: &mut Effects,
    ) {
        let key = self.next_row;
        self.next_row += 1;
        self.history.push_back(HistoryRow {
            key,
            item,
            doc,
            reveal,
        });
        while self.history.len() > self.settings.history.max_items as usize {
            self.history.pop_front();
        }
        fx.history_changed = true;
    }

    fn follow_row(&mut self, reveal: Reveal, fx: &mut Effects) {
        let Some(id) = self.active.as_ref().map(|a| a.line.id) else {
            return;
        };
        if let Some(row) = self
            .history
            .iter_mut()
            .rev()
            .find(|r| r.item.line == Some(id))
            && row.reveal.is_some()
        {
            // Paging clears the display, but history keeps the whole line.
            row.reveal = Some(Reveal {
                visible_from: 0,
                ..reveal
            });
            fx.history_changed = true;
        }
    }

    fn complete_row(&mut self, id: LineId, fx: &mut Effects) {
        let Some(row) = self
            .history
            .iter_mut()
            .rev()
            .find(|r| r.item.line == Some(id))
        else {
            return;
        };
        let Some(reveal) = row.reveal.clone() else {
            return;
        };
        let units = row.doc.unit_count() as u32;
        if reveal.is_full(units) {
            return;
        }
        // Let the running animation finish; nothing stays hidden any more.
        row.reveal = reveal.segment.is_some().then_some(Reveal {
            visible_from: 0,
            hidden_from: units,
            ..reveal
        });
        fx.history_changed = true;
    }
}
