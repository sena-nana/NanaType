// Vanilla: a Swiss editorial plate. Warm white paper, square corners, a
// vanilla-bean spine on the left and a hairline edge. Static by design.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn line(d:f32,w:f32)->f32 { return aa(abs(d)-w); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size;
 // params0.x: 1 portrait, 2 compact; compact and minimal plates use a 4px spine.
 let compact=i.params0.x>1.5;
 let edge=rect(p-s*.5,s*.5-vec2(.5),2.);
 var c=mix(rgb(255.,253.,248.),rgb(251.,248.,241.),i.uv.y);
 c=mix(c,rgb(36.,32.,27.),line(edge+1.,.5)*.12);
 c=mix(c,rgb(59.,42.,32.),aa(p.x-select(6.,4.,compact)));
 return vec4(c,aa(edge));
}
