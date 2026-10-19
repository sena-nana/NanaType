//! Actual consent controls and content-bound execution approval.

mod common;

use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    msg::Msg,
    plugins::{
        Activation, ActivationReview, Manager, approval_matches, changed, review, review_digests,
    },
    ui::{self, Ui},
};
use nanatype_i18n::Locale;
use nanatype_settings::{PLUGIN_CONSENT_POLICY, PluginApproval, Settings};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

fn ui() -> Ui {
    Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::EnUs,
    )
}

fn ready(activation: &mut Activation, review: &Arc<ActivationReview>, ui: Ui) {
    let ticket = activation.request(review.id.clone(), ui);
    assert!(ui.plugin_activation_busy.get_untracked());
    assert!(!ui.plugin_source_trusted.get_untracked());
    assert!(
        activation
            .complete(ticket, Ok(review.clone()), ui)
            .unwrap()
            .is_none(),
        "reviewing alone never grants execution"
    );
    assert!(!ui.plugin_activation_busy.get_untracked());
}

#[test]
fn out_of_order_reviews_and_failures_never_replace_the_current_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let review = Arc::new(review(&registry, "example.script").unwrap());
    let ui = ui();
    let mut activation = Activation::default();
    let old = activation.request(review.id.clone(), ui);
    let current = activation.request(review.id.clone(), ui);
    activation.complete(old, Ok(review.clone()), ui).unwrap();
    assert!(ui.plugin_activation.get_untracked().is_none());
    assert!(ui.plugin_activation_busy.get_untracked());
    activation
        .complete(current, Ok(review.clone()), ui)
        .unwrap();
    ui.plugin_source_trusted.set(true);
    activation
        .complete(old, Err("stale error".into()), ui)
        .unwrap();
    assert!(ui.plugin_source_trusted.get_untracked());
    assert!(ui.plugin_activation_error.get_untracked().is_none());
    assert_eq!(ui.plugin_activation.get_untracked(), Some(review.clone()));
    // New requests clear the previous review and its checkbox immediately.
    let ticket = activation.request(review.id.clone(), ui);
    assert!(ui.plugin_activation.get_untracked().is_none());
    assert!(!ui.plugin_source_trusted.get_untracked());
    activation.cancel(ui);
    activation.complete(ticket, Ok(review), ui).unwrap();
    assert!(ui.plugin_activation.get_untracked().is_none());
}

#[test]
fn confirmation_needs_trust_and_is_single_use_even_for_identical_content() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let review = Arc::new(review(&registry, "example.script").unwrap());
    let ui = ui();
    let mut activation = Activation::default();
    ready(&mut activation, &review, ui);
    assert!(activation.confirm(ui).is_none());
    ui.plugin_source_trusted.set(true);
    let (old, _) = activation.confirm(ui).unwrap();
    assert!(
        activation.confirm(ui).is_none(),
        "a double click cannot create a second confirmation"
    );
    activation.cancel(ui);
    ready(&mut activation, &review, ui);
    ui.plugin_source_trusted.set(true);
    let (current, _) = activation.confirm(ui).unwrap();
    assert_ne!(old, current);
    assert!(
        activation
            .complete(old, Ok(review.clone()), ui)
            .unwrap()
            .is_none()
    );
    assert!(
        activation
            .complete(old, Err("late failure".into()), ui)
            .unwrap()
            .is_none()
    );
    assert!(ui.plugin_activation_busy.get_untracked());
    assert!(ui.plugin_source_trusted.get_untracked());
    assert_eq!(
        activation
            .complete(current, Ok(review.clone()), ui)
            .unwrap(),
        Some(review.clone())
    );
    assert!(
        activation
            .complete(current, Ok(review), ui)
            .unwrap()
            .is_none()
    );
    assert!(!ui.plugin_activation_busy.get_untracked());
    assert!(!ui.plugin_source_trusted.get_untracked());
}

#[test]
fn cancel_disable_and_catalogue_changes_discard_pending_confirmations() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let review = Arc::new(review(&registry, "example.script").unwrap());
    let ui = ui();
    let mut activation = Activation::default();
    for disable in [false, true] {
        ready(&mut activation, &review, ui);
        ui.plugin_source_trusted.set(true);
        let (ticket, _) = activation.confirm(ui).unwrap();
        if disable {
            activation.cancel_for(&review.id, ui);
        } else {
            activation.cancel(ui);
        }
        assert!(
            activation
                .complete(ticket, Ok(review.clone()), ui)
                .unwrap()
                .is_none()
        );
        assert!(ui.plugin_activation.get_untracked().is_none());
        assert!(!ui.plugin_activation_busy.get_untracked());
    }
    // Disabling a different plugin does not revoke this dialog's trust.
    ready(&mut activation, &review, ui);
    ui.plugin_source_trusted.set(true);
    activation.cancel_for("another.plugin", ui);
    assert!(activation.confirm(ui).is_some());
}

#[test]
fn changed_content_invalid_signatures_and_worker_creation_errors_reset_trust() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let ui = ui();
    let mut activation = Activation::default();
    for path in ["example.script/main.lua", "example.assets/note.txt"] {
        let before = Arc::new(review(&registry, "example.script").unwrap());
        ready(&mut activation, &before, ui);
        ui.plugin_source_trusted.set(true);
        let (ticket, _) = activation.confirm(ui).unwrap();
        std::fs::write(dir.path().join(path), "-- changed content").unwrap();
        let current = Arc::new(review(&registry, "example.script").unwrap());
        assert!(activation.complete(ticket, Ok(current), ui).is_err());
        assert!(ui.plugin_activation.get_untracked().is_none());
        assert!(!ui.plugin_source_trusted.get_untracked());
        assert!(!ui.plugin_activation_busy.get_untracked());
        assert!(activation.confirm(ui).is_none());
    }
    let files = registry.package_files("example.script").unwrap();
    let mut project = nanatype_theme::PackageProject::capture(&*files).unwrap();
    project
        .seal(
            &nanatype_theme::SealIdentity::generate().unwrap(),
            &["main.lua".into()].into(),
        )
        .unwrap();
    let script = dir.path().join("example.script");
    std::fs::write(
        script.join(nanatype_theme::INTEGRITY_FILE),
        &project.files[nanatype_theme::INTEGRITY_FILE],
    )
    .unwrap();
    let before = Arc::new(review(&registry, "example.script").unwrap());
    ready(&mut activation, &before, ui);
    ui.plugin_source_trusted.set(true);
    let (ticket, _) = activation.confirm(ui).unwrap();
    std::fs::write(script.join("main.lua"), "tampered").unwrap();
    let failed = review(&registry, "example.script").map(Arc::new);
    assert!(failed.is_err());
    assert!(activation.complete(ticket, failed, ui).is_err());
    assert!(!ui.plugin_source_trusted.get_untracked());
    // The host routes a failed background spawn to the same completion path.
    let ticket = activation.request("example.script".into(), ui);
    assert!(
        activation
            .complete(ticket, Err("thread creation failed".into()), ui)
            .is_err()
    );
    assert!(!ui.plugin_activation_busy.get_untracked());
    ready(&mut activation, &before, ui);
    ui.plugin_source_trusted.set(true);
    let (ticket, _) = activation.confirm(ui).unwrap();
    assert!(
        activation
            .complete(ticket, Err("thread creation failed".into()), ui)
            .is_err()
    );
    assert!(!ui.plugin_activation_busy.get_untracked());
    assert!(!ui.plugin_source_trusted.get_untracked());
}
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

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // Query only our own fixture worker; no user process is modified.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0;
        let running = GetExitCodeProcess(handle, &mut code).is_ok() && code == 259;
        let _ = CloseHandle(handle);
        running
    }
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn production_workers_require_approval_stop_on_changes_and_ignore_late_responses() {
    for change in [
        "disable",
        "example.script/main.lua",
        "example.assets/note.txt",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let registry = fixture(dir.path());
        let reviewed = review(&registry, "example.script").unwrap();
        let ui = ui();
        let mut manager = Manager::default();
        let worker = std::path::Path::new(env!("CARGO_BIN_EXE_nanatype"));
        let mut settings = Settings {
            enabled_plugins: vec![reviewed.id.clone()],
            ..Default::default()
        };
        let (tx, rx) = mpsc::channel();
        let start = |manager: &mut Manager, approval: Option<&PluginApproval>| {
            let tx = tx.clone();
            manager.start(
                &reviewed,
                approval,
                worker,
                move |generation, response| {
                    let _ = tx.send((generation, response));
                },
                ui,
            )
        };
        assert!(
            start(&mut manager, None).is_err(),
            "legacy enabled IDs cannot start a worker"
        );
        assert!(!manager.has_running());
        for (trusted_source, policy, digest) in [
            (false, PLUGIN_CONSENT_POLICY, reviewed.digest.clone()),
            (true, PLUGIN_CONSENT_POLICY - 1, reviewed.digest.clone()),
            (true, PLUGIN_CONSENT_POLICY, "0".repeat(64)),
        ] {
            assert!(
                start(
                    &mut manager,
                    Some(&PluginApproval {
                        trusted_source,
                        policy,
                        digest
                    })
                )
                .is_err()
            );
            assert!(!manager.has_running());
            assert!(rx.try_recv().is_err());
        }
        let approval = PluginApproval {
            trusted_source: true,
            policy: PLUGIN_CONSENT_POLICY,
            digest: reviewed.digest.clone(),
        };
        settings
            .plugin_approvals
            .insert(reviewed.id.clone(), approval.clone());
        let pid = start(&mut manager, Some(&approval)).unwrap();
        let (generation, response) = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(
            matches!(response, nanatype_plugins::WorkerResponse::Update { .. }),
            "{response:?}"
        );
        manager.receive(&reviewed.id, generation, &response, ui);
        assert!(ui.plugin_views.get_untracked().contains_key(&reviewed.id));
        assert!(alive(pid));
        if change == "disable" {
            manager.stop(&reviewed.id, ui);
            settings.enabled_plugins.clear();
            settings.plugin_approvals.remove(&reviewed.id);
            assert!(settings.plugin_approvals.is_empty());
        } else {
            std::fs::write(
                dir.path().join(change),
                "return {view=function() return {views={}} end} -- changed",
            )
            .unwrap();
            manager.revalidate(
                &review_digests(&registry, std::slice::from_ref(&reviewed.id)),
                ui,
            );
            let current = review(&registry, &reviewed.id).unwrap();
            assert!(!approval_matches(
                &current,
                settings.plugin_approvals.get(&reviewed.id)
            ));
        }
        assert!(!manager.has_running());
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!alive(pid), "{change}: worker {pid} was not reaped");
        manager.receive(&reviewed.id, generation, &response, ui);
        assert!(!ui.plugin_views.get_untracked().contains_key(&reviewed.id));
        // A fresh approval starts current bytes with a new process generation.
        let current = review(&registry, &reviewed.id).unwrap();
        let approval = PluginApproval {
            digest: current.digest.clone(),
            ..approval
        };
        let (tx, new_rx) = mpsc::channel();
        let new_pid = manager
            .start(
                &current,
                Some(&approval),
                worker,
                move |generation, response| {
                    let _ = tx.send((generation, response));
                },
                ui,
            )
            .unwrap();
        let (new_generation, new_response) = new_rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(
            matches!(
                new_response,
                nanatype_plugins::WorkerResponse::Update { .. }
            ),
            "{new_response:?}"
        );
        assert_ne!(new_generation, generation);
        manager.receive(&reviewed.id, generation, &response, ui);
        assert!(!ui.plugin_views.get_untracked().contains_key(&reviewed.id));
        manager.receive(&reviewed.id, new_generation, &new_response, ui);
        assert!(ui.plugin_views.get_untracked().contains_key(&reviewed.id));
        manager.stop(&reviewed.id, ui);
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(new_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!alive(new_pid));
    }
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
fn capture(
    session: &mut RuntimeAgentSession,
    dir: &std::path::Path,
    locale: Locale,
    width: u32,
    state: &str,
) {
    session.flush().unwrap();
    for node in session
        .accessibility_dump()
        .iter()
        .filter(|n| n.role == "button")
    {
        assert!(
            node.bounds.x >= 0. && node.bounds.x + node.bounds.width <= width as f32 + 0.5,
            "{state} at {width}px: {:?}",
            node.label
        );
    }
    session
        .screenshot_png(dir.join(format!("{}-{width}-{state}.png", locale.tag())))
        .unwrap();
}

#[test]
fn consent_panel_discloses_risks_and_needs_a_real_trusted_source_selection() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let mut reviewed = review(&registry, "example.script").unwrap();
    reviewed.location = format!(
        "C:/Users/Example/AppData/Roaming/NanaType/{}/example.script",
        "long-package-directory/".repeat(8)
    );
    let review = Arc::new(reviewed);
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
            let mut activation = Activation::default();
            let first = activation.request(review.id.clone(), ui);
            let mut session = common::session()
                .themed(false)
                .mount(width, 1800, move || ui::plugins::activation(ui));
            session.flush().unwrap();
            assert!(session.accessibility_dump().iter().any(
                |n| n.label.as_deref() == Some(ui.now_t("packages-consent-reviewing").as_str())
            ));
            capture(&mut session, &captures, locale, width, "loading");
            let cancel = session
                .accessibility_dump()
                .into_iter()
                .find(|n| {
                    n.agent_path
                        .as_deref()
                        .is_some_and(|p| p.ends_with("plugin-consent-cancel"))
                })
                .unwrap();
            assert!(!cancel.disabled);
            assert!(session.click_node(cancel.id).unwrap());
            assert!(
                messages(&mut session)
                    .iter()
                    .any(|m| matches!(m, Msg::CancelPluginActivation))
            );
            activation.cancel(ui);
            activation.complete(first, Ok(review.clone()), ui).unwrap();
            capture(&mut session, &captures, locale, width, "cancelled");
            assert!(ui.plugin_activation.get_untracked().is_none());
            ready(&mut activation, &review, ui);
            session.flush().unwrap();
            capture(&mut session, &captures, locale, width, "unchecked");
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
                        && l.contains("json v1")
                        && l.contains("CC0-1.0")
                        && l.contains(&ui.now_t("package-data-unprotected"))
                })),
                "declared reads include owner, format, source, license and protection"
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
            let (confirmation, _) = activation.confirm(ui).unwrap();
            capture(&mut session, &captures, locale, width, "confirming");
            let nodes = session.accessibility_dump();
            for key in ["plugin-source-trusted", "plugin-consent-confirm"] {
                assert!(
                    nodes
                        .iter()
                        .find(|n| n.agent_path.as_deref().is_some_and(|p| p.ends_with(key)))
                        .unwrap()
                        .disabled
                );
            }
            assert!(activation.confirm(ui).is_none());
            let cancel = nodes
                .iter()
                .find(|n| {
                    n.agent_path
                        .as_deref()
                        .is_some_and(|p| p.ends_with("plugin-consent-cancel"))
                })
                .unwrap();
            assert!(!cancel.disabled);
            assert!(session.click_node(cancel.id).unwrap());
            assert!(
                messages(&mut session)
                    .iter()
                    .any(|m| matches!(m, Msg::CancelPluginActivation))
            );
            activation.cancel(ui);
            assert!(
                activation
                    .complete(confirmation, Ok(review.clone()), ui)
                    .unwrap()
                    .is_none()
            );
            ready(&mut activation, &review, ui);
            ui.plugin_source_trusted.set(true);
            let (ticket, _) = activation.confirm(ui).unwrap();
            assert!(
                activation
                    .complete(ticket, Err(ui.now_t("packages-changed-during-review")), ui)
                    .is_err()
            );
            capture(&mut session, &captures, locale, width, "error");
            assert!(!ui.plugin_source_trusted.get_untracked());
            assert!(!session.accessibility_dump().iter().any(|n| {
                n.agent_path
                    .as_deref()
                    .is_some_and(|p| p.ends_with("plugin-consent-confirm"))
            }));
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

#[test]
fn package_metadata_and_signature_states_are_visible_in_every_locale_and_width() {
    let dir = tempfile::tempdir().unwrap();
    let registry = fixture(dir.path());
    let script = dir.path().join("example.script");
    let manifest = std::fs::read_to_string(script.join("package.toml")).unwrap();
    let manifest = manifest
        .lines()
        .filter(|line| {
            !["author=", "created_at=", "licensed_to=", "license="]
                .iter()
                .any(|key| line.starts_with(key))
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(script.join("package.toml"), manifest).unwrap();
    let files = registry.package_files("example.script").unwrap();
    let mut project = nanatype_theme::PackageProject::capture(&*files).unwrap();
    let key = nanatype_theme::SealIdentity::generate().unwrap();
    let seal = project.seal(&key, &["main.lua".into()].into()).unwrap();
    let fingerprint = seal
        .fingerprint
        .as_bytes()
        .chunks(8)
        .map(|bytes| std::str::from_utf8(bytes).unwrap())
        .collect::<Vec<_>>()
        .join(" ");
    let captures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-consent");
    std::fs::create_dir_all(&captures).unwrap();
    for locale in Locale::ALL {
        for width in [900, 440, 300] {
            for (name, integrity, label) in [
                ("unsigned", Ok(None), "package-unsigned"),
                ("signed", Ok(Some(seal.clone())), "package-signature-valid"),
                (
                    "invalid",
                    Err("protected file was modified: main.lua".to_owned()),
                    "package-integrity-failed",
                ),
            ] {
                let ui = Ui::new(
                    Arc::new(Settings::default()),
                    Arc::new(Default::default()),
                    locale,
                );
                let row = nanatype::plugins::PackageRow {
                    manifest: project.manifest().unwrap(),
                    origin: nanatype_theme::SkinOrigin::Directory(script.clone()),
                    integrity,
                };
                ui.packages.set(Arc::new(vec![row]));
                let mut session = common::session()
                    .themed(false)
                    .mount(width, 1800, move || ui::plugins::settings(ui));
                capture(&mut session, &captures, locale, width, name);
                let labels: Vec<_> = session
                    .accessibility_dump()
                    .into_iter()
                    .filter_map(|n| n.label)
                    .collect();
                assert!(labels.iter().any(|l| l.contains(&ui.now_t(label))));
                assert!(
                    labels
                        .iter()
                        .filter(|l| l.contains(&ui.now_t("package-unspecified")))
                        .count()
                        >= 2
                );
                if name == "signed" {
                    assert!(labels.iter().any(|l| l.contains(&fingerprint)));
                }
                if name == "invalid" {
                    assert!(
                        labels
                            .iter()
                            .any(|l| l.contains("protected file was modified: main.lua"))
                    );
                }
                ui.plugin_activation_busy.set(true);
                session.flush().unwrap();
                let button = session
                    .accessibility_dump()
                    .into_iter()
                    .find(|n| {
                        n.agent_path
                            .as_deref()
                            .is_some_and(|p| p.ends_with("package-review:example.script"))
                    })
                    .unwrap();
                assert!(
                    button.disabled,
                    "review cannot be submitted twice while busy"
                );
                let _ = session.click_node(button.id).unwrap();
                assert!(messages(&mut session).is_empty());
            }
        }
    }
}
