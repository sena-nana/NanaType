// Ivory laid writing paper: chain lines, laid lines, ruled lines on the text
// pitch and one dark-wine wax seal. Static; logical pixels of the shape.
// params: x 1 = wax seal (hidden on narrow boxes), y 1 = ruled lines.
fn lin(r: f32, g: f32, b: f32) -> vec3<f32> { return pow(vec3(r, g, b) / 255., vec3(2.2)); }
fn aa(d: f32) -> f32 { return 1. - smoothstep(-.6, .6, d); }
fn hash(p: vec2<f32>) -> f32 { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p); let f = fract(p); let u = f * f * (3. - 2. * f);
    return mix(mix(hash(i), hash(i + vec2(1., 0.)), u.x), mix(hash(i + vec2(0., 1.)), hash(i + vec2(1., 1.)), u.x), u.y);
}
fn dot2(v: vec2<f32>) -> f32 { return dot(v, v); }
// Heart distance, unit size, y pointing up.
fn heart(q: vec2<f32>) -> f32 {
    let p = vec2(abs(q.x), q.y);
    if (p.y + p.x > 1.) { return sqrt(dot2(p - vec2(.25, .75))) - sqrt(2.) / 4.; }
    return sqrt(min(dot2(p - vec2(0., 1.)), dot2(p - .5 * max(p.x + p.y, 0.)))) * sign(p.x - p.y);
}
fn material(i: MaterialInput) -> vec4<f32> {
    let p = i.position; let s = i.size;
    let seal_on = material_param(0u).x > .5 && s.x > 520. && s.y > 150.;
    let rules_on = material_param(0u).y > .5;
    var c = i.color.rgb;
    // Faint cloudy formation of handmade paper.
    let cloud = noise(vec2(p.x * .02, p.y * .006) + vec2(3., 7.)) * .6 + noise(p * .05) * .4;
    c = mix(c, lin(233., 220., 200.), .1 * smoothstep(.45, .9, cloud));
    // Laid lines every 3px; chain lines every 46px with a soft halo.
    let laid = aa(abs(fract(p.y / 3.) * 3. - .5) - .5);
    c = mix(c, lin(233., 220., 200.), laid * .12);
    let cx = abs(fract((p.x - 24.) / 46.) * 46. - 23.);
    let chain = 23. - cx;
    c = mix(c, lin(233., 220., 200.), aa(chain - .6) * .45 + (1. - smoothstep(0., 2.5, chain)) * .08);
    c *= .992 + .01 * hash(floor(p * .8));
    // Ruled lines sit under each body line (line height 32 x 1.6) of a plain
    // line; a reply quote is shorter than one pitch, so the body cannot align both ways.
    if (rules_on) {
        let pitch = 51.2;
        let k = round((p.y - 100.) / pitch);
        let ry = 100. + max(k, 0.) * pitch;
        let rule = aa(abs(p.y - ry) - .75) * aa(24. - p.x) * aa(p.x - s.x + 24.) * aa(p.y - s.y + 14.);
        c = mix(c, lin(228., 201., 201.), rule);
    }
    if (seal_on) {
        let o = vec2(s.x - 70., s.y - 52.);
        let a = -.14;
        let d = p - o;
        let q = vec2(cos(a) * d.x + sin(a) * d.y, -sin(a) * d.x + cos(a) * d.y);
        let ang = atan2(q.y, q.x);
        let blob = 31.5 + 1.8 * sin(ang * 5. + 1.3) + 1.2 * sin(ang * 9. + 4.) + .8 * sin(ang * 14. + 2.);
        // Soft cast shadow beneath the wax.
        let sq = q - vec2(1., 2.5);
        let sh = 1. - smoothstep(-2., 6., length(sq) - blob);
        c = mix(c, lin(42., 10., 18.), sh * .38);
        let r = length(q);
        let wax_t = clamp(length(q - vec2(-12., -10.)) / 52., 0., 1.);
        var w = mix(lin(154., 46., 69.), lin(126., 31., 53.), smoothstep(0., .55, wax_t));
        w = mix(w, lin(86., 16., 32.), smoothstep(.55, 1., wax_t));
        // Pressed well with a dark upper rim and a lit lower rim.
        let well = aa(r - 22.5);
        w = mix(w, mix(lin(122., 28., 49.), lin(94., 20., 38.), clamp(length(q - vec2(6., 7.)) / 30., 0., 1.)), well);
        w = mix(w, lin(62., 9., 21.), aa(abs(length(q + vec2(.8, .8)) - 22.5) - 1.) * .55);
        w = mix(w, lin(180., 84., 104.), aa(abs(length(q - vec2(.9, .9)) - 22.5) - .75) * .7);
        // Embossed heart: highlight up-left, shade down-right, body on top.
        let hq = (q - vec2(0., 1.)) / 14.;
        let hp = vec2(hq.x, -hq.y + .65);
        let hl = heart(hp + vec2(.1, -.1)) * 14.;
        let hd = heart(hp - vec2(.1, -.1)) * 14.;
        let hb = heart(hp) * 14.;
        w = mix(w, lin(194., 102., 122.), aa(hl) * .65);
        w = mix(w, lin(62., 9., 21.), aa(hd) * .6);
        w = mix(w, lin(130., 32., 58.), aa(hb));
        // A single soft specular glint.
        let g = q - vec2(-14., -20.);
        let gr = vec2(cos(.52) * g.x + sin(.52) * g.y, -sin(.52) * g.x + cos(.52) * g.y);
        w = mix(w, vec3(1.), (1. - smoothstep(.6, 1.2, length(gr / vec2(8., 3.5)))) * .2);
        c = mix(c, w, aa(r - blob));
    }
    return vec4(c, 1.);
}
