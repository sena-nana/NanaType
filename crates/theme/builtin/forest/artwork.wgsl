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
// HERBARIUM: uncoated paper, split botanical plate, stitched book binding.
fn leaf(p: vec2<f32>, a: f32, size: vec2<f32>) -> f32 {
    let v=rotate(p,a)/size;
    return aa((length(v+vec2(.48,0.))+length(v-vec2(.48,0.))-2.)*min(size.x,size.y));
}
// One immutable, declared PNG: SVG master lives beside this program.
fn decal(p:vec2<f32>, origin:vec2<f32>, extent:vec2<f32>) -> vec4<f32> {
    let uv=(p-origin)/extent;
    if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
    return sample0(uv);
}
fn effect(i: EffectInput) -> vec4<f32> {
    let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
    let edge=max(max(3.-p.x,p.x-s.x+3.),max(3.-p.y,p.y-s.y+3.));
    var c=i.base.rgb;
    let art=aa(s.x-158.-p.x);
    c=mix(c,ink(218.,226.,204.),art);
    c=mix(c,ink(232.,226.,204.),aa(p.x-29.));
    c=mix(c,i.border.rgb,line(p.x-29.,.55)*.4);
    let holes=line(fract((p.y-11.)/18.)*18.-9.,1.9)*line(p.x-14.,1.8);
    c=mix(c,i.border.rgb,holes*.7);
    // The pressed stem stays planted; only the flexible upper fronds bend.
    let root=vec2(s.x-114.,s.y*.5+81.);
    var leaf_p=p-root;
    let bend=smoothstep(8.,174.,-leaf_p.y);
    let tip=bend*smoothstep(8.,30.,abs(leaf_p.x+.32*leaf_p.y));
    leaf_p.x+=bend*(2.7*sin(t)+.5*(sin(2.*t+leaf_p.y*.025)-sin(leaf_p.y*.025)));
    // Small lateral pinnae flex after the rachis; the binding stays planted.
    leaf_p.y+=.45*bend*sin(2.*t)
        +tip*.65*(sin(2.*t+leaf_p.x*.055)-sin(leaf_p.x*.055));
    let specimen=decal(leaf_p+root,vec2(s.x-155.,s.y*.5-94.),vec2(150.,187.5));
    c=mix(c,specimen.rgb,specimen.a);
    let stamp=p-vec2(s.x-132.,s.y-29.);
    c=mix(c,i.border.rgb,ring(stamp,12.,.6)*.48);
    c=mix(c,i.border.rgb,segment(stamp,vec2(-6.,0.),vec2(6.,0.),.45)*.5);
    let rule=segment(p,vec2(55.,51.),vec2(s.x-181.,51.),.5);
    c=mix(c,i.border.rgb,rule*.45);
    let fold=line(p.x-s.x+158.,.45)*aa(11.-p.y)*aa(p.y-s.y+11.);
    c=mix(c,i.border.rgb,fold*.24);
    // A pressed paper rule ties the binding and specimen panel into one sheet.
    c=mix(c,i.border.rgb,line(edge+1.,.45)*.23);
    c=mix(c,ink(255.,252.,241.),line(edge+2.5,.4)*.65);
    let corner=vec2(min(p.x-36.,s.x-10.-p.x),min(p.y-11.,s.y-12.-p.y));
    let registration=max(segment(corner,vec2(0.,0.),vec2(10.,0.),.4),
        segment(corner,vec2(0.,0.),vec2(0.,10.),.4));
    c=mix(c,i.border.rgb,registration*.32);
    let footer=segment(p,vec2(46.,s.y-13.),vec2(s.x-172.,s.y-13.),.35);
    c=mix(c,i.border.rgb,footer*.19);
    // A short book-label rule shares the binding ink, with no added colour block.
    let label=segment(p,vec2(47.,20.),vec2(47.,40.),.8);
    c=mix(c,i.border.rgb,label*.58);
    c*=.975+.04*grain(p);
    let fly=p-vec2(s.x-29.+4.*sin(t),s.y*.7-6.*sin(2.*t));
    let fly2=p-vec2(s.x-141.+3.*sin(2.*t),s.y*.25+4.*sin(t));
    let breath=.3+.07*sin(2.*t)+i.waiting*.12;
    c=mix(c,ink(245.,220.,146.),(disc(fly,1.15)+disc(fly2,.85)*.6)*breath);
    c+=ink(245.,220.,146.)*(exp(-dot(fly,fly)/24.)+exp(-dot(fly2,fly2)/18.)*.5)*.012;
    c=mix(c,i.accent.rgb,label*i.waiting*(.05+.025*sin(2.*t)));
    return vec4(c,aa(edge));
}
