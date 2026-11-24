use std::collections::BTreeMap;

use nanatype_model::{Overflow, ParamValue, Rgba8, valid_param_id};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{clamp_f32, clamp_f32_opt, finite_or, set_if};

pub const DEFAULT_SKIN: &str = "nanatype.halo";
/// Skin loop rates the settings offer; the first is the default.
pub const SKIN_FRAME_RATES: [u32; 2] = [30, 60];
/// Most params a skin can declare, and so most values one profile keeps.
pub const MAX_SKIN_PARAMS: usize = 64;
const MAX_PARAM_TEXT_BYTES: usize = 256;
/// The smallest and largest [`SkinOverrides::name_scale`] and
/// [`SkinOverrides::decor_scale`], and their slider's step.
pub const SKIN_PART_SCALE: (f64, f64, f64) = (0.25, 4.0, 0.05);

/// An output's longest side: no output is larger than 1080p, either way round.
pub const OUTPUT_MAX_LONG: u32 = 1920;
/// An output's shortest side.
pub const OUTPUT_MAX_SHORT: u32 = 1080;
/// The smallest side an output has.
pub const OUTPUT_MIN: u32 = 16;
/// Partner seats in a co-op room: everyone but the local person, in seat
/// order (the host, then guests as they joined).
pub const PARTNER_SEATS: usize = 7;

/// User tweaks applied on top of the active skin.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfacePersonalization {
    pub font_family: Option<String>,
    pub text_size: Option<f32>,
    pub line_height: Option<f32>,
    pub text_color: Option<Rgba8>,
    /// Native client size, including the skin's overscan.
    pub size: Option<[f32; 2]>,
}

impl SurfacePersonalization {
    pub fn valid(&self) -> bool {
        self.text_size
            .is_none_or(|v| v.is_finite() && (8.0..=200.0).contains(&v))
            && self
                .line_height
                .is_none_or(|v| v.is_finite() && (1.0..=3.0).contains(&v))
            && self.size.is_none_or(|[w, h]| {
                w.is_finite()
                    && h.is_finite()
                    && (160.0..=8192.0).contains(&w)
                    && (96.0..=8192.0).contains(&h)
            })
            && self
                .font_family
                .as_ref()
                .is_none_or(|v| !v.trim().is_empty() && v.len() <= 256)
    }

    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        clamp_f32_opt(&mut self.text_size, 28.0, 8.0, 200.0, &mut dirty);
        clamp_f32_opt(&mut self.line_height, 1.4, 1.0, 3.0, &mut dirty);
        if let Some([w, h]) = &mut self.size {
            clamp_f32(w, 760.0, 160.0, 8192.0, &mut dirty);
            clamp_f32(h, 200.0, 96.0, 8192.0, &mut dirty);
        }
        if let Some(v) = &self.font_family {
            let mut trimmed: String = v.trim().chars().take(256).collect();
            while trimmed.len() > 256 {
                trimmed.pop();
            }
            if trimmed.is_empty() {
                self.font_family = None;
                dirty = true;
            } else if *v != trimmed {
                self.font_family = Some(trimmed);
                dirty = true;
            }
        }
        dirty
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinPersonalization {
    pub display: SurfacePersonalization,
    pub history: SurfacePersonalization,
    /// Values for the skin's declared params that differ from its defaults.
    /// The skin's schema checks them when applied; a value it refuses falls
    /// back to the default, so this map never needs a migration.
    #[serde(
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "lenient_params"
    )]
    pub params: BTreeMap<String, ParamValue>,
}

fn param_entry_valid(id: &str, value: &ParamValue) -> bool {
    valid_param_id(id)
        && match value {
            ParamValue::Number(v) => v.is_finite(),
            ParamValue::Bool(_) => true,
            ParamValue::Text(t) => t.len() <= MAX_PARAM_TEXT_BYTES,
        }
}

/// Keep the well-formed entries and drop the rest instead of failing: a bad
/// param value must not make the whole settings file unreadable.
fn lenient_params<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, ParamValue>, D::Error> {
    let Value::Object(raw) = Value::deserialize(deserializer)? else {
        return Ok(BTreeMap::new());
    };
    Ok(raw
        .into_iter()
        .filter_map(|(id, v)| {
            let value = match v {
                Value::Bool(b) => ParamValue::Bool(b),
                Value::Number(n) => ParamValue::Number(n.as_f64()?),
                Value::String(s) => ParamValue::Text(s),
                _ => return None,
            };
            param_entry_valid(&id, &value).then_some((id, value))
        })
        .take(MAX_SKIN_PARAMS)
        .collect())
}

impl SkinPersonalization {
    pub fn valid(&self) -> bool {
        self.display.valid()
            && self.history.valid()
            && self.params.len() <= MAX_SKIN_PARAMS
            && self.params.iter().all(|(id, v)| param_entry_valid(id, v))
    }

    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = self.display.sanitize();
        dirty |= self.history.sanitize();
        let before = self.params.len();
        self.params.retain(|id, v| param_entry_valid(id, v));
        while self.params.len() > MAX_SKIN_PARAMS {
            self.params.pop_last();
        }
        dirty |= self.params.len() != before;
        dirty
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinOverrides {
    pub text_color: Option<Rgba8>,
    pub name_color: Option<Rgba8>,
    pub box_color: Option<Rgba8>,
    pub accent: Option<Rgba8>,
    pub text_size: Option<f32>,
    pub region: Option<[f32; 2]>,
    pub overflow: Option<Overflow>,
    /// In page mode, turn a full page on its own after this many ms.
    pub overflow_auto_ms: Option<u32>,
    /// `Some(false)` hides the dialogue name plate; `None` leaves it to the skin.
    pub name_visible: Option<bool>,
    /// The name plate's size as a multiple of the skin's ([`SKIN_PART_SCALE`]).
    pub name_scale: Option<f32>,
    /// The dialogue box's ornaments' size as a multiple of the skin's
    /// ([`SKIN_PART_SCALE`]).
    pub decor_scale: Option<f32>,
}

impl SkinOverrides {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkinSettings {
    pub advanced_effects: bool,
    /// Skin backgrounds move. Off, each is drawn once at rest and that frame
    /// is reused.
    pub motion: bool,
    /// Frames per second for the display window's skin animation: 30 or 60.
    /// Typing and other glyph motion are not held to it.
    pub frame_rate: u32,
    pub id: String,
    pub overrides: SkinOverrides,
    /// An entry replaces legacy body overrides; None fields mean skin defaults.
    pub personalizations: BTreeMap<String, SkinPersonalization>,
    /// Reload folder skins when their files change (for skin authors).
    pub hot_reload: bool,
}

impl Default for SkinSettings {
    fn default() -> Self {
        Self {
            advanced_effects: true,
            motion: true,
            frame_rate: SKIN_FRAME_RATES[0],
            id: DEFAULT_SKIN.into(),
            overrides: SkinOverrides::default(),
            personalizations: BTreeMap::new(),
            hot_reload: false,
        }
    }
}

impl SkinSettings {
    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        if !SKIN_FRAME_RATES.contains(&self.frame_rate) {
            self.frame_rate = SKIN_FRAME_RATES[0];
            dirty = true;
        }
        clamp_f32_opt(&mut self.overrides.text_size, 28.0, 8.0, 200.0, &mut dirty);
        for scale in [
            &mut self.overrides.name_scale,
            &mut self.overrides.decor_scale,
        ] {
            let next = scale
                .map(|v| {
                    finite_or(v, 1.0).clamp(SKIN_PART_SCALE.0 as f32, SKIN_PART_SCALE.1 as f32)
                })
                .filter(|v| (v - 1.0).abs() > 1e-3);
            set_if(scale, next, &mut dirty);
        }
        for profile in self.personalizations.values_mut() {
            dirty |= profile.sanitize();
        }
        if self.id.is_empty() {
            self.id = DEFAULT_SKIN.into();
            dirty = true;
        }
        dirty
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lock {
    /// Clicks pass through the whole window.
    Locked,
    /// Drag anywhere to move; use the wheel to scale the window.
    #[default]
    #[serde(alias = "edit_layout")]
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaptureBackground {
    #[default]
    Transparent,
    /// A solid key colour for capture tools without alpha.
    Chroma { color: Rgba8 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub visible: bool,
    pub lock: Lock,
    pub always_on_top: bool,
    pub background: CaptureBackground,
    /// Logical size override; `None` uses the skin's size.
    pub size: Option<[f32; 2]>,
    /// Write the dialogue top to bottom. The skin's portrait layout still
    /// follows the window's shape.
    pub vertical_text: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            visible: false,
            lock: Lock::Interactive,
            always_on_top: true,
            background: CaptureBackground::Transparent,
            size: None,
            vertical_text: false,
        }
    }
}

fn default_input_overlay() -> OverlaySettings {
    OverlaySettings {
        visible: true,
        ..OverlaySettings::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overlays {
    #[serde(default = "default_input_overlay")]
    pub input: OverlaySettings,
    pub display: OverlaySettings,
    pub peer_display: OverlaySettings,
    pub history: OverlaySettings,
    pub danmaku: OverlaySettings,
}

impl Default for Overlays {
    fn default() -> Self {
        Self {
            input: default_input_overlay(),
            display: OverlaySettings::default(),
            peer_display: OverlaySettings::default(),
            history: OverlaySettings::default(),
            danmaku: OverlaySettings::default(),
        }
    }
}

impl Overlays {
    /// Lock the display, partner, history and danmaku overlays (clicks pass
    /// through them), or unlock them all when the display is locked.
    pub fn toggle_lock(&mut self) {
        let lock = if self.display.lock == Lock::Locked {
            Lock::Interactive
        } else {
            Lock::Locked
        };
        for o in [
            &mut self.display,
            &mut self.peer_display,
            &mut self.history,
            &mut self.danmaku,
        ] {
            o.lock = lock;
        }
    }

    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        for o in [
            &mut self.input,
            &mut self.display,
            &mut self.peer_display,
            &mut self.history,
            &mut self.danmaku,
        ] {
            if let Some([w, h]) = o.size.as_mut() {
                clamp_f32(w, 760.0, 160.0, 8192.0, &mut dirty);
                clamp_f32(h, 200.0, 60.0, 8192.0, &mut dirty);
            }
        }
        dirty
    }
}

/// `width × height` scaled down, keeping its aspect, until it fits 1080p
/// either way round, in whole pixels of at least [`OUTPUT_MIN`]. `None` for
/// a size that is not a size (under half a pixel, not finite).
pub fn fit_output_size(width: f64, height: f64) -> Option<[u32; 2]> {
    if !(width.is_finite() && height.is_finite() && width >= 0.5 && height >= 0.5) {
        return None;
    }
    let (long, short) = (width.max(height), width.min(height));
    let scale = (f64::from(OUTPUT_MAX_LONG) / long)
        .min(f64::from(OUTPUT_MAX_SHORT) / short)
        .min(1.0);
    let side = |v: f64| ((v * scale).round() as u32).clamp(OUTPUT_MIN, OUTPUT_MAX_LONG);
    Some([side(width), side(height)])
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpoutOutput {
    pub enabled: bool,
    pub name: String,
    /// Fixed sender size; `None` follows the window. Also what is sent while
    /// the OBS source below cannot be read.
    pub size: Option<[u32; 2]>,
    /// Straight alpha for receivers without a premultiplied mode.
    pub straight_alpha: bool,
    /// The OBS source (in the program scene) whose bounding box sets the
    /// size while OBS is linked; empty for none.
    pub obs_source: String,
    /// The size that box gives now, already fitted to 1080p. Runtime only:
    /// never saved, and `None` whenever OBS cannot say.
    #[serde(skip)]
    pub obs_size: Option<[u32; 2]>,
}

impl SpoutOutput {
    /// The size to send: OBS's bounding box when known, else the fixed
    /// size; `None` follows the window.
    pub fn extent(&self) -> Option<[u32; 2]> {
        self.obs_size.or(self.size)
    }
}

/// The obs-websocket server (OBS › Tools › WebSocket Server Settings). Its
/// password is kept in the system's secret store, never here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObsLink {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
}

impl Default for ObsLink {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".into(),
            port: 4455,
        }
    }
}

/// Seat `index`'s default sender name, counted from one.
pub fn partner_output_name(index: usize) -> String {
    format!("NanaType Partner {}", index + 1)
}

/// Seat `index`'s sender as it starts out: off, under its own name.
pub fn partner_output(index: usize) -> SpoutOutput {
    SpoutOutput {
        enabled: false,
        name: partner_output_name(index),
        ..SpoutOutput::default()
    }
}

fn default_danmaku_output() -> SpoutOutput {
    SpoutOutput {
        enabled: false,
        name: "NanaType Danmaku".into(),
        ..SpoutOutput::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    pub display: SpoutOutput,
    /// One sender per partner seat, remembered by seat: whoever sits there
    /// goes out under that seat's name. Always [`PARTNER_SEATS`] long.
    pub partners: Vec<SpoutOutput>,
    pub history: SpoutOutput,
    #[serde(default = "default_danmaku_output")]
    pub danmaku: SpoutOutput,
    pub obs: ObsLink,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            display: SpoutOutput {
                enabled: false,
                name: "NanaType Display".into(),
                ..SpoutOutput::default()
            },
            partners: (0..PARTNER_SEATS).map(partner_output).collect(),
            history: SpoutOutput {
                enabled: false,
                name: "NanaType History".into(),
                ..SpoutOutput::default()
            },
            danmaku: default_danmaku_output(),
            obs: ObsLink::default(),
        }
    }
}

impl OutputSettings {
    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        if self.partners.len() > PARTNER_SEATS {
            self.partners.truncate(PARTNER_SEATS);
            dirty = true;
        }
        while self.partners.len() < PARTNER_SEATS {
            self.partners.push(partner_output(self.partners.len()));
            dirty = true;
        }
        for out in [&mut self.display, &mut self.history, &mut self.danmaku]
            .into_iter()
            .chain(self.partners.iter_mut())
        {
            super::trim_take(&mut out.name, 255, &mut dirty);
            let obs_trimmed: String = out.obs_source.chars().take(255).collect();
            if out.obs_source != obs_trimmed {
                out.obs_source = obs_trimmed;
                dirty = true;
            }
            if let Some([w, h]) = out.size {
                let next = fit_output_size(f64::from(w.max(1)), f64::from(h.max(1)));
                set_if(&mut out.size, next, &mut dirty);
            }
        }
        if self.danmaku.name.is_empty() {
            self.danmaku.name = default_danmaku_output().name;
            dirty = true;
        }
        super::trim_take(&mut self.obs.host, 255, &mut dirty);
        if self.obs.port == 0 {
            self.obs.port = ObsLink::default().port;
            dirty = true;
        }
        dirty
    }
}
