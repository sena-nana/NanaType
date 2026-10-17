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
// A data packet follows the actual etched PCB route, then docks.
 let edge=max(rect(q,b,3.),abs(q.x)+abs(q.y)-b.x-b.y+13.);
 var c=mix(rgb(10.,18.,32.),rgb(19.,33.,53.),1.-i.uv.y);
 c=mix(c,rgb(92.,184.,255.),line(edge+1.,.7)*.8);
 c=mix(c,rgb(47.,83.,119.),line(edge+7.,.4)*.8);
 let a=vec2(s.x-102.,12.); let z=vec2(s.x-54.,21.);
 let trace=max(segment(p,a,vec2(s.x-72.,12.),.65),max(segment(p,vec2(s.x-72.,12.),z,.65),segment(p,z,vec2(s.x-18.,21.),.65)));
 c=mix(c,rgb(92.,184.,255.),trace*.65);
 c=mix(c,rgb(180.,149.,245.),ring(p-a,2.6,.6)*.8);
 let travel=clamp((i.phase-.12)/.56,0.,1.);
 var packet=mix(a,vec2(s.x-72.,12.),clamp(travel*3.,0.,1.));
 if(travel>1./3.) { packet=mix(vec2(s.x-72.,12.),z,clamp(travel*3.-1.,0.,1.)); }
 if(travel>2./3.) { packet=mix(z,vec2(s.x-22.,21.),clamp(travel*3.-2.,0.,1.)); }
 let envelope=smoothstep(.10,.15,i.phase)*(1.-smoothstep(.73,.82,i.phase));
 c=mix(c,rgb(181.,230.,255.),disc(p-packet,1.75)*envelope);
 c=mix(c,rgb(92.,184.,255.),ring(p-vec2(s.x-22.,21.),2.8,.55)*.65);
 let bracket=max(segment(p,vec2(19.,2.),vec2(19.,18.),1.),segment(p,vec2(19.,18.),vec2(34.,18.),1.));
 c=mix(c,rgb(128.,210.,255.),bracket*(.75+.2*speaker));
 return vec4(c,aa(edge));
}
