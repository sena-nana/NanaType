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
// A branch flexes about its attachment; the paper and pink rim stay still.
 let edge=rect(q,b,23.);
 var c=mix(rgb(255.,244.,248.),rgb(253.,228.,238.),i.uv.y);
 c=mix(c,rgb(230.,150.,180.),line(edge+1.,.6)*.9);
 c=mix(c,rgb(255.,254.,251.),line(edge+5.,.55)*.9);
 let hinge=vec2(s.x-8.,29.); let v=p-hinge;
 let angle=.020*sin(t)+.006*sin(t*2.);
 let bent=hinge+vec2(cos(angle)*v.x-sin(angle)*v.y,sin(angle)*v.x+cos(angle)*v.y);
 let branch=decal(bent,vec2(s.x-134.,0.),vec2(128.,32.));
 c=mix(c,branch.rgb,branch.a);
 return vec4(c,aa(edge));
}
