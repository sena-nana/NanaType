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
// A four-state command spinner in the title bar, no travelling phosphor.
 let edge=max(rect(q,b,2.),abs(q.x)+abs(q.y)-b.x-b.y+5.);
 var c=rgb(9.,16.,11.);
 c=mix(c,rgb(21.,41.,27.),select(aa(p.y-24.),aa(s.y-36.-p.y),portrait));
 c=mix(c,rgb(46.,100.,60.),line(edge+1.,.55)*.9);
 c=mix(c,rgb(47.,105.,62.),line(p.y-select(24.,s.y-36.,portrait),.45));
 if details {
 let bar_y=select(12.,s.y-23.,portrait);
 let dot1=disc(p-vec2(15.,bar_y),3.); let dot2=ring(p-vec2(28.,bar_y),2.5,.6); let dot3=ring(p-vec2(41.,bar_y),2.5,.6);
 c=mix(c,rgb(75.,218.,111.),dot1*.75);
 c=mix(c,rgb(76.,136.,89.),max(dot2,dot3)*.9);
 let state=floor(fract(i.phase*8.)*4.);
 let v=p-vec2(62.,bar_y);
 var cursor=segment(v,vec2(-3.,-3.),vec2(3.,3.),.65);
 if(state<1.) { cursor=segment(v,vec2(0.,-4.),vec2(0.,4.),.65); }
 else if(state<2.) { cursor=segment(v,vec2(-3.,3.),vec2(3.,-3.),.65); }
 else if(state<3.) { cursor=segment(v,vec2(-4.,0.),vec2(4.,0.),.65); }
 if(i.waiting>.5) { cursor=line(max(abs(v.x),abs(v.y))-3.,.55); }
 c=mix(c,rgb(91.,228.,126.),cursor*.85);
 let notch=segment(p,vec2(s.x-111.,bar_y-4.),vec2(s.x-111.,bar_y+5.),.55);
 c=mix(c,rgb(51.,255.,102.),notch*(.5+.2*speaker));
 }
 return vec4(c,aa(edge));
}
