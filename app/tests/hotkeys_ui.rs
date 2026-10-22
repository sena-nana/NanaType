mod common;

use nanatype::ui::{Ui, settings};
use nanatype_i18n::Locale;
use nanatype_settings::{Hotkey, QuickPhrase, Settings};
use std::sync::Arc;

const CAPTURES: &str = "NANATYPE_HOTKEYS_CAPTURES";

/// One page lists the global hotkeys, the input window's own keys and every
/// quick phrase, each with a recorder.
#[test]
fn hotkeys_page_gathers_every_shortcut() {
    let registry = common::registry();
    let skin = registry.load(nanatype_theme::DEFAULT_SKIN).unwrap().skin;
    for locale in Locale::ALL {
        let mut s = Settings::default();
        s.hotkeys.input.send = Some(Hotkey::ctrl("Enter"));
        s.assets.phrases.push(QuickPhrase {
            label: "晚安".into(),
            markup: "晚安～".into(),
            send: true,
            hotkey: Some(Hotkey {
                alt: true,
                ..Hotkey::bare("KeyN")
            }),
        });
        let ui = Ui::new(Arc::new(s), skin.clone(), locale);
        ui.page.set(settings::Page::Hotkeys);
        let mut session = common::session()
            .themed(false)
            .mount(920, 1600, move || settings::settings_window(ui));
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        let shows = |text: &str| nodes.iter().any(|n| n.label.as_deref() == Some(text));
        for key in [
            "hotkeys-global",
            "hotkeys-local",
            "hotkeys-phrases",
            "hotkeys-summon",
            "tb-send",
            "cheat-newline",
            "hotkeys-recall-prev",
            "hotkeys-recall-next",
            "hotkeys-edit-phrases",
        ] {
            let label = ui.now_t(key);
            assert_ne!(label, key, "missing translation");
            assert!(shows(&label), "{key} not shown in {locale:?}");
        }
        assert!(shows("晚安"), "phrase row missing in {locale:?}");
        // The rebound send key offers its default back.
        let reset = ui.now_t("hotkeys-reset");
        assert!(
            nodes
                .iter()
                .any(|n| n.role == "button" && n.label.as_deref() == Some(reset.as_str())),
            "{locale:?}"
        );
        if locale == Locale::ZhCn {
            common::frame(&mut session, ui, 0, 0, "hotkeys", CAPTURES);
        }
    }
}
