// Must match text_light.wgsl: fixed 2px cells, no sweeping direction.
fn grain(p: vec2<f32>) -> f32 {
    return fract(sin(dot(floor(p / 2.), vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn material(i: MaterialInput) -> vec4<f32> {
    let p = clamp((i.progress - 0.2) / 0.8, 0., 1.);
    let keep = smoothstep(p - 0.06, p + 0.06, grain(i.target_position));
    return vec4<f32>(i.color.rgb, select(select(keep, 1., i.progress <= 0.2), 0., i.progress >= 1.));
}
