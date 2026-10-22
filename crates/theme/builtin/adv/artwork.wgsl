// ADV: a cinema letterbox. No frame; the scrim fades in from a transparent
// top to near black, and one 1.5px white rule sets the upper edge. Static.
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size;
 // params0.x: 1 portrait, 2 compact. Small layouts put the rule through
 // their name tab and start the scrim darker so the tab never floats.
 let small=i.params0.x>.5;
 let y=p.y;
 var a=0.;
 var rule_y=66.;
 var fade=.06;
 if(small) {
  rule_y=24.;
  fade=.08;
  a=mix(.35,.82,smoothstep(0.,.3*s.y,y));
  a=mix(a,.95,smoothstep(.3*s.y,s.y,y));
 } else {
  a=.30*smoothstep(0.,32.,y);
  a=mix(a,.74,smoothstep(32.,68.,y));
  a=mix(a,.90,smoothstep(68.,137.,y));
  a=mix(a,.95,smoothstep(137.,s.y,y));
 }
 var c=mix(rgb(13.,13.,18.),rgb(5.,5.,7.),smoothstep(.3,.6,i.uv.y));
 // The rule: 1.5px, white at 90%, fading out at both ends.
 let ends=smoothstep(0.,fade,i.uv.x)*smoothstep(0.,fade,1.-i.uv.x);
 let rule=clamp(1.-abs(y-rule_y-.75)+.25,0.,1.)*.9*ends;
 c=mix(c,vec3(1.),rule/max(a+rule*(1.-a),.001));
 a=a+rule*(1.-a);
 return vec4(c,a);
}
