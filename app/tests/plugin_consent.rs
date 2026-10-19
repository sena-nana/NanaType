//! Actual consent controls and content-bound execution approval.

mod common;

use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    msg::Msg,
    plugins::{approval_matches, review},
    ui::{self, Ui},
};
use nanatype_i18n::Locale;
use nanatype_settings::{PluginApproval, Settings};
use std::sync::Arc;
fn fixture(root: &std::path::Path) -> nanatype_theme::SkinRegistry {
    for id in ["example.script", "example.assets"] {
        std::fs::create_dir_all(root.join(id)).unwrap();
    }
    std::fs::write(
        root.join("example.assets/package.toml"),
        "format=1\nschema=3\nid='example.assets'\nversion='1.0.0'\ncapabilities=['resources']",
    )
    .unwrap();
    std::fs::write(root.join("example.assets/note.txt"), "data").unwrap();
    std::fs::write(root.join("example.script/package.toml"),"format=1\nschema=3\nid='example.script'\nversion='1.0.0'\ncapabilities=['lua']\nentry='main.lua'\nauthor='Example author'\ncreated_at='2026-10-19'\nlicensed_to='Example Studio'\nlicense='MIT'\n[dependencies]\n'example.assets'='1.0.0'\n").unwrap();
    std::fs::write(
        root.join("example.script/main.lua"),
        "return {view=function() return {views={}} end}",
    )
    .unwrap();
    nanatype_theme::SkinRegistry::new(Some(root.into()), "1.0.0")
}
#[test]
fn approval_requires_explicit_trust_and_exact_script_and_dependency_content() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let first = review(&registry, "example.script").unwrap();
    assert!(!approval_matches(&first, None));
    let mut approval = PluginApproval {
        digest: first.digest.clone(),
        trusted_source: false,
        policy: 1,
    };
    assert!(!approval_matches(&first, Some(&approval)));
    approval.trusted_source = true;
    assert!(approval_matches(&first, Some(&approval)));
    approval.policy = 0;
    assert!(!approval_matches(&first, Some(&approval)));
    approval.policy = 1;
    std::fs::write(
        dir.path().join("example.assets/note.txt"),
        "updated resource",
    )
    .unwrap();
    let after = review(&registry, "example.script").unwrap();
    assert!(!approval_matches(&after, Some(&approval)));
    std::fs::write(
        dir.path().join("example.script/main.lua"),
        "while true do end",
    )
    .unwrap();
    let after = review(&registry, "example.script").unwrap();
    assert!(
        after.source.contains("while true"),
        "reviewing a script must never execute it"
    );
    assert!(!approval_matches(&after, Some(&approval)));
}
fn messages(session: &mut RuntimeAgentSession) -> Vec<Msg> {
    session
        .document_mut()
        .context_mut()
        .take_program_messages()
        .into_iter()
        .filter_map(|m| m.downcast::<Msg>().ok().map(|m| *m))
        .collect()
}
#[test]
fn consent_panel_discloses_risks_and_needs_a_real_trusted_source_selection() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let review = Arc::new(review(&registry, "example.script").unwrap());
    let captures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-consent");
    std::fs::create_dir_all(&captures).unwrap();
    for locale in Locale::ALL {
        for width in [900, 440, 300] {
            let ui = Ui::new(
                Arc::new(Settings::default()),
                Arc::new(Default::default()),
                locale,
            );
            ui.plugin_activation.set(Some(review.clone()));
            let mut session = common::session()
                .themed(false)
                .mount(width, 1800, move || ui::plugins::activation(ui));
            session.flush().unwrap();
            let nodes = session.accessibility_dump();
            for node in nodes.iter().filter(|n| n.role == "button") {
                assert!(
                    node.bounds.x >= 0. && node.bounds.x + node.bounds.width <= width as f32 + 0.5,
                    "button must remain visible at {width}px: {:?}",
                    node.label
                );
            }
            assert!(nodes.iter().any(|n|n.label.as_deref()==Some(ui.now_t("packages-consent-risk").as_str())));
            assert!(nodes.iter().any(|n| n.label.as_deref().is_some_and(|l| {
                l.contains("Example author")
                    && l.contains("Example Studio")
                    && l.contains("2026-10-19")
            })));
            let confirm = nodes
                .iter()
                .find(|n| {
                    n.agent_path
                        .as_deref()
                        .is_some_and(|p| p.ends_with("plugin-consent-confirm"))
                })
                .unwrap();
            assert!(confirm.disabled);
            let confirm_id = confirm.id;
            let _ = session.click_node(confirm_id).unwrap();
            assert!(messages(&mut session).is_empty());
            let checkbox = nodes
                .iter()
                .find(|n| {
                    n.agent_path
                        .as_deref()
                        .is_some_and(|p| p.ends_with("plugin-source-trusted"))
                })
                .unwrap();
            assert!(session.click_node(checkbox.id).unwrap());
            session.flush().unwrap();
            assert!(ui.plugin_source_trusted.get_untracked());
            assert!(
                !session
                    .accessibility_dump()
                    .iter()
                    .find(|n| n.id == confirm_id)
                    .unwrap()
                    .disabled
            );
            session
                .screenshot_png(captures.join(format!("{}-{width}.png", locale.tag())))
                .unwrap();
            assert!(session.click_node(confirm_id).unwrap());
            assert!(
                messages(&mut session)
                    .iter()
                    .any(|m| matches!(m, Msg::ConfirmPluginActivation))
            );
        }
    }
}

#[test]
fn complete_source_can_be_reviewed_without_granting_execution_consent() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let source = format!(
        "{}\nreturn {{}} -- FULL_SOURCE_END",
        "-- inspectable source\n".repeat(900)
    );
    std::fs::write(dir.path().join("example.script/main.lua"), &source).unwrap();
    let review = Arc::new(review(&registry, "example.script").unwrap());
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::EnUs,
    );
    ui.plugin_activation.set(Some(review));
    let mut session = common::session()
        .themed(false)
        .mount(900, 1800, move || ui::plugins::activation(ui));
    session.flush().unwrap();
    let nodes = session.accessibility_dump();
    assert!(!nodes.iter().any(|n| {
        n.value
            .as_deref()
            .is_some_and(|v| v.contains("FULL_SOURCE_END"))
    }));
    let button = nodes
        .iter()
        .find(|n| {
            n.role == "button"
                && n.label.as_deref() == Some(ui.now_t("packages-review-source").as_str())
        })
        .unwrap();
    assert!(session.click_node(button.id).unwrap());
    session.flush().unwrap();
    assert!(
        session
            .accessibility_dump()
            .iter()
            .any(|n| n.value.as_deref().is_some_and(|v| v == source))
    );
    assert!(!ui.plugin_source_trusted.get_untracked());
    assert!(messages(&mut session).is_empty());
}
