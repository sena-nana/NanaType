struct Params {
    destination: vec2<f32>, origin: vec2<f32>, size: vec2<f32>, scale: f32, opacity: f32,
    color: vec4<f32>, clock: f32, entrance_ms: f32, exit_age: f32, interval_ms: f32,
    text_origin: vec2<f32>, crop: vec2<f32>, hold_ms: f32, exit_ms: f32, alphabet: f32, seed: f32,
};
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) timing: vec4<f32>,
};
@vertex fn vertex_main(@builtin(vertex_index) i: u32, @location(0) rect: vec4<f32>,
    @location(1) timing: vec4<f32>) -> Vertex {
    let points = array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
    let uv=points[i];
    let pos=(p.origin+(p.text_origin+rect.xy+uv*rect.zw-p.crop)*p.scale)/p.destination;
    var out:Vertex;
    out.position=vec4(pos.x*2.-1.,1.-pos.y*2.,0.,1.);
    out.uv=uv; out.timing=timing; return out;
}
fn hash(value:u32)->u32 {
    var x=value; x=x^(x>>16u); x=x*0x7feb352du;
    x=x^(x>>15u); x=x*0x846ca68bu; return x^(x>>16u);
}
@fragment fn fragment_main(v:Vertex)->@location(0) vec4<f32> {
    let age=p.clock-v.timing.x;
    var keep=0.;
    if p.exit_age>=0. {
        let deadline=p.hold_ms+(p.exit_ms-p.hold_ms)*(v.timing.y+1.)/v.timing.z;
        keep=select(0.,1.,p.exit_age<deadline);
    } else {
        keep=select(0.,1.,v.timing.x>=0. && age>=0. && age<p.entrance_ms);
    }
    let tick=u32(max(0.,floor(select(age,p.exit_age,p.exit_age>=0.)/max(1.,p.interval_ms))));
    let base=hash(u32(v.timing.w)+u32(p.seed));
    // Disjoint even/odd subsets guarantee a change on adjacent ticks.
    let lane=tick%2u;
    let count=max(1u,(u32(p.alphabet)+1u-lane)/2u);
    let index=min(u32(p.alphabet)-1u,(hash(base^tick)%count)*2u+lane);
    let columns=16u;
    let rows=u32(ceil(p.alphabet/16.));
    let uv=(vec2(f32(index%columns),f32(index/columns))+clamp(v.uv,vec2(0.01),vec2(0.99)))/vec2(16.,f32(rows));
    let coverage=textureSampleLevel(atlas,atlas_sampler,uv,0.).a;
    let alpha=coverage*keep*p.opacity*p.color.a;
    let tint=mix(p.color.rgb,vec3(0.39,0.32,1.),select(0.,0.12,base%7u==0u));
    return vec4(tint*alpha,alpha);
}
