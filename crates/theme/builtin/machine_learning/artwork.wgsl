// Machine learning: a static lavender node network on graphite, dense at the
// right and thinning out to the left, plus four viewfinder brackets.
// A jittered grid: each cell may hold one node; nodes link to their right,
// lower and diagonal neighbours. A pixel only needs the 3x3 cells around it.
fn hash(c:vec2<f32>,k:f32)->f32 { return fract(sin(dot(c,vec2(127.1,311.7))+k*74.7)*43758.5453); }
// xy: node position in pixels; z: its strength (0 when the cell is empty);
// w: a per-node random used to thin out links.
fn node(c:vec2<f32>,cell:f32,x0:f32,x1:f32)->vec4<f32> {
 let h=vec3(hash(c,1.),hash(c,2.),hash(c,3.));
 let pos=(c+.5+(h.xy-.5)*.62)*cell;
 let t=clamp((pos.x-x0)/max(x1-x0,1.),0.,1.);
 let live=step(h.z,mix(.42,.92,t))*step(x0,pos.x)*step(pos.x,x1);
 return vec4(pos,live*(.10+.55*pow(t,1.4)),fract(h.z*13.7));
}
fn link(p:vec2<f32>,a:vec4<f32>,b:vec4<f32>,keep:f32)->f32 {
 let v=b.xy-a.xy; let q=p-a.xy;
 let d=length(q-v*clamp(dot(q,v)/max(dot(v,v),.01),0.,1.));
 return (1.-smoothstep(.2,1.,d))*.55*min(a.z,b.z)*step(fract(a.w+b.w*1.7),keep);
}
fn blob(p:vec2<f32>,a:vec4<f32>)->f32 {
 return (1.-smoothstep(-.6,.6,length(p-a.xy)-(1.4+2.5*a.z)))*a.z;
}
fn bracket(p:vec2<f32>,corner:vec2<f32>,dir:vec2<f32>)->f32 {
 let q=(p-corner)*dir;
 let arm=max(step(0.,q.x)*step(q.x,16.)*step(0.,q.y)*step(q.y,2.),step(0.,q.y)*step(q.y,16.)*step(0.,q.x)*step(q.x,2.));
 return arm;
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size;
 let portrait=s.y>s.x;
 let small=s.x<500.;
 // A vertical nameplate owns the right rail in portrait composition.
 let x1=select(s.x,s.x-60.,portrait);
 let x0=x1*select(.3,select(.6,.4,portrait),small || portrait);
 let cell=30.;
 let g=floor(p/cell);
 let n00=node(g+vec2(-1.,-1.),cell,x0,x1); let n10=node(g+vec2(0.,-1.),cell,x0,x1); let n20=node(g+vec2(1.,-1.),cell,x0,x1);
 let n01=node(g+vec2(-1.,0.),cell,x0,x1); let n11=node(g,cell,x0,x1); let n21=node(g+vec2(1.,0.),cell,x0,x1);
 let n02=node(g+vec2(-1.,1.),cell,x0,x1); let n12=node(g+vec2(0.,1.),cell,x0,x1); let n22=node(g+vec2(1.,1.),cell,x0,x1);
 var l=max(link(p,n00,n10,.8),link(p,n10,n20,.8));
 l=max(l,max(link(p,n01,n11,.8),link(p,n11,n21,.8)));
 l=max(l,max(link(p,n02,n12,.8),link(p,n12,n22,.8)));
 l=max(l,max(link(p,n00,n01,.7),link(p,n01,n02,.7)));
 l=max(l,max(link(p,n10,n11,.7),link(p,n11,n12,.7)));
 l=max(l,max(link(p,n20,n21,.7),link(p,n21,n22,.7)));
 l=max(l,max(link(p,n00,n11,.4),link(p,n10,n21,.4)));
 l=max(l,max(link(p,n01,n12,.4),link(p,n11,n22,.4)));
 var d=max(blob(p,n11),max(blob(p,n00),max(blob(p,n10),blob(p,n20))));
 d=max(d,max(blob(p,n01),blob(p,n21)));
 d=max(d,max(blob(p,n02),max(blob(p,n12),blob(p,n22))));
 // Soft lavender haze around the dense cluster.
 let haze=.05*(1.-smoothstep(0.,min(240.,s.x*.5),length(p-vec2(x1-90.,s.y*.5))));
 var a=max(max(l,d),haze);
 if(!small) {
  var b=max(bracket(p,vec2(8.,8.),vec2(1.,1.)),max(bracket(p,s-vec2(8.),vec2(-1.,-1.)),bracket(p,vec2(8.,s.y-8.),vec2(1.,-1.))));
  if(!portrait) { b=max(b,bracket(p,vec2(s.x-8.,8.),vec2(-1.,1.))); }
  a=max(a,b*.7);
 }
 return vec4(pow(vec3(169.,155.,255.)/255.,vec3(2.2)),a);
}
