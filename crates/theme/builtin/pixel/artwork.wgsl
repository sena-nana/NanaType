// Original NanaType plate: stationary reading field and silhouette.
// Animate the skin's own objects, never a generic edge sweep or the text.
// DMG LCD: four greens only, every edge on whole pixels, no anti-aliasing.
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
fn inbox(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>)->f32 {
 return step(a.x,p.x)*step(p.x,b.x)*step(a.y,p.y)*step(p.y,b.y);
}
// A rectangle with 4px square notches cut from each corner.
fn notched(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>)->f32 {
 let cx=1.-step(a.x+4.,p.x)*step(p.x,b.x-4.);
 let cy=1.-step(a.y+4.,p.y)*step(p.y,b.y-4.);
 return inbox(p,a,b)*(1.-cx*cy);
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size;
 let portrait=i.params0.x>.5 && i.params0.x<1.5;
 let details=i.params0.y<.5;
 let dark=rgb(15.,56.,15.); let mid=rgb(48.,98.,48.);
 let light=rgb(139.,172.,15.); let screen=rgb(155.,188.,15.);
 // Outer 4px ink outline, a light-green bezel band, then the LCD glass
 // with a 2px recessed shadow on its top and left edges.
 let outer=notched(p,vec2(0.),s);
 var c=dark;
 c=mix(c,light,notched(p,vec2(4.),s-vec2(4.)));
 c=mix(c,mid,inbox(p,vec2(10.),s-vec2(10.)));
 let glass=inbox(p,vec2(12.),s-vec2(10.));
 c=mix(c,screen,glass);
 // LCD cell grid: 1px material lines every 4px, a texture rather than a rule.
 let g=floor(p-vec2(12.));
 let cell=max(step(2.5,g.x-4.*floor(g.x/4.)),step(2.5,g.y-4.*floor(g.y/4.)));
 c=mix(c,light,glass*cell*.35);
 // Screen corner marks.
 let tr=max(inbox(p,vec2(s.x-24.,16.),vec2(s.x-14.,18.)),inbox(p,vec2(s.x-16.,16.),vec2(s.x-14.,26.)));
 let bl=max(inbox(p,vec2(16.,s.y-24.),vec2(18.,s.y-14.)),inbox(p,vec2(16.,s.y-16.),vec2(26.,s.y-14.)));
 c=mix(c,mid,max(tr,bl)*glass);
 if details {
 // Activity meter above the text column: three LCD cells switch on
 // one at a time, whole cells only, no breathing.
 let o=select(vec2(s.x-62.,16.),vec2(18.,16.),portrait);
 let m=p-o;
 let k=floor(m.x/8.);
 let seg=inbox(m,vec2(0.,0.),vec2(23.,3.))*step(m.x-8.*k,5.);
 let current=floor(fract(i.phase*4.)*3.);
 let on=1.-step(.1,abs(k-current));
 c=mix(c,mix(light,mid,on),seg*glass);
 }
 return vec4(c,outer);
}
