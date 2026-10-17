// Original procedural print artwork. All dimensions are logical pixels.
// Declared ornament texture; no user loops, persistent state or per-frame randomness.
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
fn grain(p: vec2<f32>) -> f32 { return fract(sin(dot(floor(p),vec2(12.9898,78.233)))*43758.5453); }
fn rotate(p: vec2<f32>, a: f32) -> vec2<f32> {
    return vec2(cos(a)*p.x-sin(a)*p.y, sin(a)*p.x+cos(a)*p.y);
}
// ROSE NOCTURNE: wine leather spine, etched rose window, gilt lancets.
fn lancet(p: vec2<f32>) -> f32 {
    let q=vec2(p.x,min(p.y,0.));
    let arch = max(length(q-vec2(27.,0.)),length(q+vec2(27.,0.)))-60.;
    return max(arch,p.y-82.);
}
// One immutable, declared PNG: SVG master lives beside this program.
fn decal(p:vec2<f32>, origin:vec2<f32>, extent:vec2<f32>, slot:f32) -> vec4<f32> {
    let uv=(p-origin)/extent;
    if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
    return sample0(vec2((uv.x+slot)*.5,uv.y));
}
fn effect(i: EffectInput) -> vec4<f32> {
    let p = i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
    let q = abs(p-s*.5)-s*.5+vec2(3.);
    let edge=max(max(q.x,q.y),(q.x+q.y+17.)*.7071);
    var c=i.base.rgb*(.94+.08*i.uv.x);
    c=mix(c,ink(72.,24.,44.),aa(p.x-119.));
    c=mix(c,ink(47.,24.,36.),aa(53.-p.y)*aa(p.y-s.y+23.)*aa(129.-p.x)*aa(p.x-s.x+112.));
    let spine=line(p.x-119.,.7)*aa(10.-p.y)*aa(p.y-s.y+10.);
    c=mix(c,i.border.rgb*.6,spine);
    let rose=p-vec2(61.,s.y*.52);
    let flower=decal(p,vec2(5.,s.y*.52-70.),vec2(112.,140.),0.);
    c=mix(c,flower.rgb,flower.a);
    let tracery=decal(p,vec2(s.x-115.,s.y*.5-70.),vec2(112.,140.),1.);
    c=mix(c,tracery.rgb,tracery.a*.44);
    let rule=segment(p,vec2(139.,53.),vec2(s.x-113.,53.),.45);
    c=mix(c,i.border.rgb,rule*.6);
    let rim=line(edge+7.,.55);
    c=mix(c,i.border.rgb,line(edge+2.,.4)*.24+rim*.78);
    // Gilt corner fittings and the two rivets terminate inside the frame.
    let corner=min(p,s-p);
    let fitting=max(segment(corner,vec2(23.,17.),vec2(40.,17.),.45),
        max(segment(corner,vec2(17.,23.),vec2(17.,40.),.45),
            segment(corner,vec2(17.,23.),vec2(23.,17.),.45)));
    c=mix(c,i.border.rgb,fitting*.42);
    let rivet=aa(abs(p.x-119.)+abs(min(p.y,s.y-p.y)-10.)-2.7);
    c=mix(c,i.border.rgb,rivet*.8);
    // The name plaque mounts to the same rail as the ruled reading panel.
    let mount=max(segment(p,vec2(131.,10.),vec2(131.,53.),.5),
        segment(p,vec2(131.,27.),vec2(140.,27.),.6));
    c=mix(c,i.border.rgb,mount*.6);
    // One quiet highlight follows the metal perimeter, including its uprights.
    let near=min(p,s-p);
    var along=p.x;
    if(near.x<near.y) {
        if(p.x<s.x*.5) { along=2.*s.x+2.*s.y-p.y; }
        else { along=s.x+p.y; }
    } else if(p.y>s.y*.5) { along=2.*s.x+s.y-p.x; }
    let shimmer=pow(max(0.,cos(along/(2.*s.x+2.*s.y)*6.283185-t)),32.);
    c+=i.border.rgb*(rim+fitting*.3)*(.025+.12*shimmer);
    // Light lives in the rose glass, rather than washing over the wine panel.
    let rose_light=disc(rose,37.)*flower.a;
    c+=i.accent.rgb*rose_light*(.012*(1.-cos(2.*t))+.10*exp(-i.line_age*4.));
    c*=.987+.025*grain(p);
    return vec4(c,aa(edge));
}
