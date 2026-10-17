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
// Upright stationery: a vertical blue spine and level editorial rules.
 let edge=rect(q,b,select(0.,6.,p.x>s.x*.5));
 var c=mix(rgb(237.,243.,255.),rgb(255.,255.,255.),smoothstep(0.,.38,i.uv.x));
 c=mix(c,rgb(74.,125.,255.),aa(p.x-5.));
 c=mix(c,rgb(215.,226.,250.),segment(p,vec2(26.,s.y-12.),vec2(s.x-22.,s.y-12.),.4)*.8);
 let registration=max(segment(p,vec2(s.x-59.,s.y-12.),vec2(s.x-32.,s.y-12.),.65),segment(p,vec2(s.x-25.,s.y-12.),vec2(s.x-21.,s.y-12.),.65));
 c=mix(c,rgb(74.,125.,255.),registration*.65);
 return vec4(c,aa(edge));
}
