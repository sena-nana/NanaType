use std::collections::VecDeque;
use std::sync::Arc;

use nanatype_model::{
    ChatRef, Document, Entrance, EntranceKind, HistoryItem, Line, LineId, LineMeta, Overflow,
    SourceId, SourceKind, Speaker, VtsAction, unix_millis,
};
use nanatype_playback::{
    Cue, CueKind, ExitStyle, LineQueue, PacingParams, Phase, Player, PlayerEffect, QueueDecision,
    QueuePolicy, TypingClickFilter, TypingSoundRule, WakeReason, compile, mirror_delta,
    mirror_matches, mirror_offsets,
};
use nanatype_settings::{HistorySync, InputMode, InputPresence, Settings};
use nanatype_theme::Skin;

use crate::Millis;
use crate::config::{CoreConfig, SkinCapabilities};
use crate::draft::{Draft, DraftBox};
use crate::feed::MessageFeed;
use crate::view::{DisplayExit, DisplayView, HistoryRow, Reveal, SegmentReveal, ShakeView};

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
    /// The line is read aloud and typing follows the voice: hold the units
    /// not typed yet until [`Cmd::SpeechReached`]. Without a word within
    /// [`nanatype_playback::SPEECH_WAIT_MS`] it types on its own.
    SpeechPaced(LineId),
    /// The voice reached unit `through` of `line` (exclusive).
    SpeechReached {
        line: LineId,
        through: u32,
    },
    /// The voice finished or failed: the rest types at its own pace.
    SpeechEnded(LineId),
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
    SetConfig(Box<CoreConfig>),
    /// Effective accessibility preference, including the operating system.
    SetReducedMotion(bool),
    SetSkin(Box<SkinCapabilities>),
    SetSpeaker(Option<Speaker>),
    /// Show the last sent line again, as it was said. It is not sent
    /// anywhere else and adds no history row.
    Replay,
    RemoveHistoryRows(Vec<u64>),
    /// A row from another message source (live chat).
    External(HistoryItem),
    ClearHistory,
    ClearDanmaku,
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
    /// Replace the editor content and its header metadata atomically.
    pub replace_draft: Option<Draft>,
    pub hide_input: bool,
    pub display_changed: bool,
    pub history_changed: bool,
    pub danmaku_changed: bool,
    /// The pending reply changed (set, dropped, or used by a send).
    pub reply_changed: bool,
}

struct Active {
    /// `None` when retiring an uncommitted mirror draft.
    line: Option<Line>,
    player: Player,
    /// Player generation; makes epochs unique across lines.
    generation: u64,
}

impl Active {
    fn line_id(&self) -> LineId {
        self.line
            .as_ref()
            .map_or_else(|| self.player.line(), |l| l.id)
    }
}

enum DisplayOwner {
    Empty,
    MirrorDraft {
        doc: Arc<Document>,
        timestamps: Option<Arc<[Millis]>>,
    },
    Playback(Box<Active>),
}

pub struct Core {
    reduced_motion: bool,
    config: CoreConfig,
    skin: SkinCapabilities,
    pacing: PacingParams,
    sound_rule: TypingSoundRule,
    drafts: DraftBox,
    owner: DisplayOwner,
    pending_line: Option<Line>,
    generation: u64,
    /// Epoch of the last scheduled cue batch (for cancellation).
    cue_epoch: Option<u64>,
    queue: LineQueue,
    display: DisplayView,
    feed: MessageFeed,
    /// The last speaking cue fired was a start (avatars' mouths are moving).
    speaking: bool,
    /// The chat message the next sent line answers.
    reply: Option<ChatRef>,
    speaker_override: Option<Speaker>,
    /// The last line sent from here, for [`Cmd::Replay`].
    last_sent: Option<Line>,
    /// The line [`Cmd::Replay`] last started; it adds no history row.
    replay: Option<LineId>,
    /// The sent line that replay repeats.
    replay_of: Option<LineId>,
}

fn global(generation: u64, epoch: u64) -> u64 {
    (generation << 32) | (epoch & 0xFFFF_FFFF)
}

impl Core {
    pub fn new(config: CoreConfig, skin: SkinCapabilities) -> Self {
        let pacing = config.pacing(&skin);
        let sound_rule = config.sound_rule(&skin);
        let queue = LineQueue::new(config.playback.queue);
        let feed = MessageFeed::new(config.history_cap, config.separate_danmaku);
        let entrance = config.entrance_for(&Document::new(), &skin);
        Self {
            reduced_motion: false,
            config,
            skin,
            pacing,
            sound_rule,
            drafts: DraftBox::default(),
            owner: DisplayOwner::Empty,
            pending_line: None,
            generation: 0,
            cue_epoch: None,
            queue,
            display: DisplayView::empty(entrance),
            feed,
            speaking: false,
            reply: None,
            speaker_override: None,
            last_sent: None,
            replay: None,
            replay_of: None,
        }
    }

    pub fn from_settings(settings: &Settings, skin: &Skin) -> Self {
        Self::new(CoreConfig::from(settings), SkinCapabilities::from(skin))
    }

    pub fn config(&self) -> &CoreConfig {
        &self.config
    }

    pub fn skin(&self) -> &SkinCapabilities {
        &self.skin
    }

    pub fn draft(&self) -> &Document {
        self.drafts.doc()
    }

    pub fn draft_meta(&self) -> LineMeta {
        self.drafts.meta().clone()
    }

    /// The sent line that `line` replays, when it is the last replay.
    pub fn replay_of(&self, line: LineId) -> Option<LineId> {
        self.replay_of.filter(|_| self.replay == Some(line))
    }

    pub fn display(&self) -> &DisplayView {
        &self.display
    }

    pub fn history(&self) -> &VecDeque<HistoryRow> {
        &self.feed.history
    }

    pub fn danmaku(&self) -> &VecDeque<HistoryRow> {
        &self.feed.danmaku
    }

    /// The chat message the next sent line answers.
    pub fn pending_reply(&self) -> Option<&ChatRef> {
        self.reply.as_ref()
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    pub fn phase(&self) -> Option<Phase> {
        match &self.owner {
            DisplayOwner::Playback(a) => Some(a.player.phase()),
            _ => None,
        }
    }

    pub fn mode(&self) -> InputMode {
        self.config.input_mode
    }

    fn active(&self) -> Option<&Active> {
        match &self.owner {
            DisplayOwner::Playback(a) => Some(a),
            _ => None,
        }
    }

    fn active_mut(&mut self) -> Option<&mut Active> {
        match &mut self.owner {
            DisplayOwner::Playback(a) => Some(a),
            _ => None,
        }
    }

    fn refresh_params(&mut self) {
        self.pacing = self.config.pacing(&self.skin);
        self.sound_rule = self.config.sound_rule(&self.skin);
        self.queue.policy = self.config.playback.queue;
    }

    fn entrance(&self) -> Entrance {
        self.config.entrance_for(&self.display.doc, &self.skin)
    }

    fn speaker_for(&self, meta: &LineMeta) -> Speaker {
        self.config.speaker_for(
            meta,
            self.speaker_override.as_ref(),
            self.display.portrait.clone(),
        )
    }

    /// In mirror mode the draft on the display follows a speaker change.
    fn refresh_draft_speaker(&mut self, fx: &mut Effects) {
        if matches!(self.owner, DisplayOwner::MirrorDraft { .. }) {
            let speaker = self.speaker_for(self.drafts.meta());
            if self.display.speaker != speaker {
                self.display.speaker = speaker;
                self.touch_display(fx);
            }
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
            Cmd::DraftMeta(meta) => {
                self.drafts.set_meta(meta);
                self.refresh_draft_speaker(&mut fx);
            }
            Cmd::PageBreaks { line, breaks } => self.page_breaks(line, &breaks, now, &mut fx),
            Cmd::SpeechPaced(line) => self.speech(line, now, &mut fx, |p| p.follow_speech(now)),
            Cmd::SpeechReached { line, through } => {
                self.speech(line, now, &mut fx, |p| p.speech_reached(through, now))
            }
            Cmd::SpeechEnded(line) => self.speech(line, now, &mut fx, |p| p.speech_ended(now)),
            Cmd::Send => self.send(now, &mut fx),
            Cmd::SendMarkup(markup) => {
                let parsed = nanatype_markup::parse(&markup);
                if !parsed.doc.is_blank() {
                    let mut line = Line::new(parsed.doc, self.speaker_for(&parsed.meta));
                    line.meta = parsed.meta;
                    self.submit(line, now, &mut fx);
                }
            }
            Cmd::Continue => self.continue_key(now, &mut fx),
            Cmd::Skip => self.skip_key(now, &mut fx),
            Cmd::ClearDisplay => self.clear_display(now, &mut fx),
            Cmd::Recall(dir) => self.recall(dir, now, &mut fx),
            Cmd::Wake { epoch, reason } => {
                let Some(active) = self.active_mut() else {
                    return fx;
                };
                if epoch >> 32 != active.generation {
                    return fx;
                }
                let effects = active.player.on_wake(epoch & 0xFFFF_FFFF, reason, now);
                self.apply_player(effects, now, &mut fx);
            }
            Cmd::CueFired(kind) => self.cue_fired(kind, now, &mut fx),
            Cmd::SetConfig(config) => self.set_config(*config, now, &mut fx),
            Cmd::SetReducedMotion(reduced) => self.set_reduced_motion(reduced, now, &mut fx),
            Cmd::SetSkin(skin) => self.set_skin(*skin, now, &mut fx),
            Cmd::SetSpeaker(speaker) => {
                self.speaker_override = speaker;
                self.refresh_draft_speaker(&mut fx);
            }
            Cmd::Replay => self.replay(now, &mut fx),
            Cmd::RemoveHistoryRows(keys) => self.feed.remove_non_local(&keys, &mut fx),
            Cmd::External(item) => self.feed.push_external(item, &mut fx),
            Cmd::ClearHistory => self.feed.clear_history(&mut fx),
            Cmd::ClearDanmaku => self.feed.clear_danmaku(&mut fx),
            Cmd::SetReply(reply) => {
                if reply != self.reply {
                    self.reply = reply;
                    fx.reply_changed = true;
                    if matches!(self.owner, DisplayOwner::MirrorDraft { .. }) {
                        self.display.reply = self.reply.clone();
                        self.touch_display(&mut fx);
                    }
                }
            }
        }
        fx
    }

    fn set_config(&mut self, next: CoreConfig, now: Millis, fx: &mut Effects) {
        if next == self.config {
            return;
        }
        let prev = std::mem::replace(&mut self.config, next);
        let mode_changed = self.config.input_mode != prev.input_mode;
        self.refresh_params();

        if self.config.history_cap != prev.history_cap
            || self.config.separate_danmaku != prev.separate_danmaku
        {
            self.feed
                .reconfigure(self.config.history_cap, self.config.separate_danmaku, fx);
        }

        if mode_changed {
            self.queue.clear();
            self.pending_line = None;
            self.stop_player(fx);
            if self.mode() == InputMode::Mirror {
                let draft = self.drafts.doc().clone();
                self.show_mirror(draft, &Document::new(), now, fx);
            } else {
                self.owner = DisplayOwner::Empty;
                self.blank_display(fx);
            }
            return;
        }

        let new_entrance = self.entrance();
        if self.display.entrance != new_entrance {
            self.display.entrance = new_entrance;
            if !self.display.empty {
                self.touch_display(fx);
            }
        }

        if self.config.characters != prev.characters
            || self.config.active_character != prev.active_character
        {
            self.refresh_draft_speaker(fx);
        }

        let motion_changed = self.config.playback.reduced_motion != prev.playback.reduced_motion
            || self.config.advanced_effects != prev.advanced_effects;
        if motion_changed && !self.display.empty {
            self.touch_display(fx);
        }

        self.sync_active_profile(now, fx);
    }

    fn set_reduced_motion(&mut self, reduced: bool, now: Millis, fx: &mut Effects) {
        if self.reduced_motion == reduced {
            return;
        }
        self.reduced_motion = reduced;
        if !self.display.empty {
            self.touch_display(fx);
        }
        self.sync_active_profile(now, fx);
    }

    fn set_skin(&mut self, skin: SkinCapabilities, now: Millis, fx: &mut Effects) {
        if skin == self.skin {
            return;
        }
        self.skin = skin;
        self.refresh_params();
        let new_entrance = self.entrance();
        let mut changed = false;
        if self.display.entrance != new_entrance {
            self.display.entrance = new_entrance;
            changed = true;
        }
        if let Some(p) = &self.display.portrait
            && !self.skin.portrait_variants.contains(p)
        {
            self.display.portrait = None;
            changed = true;
        }
        if changed || !self.display.empty {
            self.touch_display(fx);
        }
        if let Some(active) = self.active_mut() {
            if active.player.phase() == Phase::Exiting {
                let effects = active.player.finish_exit(now);
                self.apply_player(effects, now, fx);
            } else {
                self.sync_active_profile(now, fx);
            }
        }
    }

    fn sync_active_profile(&mut self, now: Millis, fx: &mut Effects) {
        let Some(active) = self.active() else {
            return;
        };
        let meta = active
            .line
            .as_ref()
            .map(|l| l.meta.clone())
            .unwrap_or_default();
        let doc = active
            .line
            .as_ref()
            .map(|l| l.doc.clone())
            .unwrap_or_else(|| (*self.display.doc).clone());
        let entrance = self.config.entrance_for(&doc, &self.skin);
        let profile =
            self.config
                .playback_profile(&meta, &doc, entrance, &self.skin, self.reduced_motion);
        if let Some(active) = self.active_mut() {
            let effects = active.player.set_profile(profile, now);
            if !effects.is_empty() {
                self.apply_player(effects, now, fx);
            }
        }
    }

    // ---- drafting -------------------------------------------------------

    fn draft_changed(&mut self, doc: Document, now: Millis, fx: &mut Effects) {
        let Some((previous, doc)) = self.drafts.set_doc(doc) else {
            return;
        };
        if self.mode() != InputMode::Mirror {
            return;
        }
        match &self.owner {
            DisplayOwner::Playback(_) => {
                if doc.is_blank() {
                    return;
                }
                self.pending_line = None;
                self.stop_player(fx);
                self.show_mirror(doc, &Document::new(), now, fx);
            }
            DisplayOwner::Empty | DisplayOwner::MirrorDraft { .. } => {
                self.show_mirror(doc, &previous, now, fx);
            }
        }
    }

    /// Mirror the draft: unchanged units stay, newly added units ripple in.
    fn show_mirror(&mut self, doc: Document, previous: &Document, now: Millis, fx: &mut Effects) {
        let (prev_shown, prev_times) = match &self.owner {
            DisplayOwner::MirrorDraft { timestamps, .. } if !self.display.empty => {
                (previous, timestamps.as_deref())
            }
            _ => (&Document::new(), None),
        };
        let units = doc.unit_count() as u32;
        let stagger_ms = self.config.playback.mirror_stagger_ms;
        let mut click_filter = TypingClickFilter::new(self.sound_rule);
        let mut cues = Vec::new();

        let (reveal, unit_times) = if self.skin.scramble_intro_ms.is_some() {
            let matches = mirror_matches(prev_shown, &doc);
            let added = matches.iter().filter(|m| m.is_none()).count() as u32;
            let mut offsets = mirror_offsets(added, stagger_ms, 240).into_iter();
            let doc_units = (self.sound_rule.enabled && added > 0).then(|| doc.units());
            let times: Arc<[Millis]> = matches
                .into_iter()
                .enumerate()
                .map(|(u, matched)| match matched {
                    Some(prev_i) => prev_times.and_then(|t| t.get(prev_i)).copied().unwrap_or(0),
                    None => {
                        let off = offsets.next().unwrap_or(0);
                        if let Some(ref du) = doc_units
                            && let Some(cue) = click_filter.next(u as u32, du[u].class(), off)
                        {
                            cues.push(cue);
                        }
                        now.max(1) + u64::from(off)
                    }
                })
                .collect();
            let reveal = Reveal {
                visible_from: 0,
                shown: 0,
                segment: None,
                unit_times: Some(times.clone()),
                hidden_from: units,
            };
            (reveal, Some(times))
        } else {
            let delta = mirror_delta(prev_shown, &doc);
            let offsets = mirror_offsets(delta.appended, stagger_ms, 240);
            if self.sound_rule.enabled && delta.appended > 0 {
                let doc_units = doc.units();
                for (i, &at) in offsets.iter().enumerate() {
                    let u = delta.common_prefix + i as u32;
                    if let Some(cue) = click_filter.next(u, doc_units[u as usize].class(), at) {
                        cues.push(cue);
                    }
                }
            }
            let reveal = if delta.appended == 0 {
                Reveal::full(units)
            } else {
                Reveal {
                    visible_from: 0,
                    shown: delta.common_prefix,
                    segment: Some(SegmentReveal {
                        origin_ms: now,
                        start: delta.common_prefix,
                        reveal_ms: offsets.into(),
                    }),
                    unit_times: None,
                    hidden_from: units,
                }
            };
            (reveal, None)
        };

        if !cues.is_empty() {
            fx.cues.push(CueBatch {
                epoch: 0,
                origin_ms: now,
                cues: cues.into(),
            });
        }

        let doc = Arc::new(doc);
        let empty = doc.is_blank();
        let entrance = self.config.entrance_for(&doc, &self.skin);
        self.display.line = None;
        self.display.empty = empty;
        self.display.doc = doc.clone();
        self.display.invalidate_page_cache();
        self.display.entrance = entrance;
        self.display.speaker = self.speaker_for(self.drafts.meta());
        self.display.reply = self.reply.clone();
        self.display.reveal = reveal;
        self.display.exit = None;
        self.display.continue_indicator = false;
        self.display.replay = false;
        self.owner = if empty {
            DisplayOwner::Empty
        } else {
            DisplayOwner::MirrorDraft {
                doc,
                timestamps: unit_times,
            }
        };
        self.touch_display(fx);
    }

    fn recall(&mut self, dir: i32, now: Millis, fx: &mut Effects) {
        let Some((previous, draft)) = self.drafts.recall(dir) else {
            return;
        };
        if self.mode() == InputMode::Mirror {
            self.pending_line = None;
            self.stop_player(fx);
            self.show_mirror(draft.doc.clone(), &previous, now, fx);
        }
        fx.replace_draft = Some(draft);
    }

    // ---- sending --------------------------------------------------------

    fn send(&mut self, now: Millis, fx: &mut Effects) {
        let Some(draft) = self.drafts.take_for_send() else {
            return;
        };
        fx.replace_draft = Some(Draft::default());
        if self.config.input_presence == InputPresence::Summon {
            fx.hide_input = true;
        }
        let mut line = Line::new(draft.doc, self.speaker_for(&draft.meta));
        line.meta = draft.meta;
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
    fn commit_mirror(&mut self, mut line: Line, now: Millis, fx: &mut Effects) {
        if line.meta == LineMeta::default() {
            line.meta.auto_advance_ms = self.config.playback.auto_advance_ms;
        }
        fx.events.push(Event::LineSubmitted(line.clone()));
        self.last_sent = Some(line.clone());
        let item = self.local_item(&line);
        self.feed.push_history(item, None, fx);
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
        let units = timeline.unit_count;
        self.stop_player(fx);
        self.generation += 1;
        let entrance = self.config.entrance_for(&line.doc, &self.skin);
        let profile = self.config.playback_profile(
            &line.meta,
            &line.doc,
            entrance,
            &self.skin,
            self.reduced_motion,
        );
        let (player, effects) = Player::start_completed(line.id, timeline, profile, now);
        self.install_display_for_line(&line, entrance, Reveal::full(units));
        self.owner = DisplayOwner::Playback(Box::new(Active {
            line: Some(line),
            player,
            generation: self.generation,
        }));
        self.apply_player(effects, now, fx);
        self.touch_display(fx);
    }

    fn submit(&mut self, line: Line, now: Millis, fx: &mut Effects) {
        fx.events.push(Event::LineSubmitted(line.clone()));
        self.last_sent = Some(line.clone());
        self.enqueue(line, now, fx);
    }

    fn replay(&mut self, now: Millis, fx: &mut Effects) {
        let Some(last) = &self.last_sent else { return };
        let line = Line {
            id: LineId::next(),
            created_at: unix_millis(),
            ..last.clone()
        };
        self.replay_of = Some(last.id);
        self.replay = Some(line.id);
        self.enqueue(line, now, fx);
    }

    fn enqueue(&mut self, line: Line, now: Millis, fx: &mut Effects) {
        let busy = self.active().is_some_and(|a| {
            matches!(
                a.player.phase(),
                Phase::Revealing | Phase::Settling | Phase::Gated | Phase::Exiting
            )
        });
        match self.queue.submit(line, busy) {
            QueueDecision::StartNow(line) => self.start_line(line, now, fx),
            QueueDecision::FinishCurrentThenStart(line) => {
                if let Some(active) = self.active_mut()
                    && matches!(
                        active.player.phase(),
                        Phase::Revealing | Phase::Settling | Phase::Gated
                    )
                {
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
            event: None,
        }
    }

    fn start_line(&mut self, mut line: Line, now: Millis, fx: &mut Effects) {
        if self.begin_exit_on_display(now, fx) {
            self.pending_line = Some(line);
            return;
        }
        if line.meta == LineMeta::default() {
            line.meta.auto_advance_ms = self.config.playback.auto_advance_ms;
        }
        let timeline = Arc::new(compile(&line.doc, &self.pacing, &self.sound_rule));
        let sync = self.config.history_sync;
        if sync != HistorySync::OnComplete && self.replay != Some(line.id) {
            let item = self.local_item(&line);
            let reveal = (sync == HistorySync::Reveal).then_some(Reveal {
                visible_from: 0,
                shown: 0,
                segment: None,
                unit_times: None,
                hidden_from: 0,
            });
            self.feed.push_history(item, reveal, fx);
        }
        fx.events.push(Event::LineStarted(line.clone()));
        if self.skin.has_open_sound {
            fx.ui_sounds.push(UiSound::Open);
        }

        self.stop_player(fx);
        self.generation += 1;
        let entrance = self.config.entrance_for(&line.doc, &self.skin);
        let profile = self.config.playback_profile(
            &line.meta,
            &line.doc,
            entrance,
            &self.skin,
            self.reduced_motion,
        );
        let (player, effects) = Player::start(line.id, timeline, profile, now);
        self.install_display_for_line(
            &line,
            entrance,
            Reveal {
                visible_from: 0,
                shown: 0,
                segment: None,
                unit_times: None,
                hidden_from: 0,
            },
        );
        self.owner = DisplayOwner::Playback(Box::new(Active {
            line: Some(line),
            player,
            generation: self.generation,
        }));
        self.apply_player(effects, now, fx);
    }

    fn install_display_for_line(&mut self, line: &Line, entrance: Entrance, reveal: Reveal) {
        self.display.line = Some(line.id);
        self.display.doc = Arc::new(line.doc.clone());
        self.display.invalidate_page_cache();
        self.display.speaker = line.speaker.clone();
        self.display.reply = line.reply.as_deref().cloned();
        self.display.exit = None;
        self.display.shake = None;
        self.display.entrance = entrance;
        self.display.replay = self.replay == Some(line.id);
        self.display.empty = false;
        self.display.reveal = reveal;
        self.display.sync_page_cache();
    }

    /// If the display holds non-blank content and the skin uses a presentation
    /// exit, start an exit in `Player` and return `true`.
    fn begin_exit_on_display(&mut self, now: Millis, fx: &mut Effects) -> bool {
        if self.display.empty || self.display.doc.is_blank() {
            return false;
        }
        let ExitStyle::Presentation { ms } =
            self.config.exit_style(&self.skin, self.reduced_motion)
        else {
            return false;
        };
        if ms == 0 {
            return false;
        }
        match &mut self.owner {
            DisplayOwner::Empty => false,
            DisplayOwner::MirrorDraft { doc, .. } => {
                let id = LineId::next();
                let timeline = Arc::new(compile(
                    doc,
                    &PacingParams {
                        typing: false,
                        ..self.pacing
                    },
                    &TypingSoundRule {
                        enabled: false,
                        ..self.sound_rule
                    },
                ));
                let entrance = self.config.entrance_for(doc, &self.skin);
                let profile = self.config.playback_profile(
                    &LineMeta::default(),
                    doc,
                    entrance,
                    &self.skin,
                    self.reduced_motion,
                );
                self.generation += 1;
                let (player, effects) = Player::start_exiting(id, timeline, profile, now);
                self.owner = DisplayOwner::Playback(Box::new(Active {
                    line: None,
                    player,
                    generation: self.generation,
                }));
                self.apply_player(effects, now, fx);
                true
            }
            DisplayOwner::Playback(active) => {
                let (exiting, effects) = active.player.begin_exit(now);
                if exiting {
                    self.cancel_cues(fx);
                    self.apply_player(effects, now, fx);
                }
                exiting
            }
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
        if self.skin.overflow != Overflow::Page || self.mode() == InputMode::Mirror {
            return;
        }
        let auto_ms = self.skin.overflow_auto_ms;
        let Some(active) = self
            .active_mut()
            .filter(|a| a.line.as_ref().is_some_and(|l| l.id == line))
        else {
            return;
        };
        if active.player.phase() == Phase::Exiting {
            return;
        }
        let (touched, effects) = active.player.paginate(breaks, auto_ms, now);
        if touched && let Some(epoch) = self.cue_epoch.take() {
            // Rescheduled from the same origin: talking carries on.
            fx.cancel_epochs.push(epoch);
        }
        self.apply_player(effects, now, fx);
    }

    /// Speech pacing for the active `line` (an exit freezes it).
    fn speech(
        &mut self,
        line: LineId,
        now: Millis,
        fx: &mut Effects,
        f: impl FnOnce(&mut Player) -> (bool, Vec<PlayerEffect>),
    ) {
        let Some(active) = self
            .active_mut()
            .filter(|a| a.line.as_ref().is_some_and(|l| l.id == line))
        else {
            return;
        };
        if active.player.phase() == Phase::Exiting {
            return;
        }
        let (touched, effects) = f(&mut active.player);
        if touched && let Some(epoch) = self.cue_epoch.take() {
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
        let prev = std::mem::replace(&mut self.owner, DisplayOwner::Empty);
        if let DisplayOwner::Playback(active) = prev {
            self.cancel_cues(fx);
            if let Some(line) = active.line {
                self.feed.complete_row(line.id, fx);
            }
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

    fn clear_display(&mut self, now: Millis, fx: &mut Effects) {
        self.queue.clear();
        self.pending_line = None;
        if self.begin_exit_on_display(now, fx) {
            return;
        }
        self.stop_player(fx);
        self.blank_display(fx);
    }

    fn continue_key(&mut self, now: Millis, fx: &mut Effects) {
        let phase = self.active().map(|a| a.player.phase());
        match phase {
            Some(Phase::Exiting) => {
                if let Some(active) = self.active_mut() {
                    let effects = active.player.finish_exit(now);
                    self.apply_player(effects, now, fx);
                }
            }
            Some(Phase::Revealing | Phase::Settling) => {
                if let Some(active) = self.active_mut() {
                    let (_, effects) = active.player.advance(now);
                    self.cancel_cues(fx);
                    self.apply_player(effects, now, fx);
                    self.display.entrance = Entrance {
                        kind: EntranceKind::None,
                        ms: 0,
                    };
                    self.touch_display(fx);
                }
            }
            Some(Phase::Gated) => {
                if self.skin.has_advance_sound {
                    fx.ui_sounds.push(UiSound::Advance);
                }
                if let Some(active) = self.active_mut() {
                    let (_, effects) = active.player.advance(now);
                    self.apply_player(effects, now, fx);
                }
            }
            _ => {
                if let Some(next) = self.queue.pop_next() {
                    if self.skin.has_advance_sound {
                        fx.ui_sounds.push(UiSound::Advance);
                    }
                    self.start_line(next, now, fx);
                }
            }
        }
    }

    fn skip_key(&mut self, now: Millis, fx: &mut Effects) {
        let Some(active) = self.active_mut() else {
            return;
        };
        match active.player.phase() {
            Phase::Exiting => {
                let effects = active.player.finish_exit(now);
                self.apply_player(effects, now, fx);
            }
            Phase::Revealing | Phase::Settling | Phase::Gated => {
                let effects = active.player.finish_reveal(now);
                self.cancel_cues(fx);
                self.apply_player(effects, now, fx);
                self.display.entrance = Entrance {
                    kind: EntranceKind::None,
                    ms: 0,
                };
                self.touch_display(fx);
            }
            _ => {}
        }
    }

    // ---- player effects -------------------------------------------------

    fn apply_player(&mut self, effects: Vec<PlayerEffect>, now: Millis, fx: &mut Effects) {
        let Some(generation) = self.active().map(|a| a.generation) else {
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
                    let Some(active) = self.active() else {
                        continue;
                    };
                    let tl = active.player.timeline().clone();
                    let line_id = active.line_id();
                    let s = &tl.segments[seg];
                    // A schedule shorter than the segment (typing that waits
                    // for speech) hides the units it leaves out.
                    let hidden_from = s.units.end.min(s.units.start + reveal_ms.len() as u32);
                    let reveal = Reveal {
                        visible_from: s.visible_from,
                        shown: s.units.start,
                        segment: Some(SegmentReveal {
                            origin_ms,
                            start: s.units.start,
                            reveal_ms,
                        }),
                        unit_times: None,
                        hidden_from,
                    };
                    self.display.reveal = reveal.clone();
                    self.display.sync_page_cache();
                    self.display.entrance = self.entrance();
                    self.display.exit = None;
                    self.feed.follow_row(line_id, reveal, fx);
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
                PlayerEffect::StartExit { at_ms, style } => {
                    let exit = match style {
                        ExitStyle::Instant => continue,
                        ExitStyle::Fade { ms } => DisplayExit::Fade { at_ms, ms },
                        ExitStyle::Presentation { ms } => DisplayExit::Presentation { at_ms, ms },
                    };
                    self.display.exit = Some(exit);
                    self.display.continue_indicator = false;
                    self.touch_display(fx);
                }
                PlayerEffect::LineRevealed(id) => {
                    if self.config.history_sync == HistorySync::OnComplete
                        && let Some(active) = self.active()
                        && let Some(line) = active.line.clone()
                        && self.replay != Some(id)
                    {
                        let item = self.local_item(&line);
                        self.feed.push_history(item, None, fx);
                    }
                    self.feed.complete_row(id, fx);
                    fx.events.push(Event::LineRevealed(id));
                }
                PlayerEffect::LineFinished(id) => {
                    self.set_speaking(false, fx);
                    let was_committed_line =
                        matches!(&self.owner, DisplayOwner::Playback(a) if a.line.is_some());
                    if was_committed_line {
                        fx.events.push(Event::LineFinished(id));
                    }
                    self.owner = DisplayOwner::Empty;
                    self.cue_epoch = None;
                    self.blank_display(fx);
                    if let Some(next) = self.pending_line.take() {
                        self.start_line(next, now, fx);
                    } else if self.queue.policy == QueuePolicy::Queue
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
                if self.skin.portrait_variants.contains(&name) {
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
}
