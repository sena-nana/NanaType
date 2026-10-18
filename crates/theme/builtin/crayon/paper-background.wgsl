// The original paper.wgsl drawing, recomposed for display orientation.
// Its native cream undercoat and rounded paper contour are included here.
fn grain(p: vec2<f32>) -> f32 {
    return fract(sin(dot(floor(p), vec2(127.1, 311.7))) * 43758.5453);
}
fn ink(r: f32, g: f32, b: f32) -> vec3<f32> {
    return pow(vec3(r, g, b) / 255., vec3(2.2));
}
fn stroke(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, w: f32) -> f32 {
    let v = b-a;
    let along = clamp(dot(p-a, v) / max(dot(v,v), .01), 0., 1.);
    let waviness = sin(along * 39. + a.x * .2) * .42 + sin(along * 83.) * .18;
    return 1. - smoothstep(w-.65, w+.65, length(p-a-v*along) + waviness);
}
fn oval(p: vec2<f32>, r: vec2<f32>) -> f32 {
    return 1. - smoothstep(.91, 1.04, length(p/r));
}
fn effect(i: EffectInput) -> vec4<f32> {
    let p=i.uv*i.size; let s=i.size;
    let compact=i.params0.x > .5 && i.params0.x < 1.5;
    let narrow=i.params0.x > 1.5;
    let ornament_scale=select(1.,.48,narrow);
    let tooth=grain(p*1.8);
    let fibres=grain(vec2(p.x*.65,p.y*4.));
    let wax=(.42+.58*smoothstep(.12,.82,tooth)) * (.83+.17*sin((p.x+p.y*.43)*2.4));
    var c=ink(255.,251.,239.) * (.974+.026*grain(p*.7));
    c=mix(c,ink(227.,210.,182.),.035*(1.-fibres));
    // Imperfect, overlapping wax edges leave the paper visible between strokes.
    let top=stroke(p,vec2(18.,12.),vec2(s.x-23.,10.),2.5);
    let left=stroke(p,vec2(11.,22.),vec2(13.,s.y-20.),2.7);
    let bottom=stroke(p,vec2(21.,s.y-12.),vec2(s.x-19.,s.y-10.),2.8);
    let right=stroke(p,vec2(s.x-12.,19.),vec2(s.x-10.,s.y-22.),2.5);
    c=mix(c,ink(224.,170.,89.),top*wax*.8);
    c=mix(c,ink(121.,160.,135.),left*wax*.83);
    c=mix(c,ink(202.,120.,108.),bottom*wax*.8);
    c=mix(c,ink(126.,164.,186.),right*wax*.8);
    let corner=stroke(p,vec2(9.,16.),vec2(40.,8.),1.2)
        +stroke(p,vec2(s.x-43.,s.y-7.),vec2(s.x-8.,s.y-18.),1.1);
    c=mix(c,ink(178.,139.,113.),clamp(corner,0.,1.)*wax*.48);
    let portrait=s.y>s.x;
    if !compact && (narrow || portrait || s.x > 420.) {
        // A little sun and a flower occupy their own quiet margin.
        let sun_origin=select(select(vec2(s.x-63.,43.),vec2(43.,s.y-75.),portrait),vec2(34.,s.y-38.),narrow);
        let sun=(p-sun_origin)/ornament_scale;
        let sun_fill=oval(sun,vec2(12.5,12.));
        let sun_ring=1.-smoothstep(.8,1.9,abs(length(sun)-13.));
        let rays=stroke(sun,vec2(-20.,-3.),vec2(-25.,-4.),1.1)
            +stroke(sun,vec2(20.,3.),vec2(25.,4.),1.1)
            +stroke(sun,vec2(-1.,-20.),vec2(-2.,-26.),1.2)
            +stroke(sun,vec2(3.,20.),vec2(4.,25.),1.1)
            +stroke(sun,vec2(-15.,-16.),vec2(-19.,-21.),1.1)
            +stroke(sun,vec2(16.,-14.),vec2(21.,-18.),1.1);
        c=mix(c,ink(233.,180.,87.),sun_fill*wax*.4);
        c=mix(c,ink(214.,153.,63.),clamp(sun_ring+rays,0.,1.)*wax*.82);
        let root=select(select(vec2(s.x-66.,s.y-32.),vec2(s.x-50.,s.y-26.),portrait),vec2(s.x-32.,s.y-16.),narrow);
        let flower_p=root+(p-root)/ornament_scale;
        let stem=stroke(flower_p,root,root+vec2(-7.,-70.),1.45);
        let leaf1=oval((flower_p-root-vec2(-13.,-25.))*vec2(1.,1.)+vec2(0.),vec2(12.,4.8));
        let leaf2=oval(flower_p-root-vec2(8.,-42.),vec2(11.,4.5));
        c=mix(c,ink(109.,146.,112.),clamp(stem+leaf1*.75+leaf2*.8,0.,1.)*wax*.86);
        let flower=flower_p-root-vec2(-7.,-76.);
        let petals=oval(flower-vec2(0.,-10.),vec2(6.,10.))
            +oval(flower-vec2(-10.,-2.),vec2(10.,6.))
            +oval(flower-vec2(10.,-2.),vec2(10.,6.))
            +oval(flower-vec2(-6.,9.),vec2(7.,8.))
            +oval(flower-vec2(6.,9.),vec2(7.,8.));
        c=mix(c,ink(214.,135.,122.),clamp(petals,0.,1.)*wax*.68);
        c=mix(c,ink(208.,162.,76.),oval(flower,vec2(5.5))*wax*.93);
        // Short pencil-like marks by the lower corner.
        let marks=stroke(p,vec2(32.,s.y-25.),vec2(50.,s.y-29.),1.5)
            +stroke(p,vec2(54.,s.y-26.),vec2(70.,s.y-30.),1.4);
        c=mix(c,ink(126.,164.,186.),clamp(marks,0.,1.)*wax*.6);
    }
    let edge=min(min(p.x,s.x-p.x),min(p.y,s.y-p.y));
    let rough=.7+grain(p*.65)*1.6+sin(p.x*.43+p.y*.31)*.45;
    let alpha=smoothstep(rough,rough+1.1,edge);
    // The original material sits over an opaque cream native surface.
    // Preserve that undercoat rather than cutting a new ragged silhouette.
    let q=abs(p-s*.5)-s*.5+vec2(7.);
    let rounded=length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-7.;
    return vec4(mix(i.base.rgb,c,alpha),1.-smoothstep(-.5,.5,rounded));
}
