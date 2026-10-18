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
// VERMILION IMPRESSIONS: letterpress band, gilt window lattice, suspended lantern.
// One immutable, declared PNG: SVG master lives beside this program.
fn decal(p:vec2<f32>, origin:vec2<f32>, extent:vec2<f32>) -> vec4<f32> {
    let uv=(p-origin)/extent;
    if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
    return sample0(uv);
}
// Recompose the original print vocabulary in logical pixels; no scaled canvas.
fn responsive_print(i: EffectInput) -> vec4<f32> {
    let p=i.uv*i.size; let s=i.size; let compact=i.params0.z > .5;
    let phase=i.phase*6.283185;
    let edge=max(max(3.-p.x,p.x-s.x+3.),max(3.-p.y,p.y-s.y+3.));
    var c=i.base.rgb;
    c=mix(c,i.accent.rgb,aa(p.x-select(22.,10.,compact)));
    let panel=select(aa(s.y-155.-p.y),aa(s.x-18.-p.x),compact);
    c=mix(c,ink(236.,217.,180.),panel);
    let diamond=abs(fract((p.x+p.y)/36.)*36.-18.);
    let cross=abs(fract((p.x-p.y)/36.)*36.-18.);
    let lattice=max(line(diamond,.55),line(cross,.55))*panel;
    c=mix(c,i.border.rgb,lattice*.6);
    let near=min(p,s-p);
    let frame=max(line(near.x-11.,.5),line(near.y-11.,.55));
    c=mix(c,i.border.rgb,frame*.65);
    let joint=max(segment(near,vec2(0.,5.),vec2(12.,5.),.6),segment(near,vec2(5.,0.),vec2(5.,12.),.6));
    c=mix(c,i.border.rgb,joint*.6);
    c+=i.border.rgb*frame*.02*(1.+sin(phase));
    if !compact {
        let seal=(p-vec2(12.,s.y-27.))/.65;
        c=mix(c,i.base.rgb,line(rect(seal,vec2(9.,12.),.5),.8));
        c=mix(c,i.base.rgb,line(rect(seal,vec2(4.,7.),.2),.65));
    }
    c*=.978+.035*grain(p);
    return vec4(c,aa(edge));
}
fn effect(i: EffectInput) -> vec4<f32> {
    if(i.params0.y > .5 || i.params0.z > .5) { return responsive_print(i); }
    let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
    let edge=max(max(3.-p.x,p.x-s.x+3.),max(3.-p.y,p.y-s.y+3.));
    var c=i.base.rgb;
    c=mix(c,i.accent.rgb,aa(p.x-41.));
    c=mix(c,ink(236.,217.,180.),aa(s.x-125.-p.x));
    let diamond=abs(fract((p.x+p.y)/36.)*36.-18.);
    let cross=abs(fract((p.x-p.y)/36.)*36.-18.);
    let lattice=max(line(diamond,.55),line(cross,.55))*aa(s.x-125.-p.x);
    c=mix(c,i.border.rgb,lattice*.6);
    // Suspend both the painted cord and lamp from the fixed top rail.
    let pivot=vec2(s.x-87.,9.);
    var lamp_p=pivot+rotate(p-pivot,.015*sin(t)+.005*sin(2.*t));
    // Keep the collars, bead and knot rigid; only the free silk threads bend.
    let tassel=smoothstep(s.y*.43+63.,s.y*.43+103.,lamp_p.y);
    let lag=1.5*(sin(t-.55)-sin(-.55))
        +.5*(sin(2.*t-tassel*.9)-sin(-tassel*.9));
    lamp_p.x+=tassel*lag;
    let lamp=decal(lamp_p,vec2(s.x-157.,s.y*.43-72.),vec2(140.,175.));
    let glow=(p-vec2(s.x-87.,s.y*.43+5.))/vec2(45.,65.);
    c+=i.border.rgb*exp(-dot(glow,glow)*2.)*.009*(.65+.35*sin(2.*t))
        *smoothstep(s.x-153.,s.x-134.,p.x);
    if(i.params0.x < .5) { c=mix(c,lamp.rgb,lamp.a); }
    if(i.params0.x < .5) { c=mix(c,i.accent.rgb,segment(lamp_p,vec2(s.x-87.,9.),vec2(s.x-87.,s.y*.43-54.),.7)); }
    let seal=p-vec2(20.,s.y-27.);
    c=mix(c,i.base.rgb,line(rect(seal,vec2(9.,12.),.5),.8));
    let sealglyph=line(rect(seal,vec2(4.,7.),.2),.65);
    c=mix(c,i.base.rgb,sealglyph);
    c=mix(c,i.accent.rgb,segment(p,vec2(65.,52.),vec2(s.x-154.,52.),.8));
    // The window lattice and reading panel share a continuous gilt timber frame.
    let top=line(p.y-11.,.55)*aa(51.-p.x)*aa(p.x-s.x+12.);
    c=mix(c,i.border.rgb,top*.8);
    c=mix(c,i.border.rgb,line(p.y-s.y+12.,.5)*aa(51.-p.x)*aa(p.x-s.x+12.)*.65);
    let upright=max(line(p.x-51.,.45),line(p.x-s.x+12.,.45))
        *aa(11.-p.y)*aa(p.y-s.y+12.);
    c=mix(c,i.border.rgb,upright*.42);
    let corner=vec2(min(p.x-51.,s.x-12.-p.x),min(p.y-11.,s.y-12.-p.y));
    let joint=max(segment(corner,vec2(0.,5.),vec2(12.,5.),.6),
        segment(corner,vec2(5.,0.),vec2(5.,12.),.6));
    c=mix(c,i.border.rgb,joint*.6);
    // Letterpress spine and plaque bracket, cut from the same warm gold.
    let spine=line(p.x-9.,.45)*aa(12.-p.y)*aa(p.y-s.y+48.);
    let bracket=max(segment(p,vec2(58.,12.),vec2(58.,52.),.55),
        segment(p,vec2(58.,28.),vec2(66.,28.),.55));
    c=mix(c,i.border.rgb,spine*.48+bracket*.7);
    c*=.978+.035*grain(p);
    if(i.params0.x < .5) { c+=i.border.rgb*lamp.a*.012*(.5+.5*sin(2.*t)); }
    let travel=pow(max(0.,cos((p.x-51.)/(s.x-63.)*6.283185-t)),24.);
    c+=i.border.rgb*(top+lattice*.12)*travel*.035;
    c+=i.border.rgb*(bracket+top)*.1*exp(-i.speaker_age*4.);
    return vec4(c,aa(edge));
}
