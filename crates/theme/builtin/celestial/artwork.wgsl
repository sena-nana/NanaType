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
// LUNAR ATLAS: offset astrolabe and engraved coordinates, open night sky.
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
    let edge=rect(p-s*.5,s*.5-vec2(3.),select(22.,12.,compact));
    var c=mix(ink(17.,20.,43.),i.base.rgb,i.uv.x);
    c=mix(c,i.border.rgb,line(edge+8.,.5)*.56);
    c=mix(c,i.accent.rgb,line(edge+3.,.4)*.17);
    let cells=vec2(147.,91.); let cell=floor(p/cells); let local=fract(p/cells)*cells;
    let star=local-vec2(36.+31.*sin(cell.y*3.),24.+17.*cos(cell.x*7.));
    let spark=max(aa(abs(star.x)+abs(star.y)-2.4),disc(star,1.));
    c=mix(c,i.border.rgb,spark*(.42+.06*sin(phase+cell.x)));
    if !compact {
        let grid=aa(s.y-158.-p.y)*max(line(p.y-s.y+86.,.3),line(p.x-70.,.3));
        c=mix(c,i.accent.rgb,grid*.2);
        let ticks=line(fract((p.y-35.)/18.)*18.-9.,.35)*aa(abs(p.x-s.x+15.)-2.);
        c=mix(c,i.border.rgb,ticks*.35);
    }
    c*=.992+.015*grain(p);
    return vec4(c,aa(edge));
}
fn effect(i: EffectInput) -> vec4<f32> {
    if(i.params0.y > .5 || i.params0.z > .5) { return responsive_print(i); }
    let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
    let edge=rect(p-s*.5,s*.5-vec2(3.),22.);
    var c=mix(ink(17.,20.,43.),i.base.rgb,i.uv.x);
    let center=vec2(s.x-87.,s.y*.5); let v=p-center; let r=length(v);
    let a=atan2(v.y,v.x);
    // Responsive layouts place the subject in an independent aspect-locked layer.
    if(i.params0.x < .5) {
    c=mix(c,ink(52.,43.,82.),disc(v,111.));
    c=mix(c,i.accent.rgb,disc(v,93.)*.08);
    let instrument=decal(p,vec2(s.x-203.,s.y*.5-145.),vec2(232.,290.));
    c=mix(c,instrument.rgb,instrument.a);
    // The engraved plate stays still. A fine orbital ring precesses over it.
    let orbital=rotate(v,.42+.10*sin(t));
    let ellipse=vec2(orbital.x,orbital.y/.46);
    let orbit=ring(ellipse,91.,.55)*smoothstep(s.x-210.,s.x-196.,p.x);
    c=mix(c,i.accent.rgb,orbit*.22);
    let star_pos=rotate(vec2(cos(-t)*91.,sin(-t)*91.*.46),-.42-.10*sin(t));
    c=mix(c,i.border.rgb,disc(v-star_pos,1.8)*.8);
    let trail=ring(ellipse,91.,.65)*pow(max(0.,cos(atan2(ellipse.y,ellipse.x)+t)),28.);
    c+=i.border.rgb*trail*.07;
    }
    let header=segment(p,vec2(35.,51.),vec2(s.x-217.,51.),.5);
    c=mix(c,i.accent.rgb,header*.45);
    let grid=step(s.x-205.,p.x)*max(line(p.y-s.y*.5,.3),line(p.x-center.x,.3));
    c=mix(c,i.accent.rgb,grid*.2);
    let stars=vec2(147.,91.); let cell=floor(p/stars); let local=fract(p/stars)*stars;
    let star=local-vec2(36.+31.*sin(cell.y*3.),24.+17.*cos(cell.x*7.));
    let twinkle=.46+.10*sin(t+dot(cell,vec2(1.7,3.1)));
    let spark=max(aa(abs(star.x)+abs(star.y)-2.4),disc(star,1.));
    c=mix(c,i.border.rgb,spark*twinkle*(1.-aa(p.x-s.x+210.)));
    c=mix(c,i.border.rgb,line(edge+8.,.5)*.56);
    // A second hairline and sparse calibration marks share the atlas engraving.
    c=mix(c,i.accent.rgb,line(edge+3.,.4)*.17);
    let ticks=line(fract((p.x-35.)/18.)*18.-9.,.35)
        *aa(abs(p.y-s.y+15.)-2.)*aa(35.-p.x)*aa(p.x-s.x+217.);
    c=mix(c,i.border.rgb,ticks*.35);
    let terminal=aa(abs(p.x-s.x+217.)+abs(p.y-51.)-2.8);
    c=mix(c,i.border.rgb,terminal*.7);
    // The top-edge badge seats into a short engraved bracket, rather than floats.
    let seat=max(segment(p,vec2(29.,12.),vec2(29.,32.),.45),
        segment(p,vec2(29.,32.),vec2(41.,32.),.45));
    c=mix(c,i.border.rgb,seat*.5);
    let opening=.65*(1.-exp(-i.line_age*8.));
    let arc=ring(v,117.,.9)*aa(abs(a+.6)-opening)*exp(-i.line_age*3.);
    c+=i.accent.rgb*arc*.32*smoothstep(s.x-211.,s.x-198.,p.x);
    c*=.992+.015*grain(p);
    return vec4(c,aa(edge));
}
