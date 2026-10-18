struct Params {
    destination: vec2<f32>, origin: vec2<f32>, size: vec2<f32>, scale: f32, opacity: f32,
    color: vec4<f32>, clock: f32, entrance_ms: f32, exit: f32, radius: f32, text_origin: vec2<f32>, crop: vec2<f32>,
};
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var coverage: texture_2d<f32>;
@group(0) @binding(2) var mask_sampler: sampler;
@group(0) @binding(3) var blurred: texture_2d<f32>;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vertex_main(@builtin(vertex_index) i: u32) -> Vertex {
    let points = array<vec2<f32>, 6>(vec2(0.,0.), vec2(1.,0.), vec2(0.,1.), vec2(0.,1.), vec2(1.,0.), vec2(1.,1.));
    let uv=points[i]; let pos=(p.origin+uv*p.size)/p.destination;
    var out:Vertex;out.position=vec4(pos.x*2.-1.,1.-pos.y*2.,0.,1.);out.uv=uv;return out;
}
fn grain(q:vec2<f32>)->f32 {return fract(sin(dot(floor(q/2.),vec2(127.1,311.7)))*43758.5453);}
// RGB is an exact 24-bit reveal time, alpha is rasterized glyph coverage.
fn texel(t:texture_2d<f32>,q:vec2<f32>)->vec4<f32> {
    let pos=vec2<i32>(floor(q*p.scale));let size=vec2<i32>(textureDimensions(coverage));
    if any(pos<vec2(0)) || any(pos>=size) {return vec4(0.);}
    return textureLoad(t,pos,0);
}
fn illuminate(sample:vec4<f32>,q:vec2<f32>,dissolve:bool)->vec2<f32> {
    let b=round(sample.rgb*255.);
    let at=dot(b,vec3(1.,256.,65536.));
    if sample.a<0.01 {return vec2(0.,-1.);}
    let progress=select(clamp((p.clock-at+1.)/p.entrance_ms,0.,1.),1.,at<0.5);
    let pulse=sin(progress*3.14159265);
    if p.exit>=0. {
        let progress_out=clamp((p.exit-0.2)/0.8,0.,1.);
        let keep=select(select(smoothstep(progress_out-0.06,progress_out+0.06,grain(q-p.text_origin)),1.,p.exit<=0.2),1.,!dissolve);
        return vec2(sample.a*keep,(0.45+sin(clamp(p.exit/0.45,0.,1.)*3.14159265)*0.75)*(1.-smoothstep(0.7,1.,p.exit)));
    }
    return vec2(sample.a*smoothstep(0.,0.4,progress),select(pulse,0.,at<0.5));
}
fn ink(q:vec2<f32>)->vec2<f32> {return illuminate(texel(coverage,q),q,true);}
@fragment fn fragment_main(v:Vertex)->@location(0) vec4<f32> {
    let q=p.crop+v.uv*p.size/p.scale;let center=ink(q);
    let glow=illuminate(texel(blurred,q),q,false);
    var light=center.x*center.y*0.35+glow.x*glow.y*0.95;
    // A fixed grid seeded from actual ink creates finite spark trails.
    let cell=floor(q/6.);
    for(var y=-2;y<=2;y++){for(var x=-2;x<=2;x++){
        let seed=(cell+vec2(f32(x),f32(y)))*6.+vec2(3.);
        var source=ink(seed);let h=grain(seed-p.text_origin);
        if p.exit>=0. {
            source.x=texel(coverage,seed).a;
            let life=clamp((p.exit-0.2-h*0.55)/0.3,0.,1.);
            source.y=sin(life*3.14159265)*(1.-smoothstep(0.9,1.,p.exit));
        }
        if source.x>0.15 && source.y>0.01 && h>0.45 {
            let age=select(source.y,clamp((p.exit-0.2-h*0.55)/0.25,0.,1.),p.exit>=0.);
            let drift=vec2((h-0.5)*24.,-5.-h*10.)*age;
            let d=abs(q-seed-drift);
            let dotlight=exp(-dot(d,d)/1.2);
            let thread=exp(-d.y*d.y*3.-d.x*d.x/20.);
            light+=(dotlight+thread*0.45)*source.y*0.8;
        }
    }}
    let alpha=clamp(light*p.opacity*p.color.a,0.,0.9);
    return vec4(mix(p.color.rgb,vec3(1.),center.x*0.65)*alpha,alpha);
}
