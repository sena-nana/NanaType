mod common;

use nanatype::ui::{Ui, settings};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use nanatype_twitch::AccountState;
use std::sync::Arc;

macro_rules! has_button {
    ($nodes:expr, $label:expr) => {
        $nodes
            .iter()
            .any(|n| n.role == "button" && n.label.as_deref() == Some($label.as_str()))
    };
}

#[test]
fn twitch_page_walks_through_the_device_code_sign_in_in_all_locales() {
    let registry = common::registry();
    let skin = registry.load(nanatype_theme::DEFAULT_SKIN).unwrap().skin;
    for locale in Locale::ALL {
        let ui = Ui::new(Arc::new(Settings::default()), skin.clone(), locale);
        ui.page.set(settings::Page::Twitch);
        ui.twitch
            .update(|s| Arc::make_mut(s).account = AccountState::SignedOut);
        let mut session = common::session()
            .themed(false)
            .mount(920, 1050, move || settings::settings_window(ui));
        session.flush().unwrap();
        let login = ui.now_t("tw-login");
        assert_ne!(login, "tw-login", "missing translation");
        assert!(
            has_button!(session.accessibility_dump(), login),
            "{locale:?}"
        );

        ui.twitch.update(|s| {
            Arc::make_mut(s).account = AccountState::Pending {
                code: "ABCD-EFGH".into(),
                uri: "https://www.twitch.tv/activate?device-code=ABCD-EFGH".into(),
            }
        });
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        assert!(
            has_button!(nodes, ui.now_t("tw-open-activation")),
            "{locale:?}"
        );
        assert!(has_button!(nodes, ui.now_t("common-cancel")), "{locale:?}");
        assert!(
            nodes
                .iter()
                .any(|n| n.label.as_deref() == Some("ABCD-EFGH")),
            "the device code is not shown in {locale:?}"
        );
    }
}
