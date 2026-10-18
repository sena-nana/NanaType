fn grain(p:vec2<f32>)->f32 {return fract(sin(dot(floor(p*2.5),vec2<f32>(127.1,311.7)))*43758.5453);}
fn material(i:MaterialInput)->vec4<f32> {
    let p=clamp(i.progress,0.,1.);
    // Same lower-left / upper-right 45-degree edge as the entrance, sweeping
    // the visible text target from upper left toward lower right.
    let edge=i.position.x+i.position.y+(grain(i.target_position)-0.5)*3.
        +sin((i.target_position.x-i.target_position.y)*0.6)*0.8;
    let front=p*(i.size.x+i.size.y+8.)-4.;
    let cover=smoothstep(front-1.2,front+1.2,edge);
    return vec4<f32>(i.color.rgb,select(select(cover,0.,p>=1.),1.,p<=0.));
}
