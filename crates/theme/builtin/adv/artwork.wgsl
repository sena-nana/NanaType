// Original NanaType plate: stationary reading field and silhouette.
// Animate the skin's own objects, never a generic edge sweep or the text.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn line(d:f32,w:f32)->f32 { return aa(abs(d)-w); }
fn disc(p:vec2<f32>,r:f32)->f32 { return aa(length(p)-r); }
fn ring(p:vec2<f32>,r:f32,w:f32)->f32 { return line(length(p)-r,w); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn segment(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>,w:f32)->f32 {
 let v=b-a; return aa(length(p-a-v*clamp(dot(p-a,v)/max(dot(v,v),.01),0.,1.))-w);
}
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
fn grain(p:vec2<f32>)->f32 { return fract(sin(dot(floor(p),vec2(12.9898,78.233)))*43758.5453); }
fn decal(p:vec2<f32>,origin:vec2<f32>,extent:vec2<f32>)->vec4<f32> {
 let uv=(p-origin)/extent;
 if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
 return sample0(uv);
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
 let q=p-s*.5; let b=s*.5-vec2(1.);
 let opening=exp(-i.line_age*3.); let speaker=exp(-i.speaker_age*3.);
// Cinema matte: a fixed brass name baseline, no ambient animation.
 let edge=max(rect(q,b,6.),abs(q.x)+abs(q.y)-b.x-b.y+12.);
 var c=mix(rgb(11.,12.,20.),rgb(25.,26.,36.),1.-i.uv.y);
 c=mix(c,rgb(106.,106.,121.),line(edge+1.,.4)*.65);
 let header=segment(p,vec2(42.,45.),vec2(min(s.x-70.,s.x*.53),45.),.45);
 c=mix(c,rgb(207.,177.,115.),header*.55);
 let diamond=aa(abs(p.x-41.)+abs(p.y-45.)-2.7);
 c=mix(c,rgb(236.,211.,162.),diamond*.85);
 c=mix(c,rgb(207.,177.,115.),max(segment(p,vec2(13.,13.),vec2(26.,13.),.7),segment(p,vec2(s.x-13.,s.y-24.),vec2(s.x-13.,s.y-13.),.7))*.6);
 return vec4(c,aa(edge)*.96);
}
