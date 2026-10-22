// Original procedural print artwork. All dimensions are logical pixels.
// No user loops, persistent state or per-frame randomness.
fn aa(d: f32) -> f32 { return 1. - smoothstep(-0.65, 0.65, d); }
fn line(d: f32, w: f32) -> f32 { return aa(abs(d) - w); }
fn disc(p: vec2<f32>, r: f32) -> f32 { return aa(length(p) - r); }
fn ring(p: vec2<f32>, r: f32, w: f32) -> f32 { return line(length(p) - r, w); }
fn ink(r: f32, g: f32, b: f32) -> vec3<f32> { return pow(vec3(r,g,b)/255., vec3(2.2)); }
fn grain(p: vec2<f32>) -> f32 { return fract(sin(dot(floor(p),vec2(12.9898,78.233)))*43758.5453); }
// CATHEDRAL FRONT: two stained-glass lancets, a gable with a rose window,
// pewter tracery over a wine reading panel. Symmetric about the centre.
fn pewter(t: f32) -> vec3<f32> {
    return mix(mix(ink(214.,215.,223.), ink(168.,169.,180.), smoothstep(0.,.5,t)),
        ink(124.,125.,137.), smoothstep(.5,1.,t));
}
fn jewel(n: i32) -> vec3<f32> {
    if (n == 0) { return ink(179.,34.,74.); }
    if (n == 1) { return ink(47.,77.,168.); }
    if (n == 2) { return ink(46.,139.,106.); }
    return ink(217.,154.,58.);
}
// Pointed arch: axis cx, half width hw, springing line y = spring; the two
// arcs are centred c beyond the axis with radius r.
fn arch(q: vec2<f32>, cx: f32, hw: f32, spring: f32, c: f32, r: f32) -> f32 {
    let x = abs(q.x - cx);
    if (q.y < spring) { return length(vec2(x + c, q.y - spring)) - r; }
    return x - hw;
}
// Chamfered plate (x 8..w-8, y top..h-8) with a pointed gable on the axis.
fn plate(p: vec2<f32>, s: vec2<f32>, top: f32, cut: f32, gable: vec4<f32>) -> f32 {
    let c = vec2(s.x*.5, (top + s.y - 8.)*.5);
    let q = abs(p - c) - vec2(s.x*.5 - 8., (s.y - 8. - top)*.5);
    let body = max(max(q.x, q.y), (q.x + q.y + cut)*.7071);
    if (gable.x <= 0.) { return body; }
    let g = max(arch(p, s.x*.5, gable.x, gable.y, gable.z, gable.w), p.y - top - 1.);
    return min(body, g);
}
// One lancet in its own frame: origin at the window's top-left, 68×214.
fn lancet(q: vec2<f32>, glow: f32) -> vec4<f32> {
    let outer = max(arch(q, 34., 34., 44., 11.47, 45.47), q.y - 214.);
    let sb = abs(q - vec2(34., 210.5)) - vec2(38., 2.5);
    let sill = length(max(sb, vec2(0.))) + min(max(sb.x, sb.y), 0.);
    if (min(outer, sill) > 2.) { return vec4(0.); }
    let dark = ink(23.,17.,26.);
    let metal = pewter(clamp(q.y/214., 0., 1.));
    var c = ink(18.,12.,20.);
    let side = select(1., 0., q.x < 34.);
    let sub = max(arch(q, 22. + 24.*side, 12., 86., 20.04, 32.04), q.y - 206.);
    let row = i32(clamp(floor((q.y - 64.)/22.), 0., 5.));
    var left = array<i32, 6>(0, 1, 3, 0, 2, 1);
    var right = array<i32, 6>(1, 0, 2, 1, 3, 0);
    var glass = jewel(select(left[row], right[row], side > .5));
    // Quarry diamonds, then light through the middle of each light.
    let m = vec2(fract(q.x/12.)*12. - 6., fract(q.y/16.)*16. - 8.);
    glass = mix(glass, dark, line((abs(m.x)/6. + abs(m.y)/8. - 1.)*4.8, .5)*.55);
    let lt = length((q - vec2(22. + 24.*side, 124.4))/vec2(16.8, 91.2));
    glass = mix(glass, ink(255.,244.,224.), .28*(1. - smoothstep(0., .6, lt)));
    glass *= (1. - .45*smoothstep(.6, 1., lt))*glow;
    c = mix(c, glass, aa(sub));
    let ly = abs(fract((q.y - 86.)/22. + .5) - .5)*22.;
    c = mix(c, dark, line(ly, .75)*step(85., q.y)*step(q.y, 175.)*aa(abs(q.x - 34.) - 24.));
    c = mix(c, metal, line(sub, .75));
    let inner = max(arch(q, 34., 28., 46., 13.16, 41.16), q.y - 208.);
    c = mix(c, metal, line(inner, .75)*.55);
    // Quatrefoil head: amber and emerald foils around a ruby boss.
    let f = q - vec2(34., 38.);
    c = mix(c, dark, disc(f, 15.));
    let fa = abs(f);
    let tall = fa.y > fa.x;
    let pd = length(fa - select(vec2(7., 0.), vec2(0., 7.), tall)) - 6.2;
    c = mix(c, select(ink(46.,139.,106.), ink(217.,154.,58.), tall)*glow, aa(pd));
    c = mix(c, dark, line(pd, .5));
    let bd = length(f) - 3.4;
    c = mix(c, ink(179.,34.,74.)*glow, aa(bd));
    c = mix(c, dark, line(bd, .5));
    c = mix(c, metal, ring(f, 15., .75));
    c = mix(c, metal, line(outer, 1.25));
    c = mix(c, pewter(clamp((q.x + 4.)/76., 0., 1.)), aa(sill));
    return vec4(c, max(aa(outer - 1.25), aa(sill)));
}
// Rose window of eight foils; k scales the drawing, w keeps strokes in px.
fn rose(v: vec2<f32>, k: f32, glow: f32) -> vec4<f32> {
    let u = v*k;
    let r = length(u);
    if (r > 26.) { return vec4(0.); }
    let dark = ink(23.,17.,26.);
    var c = ink(18.,12.,20.);
    let a = atan2(u.y, u.x) + 1.570796;
    let n = round(a/.785398);
    let ca = n*.785398 - 1.570796;
    let pd = length(u - 14.*vec2(cos(ca), sin(ca))) - 6.3;
    let even = fract(n*.5) < .25;
    c = mix(c, select(ink(47.,77.,168.), ink(179.,34.,74.), even)*glow, aa(pd));
    c = mix(c, dark, line(pd, .5));
    let spoke = abs(a/.785398 - n);
    c = mix(c, dark, line((.5 - spoke)*.785398*r/k, .45)*step(7., r));
    c = mix(c, ink(168.,169.,180.), ring(u, 19.8, .5)*.5);
    c = mix(c, ink(217.,154.,58.)*glow, aa(r - 7.));
    c = mix(c, ink(168.,169.,180.), line((r - 7.)/k, .6));
    c = mix(c, ink(46.,139.,106.)*glow, aa(r - 2.));
    let sy = clamp((u.y + 23.)/46., 0., 1.);
    let sheen = mix(.32, .06, smoothstep(0., .45, sy));
    c = mix(c, vec3(1.), sheen*.8*(1. - smoothstep(.45, .5, sy)));
    c = mix(c, vec3(0.), .25*.8*smoothstep(.45, 1., sy));
    c = mix(c, pewter(sy), line((r - 23.)/k, 1.));
    return vec4(c, aa((r - 23.)/k - 1.));
}
// Narrow, short and portrait layouts: the plate alone, no lancets.
fn responsive_print(i: EffectInput, glow: f32) -> vec4<f32> {
    let p = i.uv*i.size; let s = i.size; let compact = i.params0.z > .5;
    let top = select(4., 14., compact);
    let d = plate(p, s, top, 6., select(vec4(0.), vec4(24., top + 30., 7.77, 43.7), compact));
    var c = mix(ink(42.,20.,32.), ink(23.,17.,26.), clamp(p.y/max(s.y, 1.), 0., 1.));
    if !compact { c = mix(c, ink(30.,16.,26.), aa(s.y - 158. - p.y)); }
    c = mix(c, ink(47.,77.,168.), .22*max(0., 1. - length((p - vec2(s.x*.5, top))/vec2(90., 40.))));
    c *= .987 + .025*grain(p);
    c = mix(c, i.border.rgb, line(d + 5., .6)*.32);
    c = mix(c, pewter((p.x/s.x + p.y/s.y)*.5), line(d, 1.));
    var alpha = aa(d - 1.);
    if compact {
        let w = rose(p - vec2(s.x*.5, top + 3.), 2.3, glow);
        c = mix(c, w.rgb, w.a); alpha = max(alpha, w.a);
    }
    return vec4(c, alpha);
}
fn effect(i: EffectInput) -> vec4<f32> {
    let t = i.phase*6.283185;
    // The glass breathes over the 16s cycle and brightens as a line starts.
    let glow = .9 + .16*(.5 - .5*cos(t)) + .32*exp(-i.line_age*2.5);
    if (i.params0.y > .5 || i.params0.z > .5) { return responsive_print(i, glow); }
    let p = i.uv*i.size; let s = i.size;
    let gable = vec4(52., 90., 13.85, 89.08);
    let d = plate(p, s, 30., 10., gable);
    var c = mix(ink(42.,20.,32.), ink(23.,17.,26.), clamp((p.y - 2.)/234., 0., 1.));
    // Glass light pooled on the lacquer. Still: nothing moves behind the text.
    let mx = min(p.x, s.x - p.x);
    c = mix(c, ink(179.,34.,74.), .32*max(0., 1. - length((vec2(mx, p.y) - vec2(56., 130.))/vec2(120., 110.))));
    c = mix(c, ink(47.,77.,168.), .28*max(0., 1. - length((p - vec2(s.x*.5, 40.))/vec2(140., 60.))));
    c *= .987 + .025*grain(p);
    c = mix(c, i.border.rgb, line(d + 6., .75)*.32);
    c = mix(c, pewter((p.x/s.x + p.y/s.y)*.5), line(d, 1.));
    var alpha = aa(d - 1.);
    let fin = disc(p - vec2(s.x*.5, 2.5), 2.5);
    c = mix(c, ink(214.,215.,223.), fin); alpha = max(alpha, fin);
    if (i.params0.x < .5) {
        let w = rose(p - vec2(s.x*.5, 38.), 1., glow);
        c = mix(c, w.rgb, w.a); alpha = max(alpha, w.a);
        let l = lancet(vec2(mx - 22., p.y - 10.), glow);
        c = mix(c, l.rgb, l.a); alpha = max(alpha, l.a);
    }
    // A soft lift under the plate, outside its silhouette only.
    let sh = .35*(1. - smoothstep(-3., 7., plate(p - vec2(0., 3.), s, 30., 10., gable)));
    let total = alpha + (1. - alpha)*sh;
    return vec4(c*alpha/max(total, 1e-4), total);
}
