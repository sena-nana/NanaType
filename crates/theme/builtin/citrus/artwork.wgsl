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
// SUNNY PRESS: soda label collage, oversized fruit print, mint diagonal wrap.
// One immutable, declared PNG: SVG master lives beside this program.
fn decal(p:vec2<f32>, origin:vec2<f32>, extent:vec2<f32>) -> vec4<f32> {
    let uv=(p-origin)/extent;
    if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
    return sample0(uv);
}
fn fizz(p:vec2<f32>, size:vec2<f32>, phase:f32, x:f32, r:f32, wobble:f32) -> f32 {
    let q=fract(phase);
    let v=p-vec2(size.x-x+wobble*sin(phase*6.283185),size.y+12.-q*(size.y+24.));
    let fade=smoothstep(15.,34.,p.y)*(1.-smoothstep(size.y-34.,size.y-15.,p.y));
    return ring(v,r,.55)*fade;
}
fn effect(i: EffectInput) -> vec4<f32> {
    let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
    let edge=rect(p-s*.5,s*.5-vec2(5.),15.);
    var c=i.base.rgb;
    let stripe=aa(47.-p.x-p.y*.13);
    c=mix(i.border.rgb,c,stripe);
    let art=aa(s.x-167.-p.x+p.y*.17);
    c=mix(c,i.border.rgb,art);
    let dots=disc(fract(p/9.)*9.-vec2(4.5),1.05)*art;
    c=mix(c,ink(48.,116.,100.),dots*.26);
    // Carbonation rises behind the fruit, never through the cream reading panel.
    let typing=4.*i.reveal*(1.-i.reveal);
    let bubbles=fizz(p,s,i.phase,25.,3.8,1.6)
        +fizz(p,s,i.phase+.37,78.,2.7,1.1)*.7
        +fizz(p,s,i.phase+.73,143.,2.,1.4)*.5;
    c=mix(c,i.base.rgb,bubbles*art*(.22+.06*typing));
    let fruit=decal(p,vec2(s.x-159.,s.y*.52-112.),vec2(160.,200.));
    c=mix(c,fruit.rgb,fruit.a);
    let sticker=rotate(p-vec2(s.x-143.,s.y-37.),-.22);
    c=mix(c,i.accent.rgb,aa(rect(sticker,vec2(45.,13.),3.)));
    let dash=line(fract((sticker.x+40.)/7.)*7.-3.5,.8)*aa(abs(sticker.y)-6.)*aa(abs(sticker.x)-34.);
    c=mix(c,i.base.rgb,dash*.8);
    c=mix(c,i.accent.rgb,segment(p,vec2(68.,52.),vec2(s.x-201.,52.),1.));
    let gleam=line(edge+5.,.6)*aa(p.y-s.y*.6);
    let wrap=max(art,1.-stripe);
    c=mix(c,i.base.rgb,gleam*(.66+.09*sin(t))*wrap);
    // A fine printed keyline follows the wrapper; perforations stop at its hem.
    c=mix(c,i.border.rgb,line(edge+1.,.45)*.58);
    let seam=max(line(p.x+p.y*.13-47.,.45),line(p.x-p.y*.17-s.x+167.,.45));
    let perforation=line(fract((p.y-13.)/8.)*8.-4.,1.5)*seam
        *aa(14.-p.y)*aa(p.y-s.y+14.);
    c=mix(c,ink(255.,252.,234.),perforation*.8);
    // The small punched tie nests beside the existing tilted name sticker.
    let tie=ring(p-vec2(60.,27.),2.,.55);
    let fold=segment(p,vec2(60.,31.),vec2(68.,52.),.4);
    c=mix(c,i.border.rgb,tie*.75+fold*.35);
    c*=.988+.02*grain(p);
    return vec4(c,aa(edge));
}
