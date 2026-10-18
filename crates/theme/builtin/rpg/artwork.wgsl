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
// Enamel game window: stepped corners, three bevel bands and inset rivets.
 let edge=max(max(abs(q.x)-b.x,abs(q.y)-b.y),min(abs(q.x)-b.x+6.,abs(q.y)-b.y+6.));
 var c=mix(rgb(12.,23.,89.),rgb(42.,76.,159.),1.-i.uv.y);
 c=mix(c,rgb(244.,247.,255.),line(edge+1.4,1.3));
 c=mix(c,rgb(111.,158.,230.),line(edge+4.6,.55));
 c=mix(c,rgb(5.,12.,52.),line(edge+7.,1.));
 let top=segment(p,vec2(22.,11.),vec2(s.x-22.,11.),.45);
 c=mix(c,rgb(121.,174.,240.),top*.65);
 let studs=max(aa(max(abs(p.x-12.),abs(p.y-s.y+12.))-2.),aa(max(abs(p.x-s.x+12.),abs(p.y-12.))-2.));
 c=mix(c,rgb(225.,238.,255.),studs*.8);
 if details {
 // The READY crystal belongs to the game's top-right bezel. It only
 // takes two pixel-grid positions while waiting for the next page.
 let bob=select(0.,2.*step(0.,sin(t*4.)),i.waiting>.5);
 let crystal=p-select(vec2(s.x-36.,13.+bob),vec2(25.,s.y-25.+bob),portrait);
 let diamond=aa(abs(crystal.x)+abs(crystal.y)-4.5);
 c=mix(c,rgb(168.,205.,255.),diamond*.95);
 c=mix(c,rgb(244.,247.,255.),diamond*step(crystal.x,0.)*.6);
 }
 return vec4(c,aa(edge));
}
