fn grain(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn material(i: MaterialInput) -> vec4<f32> {
    let p = clamp(i.progress, 0., 1.);
    if p <= 0. { return vec4<f32>(1.); }
    if p >= 1. { return vec4<f32>(1., 1., 1., 0.); }
    let paper_grain = grain(floor(i.target_position * 0.65));
    let soft_grain = grain(floor(i.target_position * 0.16));
    let threshold = paper_grain * 0.55 + soft_grain * 0.45;
    let ink = smoothstep(p - 0.12, p + 0.16, threshold);
    return vec4<f32>(1., 1., 1., ink * (1. - p * p));
}
