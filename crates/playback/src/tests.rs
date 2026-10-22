use std::sync::Arc;

use nanatype_markup::parse;
use nanatype_model::{AfterLine, Document, Line, LineId, Speaker};

use crate::*;

fn no_punct() -> PacingParams {
    PacingParams {
        punctuation_pauses: false,
        ..PacingParams::default()
    }
}

fn quiet() -> TypingSoundRule {
    TypingSoundRule {
        enabled: false,
        ..TypingSoundRule::default()
    }
}

fn tl(markup: &str, pacing: &PacingParams) -> Arc<Timeline> {
    Arc::new(compile(&parse(markup).doc, pacing, &quiet()))
}

fn wakes(effects: &[PlayerEffect]) -> Vec<(u64, WakeReason)> {
    effects
        .iter()
        .filter_map(|e| match e {
            PlayerEffect::WakeAt { at_ms, reason, .. } => Some((*at_ms, *reason)),
            _ => None,
        })
        .collect()
}

#[test]
fn reveal_waits_a_beat_and_cjk_waits_two() {
    let t = tl("a你b", &no_punct());
    let s = &t.segments[0];
    assert_eq!(&*s.reveal_ms, &[0, 50, 150]);
    assert_eq!(s.duration_ms, 200);
    assert_eq!(s.end, SegmentEnd::LineEnd);
}

#[test]
fn punctuation_adds_its_pause() {
    let t = tl("a，b。c", &PacingParams::default());
    assert_eq!(&*t.segments[0].reveal_ms, &[0, 50, 220, 270, 580]);
}

#[test]
fn wait_speed_and_instant_change_the_clock() {
    let t = tl(
        "a[wait=400]b[speed=2]cd[/speed][instant]efg[/instant]h",
        &no_punct(),
    );
    let r = &t.segments[0].reveal_ms;
    assert_eq!(r[0], 0);
    assert_eq!(r[1], 450, "a's beat plus the wait");
    assert_eq!(r[2], 500);
    assert_eq!(r[3], 600, "half speed doubles the beat");
    assert_eq!(r[4], 700);
    assert_eq!(r[5], 700, "instant span appears together");
    assert_eq!(r[6], 700);
    assert_eq!(r[7], 700);
    let fixed = tl("[speed=10ms]你好[/speed]", &no_punct());
    assert_eq!(&*fixed.segments[0].reveal_ms, &[0, 10]);
}

#[test]
fn emphasis_waits_before_and_after_each_character() {
    let r = &tl("a[em]bc[/em]d", &no_punct()).segments[0].reveal_ms;
    assert_eq!(&**r, &[0, 150, 400, 550], "lead-in, both sides, trailing");
    let spaced = &tl("[em=20]b c[/em]", &no_punct()).segments[0].reveal_ms;
    assert_eq!(&**spaced, &[20, 90, 160], "spaces get no wait of their own");
    let off = PacingParams {
        typing: false,
        ..no_punct()
    };
    assert_eq!(&*tl("a[em]bc[/em]", &off).segments[0].reveal_ms, &[0, 0, 0]);
}

#[test]
fn global_speed_scale_and_typing_off() {
    let fast = PacingParams {
        speed_scale: 0.5,
        ..no_punct()
    };
    assert_eq!(&*tl("ab", &fast).segments[0].reveal_ms, &[0, 25]);
    let off = PacingParams {
        typing: false,
        ..no_punct()
    };
    let t = tl("ab[wait=900]c", &off);
    assert_eq!(&*t.segments[0].reveal_ms, &[0, 0, 0]);
    assert_eq!(t.segments[0].duration_ms, 0);
}

#[test]
fn pauses_split_segments_and_wait_for_continue() {
    let t = tl("ab[pause]cd", &no_punct());
    assert_eq!(t.segments.len(), 2);
    assert_eq!(t.segments[0].units, 0..2);
    assert_eq!(t.segments[1].units, 2..4);
    let (mut p, fx) = Player::start(LineId(1), t, AfterLine::Hold, None, 1_000);
    assert_eq!(wakes(&fx), vec![(1_100, WakeReason::SegmentEnd)]);
    let fx = p.on_wake(p.epoch(), WakeReason::SegmentEnd, 1_100);
    assert!(fx.contains(&PlayerEffect::ContinueIndicator(true)));
    assert_eq!(p.phase(), Phase::Gated);
    assert_eq!(p.snapshot(5_000).revealed, 2);
    let (did, fx) = p.advance(6_000);
    assert!(did);
    assert!(matches!(
        fx.iter()
            .find(|e| matches!(e, PlayerEffect::ShowSegment { .. })),
        Some(PlayerEffect::ShowSegment {
            seg: 1,
            origin_ms: 6_000,
            ..
        })
    ));
    assert_eq!(p.snapshot(6_000).revealed, 3);
    assert_eq!(p.snapshot(6_050).revealed, 4);
}

#[test]
fn auto_pause_continues_on_its_own() {
    let t = tl("a[pause auto=300]b", &no_punct());
    let (mut p, _) = Player::start(LineId(1), t, AfterLine::Hold, None, 0);
    let fx = p.on_wake(p.epoch(), WakeReason::SegmentEnd, 50);
    assert_eq!(wakes(&fx), vec![(350, WakeReason::AutoContinue)]);
    let fx = p.on_wake(p.epoch(), WakeReason::AutoContinue, 350);
    assert!(
        fx.iter()
            .any(|e| matches!(e, PlayerEffect::ShowSegment { seg: 1, .. }))
    );
}

#[test]
fn page_clears_what_came_before() {
    let t = tl("ab[page]cd", &no_punct());
    assert_eq!(t.segments[0].end, SegmentEnd::Page { auto_ms: None });
    assert_eq!(t.segments[1].visible_from, 2);
    let (mut p, _) = Player::start(LineId(1), t, AfterLine::Hold, None, 0);
    p.on_wake(p.epoch(), WakeReason::SegmentEnd, 100);
    p.advance(200);
    assert_eq!(p.snapshot(200).visible_from, 2);
}

#[test]
fn hold_stays_and_fade_uses_delay_plus_per_unit() {
    let t = tl("abc", &no_punct());
    let (mut p, _) = Player::start(LineId(7), t.clone(), AfterLine::Hold, None, 0);
    let fx = p.on_wake(p.epoch(), WakeReason::SegmentEnd, 150);
    assert!(fx.contains(&PlayerEffect::LineRevealed(LineId(7))));
    assert!(wakes(&fx).is_empty());
    assert_eq!(p.phase(), Phase::Done);

    let fade = AfterLine::Fade {
        delay_ms: 1_000,
        per_unit_ms: 40,
    };
    let (mut p, _) = Player::start(LineId(8), t, fade, None, 0);
    let fx = p.on_wake(p.epoch(), WakeReason::SegmentEnd, 150);
    assert_eq!(wakes(&fx), vec![(150 + 1_000 + 120, WakeReason::AfterLine)]);
    let fx = p.on_wake(p.epoch(), WakeReason::AfterLine, 1_270);
    assert!(matches!(fx[0], PlayerEffect::FadeOut { at_ms: 1_270, .. }));
    assert!(p.snapshot(1_270 + 225).opacity < 0.6);
    let fx = p.on_wake(p.epoch(), WakeReason::FadeDone, 1_270 + 450);
    assert_eq!(fx, vec![PlayerEffect::LineFinished(LineId(8))]);
    assert_eq!(p.snapshot(9_999).opacity, 0.0);
}

#[test]
fn stale_wakes_are_ignored_after_a_skip() {
    let t = tl("abcdef", &no_punct());
    let (mut p, fx) = Player::start(LineId(1), t, AfterLine::Hold, None, 0);
    let old = match &fx[1] {
        PlayerEffect::ShowSegment { epoch, .. } => *epoch,
        other => panic!("{other:?}"),
    };
    let (_, fx) = p.advance(60);
    assert!(
        fx.iter()
            .any(|e| matches!(e, PlayerEffect::LineRevealed(_)))
    );
    assert_eq!(p.snapshot(60).revealed, 6);
    assert!(p.on_wake(old, WakeReason::SegmentEnd, 300).is_empty());
}

#[test]
fn skip_fires_pending_expressions_but_not_clicks() {
    let doc = parse("ab[expr=smile]cd[se=coin]").doc;
    let t = Arc::new(compile(&doc, &no_punct(), &TypingSoundRule::default()));
    let (mut p, _) = Player::start(LineId(1), t, AfterLine::Hold, None, 0);
    let (_, fx) = p.advance(10);
    let fired = fx
        .iter()
        .find_map(|e| match e {
            PlayerEffect::FireNow { cues } => Some(cues.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(fired.len(), 2);
    assert!(fired.iter().all(|c| c.kind.survives_skip()));
}

#[test]
fn auto_advance_fires_after_the_reveal() {
    let t = tl("a", &no_punct());
    let (mut p, _) = Player::start(LineId(3), t, AfterLine::Hold, Some(800), 0);
    let fx = p.on_wake(p.epoch(), WakeReason::SegmentEnd, 50);
    assert_eq!(wakes(&fx), vec![(850, WakeReason::AutoAdvance)]);
    let fx = p.on_wake(p.epoch(), WakeReason::AutoAdvance, 850);
    assert_eq!(fx, vec![PlayerEffect::AutoAdvance(LineId(3))]);
}

#[test]
fn typing_sounds_skip_spaces_punctuation_and_collapse_instant_spans() {
    let doc = parse("a b。[instant]cdef[/instant]").doc;
    let t = compile(&doc, &no_punct(), &TypingSoundRule::default());
    let clicks: Vec<u32> = t.segments[0]
        .cues
        .iter()
        .filter_map(|c| match c.kind {
            CueKind::TypeSound { unit, .. } => Some(unit),
            _ => None,
        })
        .collect();
    assert_eq!(clicks, vec![0, 2, 4]);
    let every_other = TypingSoundRule {
        every_n: 2,
        ..TypingSoundRule::default()
    };
    let t = compile(&parse("abcd").doc, &no_punct(), &every_other);
    let n = t.segments[0]
        .cues
        .iter()
        .filter(|c| matches!(c.kind, CueKind::TypeSound { .. }))
        .count();
    assert_eq!(n, 2);
}

#[test]
fn speaking_cues_bracket_each_segment() {
    let t = tl("ab[pause]c", &no_punct());
    for s in &t.segments {
        assert_eq!(s.cues.first().unwrap().kind, CueKind::SpeakingStart);
        assert_eq!(s.cues.last().unwrap().kind, CueKind::SpeakingStop);
    }
}

#[test]
fn markers_cue_at_the_moment_they_are_reached() {
    let t = tl("ab[expr=smile]c[screenshake=4,200]", &no_punct());
    let at = |pred: fn(&CueKind) -> bool| {
        t.segments[0]
            .cues
            .iter()
            .find(|c| pred(&c.kind))
            .unwrap()
            .at_ms
    };
    assert_eq!(at(|k| matches!(k, CueKind::Expression { .. })), 100);
    assert_eq!(at(|k| matches!(k, CueKind::Shake { .. })), 150);
}

#[test]
fn mirror_appends_repeated_characters() {
    let a = Document::plain("哈");
    let b = Document::plain("哈哈");
    assert_eq!(
        mirror_delta(&a, &b),
        MirrorDelta {
            common_prefix: 1,
            removed: 0,
            appended: 1
        }
    );
    let c = Document::plain("哈呵哈");
    assert_eq!(
        mirror_delta(&b, &c),
        MirrorDelta {
            common_prefix: 1,
            removed: 1,
            appended: 2
        }
    );
    let styled = parse("[b]哈[/b]哈").doc;
    assert_eq!(mirror_delta(&b, &styled).common_prefix, 0);
    assert_eq!(mirror_offsets(3, 30, 60), vec![0, 20, 40]);
}

#[test]
fn queue_policies() {
    let line = || Line::new(Document::plain("x"), Speaker::default());
    let mut q = LineQueue::new(QueuePolicy::Queue);
    assert!(matches!(
        q.submit(line(), false),
        QueueDecision::StartNow(_)
    ));
    assert!(matches!(
        q.submit(line(), true),
        QueueDecision::Enqueued { position: 1 }
    ));
    assert!(q.pop_next().is_some());
    let mut q = LineQueue::new(QueuePolicy::FinishThenNext);
    assert!(matches!(
        q.submit(line(), true),
        QueueDecision::FinishCurrentThenStart(_)
    ));
    let mut q = LineQueue::new(QueuePolicy::Replace);
    assert!(matches!(q.submit(line(), true), QueueDecision::StartNow(_)));
}

#[test]
fn long_pauses_close_the_mouth_until_the_next_character() {
    let t = tl("ab[wait=500]cd", &no_punct());
    let speaking: Vec<_> = t.segments[0]
        .cues
        .iter()
        .filter_map(|c| match c.kind {
            CueKind::SpeakingStart => Some((c.at_ms, true)),
            CueKind::SpeakingStop => Some((c.at_ms, false)),
            _ => None,
        })
        .collect();
    assert_eq!(speaking.len(), 4, "{speaking:?}");
    assert!(speaking.iter().map(|s| s.1).eq([true, false, true, false]));
    assert!(speaking[1].0 < speaking[2].0);
    // Short gaps between characters keep talking.
    let t = tl("abcd", &no_punct());
    let starts = t.segments[0]
        .cues
        .iter()
        .filter(|c| c.kind == CueKind::SpeakingStart)
        .count();
    assert_eq!(starts, 1);
}

#[test]
fn vts_markers_cue_and_survive_a_skip() {
    let t = tl("a[vts hotkey=Wave]b", &no_punct());
    let cue = t.segments[0]
        .cues
        .iter()
        .find(|c| matches!(c.kind, CueKind::Vts(_)))
        .unwrap();
    assert_eq!(cue.at_ms, 50);
    assert!(cue.kind.survives_skip());
}

#[test]
fn finishing_the_reveal_fires_later_segments_expressions() {
    let t = tl("a[pause]b[expr=smile]c[page]d[se=coin]", &no_punct());
    let (mut p, _) = Player::start(LineId(1), t, AfterLine::Hold, None, 0);
    let out = p.finish_reveal(10);
    let fired: Vec<_> = out
        .iter()
        .find_map(|e| match e {
            PlayerEffect::FireNow { cues } => Some(cues.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(fired.len(), 2, "{fired:?}");
    assert_eq!(p.phase(), Phase::Done);
}

fn speaking_balanced(s: &Segment) -> bool {
    let mut on = false;
    for c in s.cues.iter() {
        match c.kind {
            CueKind::SpeakingStart if on => return false,
            CueKind::SpeakingStart => on = true,
            CueKind::SpeakingStop if !on => return false,
            CueKind::SpeakingStop => on = false,
            _ => {}
        }
    }
    !on
}

#[test]
fn splitting_a_page_cuts_the_segment_and_moves_the_rest_of_the_page() {
    let sound = TypingSoundRule::default();
    let mut t = compile(
        &parse("abcd[expr=smile]efgh[pause]ij[page]kl").doc,
        &no_punct(),
        &sound,
    );
    assert!(t.split_page(2, Some(900)));
    let [a, b, c, d] = &t.segments[..] else {
        panic!("{:?}", t.segments)
    };
    assert_eq!(
        (a.units.clone(), a.end),
        (0..2, SegmentEnd::Page { auto_ms: Some(900) })
    );
    assert_eq!(a.duration_ms, 100);
    assert_eq!(&*b.reveal_ms, &[0, 50, 100, 150, 200, 250]);
    assert_eq!(b.units, 2..8);
    // The page the pause led on to moved too; the explicit page did not.
    assert_eq!((b.visible_from, c.visible_from, d.visible_from), (2, 2, 10));
    // Typing clicks go by unit, markers by time.
    let clicks = |s: &Segment| {
        s.cues
            .iter()
            .filter_map(|c| match c.kind {
                CueKind::TypeSound { unit, .. } => Some(unit),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(clicks(a).iter().all(|u| *u < 2));
    assert!(clicks(b).iter().all(|u| *u >= 2));
    let smile = b
        .cues
        .iter()
        .find(|c| matches!(c.kind, CueKind::Expression { .. }))
        .unwrap();
    assert_eq!(smile.at_ms, 100);
    assert!(speaking_balanced(a) && speaking_balanced(b));
    // Idempotent; a pause boundary becomes a page.
    assert!(!t.split_page(2, None));
    assert!(t.split_page(8, None));
    assert_eq!(t.segments[1].end, SegmentEnd::Page { auto_ms: None });
    assert_eq!(t.segments[2].visible_from, 8);
    assert!(!t.split_page(0, None));
}

fn shown_until(effects: &[PlayerEffect], t: &Timeline) -> Option<u32> {
    effects.iter().rev().find_map(|e| match e {
        PlayerEffect::ShowSegment { seg, .. } => Some(t.segments[*seg].units.end),
        _ => None,
    })
}

#[test]
fn paginating_before_the_cut_reschedules_the_segment() {
    let t = tl("abcdefgh", &no_punct());
    let (mut p, _) = Player::start(LineId(1), t, AfterLine::Hold, None, 0);
    let (touched, out) = p.paginate(&[4], None, 60);
    assert!(touched);
    assert_eq!(shown_until(&out, p.timeline()), Some(4));
    assert_eq!(wakes(&out), vec![(200, WakeReason::SegmentEnd)]);
    // The old wake is stale; the new one gates at the page.
    assert!(p.on_wake(1, WakeReason::SegmentEnd, 350).is_empty());
    p.on_wake(p.epoch(), WakeReason::SegmentEnd, 200);
    assert_eq!(p.phase(), Phase::Gated);
    let (_, out) = p.advance(1000);
    assert_eq!(p.snapshot(1000).visible_from, 4);
    assert_eq!(wakes(&out), vec![(1000 + 200, WakeReason::SegmentEnd)]);
    // A second measurement of the new page is not paged again.
    assert_eq!(p.paginate(&[6], None, 1010), (false, vec![]));
}

#[test]
fn paginating_past_the_cut_gates_at_once_and_auto_turns() {
    let t = tl("abcdefgh", &no_punct());
    let (mut p, _) = Player::start(LineId(1), t, AfterLine::Hold, None, 0);
    let (touched, out) = p.paginate(&[2], Some(800), 300);
    assert!(touched);
    assert_eq!(p.phase(), Phase::Gated);
    assert!(out.contains(&PlayerEffect::ContinueIndicator(true)));
    assert_eq!(wakes(&out), vec![(1100, WakeReason::AutoContinue)]);
}

#[test]
fn a_line_shown_at_once_goes_back_to_its_first_page() {
    let pacing = PacingParams {
        typing: false,
        ..no_punct()
    };
    let t = tl("a[expr=smile]bcdef", &pacing);
    let (mut p, _) = Player::start(LineId(1), t, AfterLine::Hold, None, 0);
    let out = p.on_wake(p.epoch(), WakeReason::SegmentEnd, 0);
    assert!(out.contains(&PlayerEffect::LineRevealed(LineId(1))));
    let (touched, out) = p.paginate(&[3], None, 20);
    assert!(touched);
    assert_eq!(p.phase(), Phase::Gated);
    assert_eq!(shown_until(&out, p.timeline()), Some(3));
    let (_, out) = p.advance(30);
    // The expression fired when the line started; done is not announced twice.
    assert!(!out.iter().any(|e| matches!(e, PlayerEffect::ScheduleCues { cues, .. } if cues.iter().any(|c| matches!(c.kind, CueKind::Expression { .. })))));
    let out = p.on_wake(p.epoch(), WakeReason::SegmentEnd, 30);
    assert_eq!(p.phase(), Phase::Done);
    assert!(!out.contains(&PlayerEffect::LineRevealed(LineId(1))));
}

#[test]
fn skipping_to_the_end_keeps_the_last_page() {
    let t = tl("abcdefgh", &no_punct());
    let (mut p, _) = Player::start(LineId(1), t, AfterLine::Hold, None, 0);
    p.finish_reveal(10);
    assert_eq!(p.paginate(&[4], None, 20), (false, vec![]));
}
