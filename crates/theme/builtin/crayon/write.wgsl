fn grain(p:vec2<f32>)->f32 {return fract(sin(dot(floor(p*3.),vec2<f32>(127.1,311.7)))*43758.5453);}
fn material(i:MaterialInput)->vec4<f32> {
    let p=clamp(i.progress,0.,1.);
    // In screen coordinates, x+y is constant along a lower-left / upper-right
    // edge. Logical pixels keep that edge at 45 degrees at every aspect ratio.
    // Increasing x+y advances the edge from upper left toward lower right.
    let edge=i.position.x+i.position.y+(grain(i.position)-0.5)*3.
        +sin((i.position.x-i.position.y)*2.8)*0.6;
    let front=p*(i.size.x+i.size.y+8.)-4.;
    let cover=1.-smoothstep(front-1.2,front+1.2,edge);
    return vec4<f32>(i.color.rgb,select(select(cover,1.,p>=1.),0.,p<=0.));
}
