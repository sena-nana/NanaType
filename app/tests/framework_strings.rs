//! What NanaUI's own controls say follows the interface language: a toast's
//! close button, the toast region, and the color fields the skins page leaves
//! unlabelled. Switching languages rewrites the controls already built
//! without losing any of the window.
mod common;

use nana_ui::runtime::view::{entity_ref, with_refs};
use nana_ui::runtime::{Entity, ToastStack};
use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::ui::{Ui, feedback, settings};
use nanatype_i18n::{Locale, framework_strings};
use nanatype_settings::Settings;
use std::sync::Arc;

fn labels(session: &mut RuntimeAgentSession) -> Vec<(String, String)> {
    session
        .accessibility_dump()
        .into_iter()
        .filter_map(|n| n.label.map(|label| (n.role, label)))
        .collect()
}

fn count(labels: &[(String, String)], text: &str) -> usize {
    labels.iter().filter(|(_, label)| label == text).count()
}

fn skins_page(locale: Locale) -> (Ui, RuntimeAgentSession, Entity<ToastStack>) {
    let ui = Ui::new(
        Arc::new(Settings::default()),
        common::skin("nanatype.halo"),
        locale,
    );
    ui.page.set(settings::Page::Skins);
    let (mut session, (_, stack)) =
        common::session()
            .themed(false)
            .locale(locale)
            .mount_output(920, 2400, move || {
                let stack = entity_ref::<ToastStack>();
                with_refs(
                    feedback::with_toasts(settings::settings_window(ui), stack),
                    stack,
                )
            });
    session.flush().unwrap();
    (ui, session, stack)
}

#[test]
fn nanaui_controls_speak_the_interface_language_and_follow_a_switch() {
    let (ui, mut session, stack) = skins_page(Locale::EnUs);
    let toast = feedback::ToastRequest::notice("Saved".into(), false);
    assert!(toast.push(session.document_mut(), stack, |_| ()));
    session.flush().unwrap();

    let en = framework_strings(Locale::EnUs);
    let before = labels(&mut session);
    assert_eq!(count(&before, &en.toast_close), 1, "{before:?}");
    assert_eq!(count(&before, &en.toast_region), 1, "{before:?}");
    // Three color fields, each named on the field and on its swatch menu.
    assert_eq!(count(&before, &en.color_field_label), 6, "{before:?}");
    let chinese = nana_ui_core::FrameworkStrings::default();
    for text in [
        &chinese.toast_close,
        &chinese.toast_region,
        &chinese.color_field_label,
    ] {
        assert_eq!(count(&before, text), 0, "{text} in English: {before:?}");
    }

    // As the app switches: the copy's locale, then each window's document.
    ui.locale.set(Locale::JaJp);
    nanatype_frame::apply_locale(session.document_mut().context_mut(), Locale::JaJp);
    session.flush().unwrap();
    let ja = framework_strings(Locale::JaJp);
    let after = labels(&mut session);
    assert_eq!(count(&after, &ja.toast_close), 1, "{after:?}");
    assert_eq!(count(&after, &ja.toast_region), 1, "{after:?}");
    assert_eq!(count(&after, &ja.color_field_label), 6, "{after:?}");
    assert_eq!(count(&after, &en.toast_close), 0, "{after:?}");
    // Rebuilt in place: the same controls in the same roles, none lost.
    let roles = |labels: &[(String, String)]| {
        let mut roles: Vec<_> = labels.iter().map(|(role, _)| role.clone()).collect();
        roles.sort();
        roles
    };
    assert_eq!(roles(&after), roles(&before));
}
