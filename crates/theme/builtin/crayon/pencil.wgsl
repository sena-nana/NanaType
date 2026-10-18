// Colored-pencil tooth, following the glyph rather than the window clock.
fn material(i: MaterialInput) -> vec4<f32> {
    let p=i.position;
    let tooth=fract(sin(dot(floor(p*2.2),vec2(127.1,311.7)))*43758.5453);
    let hatch=.5+.5*sin((p.x*.84+p.y*.55)*10.8);
    let density=.77+.16*smoothstep(.08,.88,tooth)+.07*hatch;
    // Preserve explicit rich-text colors and the original stroke silhouette.
    let pigment=i.color.rgb*(.91+.09*tooth);
    return vec4(pigment,i.color.a*density);
}
