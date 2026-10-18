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
// Inked panel: corner screen tone is trimmed by the panel and left clear of text.
 let edge=max(rect(q,b,3.),abs(q.x)+abs(q.y)-b.x-b.y+8.);
 var c=rgb(255.,255.,255.);
 let cell=fract(p/5.)*5.-2.5;
 let upper=clamp(1.-((s.x-p.x)+p.y*.7)/88.,0.,1.);
 let upper_margin=max(1.-smoothstep(23.,29.,p.y),smoothstep(s.x-25.,s.x-19.,p.x));
 let lower=clamp(1.-(p.x+(s.y-p.y)*.9)/68.,0.,1.);
 let lower_margin=max(1.-smoothstep(17.,23.,p.x),smoothstep(s.y-18.,s.y-12.,p.y));
 let dots=disc(cell,.65+upper*.85)*upper*upper_margin;
 let low_dots=disc(cell,.8+lower*.45)*lower*lower_margin;
 c=mix(c,rgb(17.,17.,17.),max(dots*.84,low_dots*.58));
 c=mix(c,rgb(0.,0.,0.),line(edge+1.8,1.8));
 return vec4(c,aa(edge));
}
