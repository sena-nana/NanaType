//! User-flow regressions against the production settings view and pointer router.

mod common;

use std::{sync::Arc, time::Duration};

use nana_ui::runtime::{ComponentGeometry, StableNodeId};
use nana_ui_devtools::agent::{
    A11yFilter, AccessibilityDumpNode, AgentSession, RuntimeAgentSession,
};
use nana_ui_platform::InputModifiers;
use nanatype::{
    msg::{Msg, WindowMsg},
    ui::{
        Ui,
        settings::{self, Page},
    },
};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;

struct Fixture {
    ui: Ui,
    session: RuntimeAgentSession,
    registry: nanatype_theme::SkinRegistry,
}

impl Fixture {
    fn new(locale: Locale, onboarding: bool, page: Page) -> Self {
        let registry = common::registry();
        let skin = registry.load(nanatype_theme::DEFAULT_SKIN).unwrap().skin;
        let mut settings = Settings::default();
        settings.general.onboarding_done = !onboarding;
        let ui = Ui::new(Arc::new(settings), skin, locale);
        ui.skins
            .set(Arc::new(nanatype::ui::skin_rows(&registry, locale)));
        ui.page.set(page);
        let session = common::session()
            .themed(false)
            .mount(720, 480, move || settings::settings_window(ui));
        let mut f = Self {
            ui,
            session,
            registry,
        };
        let active = f.ui.settings.get_untracked().skin.id.clone();
        f.prepare(&active);
        f
    }

    /// What the program does when the preview asks for skin `id`'s stage.
    fn prepare(&mut self, id: &str) {
        nanatype::ui::prepare_skin_preview(self.ui, &self.registry, id, false).unwrap();
        self.session.flush().unwrap();
    }

    fn find(&self, role: &str, key: &str) -> AccessibilityDumpNode {
        let label = self.ui.now_t(key);
        self.session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.role == role && n.label.as_deref() == Some(&label))
            .unwrap_or_else(|| panic!("missing {role}: {key}"))
    }

    fn keyed(&self, key: &str) -> AccessibilityDumpNode {
        self.session
            .accessibility_dump()
            .into_iter()
            .find(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.rsplit('/').next() == Some(key))
            })
            .unwrap_or_else(|| panic!("missing {key}"))
    }

    fn scroll(&mut self, x: f32, dy: f32) {
        self.session.scroll_by(x, 300.0, 0.0, dy).unwrap();
    }

    fn reach(
        &mut self,
        x: f32,
        bottom: f32,
        find: impl Fn(&Self) -> AccessibilityDumpNode,
    ) -> AccessibilityDumpNode {
        for _ in 0..4 {
            let node = find(self);
            if node.bounds.y >= 56.0 && node.bounds.y + node.bounds.height <= bottom {
                return node;
            }
            self.scroll(x, 200.0 - node.bounds.y);
        }
        panic!("unreachable control: {:?}", find(self));
    }

    fn click(&mut self, node: &AccessibilityDumpNode) {
        let b = &node.bounds;
        self.session
            .click_xy(b.x + b.width / 2.0, b.y + b.height / 2.0)
            .unwrap();
    }

    fn messages(&mut self) -> Vec<Msg> {
        self.session
            .document_mut()
            .context_mut()
            .take_program_messages()
            .into_iter()
            .filter_map(|m| m.downcast::<Msg>().ok().map(|m| *m))
            .collect()
    }

    fn apply_edits(&mut self) {
        let mut settings = (*self.ui.settings.get_untracked()).clone();
        for message in self.messages() {
            if let Msg::Edit(edit) = message {
                edit(&mut settings);
            }
        }
        self.ui.settings.set(Arc::new(settings));
        self.session.flush().unwrap();
    }

    /// The text field in the settings row labelled `key`.
    fn field(&self, key: &str) -> AccessibilityDumpNode {
        let filter = A11yFilter {
            root: Some(self.find("generic", key).id),
            role: Some("text-input".into()),
            ..A11yFilter::default()
        };
        let (fields, _) = self.session.filtered_accessibility(&filter);
        fields
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no text field in {key}"))
    }

    fn settle(&mut self, seconds: u64) {
        let at = Duration::from_secs(seconds);
        self.session
            .document_mut()
            .context_mut()
            .advance_animations(at);
        self.session.document_mut().sync_presentation_clock(at);
        self.session.flush().unwrap();
    }

    fn preview(&self, width: u32, skin: &nanatype_theme::Skin) -> f32 {
        let container = self.keyed("skin-preview").bounds;
        let column = self.keyed("skin-preview-stage-column").bounds;
        let display = self.keyed("skin-preview-display-box").bounds;
        let history = self.keyed("skin-preview-history-box").bounds;
        assert!(container.x >= 196.0 && container.x + container.width <= width as f32 + 0.5);
        assert!(display.x > container.x && history.x > container.x);
        assert!(
            history.x + history.width <= column.x + column.width + 0.5,
            "clipped preview"
        );
        let content = (column.width - 24.0).max(1.0);
        assert!(
            (history.width - content).abs() < 1.5,
            "history does not fill the column: {history:?} content {content}"
        );
        let logical = nanatype::ui::settings::preview::default_size(skin, false);
        assert!(display.width <= logical[0] + 0.5, "upscaled display");
        assert!(display.width <= content + 0.5);
        history.width
    }
}

#[test]
fn speaker_hints_remain_readable_in_all_locales_and_window_sizes() {
    for locale in Locale::ALL {
        for onboarding in [true, false] {
            let mut f = Fixture::new(locale, onboarding, Page::General);
            for (w, h) in [(720, 480), (920, 660)] {
                f.session.set_viewport(w, h, 1.0).unwrap();
                if onboarding {
                    assert!(f.find("text", "general-speaker-hint").bounds.width >= 120.0);
                } else {
                    // On the page it is the characters' info mark, read on hover.
                    assert!(f.find("button", "general-speaker-hint").bounds.width > 0.0);
                }
            }
        }
    }
}

/// The empty speaker field, in the first-run guide and on the page: a click
/// focuses it with a caret, the focus fill and a firmer neutral border (never
/// the accent edge); typing and Backspace edit the setting.
#[test]
fn the_empty_speaker_field_takes_a_click_typing_and_backspace() {
    let palette = nanatype::ui::kit::theme::palette(false);
    for onboarding in [true, false] {
        let mut f = Fixture::new(Locale::ZhCn, onboarding, Page::General);
        // The page lists characters; the first one's name is the field.
        let (key, field) = if onboarding {
            ("onboard-speaker", f.field("onboard-speaker"))
        } else {
            // Below the fold at the window's smallest size.
            let field = f.reach(650.0, 480.0, |f| f.find("text-input", "general-speaker"));
            ("general-speaker", field)
        };
        f.click(&field);
        let id = StableNodeId::new(field.id).unwrap();
        let world = f.session.document().context().world();
        assert_eq!(world.focused(f.session.document().document()), Some(id));
        let Some(ComponentGeometry::TextInput {
            caret,
            background,
            border,
            ..
        }) = world.component_geometry(id)
        else {
            panic!("{key}: the focused field has no geometry");
        };
        assert!(caret.is_some_and(|c| c.height > 0.0), "{key}: no caret");
        assert_eq!(border, Some(palette.border_strong.as_rgba_array()));
        assert_ne!(border, Some(palette.focus_border.as_rgba_array()));
        assert!(background.is_some_and(|b| b != palette.background.as_rgba_array()));
        f.session.type_text("ab").unwrap();
        f.session
            .key_press("Backspace", "Backspace", InputModifiers::default())
            .unwrap();
        f.apply_edits();
        assert_eq!(
            f.ui.settings
                .get_untracked()
                .general
                .active_character()
                .name,
            "a"
        );
    }
}

#[test]
fn first_run_finishes_all_steps_by_pointer_at_minimum_size() {
    for locale in Locale::ALL {
        let mut f = Fixture::new(locale, true, Page::General);
        for step in 0..4 {
            assert_eq!(f.ui.onboard_step.get_untracked(), step);
            f.settle(u64::from(step) * 2 + 1);
            let key = if step == 3 {
                "onboard-done"
            } else {
                "onboard-next"
            };
            let next = f.reach(650.0, 480.0, |f| f.find("button", key));
            assert!(next.bounds.x >= 196.0 && next.bounds.x + next.bounds.width <= 720.0);
            f.click(&next);
            f.apply_edits();
        }
        let general = f.ui.settings.get_untracked().general.clone();
        assert!(general.onboarding_done);
        assert_eq!(general.onboarding_step, 0);
    }
}

/// A guide closed halfway remembers its step and reopens there.
#[test]
fn first_run_resumes_at_the_step_it_was_left_at() {
    let mut f = Fixture::new(Locale::ZhCn, true, Page::General);
    for step in [1, 2] {
        f.settle(u64::from(step) * 2);
        let next = f.reach(650.0, 480.0, |f| f.find("button", "onboard-next"));
        f.click(&next);
        f.apply_edits();
        assert_eq!(f.ui.settings.get_untracked().general.onboarding_step, step);
    }
    let back = f.reach(650.0, 480.0, |f| f.find("button", "onboard-back"));
    f.click(&back);
    f.apply_edits();
    let saved = f.ui.settings.get_untracked();
    assert_eq!(saved.general.onboarding_step, 1);
    assert!(!saved.general.onboarding_done);

    let skin = f.registry.load(nanatype_theme::DEFAULT_SKIN).unwrap().skin;
    let reopened = Ui::new(saved, skin, Locale::ZhCn);
    assert_eq!(reopened.onboard_step.get_untracked(), 1);
}

#[test]
fn sidebar_reaches_every_page_and_new_pages_start_at_the_top() {
    for locale in Locale::ALL {
        let mut f = Fixture::new(locale, false, Page::General);
        let draft = Arc::new(nanatype_model::Document::plain("保留草稿"));
        f.ui.draft.set(draft.clone());
        let about_y = f.find("list-item", "settings-about").bounds.y;
        assert!(about_y > 400.0 && about_y < 480.0);
        for page in Page::ALL {
            let bottom = if page == Page::About { 480.0 } else { about_y };
            let nav = f.reach(100.0, bottom, |f| f.find("list-item", page.label_key()));
            f.click(&nav);
            assert!(
                f.messages()
                    .iter()
                    .any(|m| matches!(m, Msg::Window(WindowMsg::SettingsPage(p)) if *p == page))
            );
            f.ui.page.set(page);
            f.session.flush().unwrap();
            let heading = f.find("text", page.label_key()).bounds;
            assert!(
                heading.y >= 0.0 && heading.y < 100.0,
                "stale scroll: {page:?}"
            );
            f.scroll(650.0, -3000.0);
            assert_eq!(f.find("list-item", "settings-about").bounds.y, about_y);
        }
        assert_eq!(f.ui.draft.get_untracked(), draft);
    }
}

#[test]
fn settings_body_scroll_reaches_and_operates_bottom_controls() {
    for locale in Locale::ALL {
        for (page, key) in [
            (Page::General, "general-check-updates"),
            (Page::History, "history-archive-chat"),
        ] {
            let mut f = Fixture::new(locale, false, page);
            assert!(
                f.find("text", key).bounds.y > 480.0,
                "fixture must overflow"
            );
            let target = f.reach(650.0, 480.0, |f| f.find("text", key));
            assert!(target.bounds.y >= 0.0);
            f.scroll(650.0, -3000.0);
            let toggle = f
                .session
                .accessibility_dump()
                .into_iter()
                .filter(|n| {
                    n.role == "switch" && n.bounds.y >= 0.0 && n.bounds.y + n.bounds.height <= 480.0
                })
                .max_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y))
                .unwrap();
            f.click(&toggle);
            f.apply_edits();
            let s = f.ui.settings.get_untracked();
            // The lowest switches on General and History are the
            // (default-on) update check and the (default-off) saving of
            // viewer chat.
            assert!(if page == Page::General {
                !s.general.check_updates
            } else {
                s.history.archive.chat
            });
            f.scroll(650.0, 3000.0);
            let heading = f.find("text", page.label_key()).bounds;
            assert!(heading.y >= 0.0 && heading.y < 100.0);
        }
    }
}

#[test]
fn saved_history_lists_sessions_and_acts_on_the_open_one() {
    use nanatype::archive::{Format, Record, Session, Source, View};
    use nanatype::msg::HistoryMsg;
    for locale in Locale::ALL {
        let mut f = Fixture::new(locale, false, Page::History);
        let record = |at: u64, speaker: &str, text: &str| Record {
            at,
            speaker: speaker.into(),
            source: Source::Own,
            platform: None,
            text: text.into(),
            markup: None,
            translation: None,
        };
        let mut view = View {
            sessions: vec![
                Session {
                    name: "2026-11-02_213005".into(),
                    lines: 2,
                },
                Session {
                    name: "2026-11-01_200000".into(),
                    lines: 7,
                },
            ],
            ..View::default()
        };
        view.open(
            "2026-11-02_213005".into(),
            vec![
                record(1_762_000_000_000, "七七", "大家晚上好"),
                record(1_762_000_005_000, "七七", "今天玩什么呢"),
            ],
        );
        f.ui.archive.set(Arc::new(view));
        f.session.flush().unwrap();
        let line = f.reach(650.0, 480.0, |f| {
            f.session
                .accessibility_dump()
                .into_iter()
                .find(|n| n.label.as_deref() == Some("今天玩什么呢"))
                .expect("the open session's lines")
        });
        assert!(line.bounds.y >= 0.0);
        let srt = f.reach(650.0, 480.0, |f| {
            f.find("button", "history-saved-export-srt")
        });
        f.click(&srt);
        assert!(
            f.messages()
                .iter()
                .any(|m| matches!(m, Msg::History(HistoryMsg::Export(Format::Srt))))
        );
        // Deleting asks first; the confirming button takes the trigger's place.
        let delete = f.reach(650.0, 480.0, |f| f.find("button", "history-saved-delete"));
        f.click(&delete);
        f.session.flush().unwrap();
        assert!(f.messages().is_empty());
        let sure = f.reach(650.0, 480.0, |f| {
            let label = f.ui.now_t("history-saved-delete");
            f.session
                .accessibility_dump()
                .into_iter()
                .filter(|n| n.role == "button" && n.label.as_deref() == Some(&label))
                .find(|n| n.bounds.width > 0.0)
                .expect("the confirming delete")
        });
        f.click(&sure);
        assert!(f.messages().iter().any(
            |m| matches!(m, Msg::History(HistoryMsg::Delete(name)) if name == "2026-11-02_213005")
        ));
        let view = f.reach(650.0, 480.0, |f| f.find("button", "history-saved-view"));
        f.click(&view);
        assert!(f.messages().iter().any(|m| matches!(
            m,
            Msg::History(HistoryMsg::Open(Some(name))) if name == "2026-11-01_200000"
        )));
    }
}

/// A long session lists its newest page, and earlier pages a click away
/// until the whole session shows.
#[test]
fn saved_history_pages_back_through_a_long_session() {
    use nanatype::archive::{Record, Session, Source, View};
    use nanatype::msg::HistoryMsg;
    let mut f = Fixture::new(Locale::EnUs, false, Page::History);
    let records: Vec<Record> = (0..View::PAGE as u64 + 30)
        .map(|i| Record {
            at: 1_762_000_000_000 + i * 1000,
            speaker: "Nana".into(),
            source: Source::Own,
            platform: None,
            text: format!("line {i}"),
            markup: None,
            translation: None,
        })
        .collect();
    let mut view = View {
        sessions: vec![Session {
            name: "2026-11-02_213005".into(),
            lines: records.len(),
        }],
        ..View::default()
    };
    view.open("2026-11-02_213005".into(), records);
    f.ui.archive.set(Arc::new(view.clone()));
    f.session.flush().unwrap();
    let has = |f: &Fixture, text: &str| {
        f.session
            .accessibility_dump()
            .into_iter()
            .any(|n| n.label.as_deref() == Some(text))
    };
    let newest = format!("line {}", View::PAGE + 29);
    assert!(has(&f, &newest));
    assert!(!has(&f, "line 0"));
    let label = f.ui.tr("history-saved-earlier", &[("count", 30u64.into())]);
    let earlier = f.reach(650.0, 480.0, |f| {
        f.session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str()))
            .expect("the earlier lines button")
    });
    f.click(&earlier);
    assert!(
        f.messages()
            .iter()
            .any(|m| matches!(m, Msg::History(HistoryMsg::Earlier)))
    );
    view.earlier();
    f.ui.archive.set(Arc::new(view));
    f.session.flush().unwrap();
    assert!(has(&f, "line 0") && has(&f, &newest));
    assert!(
        !f.session
            .accessibility_dump()
            .into_iter()
            .any(|n| n.label.as_deref() == Some(label.as_str())),
        "nothing earlier left"
    );
}

#[test]
#[cfg(any(windows, target_os = "macos"))]
fn obs_guide_enables_output_before_selecting_the_sender() {
    for locale in Locale::ALL {
        let mut f = Fixture::new(locale, false, Page::Output);
        let enable = f.find("button", "output-obs-enable");
        let source = f.find(
            "text",
            if cfg!(windows) {
                "output-obs-step-source"
            } else {
                "output-obs-step-syphon-source"
            },
        );
        assert!(enable.bounds.y + enable.bounds.height <= source.bounds.y);
        f.click(&enable);
        f.apply_edits();
        let s = f.ui.settings.get_untracked();
        assert!(
            s.output.display.enabled
                && !s.output.partners.iter().any(|o| o.enabled)
                && !s.output.history.enabled
        );
        assert!(f.find("button", "output-obs-enabled").disabled);
    }
}

#[test]
fn skin_preview_fits_first_run_and_settings_through_resizes() {
    for locale in Locale::ALL {
        for onboarding in [true, false] {
            let mut f = Fixture::new(
                locale,
                onboarding,
                if onboarding {
                    Page::General
                } else {
                    Page::Skins
                },
            );
            f.ui.onboard_step.set(1);
            let skin = f.ui.skin.get_untracked();
            let widths: Vec<_> = [(720, 480), (920, 660), (1280, 800), (720, 480)]
                .into_iter()
                .map(|(w, h)| {
                    f.session.set_viewport(w, h, 1.0).unwrap();
                    let sample_width = f.preview(w, &skin);
                    let preview = f.keyed("skin-preview-stage-column").bounds;
                    let controls = f.keyed("skin-preview-controls-column").bounds;
                    assert!(sample_width <= preview.width - 24.0 + 0.5);
                    if w == 720 {
                        assert!((preview.x - controls.x).abs() < 1.0);
                        assert!(controls.y >= preview.y + preview.height - 1.0);
                    } else {
                        assert!(controls.x >= preview.x + preview.width - 1.0);
                    }
                    sample_width
                })
                .collect();
            assert!(widths[1] < widths[2], "not responsive: {widths:?}");
            assert!((widths[0] - widths[3]).abs() < 0.5);
        }
    }
}

#[test]
fn onboarding_skin_step_enters_before_it_mounts_the_preview() {
    let mut f = Fixture::new(Locale::ZhCn, true, Page::General);
    f.ui.onboard_skin_shown.set(false);
    f.ui.onboard_step.set(1);
    f.session.flush().unwrap();
    let paths = |f: &Fixture| {
        f.session
            .accessibility_dump()
            .into_iter()
            .filter_map(|n| n.agent_path)
            .collect::<Vec<_>>()
    };
    let held = paths(&f);
    assert!(
        held.iter().all(|p| !p.ends_with("skin-preview")),
        "{held:?}"
    );
    f.ui.onboard_skin_shown.set(true);
    f.session.flush().unwrap();
    let shown = paths(&f);
    assert!(
        shown.iter().any(|p| p.ends_with("skin-preview")),
        "{shown:?}"
    );
}

#[test]
fn onboarding_skin_step_enables_the_live_preview_clock() {
    let f = Fixture::new(Locale::ZhCn, true, Page::General);
    assert!(!f.ui.live_preview.get());
    f.ui.onboard_step.set(1);
    assert!(f.ui.live_preview.get());
    f.ui.onboard_step.set(2);
    assert!(!f.ui.live_preview.get());
    f.ui.page.set(Page::Skins);
    assert!(f.ui.live_preview.get());
}

#[test]
fn builtin_skins_can_be_previewed_and_applied_in_a_narrow_window() {
    for locale in Locale::ALL {
        let mut f = Fixture::new(locale, false, Page::Skins);
        f.ui.skins_grid.set(false);
        let rows = f.ui.skins.get_untracked();
        f.session.flush().unwrap();
        for row in rows.iter() {
            let skin = row.skin.as_ref().expect("builtin skin must load");
            if row.id == f.ui.settings.get_untracked().skin.id {
                continue;
            }
            let item = f.reach(650.0, 480.0, |f| {
                f.session
                    .accessibility_dump()
                    .into_iter()
                    .find(|n| n.role == "list-item" && n.label.as_deref() == Some(&row.name))
                    .unwrap()
            });
            // Click the swatches, away from the row's action buttons.
            f.session
                .click_xy(
                    item.bounds.x + 35.0,
                    item.bounds.y + item.bounds.height / 2.0,
                )
                .unwrap();
            f.prepare(&row.id);
            f.preview(720, skin);
            let apply = f.reach(650.0, 480.0, |f| f.keyed("apply-preview"));
            assert!(apply.bounds.x >= 196.0 && apply.bounds.x + apply.bounds.width <= 720.0);
            f.click(&apply);
            f.apply_edits();
            let s = f.ui.settings.get_untracked();
            assert_eq!(s.skin.id, row.id);
            f.ui.skin.set(Arc::new(nanatype::skin_theme::prepare(
                skin,
                &s.skin.overrides,
                &Default::default(),
                locale.tag(),
            )));
            f.session.flush().unwrap();
            assert!(!f.session.accessibility_dump().iter().any(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.ends_with("/apply-preview"))
            }));
        }
    }
}
