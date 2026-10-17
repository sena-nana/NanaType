// CC0-1.0. EffectInput and sampling helpers are supplied by NanaType.
fn effect(i: EffectInput) -> vec4<f32> {
    let wave = 0.5 + 0.5 * sin(i.uv.x * 12.0 + i.phase * 6.28318);
    let event = 1.0 - clamp(i.line_age, 0.0, 1.0);
    return vec4(mix(i.accent.rgb, i.border.rgb, i.params0.x), wave * (0.15 + event * 0.15));
}
