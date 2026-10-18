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
// Discrete LCD activity cells, exactly three solid ink tones, no breathing.
 let edge=max(max(abs(q.x)-b.x,abs(q.y)-b.y),min(abs(q.x)-b.x+6.,abs(q.y)-b.y+6.));
 var c=rgb(155.,188.,15.);
 c=mix(c,rgb(15.,56.,15.),line(edge+1.8,1.8));
 c=mix(c,rgb(48.,98.,48.),line(edge+5.5,.9));
 let edge_shade=aa(11.-p.y)*aa(p.y-7.);
 c=mix(c,rgb(139.,172.,15.),edge_shade*.55);
 if details {
 let meter_p=p-select(vec2(s.x-41.,10.),vec2(40.,s.y-23.),portrait);
 let local=vec2(floor((24.-meter_p.x)/8.),meter_p.y);
 let meter=aa(abs(local.y)-1.)*aa(abs(fract((24.-meter_p.x)/8.)*8.-4.)-2.)*step(0.,local.x)*(1.-step(3.,local.x));
 let current=floor(fract(i.phase*4.)*3.);
 let on=1.-step(.1,abs(local.x-current));
 let ink=mix(rgb(139.,172.,15.),rgb(48.,98.,48.),on);
 c=mix(c,ink,meter);
 let bolt=max(aa(max(abs(p.x-11.),abs(p.y-s.y+11.))-2.),aa(max(abs(p.x-s.x+11.),abs(p.y-11.))-2.));
 c=mix(c,rgb(15.,56.,15.),bolt);
 }
 return vec4(c,aa(edge));
}
