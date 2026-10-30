//! Every settings card insets its rows. NanaUI's settings group adds the
//! hairlines between rows but no padding, so a button or caption passed to
//! `section` as it is sits flush against the card's edge. The row builders
//! (`row`, `block`, `kit::line`) bring the card inset; this catches a page
//! that skips them.
mod common;
use nanatype::ui::{Ui, kit::inset, settings};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Left of the settings window's content pane (the sidebar is narrower).
const CONTENT_MIN_X: f32 = 216.0;

#[test]
fn settings_rows_keep_the_card_inset() {
    let mut flush = Vec::new();
    for locale in [Locale::ZhCn, Locale::EnUs] {
        for page in settings::Page::ALL {
            let ui = Ui::new(
                Arc::new(Settings::default()),
                common::skin("nanatype.halo"),
                locale,
            );
            ui.page.set(page);
            let mut session =
                common::session()
                    .themed(false)
                    .locale(locale)
                    .mount(1000, 2400, move || settings::settings_window(ui));
            for _ in 0..4 {
                session.flush().unwrap();
            }
            let nodes = session.accessibility_dump();
            // The cards' left edge, and the top of the first one, below the
            // page heading.
            let body = nodes
                .iter()
                .filter(|n| n.role == "generic" && n.bounds.x > CONTENT_MIN_X)
                .filter(|n| n.bounds.y > 60.0);
            let left = body.clone().map(|n| n.bounds.x).fold(f32::MAX, f32::min);
            let top = body.map(|n| n.bounds.y).fold(f32::MAX, f32::min);
            let title_x = left + nana_ui_core::space::XS;
            // A page-level control (the platform switch) heads the page,
            // where no card row can start; what is inside it goes with it.
            let parents: HashMap<u64, Option<u64>> =
                nodes.iter().map(|n| (n.id, n.parent)).collect();
            let heads: HashSet<u64> = nodes
                .iter()
                .filter(|n| n.role != "generic" && (n.bounds.y - top).abs() < 0.5)
                .map(|n| n.id)
                .collect();
            let page_level = |mut id: u64| loop {
                if heads.contains(&id) {
                    break true;
                }
                match parents.get(&id).copied().flatten() {
                    Some(p) => id = p,
                    None => break false,
                }
            };
            for n in &nodes {
                if matches!(n.role.as_str(), "generic" | "separator") {
                    continue;
                }
                let b = &n.bounds;
                let in_gutter = b.x > left - 0.5 && b.x < left + inset::CARD - 0.5;
                // A group's title sits just inside the card's edge, above it.
                let title = n.role == "text" && (b.x - title_x).abs() < 0.5;
                if in_gutter && !title && !page_level(n.id) {
                    flush.push(format!(
                        "{locale:?} {page:?}: {} {:?} at x={} (cards at {left}, rows from {})",
                        n.role,
                        n.label,
                        b.x,
                        left + inset::CARD
                    ));
                }
            }
        }
    }
    assert!(
        flush.is_empty(),
        "settings rows flush against the card edge; wrap them in row, block or kit::line:\n{}",
        flush.join("\n")
    );
}
