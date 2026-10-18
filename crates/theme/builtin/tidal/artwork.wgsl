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
// PELAGIC GLASS: sculpted water edge, deep teal reading plane, medusa specimen.
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
    let wave=select(3.,1.4,compact)*sin(p.x*.045);
    let edge=rect(p-s*.5+vec2(0.,wave),s*.5-vec2(5.,9.),select(29.,12.,compact));
    var c=mix(ink(12.,44.,58.),i.base.rgb,i.uv.y);
    let panel=select(aa(s.y-158.-p.y),aa(s.x-24.-p.x),compact);
    let caustic=sin(p.x*.055+sin(p.y*.042+phase)*.9)+sin(p.y*.07-phase);
    c=mix(c,i.accent.rgb,line(caustic,.045)*.035*panel);
    let glass=line(edge+6.,.7); let inner=line(edge+11.,.45);
    c=mix(c,i.accent.rgb,glass*.72+inner*.22);
    let bevel=aa(edge+1.)*(1.-aa(edge+6.));
    c=mix(c,i.accent.rgb,bevel*(.08+.1*(.5+.5*sin(i.uv.x*4.2-i.uv.y*2.4+.5))));
    let bubble=p-vec2(s.x-26.+1.8*sin(phase),s.y-38.-12.*sin(phase));
    c=mix(c,i.border.rgb,ring(bubble,3.8,.55)*.23);
    c+=i.accent.rgb*glass*.025*(1.+sin(phase));
    return vec4(c,aa(edge));
}
fn effect(i: EffectInput) -> vec4<f32> {
    if(i.params0.y > .5 || i.params0.z > .5) { return responsive_print(i); }
    let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
    // Keep the glass silhouette aligned with the static fallback beneath it.
    // Reflections and caustics carry the motion without moving this alpha edge.
    let wave=3.*sin(i.uv.x*12.);
    let edge=rect(p-s*.5+vec2(0.,wave),s*.5-vec2(5.,9.),29.);
    var c=mix(ink(12.,44.,58.),i.base.rgb,i.uv.y);
    let v=p-vec2(s.x-93.,s.y*.49); let r=length(v);
    if(i.params0.x < .5) { c=mix(c,ink(34.,103.,116.),disc(v,94.)*.9); }
    // Integer temporal harmonics close the 16s loop; fade into the water panel.
    let caustic=sin(p.x*.055+sin(p.y*.042+t)*.9)+sin(p.y*.07-t);
    let water=smoothstep(s.x-205.,s.x-110.,p.x);
    c=mix(c,i.accent.rgb,line(caustic,.045)*.035*water);
    // A quick squeeze pushes water, the body follows with a short inertial lag,
    // and the bell opens slowly. All three motions share the same 4s stroke.
    let pivot=vec2(s.x-93.,s.y*.49-24.);
    let cycle=fract(i.phase*4.);
    let squeeze=smoothstep(.08,.30,cycle)*(1.-smoothstep(.30,.88,cycle));
    let push=smoothstep(.13,.43,cycle)*(1.-smoothstep(.43,1.,cycle));
    let bank=.16*sin(t)+.018*sin(2.*t);
    let old_bank=.16*sin(t-.28)+.018*sin(2.*t-.56);
    let drift=vec2(5.8*sin(t)-sin(bank)*8.5*push,-8.5*push-1.8*(1.-cos(t)));
    var local=rotate(p-pivot-drift,bank);
    let bell=1.-smoothstep(18.,48.,local.y);
    let tail=smoothstep(24.,112.,local.y);
    // The collar follows the bell, then the lag grows down the free tentacles.
    let drag=3.8*sin(4.*t-tail*1.6)+sin(8.*t-tail*2.3)
        +local.y*(sin(bank)-sin(old_bank));
    local.x=local.x*(1.+.18*squeeze*bell)+tail*drag;
    local.y=local.y*(1.-.10*squeeze*bell);
    let jelly=decal(local+pivot,vec2(s.x-163.4,s.y*.49-70.),vec2(140.8,176.));
    if(i.params0.x < .5) { c=mix(c,jelly.rgb,jelly.a); }
    let halo=ring(v,87.,.45)*aa(v.x+55.);
    if(i.params0.x < .5) { c=mix(c,i.accent.rgb,halo*.58); }
    // Bubbles fade before wrapping, and stay inside the ornamental water strip.
    let bubble=p-vec2(s.x-30.+1.8*sin(2.*t),s.y+12.-fract(i.phase+.3)*(s.y+24.));
    let bubble2=p-vec2(s.x-156.+1.2*sin(t),s.y+12.-fract(i.phase+.78)*(s.y+24.));
    let fade=smoothstep(15.,34.,p.y)*(1.-smoothstep(s.y-34.,s.y-15.,p.y));
    c=mix(c,i.border.rgb,(ring(bubble,3.8,.55)*.23+ring(bubble2,2.3,.45)*.15)*fade);
    let glass=line(edge+6.,.7); let inner=line(edge+11.,.45);
    c=mix(c,i.accent.rgb,glass*.72+inner*.22);
    // The rounded glass lip catches light at the upper left and lower right.
    let bevel=aa(edge+1.)*(1.-aa(edge+6.));
    let reflection=.5+.5*sin(i.uv.x*4.2-i.uv.y*2.4+.5);
    c=mix(c,i.accent.rgb,bevel*(.08+.1*reflection));
    let glint=line(edge+8.,.35)*pow(max(0.,cos(i.uv.x*5.-i.uv.y*3.)),12.);
    c=mix(c,i.border.rgb,glint*(.44+.08*sin(t)));
    let header=segment(p,vec2(37.,52.),vec2(s.x-208.,52.),.55);
    c=mix(c,i.accent.rgb,header*.5);
    // Pearl mount: a small open cradle echoes the name capsule and water halo.
    let cradle=ring(p-vec2(53.,27.),23.,.5)*aa(p.x-38.)*aa(19.-p.y);
    c=mix(c,i.accent.rgb,cradle*.65);
    c=mix(c,i.border.rgb,disc(p-vec2(s.x-208.,52.),1.5)*.6);
    let ripple=ring(p-vec2(45.,36.),i.line_age*160.,1.4)*exp(-i.line_age*3.);
    c+=i.accent.rgb*ripple*(glass*.12+header*.08+halo*.06);
    c+=i.accent.rgb*glass*.025*(1.+sin(t));
    return vec4(c,aa(edge));
}
