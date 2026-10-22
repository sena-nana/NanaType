// Original procedural print artwork. All dimensions are logical pixels.
// No user loops, persistent state or per-frame randomness.
fn aa(d: f32) -> f32 { return 1. - smoothstep(-0.65, 0.65, d); }
fn line(d: f32, w: f32) -> f32 { return aa(abs(d) - w); }
fn disc(p: vec2<f32>, r: f32) -> f32 { return aa(length(p) - r); }
fn ring(p: vec2<f32>, r: f32, w: f32) -> f32 { return line(length(p) - r, w); }
fn rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + r;
    return length(max(q, vec2(0.))) + min(max(q.x,q.y),0.) - r;
}
fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, w: f32) -> f32 {
    let d = b - a;
    return aa(length(p-a-d*clamp(dot(p-a,d)/dot(d,d),0.,1.))-w);
}
fn ink(r: f32, g: f32, b: f32) -> vec3<f32> { return pow(vec3(r,g,b)/255., vec3(2.2)); }
fn rotate(p: vec2<f32>, a: f32) -> vec2<f32> {
    return vec2(cos(a)*p.x-sin(a)*p.y, sin(a)*p.x+cos(a)*p.y);
}
// LUNAR ATLAS: an astrolabe centred on the plate's lower-right corner,
// bleeding off the region; its outermost arcs pass faintly behind the text.
fn brass(t: f32) -> vec3<f32> {
    return mix(mix(ink(241.,221.,168.), ink(201.,163.,90.), smoothstep(0.,.5,t)),
        ink(156.,118.,56.), smoothstep(.5,1.,t));
}
// Distance to an ellipse outline (first-order estimate, fine for strokes).
fn ellipse(q: vec2<f32>, ab: vec2<f32>) -> f32 {
    let k0 = length(q/ab); let k1 = length(q/(ab*ab));
    return k0*(k0 - 1.)/max(k1, 1e-4);
}
// Radial ticks hanging inward from radius r0 every `step` radians; every
// fifth tick is `major` long instead of `len`.
fn ticks(v: vec2<f32>, r0: f32, len: f32, major: f32, step: f32, w: f32) -> f32 {
    let r = length(v);
    let k = atan2(v.y, v.x)/step;
    let n = round(k);
    let big = abs(n*.2 - round(n*.2)) < .05;
    let reach = select(len, major, big);
    let band = aa(r0 - reach - r)*aa(r - r0);
    return line((k - n)*step*r, select(w, w*1.5, big))*band;
}
fn sparkle(q: vec2<f32>, h: f32) -> f32 {
    let a = abs(q)/vec2(h*.45, h);
    return aa((sqrt(a.x) + sqrt(a.y) - 1.)*h*.5);
}
fn field(p: vec2<f32>) -> f32 {
    let cells = vec2(147.,91.); let cell = floor(p/cells); let local = fract(p/cells)*cells;
    let star = local - vec2(36. + 31.*sin(cell.y*3.), 24. + 17.*cos(cell.x*7.));
    return disc(star, .9);
}
// Narrow, short and portrait layouts: a quiet plate with a corner arc.
fn responsive_print(i: EffectInput) -> vec4<f32> {
    let p = i.uv*i.size; let s = i.size;
    let d = rect(p - s*.5, s*.5 - vec2(4.), 12.);
    var c = mix(ink(27.,22.,56.), ink(42.,30.,82.), clamp((i.uv.x + i.uv.y)*.5, 0., 1.));
    c = mix(c, ink(244.,238.,255.), field(p)*.35);
    let v = p - (s - vec2(4.));
    let inside = aa(d);
    c = mix(c, i.accent.rgb, ring(v, 54., .75)*.55*inside);
    let dash = step(fract(atan2(v.y, v.x)*46./5.5), .3);
    c = mix(c, i.accent.rgb, ring(v, 46., .5)*.35*dash*inside);
    c = mix(c, i.accent.rgb, line(d + .75, .75)*.55);
    return vec4(c, aa(d));
}
fn effect(i: EffectInput) -> vec4<f32> {
    if (i.params0.y > .5 || i.params0.z > .5) { return responsive_print(i); }
    let p = i.uv*i.size; let s = i.size; let t = i.phase*6.283185;
    let gold = ink(201.,163.,90.); let lilac = ink(169.,156.,200.); let pale = ink(231.,204.,148.);
    let lo = vec2(6., 16.); let hi = s - vec2(20., 12.);
    let d = rect(p - (lo + hi)*.5, (hi - lo)*.5, 14.);
    let inside = aa(d);
    let k = clamp(((p.x - lo.x)/(hi.x - lo.x) + (p.y - lo.y)/(hi.y - lo.y))*.5, 0., 1.);
    var c = mix(ink(27.,22.,56.), ink(42.,30.,82.), k);
    c = mix(c, ink(244.,238.,255.), field(p)*.35);
    // The outermost rings, ticks and a tilted ecliptic: 12%, still.
    let v = p - hi; let r = length(v);
    let f = .12*inside;
    c = mix(c, gold, ring(v, 470., .75)*f);
    c = mix(c, gold, ticks(v, 470., 8., 16., .0349066, .5)*f);
    c = mix(c, lilac, ring(v, 380., .75)*f);
    let dash = step(fract(atan2(v.y, v.x)*290./8.), .25);
    c = mix(c, gold, ring(v, 290., .75)*dash*f);
    c = mix(c, lilac, line(ellipse(rotate(v - vec2(-36.4, 19.4), .244346), vec2(420., 150.)), .75)*f);
    c = mix(c, gold, line(d + .75, .75)*.55);
    var alpha = aa(d);
    // The speaker's four-point star, beside the gilt name.
    c = mix(c, ink(231.,204.,148.), sparkle(p - vec2(47., 49.), 8.));
    // Shadow under the plate.
    let sh = .38*(1. - smoothstep(-4., 9., rect(p - (lo + hi)*.5 - vec2(0., 4.), (hi - lo)*.5, 14.)));
    // The instrument: outer brass ring breaks the top edge.
    let bt = clamp(((v.x + 210.)/420. + (v.y + 210.)/420.)*.5, 0., 1.);
    let outer = ring(v, 210., 1.) + ticks(v, 210., 7., 7., .0872665, .5)*.8;
    c = mix(c, brass(bt), clamp(outer, 0., 1.));
    alpha = max(alpha, clamp(outer, 0., 1.));
    c = mix(c, ink(27.,22.,56.), disc(v, 186.)*.55*inside);
    let face = mix(mix(ink(58.,43.,110.), ink(42.,30.,82.), smoothstep(0., .75, r/176.)),
        ink(35.,26.,71.), smoothstep(.75, 1., r/176.));
    let plate = disc(v, 177.25);
    c = mix(c, face, plate);
    alpha = max(alpha, plate);
    let dt = clamp(((v.x + 176.)/352. + (v.y + 176.)/352.)*.5, 0., 1.);
    c = mix(c, brass(dt), ring(v, 176., 1.25));
    c = mix(c, gold, ring(v, 168., .5)*.8);
    c = mix(c, gold, ticks(v, 176., 8., 16., .0523599, .5)*.9);
    c = mix(c, gold, ring(v, 132., .75)*.75);
    c = mix(c, gold, ring(v, 98., .5)*.5);
    let spokes = max(segment(v, vec2(0.), vec2(-162.3, -43.5), .5),
        max(segment(v, vec2(0.), vec2(-118.8, -118.8), .5), segment(v, vec2(0.), vec2(-43.5, -162.3), .5)));
    c = mix(c, gold, spokes*.35);
    c = mix(c, lilac, ring(v - vec2(-34., -30.), 104., .75)*.85*plate);
    c = mix(c, lilac, line(ellipse(rotate(v - vec2(-50., -60.), .628319), vec2(64., 110.)), .5)*.6*plate);
    // The orbit precesses and a small star rides it; both stay off the text.
    let quiet = 1. - aa(rect(p - vec2(395., 137.), vec2(359., 69.), 0.) - 1.);
    let tilt = .488692 + .10*sin(t);
    let o = rotate(v - vec2(-60., -40.), tilt);
    c = mix(c, gold, line(ellipse(o, vec2(120., 46.)), .75)*.8*plate*quiet);
    let star = rotate(vec2(120.*cos(-t), 46.*sin(-t)), -tilt) + vec2(-60., -40.);
    c = mix(c, pale, disc(v - star, 2.2)*quiet);
    c += pale*line(ellipse(o, vec2(120., 46.)), 1.)*pow(max(0., cos(atan2(o.y/46., o.x/120.) + t)), 24.)*.12*quiet*plate;
    // Alidade, sparkles and the crescent near the centre.
    c = mix(c, brass(.4), segment(v, vec2(0.), vec2(-197.1, -56.5), 1.));
    c = mix(c, pale, disc(v - vec2(-197.1, -56.5), 3.2));
    c = mix(c, pale, sparkle(v - vec2(-114., -126.), 7.));
    c = mix(c, ink(244.,238.,255.), sparkle(v - vec2(-62., -144.), 5.)*.9);
    c = mix(c, pale, sparkle(v - vec2(-138., -70.), 4.)*.85);
    c = mix(c, ink(244.,238.,255.), sparkle(v - vec2(-8., -106.), 4.)*.8);
    c = mix(c, pale, disc(v - vec2(-54., -52.), 34.)*.1);
    let moon = disc(v - vec2(-54., -52.), 27.)*(1. - disc(v - vec2(-40., -61.), 23.));
    c = mix(c, brass(clamp(((v.x + 120.)/120. + (v.y + 108.)/110.)*.5, 0., 1.)), moon);
    c = mix(c, pale, disc(v, 5.));
    let total = alpha + (1. - alpha)*sh;
    return vec4(c*alpha/max(total, 1e-4), total);
}
