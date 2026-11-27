//! The history window: a live-chat-style column of avatar, name and bubble.
//! Rows are drawn by the same document builder as the dialogue box, so a line
//! keeps its colours, effects and stickers; with "typing along" sync the
//! newest row reveals in step with the typewriter.

mod event_look;
mod list;
mod row;
mod selection;

pub use list::{focus_search, history_window, history_window_with};
pub use row::{local_hm, local_time};
pub use selection::{
    FadeSchedule, fade_schedule, frame_demand, row_matches, search_allowed, search_allowed_for,
};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nana_ui::runtime::Stack;
    use nana_ui::runtime::view::{IntoView, widget};
    use nana_ui_core::LengthSpec;
    use nanatype_core::HistoryRow;
    use nanatype_model::{Document, HistoryItem, LineId, Rgba8, SourceKind};
    use nanatype_theme::{HistoryEase, HistoryEntrance, Skin};

    use super::super::surface::Surface;
    use super::super::{SceneFrameDemand, Stage, c};
    use super::row::{ROWS_BUILT, bubble_view, inline_name_width, inline_time_fits};
    use super::selection::{Selection, follow_ups, selected_rows, visible_keys};
    use super::*;

    /// An inline log narrower than its three columns drops the time before
    /// the name is cut to "…"; with room it keeps both.
    #[test]
    fn a_narrow_inline_log_gives_the_time_up_for_the_name() {
        let mut h = nanatype_theme::History {
            name_size: 16.0,
            name_width: 120.0,
            time_seconds: true,
            ..nanatype_theme::History::default()
        };
        h.text.size = 20.0;
        let three = h.name_size * 3.0;
        assert!(!inline_time_fits(&h, 230.0));
        assert!(inline_name_width(&h, 230.0, false) >= three);
        assert!(inline_time_fits(&h, 400.0));
        assert!(inline_name_width(&h, 400.0, true) >= three);
    }

    #[test]
    fn search_filters_the_window_but_never_the_broadcast_output() {
        let settings = nanatype_settings::Settings::default();
        let ui = Stage::new(
            Arc::new(settings.clone()),
            Arc::new(Skin::default()),
            nanatype_i18n::Locale::EnUs,
        );
        let row = |key, author: &str, text: &str| HistoryRow {
            key,
            item: HistoryItem {
                source: nanatype_model::SourceId::local(),
                kind: SourceKind::Typewriter,
                line: None,
                author: author.into(),
                avatar: None,
                doc: Document::plain(text),
                at: 0,
                uid: None,
                reply: None,
                event: None,
            },
            reveal: None,
        };
        let rows: Vec<HistoryRow> = (0..40)
            .map(|i| row(10 + i, "Viewer", "filler"))
            .chain([
                row(1, "Nana", "Hello there"),
                row(2, "Bob", "good night"),
                row(3, "nana", "bye"),
            ])
            .collect();
        ui.history.set(Arc::new(rows));
        let shown = settings.history.visible_items as usize;
        assert_eq!(visible_keys(ui).len(), shown);
        // Found rows come from the whole kept log, by text or name, in any case.
        ui.history_search.set(Some(" NANA ".into()));
        assert_eq!(visible_keys(ui), [1, 3]);
        ui.history_search.set(Some("NIGHT".into()));
        assert_eq!(visible_keys(ui), [2]);
        ui.history_search.set(Some(String::new()));
        assert_eq!(visible_keys(ui).len(), shown);
        // The window is what Spout/Syphon sends: while it does, nothing is
        // filtered, and a locked window cannot be searched either.
        ui.history_search.set(Some("night".into()));
        for edit in [
            (|s: &mut nanatype_settings::Settings| s.output.history.enabled = true)
                as fn(&mut nanatype_settings::Settings),
            |s| s.overlays.history.lock = nanatype_settings::Lock::Locked,
        ] {
            let mut s = settings.clone();
            edit(&mut s);
            assert!(!search_allowed(&s));
            ui.settings.set(Arc::new(s));
            assert_eq!(visible_keys(ui).len(), shown);
        }
        ui.settings.set(Arc::new(settings));
        assert_eq!(visible_keys(ui), [2]);
    }

    /// The search button unfolds a field that takes the caret at once, so
    /// typing filters straight away; folding and unfolding again does too.
    #[test]
    fn unfolding_the_search_focuses_its_field() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(Skin::default()),
            nanatype_i18n::Locale::EnUs,
        );
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .mount_view_root(id, move || history_window_with(ui, true, || ().into_any()))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 420, 560).unwrap();
        session.flush().unwrap();
        let mut open = false;
        let label = ui.now_t("history-search");
        for round in 0..2 {
            let button = session
                .accessibility_dump()
                .into_iter()
                .find(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str()))
                .expect("search button");
            session.activate(button.id).unwrap();
            session.flush().unwrap();
            focus_search(ui, session.document_mut(), &mut open);
            assert!(open, "round {round}");
            let document = session.document().document();
            let focused = session.document().context().world().focused(document);
            assert_eq!(
                focused,
                ui.history_search_field.get_untracked(),
                "round {round}"
            );
            session.type_text("nana").unwrap();
            session.flush().unwrap();
            assert_eq!(ui.history_search.get_untracked().as_deref(), Some("nana"));
            // Folded (the button again, or Esc through the host): the next
            // unfolding focuses afresh.
            ui.history_search.set(None);
            session.flush().unwrap();
            focus_search(ui, session.document_mut(), &mut open);
            assert!(!open);
        }
    }

    #[test]
    fn translucent_bubble_artwork_keeps_its_content_opaque() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
        use nanatype_theme::{ColorRef, ContourPreset, GradientSpec};

        for contour in [None, Some(ContourPreset::Rounded)] {
            for opacity in [0.0, 0.35, 0.7, 1.0] {
                let mut skin = Skin::default();
                skin.history.bubble_color = Rgba8::rgb(160, 40, 60);
                skin.history.bubble_border = Some(Rgba8::WHITE);
                skin.history.bubble_border_width = 2.0;
                skin.history.bubble_contour = contour;
                skin.history.bubble_gradient = Some(GradientSpec {
                    stops: vec![
                        (0.0, ColorRef::from(Rgba8::rgb(200, 80, 100))),
                        (1.0, ColorRef::from(Rgba8::rgb(100, 20, 40))),
                    ],
                    ..Default::default()
                });
                let surface = Surface::bubble(&skin, None);
                let id = DocumentId::new(1).unwrap();
                let mut doc = RuntimeDocument::new(id);
                doc.context_mut()
                    .mount_view_root(id, move || {
                        let content = widget(
                            Stack::row(0.0)
                                .width(LengthSpec::Px(80.0))
                                .height(LengthSpec::Px(12.0))
                                .with_layout(|l| {
                                    l.paint_colors.background =
                                        Some(nana_ui_core::PaintColor::srgb(c(Rgba8::WHITE)))
                                }),
                        )
                        .into_any();
                        widget(Stack::column(0.0).padding(12.0)).children(bubble_view(
                            surface,
                            opacity,
                            [12.0, 8.0],
                            160.0,
                            false,
                            content,
                        ))
                    })
                    .unwrap();
                let mut session = RuntimeAgentSession::new(doc, 200, 80).unwrap();
                session.set_clear(Some([0.0; 4]));
                let (size, pixels) = session.screenshot_rgba().unwrap();
                let alpha = |x, y| pixels[(y * size.width as usize + x) * 4 + 3];
                let expected = (opacity * 255.0).round() as i16;
                assert!(
                    (i16::from(alpha(20, 28)) - expected).abs() <= 2,
                    "{contour:?} {opacity}: gradient alpha {}, expected {expected}",
                    alpha(20, 28)
                );
                assert!(
                    (i16::from(alpha(13, 28)) - expected).abs() <= 2,
                    "{contour:?} {opacity}: border alpha {}, expected {expected}",
                    alpha(13, 28)
                );
                assert_eq!(
                    alpha(50, 28),
                    255,
                    "{contour:?} {opacity}: content must stay opaque"
                );
            }
        }
    }

    #[test]
    fn the_line_on_screen_waits_until_it_leaves() {
        let rows: Vec<_> = [(1, Some(LineId(10))), (2, None), (3, Some(LineId(11)))]
            .into_iter()
            .map(|(key, line)| {
                let mut row = row(key, 0);
                row.item.line = line;
                row
            })
            .collect();
        let keys = |visible, hidden| {
            let mut keys: Vec<_> = selected_rows(&rows, Selection { visible, hidden })
                .map(|r| r.key)
                .collect();
            keys.reverse();
            keys
        };
        assert_eq!(keys(30, None), [1, 2, 3]);
        // Only the line still on the box is left out; external rows stay.
        assert_eq!(keys(30, Some(LineId(11))), [1, 2]);
        // The cap counts the rows that show.
        assert_eq!(keys(1, Some(LineId(11))), [2]);
        assert!(keys(0, None).is_empty());
    }

    fn row(key: u64, at: u64) -> HistoryRow {
        let mut item = item("Nana", SourceKind::Typewriter, 0);
        item.at = at;
        HistoryRow {
            key,
            item,
            reveal: None,
        }
    }

    #[test]
    fn fade_schedule_uses_the_same_visible_rows_as_the_list() {
        use nanatype_settings::{HideLatest, Settings};
        let mut settings = Settings::default();
        settings.history.visible_items = 1;
        settings.history.fade_after_s = Some(2);
        settings.history.hide_latest = HideLatest::Never;
        let ui = Stage::new(
            Arc::new(settings),
            Arc::new(Skin::default()),
            nanatype_i18n::Locale::ZhCn,
        );
        let base = crate::clock::unix_of(0);
        ui.history
            .set(Arc::new(vec![row(1, base), row(2, base + 3000)]));
        assert_eq!(visible_keys(ui), [2]);
        // Row 1 is fading, but it isn't mounted. Only row 2 schedules work.
        assert_eq!(
            fade_schedule(ui, 2000),
            FadeSchedule {
                animating: false,
                next_at: Some(5000)
            }
        );
        assert!(fade_schedule(ui, 5000).animating);
        assert_eq!(fade_schedule(ui, 5701), FadeSchedule::default());
        ui.settings
            .update(|s| Arc::make_mut(s).history.visible_items = 2);
        assert_eq!(visible_keys(ui), [1, 2]);
        assert!(fade_schedule(ui, 2000).animating);
        ui.settings
            .update(|s| Arc::make_mut(s).history.visible_items = 0);
        assert_eq!(fade_schedule(ui, 2000), FadeSchedule::default());
        ui.settings.update(|s| {
            let s = Arc::make_mut(s);
            s.history.visible_items = 30;
            s.history.hide_latest = HideLatest::Always;
        });
        ui.history.update(|rows| {
            let rows = Arc::make_mut(rows);
            rows[0].item.line = Some(LineId(10));
            rows[1].item.line = Some(LineId(11));
        });
        ui.display
            .update(|d| Arc::make_mut(d).line = Some(LineId(10)));
        assert_eq!(visible_keys(ui), [2]);
        assert!(!fade_schedule(ui, 2000).animating);
    }

    #[test]
    fn frame_demand_combines_fade_and_reveal_compositor() {
        let mut settings = nanatype_settings::Settings::default();
        settings.history.visible_items = 5;
        settings.history.fade_after_s = Some(3);
        let mut skin = Skin::default();
        skin.dialogue.reveal.entrance.ms = 120;
        let ui = Stage::new(
            Arc::new(settings),
            Arc::new(skin),
            nanatype_i18n::Locale::EnUs,
        );
        let base = crate::clock::unix_of(0);
        ui.history.set(Arc::new(vec![row(1, base)]));
        assert_eq!(
            frame_demand(ui, 1000),
            SceneFrameDemand {
                tick_clock: false,
                render_only: false,
                next_at: Some(3000),
            }
        );

        let mut revealing = row(2, base + 10_000);
        revealing.reveal = Some(nanatype_core::Reveal {
            visible_from: 0,
            shown: 1,
            hidden_from: 2,
            unit_times: None,
            segment: Some(nanatype_core::SegmentReveal {
                start: 1,
                origin_ms: 1000,
                reveal_ms: Arc::from([0u32]),
            }),
        });
        ui.history
            .update(|rows| Arc::make_mut(rows).push(revealing));
        assert!(frame_demand(ui, 1010).tick_clock);
    }

    fn item(author: &str, kind: SourceKind, at_s: u64) -> HistoryItem {
        HistoryItem {
            source: nanatype_model::SourceId::local(),
            kind,
            line: None,
            author: author.into(),
            avatar: None,
            doc: nanatype_model::Document::new(),
            at: at_s * 1000,
            uid: None,
            reply: None,
            event: None,
        }
    }

    #[test]
    fn a_long_name_ends_before_its_role_in_every_locale() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let name = "这是一位名字特别长的观众 A very long viewer name";
        for locale in nanatype_i18n::Locale::ALL {
            for skin_id in ["nanatype.pixel", "nanatype.vanilla"] {
                let skin = registry.load(skin_id).unwrap().skin;
                // As the skin writes it: pixel adds a colon.
                let written = skin.history.name(name);
                let (w, h) = (skin.history.width as u32, skin.history.height as u32);
                let mut settings = nanatype_settings::Settings::default();
                settings.history.show_time = true;
                let ui = super::super::Stage::new(Arc::new(settings), skin, locale);
                let mut row_item = item(name, SourceKind::Chat, 100);
                row_item.doc = Document::plain("求个自我介绍～");
                ui.history.set(Arc::new(vec![HistoryRow {
                    key: 1,
                    item: row_item,
                    reveal: None,
                }]));
                let id = DocumentId::new(1).unwrap();
                let mut doc = RuntimeDocument::new(id);
                doc.context_mut()
                    .mount_view_root(id, move || history_window(ui))
                    .unwrap();
                let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
                session.flush().unwrap();
                let nodes = session.accessibility_dump();
                let find = |label: &str| {
                    nodes
                        .iter()
                        .find(|n| n.label.as_deref() == Some(label))
                        .unwrap_or_else(|| panic!("{locale:?} {skin_id}: no {label}"))
                        .bounds
                        .clone()
                };
                let shown = find(&written);
                let role = find(&ui.now_t("history-role-viewer"));
                assert!(
                    shown.x + shown.width <= role.x,
                    "{locale:?} {skin_id}: the name {shown:?} runs under its role {role:?}"
                );
            }
        }
    }

    /// A log of opaque red bubbles, `w`×`h`, with rows `1..=rows`.
    fn motion_log(
        w: u32,
        h: u32,
        rows: u64,
        visible: u32,
    ) -> (Stage, nana_ui_devtools::agent::RuntimeAgentSession) {
        motion_log_with(w, h, rows, visible, |_| {})
    }

    /// [`motion_log`] with the skin's history changed by `dress`.
    fn motion_log_with(
        w: u32,
        h: u32,
        rows: u64,
        visible: u32,
        dress: impl FnOnce(&mut nanatype_theme::History),
    ) -> (Stage, nana_ui_devtools::agent::RuntimeAgentSession) {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
        let mut skin = Skin::default();
        skin.history.entrance = HistoryEntrance::SlideUp;
        skin.history.bubble_color = Rgba8::rgb(220, 30, 30);
        skin.history.group_speakers = false;
        dress(&mut skin.history);
        let mut settings = nanatype_settings::Settings::default();
        settings.history.show_avatar = false;
        settings.history.show_name = false;
        settings.history.show_time = false;
        settings.history.fade_after_s = None;
        settings.history.visible_items = visible;
        settings.history.hide_latest = nanatype_settings::HideLatest::Never;
        let ui = Stage::new(
            Arc::new(settings),
            Arc::new(skin),
            nanatype_i18n::Locale::ZhCn,
        );
        ui.history.set(Arc::new((1..=rows).map(text_row).collect()));
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        crate::clock::capture_at(0, || {
            doc.context_mut()
                .mount_view_root(id, move || history_window(ui))
                .unwrap()
        });
        let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
        session.set_clear(Some([0.0; 4]));
        crate::clock::capture_at(0, || session.flush().unwrap());
        (ui, session)
    }

    fn text_row(key: u64) -> HistoryRow {
        let mut item = item("Nana", SourceKind::Typewriter, key);
        item.doc = Document::plain("第一行文字\n第二行文字");
        HistoryRow {
            key,
            item,
            reveal: None,
        }
    }

    /// The bubbles crossing the column at x = 30, top to bottom: their
    /// vertical extent and whether they are fully opaque.
    fn bubbles(
        session: &mut nana_ui_devtools::agent::RuntimeAgentSession,
        at: u64,
    ) -> Vec<(usize, usize, bool)> {
        let t = std::time::Duration::from_millis(at);
        session.document_mut().context_mut().advance_animations(t);
        session.document_mut().sync_presentation_clock(t);
        let (size, pixels) = crate::clock::capture_at(at, || session.screenshot_rgba().unwrap());
        let alpha = |y: usize| pixels[(y * size.width as usize + 30) * 4 + 3];
        let mut runs = Vec::new();
        let mut start = None;
        for y in 0..=size.height as usize {
            let inked = y < size.height as usize && alpha(y) > 8;
            match (inked, start) {
                (true, None) => start = Some(y),
                (false, Some(s)) => {
                    runs.push((s, y, alpha((s + y) / 2) == 255));
                    start = None;
                }
                _ => {}
            }
        }
        runs
    }

    /// Append the next row at `at` ms, as a program message would.
    fn push_row(
        ui: Stage,
        session: &mut nana_ui_devtools::agent::RuntimeAgentSession,
        key: u64,
        at: u64,
    ) {
        let t = std::time::Duration::from_millis(at);
        session.document_mut().context_mut().advance_animations(t);
        ui.history
            .update(|rows| Arc::make_mut(rows).push(text_row(key)));
        crate::clock::capture_at(at, || session.flush().unwrap());
    }

    #[test]
    fn a_new_row_rises_and_fades_in() {
        let (ui, mut session) = motion_log(300, 480, 1, 30);
        let before = bubbles(&mut session, 5_000);
        assert_eq!(before.len(), 1, "{before:?}");
        push_row(ui, &mut session, 2, 5_000);
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        assert_eq!(done.len(), 2, "{done:?}");
        assert!(done[1].2, "the new row settles opaque: {done:?}");
        let entering = mid.last().unwrap();
        assert!(!entering.2, "the new row is still fading in: {mid:?}");
        assert!(
            entering.0 > done[1].0,
            "the new row rises into place: {mid:?} -> {done:?}"
        );
    }

    #[test]
    fn every_entrance_settles_where_a_plain_row_sits() {
        use HistoryEntrance::*;
        let rest = |entrance, ease| {
            let (ui, mut session) = motion_log_with(300, 480, 1, 30, |h| {
                h.entrance = entrance;
                h.entrance_ease = ease;
                h.entrance_ms = 400;
            });
            bubbles(&mut session, 5_000);
            push_row(ui, &mut session, 2, 5_000);
            let mid = bubbles(&mut session, 5_100);
            (mid, bubbles(&mut session, 6_000))
        };
        let (_, plain) = rest(None, HistoryEase::Smooth);
        for entrance in [
            SlideUp, Fade, SlideLeft, SlideRight, Drop, Pop, Stamp, Unfold, Tilt,
        ] {
            for ease in [
                HistoryEase::Smooth,
                HistoryEase::Gentle,
                HistoryEase::Overshoot,
                HistoryEase::Linear,
            ] {
                let (mid, done) = rest(entrance, ease);
                assert_eq!(done, plain, "{entrance:?} {ease:?} rests out of place");
                assert_ne!(mid, done, "{entrance:?} {ease:?} does not play");
            }
        }
        // A dropped row comes from above, an unfolding one grows downwards.
        let (mid, done) = rest(Drop, HistoryEase::Smooth);
        assert!(mid[1].0 < done[1].0, "{mid:?} -> {done:?}");
        let (mid, done) = rest(Unfold, HistoryEase::Smooth);
        assert!(mid[1].1 < done[1].1, "{mid:?} -> {done:?}");
    }

    #[test]
    fn the_entrance_takes_the_time_the_skin_gives_it() {
        let playing = |ms, at: u64| {
            let (ui, mut session) = motion_log_with(300, 480, 1, 30, |h| h.entrance_ms = ms);
            bubbles(&mut session, 5_000);
            push_row(ui, &mut session, 2, 5_000);
            !bubbles(&mut session, 5_000 + at).last().unwrap().2
        };
        assert!(!playing(80, 150), "an 80ms entrance is over by 150ms");
        assert!(
            playing(600, 150),
            "a 600ms entrance is still fading at 150ms"
        );
    }

    #[test]
    fn older_rows_step_back_behind_the_newest() {
        let (ui, mut session) = motion_log_with(300, 480, 2, 30, |h| {
            h.recede = Some(nanatype_theme::HistoryRecede {
                step: 0.3,
                floor: 0.2,
            });
        });
        let before = bubbles(&mut session, 5_000);
        assert_eq!(
            before.iter().map(|b| b.2).collect::<Vec<_>>(),
            [false, true],
            "only the newest row is at full strength: {before:?}"
        );
        push_row(ui, &mut session, 3, 5_000);
        let done = bubbles(&mut session, 6_000);
        assert_eq!(
            done.iter().map(|b| b.2).collect::<Vec<_>>(),
            [false, false, true],
            "the row that was newest stepped back: {done:?}"
        );
    }

    #[test]
    fn a_separator_is_ruled_between_rows_only() {
        let (_, mut session) = motion_log_with(300, 480, 3, 30, |h| {
            h.separator = Some(nanatype_theme::HistorySeparator {
                color: nanatype_theme::ColorRef::Rgba(Rgba8::rgb(0, 0, 200)),
                width: 2.0,
                inset: 0.0,
                fade: false,
            });
        });
        let runs = bubbles(&mut session, 5_000);
        // Three bubbles and the two rules between them.
        assert_eq!(runs.len(), 5, "{runs:?}");
        assert_eq!(runs[1].1 - runs[1].0, 2, "{runs:?}");
        let (above, below) = (runs[1].0 - runs[0].1, runs[2].0 - runs[1].1);
        assert!(
            above.abs_diff(below) <= 1,
            "the rule sits mid-gap: {runs:?}"
        );
    }

    #[test]
    fn rows_slide_up_when_a_new_row_overflows_the_log() {
        let (ui, mut session) = motion_log(300, 200, 3, 30);
        let before = bubbles(&mut session, 5_000);
        push_row(ui, &mut session, 4, 5_000);
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        // The row that was newest: last opaque bubble before, and the one
        // above the new row once settled.
        let was = before.iter().rev().find(|b| b.2).unwrap().1;
        let settled = done[done.len() - 2].1;
        let moving = mid.iter().rev().find(|b| b.2).unwrap().1;
        assert!(settled < was, "the log scrolled: {before:?} -> {done:?}");
        assert!(
            settled < moving && moving < was,
            "the old rows slide up rather than jump: {was} -> {moving} -> {settled}"
        );
        // Rows scrolled off the top are still drawn while the list slides.
        // The log overflows, so its top row is cut at the viewport's edge.
        assert_eq!(
            mid[0].0, done[0].0,
            "the top of the log stays inked: {mid:?} / {done:?}"
        );
    }

    #[test]
    fn rows_close_the_gap_when_the_oldest_row_drops_out() {
        let (ui, mut session) = motion_log(300, 480, 2, 2);
        let before = bubbles(&mut session, 5_000);
        assert_eq!(before.len(), 2, "{before:?}");
        push_row(ui, &mut session, 3, 5_000);
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        assert_eq!(done.len(), 2, "{done:?}");
        // Row 2 moves from the second place to the first.
        let (from, to) = (before[1].0, done[0].0);
        let moving = mid.iter().find(|b| b.2).unwrap().0;
        // Part of the way: not a jump to the end, nor just the lost gap.
        let progress = (from - moving) as f32 / (from - to) as f32;
        assert!(
            (0.3..0.97).contains(&progress),
            "row 2 slides up into the gap: {from} -> {moving} -> {to}"
        );
    }

    #[test]
    fn resizing_the_window_does_not_slide_the_log() {
        use nana_ui_devtools::agent::AgentSession;
        let (_, mut session) = motion_log(300, 200, 4, 30);
        bubbles(&mut session, 5_000);
        let built = ROWS_BUILT.get();
        session.set_viewport(300, 260, 1.0).unwrap();
        let t = std::time::Duration::from_millis(5_000);
        session.document_mut().context_mut().advance_animations(t);
        crate::clock::capture_at(5_000, || session.flush().unwrap());
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        assert_eq!(mid, done, "a resize lands at once");
        session.set_viewport(360, 260, 1.0).unwrap();
        crate::clock::capture_at(5_600, || session.flush().unwrap());
        assert_eq!(
            ROWS_BUILT.get(),
            built,
            "resizing the history window must not remount rows"
        );
    }

    #[test]
    fn changing_a_setting_keeps_the_rows_on_screen() {
        // The log overflows and has slid up for new rows a few times.
        let (ui, mut session) = motion_log(300, 200, 3, 30);
        bubbles(&mut session, 1_000);
        for (key, at) in [(4, 1_000), (5, 2_000), (6, 3_000)] {
            push_row(ui, &mut session, key, at);
            bubbles(&mut session, at + 600);
        }
        let before = bubbles(&mut session, 5_000);
        // The app replaces the whole settings value on every edit.
        let edit = |session: &mut nana_ui_devtools::agent::RuntimeAgentSession,
                    at: u64,
                    change: fn(&mut nanatype_settings::Settings)| {
            ui.settings.update(|s| {
                let mut next = (**s).clone();
                change(&mut next);
                *s = Arc::new(next);
            });
            let t = std::time::Duration::from_millis(at);
            session.document_mut().context_mut().advance_animations(t);
            crate::clock::capture_at(at, || session.flush().unwrap());
        };
        // A setting the log does not draw from neither rebuilds its rows
        // (every row would mount anew) nor moves them.
        let built = ROWS_BUILT.get();
        edit(&mut session, 5_000, |s| s.sound.master = 0.5);
        edit(&mut session, 5_000, |s| {
            s.overlays.display.always_on_top ^= true
        });
        assert_eq!(
            ROWS_BUILT.get(),
            built,
            "unrelated settings rebuilt the rows"
        );
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        assert_eq!(
            mid, before,
            "rows on screen stay put: {before:?} -> {mid:?}"
        );
        assert_eq!(done, before);
        // One it does draw from still reaches the rows.
        edit(&mut session, 6_000, |s| s.history.show_time = true);
        assert!(ROWS_BUILT.get() > built, "the log ignored its own setting");
    }

    #[test]
    fn rows_of_one_speaker_close_together_group() {
        use SourceKind::{Chat, Typewriter};
        let rows: Vec<HistoryRow> = [
            item("Nana", Typewriter, 0),
            item("Nana", Typewriter, 30),
            item("Nana", Chat, 40),
            item("Nana", Typewriter, 50),
            item("Nana", Typewriter, 200),
            item("Mio", Typewriter, 210),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, item)| HistoryRow {
            key: i as u64,
            item,
            reveal: None,
        })
        .collect();
        // A chat row with the streamer's name, a two-minute pause and a new
        // speaker each start a group.
        assert_eq!(follow_ups(&rows), [1]);
        // The first shown row always has its header.
        assert_eq!(follow_ups(&rows[1..]), Vec::<u64>::new());
    }

    #[test]
    fn danmaku_log_renders_separately_and_hides_role_badges() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;

        let mut settings = nanatype_settings::Settings::default();
        settings.history.show_time = true;
        settings.history.chat.separate = true;
        settings.output.danmaku.enabled = true;
        assert!(search_allowed_for(
            crate::canvas::Canvas::History,
            &settings
        ));
        assert!(!search_allowed_for(
            crate::canvas::Canvas::Danmaku,
            &settings
        ));
        settings.output.danmaku.enabled = false;
        assert!(search_allowed_for(
            crate::canvas::Canvas::Danmaku,
            &settings
        ));

        let skin = Arc::new(Skin::default());
        let ui = Stage::new(
            Arc::new(settings.clone()),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );

        // Empty danmaku stage uses the danmaku placeholder.
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        let danmaku_ui = ui.danmaku_log();
        doc.context_mut()
            .mount_view_root(id, move || {
                history_window_with(danmaku_ui, true, || ().into_any())
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 400, 600).unwrap();
        session.flush().unwrap();
        let placeholder = ui.now_t("danmaku-placeholder");
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some(placeholder.as_str())),
            "expected danmaku placeholder when empty"
        );

        // Populated danmaku log shows viewer messages without role badges, even when host history has its own rows.
        ui.settings.update(|s| {
            let mut next = (**s).clone();
            next.history.fade_after_s = None;
            *s = Arc::new(next);
        });
        let mut host_item = item("HostPerson", SourceKind::Typewriter, 10);
        host_item.doc = Document::plain("host line");
        ui.history.set(Arc::new(vec![HistoryRow {
            key: 1,
            item: host_item,
            reveal: None,
        }]));
        let mut viewer_item = item("ViewerPerson", SourceKind::Chat, 20);
        viewer_item.doc = Document::plain("viewer danmaku");
        ui.danmaku.set(Arc::new(vec![HistoryRow {
            key: 2,
            item: viewer_item,
            reveal: None,
        }]));
        ui.danmaku_search.set(Some("viewer".into()));
        assert_eq!(visible_keys(danmaku_ui), [2]);
        assert_eq!(visible_keys(ui), [1]);
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        let viewer_name = skin.history.name("ViewerPerson");
        let host_name = skin.history.name("HostPerson");
        assert!(
            nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(viewer_name.as_str())),
            "expected danmaku author in accessibility tree: {nodes:?}"
        );
        assert!(
            !nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(host_name.as_str())),
            "host history row leaked into danmaku log"
        );
        let viewer_role = ui.now_t("history-role-viewer");
        assert!(
            !nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(viewer_role.as_str())),
            "role badge should be hidden in separate danmaku log"
        );

        // When `separate == true`, Canvas::History shows the host row without a role badge.
        let mut hist_doc = RuntimeDocument::new(id);
        hist_doc
            .context_mut()
            .mount_view_root(id, move || history_window(ui))
            .unwrap();
        let mut hist_session = RuntimeAgentSession::new(hist_doc, 400, 600).unwrap();
        hist_session.flush().unwrap();
        let hist_nodes = hist_session.accessibility_dump();
        assert!(
            hist_nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(host_name.as_str())),
            "expected host row in history window"
        );
        let host_role = ui.now_t("history-role-host");
        assert!(
            !hist_nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(host_role.as_str())),
            "host role badge should be hidden when separate mode is active"
        );
    }
}
