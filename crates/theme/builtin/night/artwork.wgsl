// Night: a neon sign. One continuous magenta tube (haze, bloom, tube, hot
// core) on an aubergine board, held by metal clips. The tube stutters twice
// when a new line lights up, then burns steady; nothing moves otherwise.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
// Mixed in sRGB like the flat design, converted to linear once at the end.
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return vec3(r,g,b)/255.; }
fn clip(p:vec2<f32>,c:vec2<f32>,vertical:bool)->vec4<f32> {
 let half=select(vec2(3.,7.),vec2(7.,3.),vertical);
 let q=p-c;
 let shade=select(q.x/half.x,q.y/half.y,vertical);
 let metal=mix(rgb(90.,80.,100.),rgb(185.,176.,196.),1.-abs(shade+.1));
 return vec4(metal,aa(rect(q,half,1.5)));
}
fn dip(t:f32,a:f32,b:f32)->f32 { return smoothstep(a-.02,a+.02,t)*(1.-smoothstep(b-.02,b+.02,t)); }
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size;
 // params0.x: 1 portrait, 2 compact; compact plates keep only tube and core.
 let small=i.params0.x>.5;
 let board=rect(p-s*.5,s*.5-vec2(.5),select(22.,14.,small));
 var c=mix(rgb(27.,17.,40.),rgb(18.,11.,28.),i.uv.y);
 // Light spilled from the tube onto the board, strongest top-left.
 let spill=exp(-length((p-vec2(s.x*.12,0.))/vec2(s.x*.75,s.y*3.))*2.5);
 c=mix(c,rgb(255.,79.,184.),.16*spill*select(1.,.5,small));
 c=mix(c,rgb(58.,38.,80.),(1.-smoothstep(.0,1.5,abs(board+1.)))*select(1.,0.,small));
 // Tube centre line: a rounded rectangle inset from the board.
 let inset=select(vec4(16.,12.,12.,12.),vec4(12.,8.,8.,8.),small);
 let lo=vec2(inset.w,inset.x); let hi=s-vec2(inset.y,inset.z);
 let d=abs(rect(p-(lo+hi)*.5,(hi-lo)*.5,select(16.,10.,small)));
 // Two stutters in the first 0.6 s of a line; steady afterwards.
 let t=i.line_age;
 let lit=1.-.8*max(dip(t,.06,.16),dip(t,.30,.40));
 let haze=exp(-d*d/200.)*.26*select(1.,0.,small);
 let bloom=exp(-d*d/(2.*select(3.5,3.,small)*select(3.5,3.,small)))*.65;
 c=mix(c,rgb(255.,79.,184.),clamp(haze+bloom,0.,1.)*lit);
 c=mix(c,rgb(255.,79.,184.),aa(d-select(1.5,1.25,small))*mix(.35,1.,lit));
 c=mix(c,rgb(255.,227.,244.),aa(d-.5)*lit);
 if(!small) {
  // Metal clips hold the tube; they sit on it, so they stay lit when it dims.
  let k1=clip(p,vec2(s.x*.336,lo.y),false); c=mix(c,k1.rgb,k1.a);
  let k2=clip(p,vec2(s.x*.735,lo.y),false); c=mix(c,k2.rgb,k2.a);
  let k3=clip(p,vec2(s.x*.23,hi.y),false); c=mix(c,k3.rgb,k3.a);
  let k4=clip(p,vec2(s.x*.63,hi.y),false); c=mix(c,k4.rgb,k4.a);
  let k5=clip(p,vec2(lo.x,(lo.y+hi.y)*.5),true); c=mix(c,k5.rgb,k5.a);
  let k6=clip(p,vec2(hi.x,(lo.y+hi.y)*.5),true); c=mix(c,k6.rgb,k6.a);
 }
 return vec4(pow(c,vec3(2.2)),aa(board));
}
