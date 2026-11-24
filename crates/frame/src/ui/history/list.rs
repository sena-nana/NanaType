use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nana_ui::runtime::view::{
    AnyView, Computed, Implicit, IntoView, Signal, computed, dynamic, each, signal, watch_effect,
    widget,
};
use nana_ui::runtime::{
    Activate, AnimatableProperty, IconButton, NodeStyle, ScrollAxes, ScrollView, SizeChanged,
    Stack, Text, TextChanged, TextInput, TextInputState,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, LengthSpec, PointerEventsSpec, PositionSpec};
use nanatype_core::HistoryRow;
use nanatype_theme::{DecorLayer, HistoryLayout, HistoryTitle, Skin};

use super::super::dialogue;
use super::super::surface::Surface;
use super::super::{Stage, c};
use super::row::{
    RECEDE_MS, ROW_GENTLE, column_width, entrance, glide_ease, inline_time_fits, message_view,
    rise, row_gap, separator_view, viewport_height,
};
use super::selection::{
    follow_ups, log_presence, query, search_allowed_for, visible_keys, visible_rows,
};

#[allow(clippy::too_many_arguments)]
fn entry_view(
    ui: Stage,
    row_sig: Signal<HistoryRow>,
    key: u64,
    first: bool,
    follows: bool,
    slide: Option<Slide>,
    items: Computed<Vec<u64>>,
    skin: Arc<Skin>,
    settings: Arc<nanatype_settings::Settings>,
    col_w: Computed<f32>,
) -> AnyView {
    let gap = if first { 0.0 } else { row_gap(&skin, follows) };
    let origin = entrance(&skin.history, false).map(|(_, origin)| origin);
    let rule = (!first && !follows)
        .then(|| separator_view(&skin, gap))
        .flatten();
    let message = message_view(ui, row_sig, follows, skin.clone(), settings, col_w);
    // Older rows step back as new ones arrive; a search shows what it found
    // at full strength.
    let message = match skin.history.recede {
        Some(recede) => widget(Stack::column(0.0).width(LengthSpec::Fill))
            .bind(move |s: &mut Stack| {
                let rank = items.with(|shown| {
                    shown
                        .iter()
                        .rev()
                        .position(|k| *k == key)
                        .unwrap_or_default()
                });
                let opacity = if query(ui).is_some() {
                    1.0
                } else {
                    recede.opacity(rank)
                };
                s.layout_mut().opacity = Some(opacity);
            })
            .animate([Implicit::new(
                AnimatableProperty::Opacity,
                std::time::Duration::from_millis(RECEDE_MS),
            )
            .ease(ROW_GENTLE)])
            .children(message)
            .into_any(),
        None => message,
    };
    // The gap is padding, so the row's measured height includes it.
    let row = widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .with_layout(move |l| {
                l.position = PositionSpec::Relative;
                l.padding_top = Some(LengthSpec::Px(gap));
                l.transforms.origin = origin;
            }),
    )
    .children((rule, message));
    match slide {
        Some(slide) => {
            slide.built(key, gap);
            row.on(move |e: &SizeChanged| slide.row_laid_out(key, e.height))
                .into_any()
        }
        None => row.into_any(),
    }
}

/// What the log showed after its last layout, to tell how far the rows
/// already on screen moved.
#[derive(Default)]
struct SlideState {
    /// Each row's laid-out height, its gap included.
    heights: HashMap<u64, f32>,
    /// The gap each row was last built with.
    gaps: HashMap<u64, f32>,
    /// The rows, and their gaps, as last laid out.
    order: Vec<u64>,
    laid_gaps: HashMap<u64, f32>,
    /// The list's height, the viewport's and the scroll offset they made.
    content: f32,
    viewport: f32,
    offset: Option<f32>,
}

/// Rows already on screen glide to their new place instead of jumping
/// there. `follow_end` scrolls to a new row at once, and the list's own
/// FLIP sees no scroll; so the list's outer layer jumps back by each move
/// and its inner layer eases the same distance back out. Each layout event
/// adds only its own part of the move, so they may arrive in any order.
#[derive(Clone)]
struct Slide {
    state: Arc<Mutex<SlideState>>,
    /// The sum of all moves so far: the outer layer sits this far down, the
    /// inner layer eases to as far up.
    travel: Signal<f32>,
    items: Computed<Vec<u64>>,
}

impl Slide {
    fn new(items: Computed<Vec<u64>>) -> Self {
        Self {
            state: Arc::default(),
            travel: signal(0.0),
            items,
        }
    }

    fn built(&self, key: u64, gap: f32) {
        self.state.lock().unwrap().gaps.insert(key, gap);
    }

    fn row_laid_out(&self, key: u64, height: f32) {
        self.state.lock().unwrap().heights.insert(key, height);
        self.settle();
    }

    fn content_laid_out(&self, height: f32) {
        self.state.lock().unwrap().content = height;
        self.settle();
    }

    fn viewport_laid_out(&self, height: f32) {
        // Resizing the window changes `viewport` and therefore `(content - viewport)`,
        // which should snap immediately without animating existing rows.
        self.state.lock().unwrap().rebase_viewport(height);
    }

    fn settle(&self) {
        let order = nana_ui::runtime::view::untrack(|| self.items.with(Vec::clone));
        let moved = self.state.lock().unwrap().moved(order);
        if moved.abs() > 0.01 {
            self.travel.update(|t| *t += moved);
        }
    }
}

impl SlideState {
    fn rebase_viewport(&mut self, height: f32) {
        if (self.viewport - height).abs() <= 0.01 {
            return;
        }
        self.viewport = height;
        if self.content > 0.0 && self.viewport > 0.0 {
            self.offset = Some((self.content - self.viewport).max(0.0));
        }
    }

    /// How far the rows still shown moved down on screen since the last
    /// call, which now lays out `order`.
    fn moved(&mut self, order: Vec<u64>) -> f32 {
        if self.content <= 0.0 || self.viewport <= 0.0 {
            return 0.0;
        }
        // `follow_end` keeps the list scrolled to its end.
        let offset = (self.content - self.viewport).max(0.0);
        let Some(was) = self.offset.replace(offset) else {
            self.remember(order);
            return 0.0;
        };
        // The first row still shown rose by the rows dropped above it and by
        // the gap it no longer has.
        let first = order
            .iter()
            .find_map(|key| Some((*key, self.order.iter().position(|k| k == key)?)));
        let moved = first.map(|(key, at)| {
            let dropped: f32 = self.order[..at]
                .iter()
                .map(|k| self.heights.get(k).copied().unwrap_or(0.0))
                .sum();
            let gap = |gaps: &HashMap<u64, f32>| gaps.get(&key).copied().unwrap_or(0.0);
            dropped + gap(&self.laid_gaps) - gap(&self.gaps) + offset - was
        });
        self.remember(order);
        moved.unwrap_or(0.0)
    }

    fn remember(&mut self, order: Vec<u64>) {
        self.heights.retain(|k, _| order.contains(k));
        self.gaps.retain(|k, _| order.contains(k));
        self.laid_gaps.clone_from(&self.gaps);
        self.order = order;
    }
}

/// Keep one `Signal<HistoryRow>` per visible key so individual row reveal ticks
/// update only that row without forcing all other rows to inspect `ui.history`.
fn sync_row_signals(map: &Mutex<HashMap<u64, Signal<HistoryRow>>>, rows: &[HistoryRow]) {
    let mut map = map.lock().unwrap();
    for row in rows {
        match map.get(&row.key).copied() {
            Some(sig) => {
                sig.set_if_changed(row.clone());
            }
            None => {
                map.insert(row.key, signal(row.clone()));
            }
        }
    }
    map.retain(|k, _| rows.iter().any(|r| r.key == *k));
}

fn row_signal_for(
    map: &Mutex<HashMap<u64, Signal<HistoryRow>>>,
    visible: Computed<Arc<Vec<HistoryRow>>>,
    key: u64,
) -> Option<Signal<HistoryRow>> {
    if let Some(sig) = map.lock().unwrap().get(&key).copied() {
        return Some(sig);
    }
    let found = nana_ui::runtime::view::untrack(|| {
        visible.with(|rows| rows.iter().find(|r| r.key == key).cloned())
    })?;
    let mut guard = map.lock().unwrap();
    Some(*guard.entry(key).or_insert_with(|| signal(found)))
}

/// The settings the history window reads while it builds its rows; the
/// rest (sound, hotkeys, integrations, the dialogue's own window...) never
/// changes what the log shows.
#[derive(Clone, PartialEq)]
struct LogSettings {
    history: nanatype_settings::HistorySettings,
    lock: nanatype_settings::Lock,
    entrance: Option<nanatype_model::EntranceKind>,
    typing: bool,
    advanced_effects: bool,
    motion: bool,
    skin: String,
    overrides: nanatype_settings::SkinOverrides,
    personalization: Option<nanatype_settings::SkinPersonalization>,
}

impl LogSettings {
    fn of(canvas: crate::canvas::Canvas, s: &Arc<nanatype_settings::Settings>) -> Self {
        let overlay = canvas.overlay(s);
        Self {
            history: s.history.clone(),
            lock: overlay.lock,
            entrance: s.playback.entrance,
            typing: s.playback.typing,
            advanced_effects: s.skin.advanced_effects,
            motion: s.skin.motion,
            skin: s.skin.id.clone(),
            overrides: s.skin.overrides.clone(),
            personalization: s.skin.personalizations.get(&s.skin.id).cloned(),
        }
    }
}

pub fn history_window(ui: Stage) -> impl IntoView {
    history_window_with(ui, false, || widget(Stack::column(0.)).into_any())
}

/// [`history_window`] with `overlay` (the product's plugin views) above the
/// log, and with `search` a search button in its corner.
pub fn history_window_with(
    ui: Stage,
    search: bool,
    overlay: impl Fn() -> AnyView + Send + Sync + 'static,
) -> impl IntoView {
    let overlay = Arc::new(overlay);
    let shown_rows = computed(move || visible_rows(ui));
    let items = computed(move || visible_keys(ui));
    let follows = computed(move || {
        if ui.skin.with(|s| !s.history.group_speakers) {
            return Vec::new();
        }
        shown_rows.with(|rows| follow_ups(rows))
    });
    // Rebuilding mounts every row anew, so only what the log is drawn from
    // rebuilds it: not every edit, which replaces the whole settings value,
    // and never viewport width/height changes.
    let key = move || {
        let skin = ui.skin.get();
        (
            Arc::as_ptr(&skin) as usize,
            ui.settings.with(|s| LogSettings::of(ui.log_canvas, s)),
            ui.locale.get(),
            ui.reduced_motion.get(),
        )
    };
    dynamic(key, move |_| {
        let skin = ui.skin.get_untracked();
        let settings = ui.settings.get_untracked();
        let materials_on = super::super::materials::enabled(ui);
        let reduced = ui.reduced_motion.get_untracked();
        let ms = std::time::Duration::from_millis(u64::from(skin.history.entrance_ms.min(2000)));
        let glide = glide_ease(&skin.history);
        let transition = entrance(&skin.history, reduced).map(|(transition, _)| transition);
        let slide = transition.is_some().then(|| Slide::new(items));
        let col_w = computed({
            let skin = skin.clone();
            let settings = settings.clone();
            move || column_width(ui, &skin, &settings)
        });
        let vp_h = computed({
            let skin = skin.clone();
            let settings = settings.clone();
            move || viewport_height(ui, &skin, &settings)
        });
        let is_compact = computed(move || vp_h.get() < 180.0);

        let row_signals: Arc<Mutex<HashMap<u64, Signal<HistoryRow>>>> = Arc::default();
        nana_ui::runtime::view::untrack(|| {
            shown_rows.with(|rows| sync_row_signals(&row_signals, rows));
        });
        let row_signals_effect = row_signals.clone();
        watch_effect(move || {
            shown_rows.with(|rows| sync_row_signals(&row_signals_effect, rows));
        });

        let row_slide = slide.clone();
        let row_skin = skin.clone();
        let row_settings = settings.clone();
        let is_inline_skin = skin.history.layout == HistoryLayout::Inline;
        let list = each(
            items,
            |k: &u64| *k,
            move |k: u64| {
                let slide = row_slide.clone();
                let skin = row_skin.clone();
                let settings = row_settings.clone();
                let row_signals = row_signals.clone();
                let h_for_key = skin.history.clone();
                dynamic(
                    move || {
                        let inline_mode = is_inline_skin.then(|| {
                            let cw = col_w.get();
                            (cw >= 160.0, inline_time_fits(&h_for_key, cw))
                        });
                        (
                            items.with(|i| i.first() == Some(&k)),
                            follows.with(|f| f.contains(&k)),
                            inline_mode,
                        )
                    },
                    move |(first, f, _)| {
                        let Some(row_sig) = row_signal_for(&row_signals, shown_rows, k) else {
                            return widget(Stack::row(0.0)).into_any();
                        };
                        entry_view(
                            ui,
                            row_sig,
                            k,
                            *first,
                            *f,
                            slide.clone(),
                            items,
                            skin.clone(),
                            settings.clone(),
                            col_w,
                        )
                    },
                )
            },
        );
        let list = match transition {
            Some(t) => list.transition(t),
            None => list,
        };
        let list = match &slide {
            Some(slide) => {
                let travel = slide.travel;
                widget(Stack::column(0.0).width(LengthSpec::Fill))
                    .bind(move |s: &mut Stack| {
                        s.layout_mut().transforms.transform = Some(rise(-travel.get()));
                    })
                    .animate([Implicit::new(AnimatableProperty::Transform, ms).ease(glide)])
                    .children(list)
                    .into_any()
            }
            None => list.into_any(),
        };
        let h = &skin.history;
        let [mt, mr, mb, ml] = h.panel.as_ref().map_or(h.margin, |p| p.margin);
        let [pl, pt, pr, pb] = h.panel.as_ref().map_or(h.padding, |p| p.padding);
        let panel_surface = Surface::history_panel(&skin).map(|s| s.materials(materials_on));
        let unlocked = ui
            .settings
            .with_untracked(|x| ui.log_canvas.overlay(x).lock != nanatype_settings::Lock::Locked);
        let compact_height = computed(move || {
            let height = vp_h.get();
            (height < 180.0).then_some(height)
        });
        let decor_layer = |layer: DecorLayer| -> AnyView {
            if !h.decor.iter().any(|d| d.layer == layer) {
                return widget(Stack::row(0.0)).into_any();
            }
            let skin = skin.clone();
            dynamic(
                move || compact_height.get().map(|h| h.to_bits()),
                move |compact_bits| {
                    let compact_h = compact_bits.map(f32::from_bits);
                    let children: Vec<AnyView> = skin
                        .history
                        .decor
                        .iter()
                        .filter(|d| d.layer == layer)
                        .map(|d| {
                            let image = d.image.as_ref().and_then(|path| {
                                ui.asset_urls.with_untracked(|urls| {
                                    urls.get(&format!("skin:{path}")).cloned()
                                })
                            });
                            let mut decor = d.clone();
                            if let Some(height) = compact_h {
                                let extent = decor
                                    .size
                                    .iter()
                                    .filter_map(|v| match v {
                                        nanatype_theme::Len::Px(v) => Some(*v),
                                        _ => None,
                                    })
                                    .fold(1.0_f32, f32::max);
                                let scale = (height * 0.25 / extent).min(1.0);
                                for v in &mut decor.size {
                                    if let nanatype_theme::Len::Px(v) = v {
                                        *v *= scale;
                                    }
                                }
                                decor.offset = decor.offset.map(|v| v * scale);
                                decor.text_size *= scale;
                            }
                            dialogue::decor(
                                &skin,
                                &decor,
                                None::<fn() -> bool>,
                                image,
                                materials_on,
                            )
                        })
                        .collect();
                    widget(Stack::row(0.0)).children(children).into_any()
                },
            )
            .into_any()
        };
        let (below, above) = (
            decor_layer(DecorLayer::Below),
            decor_layer(DecorLayer::Above),
        );
        let title = h.title.as_ref().map(|t| {
            widget(Stack::row(0.0).width(LengthSpec::Fill))
                .visible(move || !is_compact.get())
                .children(title_view(&skin, t))
        });
        widget(Stack::fill_column(0.0))
            .bind(move |s: &mut Stack| {
                let compact = is_compact.get();
                let l = s.layout_mut();
                l.padding_top = Some(LengthSpec::Px(if compact { mt.min(4.0) } else { mt }));
                l.padding_right = Some(LengthSpec::Px(mr));
                l.padding_bottom = Some(LengthSpec::Px(if compact { mb.min(4.0) } else { mb }));
                l.padding_left = Some(LengthSpec::Px(ml));
            })
            .on(move |e: &SizeChanged| {
                ui.history_width.set_if_changed(e.width);
                ui.history_height.set_if_changed(e.height);
            })
            .children((
                // An empty log is invisible without a panel: outline the window
                // while it can be dragged, as the display window does.
                unlocked.then(|| {
                    let (id, key) = if ui.log_canvas == crate::canvas::Canvas::Danmaku {
                        ("danmaku-empty-placeholder", "danmaku-placeholder")
                    } else {
                        ("history-empty-placeholder", "history-placeholder")
                    };
                    dialogue::empty_placeholder(ui, id, key, move || log_presence(ui))
                }),
                widget(
                    Surface::optional_stack(panel_surface.as_ref(), Stack::fill_column(0.0))
                        .with_layout(move |l| {
                            l.position = PositionSpec::Relative;
                            // Window move/scale gestures take priority over scrolling
                            // the list, including in the default interactive mode.
                            if unlocked {
                                l.pointer_events = Some(PointerEventsSpec::None);
                            }
                        }),
                )
                .children((
                    below,
                    title,
                    // Takes the panel's height left under the title (and no
                    // more), so the list scrolls inside it instead of running
                    // past the window's foot.
                    {
                        let scroll = widget(
                            ScrollView::new(ScrollAxes::Vertical)
                                .follow_end(true)
                                .with_layout(|l| {
                                    l.flex_grow = Some(1.0);
                                    l.flex_shrink = Some(1.0);
                                    l.min_height = Some(LengthSpec::Px(0.0));
                                }),
                        );
                        let travel_sig = slide.as_ref().map(|s| s.travel);
                        let content = widget(Stack::column(0.0).with_layout(|l| {
                            // A short log always starts at its content area's top;
                            // follow_end still scrolls overflowing logs to the newest row.
                            l.min_height = Some(LengthSpec::Percent(100.0));
                            l.justify_content = nana_ui_core::JustifySpec::Start;
                        }))
                        .bind(move |s: &mut Stack| {
                            let compact = is_compact.get();
                            let l = s.layout_mut();
                            l.padding_left = Some(LengthSpec::Px(pl));
                            l.padding_top =
                                Some(LengthSpec::Px(if compact { pt.min(4.0) } else { pt }));
                            l.padding_right = Some(LengthSpec::Px(pr));
                            l.padding_bottom =
                                Some(LengthSpec::Px(if compact { pb.min(4.0) } else { pb }));
                            if let Some(travel) = travel_sig {
                                l.transforms.transform = Some(rise(travel.get()));
                            }
                        });
                        match slide {
                            Some(slide) => {
                                let viewport = slide.clone();
                                scroll
                                    .on(move |e: &SizeChanged| {
                                        viewport.viewport_laid_out(e.height);
                                    })
                                    .children(
                                        content
                                            .on(move |e: &SizeChanged| {
                                                slide.content_laid_out(e.height);
                                            })
                                            .children(list),
                                    )
                                    .into_any()
                            }
                            None => scroll.children(content.children(list)).into_any(),
                        }
                    },
                    above,
                    overlay(),
                )),
                search.then(|| search_bar(ui)),
            ))
    })
}

/// A quiet search button in the window's top corner that unfolds into a
/// field. The button again (or Esc, through the host) folds it and clears it.
fn search_bar(ui: Stage) -> AnyView {
    let open = move || ui.history_search.with(Option::is_some);
    let label = ui.now_t("history-search");
    let close = ui.now_t("history-search-close");
    let cluster = widget(Stack::row(0.0).gap(4.0).align(AlignSpec::Center));
    // Across the window's top, at its end; only the field and the button
    // take clicks, so the rest still drags the window.
    let layer = widget(
        Stack::column(0.0)
            .padding(6.0)
            .align(AlignSpec::End)
            .with_layout(|l| {
                l.position = PositionSpec::Absolute;
                l.offset_left = Some(LengthSpec::Px(0.0));
                l.offset_top = Some(LengthSpec::Px(0.0));
                l.offset_right = Some(LengthSpec::Px(0.0));
                l.pointer_events = Some(PointerEventsSpec::None);
            }),
    );
    layer
        .visible(move || ui.settings.with(|s| search_allowed_for(ui.log_canvas, s)))
        .children(
            cluster.children((
                widget(Stack::row(0.0).width(LengthSpec::Px(160.0)))
                    .visible(open)
                    .children(
                        widget(
                            TextInput::new("")
                                .placeholder(ui.now_t("history-search-placeholder"))
                                .label(label.clone())
                                .size(ControlSize::Small),
                        )
                        .node_ref(ui.history_search_field)
                        // Folding clears it, so it opens empty.
                        .bind(move |t: &mut TextInput| {
                            if !open() && !t.state.value.is_empty() {
                                t.state = TextInputState::new(String::new());
                            }
                        })
                        .on(move |e: &TextChanged| {
                            ui.history_search.set(Some(e.value.to_string()));
                        }),
                    ),
                widget(
                    IconButton::new(nana_ui::icons_tabler::SEARCH, label.clone())
                        .size(ControlSize::Small)
                        .kind(ButtonKind::Ghost)
                        .with_tooltip(label.clone()),
                )
                .bind(move |b: &mut IconButton| {
                    let shown = open();
                    b.icon = if shown {
                        nana_ui::icons_tabler::X
                    } else {
                        nana_ui::icons_tabler::SEARCH
                    };
                    b.label = if shown { close.clone() } else { label.clone() }.into();
                })
                .on(move |_: &Activate| {
                    ui.history_search
                        .set(if open() { None } else { Some(String::new()) });
                }),
            )),
        )
        .into_any()
}

/// Put the caret in the search field as it unfolds; `open` is whether the
/// last look saw it open. Folding needs nothing: the field hides and clears.
pub fn focus_search(ui: Stage, doc: &mut nana_ui::runtime::RuntimeDocument, open: &mut bool) {
    if !ui.history_search.with_untracked(Option::is_some) {
        *open = false;
        return;
    }
    if *open {
        return;
    }
    let document = doc.document();
    let cx = doc.context_mut();
    let _ = cx.flush_reactive();
    // Not mounted yet: try again on the next look.
    let Some(field) = ui.history_search_field.get_untracked() else {
        return;
    };
    let _ = cx.focus_node(document, field);
    *open = cx.world().focused(document) == Some(field);
}

/// The header bar across the top of the panel.
fn title_view(skin: &Skin, t: &HistoryTitle) -> AnyView {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.font_size = Some(t.size);
        l.font_weight = Some(t.weight);
        l.text_ext.font_family = skin.fonts.name.families.first().cloned();
        l.color = Some(c(t.color));
        l.white_space_nowrap = true;
    }
    let background = c(t.background);
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Px(t.height))
            .padding_xy(t.padding, 0.0)
            .align(AlignSpec::Center)
            .with_layout(move |l| {
                l.background = Some(background);
                l.flex_shrink = Some(0.0);
            }),
    )
    .children(widget(Text::new(t.text.clone()).style(st)))
    .into_any()
}
