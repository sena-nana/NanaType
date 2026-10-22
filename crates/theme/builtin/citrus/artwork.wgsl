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
fn ink(r: f32, g: f32, b: f32) -> vec3<f32> { return pow(vec3(r,g,b)/255., vec3(2.2)); }
fn grain(p: vec2<f32>) -> f32 { return fract(sin(dot(floor(p),vec2(12.9898,78.233)))*43758.5453); }
fn rotate(p: vec2<f32>, a: f32) -> vec2<f32> {
    return vec2(cos(a)*p.x-sin(a)*p.y, sin(a)*p.x+cos(a)*p.y);
}
// SODA LABEL: a cream wrapper label, cut on the diagonal at the right, over a
// mint halftone can; a lemon slice bleeds off the top-right corner.
// One immutable, declared PNG: SVG master lives beside this program.
fn decal(p:vec2<f32>, origin:vec2<f32>, extent:vec2<f32>) -> vec4<f32> {
    let uv=(p-origin)/extent;
    if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
    return sample0(uv);
}
fn over(d: vec4<f32>, c: vec3<f32>, k: f32) -> vec4<f32> {
    let a = k + d.a*(1.-k);
    return vec4((c*k + d.rgb*d.a*(1.-k))/max(a, 1e-4), a);
}
// Halftone on a 30-degree screen; `r` is the dot radius.
fn dots(p: vec2<f32>, cell: f32, r: f32) -> f32 {
    let q = rotate(p, .5236);
    return disc(fract(q/cell)*cell-vec2(cell*.5), r);
}
fn fizz(p:vec2<f32>, s:vec2<f32>, phase:f32, x:f32, r:f32, wobble:f32) -> vec2<f32> {
    let q=fract(phase);
    let v=p-vec2(s.x-x+wobble*sin(phase*6.283185), s.y+10.-q*(s.y+20.));
    let fade=smoothstep(22.,46.,p.y)*(1.-smoothstep(s.y-30.,s.y-12.,p.y));
    return vec2(disc(v,r)*.25, ring(v,r,.75)*.85)*fade;
}
// Compact plate: same label and halftone, no fruit; portrait cuts the bottom.
fn responsive_print(i: EffectInput) -> vec4<f32> {
    let p=i.uv*i.size; let s=i.size; let compact=i.params0.z > .5;
    let k=clamp(min(s.x,s.y)/160.,.6,1.);
    let can=rect(p-s*.5, s*.5-vec2(4.*k), 10.*k);
    let mint=ink(124.,196.,174.);
    var c=vec4(mint, aa(can));
    c=vec4(mix(c.rgb, ink(79.,164.,138.), dots(p,7.,1.4)*.45), c.a);
    let box=rect(p-s*.5, s*.5-vec2(10.*k), 4.*k);
    // Landscape: the right edge is cut on the diagonal. Portrait: the bottom
    // edge is, leaving a strip of can for the fruit decoration.
    let n=select(vec2(0.2425,.9701), vec2(.9701,.2425), compact);
    let cut_at=select(vec2(0., s.y-min(150.,s.y*.3)), vec2(s.x-24.*k, 0.), compact);
    let cut=dot(p-cut_at, n);
    c=vec4(mix(c.rgb, ink(255.,201.,60.), aa(max(box, cut-6.*k))*step(0.,cut)), c.a);
    c=vec4(mix(c.rgb, ink(255.,246.,214.), aa(max(box, cut))), c.a);
    return vec4(c.rgb*(.99+.015*grain(p)), c.a);
}
fn effect(i: EffectInput) -> vec4<f32> {
    if(i.params0.y > .5 || i.params0.z > .5) { return responsive_print(i); }
    let p=i.uv*i.size; let s=i.size;
    // The can sits 14px down, leaving headroom for the tilted sticker.
    let mid=vec2(s.x*.5, (14.+s.y-6.)*.5);
    let hb=vec2(s.x*.5-6., (s.y-20.)*.5);
    let can=rect(p-mid, hb, 14.);
    let inside=aa(can);
    let mint=ink(124.,196.,174.);
    var c=vec4(0.);
    c=over(c, ink(47.,110.,92.), .25*(1.-smoothstep(-3.,10.,rect(p-mid-vec2(0.,4.), hb, 14.))));
    c=over(c, mint, inside);
    let fade=mix(mix(.15,.35,clamp(p.x/s.x/.7,0.,1.)), .75, clamp((p.x/s.x-.7)/.3,0.,1.));
    c=vec4(mix(c.rgb, ink(79.,164.,138.), dots(p,9.,1.9)*fade), c.a);
    // The cream label, cut on a fixed diagonal measured from the right edge.
    let n=vec2(.9207,.3903);
    let cut=dot(p-vec2(s.x-148.,28.), n);
    let lmid=vec2(s.x*.5, (28.+s.y-18.)*.5);
    let lhb=vec2(s.x*.5-22., (s.y-46.)*.5);
    let label=max(rect(p-lmid, lhb, 8.), cut);
    let shade=max(rect(p-lmid-vec2(2.,3.), lhb, 8.), cut-2.*n.x-3.*n.y);
    c=vec4(mix(c.rgb, ink(47.,110.,92.), .3*(1.-smoothstep(-2.,3.,shade))*inside), c.a);
    let band=aa(max(rect(p-lmid, lhb+vec2(40.,0.), 0.), cut-11.))*step(0.,cut);
    c=vec4(mix(c.rgb, ink(255.,201.,60.), band), c.a);
    let paper=mix(ink(255.,249.,228.), ink(255.,246.,214.), clamp((p.y-28.)/(s.y-46.),0.,1.));
    c=vec4(mix(c.rgb, paper*(.99+.015*grain(p)), aa(label)), c.a);
    let key=max(rect(p-lmid, lhb-vec2(6.), 6.), cut+9.);
    c=vec4(mix(c.rgb, mint, line(key,.75)*.7), c.a);
    // Carbonation rises in the can, right of the cut, behind the fruit.
    // It never reaches the label, so the reading area stays still.
    let t=i.phase;
    let b=fizz(p,s,t,114.,7.,1.6)+fizz(p,s,t+.37,84.,4.5,1.1)+fizz(p,s,t+.71,56.,3.,1.4);
    let can_only=smoothstep(14.,18.,cut)*aa(can+3.);
    c=vec4(mix(c.rgb, vec3(1.), clamp(b.x+b.y,0.,1.)*can_only), c.a);
    // The lemon slice: rotated 200 degrees, scaled 1.2, bled off the corner.
    let q=rotate(p-vec2(s.x-38.,34.), -3.4907)/1.2+vec2(128.,181.);
    let fruit=decal(q, vec2(0.), vec2(256.,320.));
    let qs=rotate(p-vec2(s.x-36.5,37.), -3.4907)/1.2+vec2(128.,181.);
    let drop=decal(qs, vec2(0.), vec2(256.,320.)).a;
    c=vec4(mix(c.rgb, ink(74.,55.,32.), .3*drop*inside), c.a);
    c=vec4(mix(c.rgb, fruit.rgb, fruit.a*inside), c.a);
    return c;
}
