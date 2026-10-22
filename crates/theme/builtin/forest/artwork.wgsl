// Original procedural print artwork. All dimensions are logical pixels.
// Declared ornament texture; no user loops, persistent state or per-frame randomness.
// SPECIMEN CARD: sage mount, a smooth herbarium sheet, a pressed fern crossing
// the mount's top edge. The washi-tape nameplate and label are native nodes.
fn aa(d: f32) -> f32 { return 1. - smoothstep(-0.65, 0.65, d); }
fn rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + r;
    return length(max(q, vec2(0.))) + min(max(q.x,q.y),0.) - r;
}
fn ink(r: f32, g: f32, b: f32) -> vec3<f32> { return pow(vec3(r,g,b)/255., vec3(2.2)); }
fn grain(p: vec2<f32>) -> f32 { return fract(sin(dot(floor(p),vec2(12.9898,78.233)))*43758.5453); }
// One immutable, declared PNG: SVG master lives beside this program.
fn decal(p:vec2<f32>, origin:vec2<f32>, extent:vec2<f32>) -> vec4<f32> {
    let uv=(p-origin)/extent;
    if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
    return sample0(uv);
}
// Mount card from lo to hi with a smooth sheet inset by `inset`; returns rgba.
fn card(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>, inset: vec2<f32>) -> vec4<f32> {
    let mid = (lo + hi) * .5; let half = (hi - lo) * .5;
    let mount = rect(p - mid, half, 4.);
    let lift = 1. - smoothstep(-1., 7., rect(p - mid - vec2(0., 3.), half, 4.));
    // Felted sage board: fine tooth, slightly darker towards its edge.
    var c = ink(201., 211., 182.) * (.955 + .07 * grain(p * .9));
    c = mix(c, ink(176., 190., 158.), (1. - smoothstep(0., 6., -mount)) * .35);
    let sheet = rect(p - mid, half - inset, 1.);
    let shade = (1. - smoothstep(-1., 5., rect(p - mid - vec2(0., 2.), half - inset, 1.))) * (1. - aa(sheet));
    c = mix(c, ink(120., 140., 108.), shade * .28);
    var paper = ink(238., 240., 226.) * (.988 + .018 * grain(vec2(p.x * .5, p.y * 1.7)));
    paper = mix(paper, ink(250., 251., 243.), aa(abs(sheet + 1.) - .5) * .6);
    c = mix(c, paper, aa(sheet));
    let a = aa(mount);
    // A soft cast shadow below the mount (premultiplied into transparency).
    let sh = lift * (1. - a) * .22;
    return vec4(mix(ink(40., 60., 44.), c, a), max(a, sh));
}
fn effect(i: EffectInput) -> vec4<f32> {
    let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
    // Responsive instances: mount + sheet only, in logical pixels.
    if(i.params0.y > .5 || i.params0.z > .5) {
        let compact = i.params0.z > .5;
        let inset = select(vec2(14., 12.), vec2(7., 8.), compact);
        return card(p, vec2(2.), s - vec2(2., 4.), inset);
    }
    var out = card(p, vec2(2., 20.), vec2(s.x - 18., s.y - 4.), vec2(14., 12.));
    // The pressed stem stays planted; only the flexible upper fronds bend.
    let origin = vec2(s.x - 196., -11.);
    let extent = vec2(174.1, 217.6);
    let root = origin + extent * vec2(.273, .933);
    var leaf_p = p - root;
    // Motion stays on the opaque mount: the fronds crossing its top edge are
    // still, so the silhouette never changes; bending fades in below the edge.
    let lo = vec2(2., 20.); let hi = vec2(s.x - 18., s.y - 4.);
    // It also stays clear of the reading field (right padding 150).
    let held = smoothstep(1., 22., -rect(p - (lo + hi) * .5, (hi - lo) * .5, 4.))
        * smoothstep(s.x - 150., s.x - 128., p.x);
    let bend = smoothstep(9., 202., -leaf_p.y) * held;
    let tip = bend * smoothstep(9., 35., abs(leaf_p.x + .32 * leaf_p.y));
    leaf_p.x += bend * (2.7 * sin(t) + .5 * (sin(2. * t + leaf_p.y * .022) - sin(leaf_p.y * .022)));
    // Small lateral pinnae flex after the rachis.
    leaf_p.y += .45 * bend * sin(2. * t)
        + tip * .65 * (sin(2. * t + leaf_p.x * .05) - sin(leaf_p.x * .05));
    if(i.params0.x < .5) {
        let fern = decal(leaf_p + root, origin, extent);
        let a = fern.a + out.a * (1. - fern.a);
        let rgb = (fern.rgb * fern.a + out.rgb * out.a * (1. - fern.a)) / max(a, 1e-4);
        out = vec4(rgb, a);
    }
    return out;
}
