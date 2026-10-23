//! Isolated, scaled samples of a skin through the real dialogue, history and
//! input renderers. Samples use private signals: no live draft, history or
//! source is written.
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, widget};
use nana_ui_core::{
    BackgroundImage, LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec, TransformOrigin,
};
use nanatype_model::{HistoryItem, Rgba8, SourceId, SourceKind};
use nanatype_settings::{Settings, SkinPersonalization, SurfacePersonalization};
use nanatype_theme::Skin;

use super::controls::hint;
use super::kit::{gap, inset};
use super::{Stage, dialogue, history};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pane {
    pub font: String,
    pub size: String,
    pub line_height: String,
    pub color: String,
    pub width: String,
    pub height: String,
    pub text: String,
}

impl Pane {
    fn from_saved(p: &SurfacePersonalization, text: String) -> Self {
        Self {
            font: p.font_family.clone().unwrap_or_default(),
            size: p.text_size.map(|v| v.to_string()).unwrap_or_default(),
            line_height: p.line_height.map(|v| v.to_string()).unwrap_or_default(),
            color: p.text_color.map(|v| v.to_hex()).unwrap_or_default(),
            width: p.size.map(|v| v[0].to_string()).unwrap_or_default(),
            height: p.size.map(|v| v[1].to_string()).unwrap_or_default(),
            text,
        }
    }

    fn parse(&self, default_size: [f32; 2]) -> Result<SurfacePersonalization, ()> {
        let number = |s: &str| -> Result<Option<f32>, ()> {
            if s.trim().is_empty() {
                Ok(None)
            } else {
                s.trim().parse().map(Some).map_err(|_| ())
            }
        };
        let width = number(&self.width)?;
        let height = number(&self.height)?;
        let p = SurfacePersonalization {
            font_family: (!self.font.trim().is_empty()).then(|| self.font.trim().to_owned()),
            text_size: number(&self.size)?,
            line_height: number(&self.line_height)?,
            text_color: if self.color.trim().is_empty() {
                None
            } else {
                Some(self.color.trim().parse::<Rgba8>().map_err(|_| ())?)
            },
            size: if width.is_none() && height.is_none() {
                None
            } else {
                Some([
                    width.unwrap_or(default_size[0]),
                    height.unwrap_or(default_size[1]),
                ])
            },
        };
        p.valid().then_some(p).ok_or(())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub display: Pane,
    pub history: Pane,
    pub params: ParamDraft,
}

/// Unsaved param values. `values` holds the last valid value of each param
/// the user touched; `raw` the text of fields that do not parse yet, which
/// the page shows with an error while the sample keeps the valid value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamDraft {
    pub values: std::collections::BTreeMap<String, nanatype_model::ParamValue>,
    pub raw: std::collections::BTreeMap<String, String>,
}

impl ParamDraft {
    /// The value `id` currently shows: the draft's, else the skin default.
    pub fn current(&self, skin: &Skin, id: &str) -> Option<nanatype_theme::Resolved> {
        let spec = skin.params.get(id)?;
        self.values
            .get(id)
            .and_then(|v| spec.coerce(v).ok())
            .or_else(|| skin.param_defaults().remove(id))
    }

    /// Every param's current value.
    pub fn resolved(
        &self,
        skin: &Skin,
    ) -> std::collections::BTreeMap<String, nanatype_theme::Resolved> {
        nanatype_theme::coerce_values(skin, &self.values, &mut Vec::new())
    }

    pub fn set(&mut self, id: &str, value: nanatype_model::ParamValue) {
        self.raw.remove(id);
        self.values.insert(id.to_owned(), value);
    }

    /// Text typed into `id`'s field: kept as the value when it parses,
    /// otherwise kept aside so the field can show what is wrong.
    pub fn input(
        &mut self,
        skin: &Skin,
        id: &str,
        text: &str,
    ) -> Result<(), nanatype_theme::ParamError> {
        let Some(spec) = skin.params.get(id) else {
            return Ok(());
        };
        match spec.parse_input(text) {
            Ok(v) => {
                self.set(id, v);
                Ok(())
            }
            Err(e) => {
                self.raw.insert(id.to_owned(), text.to_owned());
                Err(e)
            }
        }
    }

    /// Apply a preset: its values, and every other param back to default.
    pub fn apply_preset(&mut self, skin: &Skin, preset: &str) {
        if let Some(values) = skin.preset_values(preset) {
            self.raw.clear();
            self.values = values.into_iter().map(|(k, v)| (k, v.to_value())).collect();
        }
    }

    /// The preset whose values the draft shows exactly, if any.
    pub fn matching_preset<'a>(&self, skin: &'a Skin) -> Option<&'a str> {
        let current = self.resolved(skin);
        skin.ordered_presets()
            .into_iter()
            .map(|(id, _)| id)
            .find(|id| skin.preset_values(id).as_ref() == Some(&current))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InvalidProfile;
impl std::fmt::Display for InvalidProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid preview font, dimensions or color")
    }
}
impl std::error::Error for InvalidProfile {}

impl Draft {
    /// The saved personalization of skin `id`, as fields. A legacy region
    /// override is padded by `overscan` (that skin's; the active skin's when
    /// `None`).
    pub fn load(settings: &Settings, id: &str, ui: Stage, overscan: Option<f32>) -> Self {
        let p = settings
            .skin
            .personalizations
            .get(id)
            .cloned()
            .unwrap_or_else(|| SkinPersonalization {
                display: SurfacePersonalization {
                    text_size: settings.skin.overrides.text_size,
                    text_color: settings.skin.overrides.text_color,
                    size: settings.overlays.display.size.or_else(|| {
                        settings.skin.overrides.region.map(|[w, h]| {
                            let pad = overscan
                                .unwrap_or_else(|| ui.skin.with_untracked(|s| s.dialogue.overscan))
                                * 2.0;
                            [w + pad, h + pad]
                        })
                    }),
                    ..Default::default()
                },
                history: SurfacePersonalization {
                    text_color: settings.skin.overrides.text_color,
                    size: settings.overlays.history.size,
                    ..Default::default()
                },
                params: Default::default(),
            });
        Self {
            display: Pane::from_saved(&p.display, ui.now_t("skin-edit-sample")),
            history: Pane::from_saved(&p.history, ui.now_t("skin-edit-history-sample")),
            params: ParamDraft {
                values: p.params,
                raw: Default::default(),
            },
        }
    }

    pub fn pane(&self, history: bool) -> &Pane {
        if history {
            &self.history
        } else {
            &self.display
        }
    }
    pub fn pane_mut(&mut self, history: bool) -> &mut Pane {
        if history {
            &mut self.history
        } else {
            &mut self.display
        }
    }

    pub fn profile(&self, base: &Skin) -> Result<SkinPersonalization, InvalidProfile> {
        Ok(SkinPersonalization {
            display: self
                .display
                .parse(default_size(base, false))
                .map_err(|()| InvalidProfile)?,
            history: self
                .history
                .parse(default_size(base, true))
                .map_err(|()| InvalidProfile)?,
            params: {
                let mut values = self.params.values.clone();
                nanatype_theme::retain_valid_params(base, &mut values);
                values
            },
        })
    }
}

pub fn default_size(skin: &Skin, history: bool) -> [f32; 2] {
    if history {
        [skin.history.width, skin.history.height]
    } else {
        skin.dialogue
            .region
            .map(|v| v + skin.dialogue.overscan * 2.0)
    }
}

/// The unsaved fields of skin `id`, or its saved ones (see [`Draft::load`]).
pub fn draft(ui: Stage, id: &str, overscan: Option<f32>) -> Draft {
    ui.skin_drafts
        .with(|map| map.get(id).cloned())
        .unwrap_or_else(|| {
            ui.settings
                .with_untracked(|s| Draft::load(s, id, ui, overscan))
        })
}

pub fn change(ui: Stage, id: &str, overscan: Option<f32>, f: impl FnOnce(&mut Draft)) {
    let mut value = draft(ui, id, overscan);
    f(&mut value);
    ui.skin_drafts.update(|map| {
        map.insert(id.to_owned(), value);
    });
}

/// Whether the skin previews on screen have effects that move, so the window
/// showing them needs frames.
pub fn animating(ui: Stage, now: u64) -> bool {
    super::shader_effects::preview_animating(ui)
        || super::text_light::preview_animating(ui, now)
        || super::text_scramble::preview_animating(ui, now)
}

/// Which window a sample draws the skin in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    Display,
    History,
    /// The input window's skin preview: the box around a private editor.
    Input,
}
impl Surface {
    pub fn of(history: bool) -> Self {
        if history {
            Self::History
        } else {
            Self::Display
        }
    }
}

/// Sample data uses private signals; no production draft, history or source is written.
pub fn sample(
    ui: Stage,
    base: &Skin,
    id: &str,
    draft: &Draft,
    surface: Surface,
    urls: Arc<std::collections::HashMap<String, String>>,
) -> Option<(AnyView, [f32; 2])> {
    let profile = draft.profile(base).ok()?;
    let history_mode = surface == Surface::History;
    let mut settings = (*ui.settings.get_untracked()).clone();
    settings.skin.id = id.to_owned();
    // The input window draws its box from the display profile, with the
    // skin preview on and the sample speaker in the name plate.
    settings.general.input_skin_preview = true;
    settings.general.speaker_name = ui.now_t("skins-preview-name");
    settings
        .skin
        .personalizations
        .insert(id.to_owned(), profile);
    settings.overlays.history.lock = nanatype_settings::Lock::Locked;
    settings.history.hide_latest = nanatype_settings::HideLatest::Never;
    settings.history.fade_after_s = None;
    settings.history.chat.enabled = true;
    settings.history.show_time = true;
    let skin = Arc::new(crate::skin_theme::prepare(
        &crate::skin_theme::personalized(base, &settings, id),
        &Default::default(),
        &Default::default(),
        ui.locale.get_untracked().tag(),
    ));
    let canvas = if history_mode {
        crate::canvas::Canvas::History
    } else {
        crate::canvas::Canvas::Display
    };
    let (w, h) = crate::canvas::size_of(canvas, &skin, &settings);
    let mut preview = Stage::new(Arc::new(settings), skin.clone(), ui.locale.get_untracked());
    preview.asset_urls.set(urls.clone());
    preview.reduced_motion = ui.reduced_motion;
    preview.now = ui.now;
    preview.shader_preview = ui.shader_preview;
    preview.live_preview = ui.live_preview;
    let view = if history_mode {
        let rows = draft
            .history
            .text
            .lines()
            .enumerate()
            .map(|(i, text)| {
                let doc = nanatype_markup::parse(text).doc;
                let item = HistoryItem {
                    source: SourceId::local(),
                    kind: SourceKind::Typewriter,
                    line: None,
                    author: if i % 2 == 0 {
                        ui.now_t("skins-preview-name")
                    } else {
                        ui.now_t("skin-edit-long-name")
                    },
                    avatar: None,
                    doc: doc.clone(),
                    at: 1_700_000_000_000 + i as u64 * 180_000,
                    uid: None,
                    reply: None,
                };
                nanatype_core::HistoryRow {
                    key: i as u64,
                    item,
                    doc: Arc::new(doc),
                    reveal: None,
                }
            })
            .collect();
        preview.history.set(Arc::new(rows));
        history::history_window(preview).into_any()
    } else if surface == Surface::Input {
        preview
            .draft
            .set(Arc::new(nanatype_markup::parse(&draft.display.text).doc));
        widget(
            Stack::column(0.0)
                .width(LengthSpec::Px(w))
                .height(LengthSpec::Px(h)),
        )
        .key("skin-preview-input")
        .children(dialogue::dialogue_box(
            preview,
            dialogue::Source::Draft,
            Some(super::editor_view::preview_editor(preview)),
        ))
        .into_any()
    } else {
        let ctx = dialogue::BoxCtx {
            skin,
            urls,
            source: dialogue::Source::Preview,
            preview_name: ui.now_t("skins-preview-name"),
        };
        dialogue::preview_box(
            preview,
            ctx,
            &nanatype_markup::parse(&draft.display.text).doc,
        )
    };
    Some((view, [w, h]))
}

/// A font registered for the skin previews. Previews that use the same font
/// share its registration; dropping the last one withdraws it.
pub struct PreviewFont {
    _scope: nana_ui::HostFontScope,
    family: Option<String>,
}

/// Preview fonts still held, by what they were registered from: a
/// `builtin:` reference, or a skin's font file with a hash of its bytes.
type FontKey = (String, u64);
static PREVIEW_FONTS: LazyLock<Mutex<HashMap<FontKey, Weak<PreviewFont>>>> =
    LazyLock::new(Default::default);

/// The registration of `key`, registering `bytes` only when no preview holds
/// it: every registration restarts the text layout of every window.
fn preview_font(key: FontKey, bytes: impl FnOnce() -> Option<Vec<u8>>) -> Option<Arc<PreviewFont>> {
    let mut fonts = PREVIEW_FONTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(font) = fonts.get(&key).and_then(Weak::upgrade) {
        return Some(font);
    }
    let mut scope = nana_ui::HostFontScope::new();
    let family = scope.add_bytes(bytes()?).ok()?.families().first().cloned();
    let font = Arc::new(PreviewFont {
        _scope: scope,
        family,
    });
    fonts.retain(|_, f| f.strong_count() > 0);
    fonts.insert(key, Arc::downgrade(&font));
    Some(font)
}

/// What a skin preview needs before it can draw: the skin with its own fonts
/// registered and its font roles resolved. Building it reads and registers
/// font files, so a product builds it off the UI thread.
pub struct StageAssets {
    pub base: Arc<Skin>,
    fonts: Vec<Arc<PreviewFont>>,
    /// Built-in fonts a draft named by family, registered when first named.
    picked: Mutex<Vec<Arc<PreviewFont>>>,
}

impl StageAssets {
    /// `raw` with its fonts registered, read with `read_font` unless built
    /// in, and prepared for the UI `language`.
    pub fn prepare(
        mut raw: Skin,
        read_font: impl Fn(&str) -> Option<Vec<u8>>,
        language: &str,
    ) -> Self {
        let mut fonts = Vec::new();
        let mut register = |role: &nanatype_theme::FontSpec| {
            let file = role.file.as_deref()?;
            let font = if nanatype_theme::is_builtin_font(file) {
                preview_font((file.to_owned(), 0), || {
                    nanatype_theme::builtin_font(file).map(<[u8]>::to_vec)
                })
            } else {
                let bytes = read_font(file)?;
                let mut hash = std::hash::DefaultHasher::new();
                std::hash::Hash::hash(&bytes, &mut hash);
                let key = (file.to_owned(), std::hash::Hasher::finish(&hash));
                preview_font(key, move || Some(bytes))
            }?;
            let family = font.family.clone();
            fonts.push(font);
            family
        };
        let families = [&raw.fonts.body, &raw.fonts.name, &raw.fonts.accent].map(&mut register);
        if let Some(role) = &mut raw.fonts.latin
            && let Some(family) = register(role)
        {
            role.families.insert(0, family);
        }
        let base = Arc::new(crate::skin_theme::prepare(
            &raw,
            &Default::default(),
            &families,
            language,
        ));
        Self {
            base,
            fonts,
            picked: Mutex::default(),
        }
    }

    /// Register the built-in font whose family `family` names, so a draft
    /// that picks it shows it. Other families are the system's.
    pub fn pick_family(&self, family: &str) {
        let family = family.trim();
        let named = |f: &Arc<PreviewFont>| {
            f.family
                .as_deref()
                .is_some_and(|g| g.eq_ignore_ascii_case(family))
        };
        let mut picked = self.picked.lock().unwrap_or_else(|e| e.into_inner());
        if family.is_empty() || self.fonts.iter().chain(picked.iter()).any(named) {
            return;
        }
        let Some(font) = nanatype_theme::BuiltinFonts::embedded()
            .fonts()
            .iter()
            .find(|f| f.family().is_some_and(|g| g.eq_ignore_ascii_case(family)))
        else {
            return;
        };
        let bytes = font.bytes;
        if let Some(font) = preview_font((format!("builtin:{}", font.path), 0), || {
            Some(bytes.to_vec())
        }) {
            picked.push(font);
        }
    }
}

/// How far a `w` × `h` sample shrinks to fit `available` px: tall previews
/// as well as wide ones, without altering their layout.
fn fit(available: f32, [w, h]: [f32; 2]) -> f32 {
    ((available - 24.0).max(1.0) / w).min(340.0 / h).min(1.0)
}

/// The stage around `content`: the checkerboard and, under it, the
/// sample's size and scale.
fn framed(dark: bool, content: AnyView, [w, h]: [f32; 2], scale: f32) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::CONTROL))
        .children((
            widget(
                Stack::column(0.0)
                    .padding_step(inset::step::PANEL, inset::step::PANEL)
                    .with_layout(move |l| {
                        l.paint.background_image =
                            Some(BackgroundImage::url(super::kit::checker_url(dark)));
                    }),
            )
            .children(content),
            hint(format!(
                "{} × {} px · {:.0}%",
                w.round(),
                h.round(),
                scale * 100.0
            )),
        ))
        .into_any()
}

/// The stage while its [`StageAssets`] are prepared: the frame at the size
/// the sample will take, so nothing moves when it lands.
pub fn stage_placeholder(ui: Stage, skin: &Skin, surface: Surface, width: Signal<f32>) -> AnyView {
    let history_mode = surface == Surface::History;
    let size = draft(ui, &skin.meta.id, Some(skin.dialogue.overscan))
        .profile(skin)
        .ok()
        .and_then(|p| {
            if history_mode {
                p.history.size
            } else {
                p.display.size
            }
        })
        .unwrap_or_else(|| default_size(skin, history_mode));
    dynamic(
        move || (width.get(), ui.chrome_dark.get()),
        move |(available, dark)| {
            let scale = fit(*available, size);
            let content = widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(size[0] * scale))
                    .height(LengthSpec::Px(size[1] * scale))
                    .align(nana_ui_core::AlignSpec::Center)
                    .justify(nana_ui_core::JustifySpec::Center),
            )
            .key("skin-preview-loading")
            .children(hint(ui.now_t("skin-preview-loading")));
            framed(*dark, content.into_any(), size, scale)
        },
    )
    .into_any()
}

/// An isolated, scaled sample of the prepared skin through the real dialogue
/// and history renderers. Nothing touches production state.
pub fn stage_view(
    ui: Stage,
    assets: Arc<StageAssets>,
    urls: Arc<std::collections::HashMap<String, String>>,
    surface: Surface,
    width: Signal<f32>,
) -> AnyView {
    let sample_id = assets.base.meta.id.clone();
    let sample_overscan = assets.base.dialogue.overscan;
    dynamic(
        move || {
            (
                draft(ui, &sample_id, Some(sample_overscan)),
                width.get(),
                ui.chrome_dark.get(),
                ui.shader_preview.get(),
                ui.settings.with(|s| s.skin.overrides.clone()),
            )
        },
        move |(draft, available, dark, _, _)| {
            assets.pick_family(&draft.pane(surface == Surface::History).font);
            let base = &assets.base;
            let Some((view, [w, h])) =
                sample(ui, base, &base.meta.id, draft, surface, urls.clone())
            else {
                return hint(ui.now_t("skin-edit-invalid"));
            };
            let scale = fit(*available, [w, h]);
            let stage = widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(w * scale))
                    .height(LengthSpec::Px(h * scale))
                    .with_layout(|l| {
                        l.position = PositionSpec::Relative;
                        l.overflow_x = OverflowSpec::Hidden;
                        l.overflow_y = OverflowSpec::Hidden;
                    }),
            )
            .key("skin-preview-box")
            .children(
                widget(
                    Stack::column(0.0)
                        .width(LengthSpec::Px(w))
                        .height(LengthSpec::Px(h))
                        .with_layout(move |l| {
                            l.position = PositionSpec::Absolute;
                            l.offset_left = Some(LengthSpec::Px(0.0));
                            l.offset_top = Some(LengthSpec::Px(0.0));
                            l.pointer_events = Some(PointerEventsSpec::None);
                            l.transform = Some(super::surface::affine(0.0, 0.0, 0.0, scale));
                            l.transform_origin = Some(TransformOrigin {
                                x: LengthSpec::Px(0.0),
                                y: LengthSpec::Px(0.0),
                            });
                        }),
                )
                .key("skin-preview-canvas")
                .children(view),
            );
            framed(*dark, stage.into_any(), [w, h], scale)
        },
    )
    .into_any()
}

/// [`StageAssets::prepare`] and [`stage_view`] in one go, on this thread.
pub fn stage(
    ui: Stage,
    raw: Skin,
    read_font: impl Fn(&str) -> Option<Vec<u8>>,
    urls: Arc<std::collections::HashMap<String, String>>,
    surface: Surface,
    width: Signal<f32>,
) -> (AnyView, Arc<Skin>) {
    let assets = Arc::new(StageAssets::prepare(
        raw,
        read_font,
        ui.locale.get_untracked().tag(),
    ));
    let base = assets.base.clone();
    (stage_view(ui, assets, urls, surface, width), base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skin(id: &str) -> Skin {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        (*registry.load(id).unwrap().skin).clone()
    }

    #[test]
    fn previews_register_only_the_fonts_they_use_once_and_share_them() {
        let mut halo = skin("nanatype.halo");
        for role in [
            &mut halo.fonts.body,
            &mut halo.fonts.name,
            &mut halo.fonts.accent,
        ] {
            role.file = Some("fonts/body.ttf".into());
        }
        let read =
            |file: &str| (file == "fonts/body.ttf").then(|| nana_ui::UI_FONT_REGULAR.to_vec());
        let a = StageAssets::prepare(halo.clone(), read, "zh-CN");
        let b = StageAssets::prepare(halo, read, "zh-CN");
        // Body, name and accent name the same file: one registration, which
        // a second preview of the skin reuses.
        assert_eq!(a.fonts.len(), 3);
        assert!(a.fonts.iter().all(|f| Arc::ptr_eq(f, &a.fonts[0])));
        assert!(Arc::ptr_eq(&a.fonts[0], &b.fonts[0]));
        let own = a.fonts[0].family.clone().unwrap();
        assert_eq!(a.base.fonts.body.families.first(), Some(&own));
        // Nothing else registers until a draft names a built-in family.
        a.pick_family(&own);
        a.pick_family("Arial");
        assert!(a.picked.lock().unwrap().is_empty());
        if let Some(other) = nanatype_theme::BuiltinFonts::embedded()
            .fonts()
            .iter()
            .filter_map(|f| f.family())
            .find(|f| !f.eq_ignore_ascii_case(&own))
        {
            a.pick_family(&other);
            a.pick_family(&other.to_uppercase());
            assert_eq!(a.picked.lock().unwrap().len(), 1);
        }
    }
}
