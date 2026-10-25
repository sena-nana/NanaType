//! Isolated, scaled samples of a skin through the real dialogue, history and
//! input renderers. Samples use private signals: no live draft, history or
//! source is written.
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, watch_effect, widget};
use nana_ui::runtime::{Activate, ListItem, ListItemRole, Stack};
use nana_ui_core::{
    AlignSpec, BackgroundImage, LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec,
    RadiusTier, SemanticColorRole, TransformOrigin,
};
use nanatype_model::{ChatRef, ChatUser, HistoryItem, Rgba8, SourceId, SourceKind};
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
            history: Pane::from_saved(&p.history, ui.now_t("skin-edit-history-host")),
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

/// Space between the display and the history when one preview draws both.
pub const PAIR_GAP: f32 = 16.0;

/// The box around both windows: the wider width, and the heights plus [`PAIR_GAP`].
pub fn pair_extent(display: [f32; 2], history: [f32; 2]) -> [f32; 2] {
    [
        display[0].max(history[0]),
        display[1] + PAIR_GAP + history[1],
    ]
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

/// One prepared skin and the private stage both windows draw from.
struct SampleStage {
    preview: Stage,
    skin: Arc<Skin>,
    urls: Arc<HashMap<String, String>>,
    display: [f32; 2],
    history: [f32; 2],
}

/// Personalise and prepare `base` once. Display and history share this skin.
fn sample_stage(
    ui: Stage,
    base: &Skin,
    id: &str,
    draft: &Draft,
    urls: Arc<HashMap<String, String>>,
) -> Option<SampleStage> {
    let profile = draft.profile(base).ok()?;
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
    let settings = Arc::new(settings);
    let display = crate::canvas::size_of(crate::canvas::Canvas::Display, &skin, &settings);
    let history = crate::canvas::size_of(crate::canvas::Canvas::History, &skin, &settings);
    let mut preview = Stage::new(settings, skin.clone(), ui.locale.get_untracked());
    preview.asset_urls.set(urls.clone());
    preview.reduced_motion = ui.reduced_motion;
    preview.now = ui.now;
    preview.preview_playback = ui.preview_playback;
    preview.shader_preview = ui.shader_preview;
    preview.live_preview = ui.live_preview;
    Some(SampleStage {
        preview,
        skin,
        urls,
        display: [display.0, display.1],
        history: [history.0, history.1],
    })
}

/// The designed display sample. Previews show this, not text the user typed.
fn designed_display(ui: Stage) -> String {
    ui.now_t("skin-edit-sample")
}

fn history_row(
    key: u64,
    kind: SourceKind,
    author: String,
    uid: Option<&str>,
    reply: Option<ChatRef>,
    at: u64,
    markup: String,
) -> nanatype_core::HistoryRow {
    let parsed = nanatype_markup::parse(&markup);
    debug_assert!(
        parsed.diagnostics.is_empty(),
        "{markup:?} {:?}",
        parsed.diagnostics
    );
    let doc = parsed.doc;
    nanatype_core::HistoryRow {
        key,
        item: HistoryItem {
            source: if kind == SourceKind::Typewriter {
                SourceId::local()
            } else {
                SourceId("bilibili".into())
            },
            kind,
            line: None,
            author,
            avatar: None,
            doc: doc.clone(),
            at,
            uid: uid.map(str::to_owned),
            reply: reply.map(Box::new),
        },
        doc: Arc::new(doc),
        reveal: None,
    }
}

/// History preview rows, oldest first. Twenty seconds keeps a follow-up in
/// one speaker group; three minutes starts another. The host, a viewer, a
/// long name, a reply, a gift and a membership each appear once.
fn designed_history(ui: Stage) -> Vec<nanatype_core::HistoryRow> {
    let host = ui.now_t("skins-preview-name");
    let viewer = ui.now_t("skin-edit-history-viewer-name");
    let said = ui.now_t("skin-edit-history-viewer");
    let quote = ChatRef::new(
        ChatUser {
            uid: "preview-viewer".into(),
            name: viewer.clone(),
            avatar: None,
        },
        &said,
        SourceKind::Chat,
    );
    let t0 = 1_700_000_000_000u64;
    vec![
        history_row(
            0,
            SourceKind::Typewriter,
            host.clone(),
            None,
            None,
            t0,
            ui.now_t("skin-edit-history-host"),
        ),
        history_row(
            1,
            SourceKind::Typewriter,
            host.clone(),
            None,
            None,
            t0 + 20_000,
            ui.now_t("skin-edit-history-host-more"),
        ),
        history_row(
            2,
            SourceKind::Typewriter,
            host.clone(),
            None,
            None,
            t0 + 200_000,
            ui.now_t("skin-edit-history-long"),
        ),
        history_row(
            3,
            SourceKind::Chat,
            viewer.clone(),
            Some("preview-viewer"),
            None,
            t0 + 260_000,
            said,
        ),
        history_row(
            4,
            SourceKind::Chat,
            viewer,
            Some("preview-viewer"),
            None,
            t0 + 275_000,
            ui.now_t("skin-edit-history-viewer-more"),
        ),
        history_row(
            5,
            SourceKind::Chat,
            ui.now_t("skin-edit-long-name"),
            Some("preview-long"),
            None,
            t0 + 400_000,
            ui.now_t("skin-edit-history-long-body"),
        ),
        history_row(
            6,
            SourceKind::Typewriter,
            host,
            None,
            Some(quote),
            t0 + 460_000,
            ui.now_t("skin-edit-history-reply"),
        ),
        history_row(
            7,
            SourceKind::Event,
            ui.now_t("skin-edit-history-fan-name"),
            Some("preview-fan"),
            None,
            t0 + 520_000,
            ui.now_t("skin-edit-history-gift"),
        ),
        history_row(
            8,
            SourceKind::Event,
            ui.now_t("skin-edit-history-captain-name"),
            Some("preview-captain"),
            None,
            t0 + 580_000,
            ui.now_t("skin-edit-history-member"),
        ),
    ]
}

/// How long one history-sample message stays before the next appears.
const HISTORY_PLAYBACK_STEP_MS: u64 = 2_000;

fn history_looping(ui: Stage) -> bool {
    nana_ui::runtime::view::untrack(|| ui.live_preview.get()) && !ui.reduced_motion.get_untracked()
}

/// Reveal the next sample message when one is due. The first call only
/// starts the clock. Returns when the following message is due.
pub fn tick_history_playback(ui: Stage, now: u64) -> Option<u64> {
    if !history_looping(ui) {
        return None;
    }
    let (step, at) = ui.preview_playback.get_untracked();
    let (step, at) = if at == 0 {
        (step, now)
    } else if now >= at.saturating_add(HISTORY_PLAYBACK_STEP_MS) {
        (step.saturating_add(1), now)
    } else {
        return Some(at.saturating_add(HISTORY_PLAYBACK_STEP_MS));
    };
    ui.preview_playback.set((step, at));
    Some(at.saturating_add(HISTORY_PLAYBACK_STEP_MS))
}

/// When the next message is due, without advancing. `now` is the deadline
/// until the clock has started.
pub fn history_playback_deadline(ui: Stage, now: u64) -> Option<u64> {
    if !history_looping(ui) {
        return None;
    }
    let (_, at) = ui.preview_playback.get_untracked();
    Some(if at == 0 {
        now
    } else {
        at.saturating_add(HISTORY_PLAYBACK_STEP_MS)
    })
}

/// Messages revealed at `step`. The last phase of a cycle holds the whole
/// log, then the next step starts again with new row keys so entrances replay.
fn playback_rows(
    script: &[nanatype_core::HistoryRow],
    step: u64,
) -> Vec<nanatype_core::HistoryRow> {
    let n = script.len();
    if n == 0 {
        return Vec::new();
    }
    let cycle = n as u64 + 1;
    let phase = step % cycle;
    let count = if phase + 1 >= cycle {
        n
    } else {
        phase as usize + 1
    };
    let generation = step / cycle;
    script
        .iter()
        .take(count)
        .enumerate()
        .map(|(i, row)| {
            let mut row = row.clone();
            row.key = generation * 16 + i as u64;
            row
        })
        .collect()
}

/// Show the script one message at a time while a preview is on screen.
/// Reduced motion, and any preview that is not live, keeps the whole log.
fn play_history(ui: Stage, script: Arc<Vec<nanatype_core::HistoryRow>>) {
    watch_effect(move || {
        let (step, _) = ui.preview_playback.get();
        let rows = if ui.live_preview.get() && !ui.reduced_motion.get() {
            playback_rows(&script, step)
        } else {
            (*script).clone()
        };
        ui.history.set(Arc::new(rows));
    });
}

fn display_view(ready: &SampleStage, ui: Stage, text: &str) -> AnyView {
    let ctx = dialogue::BoxCtx {
        skin: ready.skin.clone(),
        urls: ready.urls.clone(),
        source: dialogue::Source::Preview,
        preview_name: ui.now_t("skins-preview-name"),
    };
    dialogue::preview_box(ready.preview, ctx, &nanatype_markup::parse(text).doc)
}

/// Sample data uses private signals; no production draft, history or source is written.
pub fn sample(
    ui: Stage,
    base: &Skin,
    id: &str,
    draft: &Draft,
    surface: Surface,
    urls: Arc<HashMap<String, String>>,
) -> Option<(AnyView, [f32; 2])> {
    let ready = sample_stage(ui, base, id, draft, urls)?;
    let (view, size) = if surface == Surface::History {
        play_history(ready.preview, Arc::new(designed_history(ui)));
        (
            history::history_window(ready.preview).into_any(),
            ready.history,
        )
    } else if surface == Surface::Input {
        ready
            .preview
            .draft
            .set(Arc::new(nanatype_markup::parse(&designed_display(ui)).doc));
        let [w, h] = ready.display;
        (
            widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(w))
                    .height(LengthSpec::Px(h)),
            )
            .key("skin-preview-input")
            .children(dialogue::dialogue_box(
                ready.preview,
                dialogue::Source::Draft,
                Some(super::editor_view::preview_editor(ready.preview)),
            ))
            .into_any(),
            ready.display,
        )
    } else {
        (
            display_view(&ready, ui, &designed_display(ui)),
            ready.display,
        )
    };
    Some((view, size))
}

/// Display and history from one prepared skin, on one private stage.
fn sample_both(
    ui: Stage,
    base: &Skin,
    id: &str,
    draft: &Draft,
    urls: Arc<HashMap<String, String>>,
) -> Option<(AnyView, AnyView, [f32; 2], [f32; 2])> {
    let ready = sample_stage(ui, base, id, draft, urls)?;
    play_history(ready.preview, Arc::new(designed_history(ui)));
    let display = display_view(&ready, ui, &designed_display(ui));
    let history = history::history_window(ready.preview).into_any();
    Some((display, history, ready.display, ready.history))
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

fn size_caption([w, h]: [f32; 2], scale: f32) -> String {
    format!("{} × {} px · {:.0}%", w.round(), h.round(), scale * 100.0)
}

/// The stage around `content`: the checkerboard and, under it, `caption`.
fn framed(dark: bool, content: AnyView, caption: String) -> AnyView {
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
            hint(caption),
        ))
        .into_any()
}

/// The stage while its [`StageAssets`] are prepared: the frame at the size
/// the sample will take, so nothing moves when it lands.
pub fn stage_placeholder(
    ui: Stage,
    skin: &Skin,
    surface: Surface,
    width: Signal<f32>,
    key: &'static str,
) -> AnyView {
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
            .key(key)
            .children(hint(ui.now_t("skin-preview-loading")));
            framed(*dark, content.into_any(), size_caption(size, scale))
        },
    )
    .into_any()
}

/// The scaled canvas under a stage keyed `box_key`.
fn canvas_key(box_key: &'static str) -> &'static str {
    match box_key {
        "skin-preview-history-box" => "skin-preview-history-canvas",
        "skin-preview-input-box" => "skin-preview-input-canvas",
        _ => "skin-preview-canvas",
    }
}

/// An isolated, scaled sample of the prepared skin through the real dialogue
/// and history renderers. Nothing touches production state.
///
/// `key` names the stage. A page that shows the display and the history
/// together gives the history its own key; a single stage keeps
/// `skin-preview-box`.
pub fn stage_view(
    ui: Stage,
    assets: Arc<StageAssets>,
    urls: Arc<std::collections::HashMap<String, String>>,
    surface: Surface,
    width: Signal<f32>,
    key: &'static str,
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
            .key(key)
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
                .key(canvas_key(key))
                .children(view),
            );
            framed(*dark, stage.into_any(), size_caption([w, h], scale))
        },
    )
    .into_any()
}

fn placed(key: &'static str, size: [f32; 2], top: f32, view: AnyView) -> AnyView {
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Px(size[0]))
            .height(LengthSpec::Px(size[1]))
            .with_layout(move |l| {
                l.position = PositionSpec::Absolute;
                l.offset_left = Some(LengthSpec::Px(0.0));
                l.offset_top = Some(LengthSpec::Px(top));
                l.overflow_x = OverflowSpec::Hidden;
                l.overflow_y = OverflowSpec::Hidden;
                l.pointer_events = Some(PointerEventsSpec::None);
            }),
    )
    .key(key)
    .children(view)
    .into_any()
}

/// No fill, so the checkerboard stays visible. The accent border marks the
/// window whose fields are open.
fn pick_style(height: f32) -> nana_ui::runtime::NodeStyle {
    let mut style = ListItem::new("")
        .style
        .outline(SemanticColorRole::BorderSoft, 1.0)
        .radius(RadiusTier::Sm);
    style.control_padding_x = None;
    style.control_height = None;
    let layout = Arc::make_mut(&mut style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.height = Some(LengthSpec::Px(height));
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.min_height = Some(LengthSpec::Px(height));
    layout.max_height = Some(LengthSpec::Px(height));
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    for paint in [
        &mut style.interaction.hovered,
        &mut style.interaction.pressed,
        &mut style.interaction.selected,
        &mut style.interaction.selected_hovered,
        &mut style.interaction.selected_pressed,
    ] {
        paint.background = None;
    }
    style.interaction.selected.border = Some(SemanticColorRole::Accent);
    style.interaction.selected_hovered.border = Some(SemanticColorRole::Accent);
    style.interaction.selected_pressed.border = Some(SemanticColorRole::Accent);
    style
}

/// A transparent button over one window. The window ignores the pointer.
fn surface_hit(ui: Stage, history: bool, selected: Signal<bool>, height: f32) -> AnyView {
    let name = if history {
        "windows-history"
    } else {
        "windows-display"
    };
    let pick = if history {
        "skin-preview-pick-history"
    } else {
        "skin-preview-pick-display"
    };
    widget(
        ListItem::new(ui.now_t(name))
            .role(ListItemRole::Button)
            .style(pick_style(height))
            .auto_height(true),
    )
    .key(pick)
    .bind(move |item: &mut ListItem| {
        item.selected = selected.get() == history;
    })
    .on(move |_: &Activate| {
        selected.set_if_changed(history);
    })
    .content(widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Px(height)),
    ))
    .into_any()
}

fn pair_caption(display: [f32; 2], history: [f32; 2], scale: f32) -> String {
    format!(
        "{} × {} px · {} × {} px · {:.0}%",
        display[0].round(),
        display[1].round(),
        history[0].round(),
        history[1].round(),
        scale * 100.0
    )
}

fn pane_size(ui: Stage, skin: &Skin, history: bool) -> [f32; 2] {
    draft(ui, &skin.meta.id, Some(skin.dialogue.overscan))
        .profile(skin)
        .ok()
        .and_then(|p| {
            if history {
                p.history.size
            } else {
                p.display.size
            }
        })
        .unwrap_or_else(|| default_size(skin, history))
}

/// Both windows on one checkerboard, from one prepared skin, at one scale.
/// Clicking a window selects it through `history_selected`.
pub fn stage_pair(
    ui: Stage,
    assets: Arc<StageAssets>,
    urls: Arc<HashMap<String, String>>,
    width: Signal<f32>,
    history_selected: Signal<bool>,
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
            assets.pick_family(&draft.display.font);
            assets.pick_family(&draft.history.font);
            let Some((display, history_view, display_size, history_size)) =
                sample_both(ui, &assets.base, &assets.base.meta.id, draft, urls.clone())
            else {
                return hint(ui.now_t("skin-edit-invalid"));
            };
            let span = pair_extent(display_size, history_size);
            let scale = fit(*available, span);
            let canvas = widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(span[0]))
                    .height(LengthSpec::Px(span[1]))
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
            .children((
                placed("skin-preview-display", display_size, 0.0, display),
                placed(
                    "skin-preview-history",
                    history_size,
                    display_size[1] + PAIR_GAP,
                    history_view,
                ),
            ));
            let stage = widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(span[0] * scale))
                    .height(LengthSpec::Px(span[1] * scale))
                    .with_layout(|l| {
                        l.position = PositionSpec::Relative;
                        l.overflow_x = OverflowSpec::Hidden;
                        l.overflow_y = OverflowSpec::Hidden;
                    }),
            )
            .key("skin-preview-box")
            .children((
                canvas,
                widget(
                    Stack::column(0.0)
                        .width(LengthSpec::Px(span[0] * scale))
                        .height(LengthSpec::Px(span[1] * scale))
                        .with_layout(|l| {
                            l.position = PositionSpec::Absolute;
                            l.offset_left = Some(LengthSpec::Px(0.0));
                            l.offset_top = Some(LengthSpec::Px(0.0));
                        }),
                )
                .children((
                    surface_hit(ui, false, history_selected, display_size[1] * scale),
                    widget(
                        Stack::column(0.0)
                            .height(LengthSpec::Px(PAIR_GAP * scale))
                            .with_layout(|l| {
                                l.pointer_events = Some(PointerEventsSpec::None);
                            }),
                    ),
                    surface_hit(ui, true, history_selected, history_size[1] * scale),
                )),
            ));
            framed(
                *dark,
                stage.into_any(),
                pair_caption(display_size, history_size, scale),
            )
        },
    )
    .into_any()
}

/// [`stage_pair`] at the size both windows will take, before the skin is prepared.
pub fn stage_pair_placeholder(ui: Stage, skin: &Skin, width: Signal<f32>) -> AnyView {
    let display = pane_size(ui, skin, false);
    let history = pane_size(ui, skin, true);
    let span = pair_extent(display, history);
    dynamic(
        move || (width.get(), ui.chrome_dark.get()),
        move |(available, dark)| {
            let scale = fit(*available, span);
            let content = widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(span[0] * scale))
                    .height(LengthSpec::Px(span[1] * scale))
                    .align(AlignSpec::Center)
                    .justify(nana_ui_core::JustifySpec::Center),
            )
            .key("skin-preview-loading")
            .children(hint(ui.now_t("skin-preview-loading")));
            framed(
                *dark,
                content.into_any(),
                pair_caption(display, history, scale),
            )
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
    (
        stage_view(ui, assets, urls, surface, width, "skin-preview-box"),
        base,
    )
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

    #[test]
    fn history_sample_reveals_one_message_at_a_time_and_then_repeats() {
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(Skin::default()),
            nanatype_i18n::Locale::ZhCn,
        );
        let script = designed_history(ui);
        let n = script.len() as u64;
        assert!(n > 4, "the sample covers more than a few lines");
        assert!(script.iter().any(|r| r.item.kind == SourceKind::Typewriter));
        assert!(script.iter().any(|r| r.item.kind == SourceKind::Chat));
        assert!(script.iter().any(|r| r.item.kind == SourceKind::Event));
        assert!(script.iter().any(|r| r.item.reply.is_some()));
        assert_eq!(playback_rows(&script, 0).len(), 1);
        assert_eq!(playback_rows(&script, 1).len(), 2);
        assert_eq!(playback_rows(&script, n - 1).len(), n as usize);
        assert_eq!(playback_rows(&script, n).len(), n as usize);
        let again = playback_rows(&script, n + 1);
        assert_eq!(again.len(), 1);
        assert_ne!(again[0].key, playback_rows(&script, 0)[0].key);
    }
}
