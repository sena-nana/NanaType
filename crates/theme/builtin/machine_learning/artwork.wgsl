fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    return length(p - a - ab * clamp(dot(p - a, ab) / dot(ab, ab), 0., 1.));
}
fn effect(i: EffectInput) -> vec4<f32> {
    // Keep the network in the header, away from the reading field.
    let p = i.uv * i.size;
    // A vertical nameplate owns the right rail in portrait composition.
    let name_rail = select(0., 40., i.size.y > i.size.x);
    let origin = vec2<f32>(i.size.x - 112. - name_rail, 8.);
    let q = p - origin;
    let nodes = array<vec2<f32>, 6>(vec2(8.,18.), vec2(32.,6.), vec2(34.,30.), vec2(60.,14.), vec2(82.,5.), vec2(92.,28.));
    var line = 0.;
    var node = 0.;
    line = max(line, 1. - smoothstep(0.3, 1.1, segment(q, nodes[0], nodes[1])));
    line = max(line, 1. - smoothstep(0.3, 1.1, segment(q, nodes[1], nodes[2])));
    line = max(line, 1. - smoothstep(0.3, 1.1, segment(q, nodes[2], nodes[3])));
    line = max(line, 1. - smoothstep(0.3, 1.1, segment(q, nodes[3], nodes[4])));
    line = max(line, 1. - smoothstep(0.3, 1.1, segment(q, nodes[4], nodes[5])));
    line = max(line, 1. - smoothstep(0.3, 1.1, segment(q, nodes[0], nodes[2])));
    line = max(line, 1. - smoothstep(0.3, 1.1, segment(q, nodes[2], nodes[4])));
    node = max(node, 1. - smoothstep(1.4, 2.4, length(q - nodes[0])));
    node = max(node, 1. - smoothstep(1.4, 2.4, length(q - nodes[1])));
    node = max(node, 1. - smoothstep(1.4, 2.4, length(q - nodes[2])));
    node = max(node, 1. - smoothstep(1.4, 2.4, length(q - nodes[3])));
    node = max(node, 1. - smoothstep(1.4, 2.4, length(q - nodes[4])));
    node = max(node, 1. - smoothstep(1.4, 2.4, length(q - nodes[5])));
    let alpha = max(line * 0.28, node * 0.8) * smoothstep(180., 270., i.size.x);
    return vec4<f32>(mix(pow(vec3(0.392,0.91,0.878),vec3(2.2)), pow(vec3(0.659,0.604,1.),vec3(2.2)), node), alpha);
}
