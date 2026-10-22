// Original NanaType plate: cold rice paper and a single dry-brush stroke.
// Asymmetric white space: no left rule; the licensed bamboo stays a decor.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
fn hash(p:vec2<f32>)->f32 { return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453); }
fn noise(p:vec2<f32>)->f32 {
 let i=floor(p); let f=fract(p); let u=f*f*(3.-2.*f);
 return mix(mix(hash(i),hash(i+vec2(1.,0.)),u.x),mix(hash(i+vec2(0.,1.)),hash(i+vec2(1.,1.)),u.x),u.y);
}
// One loaded-brush stroke: blunt wet head at x0, dry split bristles towards x1.
fn brush(p:vec2<f32>,x0:f32,x1:f32,y:f32,w:f32)->vec2<f32> {
 let u=clamp((p.x-x0)/max(x1-x0,1.),0.,1.);
 let yc=y+1.2*sin(u*3.4)-1.5*u+.8*(noise(vec2(p.x*.05,3.))-.5);
 let hw=w*(1.-.5*u)*(1.-.6*smoothstep(.82,1.,u))+w*.3*exp(-u*14.);
 let dy=(p.y-yc)/hw;
 let rough=.16*(noise(vec2(p.x*.4,dy*2.+7.))-.5);
 let body=aa((abs(dy)-1.-rough)*hw)*aa(p.x-x1)*aa(x0-p.x);
 let head=aa(length((p-vec2(x0+hw*.6,yc))/vec2(hw*1.1,hw*1.05))*hw-hw);
 let bristle=noise(vec2(p.x*.006,dy*4.+11.))*.72+noise(vec2(p.x*.03,dy*9.+2.))*.28;
 let th=.06+.5*smoothstep(.12,1.,u);
 let dry=smoothstep(th,th+.1,bristle);
 let cover=max(body*mix(1.,dry,smoothstep(.06,.3,u)),head*(1.-u));
 return vec2(cover,mix(.97,.6,smoothstep(0.,1.,u))*(1.-.5*smoothstep(.85,1.,u)));
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size;
 // params: x 0 = full plate, 1 = portrait, 2 = compact; y 1 = minimal.
 let portrait=i.params0.x>.5 && i.params0.x<1.5;
 let compact=i.params0.x>1.5;
 let details=i.params0.y<.5;
 let q=p-s*.5;
 let edge=rect(q,s*.5-vec2(1.),2.);
 // Xuan paper: cold white, faint cloud and long fibres.
 var c=rgb(251.,250.,246.);
 let cloud=noise(p*.012)*.6+noise(p*.03+vec2(9.,4.))*.4;
 c*=.985+.02*cloud;
 let fibre=smoothstep(.72,.8,noise(vec2(p.x*.09,p.y*.55)+vec2(3.,1.)))*.5
  +smoothstep(.76,.82,noise(vec2(p.x*.55,p.y*.08)+vec2(1.,8.)))*.35;
 c=mix(c,rgb(214.,210.,200.),fibre*.12);
 c*=.992+.012*hash(floor(p));
 c=mix(c,rgb(205.,199.,186.),aa(edge+1.2)*.18);
 if details {
  var x0=22.; var x1=s.x*.55; var w=6.5;
  if compact { x0=16.; x1=min(s.x*.47,260.); w=3.6; }
  if portrait { x0=s.x*.38; x1=s.x-22.; w=4.2; }
  let b=brush(p,x0,x1,s.y-select(14.,9.,compact),w);
  c=mix(c,rgb(20.,20.,20.),b.x*b.y);
 }
 return vec4(c,aa(edge));
}
