struct FxParams {
    geometry: vec4<f32>, size_phase: vec4<f32>, events: vec4<f32>,
    base: vec4<f32>, accent: vec4<f32>, border: vec4<f32>, text: vec4<f32>,
    params0: vec4<f32>, params1: vec4<f32>, extent: vec4<f32>, rotation: vec4<f32>,
};
@group(0) @binding(0) var<uniform> fx_p: FxParams;
@group(0) @binding(1) var fx_mask: texture_2d<f32>;
struct FxVertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn fx_vertex(@builtin(vertex_index) i: u32) -> FxVertex {
    let points = array<vec2<f32>, 6>(vec2(0.,0.), vec2(1.,0.), vec2(0.,1.), vec2(0.,1.), vec2(1.,0.), vec2(1.,1.));
    let uv = points[i];
    let pos = (fx_p.geometry.zw + uv * fx_p.extent.xy) / fx_p.geometry.xy;
    return FxVertex(vec4(pos.x * 2. - 1., 1. - pos.y * 2., 0., 1.), uv);
}
@fragment fn fx_fragment(v: FxVertex) -> @location(0) vec4<f32> {
    let p = (v.uv - vec2(0.5)) * fx_p.extent.xy;
    let uv = vec2(p.x * fx_p.rotation.x + p.y * fx_p.rotation.y,
        -p.x * fx_p.rotation.y + p.y * fx_p.rotation.x) / fx_p.rotation.zw + vec2(0.5);
    let inside = step(0.,uv.x) * step(uv.x,1.) * step(0.,uv.y) * step(uv.y,1.);
    let input = EffectInput(uv, fx_p.size_phase.xy, fx_p.size_phase.z, fx_p.size_phase.w,
        fx_p.events.x, fx_p.events.y, fx_p.events.z, fx_p.events.w,
        fx_p.base, fx_p.accent, fx_p.border, fx_p.text, fx_p.params0, fx_p.params1);
    let color = clamp(effect(input), vec4(0.), vec4(1.));
    let alpha = color.a * textureSampleLevel(fx_mask, fx_sampler, uv, 0.).a * fx_p.extent.z * inside;
    return vec4(color.rgb * alpha, alpha);
}
