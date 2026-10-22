// Original NanaType plate: stationary reading field and silhouette.
// Animate the skin's own objects, never a generic edge sweep or the text.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
// The READY crystal: a 5x9 cell pixel gem lit from the left.
fn crystal(cell:vec2<f32>)->vec4<f32> {
 let x=cell.x; let y=cell.y;
 let hw=select(select(1.,2.,y>2.5 && y<5.5),0.,y<.5 || y>7.5);
 let inside=step(abs(x-2.),hw+.1)*step(-.1,y)*step(y,8.1);
 let left=2.-hw; let right=2.+hw;
 var c=rgb(228.,234.,255.);
 if y>5.5 && abs(x-2.)<.1 { c=rgb(159.,178.,255.); }
 if abs(x-right+1.)<.1 && hw>1.5 { c=rgb(159.,178.,255.); }
 if abs(x-right)<.1 { c=select(rgb(159.,178.,255.),rgb(90.,112.,224.),hw>1.5 || y>5.5); }
 if abs(x-left)<.1 && y<5.5 { c=rgb(255.,255.,255.); }
 if y>7.5 { c=rgb(90.,112.,224.); }
 return vec4(c,inside);
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
 let details=i.params0.y<.5;
 let q=p-s*.5; let h=s*.5;
// Enamel window: dark rim, white main edge, pale blue inner bevel.
 let outer=rect(q,h,8.);
 var c=rgb(5.,10.,51.);
 c=mix(c,rgb(255.,255.,255.),aa(rect(q,h-2.,6.)));
 c=mix(c,rgb(198.,208.,255.),aa(rect(q,h-6.,3.)));
 let field=aa(rect(q,h-8.,2.));
 let enamel=mix(rgb(42.,63.,184.),rgb(20.,37.,126.),clamp((p.y-8.)/max(s.y-16.,1.),0.,1.));
 c=mix(c,enamel,field);
 c=mix(c,rgb(16.,32.,110.),field*aa(p.y-10.)*.55);
 c=mix(c,rgb(255.,255.,255.),field*(1.-clamp((p.y-10.)/56.,0.,1.))*.14);
 if details {
 // The crystal sits in the bottom-right bezel, right of the text column,
 // and only takes two pixel-grid positions while waiting for the page.
 let bob=select(0.,-4.*step(0.,sin(t*4.)),i.waiting>.5);
 let o=vec2(s.x-38.,s.y-58.+bob);
 let g=crystal(floor((p-o)/4.));
 c=mix(c,g.rgb,g.a*field);
 let sh=aa(rect(p-vec2(s.x-28.,s.y-13.),vec2(10.,1.5),1.5));
 c=mix(c,rgb(5.,10.,51.),sh*.45*field);
 }
 return vec4(c,aa(outer));
}
