// Original NanaType plate: a frosted pink glass panel in spring haze.
// The canvas is the whole region; the panel is inset so the branch can grow
// past its top-right corner and a few petals rest outside it. Only the branch
// sways about its attachment; the glass and the reading field stay still.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
fn rot(v:vec2<f32>,a:f32)->vec2<f32> { return vec2(cos(a)*v.x-sin(a)*v.y,sin(a)*v.x+cos(a)*v.y); }
fn tex(uv:vec2<f32>)->vec4<f32> {
 if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
 return sample0(uv);
}
// Straight-alpha "over".
fn over(top:vec4<f32>,base:vec4<f32>)->vec4<f32> {
 let a=top.a+base.a*(1.-top.a);
 return vec4((top.rgb*top.a+base.rgb*base.a*(1.-top.a))/max(a,1e-4),a);
}
// Five-sided notched petal, about 9 x 14 px before scaling.
fn petal(p:vec2<f32>,o:vec2<f32>,a:f32,k:f32)->vec2<f32> {
 let q=rot(p-o,-a)/k;
 let body=length(q/vec2(4.6,7.))-1.;
 let notch=length((q-vec2(0.,-7.2))/vec2(2.4,2.6))-1.;
 let d=max(body*5.5,-notch*2.5)*k;
 return vec2(aa(d),aa(abs(d+.5)-.45));
}
// Glass panel between lo and hi; mist positions are fractions of the panel.
fn glass(p:vec2<f32>,lo:vec2<f32>,hi:vec2<f32>,r:f32)->vec4<f32> {
 let mid=(lo+hi)*.5; let half=(hi-lo)*.5; let size=hi-lo;
 let e=rect(p-mid,half,r);
 let u=(p-lo)/size;
 var c=mix(rgb(255.,247.,250.),rgb(255.,240.,245.),clamp(u.y*.85+u.x*.3,0.,1.));
 var a=mix(.92,.86,u.y);
 let mist=rgb(247.,184.,206.);
 let ma=(1.-smoothstep(0.,.55,length((u-vec2(.12,1.))*vec2(size.x/size.y*.35,1.))))*.55;
 let mb=(1.-smoothstep(0.,.45,length((u-vec2(.92,.05))*vec2(size.x/size.y*.35,1.))))*.45;
 let mc=(1.-smoothstep(0.,.35,length((u-vec2(.6,.9))*vec2(size.x/size.y*.35,1.))))*.3;
 c=mix(c,mist,ma); c=mix(c,mist,mb); c=mix(c,rgb(252.,211.,225.),mc);
 a=max(a,.9);
 // Rim: a bright sheen on top, soft below, then a faint pink outer line.
 let sheen=mix(.9,.15,smoothstep(0.,.5,u.y))+.3*smoothstep(.5,1.,u.y);
 c=mix(c,vec3(1.),aa(abs(e+1.25)-.75)*sheen);
 c=mix(c,rgb(231.,163.,188.),aa(abs(e+.25)-.5)*.45);
 // Soft rose shadow under the panel.
 let sh=(1.-smoothstep(-2.,10.,rect(p-mid-vec2(0.,4.),half,r)))*.12;
 let inside=aa(e);
 return vec4(mix(rgb(194.,65.,111.),c,inside),max(inside*a,sh*(1.-inside)));
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
 // params: x 0 = full plate, 1 = portrait, 2 = compact; y 1 = minimal.
 if(i.params0.x>.5) {
  return glass(p,vec2(2.),s-vec2(2.,4.),14.);
 }
 let lo=vec2(4.,24.); let hi=vec2(s.x-36.,s.y-6.);
 var out=glass(p,lo,hi,18.);
 // The branch attaches outside the right edge and sways about that point.
 // Outside the glass it holds still, so the silhouette never changes; on the
 // glass it only recolours the panel (alpha stays the glass's), and the sway
 // fades in a few pixels inside the rim and right of the reading field.
 let sdf=rect(p-(lo+hi)*.5,(hi-lo)*.5,18.);
 let inside=aa(sdf);
 let corner=1.-smoothstep(80.,136.,length(p-vec2(s.x+12.,62.)));
 let clear=max(1.-inside,corner);
 let hinge=vec2(s.x+26.,112.);
 let free=smoothstep(5.,26.,-sdf)*smoothstep(s.x-106.,s.x-90.,p.x);
 let angle=(.020*sin(t)+.006*sin(t*2.))*free;
 let w=rot(p-hinge,-angle);
 let base=-4./57.29578;
 let uv=(vec2(762.,248.)+rot(w,-base)*2.)/vec2(768.,264.);
 let sharp=tex(uv);
 // Behind the glass the branch is frosted: a small fixed-tap blur.
 let px=vec2(5.)/vec2(768.,264.);
 let soft=(sharp+tex(uv+vec2(px.x,0.))+tex(uv-vec2(px.x,0.))+tex(uv+vec2(0.,px.y))+tex(uv-vec2(0.,px.y))
  +tex(uv+px*1.6)+tex(uv-px*1.6)+tex(uv+vec2(px.x,-px.y)*1.6)+tex(uv+vec2(-px.x,px.y)*1.6))/9.;
 let fa=soft.a*.55*inside*(1.-clear);
 let tinted=mix(out.rgb,soft.rgb/max(soft.a,1e-4),fa);
 let on_glass=vec4(mix(tinted,sharp.rgb,sharp.a*clear),out.a);
 let over_all=over(vec4(sharp.rgb,sharp.a*clear),vec4(tinted,out.a));
 out=mix(over_all,on_glass,smoothstep(0.,4.,-sdf));
 // Two to four petals have settled outside the glass only.
 let p1=petal(p,vec2(s.x-20.,136.),.52,1.);
 let p2=petal(p,vec2(s.x-10.,166.),-.7,.8);
 let p3=petal(p,vec2(s.x-26.,188.),1.3,.9);
 let pc=rgb(247.,184.,206.); let pe=rgb(229.,146.,175.);
 out=over(vec4(mix(pc,pe,p1.y),p1.x),out);
 out=over(vec4(mix(pc,pe,p2.y),p2.x*.85),out);
 out=over(vec4(mix(pc,pe,p3.y),p3.x),out);
 return out;
}
