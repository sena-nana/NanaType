struct Params {
    destination: vec2<f32>,
    origin: vec2<f32>,
    size: vec2<f32>,
    phase: f32,
    opacity: f32,
    color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var coverage: texture_2d<f32>;
@group(0) @binding(2) var mask_sampler: sampler;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> Vertex {
    let points = array<vec2<f32>, 6>(vec2(0.,0.), vec2(1.,0.), vec2(0.,1.), vec2(0.,1.), vec2(1.,0.), vec2(1.,1.));
    let uv = points[i];
    let pos = (p.origin + uv * p.size) / p.destination;
    var out: Vertex;
    out.position = vec4(pos.x * 2. - 1., 1. - pos.y * 2., 0., 1.);
    out.uv = uv;
    return out;
}
@fragment fn fragment_main(v: Vertex) -> @location(0) vec4<f32> {
    let center = -0.35 + p.phase * 1.7;
    let band = 1. - smoothstep(0., 0.15, abs(v.uv.x + (v.uv.y - 0.5) * 0.18 - center));
    let alpha = textureSample(coverage, mask_sampler, v.uv).a * band * p.opacity * p.color.a;
    // The host owns the final colour conversion. This is premultiplied linear scRGB.
    return vec4(p.color.rgb * alpha, alpha);
}
