//! Actual consent controls and content-bound execution approval.

mod common;

use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    msg::Msg,
    plugins::{approval_matches, changed, review, review_digests},
    ui::{self, Ui},
};
use nanatype_i18n::Locale;
use nanatype_settings::{PLUGIN_CONSENT_POLICY, PluginApproval, Settings};
use std::sync::Arc;
fn fixture(root: &std::path::Path) -> nanatype_theme::SkinRegistry {
    for id in ["example.script", "example.assets"] {
        std::fs::create_dir_all(root.join(id)).unwrap();
    }
    std::fs::write(
        root.join("example.assets/package.toml"),
        "format=1\nschema=3\nid='example.assets'\nversion='1.0.0'\ncapabilities=['resources']\n\
         [data.words]\npath='words.json'\nformat='json'\nformat_version=1\nsource='Example word list'\nlicense='CC0-1.0'\n\
         [data.private]\npath='private.txt'\nformat='text'\nformat_version=1\nsource='Example'\nlicense='CC0-1.0'\n",
    )
    .unwrap();
    std::fs::write(root.join("example.assets/note.txt"), "data").unwrap();
    std::fs::write(root.join("example.assets/words.json"), r#"["hello"]"#).unwrap();
    std::fs::write(root.join("example.assets/private.txt"), "not granted").unwrap();
    std::fs::write(root.join("example.script/package.toml"),"format=1\nschema=3\nid='example.script'\nversion='1.0.0'\ncapabilities=['lua']\nauthor='Example author'\ncreated_at='2026-10-19'\nlicensed_to='Example Studio'\nlicense='MIT'\n[dependencies]\n'example.assets'='1.0.0'\n[lua]\nentry='main.lua'\nreads=['pkg:example.assets/words']\n").unwrap();
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
        policy: PLUGIN_CONSENT_POLICY,
    };
    assert!(!approval_matches(&first, Some(&approval)));
    approval.trusted_source = true;
    assert!(approval_matches(&first, Some(&approval)));
    approval.policy = PLUGIN_CONSENT_POLICY - 1;
    assert!(!approval_matches(&first, Some(&approval)));
    approval.policy = PLUGIN_CONSENT_POLICY;
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
#[test]
fn declared_reads_are_disclosed_and_older_policy_approvals_need_review() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let reviewed = review(&registry, "example.script").unwrap();
    let reads: Vec<_> = reviewed.data.disclosures().collect();
    assert_eq!(reads.len(), 1, "only the declared read is disclosed");
    assert_eq!(
        (
            reads[0].reference.as_str(),
            reads[0].source.as_str(),
            reads[0].license.as_str()
        ),
        ("pkg:example.assets/words", "Example word list", "CC0-1.0")
    );
    let query = serde_json::json!({"kind": "whole"});
    assert_eq!(
        reviewed
            .data
            .answer("pkg:example.assets/words", &query)
            .unwrap(),
        serde_json::json!(["hello"])
    );
    assert!(
        reviewed
            .data
            .answer("pkg:example.assets/private", &query)
            .is_err(),
        "undeclared reads get errors"
    );
    let stage_one = PluginApproval {
        digest: reviewed.digest.clone(),
        trusted_source: true,
        policy: 1,
    };
    assert!(
        !approval_matches(&reviewed, Some(&stage_one)),
        "consent given before data reads existed does not cover them"
    );
    std::fs::write(
        dir.path().join("example.script/package.toml"),
        std::fs::read_to_string(dir.path().join("example.script/package.toml"))
            .unwrap()
            .replace(
                "reads=['pkg:example.assets/words']",
                "reads=['pkg:example.assets/words','pkg:example.assets/private']",
            ),
    )
    .unwrap();
    let widened = review(&registry, "example.script").unwrap();
    assert_ne!(
        widened.digest, reviewed.digest,
        "widening reads needs new consent"
    );
    assert_eq!(widened.data.disclosures().count(), 2);
}
/// Each lifecycle change must invalidate consent given for other content.
#[test]
fn rollback_signature_removal_and_revocation_all_require_new_consent() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let script = dir.path().join("example.script");
    let approve = |digest: &str| PluginApproval {
        digest: digest.into(),
        trusted_source: true,
        policy: PLUGIN_CONSENT_POLICY,
    };
    let v1 = review(&registry, "example.script").unwrap();
    let v1_manifest = std::fs::read_to_string(script.join("package.toml")).unwrap();
    // Update to 1.1.0 and approve it.
    std::fs::write(
        script.join("package.toml"),
        v1_manifest.replace("version='1.0.0'", "version='1.1.0'"),
    )
    .unwrap();
    let v2 = review(&registry, "example.script").unwrap();
    let approval = approve(&v2.digest);
    assert!(approval_matches(&v2, Some(&approval)));
    // Rolling back to 1.0.0 is different content: the 1.1.0 consent does not cover it.
    std::fs::write(script.join("package.toml"), &v1_manifest).unwrap();
    let rolled_back = review(&registry, "example.script").unwrap();
    assert_eq!(rolled_back.digest, v1.digest);
    assert!(!approval_matches(&rolled_back, Some(&approval)));
    // Sign the package and approve the signed content.
    let files = registry.package_files("example.script").unwrap();
    let mut project = nanatype_theme::PackageProject::capture(&*files).unwrap();
    let identity = nanatype_theme::SealIdentity::generate().unwrap();
    let protected = ["package.toml", "main.lua"].map(String::from).into();
    project.seal(&identity, &protected).unwrap();
    std::fs::write(
        script.join(nanatype_theme::INTEGRITY_FILE),
        &project.files[nanatype_theme::INTEGRITY_FILE],
    )
    .unwrap();
    let signed = review(&registry, "example.script").unwrap();
    assert!(signed.integrity.is_some(), "the seal verifies");
    let approval = approve(&signed.digest);
    assert!(approval_matches(&signed, Some(&approval)));
    // Removing the signature changes the reviewed content.
    std::fs::remove_file(script.join(nanatype_theme::INTEGRITY_FILE)).unwrap();
    let unsigned = review(&registry, "example.script").unwrap();
    assert!(unsigned.integrity.is_none());
    assert!(!approval_matches(&unsigned, Some(&approval)));
    // Revoking trust (or disabling, which drops the record) removes consent.
    let revoked = PluginApproval {
        trusted_source: false,
        ..approve(&unsigned.digest)
    };
    assert!(!approval_matches(&unsigned, Some(&revoked)));
    let mut settings = Settings {
        enabled_plugins: vec!["example.script".into()],
        ..Default::default()
    };
    settings
        .plugin_approvals
        .insert("example.script".into(), revoked);
    settings.sanitize();
    assert!(settings.plugin_approvals.is_empty());
}
#[test]
fn running_plugins_are_stopped_when_their_package_or_dependency_changes_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let ids = vec!["example.script".to_owned()];
    let started = review(&registry, "example.script").unwrap().digest;
    let running = [("example.script", started.as_str())];
    assert!(
        changed(running, &review_digests(&registry, &ids)).is_empty(),
        "unchanged content keeps running"
    );
    std::fs::write(dir.path().join("example.assets/note.txt"), "edited").unwrap();
    assert_eq!(
        changed(running, &review_digests(&registry, &ids)),
        ids,
        "a dependency edit invalidates the running instance"
    );
    std::fs::remove_file(dir.path().join("example.script/main.lua")).unwrap();
    let reviewed = review_digests(&registry, &ids);
    assert!(reviewed[0].1.is_err());
    assert_eq!(
        changed(running, &reviewed),
        ids,
        "unreviewable content stops"
    );
    assert!(
        changed([("example.other", "digest")], &reviewed).is_empty(),
        "plugins without a fresh review are left to the next sync"
    );
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
            assert!(
                nodes.iter().any(|n| n.label.as_deref().is_some_and(|l| {
                    l.contains("words")
                        && l.contains("example.assets")
                        && l.contains("Example word list")
                })),
                "declared reads are listed with owner and source"
            );
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
