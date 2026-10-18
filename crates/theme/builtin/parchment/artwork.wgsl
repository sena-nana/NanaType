// Original NanaType plate: stationary reading field and silhouette.
// Animate the skin's own objects, never a generic edge sweep or the text.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn line(d:f32,w:f32)->f32 { return aa(abs(d)-w); }
fn disc(p:vec2<f32>,r:f32)->f32 { return aa(length(p)-r); }
fn ring(p:vec2<f32>,r:f32,w:f32)->f32 { return line(length(p)-r,w); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn segment(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>,w:f32)->f32 {
 let v=b-a; return aa(length(p-a-v*clamp(dot(p-a,v)/max(dot(v,v),.01),0.,1.))-w);
}
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
fn grain(p:vec2<f32>)->f32 { return fract(sin(dot(floor(p),vec2(12.9898,78.233)))*43758.5453); }
fn decal(p:vec2<f32>,origin:vec2<f32>,extent:vec2<f32>)->vec4<f32> {
 let uv=(p-origin)/extent;
 if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
 return sample0(uv);
}
// Smooth, fixed-seed paper variation at several physical scales.
fn paper_noise(p:vec2<f32>)->f32 {
 let a=floor(p); let f=fract(p); let u=f*f*(3.-2.*f);
 return mix(mix(grain(a),grain(a+vec2(1.,0.)),u.x),
            mix(grain(a+vec2(0.,1.)),grain(a+vec2(1.,1.)),u.x),u.y);
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
 // Portrait/compact instances reuse the original ink and material in logical pixels.
 let portrait=i.params0.x>.5 && i.params0.x<1.5;
 let details=i.params0.y<.5;
 let q=p-s*.5; let b=s*.5-vec2(1.);
 let opening=exp(-i.line_age*3.); let speaker=exp(-i.speaker_age*3.);
// Warm, irregular paper pulp: mottling, fibres, flecks and edge wicking.
 let edge=rect(q,b,5.);
 let cloud=paper_noise(p/42.)*.65+paper_noise(p/113.+vec2(7.,13.))*.35;
 let pulp=paper_noise(p/9.);
 let fibres=paper_noise(p*vec2(.55,.065));
 let cross=paper_noise(p*vec2(.075,.7));
 let margin=min(min(p.x,s.x-p.x),min(p.y,s.y-p.y));
 let wicked=exp(-max(margin,0.)/14.)*(.45+.55*paper_noise(p/16.));
 var c=mix(rgb(249.,235.,204.),rgb(223.,194.,143.),cloud*.65);
 c*=.95+.065*pulp+.035*fibres+.018*cross;
 c=mix(c,rgb(140.,91.,43.),wicked*.27);
 c=mix(c,rgb(133.,94.,50.),step(.983,grain(p))*.09);
 c=mix(c,rgb(255.,249.,229.),step(.985,grain(p+vec2(97.,31.)))*.18);
 // Two quiet impressed fillets, with a broken ivory lip rather than a
 // polished raised rim. Four corner sprigs grow out of the inner fillet.
 let impression=.76+.18*paper_noise(p/3.);
 c=mix(c,rgb(128.,89.,48.),line(edge+1.8,.48)*.72*impression);
 c=mix(c,rgb(154.,117.,73.),line(edge+8.,.35)*.56*impression);
 c=mix(c,rgb(255.,245.,218.),line(edge+2.7,.32)*.42);
 let sprig=select(vec2(144.,56.),vec2(min(144.,s.x*.42),min(144.,s.x*.42)*56./144.),i.params0.x>.5);
 let tl=decal(p,vec2(4.,2.),sprig);
 let tr=decal(vec2(s.x-p.x,p.y),vec2(4.,2.),sprig);
 let bl=decal(vec2(p.x,s.y-p.y),vec2(4.,2.),sprig);
 let br=decal(s-p,vec2(4.,2.),sprig);
 let pressure=.78+.14*pulp;
 c=mix(c,tl.rgb,tl.a*pressure); c=mix(c,tr.rgb,tr.a*pressure);
 c=mix(c,bl.rgb,bl.a*pressure); c=mix(c,br.rgb,br.a*pressure);
 return vec4(c,aa(edge));
}
