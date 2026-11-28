//! Isolated, unsaved skin editing. Only Save dispatches an application message.
mod preview_session;
mod preview_stage;
mod skin_editor;

use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{
    AnyView, IntoView, Signal, dynamic, entity_ref, on_mount, signal, widget,
};
use nana_ui::runtime::{Activate, SizeChanged, Stack};
use nana_ui_core::{AlignSpec, LengthSpec};
pub use nanatype_frame::ui::preview::{
    Draft, InvalidProfile, Pane, StageAssets, Surface, default_size, pair_extent, sample, stage,
    stage_pair, stage_pair_placeholder, stage_placeholder, stage_view,
};

use self::preview_session::param_schema_version;
use self::preview_stage::preview_stage;
use self::skin_editor::{Host, action, controls};
use super::controls::hint;
use crate::msg::{Msg, SkinMsg};
use crate::ui::Ui;
use crate::ui::kit::{self, gap};

pub use self::preview_session::load_draft;
pub(in crate::ui::settings) use self::preview_session::select;
#[cfg(test)]
pub(in crate::ui::settings) use self::preview_session::{change, draft};
pub(in crate::ui::settings) use self::skin_editor::{guide_save, guide_tools};

/// The stage and the fields side by side: each column's basis and the
/// least it narrows to before they stack.
const COLUMN_W: f32 = 300.0;
const COLUMN_MIN_W: f32 = 280.0;

pub fn view(ui: Ui, previewing: Signal<Option<String>>) -> AnyView {
    view_in(ui, previewing, Host::Page)
}

/// The active skin's preview and fields as the first-run guide shows them.
pub fn guide_view(ui: Ui) -> AnyView {
    view_in(ui, signal(None), Host::Guide)
}

fn view_in(ui: Ui, previewing: Signal<Option<String>>, host: Host) -> AnyView {
    let width = signal(0.0_f32);
    dynamic(
        move || {
            let id = previewing
                .get()
                .unwrap_or_else(|| ui.settings.with(|s| s.skin.id.clone()));
            let version = ui
                .skins
                .with(|rows| param_schema_version(rows.iter().find(|r| r.id == id)));
            (id, version, ui.locale.get())
        },
        move |(id, _, _)| {
            let id = id.clone();
            let row = ui
                .skins
                .with_untracked(|rows| rows.iter().find(|r| r.id == id).cloned());
            let Some(row) = row.filter(|r| r.skin.is_some() && r.error.is_none()) else {
                return hint(ui.now_t("skin-edit-unavailable"));
            };
            let skin = row.skin.clone().expect("checked above");
            let fallback_font = row.fallback_font;
            let apply_id = id.clone();
            let apply_read = id.clone();
            let ask = id.clone();
            let root = entity_ref::<Stack>();
            on_mount(move |cx| {
                if let Some(root) = root.get() {
                    let _ = cx.update(root, |_: &mut Stack, cx| {
                        cx.dispatch_program_all(Msg::Skin(SkinMsg::PreparePreview {
                            id: ask.clone(),
                            play: true,
                        }));
                    });
                }
            });
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::GROUP)
                    .width(LengthSpec::Fill),
            )
            .entity_ref(root)
            .key("skin-preview")
            .children((
                // The guide previews the active skin only: nothing to apply.
                (host == Host::Page).then(|| {
                    widget(
                        Stack::row(0.0)
                            .gap_step(gap::step::CONTROL)
                            .align(AlignSpec::Center)
                            .width(LengthSpec::Fill),
                    )
                    .children((
                        kit::spacer(),
                        widget(action(ui, ti::CHECK, "common-apply"))
                            .key("apply-preview")
                            .visible(move || ui.settings.with(|s| s.skin.id != apply_read))
                            .on_cx(move |_, _: &Activate, cx| {
                                let id = apply_id.clone();
                                cx.dispatch_program_all(super::controls::edit(move |s| {
                                    s.skin.id = id
                                }));
                            }),
                    ))
                }),
                widget(
                    Stack::row(0.0)
                        .gap_step(gap::step::GROUP)
                        .wrap(true)
                        .align(AlignSpec::Start)
                        .width(LengthSpec::Fill),
                )
                .children((
                    widget(
                        Stack::column(0.0)
                            .width(LengthSpec::Px(COLUMN_W))
                            .grow(1.0)
                            .shrink(1.0)
                            .with_layout(|l| l.min_width = Some(LengthSpec::Px(COLUMN_MIN_W))),
                    )
                    .key("skin-preview-stage-column")
                    .on(move |e: &SizeChanged| {
                        width.set_if_changed(e.width);
                    })
                    .children(preview_stage(ui, row, width)),
                    widget(
                        Stack::column(0.0)
                            .width(LengthSpec::Px(COLUMN_W))
                            .grow(1.0)
                            .shrink(1.0)
                            .with_layout(|l| l.min_width = Some(LengthSpec::Px(COLUMN_MIN_W))),
                    )
                    .key("skin-preview-controls-column")
                    .children(controls(ui, &id, skin, fallback_font, host)),
                )),
            ))
            .into_any()
        },
    )
    .into_any()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nana_ui::runtime::{DocumentId, RuntimeDocument};
    use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
    use nanatype_settings::{Settings, SurfacePersonalization};
    use nanatype_theme::Skin;

    use super::skin_editor::{follows_skin, line_marks};
    use super::*;

    fn fixture() -> (Ui, Arc<Skin>) {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let skin = registry.load("nanatype.halo").unwrap().skin;
        let ui = Ui::new(
            Arc::new(Settings::default()),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );
        ui.skins.set(Arc::new(crate::ui::skin_rows(
            &registry,
            nanatype_i18n::Locale::ZhCn,
        )));
        (ui, skin)
    }

    fn click(session: &mut RuntimeAgentSession, name: &str) {
        session.flush().unwrap();
        let b = session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.role == "button" && n.label.as_deref() == Some(name))
            .unwrap()
            .bounds;
        session
            .click_xy(b.x + b.width / 2.0, b.y + b.height / 2.0)
            .unwrap();
        session.flush().unwrap();
    }

    #[test]
    fn drafts_keep_each_skin_and_surface_separate_and_validate_both_before_save() {
        let (ui, base) = fixture();
        let original = ui.settings.get_untracked();
        change(ui, "nanatype.halo", |d| {
            d.display.size = "48".into();
            d.history.line_height = "2".into();
        });
        change(ui, "nanatype.night", |d| d.display.size = "32".into());
        let p = draft(ui, "nanatype.halo").profile(&base).unwrap();
        assert_eq!(p.display.text_size, Some(48.0));
        assert_eq!(p.history.text_size, None);
        assert_eq!(p.history.line_height, Some(2.0));
        assert_eq!(draft(ui, "nanatype.night").display.size, "32");
        assert!(Arc::ptr_eq(&ui.settings.get_untracked(), &original));
        for bad in ["NaN", "inf", "201", "7", "abc"] {
            change(ui, "nanatype.halo", |d| d.history.size = bad.into());
            assert!(draft(ui, "nanatype.halo").profile(&base).is_err(), "{bad}");
        }
        change(ui, "nanatype.halo", |d| {
            d.history.size.clear();
            d.display.width = "300".into();
        });
        let p = draft(ui, "nanatype.halo").profile(&base).unwrap();
        assert_eq!(p.display.size.unwrap()[0], 300.0);
        assert_eq!(
            p.display.size.unwrap()[1],
            base.dialogue.region[1] + 2.0 * base.dialogue.overscan
        );
    }

    #[test]
    fn native_controls_edit_only_the_draft_and_save_the_browsed_skin_without_activation() {
        let (ui, base) = fixture();
        let active = ui.settings.get_untracked();
        let source = ui.source.get_untracked();
        let history = ui.history.get_untracked();
        let id = "nanatype.night";
        let doc_id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                controls(ui, id, render_base, false, Host::Page)
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1400).unwrap();
        session.flush().unwrap();
        let input = session
            .accessibility_dump()
            .into_iter()
            .find(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|id| id.ends_with("skin-edit-size-display"))
            })
            .expect("body size field");
        session.click_node(input.id).unwrap();
        session.key_press("End", "End", Default::default()).unwrap();
        for _ in 0..4 {
            session
                .key_press("Backspace", "Backspace", Default::default())
                .unwrap();
        }
        session.type_text("42").unwrap();
        // A number field commits on Enter, not per keystroke.
        assert_eq!(draft(ui, id).display.size, "");
        session
            .key_press("Enter", "Enter", Default::default())
            .unwrap();
        assert_eq!(draft(ui, id).display.size, "42");
        assert_eq!(ui.settings.get_untracked(), active);
        assert_eq!(ui.source.get_untracked(), source);
        assert!(Arc::ptr_eq(&ui.history.get_untracked(), &history));
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
        change(ui, id, |d| d.history.size = "201".into());
        session.flush().unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(
                    |n| n.label.as_deref() == Some(ui.now_t("skin-edit-save").as_str())
                        && n.disabled
                )
        );
        click(&mut session, &ui.now_t("skin-edit-save"));
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
        change(ui, id, |d| d.history.size.clear());
        session.flush().unwrap();
        click(&mut session, &ui.now_t("skin-edit-save"));
        let messages = session.document_mut().context_mut().take_program_messages();
        let saved = messages
            .into_iter()
            .find_map(|m| m.downcast::<Msg>().ok())
            .unwrap();
        let Msg::Skin(SkinMsg::SavePersonalization {
            id: saved_id,
            profile,
        }) = *saved
        else {
            panic!("save message");
        };
        assert_eq!(saved_id, id);
        assert_eq!(profile.display.text_size, Some(42.0));
        assert_eq!(ui.settings.get_untracked(), active);
        click(&mut session, &ui.now_t("skin-edit-reset"));
        let restored = draft(ui, id).profile(&base).unwrap();
        assert_eq!(restored.display, SurfacePersonalization::default());
        assert_eq!(restored.history, SurfacePersonalization::default());
        change(ui, id, |d| d.history.size = "72".into());
        click(&mut session, &ui.now_t("skin-edit-discard"));
        assert!(!ui.skin_drafts.with_untracked(|m| m.contains_key(id)));
    }

    #[test]
    fn name_plate_and_ornament_fields_sit_with_the_display_and_edit_this_skins_draft() {
        let (ui, base) = fixture();
        let active = ui.settings.get_untracked();
        let id = "nanatype.night";
        let doc_id = DocumentId::new(11).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                controls(ui, id, render_base, false, Host::Page)
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1600).unwrap();
        session.flush().unwrap();
        let find = |session: &mut RuntimeAgentSession, key: &str| {
            session
                .accessibility_dump()
                .into_iter()
                .filter(|n| n.agent_path.as_deref().is_some_and(|p| p.ends_with(key)))
                .collect::<Vec<_>>()
        };
        // One of each: the display has them, the history does not.
        for key in [
            "skin-part-name-visible",
            "skin-part-name-scale",
            "skin-part-decor-scale",
        ] {
            // A slider and its back-to-default button share the field's path.
            let mut paths: Vec<_> = find(&mut session, key)
                .into_iter()
                .filter_map(|n| n.agent_path)
                .collect();
            paths.dedup();
            assert_eq!(paths.len(), 1, "{key}");
        }
        let switch = find(&mut session, "skin-part-name-visible")[0].id;
        session.click_node(switch).unwrap();
        session.flush().unwrap();
        assert_eq!(draft(ui, id).name_visible, Some(false));
        // Only the draft changes; nothing is saved yet.
        assert_eq!(ui.settings.get_untracked(), active);
        change(ui, id, |d| d.name_scale = Some(1.5));
        click(&mut session, &ui.now_t("skin-edit-save"));
        let saved = session
            .document_mut()
            .context_mut()
            .take_program_messages()
            .into_iter()
            .find_map(|m| m.downcast::<Msg>().ok())
            .unwrap();
        let Msg::Skin(SkinMsg::SavePersonalization {
            id: saved_id,
            profile,
        }) = *saved
        else {
            panic!("save message");
        };
        assert_eq!(saved_id, id);
        assert_eq!(profile.name_visible, Some(false));
        assert_eq!(profile.name_scale, Some(1.5));
        assert_eq!(profile.decor_scale, None);
        // Reset takes them back to the skin's own.
        click(&mut session, &ui.now_t("skin-edit-reset"));
        let d = draft(ui, id);
        assert_eq!((d.name_visible, d.name_scale), (None, None));
        assert!(follows_skin(&d));
    }

    #[test]
    fn the_guide_saves_the_active_draft_on_next_and_undoes_beside_the_picker() {
        let (ui, _) = fixture();
        let id = ui.settings.with_untracked(|s| s.skin.id.clone());
        // Nothing touched: nothing to save.
        assert!(matches!(guide_save(ui), Ok(None)));
        change(ui, &id, |d| d.display.size = "40".into());
        let Ok(Some(Msg::Skin(SkinMsg::SavePersonalization { id: saved, profile }))) =
            guide_save(ui)
        else {
            panic!("a save for the active skin");
        };
        assert_eq!(saved, id);
        assert_eq!(profile.display.text_size, Some(40.0));
        // Fields that do not make a profile hold Next.
        change(ui, &id, |d| d.history.size = "abc".into());
        assert!(guide_save(ui).is_err());

        let doc_id = DocumentId::new(9).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || guide_tools(ui))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 200, 60).unwrap();
        // Reset keeps the draft but clears every value.
        click(&mut session, &ui.now_t("skin-edit-reset"));
        assert!(follows_skin(&draft(ui, &id)));
        assert!(ui.skin_drafts.with_untracked(|m| m.contains_key(&id)));
        click(&mut session, &ui.now_t("skin-edit-discard"));
        assert!(!ui.skin_drafts.with_untracked(|m| m.contains_key(&id)));
        session.flush().unwrap();
        // With nothing to undo, both are disabled.
        let dump = session.accessibility_dump();
        for key in ["skin-edit-reset", "skin-edit-discard"] {
            assert!(
                dump.iter()
                    .any(|n| n.label.as_deref() == Some(ui.now_t(key).as_str()) && n.disabled),
                "{key}"
            );
        }
    }

    #[test]
    fn line_height_snaps_to_common_values_and_the_skins_own() {
        let values = |default| {
            line_marks(default)
                .into_iter()
                .map(|m| m.value)
                .collect::<Vec<_>>()
        };
        // The skin's value stands in for the snap too close to it.
        assert_eq!(values(1.45), [1.0, 1.2, 1.45, 2.0, 2.5, 3.0]);
        assert_eq!(values(1.5), [1.0, 1.2, 1.5, 2.0, 2.5, 3.0]);
        assert_eq!(values(1.75), [1.0, 1.2, 1.5, 1.75, 2.0, 2.5, 3.0]);
    }

    #[test]
    fn the_guide_layout_has_no_save_row() {
        let (ui, base) = fixture();
        let id = "nanatype.halo";
        let doc_id = DocumentId::new(10).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                controls(ui, id, base.clone(), false, Host::Guide)
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1400).unwrap();
        session.flush().unwrap();
        let dump = session.accessibility_dump();
        for key in [
            "skin-edit-save",
            "skin-edit-discard",
            "skin-edit-reset",
            "skin-edit-saved",
            "skin-edit-draft-hint",
        ] {
            let label = ui.now_t(key);
            assert!(
                !dump
                    .iter()
                    .any(|n| n.label.as_deref() == Some(label.as_str())),
                "{key}"
            );
        }
        for key in ["windows-display", "windows-history"] {
            let label = ui.now_t(key);
            assert!(
                dump.iter()
                    .any(|n| n.label.as_deref() == Some(label.as_str())),
                "{key}"
            );
        }
    }

    fn commit_size(session: &mut RuntimeAgentSession, key: &str, text: &str) {
        session.flush().unwrap();
        let input = session
            .accessibility_dump()
            .into_iter()
            .find(|n| n.agent_path.as_deref().is_some_and(|id| id.ends_with(key)))
            .unwrap_or_else(|| panic!("missing {key}"));
        session.click_node(input.id).unwrap();
        session.key_press("End", "End", Default::default()).unwrap();
        for _ in 0..4 {
            session
                .key_press("Backspace", "Backspace", Default::default())
                .unwrap();
        }
        session.type_text(text).unwrap();
        session
            .key_press("Enter", "Enter", Default::default())
            .unwrap();
    }

    #[test]
    fn each_surface_has_its_own_font_picker_and_typed_values_follow_the_skin() {
        let (ui, base) = fixture();
        let id = "nanatype.halo";
        let doc_id = DocumentId::new(7).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                controls(ui, id, render_base, false, Host::Page)
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1400).unwrap();
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        let fonts: Vec<_> = nodes
            .iter()
            .filter(|n| {
                n.agent_path.as_deref().is_some_and(|p| {
                    p.rsplit('/')
                        .next()
                        .is_some_and(|k| k.starts_with("skin-edit-font-"))
                })
            })
            .collect();
        assert_eq!(fonts.len(), 2, "{fonts:?}");
        for key in ["windows-display", "windows-history"] {
            assert!(
                nodes
                    .iter()
                    .any(|n| n.label.as_deref() == Some(ui.now_t(key).as_str())),
                "{key}"
            );
        }
        // The text buttons are icon buttons now, still named by their text.
        for key in ["skin-edit-reset", "skin-edit-discard", "skin-edit-save"] {
            assert!(
                nodes
                    .iter()
                    .any(|n| n.role == "button"
                        && n.label.as_deref() == Some(ui.now_t(key).as_str())),
                "{key}"
            );
        }

        commit_size(&mut session, "skin-edit-size-display", "40");
        assert_eq!(draft(ui, id).display.size, "40");
        let default = base.dialogue.text.size;
        commit_size(&mut session, "skin-edit-size-display", &default.to_string());
        assert_eq!(draft(ui, id).display.size, "");

        // The font list keeps a family typed before it was offered.
        change(ui, id, |d| d.display.font = "Some Old Family".into());
        session.flush().unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some("Some Old Family")
                    || n.value.as_deref() == Some("Some Old Family")),
        );
    }

    #[test]
    fn resolution_field_edits_both_sides_and_its_presets_follow_the_skin() {
        let (ui, base) = fixture();
        let id = "nanatype.halo";
        let doc_id = DocumentId::new(7).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                controls(ui, id, render_base, false, Host::Page)
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1400).unwrap();
        session.flush().unwrap();
        let key = "skin-edit-resolution-display";
        let node = |session: &mut RuntimeAgentSession, role: &str, label: &str| {
            session.flush().unwrap();
            session
                .accessibility_dump()
                .into_iter()
                .find(|n| {
                    n.role == role
                        && n.label.as_deref() == Some(label)
                        && n.agent_path.as_deref().is_some_and(|p| p.ends_with(key))
                })
                .unwrap_or_else(|| panic!("missing {role} {label}"))
                .id
        };
        let [w, h] = default_size(&base, false).map(f32::round);

        // A typed width lands in the draft; the height keeps following the skin.
        let width = node(&mut session, "text-input", &ui.now_t("skin-edit-width"));
        session.click_node(width).unwrap();
        session.key_press("End", "End", Default::default()).unwrap();
        for _ in 0..5 {
            session
                .key_press("Backspace", "Backspace", Default::default())
                .unwrap();
        }
        session.type_text("900").unwrap();
        session
            .key_press("Enter", "Enter", Default::default())
            .unwrap();
        assert_eq!(draft(ui, id).display.width, "900");
        assert_eq!(draft(ui, id).display.height, "");

        let pick = |session: &mut RuntimeAgentSession, label: String| {
            let menu = node(session, "menu", &ui.now_t("skin-edit-size-presets"));
            session.click_node(menu).unwrap();
            session.flush().unwrap();
            let item = session
                .accessibility_dump()
                .into_iter()
                .find(|n| n.label.as_deref() == Some(label.as_str()))
                .unwrap_or_else(|| panic!("missing preset {label}"))
                .id;
            session.click_node(item).unwrap();
            session.flush().unwrap();
        };
        pick(&mut session, format!("{}% · {}×{}", 200, w * 2.0, h * 2.0));
        assert_eq!(draft(ui, id).display.width, (w * 2.0).to_string());
        assert_eq!(draft(ui, id).display.height, (h * 2.0).to_string());
        // The skin's own size clears the draft, so it follows the skin again.
        pick(
            &mut session,
            format!("{} · {w}×{h}", ui.now_t("common-skin-default")),
        );
        assert_eq!(draft(ui, id).display.width, "");
        assert_eq!(draft(ui, id).display.height, "");
        let history = draft(ui, id).history;
        assert_eq!(
            (history.width, history.height),
            (String::new(), String::new())
        );
    }

    /// A standalone skin with one param of each kind and a preset.
    fn param_skin() -> Arc<Skin> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(nanatype_theme::MANIFEST),
            r##"
[meta]
id = "test.params"
[params.fur]
type = "color"
default = "#fff7ec"
name = { zh-CN = "毛色" }
[params.size]
type = "number"
default = 28
min = 16
max = 40
step = 2
[params.align]
type = "enum"
default = "start"
options = [{ id = "start" }, { id = "center" }]
[params.shadow]
type = "bool"
default = false
[presets.night]
name = { zh-CN = "夜晚" }
values = { fur = "#2b2730", size = 32 }
[dialogue.box]
color = { param = "fur" }
[dialogue.text]
size = { param = "size" }
align = { param = "align" }
"##,
        )
        .unwrap();
        let files: Arc<dyn nanatype_theme::SkinFiles> =
            Arc::new(nanatype_theme::DirFiles::new(dir.path()));
        let loaded = nanatype_theme::load(
            files,
            nanatype_theme::SkinOrigin::Directory(dir.path().into()),
            &(),
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        loaded.skin
    }

    #[test]
    fn param_controls_edit_the_draft_keep_valid_values_and_save_them() {
        let (ui, _) = fixture();
        let base = param_skin();
        let id = "test.params";
        let doc_id = DocumentId::new(5).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let render_base = base.clone();
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                controls(ui, id, render_base, false, Host::Page)
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 700, 1800).unwrap();
        session.flush().unwrap();
        let fur = session
            .accessibility_dump()
            .into_iter()
            .find(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.contains("skin-param-fur"))
            })
            .expect("hex field for the colour param");
        session.click_node(fur.id).unwrap();
        session.type_text("zz").unwrap();
        session.flush().unwrap();
        // Unparsable text stays in the field; the sample keeps the last
        // valid colour and the draft can still be saved.
        let d = draft(ui, id);
        assert!(d.params.raw.contains_key("fur"));
        assert!(!d.params.values.contains_key("fur"));
        assert!(d.profile(&base).unwrap().params.is_empty());
        assert!(session.accessibility_dump().iter().any(|n| {
            n.label.as_deref() == Some(ui.now_t("skin-params-invalid-color").as_str())
        }));

        change(ui, id, |d| {
            let _ = d.params.input(&base, "fur", "#102030");
        });
        let p = draft(ui, id).profile(&base).unwrap();
        assert_eq!(
            p.params.get("fur"),
            Some(&nanatype_model::ParamValue::Text("#102030".into()))
        );

        // A preset chip applies its values; matching values light it up.
        click(&mut session, "夜晚");
        let d = draft(ui, id);
        assert_eq!(d.params.matching_preset(&base), Some("night"));
        assert!(d.params.raw.is_empty());
        click(&mut session, &ui.now_t("skin-edit-save"));
        let saved = session
            .document_mut()
            .context_mut()
            .take_program_messages()
            .into_iter()
            .find_map(|m| m.downcast::<Msg>().ok())
            .unwrap();
        let Msg::Skin(SkinMsg::SavePersonalization { profile, .. }) = *saved else {
            panic!("save message");
        };
        // Only values that differ from the defaults are kept.
        assert_eq!(profile.params.len(), 2);
        assert_eq!(
            profile.params.get("size"),
            Some(&nanatype_model::ParamValue::Number(32.0))
        );

        click(&mut session, &ui.now_t("skin-params-default"));
        assert!(draft(ui, id).params.values.is_empty());
        change(ui, id, |d| {
            d.params
                .set("shadow", nanatype_model::ParamValue::Bool(true))
        });
        click(&mut session, &ui.now_t("skin-edit-reset"));
        assert!(draft(ui, id).params.values.is_empty());
    }

    #[test]
    fn sample_does_not_write_to_live_settings_history_or_editors() {
        let (ui, base) = fixture();
        let settings = ui.settings.get_untracked();
        let history = ui.history.get_untracked();
        let source = ui.source.get_untracked();
        let draft = ui.draft.get_untracked();
        let mut edited = load_draft(&settings, &settings.skin.id, ui);
        edited.history.size = "56".into();
        let doc_id = DocumentId::new(2).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                sample(
                    *ui,
                    &base,
                    "nanatype.night",
                    &edited,
                    Surface::History,
                    Arc::default(),
                )
                .unwrap()
                .0
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 300, 560).unwrap();
        session.flush().unwrap();
        assert_eq!(ui.settings.get_untracked(), settings);
        assert!(Arc::ptr_eq(&ui.history.get_untracked(), &history));
        assert!(Arc::ptr_eq(&ui.draft.get_untracked(), &draft));
        assert_eq!(ui.source.get_untracked(), source);
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
    }

    fn node_keys(session: &mut RuntimeAgentSession) -> Vec<String> {
        session.flush().unwrap();
        session
            .accessibility_dump()
            .into_iter()
            .filter_map(|n| n.agent_path)
            .filter_map(|p| p.rsplit('/').next().map(str::to_owned))
            .collect()
    }

    #[test]
    fn the_page_shows_before_its_stage_and_a_repeated_selection_keeps_it() {
        let (ui, _) = fixture();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let id = "nanatype.sakura";
        let doc_id = DocumentId::new(6).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        let previewing = signal(Some(id.to_owned()));
        doc.context_mut()
            .mount_view_root(doc_id, move || view(ui, previewing))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 720, 1400).unwrap();
        let keys = node_keys(&mut session);
        assert!(keys.iter().any(|k| k == "skin-preview-loading"), "{keys:?}");
        assert!(
            !keys.iter().any(|k| k == "skin-preview-display"),
            "{keys:?}"
        );
        assert!(
            !keys.iter().any(|k| k == "skin-preview-history"),
            "{keys:?}"
        );
        assert!(keys.iter().any(|k| k == "apply-preview"));
        assert!(!keys.iter().any(|k| k == "play-effects"), "{keys:?}");
        assert!(ui.skin_preview.get_untracked().is_none());
        let messages: Vec<_> = session
            .document_mut()
            .context_mut()
            .take_program_messages()
            .into_iter()
            .filter_map(|m| m.downcast::<Msg>().ok().map(|b| *b))
            .collect();
        assert!(matches!(
            messages.as_slice(),
            [Msg::Skin(SkinMsg::PreparePreview { id: got_id, play: true })] if got_id == id
        ));

        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        let keys = node_keys(&mut session);
        assert!(
            keys.iter().any(|k| k == "skin-preview-display-box"),
            "{keys:?}"
        );
        assert!(
            keys.iter().any(|k| k == "skin-preview-history-box"),
            "{keys:?}"
        );
        assert!(keys.iter().any(|k| k == "skin-preview-display"), "{keys:?}");
        assert!(keys.iter().any(|k| k == "skin-preview-history"), "{keys:?}");
        assert!(!keys.iter().any(|k| k == "skin-preview-loading"));
        // Preparing the stage updates only the stage dynamic; the editor dynamic
        // does not remount or re-request preparation.
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .is_empty()
        );
        let stage = ui.skin_preview.get_untracked().unwrap();
        let rows = ui.skins.get_untracked();
        // Selecting it again, with or without effects, keeps the stage and
        // the catalogue as they are.
        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        crate::ui::prepare_skin_preview(ui, &registry, id, true).unwrap();
        assert!(Arc::ptr_eq(
            &ui.skin_preview.get_untracked().unwrap(),
            &stage
        ));
        assert!(Arc::ptr_eq(&ui.skins.get_untracked(), &rows));
    }

    #[test]
    fn tall_and_maximum_previews_scale_the_entire_logical_canvas() {
        let (ui, _) = fixture();
        let id = "nanatype.halo";
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        let doc_id = DocumentId::new(4).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || view(ui, signal(None)))
            .unwrap();
        let skin = registry.load(id).unwrap().skin;
        let mut session = RuntimeAgentSession::new(doc, 720, 1400).unwrap();
        for history in [false, true] {
            for [w, h] in [
                [300.0, 1200.0],
                [800.0, 2400.0],
                [160.0, 8192.0],
                [8192.0, 8192.0],
                [8192.0, 96.0],
                [300.0, 240.0],
            ] {
                change(ui, id, |d| {
                    let (pane, other) = if history {
                        (&mut d.history, &mut d.display)
                    } else {
                        (&mut d.display, &mut d.history)
                    };
                    pane.width = w.to_string();
                    pane.height = h.to_string();
                    pane.size = "200".into();
                    *other = Pane::default();
                });
                session.flush().unwrap();
                let nodes = session.accessibility_dump();
                let bounds = |key| {
                    nodes
                        .iter()
                        .find(|n| {
                            n.agent_path
                                .as_deref()
                                .is_some_and(|p| p.rsplit('/').next() == Some(key))
                        })
                        .unwrap_or_else(|| panic!("missing {key}"))
                        .bounds
                        .clone()
                };
                let column = bounds("skin-preview-stage-column");
                let content_w = (column.width - 24.0).max(1.0);
                let display_logical = if history {
                    default_size(&skin, false)
                } else {
                    [w, h]
                };
                let history_logical = if history {
                    [w, h]
                } else {
                    default_size(&skin, true)
                };
                let display_expected = (content_w / display_logical[0])
                    .min(340.0 / display_logical[1])
                    .min(1.0);
                let history_expected = content_w / history_logical[0].max(1.0);
                let cases = [
                    (
                        "display",
                        bounds("skin-preview-display-box"),
                        bounds("skin-preview-canvas"),
                        bounds("skin-preview-display"),
                        display_logical,
                        display_expected,
                        "skin-preview-display-metrics",
                    ),
                    (
                        "history",
                        bounds("skin-preview-history-box"),
                        bounds("skin-preview-history-canvas"),
                        bounds("skin-preview-history"),
                        history_logical,
                        history_expected,
                        "skin-preview-history-metrics",
                    ),
                ];
                for (name, stage, canvas, content, logical, expected, metrics) in cases {
                    let scale = content.width / logical[0];
                    assert!(
                        (content.height - logical[1] * scale).abs() < 1.0,
                        "{history} {name} {w}x{h}: scale {scale} {content:?}"
                    );
                    assert!(
                        (scale - expected).abs() < 0.02,
                        "{history} {name} {w}x{h}: scale {scale} expected {expected}"
                    );
                    for actual in [&stage, &canvas, &content] {
                        assert!(
                            actual.x >= column.x
                                && actual.x + actual.width <= column.x + column.width + 0.5,
                            "{history} {name} {w}x{h}: {actual:?}"
                        );
                    }
                    assert!((stage.x - canvas.x).abs() < 0.5 && (stage.y - canvas.y).abs() < 0.5);
                    let label = bounds(metrics);
                    assert!(
                        label.y + label.height <= canvas.y + 1.0,
                        "{history} {name} metrics cover the render: {label:?} {canvas:?}"
                    );
                    assert!(
                        (stage.width - canvas.width).abs() < 0.5
                            && (stage.height - canvas.height).abs() < 0.5
                    );
                    assert!(
                        nodes.iter().any(|n| n
                            .agent_path
                            .as_deref()
                            .is_some_and(|p| p.ends_with(metrics))),
                        "{history} {name} missing {metrics}"
                    );
                    let caption = format!(
                        "{} × {} px · {:.0}%",
                        logical[0].round(),
                        logical[1].round(),
                        expected * 100.0
                    );
                    assert!(
                        nodes.iter().any(|n| {
                            n.label.as_deref() == Some(caption.as_str())
                                || n.value.as_deref() == Some(caption.as_str())
                        }),
                        "{history} {name} missing {caption}"
                    );
                }
                assert!(
                    (bounds("skin-preview-history-box").width - content_w).abs() < 1.5,
                    "history does not fill the column"
                );
                assert!(bounds("skin-preview-display-box").width <= display_logical[0] + 0.5);
            }
        }
    }

    #[test]
    fn both_surfaces_edit_their_own_pane_without_switching() {
        let (ui, _) = fixture();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let id = "nanatype.halo";
        crate::ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        let doc_id = DocumentId::new(8).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || view(ui, signal(None)))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 900, 2400).unwrap();
        session.flush().unwrap();
        let dump = session.accessibility_dump();
        let display = ui.now_t("windows-display");
        let history_label = ui.now_t("windows-history");
        assert!(
            dump.iter().all(|n| {
                n.role != "button"
                    || !matches!(
                        n.label.as_deref(),
                        Some(label) if label == display || label == history_label
                    )
            }),
            "the surfaces are not a switch"
        );
        for key in [
            "skin-edit-size-display",
            "skin-edit-size-history",
            "skin-edit-font-display",
            "skin-edit-font-history",
        ] {
            assert!(
                dump.iter()
                    .any(|n| n.agent_path.as_deref().is_some_and(|p| p.ends_with(key))),
                "{key}"
            );
        }
        assert!(
            dump.iter().any(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.ends_with("skin-preview-display"))
            }),
            "display render is shown"
        );
        let has = |key: &str| {
            let label = ui.now_t(key);
            dump.iter()
                .any(|n| n.label.as_deref() == Some(label.as_str()))
        };
        for key in [
            "skin-edit-long-name",
            "skin-edit-history-viewer-name",
            "skin-edit-history-fan-name",
            "skin-edit-history-captain-name",
            "history-role-host",
            "history-role-viewer",
        ] {
            assert!(has(key), "{key} is shown");
        }
        commit_size(&mut session, "skin-edit-size-history", "42");
        let edited = draft(ui, id);
        assert_eq!(edited.history.size, "42");
        assert_eq!(edited.display.size, "");
        commit_size(&mut session, "skin-edit-size-display", "36");
        let edited = draft(ui, id);
        assert_eq!(edited.display.size, "36");
        assert_eq!(edited.history.size, "42");
    }

    #[test]
    fn minimum_history_keeps_room_for_messages_and_restores_its_title_when_enlarged() {
        let (ui, _) = fixture();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let base = registry.load("nanatype.celestial").unwrap().skin;
        let edited = load_draft(&ui.settings.get_untracked(), "nanatype.celestial", ui);
        let doc_id = DocumentId::new(3).unwrap();
        let mut doc = RuntimeDocument::new(doc_id);
        doc.context_mut()
            .mount_view_root(doc_id, move || {
                sample(
                    *ui,
                    &base,
                    "nanatype.celestial",
                    &edited,
                    Surface::History,
                    Arc::default(),
                )
                .unwrap()
                .0
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 160, 96).unwrap();
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        assert!(!nodes.iter().any(|n| n.label.as_deref() == Some("HISTORY")));
        for node in &nodes {
            if node.label.as_ref().is_some_and(|s| !s.is_empty()) {
                assert!(
                    node.bounds.x >= -1.0 && node.bounds.x + node.bounds.width <= 161.0,
                    "minimum history overflows: {:?}",
                    node
                );
            }
        }
        session.set_viewport(300, 560, 1.0).unwrap();
        session.flush().unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some("HISTORY"))
        );
    }

    #[test]
    fn an_unsaved_skin_preview_preserves_legacy_window_geometry_and_region() {
        let (ui, base) = fixture();
        let mut settings = (*ui.settings.get_untracked()).clone();
        settings.overlays.display.size = Some([900.0, 240.0]);
        settings.overlays.history.size = Some([300.0, 560.0]);
        let p = load_draft(&settings, "other.skin", ui)
            .profile(&base)
            .unwrap();
        assert_eq!(p.display.size, settings.overlays.display.size);
        assert_eq!(p.history.size, settings.overlays.history.size);
        settings.overlays.display.size = None;
        settings.skin.overrides.region = Some([420.0, 180.0]);
        let p = load_draft(&settings, "other.skin", ui)
            .profile(&base)
            .unwrap();
        assert_eq!(
            p.display.size,
            Some([
                420.0 + 2.0 * base.dialogue.overscan,
                180.0 + 2.0 * base.dialogue.overscan
            ])
        );
    }
}
