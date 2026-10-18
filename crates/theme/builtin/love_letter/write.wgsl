// Skin-owned program; texture 0 is an immutable, layout-local stroke-time map.
// Alpha holds the threshold (linear). B marks characters with stroke data.
fn material(i: MaterialInput) -> vec4<f32> {
    let p = clamp(i.progress, 0., 1.);
    if p <= 0. { return vec4<f32>(1., 1., 1., 0.); }
    if p >= 1. { return vec4<f32>(1.); }
    let extent = max(material_param(0u).xy, vec2<f32>(1.));
    let offset = material_param(0u).zw;
    let sample = material_sample0((i.target_position + offset) / extent);
    let at = sample.a;
    let written = smoothstep(at - 0.018, at + 0.018, p);
    let opacity = select(smoothstep(0., 0.36, p), written, sample.b > 0.5);
    return vec4<f32>(1., 1., 1., opacity);
}
