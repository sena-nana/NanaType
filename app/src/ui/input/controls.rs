//! The sidebar's control palette: pacing, staging and text effects as a
//! grouped list, one control per row. Markers that go in at the caret are drawn as the chip the
//! editor shows for them; effects that wrap the selection say so in their
//! group's heading. Hovering a tile names what it does and its markup.
use super::*;
use crate::ui::kit::{Type, gap, inset};
use nana_ui::runtime::PointerHoverChanged;

/// A row's height: one chip, one small control.
const TILE_H: f32 = 28.0;

/// What a tile shows: the editor's chip for a marker, or a glyph and label.
#[derive(Clone)]
enum Face {
    Chip(ChipKind, nana_ui::runtime::Icon),
    Plain(nana_ui::runtime::Icon),
}

#[derive(Clone)]
struct Tile {
    face: Face,
    label: String,
    /// Its markup, for the hint line and for search.
    markup: String,
    /// What it does, for the hint line.
    meaning: String,
    tool: Tool,
}

impl Tile {
    fn new(
        face: Face,
        label: String,
        markup: impl Into<String>,
        meaning: String,
        tool: Tool,
    ) -> Self {
        Self {
            face,
            label,
            markup: markup.into(),
            meaning,
            tool,
        }
    }

    /// Search by label, markup or meaning, ignoring case.
    fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || [&self.label, &self.markup, &self.meaning]
                .iter()
                .any(|s| s.to_lowercase().contains(&q))
    }
}

/// A heading and its tiles. `wraps` groups apply to the selection.
struct Group {
    title: String,
    wraps: bool,
    tiles: Vec<Tile>,
}

fn point(m: Marker) -> Tool {
    Tool::Insert(Inline::Marker(m))
}

fn groups(ui: Ui) -> Vec<Group> {
    let t = |k: &str| ui.now_t(k);
    let skin = ui.skin.get_untracked();
    let chip = Face::Chip;
    let wait = |ms: u32| {
        let label = if ms < 1000 {
            format!("{ms}ms")
        } else {
            format!("{}s", ms as f32 / 1000.0)
        };
        Tile::new(
            chip(ChipKind::Wait, ti::CLOCK),
            label,
            format!("[wait={ms}]"),
            t("cheat-wait"),
            point(Marker::Wait { ms }),
        )
    };
    let rhythm = vec![
        wait(250),
        wait(500),
        wait(1000),
        Tile::new(
            chip(ChipKind::Continue, ti::PLAYER_PLAY),
            t("tb-pause"),
            "[pause]",
            t("cheat-pause"),
            point(Marker::Pause { auto_ms: None }),
        ),
        Tile::new(
            chip(ChipKind::Continue, ti::PLAYER_PLAY),
            t("tb-pause-auto"),
            "[pause auto=1500]",
            t("tb-pause-auto"),
            point(Marker::Pause {
                auto_ms: Some(1500),
            }),
        ),
        Tile::new(
            chip(ChipKind::Page, ti::PAGE_BREAK),
            t("tb-page"),
            "[page]",
            t("cheat-page"),
            point(Marker::Page),
        ),
    ];
    let shake = skin.effects.screen_shake_px;
    let mut stage = vec![Tile::new(
        chip(ChipKind::Shake, ti::WAVE_SINE),
        t("tb-screenshake"),
        format!("[screenshake={shake},300]"),
        t("tb-screenshake"),
        point(Marker::ScreenShake {
            amp: shake,
            ms: 300,
        }),
    )];
    stage.extend(skin.sounds.se.keys().map(|name| {
        Tile::new(
            chip(ChipKind::Sound, ti::MUSIC),
            name.clone(),
            format!("[se={name}]"),
            t("cheat-se"),
            point(Marker::Sound {
                sound: name.clone(),
                gain: 1.0,
            }),
        )
    }));
    let wrap = |icon, key: &str, tag: &str, tool: Tool| {
        Tile::new(
            Face::Plain(icon),
            t(key),
            format!("[{tag}]…[/{tag}]"),
            t(key),
            tool,
        )
    };
    let effects = vec![
        wrap(
            ti::ACTIVITY,
            "tb-effect-shake",
            "shake",
            Tool::Effect(Continuous::new(ContinuousKind::Shake)),
        ),
        wrap(
            ti::WAVE_SINE,
            "tb-effect-wave",
            "wave",
            Tool::Effect(Continuous::new(ContinuousKind::Wave)),
        ),
        wrap(
            ti::ARROW_BOUNCE,
            "tb-effect-jump",
            "jump",
            Tool::Effect(Continuous::new(ContinuousKind::Jump)),
        ),
        wrap(
            ti::RAINBOW,
            "tb-effect-rainbow",
            "rainbow",
            Tool::Effect(Continuous::new(ContinuousKind::Rainbow)),
        ),
        wrap(
            ti::HEARTBEAT,
            "tb-effect-pulse",
            "pulse",
            Tool::Effect(Continuous::new(ContinuousKind::Pulse)),
        ),
        wrap(
            ti::BULB_OFF,
            "tb-effect-flicker",
            "flicker",
            Tool::Effect(Continuous::new(ContinuousKind::Flicker)),
        ),
        wrap(
            ti::BOLD,
            "tb-emphasis",
            "em",
            Tool::Emphasis(Emphasis::default()),
        ),
    ];
    let entrances = vec![
        wrap(
            ti::BLUR,
            "tb-entrance-fade",
            "fade",
            Tool::Entrance(EntranceKind::Fade, None),
        ),
        wrap(
            ti::ZOOM_IN,
            "tb-entrance-pop",
            "pop",
            Tool::Entrance(EntranceKind::Pop, None),
        ),
        wrap(
            ti::ARROW_UP,
            "tb-entrance-slide",
            "slide",
            Tool::Entrance(EntranceKind::SlideUp, None),
        ),
    ];
    let pace = |icon, key: &str, markup: &str, pace: Pace| {
        Tile::new(Face::Plain(icon), t(key), markup, t(key), Tool::Pace(pace))
    };
    let speed = vec![
        pace(
            ti::WALK,
            "tb-speed-slow",
            "[speed=2]…[/speed]",
            Pace::Scale(2.0),
        ),
        pace(
            ti::RUN,
            "tb-speed-fast",
            "[speed=0.5]…[/speed]",
            Pace::Scale(0.5),
        ),
        pace(
            ti::BOLT,
            "tb-speed-instant",
            "[instant]…[/instant]",
            Pace::Instant,
        ),
        pace(ti::REFRESH, "tb-speed-reset", "", Pace::Inherit),
    ];
    vec![
        Group {
            title: t("dock-group-rhythm"),
            wraps: false,
            tiles: rhythm,
        },
        Group {
            title: t("dock-group-stage"),
            wraps: false,
            tiles: stage,
        },
        Group {
            title: t("dock-group-effects"),
            wraps: true,
            tiles: effects,
        },
        Group {
            title: t("dock-group-entrance"),
            wraps: true,
            tiles: entrances,
        },
        Group {
            title: t("dock-group-speed"),
            wraps: true,
            tiles: speed,
        },
    ]
}

/// A tile as a full-width flat row: the face at the start (a marker's is the
/// editor's chip), its markup faint at the end.
fn tile_view(ui: Ui, tile: Tile, query: Signal<String>, hint: Signal<Option<String>>) -> AnyView {
    let look = super::super::rich::MentionLook::for_ui(ui.chrome_dark.get_untracked()).markers;
    let face = match tile.face {
        Face::Chip(kind, icon) => marker_chip(&look, kind, icon, &tile.label),
        Face::Plain(icon) => {
            let mut label = Type::Caption.style();
            Arc::make_mut(&mut label.layout).white_space_nowrap = true;
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center)
                    .with_layout(|l| {
                        l.padding_left = Some(LengthSpec::Px(inset::CHIP_X));
                        l.padding_right = Some(LengthSpec::Px(inset::CHIP_X));
                    }),
            )
            .children((
                widget(IconGlyph::new(icon).size(icon::SM).decorative()),
                widget(Text::new(tile.label.clone()).decorative().style(label)),
            ))
            .into_any()
        }
    };
    let markup = (!tile.markup.is_empty()).then(|| {
        widget(
            Text::new(tile.markup.clone())
                .decorative()
                .style(Type::Micro.style())
                .color(SemanticColorRole::Faint)
                .truncating(),
        )
    });
    let row = widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .with_layout(|l| l.padding_right = Some(LengthSpec::Px(inset::CHIP_X))),
    )
    .children((face, widget(Stack::spacer()), markup));
    let words = tile.clone();
    let line = if tile.markup.is_empty() {
        tile.meaning.clone()
    } else {
        format!("{} · {}", tile.meaning, tile.markup)
    };
    let tool = tile.tool;
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.height = Some(LengthSpec::Px(TILE_H));
        l.min_height = Some(LengthSpec::Px(TILE_H));
        // The face carries its own inset, so the frame hugs it evenly.
        l.padding_left = Some(LengthSpec::Px(gap::HAIR));
        l.padding_right = Some(LengthSpec::Px(gap::HAIR));
        l.border_width = Some(1.0);
        l.border_style = Some(nana_ui_core::BorderStyle::Solid);
        l.width = Some(LengthSpec::Fill);
        l.flex_shrink = Some(0.0);
    }
    st.radius = Some(nana_ui_core::RadiusTier::Sm);
    st.border = Some(SemanticColorRole::Border);
    st.background = Some(SemanticColorRole::Surface);
    st.interaction.hovered.border = Some(SemanticColorRole::BorderStrong);
    widget(
        Button::new("")
            .accessible_name(tile.label)
            .kind(ButtonKind::Text)
            .style(st),
    )
    .children(row)
    .visible(move || words.matches(&query.get()))
    .on(move |e: &PointerHoverChanged| {
        if e.hovered {
            hint.set(Some(line.clone()));
        } else if hint.with_untracked(|h| h.as_deref() == Some(line.as_str())) {
            hint.set(None);
        }
    })
    .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(act(tool.clone())))
    .into_any()
}

/// A group's heading: its name, and a bracket mark where it wraps the
/// selection rather than going in at the caret.
fn heading(ui: Ui, group: &Group) -> AnyView {
    let mode = if group.wraps {
        "dock-mode-wrap"
    } else {
        "dock-mode-caret"
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill),
    )
    .children((
        caption(group.title.clone()),
        widget(Stack::spacer()),
        group.wraps.then(|| {
            let mut glyph = IconGlyph::new(ti::BRACKETS).size(icon::SM).decorative();
            glyph.style.foreground = Some(SemanticColorRole::Faint);
            widget(glyph)
        }),
        caption(ui.now_t(mode)),
    ))
    .into_any()
}

/// The control palette: search, the groups, and the hint line.
pub(super) fn panel(ui: Ui) -> AnyView {
    let query = signal(String::new());
    let hint = signal(None::<String>);
    let mut search = kit::text_input("").placeholder(ui.now_t("dock-controls-search"));
    Arc::make_mut(&mut search.style.layout).width = Some(LengthSpec::Fill);
    let body = dynamic(
        move || {
            (
                ui.locale.get(),
                ui.chrome_dark.get(),
                ui.skin.with(|s| Arc::as_ptr(s) as usize),
            )
        },
        move |_| {
            let sections: Vec<AnyView> = groups(ui)
                .into_iter()
                .map(|group| {
                    let tiles = group.tiles.clone();
                    let shown = move || query.with(|q| tiles.iter().any(|t| t.matches(q)));
                    let cells: Vec<AnyView> = group
                        .tiles
                        .iter()
                        .cloned()
                        .map(|tile| tile_view(ui, tile, query, hint))
                        .collect();
                    widget(
                        Stack::column(0.0)
                            .gap_step(gap::step::TIGHT)
                            .width(LengthSpec::Fill),
                    )
                    .visible(shown)
                    .children((
                        heading(ui, &group),
                        widget(
                            Stack::column(0.0)
                                .gap_step(gap::step::HAIR)
                                .width(LengthSpec::Fill),
                        )
                        .children(cells),
                    ))
                    .into_any()
                })
                .collect();
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::SECTION)
                    .width(LengthSpec::Fill)
                    .padding_step(gap::step::TIGHT, gap::step::TIGHT),
            )
            .children(sections)
            .into_any()
        },
    );
    widget(
        Stack::fill_column(0.0)
            .gap_step(gap::step::CONTROL)
            .with_layout(|l| {
                l.min_height = Some(LengthSpec::Px(0.0));
                l.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .key("input-controls")
    .children((
        widget(search)
            .key("input-controls-search")
            .on(move |e: &TextChanged| query.set(e.value.to_string())),
        widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
            l.width = Some(LengthSpec::Fill);
            l.height = Some(LengthSpec::Px(0.0));
            l.min_height = Some(LengthSpec::Px(0.0));
            l.flex_grow = Some(1.0);
            l.flex_shrink = Some(1.0);
        }))
        .key("input-controls-scroll")
        .children(body),
        widget(
            Text::new("")
                .style(Type::Micro.style())
                .color(SemanticColorRole::Muted)
                .truncating(),
        )
        .key("input-controls-hint")
        .bind(move |text: &mut Text| {
            let v = hint.get().unwrap_or_else(|| ui.now_t("dock-controls-hint"));
            if text.value != v {
                text.value = v;
            }
        }),
    ))
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_i18n::Locale;

    fn ui() -> Ui {
        let skin = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"))
            .load("nanatype.halo")
            .unwrap()
            .skin;
        Ui::new(Arc::new(Settings::default()), skin, Locale::ZhCn)
    }

    #[test]
    fn markers_go_in_at_the_caret_and_effects_wrap_the_selection() {
        let groups = groups(ui());
        let all: Vec<&Tile> = groups.iter().flat_map(|g| &g.tiles).collect();
        let find = |markup: &str| {
            all.iter()
                .find(|t| t.markup == markup)
                .unwrap_or_else(|| panic!("no tile for {markup}"))
        };
        assert!(matches!(
            find("[wait=500]").tool,
            Tool::Insert(Inline::Marker(Marker::Wait { ms: 500 }))
        ));
        assert!(matches!(
            find("[page]").tool,
            Tool::Insert(Inline::Marker(Marker::Page))
        ));
        assert!(matches!(
            find("[rainbow]…[/rainbow]").tool,
            Tool::Effect(Continuous {
                kind: ContinuousKind::Rainbow,
                ..
            })
        ));
        for group in &groups {
            let point = group.tiles.iter().all(|t| matches!(t.face, Face::Chip(..)));
            assert_eq!(
                point,
                !group.wraps || group.tiles.is_empty(),
                "{}",
                group.title
            );
        }
    }

    #[test]
    fn search_matches_label_markup_and_meaning() {
        let groups = groups(ui());
        let tiles: Vec<&Tile> = groups.iter().flat_map(|g| &g.tiles).collect();
        let hits = |q: &str| tiles.iter().filter(|t| t.matches(q)).count();
        assert_eq!(hits(""), tiles.len());
        assert_eq!(hits("  "), tiles.len());
        assert!(hits("WAIT") >= 3);
        assert_eq!(hits("[rainbow]"), 1);
        assert_eq!(hits("no such control"), 0);
    }
}
