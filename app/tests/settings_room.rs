//! Every settings row leaves its copy room to read and stays in its card.
//! A control slot that takes the row (a switching block's full-width
//! column, a confirmation's question on one line) or a wide control beside
//! a long hint squeezed the label and hint into a column a character or two
//! wide; a header sized to its content ran past the card's edge.
mod common;
use nanatype::ui::{Ui, settings};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::sync::Arc;

/// Narrower than this, a text of a few characters reads as a column.
const MIN_TEXT_W: f32 = 120.0;

fn check_pages(pages: &[settings::Page], configure: impl Fn(&mut Settings)) {
    let mut bad = Vec::new();
    for locale in Locale::ALL {
        // The window's narrowest, and a roomier one.
        for width in [720u32, 920] {
            for &page in pages {
                let mut s = Settings::default();
                s.general.onboarding_done = true;
                configure(&mut s);
                let ui = Ui::new(Arc::new(s), common::skin("nanatype.halo"), locale);
                ui.page.set(page);
                let mut session =
                    common::session()
                        .themed(false)
                        .locale(locale)
                        .mount(width, 3000, move || settings::settings_window(ui));
                for _ in 0..4 {
                    session.flush().unwrap();
                }
                let nodes = session.accessibility_dump();
                let shown = nodes
                    .iter()
                    .filter(|n| n.bounds.width > 0.0 && n.bounds.height > 0.0);
                // The cards' inner right edge, where their dividers end.
                let right = shown
                    .clone()
                    .filter(|n| n.role == "separator")
                    .map(|n| n.bounds.x + n.bounds.width)
                    .fold(0.0f32, f32::max);
                for n in shown {
                    let b = &n.bounds;
                    let chars = n.label.as_deref().map_or(0, |l| l.chars().count());
                    if n.role == "text" && chars >= 3 && b.height > 40.0 && b.width < MIN_TEXT_W {
                        bad.push(format!(
                            "{locale:?} {width} {page:?}: {:?} squeezed to {}px",
                            n.label, b.width
                        ));
                    }
                    if n.role != "generic" && right > 0.0 && b.x + b.width > right + 0.5 {
                        bad.push(format!(
                            "{locale:?} {width} {page:?}: {} {:?} ends at {}, past the card at {right}",
                            n.role,
                            n.label,
                            b.x + b.width
                        ));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn settings_rows_leave_their_copy_room() {
    check_pages(&settings::Page::ALL, |_| {});
}

#[test]
fn separate_danmaku_settings_rows_leave_their_copy_room() {
    check_pages(
        &[
            settings::Page::History,
            settings::Page::Windows,
            settings::Page::Output,
        ],
        |s| {
            s.history.chat.separate = true;
            s.output.danmaku.enabled = true;
        },
    );
}

#[test]
fn about_logs_card_places_hint_above_wide_button() {
    for locale in Locale::ALL {
        let ui = Ui::new(
            Arc::new(Settings::default()),
            common::skin("nanatype.halo"),
            locale,
        );
        ui.page.set(settings::Page::About);
        let mut session =
            common::session()
                .themed(false)
                .locale(locale)
                .mount(920, 3000, move || settings::settings_window(ui));
        for _ in 0..4 {
            session.flush().unwrap();
        }
        let nodes = session.accessibility_dump();
        let hint_label = ui.now_t("about-logs-hint");
        let button_label = ui.now_t("about-logs");
        let hint_node = nodes
            .iter()
            .find(|n| n.role == "text" && n.label.as_deref() == Some(hint_label.as_str()))
            .unwrap_or_else(|| panic!("{locale:?}: missing about-logs-hint"));
        let button_node = nodes
            .iter()
            .find(|n| n.role == "button" && n.label.as_deref() == Some(button_label.as_str()))
            .unwrap_or_else(|| panic!("{locale:?}: missing about-logs button"));
        let right = nodes
            .iter()
            .filter(|n| n.role == "separator" && n.bounds.width > 0.0)
            .map(|n| n.bounds.x + n.bounds.width)
            .fold(0.0f32, f32::max);
        assert!(
            hint_node.bounds.y + hint_node.bounds.height <= button_node.bounds.y + 0.5,
            "{locale:?}: hint ({:?}) should sit above button ({:?})",
            hint_node.bounds,
            button_node.bounds
        );
        assert!(
            button_node.bounds.width >= 300.0,
            "{locale:?}: logs button should be wide, got {}",
            button_node.bounds.width
        );
        assert!(
            button_node.bounds.x >= hint_node.bounds.x - 0.5
                && (right == 0.0 || button_node.bounds.x + button_node.bounds.width <= right + 0.5),
            "{locale:?}: logs button ({:?}) outside card bounds (hint_x={}, right={right})",
            button_node.bounds,
            hint_node.bounds.x
        );
    }
}
