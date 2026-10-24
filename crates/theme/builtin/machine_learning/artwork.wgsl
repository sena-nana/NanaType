// Machine learning: a slowly drifting lavender node network on graphite, dense
// at the right and thinning out to the left, plus four viewfinder brackets.
// A jittered grid: each cell may hold one node; nodes link to their right,
// lower and diagonal neighbours. A pixel only needs the 3x3 cells around it.
// Motion loops on `phase` (0..1 over period_s): every angle is an integer
// number of turns per period, so the wrap is seamless.
const TAU=6.2831853;
// Jitter (.31 cell) + drift must stay under ~.47 cell, or links get cut at
// cell borders: 4px of 30px is .13 cell.
const DRIFT=4.;
fn hash(c:vec2<f32>,k:f32)->f32 { return fract(sin(dot(c,vec2(127.1,311.7))+k*74.7)*43758.5453); }
// xy: node position in pixels; z: its strength (0 when the cell is empty);
// w: a per-node random used to thin out links.
fn node(c:vec2<f32>,cell:f32,x0:f32,x1:f32,ph:f32)->vec4<f32> {
 let h=vec3(hash(c,1.),hash(c,2.),hash(c,3.));
 let home=(c+.5+(h.xy-.5)*.62)*cell;
 // Presence and strength follow the home position, so a node never blinks
 // at the x0/x1 edges while it drifts.
 let t=clamp((home.x-x0)/max(x1-x0,1.),0.,1.);
 let live=step(h.z,mix(.42,.92,t))*step(x0,home.x)*step(home.x,x1);
 // One small loop per node: 1 or 2 turns per period, random start angle.
 let ang=TAU*(ph*(1.+floor(h.x*2.))+h.y);
 var pos=home+vec2(cos(ang),sin(ang))*DRIFT;
 // Never drift into the portrait nameplate rail.
 pos.x=min(pos.x,x1);
 return vec4(pos,live*(.10+.55*t*sqrt(t)),fract(h.z*13.7));
}
// keep: the share of time this kind of link is connected. Each link fades
// in and out on its own phase (1 or 2 cycles per period, ~2s fades).
fn link(p:vec2<f32>,a:vec4<f32>,b:vec4<f32>,keep:f32,ph:f32)->f32 {
 let v=b.xy-a.xy; let q=p-a.xy;
 let d=length(q-v*clamp(dot(q,v)/max(dot(v,v),.01),0.,1.));
 let r=fract(a.w+b.w*1.7);
 // Triangle wave: as smooth as a sine once smoothstep shapes it, no trig.
 let on=abs(2.*fract(ph*(1.+floor(fract(r*7.3)*2.))+r)-1.);
 let w=smoothstep(1.-keep-.15,1.-keep+.15,on);
 return (1.-smoothstep(.2,1.,d))*.55*min(a.z,b.z)*w;
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
 // Soft lavender haze around the dense cluster.
 let haze=.05*(1.-smoothstep(0.,min(240.,s.x*.5),length(p-vec2(x1-90.,s.y*.5))));
 var a=haze;
 if(!small) {
  var b=max(bracket(p,vec2(8.,8.),vec2(1.,1.)),max(bracket(p,s-vec2(8.),vec2(-1.,-1.)),bracket(p,vec2(8.,s.y-8.),vec2(1.,-1.))));
  if(!portrait) { b=max(b,bracket(p,vec2(s.x-8.,8.),vec2(-1.,1.))); }
  a=max(a,b*.7);
 }
 let col=pow(vec3(169.,155.,255.)/255.,vec3(2.2));
 // No node lives left of x0; a full cell covers drift plus node radius.
 // Whole columns take this branch together, so it stays coherent on the GPU.
 if(p.x<x0-cell) { return vec4(col,a); }
 let ph=i.phase;
 let g=floor(p/cell);
 let n00=node(g+vec2(-1.,-1.),cell,x0,x1,ph); let n10=node(g+vec2(0.,-1.),cell,x0,x1,ph); let n20=node(g+vec2(1.,-1.),cell,x0,x1,ph);
 let n01=node(g+vec2(-1.,0.),cell,x0,x1,ph); let n11=node(g,cell,x0,x1,ph); let n21=node(g+vec2(1.,0.),cell,x0,x1,ph);
 let n02=node(g+vec2(-1.,1.),cell,x0,x1,ph); let n12=node(g+vec2(0.,1.),cell,x0,x1,ph); let n22=node(g+vec2(1.,1.),cell,x0,x1,ph);
 var l=max(link(p,n00,n10,.8,ph),link(p,n10,n20,.8,ph));
 l=max(l,max(link(p,n01,n11,.8,ph),link(p,n11,n21,.8,ph)));
 l=max(l,max(link(p,n02,n12,.8,ph),link(p,n12,n22,.8,ph)));
 l=max(l,max(link(p,n00,n01,.7,ph),link(p,n01,n02,.7,ph)));
 l=max(l,max(link(p,n10,n11,.7,ph),link(p,n11,n12,.7,ph)));
 l=max(l,max(link(p,n20,n21,.7,ph),link(p,n21,n22,.7,ph)));
 l=max(l,max(link(p,n00,n11,.4,ph),link(p,n10,n21,.4,ph)));
 l=max(l,max(link(p,n01,n12,.4,ph),link(p,n11,n22,.4,ph)));
 var d=max(blob(p,n11),max(blob(p,n00),max(blob(p,n10),blob(p,n20))));
 d=max(d,max(blob(p,n01),blob(p,n21)));
 d=max(d,max(blob(p,n02),max(blob(p,n12),blob(p,n22))));
 return vec4(col,max(a,max(l,d)));
}
