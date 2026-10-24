// Copies a skin program's rest frame, texel for texel, into the destination.
// The frame is premultiplied linear colour at full opacity.
struct Rest {
    // Destination size (xy) and the frame's top-left in it (zw), in pixels.
    dest: vec4<f32>,
    // Frame size (xy) and node opacity (z).
    frame: vec4<f32>,
};
@group(0) @binding(0) var<uniform> rest: Rest;
@group(0) @binding(1) var rest_frame: texture_2d<f32>;
@group(0) @binding(2) var rest_sampler: sampler;

@vertex fn rest_vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let points = array<vec2<f32>, 6>(vec2(0.,0.), vec2(1.,0.), vec2(0.,1.), vec2(0.,1.), vec2(1.,0.), vec2(1.,1.));
    let pos = (rest.dest.zw + points[i] * rest.frame.xy) / rest.dest.xy;
    return vec4(pos.x * 2. - 1., 1. - pos.y * 2., 0., 1.);
}
@fragment fn rest_fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(rest.frame.xy);
    let texel = clamp(vec2<i32>(floor(at.xy - rest.dest.zw)), vec2(0), size - vec2(1));
    return textureLoad(rest_frame, texel, 0) * rest.frame.z;
}
