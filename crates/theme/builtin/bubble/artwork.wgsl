// Original NanaType plate: stationary reading field and silhouette.
// Animate the skin's own objects, never a generic edge sweep or the text.
// Candy sticker: pastel pink bubble, thick plum outline, a pink colour-block
// offset shadow and a heart sticker that rocks and squashes.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn line(d:f32,w:f32)->f32 { return aa(abs(d)-w); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn segment(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>,w:f32)->f32 {
 let v=b-a; return aa(length(p-a-v*clamp(dot(p-a,v)/max(dot(v,v),.01),0.,1.))-w);
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
fn bubble(p:vec2<f32>,lo:vec2<f32>,hi:vec2<f32>,tail:bool)->f32 {
 let r=select(20.,30.,tail);
 var d=rect(p-(lo+hi)*.5,(hi-lo)*.5,r);
 if tail {
  // A short rounded tail sweeping down to the left.
  d=min(d,tri(p,vec2(lo.x+86.,hi.y-8.),vec2(lo.x+132.,hi.y-8.),vec2(lo.x+64.,hi.y+17.))-3.5);
 }
 return d;
}
fn heart(h:vec2<f32>)->f32 {
 return min(min(length(h-vec2(-2.4,-1.8))-3.6,length(h-vec2(2.4,-1.8))-3.6),(abs(h.x)+abs(h.y-1.1)-5.1)*.707);
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
 let main=i.params0.x<.5;
 let portrait=i.params0.x>.5 && i.params0.x<1.5;
 let details=i.params0.y<.5 && (main || portrait);
 let plum=rgb(122.,46.,89.);
 let lo=select(vec2(3.),vec2(18.,22.),main);
 let hi=select(s-vec2(9.),vec2(s.x-36.,s.y-28.),main);
 let off=select(vec2(5.),vec2(8.),main);
 let lw=select(3.,4.5,main);
 let d=bubble(p,lo,hi,main);
 let ds=bubble(p-off,lo,hi,main);
 let inside=aa(d);
 let k=clamp((p.y-lo.y)/max(hi.y-lo.y,1.),0.,1.);
 var c=mix(rgb(255.,244.,248.),rgb(255.,227.,238.),k);
 c=mix(c,plum,1.-aa(d+lw));
 // Candy gloss in the top-left corner, left of the text column.
 if main {
  let g=lo+vec2(37.,36.);
  let arc=line(length(p-g)-22.,2.5)*step(p.x,g.x-10.)*step(p.y,g.y);
  let gloss=max(max(arc,segment(p,vec2(lo.x+15.,lo.y+50.),vec2(lo.x+15.,lo.y+36.),2.5)),aa(length(p-vec2(lo.x+15.,lo.y+62.))-2.5));
  c=mix(c,vec3(1.),gloss*.95);
 }
 c=mix(rgb(255.,179.,207.),c,inside);
 var a=max(inside,aa(ds));
 if details {
  // The heart sticker rocks and squashes once per beat; its white rim,
  // plum ink and peeled corner move together.
  let bounce=pow(max(0.,sin(t*2.)),6.);
  let angle=.24+.10*sin(t*2.); let ca=cos(angle); let sa=sin(angle);
  let o=select(vec2(s.x-48.,30.),vec2(s.x-34.,s.y-36.),portrait);
  let v=(p-o+vec2(0.,2.8*bounce))/select(3.4,2.6,portrait);
  let h=vec2(ca*v.x-sa*v.y,sa*v.x+ca*v.y)/vec2(1.+.10*bounce,1.-.12*bounce);
  let hd=heart(h)*select(3.4,2.6,portrait);
  let rim=aa(hd-4.);
  c=mix(c,vec3(1.),rim);
  c=mix(c,plum,aa(hd-1.25));
  c=mix(c,rgb(255.,122.,168.),aa(hd+1.25));
  c=mix(c,vec3(1.),segment(h,vec2(-3.4,-3.),vec2(-2.,-4.),.55)*.85);
  let flap=aa((tri(h,vec2(2.,-5.6),vec2(5.6,-2.),vec2(2.4,-2.4))-.15)*select(3.4,2.6,portrait));
  c=mix(c,rgb(235.,211.,222.),flap);
  a=max(a,rim);
 }
 return vec4(c,a);
}
