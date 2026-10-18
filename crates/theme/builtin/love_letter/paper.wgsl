fn material(i: MaterialInput) -> vec4<f32> {
    let noise = fract(sin(dot(floor(i.position * 1.3), vec2<f32>(12.9898, 78.233))) * 43758.5453);
    let fibre = sin(i.position.y * 3.1 + sin(i.position.x * 0.07));
    let shade = 0.987 + noise * 0.013 + fibre * 0.003;
    return vec4<f32>(i.color.rgb * shade, i.color.a);
}
