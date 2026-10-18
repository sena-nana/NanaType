//! Declarative dialogue layout. Resolution never changes text, history or playback.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogueLayout {
    pub padding: Option<[f32; 4]>,
    #[serde(rename = "box")]
    pub box_: Option<BoxSpec>,
    pub nameplate: Option<NameplateSpec>,
    pub portrait: Option<PortraitSpec>,
    #[serde(rename = "continue")]
    pub continue_: Option<ContinueSpec>,
    pub decor: Option<Vec<DecorSpec>>,
    pub shader_effects: Option<Vec<ShaderEffect>>,
}
impl DialogueLayout {
    pub(crate) fn apply_to(&self, d: &mut Dialogue) {
        macro_rules! set { ($($field:ident),*) => { $(if let Some(v) = &self.$field { d.$field = v.clone(); })* }; }
        set!(
            padding,
            box_,
            nameplate,
            portrait,
            continue_,
            decor,
            shader_effects
        );
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DialogueLayoutRule {
    pub max_width: f32,
    pub max_height: f32,
    /// None matches either orientation. Both size conditions must match.
    pub portrait: Option<bool>,
    pub layout: DialogueLayout,
}
impl Default for DialogueLayoutRule {
    fn default() -> Self {
        Self {
            max_width: 8192.0,
            max_height: 8192.0,
            portrait: None,
            layout: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResponsiveDialogue {
    pub landscape: DialogueLayout,
    pub portrait: DialogueLayout,
    /// Applied in declaration order after the orientation variant.
    pub rules: Vec<DialogueLayoutRule>,
}
impl ResponsiveDialogue {
    pub fn layouts_mut(&mut self) -> impl Iterator<Item = &mut DialogueLayout> {
        [&mut self.landscape, &mut self.portrait]
            .into_iter()
            .chain(self.rules.iter_mut().map(|r| &mut r.layout))
    }
    pub fn layouts(&self) -> impl Iterator<Item = &DialogueLayout> {
        [&self.landscape, &self.portrait]
            .into_iter()
            .chain(self.rules.iter().map(|r| &r.layout))
    }
}

pub fn dialogue_is_vertical(skin: &Skin, [w, h]: [f32; 2]) -> bool {
    skin.dialogue.responsive.is_some() && w > 0.0 && h > w
}

/// Size is the actual client area, before the settings preview's paint transform.
pub fn responsive_dialogue(skin: &Skin, [w, h]: [f32; 2]) -> Skin {
    let mut result = skin.clone();
    let Some(spec) = &skin.dialogue.responsive else {
        return result;
    };
    let vertical = dialogue_is_vertical(skin, [w, h]);
    let d = &mut result.dialogue;
    if vertical {
        spec.portrait.apply_to(d);
    } else {
        spec.landscape.apply_to(d);
    }
    for rule in &spec.rules {
        if w <= rule.max_width
            && h <= rule.max_height
            && rule.portrait.is_none_or(|v| v == vertical)
        {
            rule.layout.apply_to(d);
        }
    }
    // Nine-slice corners shrink uniformly only when their opposing cuts cannot fit.
    if let Some(frame) = &mut d.box_.frame {
        let [t, r, b, l] = frame.slice.map(|v| v as f32);
        let scale = (w / (r + l).max(1.0))
            .min(h / (t + b).max(1.0))
            .min(frame.scale);
        frame.scale = scale.max(0.001);
    }
    d.decor
        .retain(|v| v.min_window.is_none_or(|[mw, mh]| w >= mw && h >= mh));
    for dec in &mut d.decor {
        let fitted_inset = dec
            .inset
            .filter(|_| dec.aspect_ratio.is_some() || dec.max_fraction.is_some());
        let [mut dw, mut dh] = std::array::from_fn(|i| match dec.size[i] {
            Len::Px(v) => v,
            Len::Percent(v) => v * 0.01 * [w, h][i],
        });
        if let Some([t, r, b, l]) = fitted_inset {
            dw = (w - l - r).max(0.0);
            dh = (h - t - b).max(0.0);
        }
        if let Some(ratio) = dec.aspect_ratio {
            if dw / dh.max(0.001) > ratio {
                dw = dh * ratio;
            } else {
                dh = dw / ratio;
            }
        }
        if let Some([fw, fh]) = dec.max_fraction {
            let factor = (w * fw / dw.max(0.001))
                .min(h * fh / dh.max(0.001))
                .min(1.0);
            dw *= factor;
            dh *= factor;
        }
        dec.size = [Len::Px(dw), Len::Px(dh)];
        if let Some([t, r, b, l]) = fitted_inset {
            dec.anchor = Anchor::TopLeft;
            dec.offset = [l + ((w - l - r) - dw) * 0.5, t + ((h - t - b) - dh) * 0.5];
            dec.inset = None;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_builtin_has_independent_portrait_and_compact_compositions() {
        let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        assert!(registry.entries().len() >= 25);
        for entry in registry.entries() {
            let loaded = registry.load(&entry.id).unwrap();
            let skin = &loaded.skin;
            assert!(skin.dialogue.responsive.is_some(), "{}", entry.id);
            for size in [
                [920., 220.],
                [300., 1200.],
                [800., 2400.],
                [160., 96.],
                [8192., 8192.],
                [1200., 120.],
            ] {
                let resolved = responsive_dialogue(skin, size);
                assert_eq!(resolved.history, skin.history);
                assert_eq!(resolved.dialogue.text, skin.dialogue.text);
                if size == [160., 96.] || size == [1200., 120.] {
                    assert_eq!(
                        resolved.dialogue.box_.margin, [0.; 4],
                        "compact outer margins consume the text area: {}",
                        entry.id
                    );
                }
                if size[1] > size[0] {
                    assert!(
                        resolved
                            .dialogue
                            .box_
                            .frame
                            .as_ref()
                            .is_none_or(|f| !f.fill),
                        "portrait stretched plate: {}",
                        entry.id
                    );
                    assert_eq!(resolved.dialogue.nameplate.anchor, Anchor::TopRight);
                }
                for d in &resolved.dialogue.decor {
                    if let Some(ratio) = d.aspect_ratio {
                        let [Len::Px(w), Len::Px(h)] = d.size else {
                            panic!("unresolved ornament")
                        };
                        assert!((w / h.max(0.001) - ratio).abs() < 0.001, "{}", entry.id);
                    }
                }
            }
            assert!(!dialogue_is_vertical(skin, [400., 400.]));
            assert!(dialogue_is_vertical(skin, [400., 401.]));
        }
    }
    #[test]
    fn portrait_and_narrow_keep_authored_prints_without_inventing_replacement_motifs() {
        let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        for entry in registry.entries() {
            let skin = registry.load(&entry.id).unwrap().skin;
            for size in [[300., 1200.], [300., 240.], [160., 96.]] {
                let resolved = responsive_dialogue(&skin, size);
                if skin.dialogue.box_.frame.is_some() {
                    assert!(
                        resolved.dialogue.shader_effects.iter().any(|e| {
                            e.target == ShaderTarget::Artwork
                                && resolved.shaders[&e.shader].static_fallback
                                && resolved.shader_programs.contains_key(&e.shader)
                        }),
                        "{} {size:?}: authored material was replaced by a plain surface",
                        entry.id
                    );
                }
                for decor in &resolved.dialogue.decor {
                    if let Some(motif) = decor.motif {
                        assert!(
                            skin.dialogue.decor.iter().any(|d| d.motif == Some(motif)),
                            "{} {size:?}: invented generic replacement motif",
                            entry.id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn legacy_skins_are_unchanged_and_layout_rules_are_ordered() {
        let mut skin = Skin::default();
        assert_eq!(responsive_dialogue(&skin, [160., 1200.]), skin);
        assert!(!dialogue_is_vertical(&skin, [160., 1200.]));
        skin.dialogue.responsive = Some(ResponsiveDialogue {
            portrait: DialogueLayout {
                padding: Some([24.; 4]),
                ..Default::default()
            },
            rules: vec![DialogueLayoutRule {
                max_width: 200.,
                layout: DialogueLayout {
                    padding: Some([8.; 4]),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        });
        assert_eq!(
            responsive_dialogue(&skin, [300., 1200.]).dialogue.padding,
            [24.; 4]
        );
        assert_eq!(
            responsive_dialogue(&skin, [160., 1200.]).dialogue.padding,
            [8.; 4]
        );
    }
}
