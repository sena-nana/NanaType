// Original procedural print artwork. All dimensions are logical pixels.
// Declared ornament texture; no user loops, persistent state or per-frame randomness.
fn aa(d: f32) -> f32 { return 1. - smoothstep(-0.65, 0.65, d); }
fn line(d: f32, w: f32) -> f32 { return aa(abs(d) - w); }
fn disc(p: vec2<f32>, r: f32) -> f32 { return aa(length(p) - r); }
fn ring(p: vec2<f32>, r: f32, w: f32) -> f32 { return line(length(p) - r, w); }
fn ink(r: f32, g: f32, b: f32) -> vec3<f32> { return pow(vec3(r,g,b)/255., vec3(2.2)); }
fn rotate(p: vec2<f32>, a: f32) -> vec2<f32> {
    return vec2(cos(a)*p.x-sin(a)*p.y, sin(a)*p.x+cos(a)*p.y);
}
// AQUARIUM WATERLINE: the plate's top edge is a rippling water surface; a
// medusa's bell breaks it, tentacles trail into the deep-teal reading water.
// One immutable, declared PNG: SVG master lives beside this program.
fn decal(p: vec2<f32>, origin: vec2<f32>, extent: vec2<f32>) -> vec4<f32> {
    let uv = (p - origin)/extent;
    if (any(uv < vec2(0.)) || any(uv > vec2(1.))) { return vec4(0.); }
    return sample0(uv);
}
// Water vessel: sides x0..x1, rounded bottom at y1, open top at the surface.
fn vessel(p: vec2<f32>, x0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let c = vec2((x0 + x1)*.5, y1 - r);
    let q = vec2(abs(p.x - c.x) - (x1 - x0)*.5 + r, p.y - c.y);
    let corner = length(max(q, vec2(0.))) - r;
    return select(max(abs(p.x - c.x) - (x1 - x0)*.5, p.y - y1), corner, q.x > 0. && q.y > 0.);
}
// Narrow, short and portrait layouts: the waterline alone, no medusa.
fn responsive_print(i: EffectInput) -> vec4<f32> {
    let p = i.uv*i.size; let s = i.size;
    let surface = 22. + 1.8*sin((p.x - 4.)*.0872665);
    let body = vessel(p, 4., s.x - 4., s.y - 4., 12.);
    let d = max(body, surface - p.y);
    var c = mix(ink(14.,58.,68.), ink(7.,38.,48.), clamp((p.y - 18.)/max(s.y - 22., 1.), 0., 1.));
    c = mix(c, i.accent.rgb, .18*(1. - smoothstep(0., 30., p.y - surface)));
    c = mix(c, i.accent.rgb, line(body, .75)*.55*step(surface, p.y));
    c = mix(c, i.accent.rgb, line(p.y - surface, 1.)*aa(abs(p.x - s.x*.5) - s.x*.5 + 4.));
    return vec4(c, max(aa(d), line(p.y - surface, 1.)*aa(abs(p.x - s.x*.5) - s.x*.5 + 4.)));
}
fn effect(i: EffectInput) -> vec4<f32> {
    if (i.params0.y > .5 || i.params0.z > .5) { return responsive_print(i); }
    let p = i.uv*i.size; let s = i.size; let t = i.phase*6.283185;
    let teal = i.accent.rgb; let foam = ink(189.,240.,235.);
    // The surface: a slow standing swell, lifted where the bell breaks it.
    let jx = s.x - 90.;
    let surface = 44. + 2.6*sin((p.x - 6.)*.0541654) - 3.*exp(-pow((p.x - jx)/40., 2.));
    let under = p.y >= surface;
    let body = vessel(p, 6., s.x - 6., s.y - 6., 18.);
    let water = max(body, surface - p.y);
    // Reading area: nothing that moves may touch it.
    let rq = abs(p - vec2(395., 143.)) - vec2(359., 76.);
    let quiet = 1. - aa(max(rq.x, rq.y) - 1.);
    var c = mix(ink(14.,58.,68.), ink(7.,38.,48.), clamp((p.y - 41.)/197., 0., 1.));
    c = mix(c, teal, .22*(1. - smoothstep(30., 70., p.y)));
    c = mix(c, teal, .22*max(0., 1. - length(p - vec2(jx, 60.))/150.));
    // Caustics shimmer only in the water beside the medusa.
    let caustic = sin(p.x*.055 + sin(p.y*.042 + t)*.9) + sin(p.y*.07 - t);
    c = mix(c, foam, line(caustic, .05)*.06*smoothstep(s.x - 200., s.x - 120., p.x)*quiet);
    // The medusa: one 4s stroke squeezes, pushes and opens the bell. The
    // silhouette above water keeps still (its alpha is the window's edge);
    // the swim grows in with depth, so the body stays joined at the surface.
    let k = 1.4545;
    let amp = smoothstep(surface, surface + 36., p.y);
    let origin = vec2(s.x - 192., -27.6); let extent = vec2(204.8, 256.);
    let pivot = origin + vec2(102.4, 66.8);
    let cycle = fract(i.phase*4.);
    let squeeze = amp*smoothstep(.08, .30, cycle)*(1. - smoothstep(.30, .88, cycle));
    let push = smoothstep(.13, .43, cycle)*(1. - smoothstep(.43, 1., cycle));
    let bank = amp*(.16*sin(t) + .018*sin(2.*t));
    let old_bank = amp*(.16*sin(t - .28) + .018*sin(2.*t - .56));
    let drift = amp*k*vec2(5.8*sin(t) - sin(bank)*8.5*push, -8.5*push - 1.8*(1. - cos(t)));
    var local = rotate(p - pivot - drift, bank);
    let bell = 1. - smoothstep(18.*k, 48.*k, local.y);
    let tail = smoothstep(24.*k, 112.*k, local.y);
    let drag = amp*k*(3.8*sin(4.*t - tail*1.6) + sin(8.*t - tail*2.3))
        + local.y*(sin(bank) - sin(old_bank));
    local.x = local.x*(1. + .18*squeeze*bell) + tail*drag;
    local.y = local.y*(1. - .10*squeeze*bell);
    let swim = local + pivot - select(vec2(0.), vec2(2., 0.), under);
    let at = mix(p - select(vec2(0.), vec2(2., 0.), under), swim, quiet);
    let jelly = decal(at, origin, extent);
    let shape = smoothstep(.03, .07, jelly.a);
    if (under) {
        c = mix(c, teal, shape*.12);
        c = mix(c, jelly.rgb, jelly.a*.75);
        let b1 = p - vec2(s.x - 36. + 1.8*sin(2.*t), s.y + 12. - fract(i.phase + .3)*(s.y + 24.));
        let b2 = p - vec2(s.x - 46. + 1.2*sin(t), s.y + 12. - fract(i.phase + .78)*(s.y + 24.));
        let fade = smoothstep(surface + 8., surface + 30., p.y)*(1. - smoothstep(s.y - 34., s.y - 15., p.y));
        c = mix(c, foam, (ring(b1, 3.8, .7)*.5 + ring(b2, 2.4, .6)*.4)*fade*quiet);
    }
    // Glass: side walls rise a little above the surface; a faint inner line.
    c = mix(c, teal, line(body, 1.)*.55*step(surface, p.y));
    c = mix(c, ink(229.,248.,245.), line(body + 6., .5)*.12*step(surface + 8., p.y));
    var alpha = aa(water);
    let wall = line(abs(p.x - s.x*.5) - s.x*.5 + 6., 1.)*step(30., p.y)*step(p.y, surface);
    let lip = .06*step(30., p.y)*step(p.y, surface)*aa(abs(p.x - s.x*.5) - s.x*.5 + 6.);
    c = mix(c, teal, max(wall*.7, lip)*(1. - alpha) + wall*.7*alpha);
    alpha = max(alpha, max(wall*.7, lip));
    // Above water: an opaque deep-teal backing keeps the translucent bell
    // legible over any scene.
    if (!under) {
        let back = mix(ink(46.,124.,130.), ink(14.,58.,68.), clamp((p.y - origin.y - 30.)/50., 0., 1.));
        c = mix(c, back, shape);
        c = mix(c, jelly.rgb, jelly.a);
        alpha = max(alpha, max(shape, jelly.a));
    }
    // The surface line, its meniscus and the rings around the bell.
    let inside = aa(abs(p.x - s.x*.5) - s.x*.5 + 6.);
    let edge = line(p.y - surface, 1.)*inside;
    c = mix(c, teal, edge);
    alpha = max(alpha, edge);
    c = mix(c, ink(229.,248.,245.), line(p.y - surface - 4., .5)*.25*inside);
    let rip = (p - vec2(jx, 42.))/vec2(96., 5.);
    let rip2 = (p - vec2(jx, 42.))/vec2(128., 7.);
    let rings = line((length(rip) - 1.)*5., .75)*.45 + line((length(rip2) - 1.)*7., .75)*.2;
    c = mix(c, foam, rings);
    alpha = max(alpha, rings);
    // The pearl's reflection under the floating name.
    let refl = max(line(p.y - 62., .75)*aa(abs(p.x - 75.) - 29.), line(p.y - 67., .75)*aa(abs(p.x - 75.) - 21.));
    c = mix(c, ink(238.,249.,246.), refl*.14);
    let sh = .4*(1. - smoothstep(-4., 10., max(vessel(p - vec2(0., 4.), 6., s.x - 6., s.y - 6., 18.), surface + 4. - p.y)));
    let total = alpha + (1. - alpha)*sh;
    return vec4(c*alpha/max(total, 1e-4), total);
}
