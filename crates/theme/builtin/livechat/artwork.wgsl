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
 // Portrait/compact instances reuse the original ink and material in logical pixels.
 let portrait=i.params0.x>.5 && i.params0.x<1.5;
 let details=i.params0.y<.5;
 let q=p-s*.5; let b=s*.5-vec2(1.);
 let opening=exp(-i.line_age*3.); let speaker=exp(-i.speaker_age*3.);
// Broadcast card: a shared cyan header rail joins avatar, name and on-air badge.
 let edge=rect(q,b,17.);
 var c=rgb(255.,255.,255.);
 c=mix(c,rgb(224.,243.,250.),line(edge+1.,.6)*.9);
 c=mix(c,rgb(238.,249.,252.),select(aa(p.y-37.),aa(s.y-58.-p.y),portrait));
 let rail_y=select(38.,s.y-58.,portrait);
 let rail=segment(p,vec2(20.,rail_y),vec2(s.x-20.,rail_y),.45);
 c=mix(c,rgb(35.,173.,229.),rail*.23);
 if details {
 // Keep status below LIVE, clear of an unframed long name's ellipsis.
 let typing=1.-step(.999,i.reveal);
 let d0=disc(p-vec2(s.x-47.,select(35.,s.y-14.,portrait)-typing*1.6*max(0.,sin(t*4.))),1.5);
 let d1=disc(p-vec2(s.x-40.,select(35.,s.y-14.,portrait)-typing*1.6*max(0.,sin(t*4.-1.2))),1.5);
 let d2=disc(p-vec2(s.x-33.,select(35.,s.y-14.,portrait)-typing*1.6*max(0.,sin(t*4.-2.4))),1.5);
 c=mix(c,rgb(35.,151.,185.),max(d0,max(d1,d2))*.72);
 }
 return vec4(c,aa(edge));
}
