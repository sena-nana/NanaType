fn material(i: MaterialInput) -> vec4<f32> {
    return vec4<f32>(i.color.rgb, smoothstep(0., 0.65, clamp(i.progress, 0., 1.)));
}
