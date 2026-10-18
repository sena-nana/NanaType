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
// A soft heart sticker squashes and rocks; its ink and paper move together.
 let edge=rect(q,b,29.);
 var c=rgb(255.,255.,255.);
 c=mix(c,rgb(255.,232.,241.),line(edge+6.,1.9)*.75);
 c=mix(c,rgb(43.,43.,58.),line(edge+1.6,1.65));
 if details {
 let bounce=pow(max(0.,sin(t*2.)),6.);
 let angle=.10*sin(t*2.); let ca=cos(angle); let sa=sin(angle);
 let heart_origin=select(vec2(s.x-38.,19.),vec2(38.,s.y-26.),portrait);
 let v=p-heart_origin+vec2(0.,2.8*bounce);
 let h=vec2(ca*v.x-sa*v.y,sa*v.x+ca*v.y)/vec2(1.+.10*bounce,1.-.12*bounce);
 let heart=min(min(length(h-vec2(-2.4,-1.8))-3.6,length(h-vec2(2.4,-1.8))-3.6),(abs(h.x)+abs(h.y-1.1)-5.1)*.707);
 c=mix(c,rgb(106.,36.,69.),aa(heart-1.1));
 c=mix(c,rgb(255.,151.,185.),aa(heart));
 c=mix(c,rgb(255.,246.,250.),segment(h,vec2(-3.,-3.),vec2(-1.8,-3.7),.55));
 let peel=segment(p-heart_origin,vec2(12.,5.),vec2(19.,-1.),.7);
 c=mix(c,rgb(80.,169.,229.),peel*.75);
 let bridge=segment(p,select(vec2(25.,8.),vec2(25.,s.y-37.),portrait),select(vec2(25.,25.),vec2(25.,s.y-20.),portrait),1.1);
 c=mix(c,rgb(255.,122.,168.),bridge*.8);
 }
 return vec4(c,aa(edge));
}
