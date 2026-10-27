//! The sidebar's stickers: search, pack tabs, the grid, and a folder-drop
//! target that imports packs.
use super::*;
use crate::ui::kit::{self, Type, gap};
use nana_ui::runtime::{FileDropEvent, TabOption, Tabs, TabsEvent};

/// The strip of pack tabs over the sticker grid.
const PACK_STRIP_H: f32 = 36.0;

/// The tile after the stickers that opens a picture from disk.
fn image_tile(ui: Ui) -> AnyView {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.width = Some(LengthSpec::Px(PICTURE_SWATCH));
        l.height = Some(LengthSpec::Px(PICTURE_SWATCH));
        l.border_width = Some(1.0);
        l.border_style = Some(nana_ui_core::BorderStyle::Dashed);
        l.padding_left = Some(LengthSpec::Px(0.0));
        l.padding_right = Some(LengthSpec::Px(0.0));
    }
    st.radius = Some(nana_ui_core::RadiusTier::Sm);
    st.border = Some(SemanticColorRole::BorderStrong);
    st.foreground = Some(SemanticColorRole::Muted);
    widget(
        Button::new("")
            .accessible_name(ui.now_t("tb-image"))
            .icon(ti::PHOTO_PLUS)
            .kind(ButtonKind::Text)
            .style(st),
    )
    .key("input-dock-sticker-image")
    .bind(move |b: &mut Button| b.disabled = crate::ui::sources::plain_text_only(ui))
    .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::PickFile(FileTarget::InsertImage)))
    .into_any()
}

pub(super) fn panel(ui: Ui) -> AnyView {
    let query = signal(String::new());
    let selected = signal(String::new());
    let hovered = signal(false);
    let target = entity_ref::<Stack>();
    nana_ui::runtime::view::on_mount(move |cx| {
        if let Some(target) = target.get() {
            let _ = cx.set_drop_target(target, nana_ui_core::DropAccepts::files());
        }
    });
    let grid = dynamic(
        move || {
            (
                query.get(),
                selected.get(),
                ui.stickers.with(|s| Arc::as_ptr(s) as usize),
            )
        },
        move |(q, series, _)| {
            let stickers = ui.stickers.get_untracked();
            if stickers.is_empty() {
                let mut style = Type::Body.style();
                let l = Arc::make_mut(&mut style.layout);
                l.width = Some(LengthSpec::Fill);
                l.min_width = Some(LengthSpec::Px(0.0));
                return widget(
                    Stack::column(0.0)
                        .gap_step(gap::step::CONTROL)
                        .width(LengthSpec::Fill),
                )
                .children((
                    widget(
                        Text::new(ui.now_t("empty-stickers"))
                            .style(style)
                            .color(SemanticColorRole::Muted),
                    ),
                    widget(
                        Button::new(ui.now_t("window-settings"))
                            .kind(ButtonKind::Text)
                            .size(ControlSize::Small),
                    )
                    .on_cx(move |_, _: &Activate, cx| {
                        cx.dispatch_program_all(Msg::SettingsPage(
                            crate::ui::settings::Page::Assets,
                        ))
                    }),
                ))
                .into_any();
            }
            let q = q.to_lowercase();
            let series = effective_series(series, &stickers);
            let cells: Vec<AnyView> = stickers
                .iter()
                .filter(|s| series.is_empty() || s.lib == series)
                .filter(|s| {
                    q.is_empty()
                        || s.name.to_lowercase().contains(&q)
                        || s.lib.to_lowercase().contains(&q)
                        || s.asset.to_lowercase().contains(&q)
                        || s.pack_title.to_lowercase().contains(&q)
                })
                .map(|s| {
                    let url = s.url.clone();
                    let asset: Option<AssetRef> = s.asset.parse().ok();
                    let height_em = s.height_em;
                    let image = crate::stickers::cache::lookup(&url);
                    let thumbnail = image.as_ref().map(|c| c.thumbnail()).unwrap_or(url);
                    let animation = image.filter(|c| c.animated());
                    let content = animation.map(|c| {
                        widget(nana_ui::runtime::Thumbnail::new(c.slot()).size(ControlSize::Large))
                            .into_any()
                    });
                    widget(image_button(
                        s.name.clone(),
                        if content.is_some() {
                            String::new()
                        } else {
                            thumbnail
                        },
                    ))
                    .children(content)
                    .bind(move |b: &mut Button| {
                        b.disabled = crate::ui::sources::plain_text_only(ui)
                    })
                    .on_cx(move |_, _: &Activate, cx| {
                        if let Some(asset) = asset.clone() {
                            cx.dispatch_program_all(act(Tool::Insert(Inline::Atom(Atom {
                                kind: AtomKind::Sticker,
                                asset,
                                height_em,
                                alt: String::new(),
                                effects: Default::default(),
                            }))));
                        }
                    })
                    .into_any()
                })
                .chain(std::iter::once(image_tile(ui)))
                .collect();
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .wrap(true)
                    .width(LengthSpec::Fill),
            )
            .children(cells)
            .into_any()
        },
    );
    let grid = widget(
        ScrollView::new(ScrollAxes::Vertical)
            .scrollbars(nana_ui_core::ScrollbarVisibility::Always)
            .with_layout(|l| {
                l.width = Some(LengthSpec::Fill);
                l.height = Some(LengthSpec::Px(0.0));
                l.min_height = Some(LengthSpec::Px(0.0));
                l.flex_grow = Some(1.0);
                l.flex_shrink = Some(1.0);
            }),
    )
    .key("input-dock-stickers-scroll")
    .children(grid);
    // Only while it has something to say: a drop under way, an import, a
    // failure, or how to add the first pack.
    let note = widget(
        Text::new("")
            .style(Type::Micro.style())
            .color(SemanticColorRole::Muted),
    )
    .key("input-dock-sticker-status")
    .visible(move || status(ui, hovered.get()).is_some())
    .bind(move |text: &mut Text| {
        text.value = status(ui, hovered.get()).unwrap_or_default();
    });
    let packs = widget(ScrollView::new(ScrollAxes::Horizontal).with_layout(|l| {
        l.width = Some(LengthSpec::Fill);
        l.height = Some(LengthSpec::Px(PACK_STRIP_H));
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_shrink = Some(0.0);
    }))
    .visible(move || ui.stickers.with(|rows| !rows.is_empty()))
    .children(
        widget(
            Tabs::new("")
                .accepts_external_drop(false)
                .label(ui.now_t("tb-sticker-series")),
        )
        .key("input-dock-sticker-series")
        .bind(move |tabs: &mut Tabs| {
            let rows = ui.stickers.get();
            let series = series(&rows);
            tabs.options =
                std::iter::once(TabOption::new("", ui.now_t("tb-sticker-all")).draggable(false))
                    .chain(
                        series
                            .into_iter()
                            .map(|(lib, title)| TabOption::new(lib, title).draggable(false)),
                    )
                    .collect();
            tabs.selected = Some(effective_series(&selected.get(), &rows).into());
        })
        .on(move |e: &TabsEvent| {
            if let TabsEvent::Select(lib) = e {
                selected.set(lib.to_string());
            }
        }),
    );
    let mut search = kit::text_input("").placeholder(ui.now_t("tb-sticker-search"));
    let layout = Arc::make_mut(&mut search.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .with_layout(|l| {
                l.width = Some(LengthSpec::Fill);
                l.min_width = Some(LengthSpec::Px(0.0));
                l.height = Some(LengthSpec::Px(0.0));
                l.min_height = Some(LengthSpec::Px(0.0));
                l.flex_grow = Some(1.0);
            }),
    )
    .entity_ref(target)
    .on_cx(move |_, e: &FileDropEvent, cx| match e {
        FileDropEvent::Hovered { .. } => hovered.set(true),
        FileDropEvent::Left => hovered.set(false),
        FileDropEvent::Dropped { paths, .. } => {
            hovered.set(false);
            selected.set(String::new());
            cx.dispatch_program_all(Msg::StickerImportFolders(paths.to_vec()));
        }
    })
    .children((
        widget(search)
            .key("input-dock-sticker-search")
            .on(move |e: &TextChanged| query.set(e.value.to_string())),
        packs,
        grid,
        note,
    ))
    .into_any()
}

fn effective_series<'a>(selected: &'a str, rows: &[super::super::StickerRow]) -> &'a str {
    if rows.iter().any(|r| r.lib == selected) {
        selected
    } else {
        ""
    }
}

fn series(rows: &[super::super::StickerRow]) -> Vec<(String, String)> {
    let mut series = std::collections::BTreeMap::new();
    for row in rows {
        series.entry(row.lib.clone()).or_insert_with(|| {
            if row.pack_title.is_empty() {
                row.lib.clone()
            } else {
                row.pack_title.clone()
            }
        });
    }
    let mut counts = std::collections::HashMap::new();
    for title in series.values() {
        *counts.entry(title.clone()).or_insert(0) += 1;
    }
    series
        .into_iter()
        .map(|(lib, title)| {
            let title = if counts[&title] > 1 {
                format!("{title} · {lib}")
            } else {
                title
            };
            (lib, title)
        })
        .collect()
}

fn status(ui: Ui, hovered: bool) -> Option<String> {
    let packs = ui.sticker_packs.get();
    if hovered {
        return Some(ui.now_t("tb-sticker-drop-now"));
    }
    if let Some(pack) = packs.iter().find(|p| p.phase.busy()) {
        let title = pack
            .preset
            .map(|p| ui.now_t(p.title_key()))
            .unwrap_or_else(|| pack.title.clone());
        return Some(ui.tr("tb-sticker-importing", &[("name", title.into())]));
    }
    if let Some(error) = packs.iter().find_map(|p| {
        if let crate::stickers::Phase::Failed(e) = &p.phase {
            Some(e)
        } else {
            None
        }
    }) {
        return Some(ui.tr("stickers-failed", &[("message", error.clone().into())]));
    }
    ui.stickers
        .with(|rows| rows.is_empty())
        .then(|| ui.now_t("tb-sticker-drop-hint"))
}
