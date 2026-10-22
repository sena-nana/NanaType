// Terminal: an amber P3 phosphor screen. A title bar with three status
// blocks and the four-state busy spinner (a hollow block while waiting);
// faint static scanlines. Only the spinner moves.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn line(d:f32,w:f32)->f32 { return aa(abs(d)-w); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn segment(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>,w:f32)->f32 {
 let v=b-a; return aa(length(p-a-v*clamp(dot(p-a,v)/max(dot(v,v),.01),0.,1.))-w);
}
// Mixed in sRGB like the flat design, converted to linear once at the end.
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return vec3(r,g,b)/255.; }
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size;
 // params0.x: 1 portrait (bar at the bottom), 2 compact (no bar);
 // params0.y: 1 minimal (no status blocks).
 let portrait=i.params0.x>.5 && i.params0.x<1.5;
 let compact=i.params0.x>1.5;
 let details=i.params0.y<.5;
 let edge=rect(p-s*.5,s*.5-vec2(.5),select(6.,5.,compact));
 let amber=rgb(255.,176.,0.);
 // Screen: a faint radial lift in the middle of the glass.
 let r=length((i.uv-vec2(.5,.55))/vec2(.6,1.));
 var c=mix(rgb(23.,17.,10.),rgb(13.,10.,5.),smoothstep(0.,.75,r));
 // Static scanlines, 1px in 3.
 c=mix(c,vec3(0.),select(0.,.16,fract(p.y/3.)<.34));
 if(!compact) {
  let bar=select(aa(p.y-24.),aa(s.y-24.-p.y),portrait);
  c=mix(c,rgb(26.,19.,8.),bar);
  c=mix(c,amber,line(p.y-select(24.,s.y-24.,portrait),.5)*.22);
 }
 c=mix(c,amber,line(edge+1.,.75)*.32);
 if(!compact && details) {
  let y=select(12.,s.y-12.,portrait);
  c=mix(c,amber,aa(rect(p-vec2(16.,y),vec2(4.),0.)));
  c=mix(c,amber,line(rect(p-vec2(29.,y),vec2(3.25),0.),.75)*.7);
  c=mix(c,rgb(168.,133.,54.),aa(rect(p-vec2(42.,y),vec2(4.),0.))*.45);
  // Busy spinner: | / - \ while typing, a hollow block while waiting.
  let state=floor(fract(i.phase*8.)*4.);
  let v=p-vec2(60.,y);
  var cursor=segment(v,vec2(-3.,-3.),vec2(3.,3.),.75);
  if(state<1.) { cursor=segment(v,vec2(0.,-4.),vec2(0.,4.),.75); }
  else if(state<2.) { cursor=segment(v,vec2(-3.,3.),vec2(3.,-3.),.75); }
  else if(state<3.) { cursor=segment(v,vec2(-4.,0.),vec2(4.,0.),.75); }
  if(i.waiting>.5) { cursor=line(max(abs(v.x),abs(v.y))-3.5,.75); }
  c=mix(c,amber,cursor*.9);
 }
 return vec4(pow(c,vec3(2.2)),aa(edge));
}
