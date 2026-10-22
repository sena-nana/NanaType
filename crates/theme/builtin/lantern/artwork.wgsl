// Original procedural print artwork. All dimensions are logical pixels.
// Declared ornament texture; no user loops, persistent state or per-frame randomness.
fn aa(d: f32) -> f32 { return 1. - smoothstep(-0.65, 0.65, d); }
fn line(d: f32, w: f32) -> f32 { return aa(abs(d) - w); }
fn rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + r;
    return length(max(q, vec2(0.))) + min(max(q.x,q.y),0.) - r;
}
fn ink(r: f32, g: f32, b: f32) -> vec3<f32> { return pow(vec3(r,g,b)/255., vec3(2.2)); }
fn grain(p: vec2<f32>) -> f32 { return fract(sin(dot(floor(p),vec2(12.9898,78.233)))*43758.5453); }
fn rotate(p: vec2<f32>, a: f32) -> vec2<f32> {
    return vec2(cos(a)*p.x-sin(a)*p.y, sin(a)*p.x+cos(a)*p.y);
}
// LANTERNS AT NIGHT: black lacquer, cinnabar band, gilt keyline and lattice
// frieze; a beam over the top right hangs two lanterns outside the box.
// One immutable, declared PNG (the lantern) is drawn twice at two scales.
fn decal(p:vec2<f32>, origin:vec2<f32>, extent:vec2<f32>) -> vec4<f32> {
    let uv=(p-origin)/extent;
    if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
    return sample0(uv);
}
// Straight-alpha "over": the headroom above the box stays transparent.
fn over(d: vec4<f32>, c: vec3<f32>, k: f32) -> vec4<f32> {
    let a = k + d.a*(1.-k);
    return vec4((c*k + d.rgb*d.a*(1.-k))/max(a, 1e-4), a);
}
// A lantern hangs from `pivot` on the beam; the free tassel lags the swing.
fn lamp(p: vec2<f32>, pivot: vec2<f32>, w: f32, ang: f32, lag: f32) -> vec4<f32> {
    var q = pivot + rotate(p - pivot, ang);
    let h = w*1.25;
    q.x += smoothstep(.82, .97, (q.y - pivot.y)/h)*lag;
    return decal(q, vec2(pivot.x - w*.5, pivot.y - 2.), vec2(w, h));
}
fn gold(p: vec2<f32>, s: vec2<f32>) -> vec3<f32> {
    let g = clamp(p.x/s.x*.6 + p.y/s.y*.4, 0., 1.);
    return mix(mix(ink(241.,212.,154.), ink(217.,168.,78.), smoothstep(0.,.5,g)),
        ink(169.,123.,54.), smoothstep(.5,1.,g));
}
// Warm lamp light on lacquer: bright core, long soft falloff.
fn warm(p: vec2<f32>, c: vec2<f32>, r: f32) -> f32 {
    let d = clamp(1. - length(p - c)/r, 0., 1.);
    return .085*d*d;
}
fn bloom(p: vec2<f32>, c: vec2<f32>, r: vec2<f32>) -> f32 {
    return clamp(1. - length((p - c)/r), 0., 1.);
}
// Compact and portrait plates: the same lacquer and gilt, no hanging lamps.
fn responsive_print(i: EffectInput) -> vec4<f32> {
    let p=i.uv*i.size; let s=i.size;
    // Tiny windows get a slimmer band so 12px text insets still clear it.
    let k=clamp(min(s.x,s.y)/160.,.6,1.);
    let o=rect(p-s*.5, s*.5-vec2(4.*k), 4.);
    var c=vec4(0.);
    c=over(c, vec3(0.), .35*(1.-smoothstep(-1.,4.,o-1.)));
    c=over(c, i.accent.rgb, aa(o));
    let lac=mix(ink(36.,20.,15.), ink(26.,15.,14.), clamp(p.y/s.y,0.,1.));
    c=over(c, lac, aa(rect(p-s*.5, s*.5-vec2(8.*k), 2.)));
    let glow=clamp(1.-length(p-vec2(s.x-20.,10.))/170.,0.,1.);
    c=vec4(mix(c.rgb, ink(255.,138.,61.), .3*glow*glow*aa(rect(p-s*.5, s*.5-vec2(8.*k), 2.))), c.a);
    c=vec4(mix(c.rgb, gold(p,s), line(rect(p-s*.5, s*.5-vec2(12.*k), 1.), .75)), c.a);
    return vec4(c.rgb*(.985+.025*grain(p)), c.a);
}
fn effect(i: EffectInput) -> vec4<f32> {
    if(i.params0.y > .5 || i.params0.z > .5) { return responsive_print(i); }
    let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
    // The box sits below 56px of headroom where the lanterns hang.
    let top=56.;
    let mid=vec2(s.x*.5, (top+s.y-4.)*.5);
    let hb=vec2(s.x*.5-6., (s.y-4.-top)*.5);
    let outer=rect(p-mid, hb, 5.);
    let panel=rect(p-mid, hb-vec2(6.), 2.);
    // Pixels in the reading column never move: the swing, its light and the
    // flicker only act right of the text column or above the quote row.
    // Above the box the canvas is see-through, so nothing there moves either:
    // the outline's alpha stays constant and only the hanging parts sway.
    let inbox=aa(outer+2.);
    let free=max(smoothstep(s.x-158.,s.x-142.,p.x), smoothstep(112.,100.,p.y))*inbox;
    let hang=smoothstep(57.,80.,p.y)*inbox;
    let small_a=(.02*sin(t+.8)+.006*sin(2.*t))*hang;
    let big_a=(.013*sin(t)+.004*sin(2.*t+.5))*hang;
    let sway=76.*sin(.013*sin(t)+.004*sin(2.*t+.5))*free;
    let flicker=1.+.08*sin(2.*t)*free;
    var c=vec4(0.);
    c=over(c, vec3(0.), .4*(1.-smoothstep(-2.,10.,rect(p-mid-vec2(0.,4.), hb, 5.))));
    let cin=mix(ink(201.,71.,63.), ink(154.,46.,44.), clamp((p.y-top)/(s.y-top),0.,1.));
    c=over(c, cin, aa(outer));
    let lac=mix(ink(36.,20.,15.), ink(26.,15.,14.), clamp((p.y-top)/(s.y-top),0.,1.));
    c=over(c, lac*(.985+.025*grain(p)), aa(panel));
    let light=warm(p, vec2(s.x-94.+sway, 84.), 360.)*aa(panel)*flicker;
    c=vec4(mix(c.rgb, ink(255.,138.,61.), light), c.a);
    // Gilt keyline, the lattice frieze under the top rail, and the rail itself.
    let span=aa(abs(p.x-s.x*.5)-s.x*.5+17.);
    c=vec4(mix(c.rgb, gold(p,s), line(rect(p-mid, hb-vec2(11.), 1.), .75)), c.a);
    let q=vec2(abs(fract((p.x-22.)/12.)*12.-6.), abs(p.y-72.));
    let frieze=line((q.x+q.y-6.)*.7071,.5)*aa(abs(p.y-72.)-6.5)*aa(abs(p.x-s.x*.5)-s.x*.5+22.);
    c=vec4(mix(c.rgb, i.border.rgb, frieze*.45), c.a);
    c=vec4(mix(c.rgb, gold(p,s), line(p.y-79.,.75)*span), c.a);
    // Lamp bloom spills past the box into the headroom.
    let warm_c=ink(255.,160.,90.);
    c=over(c, warm_c, .5*bloom(p, vec2(s.x-84.+sway, 82.), vec2(84.,74.))*flicker);
    c=over(c, warm_c, .35*bloom(p, vec2(s.x-168.+sway*.8, 56.), vec2(52.,44.))*flicker);
    let lag=1.4*(sin(t-.6)-sin(-.6))+.5*sin(2.*t-.9);
    let small=lamp(p, vec2(s.x-168., 6.), 86., small_a, lag*.7);
    c=over(c, small.rgb*(1.+.05*(flicker-1.)), small.a);
    let big=lamp(p, vec2(s.x-84., 5.), 143., big_a, lag);
    c=over(c, big.rgb*(1.+.05*(flicker-1.)), big.a);
    // The beam enters from the right edge and covers both cords.
    let beam=aa(s.x-226.-p.x)*aa(p.y-7.);
    c=over(c, mix(ink(201.,71.,63.), ink(154.,46.,44.), clamp(p.y/7.,0.,1.)), beam);
    c=vec4(mix(c.rgb, i.border.rgb, line(p.y-6.25,.75)*aa(s.x-226.-p.x)), c.a);
    let knob=length(p-vec2(s.x-226.,1.5));
    c=over(c, gold(p,s), aa(knob-7.));
    c=over(c, ink(154.,46.,44.), aa(knob-2.8));
    return c;
}
