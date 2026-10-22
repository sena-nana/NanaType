// Original NanaType plate: stationary reading field and silhouette.
// Animate the skin's own objects, never a generic edge sweep or the text.
// Black-and-white print: an inked panel whose line thickens toward the
// bottom right, a sharp tail, screen tone in two corners, a hard offset
// shadow and speed lines beside the panel.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
fn tri(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>,c:vec2<f32>)->f32 {
 let e0=b-a; let e1=c-b; let e2=a-c;
 let v0=p-a; let v1=p-b; let v2=p-c;
 let pq0=v0-e0*clamp(dot(v0,e0)/dot(e0,e0),0.,1.);
 let pq1=v1-e1*clamp(dot(v1,e1)/dot(e1,e1),0.,1.);
 let pq2=v2-e2*clamp(dot(v2,e2)/dot(e2,e2),0.,1.);
 let sg=sign(e0.x*e2.y-e0.y*e2.x);
 let d=min(min(vec2(dot(pq0,pq0),sg*(v0.x*e0.y-v0.y*e0.x)),vec2(dot(pq1,pq1),sg*(v1.x*e1.y-v1.y*e1.x))),vec2(dot(pq2,pq2),sg*(v2.x*e2.y-v2.y*e2.x)));
 return -sqrt(d.x)*sign(d.y);
}
// Panel silhouette with gently bowed edges, plus the tail when it has one.
fn panel(p:vec2<f32>,lo:vec2<f32>,hi:vec2<f32>,tail:bool)->f32 {
 let c=(lo+hi)*.5; let h=(hi-lo)*.5; let q=p-c;
 let bow=1.8*(1.-pow(clamp(q.x/h.x,-1.,1.),2.))*smoothstep(.5,1.,abs(q.y)/h.y)
  +1.2*(1.-pow(clamp(q.y/h.y,-1.,1.),2.))*smoothstep(.7,1.,abs(q.x)/h.x);
 var d=rect(q,h-vec2(2.),9.)-bow;
 if tail {
  let t=tri(p,vec2(lo.x+92.,hi.y-6.),vec2(lo.x+152.,hi.y-6.),vec2(lo.x+76.,hi.y+26.));
  d=min(d,t);
 }
 return d;
}
fn tone(p:vec2<f32>,o:vec2<f32>,r:f32,k:f32)->f32 {
 let f=clamp(1.-length(p-o)/r,0.,1.);
 let rp=vec2(p.x+p.y,p.y-p.x)*.70710678/7.;
 let cell=(fract(rp)-vec2(.5))*7.;
 return aa(length(cell)-1.8*sqrt(f)*k)*step(.04,f);
}
fn speed(p:vec2<f32>,a:vec2<f32>,len:f32,dy:f32,w:f32)->f32 {
 // A tapered ink stroke from a (thick end) toward the right.
 let x=(p.x-a.x)/len;
 let y=p.y-a.y-dy*x;
 return aa(abs(y)-w*(1.-x))*step(0.,x)*step(x,1.);
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size;
 let main=i.params0.x<.5;
 let portrait=i.params0.x>.5 && i.params0.x<1.5;
 let details=i.params0.y<.5 && (main || portrait);
 let ink=rgb(17.,17.,17.);
 let lo=select(vec2(3.),vec2(4.,2.),main);
 let hi=select(s-vec2(9.),vec2(s.x-64.,s.y-36.),main);
 let off=select(vec2(6.),vec2(8.),main);
 let d=panel(p,lo,hi,main);
 let ds=panel(p-off,lo,hi,main);
 // Line weight: thin on the top-left, heavy toward the bottom-right.
 let c0=(lo+hi)*.5; let n=normalize((p-c0)/max((hi-lo)*.5,vec2(1.)));
 let w=2.4+3.6*clamp(dot(n,vec2(.6,.8)),0.,1.);
 let inside=aa(d);
 var col=vec3(1.);
 var a=max(inside,aa(ds));
 if details {
  let t1=tone(p,lo-vec2(6.),96.,1.);
  let t2=tone(p,hi+vec2(4.,6.),150.,1.);
  col=mix(col,ink,max(t1,t2)*.7);
 }
 col=mix(col,ink,1.-aa(d+w));
 col=mix(ink,col,inside);
 if main {
  let x0=s.x-50.;
  var sp=speed(p,vec2(x0,22.),34.,-6.,2.4);
  sp=max(sp,speed(p,vec2(x0+2.,40.),22.,-3.5,1.6));
  sp=max(sp,speed(p,vec2(x0,57.),40.,-4.,2.6));
  sp=max(sp,speed(p,vec2(x0+4.,75.),18.,-.5,1.3));
  sp=max(sp,speed(p,vec2(x0,93.),36.,3.,2.2));
  sp=max(sp,speed(p,vec2(x0+2.,111.),24.,4.,1.8));
  sp=max(sp,speed(p,vec2(x0,128.),32.,8.,2.4));
  sp=sp*(1.-a);
  col=mix(col,ink,sp);
  a=max(a,sp);
 }
 return vec4(col,a);
}
