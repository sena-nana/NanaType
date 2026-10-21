mod common;

use nanatype::ui::{Ui, settings};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::sync::Arc;

#[test]
fn youtube_page_offers_connect_and_says_sending_is_unavailable_in_all_locales() {
    let registry = common::registry();
    let skin = registry.load(nanatype_theme::DEFAULT_SKIN).unwrap().skin;
    for locale in Locale::ALL {
        let ui = Ui::new(Arc::new(Settings::default()), skin.clone(), locale);
        ui.page.set(settings::Page::Youtube);
        let mut session = common::session()
            .themed(false)
            .mount(920, 1050, move || settings::settings_window(ui));
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        for key in ["yt-connect", "yt-open-panel"] {
            let label = ui.now_t(key);
            assert_ne!(label, key, "missing translation");
            assert!(
                nodes
                    .iter()
                    .any(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str())),
                "missing action {key} in {locale:?}"
            );
        }
        let unavailable = ui.now_t("yt-send-unavailable");
        assert!(
            nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(unavailable.as_str())),
            "sending status missing in {locale:?}"
        );
    }
}
