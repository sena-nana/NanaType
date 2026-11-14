//! Gifts, paid messages and memberships in the history list.
//!
//! Every built-in skin dresses them; a skin that says nothing shows them as
//! chat. Set `NANATYPE_EVENT_CAPTURES` to a directory to save each skin's
//! rows as `<skin>.png`.

mod common;

use std::sync::Arc;

use nana_ui::runtime::view::IntoView;
use nanatype::ui::{Ui, history};
use nanatype_core::HistoryRow;
use nanatype_i18n::Locale;
use nanatype_model::{
    Document, EventInfo, EventKind, HistoryItem, Inline, SourceId, SourceKind, Style, TextRun,
};
use nanatype_settings::Settings;

const PRICE: [u8; 3] = [0x00, 0xe5, 0xff];

fn row(key: u64, author: &str, doc: Document, event: Option<EventInfo>) -> HistoryRow {
    HistoryRow {
        key,
        item: HistoryItem {
            source: SourceId("blivechat".into()),
            kind: if event.is_some() {
                SourceKind::Event
            } else {
                SourceKind::Chat
            },
            line: None,
            author: author.into(),
            avatar: None,
            doc: doc.clone(),
            at: 1_700_000_000_000 + key * 200_000,
            uid: Some(format!("viewer-{key}")),
            reply: None,
            event,
        },
        doc: Arc::new(doc),
        reveal: None,
    }
}

fn event(kind: EventKind, amount: Option<&str>, color: Option<u32>) -> Option<EventInfo> {
    Some(EventInfo {
        kind,
        amount: amount.map(str::to_owned),
        color,
    })
}

/// A chat line, then one event of each kind, as the chat sources build them.
fn rows() -> Vec<HistoryRow> {
    let paid = Document::from_inlines(vec![
        Inline::Text(TextRun::styled(
            "¥30 ",
            Style {
                bold: Some(true),
                ..Style::default()
            },
        )),
        Inline::Text(TextRun::plain("主播今天也辛苦了，注意休息")),
    ]);
    vec![
        row(1, "小夜", Document::plain("晚上好呀"), None),
        row(
            2,
            "小星",
            Document::plain("赠送 小心心 ×10"),
            event(EventKind::Gift, Some("¥1"), None),
        ),
        row(
            3,
            "阿澄",
            paid,
            event(EventKind::SuperChat, Some("¥30"), Some(0x00e5ff)),
        ),
        row(
            4,
            "月见",
            Document::plain("开通了舰长 ×1个月"),
            event(EventKind::Member, Some("¥138"), None),
        ),
        row(
            5,
            "Hoshi",
            Document::plain("subscribed at Tier 1"),
            event(EventKind::Notice, None, None),
        ),
    ]
}

fn render(
    skin: Arc<nanatype_theme::Skin>,
    urls: Option<Arc<std::collections::HashMap<String, String>>>,
    name: &str,
) -> Vec<u8> {
    let mut settings = Settings::default();
    settings.history.visible_items = 8;
    let (w, h) = (skin.history.width.max(420.0) as u32, 620);
    let ui = Ui::new(Arc::new(settings), skin, Locale::ZhCn);
    if let Some(urls) = urls {
        ui.asset_urls.set(urls);
    }
    ui.history.set(Arc::new(rows()));
    ui.now.set(1_700_001_200_000);
    let mut session = common::session()
        .themed(false)
        .mount(w, h, move || history::history_window(ui).into_any());
    session.set_gpu_renderers(nanatype::ui::sweep::renderers());
    session.flush().unwrap();
    let (size, rgba) = session.screenshot_rgba().unwrap();
    if let Ok(dir) = std::env::var("NANATYPE_EVENT_CAPTURES") {
        std::fs::create_dir_all(&dir).unwrap();
        image::save_buffer(
            std::path::Path::new(&dir).join(format!("{name}.png")),
            &rgba,
            size.width,
            size.height,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    rgba
}

fn price_pixels(rgba: &[u8]) -> usize {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] > 200 && p[..3].iter().zip(PRICE).all(|(a, b)| a.abs_diff(b) <= 6))
        .count()
}

#[test]
fn every_builtin_skin_dresses_its_events() {
    let root = std::env::temp_dir().join(format!("nanatype-events-{}", std::process::id()));
    nanatype::services::media::init(&root.join("media")).unwrap();
    let registry = common::registry();
    for entry in registry.entries() {
        let skin = entry.skin.clone().unwrap();
        assert!(
            skin.history.event.is_some(),
            "{} leaves events looking like chat",
            entry.id
        );
        let urls = nanatype::assets::skin_preview_urls(&skin, entry.files().unwrap());
        let name = entry.id.rsplit('.').next().unwrap().to_owned();
        let rgba = render(skin, Some(Arc::new(urls)), &name);
        assert!(
            rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
            "{} drew nothing",
            entry.id
        );
    }
}

/// The platform's colour appears only where a skin asks for it, and a skin
/// without `[history.event]` draws an event exactly as it draws chat.
#[test]
fn a_skin_without_event_styles_falls_back_to_the_message_row() {
    let base = common::skin("nanatype.livechat");
    assert_eq!(
        base.history
            .event
            .as_ref()
            .unwrap()
            .super_chat
            .as_ref()
            .unwrap()
            .price_color,
        Some(nanatype_theme::EventPriceColor::Fill)
    );
    assert!(price_pixels(&render(base.clone(), None, "livechat-designed")) > 2_000);

    let mut plain = (*base).clone();
    plain.history.event = None;
    let plain = Arc::new(plain);
    let fallback = render(plain.clone(), None, "livechat-fallback");
    assert_eq!(price_pixels(&fallback), 0);

    // The same rows as plain chat: only the role of the row differs.
    let mut as_chat = rows();
    for row in &mut as_chat {
        row.item.kind = SourceKind::Chat;
        row.item.event = None;
    }
    let mut settings = Settings::default();
    settings.history.visible_items = 8;
    let ui = Ui::new(Arc::new(settings), plain.clone(), Locale::ZhCn);
    ui.history.set(Arc::new(as_chat));
    ui.now.set(1_700_001_200_000);
    let mut session = common::session().themed(false).mount(
        plain.history.width.max(420.0) as u32,
        620,
        move || history::history_window(ui).into_any(),
    );
    session.set_gpu_renderers(nanatype::ui::sweep::renderers());
    session.flush().unwrap();
    let (_, chat) = session.screenshot_rgba().unwrap();
    assert!(chat == fallback, "an undressed event must look like chat");
}
