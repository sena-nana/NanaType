use std::sync::Arc;

use nanatype_markup::parse;
use nanatype_model::{AfterLine, Document};
use nanatype_playback::{CueKind, Phase, QueuePolicy, WakeReason};
use nanatype_settings::{HistorySync, InputMode, InputPresence, Settings};
use nanatype_theme::SkinRegistry;

use crate::*;

fn skin() -> Arc<nanatype_theme::Skin> {
    SkinRegistry::new(None, "1.0.0")
        .load("nanatype.halo")
        .unwrap()
        .skin
}

fn core_with(f: impl FnOnce(&mut Settings)) -> Core {
    let mut s = Settings::default();
    s.playback.punctuation_pauses = false;
    s.general.speaker_name = "奈奈".into();
    f(&mut s);
    Core::new(s, skin())
}

fn type_and_send(core: &mut Core, markup: &str, now: Millis) -> Effects {
    core.handle(Cmd::DraftChanged(parse(markup).doc), now);
    core.handle(Cmd::Send, now)
}

#[test]
fn account_identity_is_snapshotted_for_queued_lines_and_history() {
    let mut core = core_with(|s| {
        s.history.sync = HistorySync::OnSubmit;
        s.playback.queue = QueuePolicy::Queue;
    });
    let speaker = nanatype_model::Speaker {
        name: "account one".into(),
        avatar: Some("blob:one".into()),
        portrait: None,
    };
    core.handle(Cmd::SetSpeaker(Some(speaker.clone())), 0);
    let first = type_and_send(&mut core, "first", 0);
    let second = type_and_send(&mut core, "second", 1);
    let submitted: Vec<_> = first
        .events
        .iter()
        .chain(&second.events)
        .filter_map(|e| {
            if let Event::LineSubmitted(line) = e {
                Some(line)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(submitted.len(), 2);
    assert!(submitted.iter().all(|line| line.speaker == speaker));
    core.handle(
        Cmd::SetSpeaker(Some(nanatype_model::Speaker {
            name: "account two".into(),
            avatar: None,
            portrait: None,
        })),
        2,
    );
    for row in core.history() {
        assert_eq!(row.item.author, "account one");
        assert_eq!(row.item.avatar.as_deref(), Some("blob:one"));
    }
    core.handle(Cmd::SetSpeaker(None), 3);
    let fx = type_and_send(&mut core, "custom", 4);
    assert!(
        fx.events
            .iter()
            .any(|e| matches!(e,Event::LineSubmitted(line) if line.speaker.name=="奈奈"))
    );
}

#[test]
fn echo_removal_cannot_delete_local_typewriter_rows() {
    let mut core = core_with(|s| s.history.sync = HistorySync::OnSubmit);
    type_and_send(&mut core, "local", 0);
    let local = core.history().back().unwrap().key;
    core.handle(
        Cmd::External(nanatype_model::HistoryItem {
            source: nanatype_model::SourceId("blivechat".into()),
            kind: nanatype_model::SourceKind::Chat,
            line: None,
            author: "viewer".into(),
            avatar: None,
            doc: Document::plain("echo"),
            at: 0,
            uid: Some("7".into()),
            reply: None,
        }),
        1,
    );
    let external = core.history().back().unwrap().key;
    core.handle(Cmd::RemoveHistoryRows(vec![local, external]), 2);
    assert_eq!(core.history().len(), 1);
    assert_eq!(core.history()[0].key, local);
}

/// Deliver every wake due by `until`, in order.
fn run_until(core: &mut Core, mut pending: Vec<Wake>, until: Millis) -> Vec<Effects> {
    let mut out = Vec::new();
    loop {
        pending.sort_by_key(|w| w.at_ms);
        let Some(pos) = pending.iter().position(|w| w.at_ms <= until) else {
            return out;
        };
        let w = pending.remove(pos);
        let fx = core.handle(
            Cmd::Wake {
                epoch: w.epoch,
                reason: w.reason,
            },
            w.at_ms,
        );
        pending.extend(fx.wakes.iter().copied());
        out.push(fx);
    }
}

#[test]
fn sending_starts_the_reveal_clears_the_draft_and_adds_a_following_row() {
    let mut core = core_with(|_| {});
    let fx = type_and_send(&mut core, "你好", 1_000);
    assert_eq!(fx.replace_draft, Some(Document::new()));
    assert!(core.draft().is_blank());
    let d = core.display();
    assert!(!d.empty);
    assert_eq!(d.speaker.name, "奈奈");
    assert_eq!(d.reveal.unit_at(0), Some(1_000));
    assert_eq!(d.reveal.unit_at(1), Some(1_100), "CJK waits two beats");
    assert_eq!(core.history().len(), 1);
    assert!(core.history()[0].reveal.is_some(), "row types along");
    assert!(fx.events.iter().any(|e| matches!(e, Event::LineStarted(_))));
    let fx = run_until(&mut core, fx.wakes, 10_000);
    assert!(
        fx.iter()
            .any(|f| f.events.iter().any(|e| matches!(e, Event::LineRevealed(_))))
    );
    assert!(
        core.history()[0]
            .reveal
            .as_ref()
            .is_none_or(|r| r.hidden_from == 2)
    );
    assert_eq!(core.phase(), Some(Phase::Done));
}

#[test]
fn blank_drafts_do_not_send() {
    let mut core = core_with(|_| {});
    let fx = type_and_send(&mut core, "  [wait=100]", 0);
    assert!(fx.replace_draft.is_none());
    assert!(core.display().empty);
    assert!(core.history().is_empty());
}

#[test]
fn submission_events_include_queued_quick_phrases_and_mirror_commits_once() {
    let mut core = core_with(|s| s.playback.queue = QueuePolicy::Queue);
    type_and_send(&mut core, "first", 0);
    let fx = core.handle(Cmd::SendMarkup("queued".into()), 1);
    assert_eq!(
        fx.events
            .iter()
            .filter(|e| matches!(e, Event::LineSubmitted(_)))
            .count(),
        1
    );
    assert!(!fx.events.iter().any(|e| matches!(e, Event::LineStarted(_))));
    let mut mirror = core_with(|s| s.general.input_mode = InputMode::Mirror);
    let fx = type_and_send(&mut mirror, "mirror", 10);
    assert_eq!(
        fx.events
            .iter()
            .filter(|e| matches!(e, Event::LineSubmitted(_)))
            .count(),
        1
    );
}

#[test]
fn continue_passes_a_pause_and_skips_a_segment() {
    let mut core = core_with(|_| {});
    let fx = type_and_send(&mut core, "abcdef[pause]gh", 0);
    // Mid-reveal: continue completes the segment and gates.
    let fx2 = core.handle(Cmd::Continue, 60);
    assert!(fx2.cancel_epochs.len() <= 1);
    assert_eq!(core.phase(), Some(Phase::Gated));
    assert!(core.display().continue_indicator);
    // Old wakes are now stale.
    run_until(&mut core, fx.wakes, 10_000);
    assert_eq!(core.phase(), Some(Phase::Gated));
    let fx3 = core.handle(Cmd::Continue, 5_000);
    assert_eq!(core.phase(), Some(Phase::Revealing));
    assert_eq!(core.display().reveal.unit_at(6), Some(5_000));
    run_until(&mut core, fx3.wakes, 20_000);
    assert_eq!(core.phase(), Some(Phase::Done));
}

#[test]
fn summon_mode_hides_the_input_after_sending() {
    let mut core = core_with(|s| s.general.input_presence = InputPresence::Summon);
    assert!(type_and_send(&mut core, "x", 0).hide_input);
}

#[test]
fn fade_after_line_blanks_the_display() {
    let mut core = core_with(|s| {
        s.playback.after = AfterLine::Fade {
            delay_ms: 500,
            per_unit_ms: 0,
        }
    });
    let fx = type_and_send(&mut core, "a", 0);
    let all = run_until(&mut core, fx.wakes, 100_000);
    assert!(
        all.iter()
            .any(|f| f.events.iter().any(|e| matches!(e, Event::LineFinished(_))))
    );
    assert!(core.display().empty);
    assert_eq!(core.history().len(), 1, "history keeps the line");
}

#[test]
fn queue_policy_holds_lines_until_continue() {
    let mut core = core_with(|s| s.playback.queue = QueuePolicy::Queue);
    let fx = type_and_send(&mut core, "first", 0);
    let fx2 = type_and_send(&mut core, "second", 10);
    assert!(
        fx2.events
            .iter()
            .any(|e| matches!(e, Event::Queued { position: 1 }))
    );
    assert_eq!(
        core.history().len(),
        1,
        "queued lines are not in history yet"
    );
    run_until(&mut core, fx.wakes, 10_000);
    assert_eq!(core.phase(), Some(Phase::Done));
    core.handle(Cmd::Continue, 20_000);
    assert_eq!(core.display().doc.plain_text(), "second");
    assert_eq!(core.history().len(), 2);
}

#[test]
fn replace_policy_interrupts_and_completes_the_old_row() {
    let mut core = core_with(|_| {});
    type_and_send(&mut core, "long line here", 0);
    let fx = type_and_send(&mut core, "next", 10);
    assert!(!fx.cancel_epochs.is_empty() || fx.cues.is_empty());
    assert_eq!(core.display().doc.plain_text(), "next");
    let first = &core.history()[0];
    assert_eq!(
        first.reveal.as_ref().map(|r| r.hidden_from),
        first.reveal.as_ref().map(|_| 14),
        "the interrupted row shows its whole text"
    );
    let reveal = first.reveal.as_ref().unwrap();
    assert!(reveal.unit_at(13).is_some(), "the tail is not hidden");
}

#[test]
fn an_interrupted_multi_segment_line_shows_its_tail_in_history() {
    let mut core = core_with(|_| {});
    type_and_send(&mut core, "ab[pause]cd", 0);
    type_and_send(&mut core, "next", 10);
    let reveal = core.history()[0].reveal.clone();
    if let Some(r) = reveal {
        assert!(r.unit_at(3).is_some());
    }
}

#[test]
fn history_sync_on_submit_and_on_complete() {
    let mut core = core_with(|s| s.history.sync = HistorySync::OnSubmit);
    type_and_send(&mut core, "x", 0);
    assert!(core.history()[0].reveal.is_none());

    let mut core = core_with(|s| s.history.sync = HistorySync::OnComplete);
    let fx = type_and_send(&mut core, "xy", 0);
    assert!(core.history().is_empty());
    run_until(&mut core, fx.wakes, 10_000);
    assert_eq!(core.history().len(), 1);
}

fn viewer() -> nanatype_model::ChatRef {
    nanatype_model::ChatRef::new(
        nanatype_model::ChatUser {
            uid: "42".into(),
            name: "观众".into(),
            avatar: None,
        },
        "主播  晚上好\n呀",
        nanatype_model::SourceKind::Chat,
    )
}

#[test]
fn a_reply_rides_on_the_next_sent_line_only() {
    let mut core = core_with(|_| {});
    let fx = core.handle(Cmd::SetReply(Some(viewer())), 0);
    assert!(fx.reply_changed);
    assert_eq!(core.pending_reply().unwrap().text, "主播 晚上好 呀");
    let fx = type_and_send(&mut core, "晚上好", 100);
    assert!(fx.reply_changed, "the send used the reply");
    assert!(core.pending_reply().is_none());
    assert_eq!(core.display().reply.as_ref().unwrap().user.name, "观众");
    assert_eq!(
        core.history()[0].item.reply.as_ref().unwrap().user.uid,
        "42"
    );
    let fx = type_and_send(&mut core, "下一句", 5_000);
    assert!(!fx.reply_changed);
    assert!(core.display().reply.is_none());
    assert!(core.history()[1].item.reply.is_none());
}

#[test]
fn quick_phrases_and_a_dropped_reply_send_without_a_quote() {
    let mut core = core_with(|_| {});
    core.handle(Cmd::SetReply(Some(viewer())), 0);
    core.handle(Cmd::SendMarkup("你好".into()), 10);
    assert!(core.display().reply.is_none());
    assert!(
        core.pending_reply().is_some(),
        "a phrase leaves the reply pending"
    );
    let fx = core.handle(Cmd::SetReply(None), 20);
    assert!(fx.reply_changed);
    type_and_send(&mut core, "嗯", 5_000);
    assert!(core.display().reply.is_none());
}

#[test]
fn mirror_mode_shows_the_pending_reply_while_typing() {
    let mut core = core_with(|s| s.general.input_mode = InputMode::Mirror);
    core.handle(Cmd::DraftChanged(Document::plain("哈")), 0);
    core.handle(Cmd::SetReply(Some(viewer())), 10);
    assert!(core.display().reply.is_some());
    core.handle(Cmd::Send, 20);
    assert!(
        core.display().reply.is_some(),
        "the committed line keeps it"
    );
    assert!(core.history()[0].item.reply.is_some());
}

#[test]
fn chat_rows_join_the_history_with_their_uid() {
    let mut core = core_with(|_| {});
    let fx = core.handle(
        Cmd::External(nanatype_model::HistoryItem {
            source: nanatype_model::SourceId("blivechat".into()),
            kind: nanatype_model::SourceKind::Chat,
            line: None,
            author: "观众".into(),
            avatar: Some("blob:blive-face-42".into()),
            doc: Document::plain("草"),
            at: 0,
            uid: Some("42".into()),
            reply: None,
        }),
        0,
    );
    assert!(fx.history_changed);
    assert_eq!(core.history()[0].item.uid.as_deref(), Some("42"));
}

#[test]
fn mirror_mode_follows_typing_including_repeats_and_commits_on_send() {
    let mut core = core_with(|s| s.general.input_mode = InputMode::Mirror);
    core.handle(Cmd::DraftChanged(Document::plain("哈")), 0);
    assert_eq!(core.display().reveal.unit_at(0), Some(0));
    core.handle(Cmd::DraftChanged(Document::plain("哈哈")), 100);
    let r = &core.display().reveal;
    assert_eq!(r.shown, 1, "first character stays");
    assert_eq!(r.unit_at(1), Some(100), "the repeat reveals");
    let fx = core.handle(Cmd::Send, 200);
    assert_eq!(fx.replace_draft, Some(Document::new()));
    assert_eq!(core.history().len(), 1);
    // The editor echoes the cleared draft; the committed line stays.
    core.handle(Cmd::DraftChanged(Document::new()), 210);
    assert_eq!(core.display().doc.plain_text(), "哈哈");
    // New typing replaces it.
    core.handle(Cmd::DraftChanged(Document::plain("嗯")), 300);
    assert_eq!(core.display().doc.plain_text(), "嗯");
}

#[test]
fn recall_walks_sent_lines_and_restores_the_stash() {
    let mut core = core_with(|_| {});
    type_and_send(&mut core, "one", 0);
    type_and_send(&mut core, "two", 10);
    core.handle(Cmd::DraftChanged(Document::plain("draft")), 20);
    let fx = core.handle(Cmd::Recall(-1), 30);
    assert_eq!(fx.replace_draft.unwrap().plain_text(), "two");
    let fx = core.handle(Cmd::Recall(-1), 31);
    assert_eq!(fx.replace_draft.unwrap().plain_text(), "one");
    let fx = core.handle(Cmd::Recall(-1), 32);
    assert_eq!(
        fx.replace_draft.unwrap().plain_text(),
        "one",
        "stays at the oldest"
    );
    core.handle(Cmd::Recall(1), 33);
    let fx = core.handle(Cmd::Recall(1), 34);
    assert_eq!(fx.replace_draft.unwrap().plain_text(), "draft");
}

#[test]
fn expression_cues_switch_portraits_and_shakes_show() {
    let mut core = core_with(|_| {});
    type_and_send(&mut core, "a", 0);
    let fx = core.handle(
        Cmd::CueFired(CueKind::Expression {
            name: "smile".into(),
            intensity: None,
        }),
        10,
    );
    assert!(
        fx.events
            .iter()
            .any(|e| matches!(e, Event::Expression { .. }))
    );
    assert_eq!(
        core.display().portrait,
        None,
        "halo has no portrait variants"
    );
    core.handle(
        Cmd::CueFired(CueKind::Portrait {
            variant: "happy".into(),
        }),
        20,
    );
    assert_eq!(core.display().portrait.as_deref(), Some("happy"));
    core.handle(CmdShake::cmd(), 30);
    assert!(core.display().shake.is_some());
}

struct CmdShake;
impl CmdShake {
    fn cmd() -> Cmd {
        Cmd::CueFired(CueKind::Shake { amp: 4.0, ms: 200 })
    }
}

#[test]
fn skip_reveals_everything_and_clear_blanks() {
    let mut core = core_with(|_| {});
    type_and_send(&mut core, "abc[pause]def", 0);
    core.handle(Cmd::Skip, 5);
    assert_eq!(core.phase(), Some(Phase::Done));
    assert_eq!(core.display().reveal.unit_at(5), Some(5));
    core.handle(Cmd::ClearDisplay, 6);
    assert!(core.display().empty);
    assert_eq!(core.phase(), None);
}

#[test]
fn quick_phrases_send_markup_with_line_meta() {
    let mut core = core_with(|_| {});
    core.handle(Cmd::DraftChanged(Document::plain("keep me")), 0);
    core.handle(Cmd::SendMarkup("[@after=clear delay=100]谢谢！".into()), 1);
    assert_eq!(core.display().doc.plain_text(), "谢谢！");
    assert_eq!(core.draft().plain_text(), "keep me");
}

#[test]
fn switching_modes_clears_the_queue_and_display() {
    let mut core = core_with(|_| {});
    type_and_send(&mut core, "x", 0);
    let mut s = core.settings().clone();
    s.general.input_mode = InputMode::Mirror;
    core.handle(Cmd::SetSettings(Box::new(s)), 5);
    assert_eq!(core.phase(), None);
    assert!(core.display().empty);
}

#[test]
fn mirror_recall_and_restore_publish_the_editor_contents_without_submitting() {
    let mut core = core_with(|s| s.general.input_mode = InputMode::Mirror);
    type_and_send(&mut core, "上一句", 0);
    core.handle(Cmd::DraftChanged(Document::plain("新草稿")), 1);
    for (direction, expected) in [(-1, "上一句"), (1, "新草稿")] {
        let fx = core.handle(Cmd::Recall(direction), 2);
        assert!(fx.display_changed);
        assert!(
            fx.events
                .iter()
                .all(|e| !matches!(e, Event::LineSubmitted(_)))
        );
        assert_eq!(core.draft().plain_text(), expected);
        assert_eq!(core.display().doc.plain_text(), expected);
        assert!(core.display().line.is_none());
        core.handle(Cmd::DraftChanged(fx.replace_draft.unwrap()), 3);
        assert_eq!(core.display().doc.plain_text(), expected);
    }
    assert_eq!(core.history().len(), 1);
}

#[test]
fn recall_restores_line_headers_and_the_stashed_drafts_header() {
    let mut core = core_with(|_| {});
    let original = parse("[@auto=200 after=clear delay=100][b]清屏台词[/b]");
    core.handle(Cmd::DraftMeta(original.meta), 0);
    core.handle(Cmd::DraftChanged(original.doc.clone()), 0);
    core.handle(Cmd::Send, 1);
    let stashed = parse("[@after=fade delay=500 per=20]暂存草稿");
    core.handle(Cmd::DraftMeta(stashed.meta), 2);
    core.handle(Cmd::DraftChanged(stashed.doc.clone()), 2);
    for (direction, expected) in [(-1, &original), (1, &stashed), (-1, &original)] {
        let fx = core.handle(Cmd::Recall(direction), 3);
        let source =
            nanatype_markup::serialize_line(&fx.replace_draft.unwrap(), &core.draft_meta());
        let restored = parse(&source);
        assert_eq!(restored.doc, expected.doc);
        assert_eq!(restored.meta, expected.meta);
        // Same host handoff as App::apply_effects -> reparse.
        core.handle(Cmd::DraftMeta(restored.meta), 4);
        core.handle(Cmd::DraftChanged(restored.doc), 4);
    }
    let fx = core.handle(Cmd::Send, 5);
    assert!(
        fx.events
            .iter()
            .any(|e| matches!(e, Event::LineSubmitted(line) if line.meta == original.meta))
    );
    assert_eq!(core.draft_meta(), Default::default());
}

#[test]
fn mode_round_trips_and_edited_recall_preserve_sends_and_headers() {
    let mut core = core_with(|_| {});
    let original = parse("[@auto=250 after=clear delay=100][b]初稿[/b][pause]尾声");
    core.handle(Cmd::DraftMeta(original.meta), 0);
    core.handle(Cmd::DraftChanged(original.doc.clone()), 0);
    for (at, mode) in [InputMode::Mirror, InputMode::Compose, InputMode::Mirror]
        .into_iter()
        .enumerate()
    {
        let mut s = core.settings().clone();
        s.general.input_mode = mode;
        core.handle(Cmd::SetSettings(Box::new(s)), at as u64 + 1);
        assert_eq!(core.draft(), &original.doc);
        assert_eq!(core.draft_meta(), original.meta);
        if mode == InputMode::Mirror {
            assert_eq!(core.display().doc.as_ref(), &original.doc);
        }
    }
    let fx = core.handle(Cmd::Send, 4);
    assert!(fx.events.iter().any(|e| matches!(e, Event::LineSubmitted(line) if line.doc == original.doc && line.meta == original.meta)));
    assert_eq!(
        fx.events
            .iter()
            .filter(|e| matches!(e, Event::LineSubmitted(_)))
            .count(),
        1
    );
    assert_eq!(core.history().len(), 1);
    assert!(core.draft().is_blank());
    assert!(core.handle(Cmd::Send, 4).events.is_empty());
    core.handle(Cmd::DraftChanged(Document::plain("暂存草稿")), 5);
    core.handle(Cmd::Recall(-1), 6);
    let edited = parse("[i]回调后修改[/i]").doc;
    core.handle(Cmd::DraftChanged(edited.clone()), 7);
    assert_eq!(core.display().doc.as_ref(), &edited);
    assert!(
        core.handle(Cmd::Recall(1), 8).replace_draft.is_none(),
        "editing should leave recall navigation"
    );
    let fx = core.handle(Cmd::Send, 9);
    assert!(fx.events.iter().any(|e| matches!(e, Event::LineSubmitted(line) if line.doc == edited && line.meta == original.meta)));
    assert_eq!(core.history().len(), 2);
    assert!(core.draft().is_blank());
}

#[test]
fn stale_wakes_from_an_older_line_are_ignored() {
    let mut core = core_with(|_| {});
    let fx = type_and_send(&mut core, "aaaa", 0);
    type_and_send(&mut core, "bbbbbbbbbbbbbbbbbbb", 10);
    for w in fx.wakes {
        let out = core.handle(
            Cmd::Wake {
                epoch: w.epoch,
                reason: WakeReason::SegmentEnd,
            },
            w.at_ms,
        );
        assert!(out == Effects::default());
    }
    assert_eq!(core.phase(), Some(Phase::Revealing));
}

#[test]
fn a_page_break_starts_the_next_page_at_the_top() {
    let mut core = core_with(|_| {});
    type_and_send(&mut core, "abc[page]\ndef", 0);
    core.handle(Cmd::Continue, 10);
    assert_eq!(core.phase(), Some(Phase::Gated));
    core.handle(Cmd::Continue, 1_000);
    let (doc, reveal) = core.display().page();
    assert_eq!(doc.plain_text(), "def");
    assert_eq!(reveal.visible_from, 0);
    assert_eq!(reveal.unit_at(0), core.display().reveal.unit_at(3));
    assert_eq!(reveal.unit_at(2), core.display().reveal.unit_at(5));
}

#[test]
fn rebasing_a_reveal_shifts_every_unit_by_the_page_start() {
    let reveal = Reveal {
        visible_from: 3,
        shown: 4,
        segment: Some(SegmentReveal {
            origin_ms: 100,
            start: 2,
            reveal_ms: vec![0, 10, 20, 30, 40].into(),
        }),
        hidden_from: 7,
    };
    let page = reveal.rebased();
    for u in 3..9 {
        assert_eq!(page.unit_at(u - 3), reveal.unit_at(u), "unit {u}");
    }
    let full = Reveal::full(5);
    assert_eq!(full.rebased(), full);
}

#[test]
fn skip_fires_the_expressions_it_jumps_over() {
    let mut core = core_with(|_| {});
    type_and_send(&mut core, "abc[expr=smile]def[pause]gh[expr=sad]i", 0);
    let fx = core.handle(Cmd::Skip, 5);
    let names: Vec<_> = fx
        .fire_now
        .iter()
        .filter_map(|c| match &c.kind {
            CueKind::Expression { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["smile", "sad"]);
}

#[test]
fn cancelled_lines_stop_speaking() {
    let mut core = core_with(|_| {});
    type_and_send(&mut core, "abcdef", 0);
    let fx = core.handle(Cmd::CueFired(CueKind::SpeakingStart), 1);
    assert!(fx.events.contains(&Event::Speaking(true)));
    let again = core.handle(Cmd::CueFired(CueKind::SpeakingStart), 2);
    assert!(again.events.is_empty(), "no repeat while already speaking");
    let fx = core.handle(Cmd::Skip, 3);
    assert!(fx.events.contains(&Event::Speaking(false)));
    let fx = core.handle(Cmd::ClearDisplay, 4);
    assert!(
        !fx.events.contains(&Event::Speaking(false)),
        "already stopped"
    );
}

#[test]
fn typed_line_headers_reach_the_sent_line() {
    let mut core = core_with(|_| {});
    let parsed = parse("[@after=clear delay=100]hi");
    core.handle(Cmd::DraftMeta(parsed.meta), 0);
    core.handle(Cmd::DraftChanged(parsed.doc), 0);
    let fx = core.handle(Cmd::Send, 1);
    let line = fx
        .events
        .iter()
        .find_map(|e| match e {
            Event::LineStarted(l) => Some(l.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(line.meta, parsed.meta);
    assert_ne!(line.meta, Default::default());
    // Used once.
    let fx = type_and_send(&mut core, "again", 2);
    assert!(fx.events.iter().any(|e| matches!(
        e,
        Event::LineStarted(l) if l.meta == Default::default()
    )));
}

fn paged_core(overflow: nanatype_model::Overflow) -> Core {
    let mut s = Settings::default();
    s.playback.punctuation_pauses = false;
    let mut skin = (*skin()).clone();
    skin.dialogue.overflow = overflow;
    skin.dialogue.overflow_auto_ms = None;
    Core::new(s, Arc::new(skin))
}

fn started_line(fx: &Effects) -> nanatype_model::LineId {
    fx.events
        .iter()
        .find_map(|e| match e {
            Event::LineStarted(l) => Some(l.id),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_full_box_turns_the_page_where_the_host_measured_it() {
    let mut core = paged_core(nanatype_model::Overflow::Page);
    let fx = type_and_send(&mut core, "abcdefgh", 0);
    let line = started_line(&fx);
    let other = core.handle(
        Cmd::PageBreaks {
            line: nanatype_model::LineId(line.0 + 100),
            breaks: vec![4],
        },
        10,
    );
    assert!(other.wakes.is_empty(), "another line's breaks are ignored");
    let fx2 = core.handle(
        Cmd::PageBreaks {
            line,
            breaks: vec![4],
        },
        10,
    );
    assert_eq!(fx2.cancel_epochs.len(), fx.cues.len().min(1));
    assert_eq!(core.display().reveal.hidden_from, 4);
    run_until(&mut core, fx2.wakes, 10_000);
    assert_eq!(core.phase(), Some(Phase::Gated));
    assert!(core.display().continue_indicator);
    core.handle(Cmd::Continue, 20_000);
    assert_eq!(core.display().reveal.visible_from, 4);
    assert_eq!(core.display().page().0.plain_text(), "efgh");
}

#[test]
fn page_breaks_need_the_page_overflow() {
    let mut core = paged_core(nanatype_model::Overflow::Scroll);
    let fx = type_and_send(&mut core, "abcdefgh", 0);
    let line = started_line(&fx);
    core.handle(
        Cmd::PageBreaks {
            line,
            breaks: vec![4],
        },
        10,
    );
    assert_eq!(core.display().reveal.hidden_from, 8);
}

fn crayon_core() -> Core {
    let skin = SkinRegistry::new(None, "1.0.0")
        .load("nanatype.crayon")
        .unwrap()
        .skin;
    Core::new(Settings::default(), skin)
}
fn retire_wake(fx: &Effects) -> Wake {
    *fx.wakes
        .iter()
        .find(|w| w.reason == WakeReason::PresentationDone)
        .expect("finite retirement deadline")
}
#[test]
fn crayon_replacement_defers_all_new_events_and_cancels_old_cues() {
    let mut core = crayon_core();
    type_and_send(&mut core, "旧台词很长", 1000);
    let old = core.display().line;
    let fx = type_and_send(&mut core, "新[ruby=しん]台词[/ruby]", 1050);
    let wake = retire_wake(&fx);
    assert_eq!(wake.at_ms, 1370);
    assert_eq!(core.display().line, old);
    assert!(core.display().text_exit.is_some());
    assert!(fx.cues.is_empty());
    assert!(!fx.events.iter().any(|e| matches!(e, Event::LineStarted(_))));
    assert!(!fx.cancel_epochs.is_empty());
    let fx = core.handle(
        Cmd::Wake {
            epoch: wake.epoch,
            reason: wake.reason,
        },
        1370,
    );
    assert_ne!(core.display().line, old);
    assert!(core.display().text_exit.is_none());
    assert!(fx.events.iter().any(|e| matches!(e, Event::LineStarted(_))));
    assert!(fx.cues.iter().all(|c| c.origin_ms >= 1370));
    assert_eq!(core.display().reveal.unit_at(0), Some(1370));
    let current = core.display().clone();
    core.handle(
        Cmd::Wake {
            epoch: wake.epoch,
            reason: wake.reason,
        },
        1500,
    );
    assert_eq!(
        *core.display(),
        current,
        "stale retirement changed the new line"
    );
}
#[test]
fn crayon_clear_is_idempotent_and_continue_finishes_retirement() {
    let mut core = crayon_core();
    type_and_send(&mut core, "保留到擦完", 1000);
    let fx = core.handle(Cmd::ClearDisplay, 1080);
    let wake = retire_wake(&fx);
    let exit = core.display().text_exit;
    assert!(!core.display().empty);
    let fx = core.handle(Cmd::ClearDisplay, 1100);
    assert!(fx.wakes.is_empty());
    assert_eq!(core.display().text_exit, exit);
    core.handle(Cmd::Continue, 1110);
    assert!(core.display().empty);
    core.handle(
        Cmd::Wake {
            epoch: wake.epoch,
            reason: wake.reason,
        },
        wake.at_ms,
    );
    assert!(core.display().empty);
}
#[test]
fn crayon_repeated_replacements_keep_deadline_and_only_start_latest() {
    let mut core = crayon_core();
    type_and_send(&mut core, "第一句", 1000);
    let fx = type_and_send(&mut core, "第二句", 1050);
    let wake = retire_wake(&fx);
    let fx = type_and_send(&mut core, "第三句", 1100);
    assert!(fx.cues.is_empty());
    assert!(fx.wakes.is_empty());
    let fx = core.handle(Cmd::Skip, 1110);
    assert_eq!(core.display().doc.plain_text(), "第三句");
    assert_eq!(
        fx.events
            .iter()
            .filter(|e| matches!(e, Event::LineStarted(_)))
            .count(),
        1
    );
    assert!(
        core.handle(
            Cmd::Wake {
                epoch: wake.epoch,
                reason: wake.reason
            },
            1370
        )
        .events
        .is_empty()
    );
}
#[test]
fn crayon_page_waits_for_erase_and_accessibility_removes_wait() {
    let mut core = crayon_core();
    type_and_send(&mut core, "甲[page]乙", 1000);
    core.handle(Cmd::Continue, 1100); // complete first segment
    let fx = core.handle(Cmd::Continue, 1200);
    let wake = retire_wake(&fx);
    assert_eq!(core.display().reveal.visible_from, 0);
    let fx = core.handle(
        Cmd::Wake {
            epoch: wake.epoch,
            reason: wake.reason,
        },
        1520,
    );
    assert_eq!(core.display().reveal.visible_from, 1);
    assert_eq!(core.display().reveal.unit_at(1), Some(1520));
    assert!(fx.cues.iter().all(|c| c.origin_ms >= 1520));
    core.handle(Cmd::SetReducedMotion(true), 1600);
    assert!(core.handle(Cmd::ClearDisplay, 1610).wakes.is_empty());
    assert!(core.display().empty);
}

#[test]
fn crayon_completion_waits_for_last_glyph_mask_and_after_line_erase() {
    let mut core = crayon_core();
    let fx = type_and_send(&mut core, "字", 1000);
    let wake = *fx
        .wakes
        .iter()
        .find(|w| w.reason == WakeReason::SegmentEnd)
        .unwrap();
    let fx = core.handle(
        Cmd::Wake {
            epoch: wake.epoch,
            reason: wake.reason,
        },
        wake.at_ms,
    );
    assert_eq!(core.phase(), Some(Phase::Revealing));
    let tail = fx.wakes[0];
    assert_eq!(tail.at_ms, 1180);
    let fx = core.handle(
        Cmd::Wake {
            epoch: tail.epoch,
            reason: tail.reason,
        },
        tail.at_ms,
    );
    assert!(
        fx.events
            .iter()
            .any(|e| matches!(e, Event::LineRevealed(_)))
    );
    let mut settings = core.settings().clone();
    settings.playback.after = AfterLine::Clear { delay_ms: 10 };
    core.handle(Cmd::SetSettings(Box::new(settings)), 1200);
    let fx = type_and_send(&mut core, "下一句", 1210);
    let retirement = retire_wake(&fx);
    let fx = core.handle(
        Cmd::Wake {
            epoch: retirement.epoch,
            reason: retirement.reason,
        },
        retirement.at_ms,
    );
    assert!(fx.events.iter().any(|e| matches!(e, Event::LineStarted(_))));
}

#[test]
fn crayon_skip_finishes_glyph_entry_and_stale_auto_page_wakes_are_ignored() {
    let mut core = crayon_core();
    type_and_send(&mut core, "旧[page]页", 1000);
    core.handle(Cmd::Continue, 1010);
    core.handle(
        Cmd::Wake {
            epoch: 0,
            reason: WakeReason::AutoContinue,
        },
        1020,
    );
    assert!(core.display().text_exit.is_none());
    core.handle(Cmd::Skip, 1030);
    assert_eq!(
        core.display().entrance.kind,
        nanatype_model::EntranceKind::None
    );
}

#[test]
fn crayon_queue_does_not_replace_the_line_already_waiting_for_erasure() {
    let mut settings = Settings::default();
    settings.playback.queue = QueuePolicy::Queue;
    settings.history.sync = HistorySync::OnSubmit;
    let skin = SkinRegistry::new(None, "1.0.0")
        .load("nanatype.crayon")
        .unwrap()
        .skin;
    let mut core = Core::new(settings, skin);
    type_and_send(&mut core, "甲", 1000);
    core.handle(Cmd::Skip, 1010);
    let fx = type_and_send(&mut core, "乙", 1100);
    let wake = retire_wake(&fx);
    let fx = type_and_send(&mut core, "丙", 1150);
    assert!(fx.events.iter().any(|e| matches!(e, Event::Queued { .. })));
    assert_eq!(core.history().len(), 1);
    core.handle(
        Cmd::Wake {
            epoch: wake.epoch,
            reason: wake.reason,
        },
        1420,
    );
    assert_eq!(core.display().doc.plain_text(), "乙");
    assert_eq!(core.queued(), 1);
    assert_eq!(core.history().len(), 2);
}
#[test]
fn crayon_mirror_edit_cancels_old_retirement_and_deletes_immediately() {
    let mut settings = Settings::default();
    settings.general.input_mode = InputMode::Mirror;
    let skin = SkinRegistry::new(None, "1.0.0")
        .load("nanatype.crayon")
        .unwrap()
        .skin;
    let mut core = Core::new(settings, skin);
    core.handle(Cmd::DraftChanged(parse("蜡笔文字").doc), 1000);
    let fx = core.handle(Cmd::ClearDisplay, 1100);
    let wake = retire_wake(&fx);
    core.handle(Cmd::DraftChanged(parse("蜡笔").doc), 1110);
    assert!(core.display().text_exit.is_none());
    assert_eq!(core.display().doc.plain_text(), "蜡笔");
    core.handle(
        Cmd::Wake {
            epoch: wake.epoch,
            reason: wake.reason,
        },
        1420,
    );
    assert_eq!(core.display().doc.plain_text(), "蜡笔");
    assert!(!core.display().empty);
}

#[test]
fn saving_skin_typography_and_dimensions_preserves_playback_progress() {
    let mut core = core_with(|s| s.general.input_mode = InputMode::Compose);
    type_and_send(&mut core, "保留逐字播放进度 Hello NanaType!", 1000);
    let before = core.display().page();
    let before = (before.0.clone(), before.1.clone());
    let phase = core.phase();
    let mut settings = core.settings().clone();
    let mut profile = nanatype_settings::SkinPersonalization::default();
    profile.display.size = Some([160.0, 96.0]);
    profile.display.text_size = Some(48.0);
    profile.history.line_height = Some(2.0);
    settings
        .skin
        .personalizations
        .insert(settings.skin.id.clone(), profile);
    core.handle(Cmd::SetSettings(Box::new(settings)), 1010);
    let mut modified_skin = (*skin()).clone();
    modified_skin.dialogue.text.size = 48.0;
    core.handle(Cmd::SetSkin(Arc::new(modified_skin)), 1010);
    let after = core.display().page();
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, before.1);
    assert_eq!(core.phase(), phase);
}
