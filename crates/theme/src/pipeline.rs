//! Declarative host processing. Registered processors turn declared data and
//! explicit render context into material textures and uniforms; there are no
//! expressions, dynamic parsers or scripts, and nothing here starts Lua.
use crate::{DataFormat, LoadedData, Severity, Skin, SkinDiagnostic, SkinFiles};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub const MAX_PIPELINES: usize = 8;
/// Longest chain of processors feeding one material.
pub const MAX_PIPELINE_DEPTH: usize = 4;
/// `vec4` slots shared by static params and pipeline uniforms.
pub const MAX_MATERIAL_VEC4: usize = 4;
/// Largest texture side a processor may produce.
pub const MAX_PIPELINE_TEXTURE_SIDE: u32 = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortType {
    Data { format: DataFormat, version: u32 },
    Texture,
    Vec4,
}

#[derive(Debug)]
pub struct ParamSig {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
}

/// A host processor's typed contract. The host implements each one natively.
#[derive(Debug, Clone, Copy)]
pub struct ProcessorSignature {
    pub name: &'static str,
    pub version: u32,
    pub inputs: &'static [(&'static str, PortType)],
    pub outputs: &'static [(&'static str, PortType)],
    pub params: &'static [ParamSig],
    /// Reads the target's text layout, which only the dialogue entrance has.
    pub text_layout: bool,
}
impl ProcessorSignature {
    fn output(&self, name: &str) -> Option<(&'static str, PortType)> {
        self.outputs.iter().copied().find(|(n, _)| *n == name)
    }
}

pub const PROCESSORS: &[ProcessorSignature] = &[ProcessorSignature {
    // Per-texel writing time from stroke medians, laid over the shaped text.
    name: "text.stroke_time_map",
    version: 1,
    inputs: &[(
        "strokes",
        PortType::Data {
            format: DataFormat::Nthz,
            version: 1,
        },
    )],
    outputs: &[("time", PortType::Texture), ("extent", PortType::Vec4)],
    params: &[],
    text_layout: true,
}];

pub fn processor(name: &str, version: u32) -> Option<&'static ProcessorSignature> {
    PROCESSORS
        .iter()
        .find(|p| p.name == name && p.version == version)
}

/// `[pipelines.<name>]` in `skin.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineSpec {
    pub processor: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, PortSource>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, f32>,
}

/// Either `{ data = "<reference>" }` or `{ pipeline = "<name>", output = "<port>" }`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineOutput {
    pub pipeline: String,
    pub output: String,
}

/// A material texture slot: an image file or a pipeline output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TextureSource {
    Image(String),
    Pipeline(PipelineOutput),
}
impl From<&str> for TextureSource {
    fn from(path: &str) -> Self {
        Self::Image(path.into())
    }
}
impl TextureSource {
    pub fn image(&self) -> Option<&str> {
        match self {
            Self::Image(path) => Some(path),
            Self::Pipeline(_) => None,
        }
    }
}

/// The resolved processors feeding one material, in evaluation order.
#[derive(Debug)]
pub struct MaterialBinding {
    /// Content key: processors, params, data hashes and wiring.
    pub key: [u8; 32],
    pub nodes: Vec<BoundNode>,
    /// Per texture slot: the producing node and output, or `None` for an image.
    pub textures: Vec<Option<(usize, &'static str)>>,
    pub uniforms: Vec<(usize, &'static str)>,
    pub text_layout: bool,
}
impl PartialEq for MaterialBinding {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

#[derive(Debug)]
pub struct BoundNode {
    pub processor: &'static ProcessorSignature,
    pub params: BTreeMap<&'static str, f32>,
    pub inputs: BTreeMap<&'static str, BoundInput>,
}

#[derive(Debug)]
pub enum BoundInput {
    Data(Arc<LoadedData>),
    Node { node: usize, output: &'static str },
}

fn valid_name(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

/// The pipeline an output reference names, when it is well-formed.
fn source_pipeline(source: &PortSource) -> Option<&str> {
    source.pipeline.as_deref()
}

impl Skin {
    /// Pipelines a material reads, directly or through other pipelines.
    fn material_pipelines(&self, material: &str) -> BTreeSet<&str> {
        let Some(spec) = self.materials.get(material) else {
            return BTreeSet::new();
        };
        let mut seen = BTreeSet::new();
        let mut stack: Vec<&str> = spec
            .textures
            .iter()
            .filter_map(|t| match t {
                TextureSource::Pipeline(p) => Some(p.pipeline.as_str()),
                TextureSource::Image(_) => None,
            })
            .chain(spec.uniforms.iter().map(|u| u.pipeline.as_str()))
            .collect();
        while let Some(name) = stack.pop() {
            if seen.len() > MAX_PIPELINES || !seen.insert(name) {
                continue;
            }
            if let Some(p) = self.pipelines.get(name) {
                stack.extend(p.inputs.values().filter_map(source_pipeline));
            }
        }
        seen
    }

    /// Whether a material depends on the target's text layout. Static, so
    /// unprepared catalogue skins answer it too.
    pub fn material_needs_text_layout(&self, material: &str) -> bool {
        self.material_pipelines(material).iter().any(|name| {
            self.pipelines
                .get(*name)
                .and_then(|p| processor(&p.processor, p.version))
                .is_some_and(|s| s.text_layout)
        })
    }

    /// The dialogue entrance is drawn from a layout-dependent pipeline.
    pub fn entrance_needs_text_layout(&self) -> bool {
        self.dialogue
            .reveal
            .entrance
            .material
            .as_deref()
            .is_some_and(|m| self.material_needs_text_layout(m))
    }
}

/// Check the declarations against `signatures` without loading data.
pub(crate) fn validate(skin: &Skin, signatures: &[ProcessorSignature]) -> Vec<SkinDiagnostic> {
    let mut out = Vec::new();
    let mut error = |field: String, message: String| {
        out.push(SkinDiagnostic {
            severity: Severity::Error,
            field,
            message,
        })
    };
    let lookup = |name: &str, version: u32| {
        signatures
            .iter()
            .find(|p| p.name == name && p.version == version)
    };
    if skin.pipelines.len() > MAX_PIPELINES {
        error(
            "pipelines".into(),
            format!("at most {MAX_PIPELINES} pipelines"),
        );
        return out;
    }
    let output_type = |reference: &PipelineOutput| -> Result<PortType, String> {
        let p = skin
            .pipelines
            .get(&reference.pipeline)
            .ok_or_else(|| format!("unknown pipeline `{}`", reference.pipeline))?;
        let signature = lookup(&p.processor, p.version)
            .ok_or_else(|| format!("pipeline `{}` is not usable", reference.pipeline))?;
        signature
            .output(&reference.output)
            .map(|(_, t)| t)
            .ok_or_else(|| {
                format!(
                    "`{}` has no output `{}`",
                    reference.pipeline, reference.output
                )
            })
    };
    for (name, spec) in &skin.pipelines {
        let field = format!("pipelines.{name}");
        if !valid_name(name) {
            error(field, "invalid pipeline name".into());
            continue;
        }
        let Some(signature) = lookup(&spec.processor, spec.version) else {
            error(
                format!("{field}.processor"),
                format!(
                    "unregistered processor `{}` version {}",
                    spec.processor, spec.version
                ),
            );
            continue;
        };
        for (input, expected) in signature.inputs {
            let Some(source) = spec.inputs.get(*input) else {
                error(
                    format!("{field}.inputs.{input}"),
                    "required input is not bound".into(),
                );
                continue;
            };
            let found = match (&source.data, &source.pipeline, &source.output) {
                (Some(reference), None, None) => crate::data::split_reference(reference)
                    .map(|_| None)
                    .map_err(|e| e.to_string()),
                (None, Some(pipeline), Some(output)) => output_type(&PipelineOutput {
                    pipeline: pipeline.clone(),
                    output: output.clone(),
                })
                .map(Some),
                _ => Err("bind either `data` or `pipeline` with `output`".into()),
            };
            match (found, expected) {
                (Err(message), _) => error(format!("{field}.inputs.{input}"), message),
                (Ok(None), PortType::Data { .. }) => {}
                (Ok(Some(t)), expected) if t == *expected => {}
                (Ok(_), expected) => error(
                    format!("{field}.inputs.{input}"),
                    format!("expects {expected:?}"),
                ),
            }
        }
        for input in spec.inputs.keys() {
            if !signature.inputs.iter().any(|(n, _)| n == input) {
                error(
                    format!("{field}.inputs.{input}"),
                    format!("`{}` declares no input `{input}`", signature.name),
                );
            }
        }
        for (param, value) in &spec.params {
            match signature.params.iter().find(|p| p.name == param) {
                None => error(
                    format!("{field}.params.{param}"),
                    "undeclared parameter".into(),
                ),
                Some(sig) if !value.is_finite() || !(sig.min..=sig.max).contains(value) => error(
                    format!("{field}.params.{param}"),
                    format!("must be within {}..={}", sig.min, sig.max),
                ),
                Some(_) => {}
            }
        }
    }
    // Cycles and depth: longest path through pipeline-to-pipeline edges.
    fn depth<'a>(
        skin: &'a Skin,
        name: &'a str,
        stack: &mut Vec<&'a str>,
        memo: &mut BTreeMap<&'a str, usize>,
    ) -> Result<usize, String> {
        if let Some(d) = memo.get(name) {
            return Ok(*d);
        }
        if stack.contains(&name) {
            stack.push(name);
            return Err(format!("pipeline cycle: {}", stack.join(" → ")));
        }
        stack.push(name);
        let mut d = 1;
        if let Some(p) = skin.pipelines.get(name) {
            for next in p.inputs.values().filter_map(source_pipeline) {
                if skin.pipelines.contains_key(next) {
                    d = d.max(1 + depth(skin, next, stack, memo)?);
                }
            }
        }
        stack.pop();
        memo.insert(name, d);
        Ok(d)
    }
    let mut memo = BTreeMap::new();
    for name in skin.pipelines.keys() {
        match depth(skin, name, &mut Vec::new(), &mut memo) {
            Err(message) => {
                error(format!("pipelines.{name}"), message);
                return out;
            }
            Ok(d) if d > MAX_PIPELINE_DEPTH => {
                error(
                    format!("pipelines.{name}"),
                    format!("pipelines nest at most {MAX_PIPELINE_DEPTH} deep"),
                );
                return out;
            }
            Ok(_) => {}
        }
    }
    let mut used = BTreeSet::new();
    for (name, spec) in &skin.materials {
        let field = format!("materials.{name}");
        for (slot, texture) in spec.textures.iter().enumerate() {
            if let TextureSource::Pipeline(reference) = texture {
                match output_type(reference) {
                    Ok(PortType::Texture) => {}
                    Ok(t) => error(
                        format!("{field}.textures"),
                        format!("slot {slot} needs a texture, not {t:?}"),
                    ),
                    Err(message) => error(format!("{field}.textures"), message),
                }
            }
        }
        for reference in &spec.uniforms {
            match output_type(reference) {
                Ok(PortType::Vec4) => {}
                Ok(t) => error(
                    format!("{field}.uniforms"),
                    format!("uniforms need vec4 outputs, not {t:?}"),
                ),
                Err(message) => error(format!("{field}.uniforms"), message),
            }
        }
        if spec.params.len().div_ceil(4) + spec.uniforms.len() > MAX_MATERIAL_VEC4 {
            error(
                field,
                format!("params and uniforms share {MAX_MATERIAL_VEC4} vec4 slots"),
            );
        }
        used.extend(skin.material_pipelines(name));
    }
    for (field, material) in skin.material_references() {
        if field != "dialogue.reveal.entrance.material" && skin.material_needs_text_layout(material)
        {
            error(
                field.into(),
                format!(
                    "`{material}` needs text layout, which only the dialogue entrance provides"
                ),
            );
        }
    }
    for name in skin.pipelines.keys() {
        if !used.contains(name.as_str()) {
            out.push(SkinDiagnostic {
                severity: Severity::Warning,
                field: format!("pipelines.{name}"),
                message: "no material uses this pipeline".into(),
            });
        }
    }
    out
}

/// Load declared data for every material fed by pipelines. Imports pass
/// `full` to validate block-compressed data completely.
pub(crate) fn bind(
    skin: &mut Skin,
    files: &dyn SkinFiles,
    full: bool,
    out: &mut Vec<SkinDiagnostic>,
) {
    let mut bindings = BTreeMap::new();
    let view: &Skin = skin;
    for (material, spec) in &view.materials {
        let skin = view;
        let pipelines = skin.material_pipelines(material);
        if pipelines.is_empty() {
            continue;
        }
        let result = (|| -> Result<MaterialBinding, (String, String)> {
            let mut nodes: Vec<BoundNode> = Vec::new();
            let mut index: BTreeMap<String, usize> = BTreeMap::new();
            let mut key = blake3::Hasher::new();
            // Dependencies first; `validate` already rejected cycles.
            fn visit(
                skin: &Skin,
                name: &str,
                files: &dyn SkinFiles,
                full: bool,
                nodes: &mut Vec<BoundNode>,
                index: &mut BTreeMap<String, usize>,
                key: &mut blake3::Hasher,
            ) -> Result<usize, (String, String)> {
                if let Some(i) = index.get(name) {
                    return Ok(*i);
                }
                let field = format!("pipelines.{name}");
                let spec = skin
                    .pipelines
                    .get(name)
                    .ok_or_else(|| (field.clone(), "unknown pipeline".to_owned()))?;
                let signature = processor(&spec.processor, spec.version)
                    .ok_or_else(|| (field.clone(), "unregistered processor".to_owned()))?;
                key.update(signature.name.as_bytes());
                key.update(&signature.version.to_le_bytes());
                let mut inputs = BTreeMap::new();
                for (input, expected) in signature.inputs {
                    let source = &spec.inputs[*input];
                    let field = format!("{field}.inputs.{input}");
                    key.update(input.as_bytes());
                    let bound = if let Some(reference) = &source.data {
                        let (data, path) = crate::data::resolve_data(files, reference)
                            .map_err(|e| (field.clone(), e))?;
                        if let PortType::Data { format, version } = expected
                            && (data.format, data.format_version) != (*format, *version)
                        {
                            return Err((
                                field,
                                format!(
                                    "`{reference}` is {} v{}, expected {} v{version}",
                                    data.format.name(),
                                    data.format_version,
                                    format.name()
                                ),
                            ));
                        }
                        let loaded = crate::data::load_data(files, &data, &path, full)
                            .map_err(|e| (field.clone(), e))?;
                        key.update(&loaded.hash);
                        BoundInput::Data(loaded)
                    } else {
                        let pipeline = source.pipeline.as_deref().unwrap_or_default();
                        let node = visit(skin, pipeline, files, full, nodes, index, key)?;
                        let output = nodes[node]
                            .processor
                            .output(source.output.as_deref().unwrap_or_default())
                            .ok_or_else(|| (field.clone(), "unknown output".to_owned()))?
                            .0;
                        key.update(&(node as u64).to_le_bytes());
                        key.update(output.as_bytes());
                        BoundInput::Node { node, output }
                    };
                    inputs.insert(*input, bound);
                }
                let mut params = BTreeMap::new();
                for sig in signature.params {
                    if let Some(v) = spec.params.get(sig.name) {
                        key.update(&v.to_le_bytes());
                        params.insert(sig.name, *v);
                    }
                }
                nodes.push(BoundNode {
                    processor: signature,
                    params,
                    inputs,
                });
                index.insert(name.to_owned(), nodes.len() - 1);
                Ok(nodes.len() - 1)
            }
            let output = |reference: &PipelineOutput,
                          nodes: &mut Vec<BoundNode>,
                          index: &mut BTreeMap<String, usize>,
                          key: &mut blake3::Hasher|
             -> Result<(usize, &'static str), (String, String)> {
                let node = visit(skin, &reference.pipeline, files, full, nodes, index, key)?;
                let port = nodes[node]
                    .processor
                    .output(&reference.output)
                    .ok_or_else(|| (format!("materials.{material}"), "unknown output".to_owned()))?
                    .0;
                key.update(&(node as u64).to_le_bytes());
                key.update(port.as_bytes());
                Ok((node, port))
            };
            let mut textures = Vec::new();
            for texture in &spec.textures {
                textures.push(match texture {
                    TextureSource::Image(_) => None,
                    TextureSource::Pipeline(reference) => {
                        Some(output(reference, &mut nodes, &mut index, &mut key)?)
                    }
                });
            }
            let mut uniforms = Vec::new();
            for reference in &spec.uniforms {
                uniforms.push(output(reference, &mut nodes, &mut index, &mut key)?);
            }
            Ok(MaterialBinding {
                key: *key.finalize().as_bytes(),
                text_layout: nodes.iter().any(|n| n.processor.text_layout),
                nodes,
                textures,
                uniforms,
            })
        })();
        match result {
            Ok(binding) => {
                bindings.insert(material.clone(), Arc::new(binding));
            }
            Err((field, message)) => out.push(SkinDiagnostic {
                severity: Severity::Error,
                field,
                message,
            }),
        }
    }
    skin.material_bindings = bindings;
}

#[cfg(test)]
mod tests;
