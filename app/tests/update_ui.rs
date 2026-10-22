#![cfg(feature = "updates")]

mod common;

use nanatype::app::update::State;
use nanatype::ui::{Ui, settings};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::sync::Arc;

/// Each update state on About offers its own actions, translated in every
/// locale. Set `NANATYPE_UPDATE_SHOTS=<dir>` to keep a frame of each.
#[test]
fn about_page_offers_the_actions_of_each_update_state() {
    let registry = common::registry();
    let skin = registry.load(nanatype_theme::DEFAULT_SKIN).unwrap().skin;
    let states = [
        ("idle", State::Idle, vec!["update-check"]),
        ("checking", State::Checking, vec![]),
        ("current", State::UpToDate, vec!["update-check"]),
        (
            "available",
            State::Available {
                version: "9.9.9".into(),
                size: 48 * 1024 * 1024,
            },
            vec!["update-notes", "update-skip", "update-download"],
        ),
        (
            "downloading",
            State::Downloading {
                version: "9.9.9".into(),
                done: 12 * 1024 * 1024,
                total: 48 * 1024 * 1024,
                source: "gh-proxy.com".into(),
            },
            vec!["update-cancel"],
        ),
        (
            "ready",
            State::Ready {
                version: "9.9.9".into(),
                kind: nanatype_update::Kind::Installer,
                path: "NanaType-9.9.9-setup.exe".into(),
            },
            vec!["update-install"],
        ),
        (
            "failed",
            State::Failed {
                message: "Could not reach an update source".into(),
                detail: "no update source reachable: GitHub: timeout".into(),
            },
            vec!["update-retry"],
        ),
    ];
    for locale in Locale::ALL {
        for (name, state, actions) in &states {
            let ui = Ui::new(Arc::new(Settings::default()), skin.clone(), locale);
            ui.page.set(settings::Page::About);
            ui.update.set(Arc::new(state.clone()));
            let mut session = common::session()
                .themed(false)
                .mount(920, 700, move || settings::settings_window(ui));
            session.flush().unwrap();
            common::frame(
                &mut session,
                ui,
                0,
                0,
                &format!("{name}-{locale:?}"),
                "NANATYPE_UPDATE_SHOTS",
            );
            let nodes = session.accessibility_dump();
            for key in actions {
                let label = ui.now_t(key);
                assert_ne!(label, *key, "missing translation");
                assert!(
                    nodes
                        .iter()
                        .any(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str())),
                    "missing {key} for {name} in {locale:?}"
                );
            }
            // The other states' actions are gone.
            for (_, _, others) in &states {
                for key in others.iter().filter(|k| !actions.contains(k)) {
                    let label = ui.now_t(key);
                    assert!(
                        !nodes
                            .iter()
                            .any(|n| n.role == "button"
                                && n.label.as_deref() == Some(label.as_str())),
                        "stray {key} for {name} in {locale:?}"
                    );
                }
            }
        }
    }
}
