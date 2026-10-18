//! Bake a static material with the production shape adapter and color pipeline.
//! material_artwork <source.wgsl> <output.png> <width> <height> [parameters...]
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{DocumentId, RuntimeDocument, Stack};
use nana_ui_core::{LengthSpec, MaterialProgram, MaterialRef};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use std::{path::PathBuf, sync::Arc};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let source = std::fs::read_to_string(&args[0]).unwrap();
    nanatype_theme::validate_material(&source).expect("restricted material");
    let out = PathBuf::from(&args[1]);
    let w: u32 = args[2].parse().unwrap();
    let h: u32 = args[3].parse().unwrap();
    let mut params = [[0.; 4]; 2];
    for (index, value) in args.iter().skip(4).enumerate() {
        params[index / 4][index % 4] = value.parse().unwrap();
    }
    let material =
        MaterialRef::new(MaterialProgram::new(source)).with_parameters(Arc::from(params));
    let id = DocumentId::new(1).unwrap();
    let mut doc = RuntimeDocument::new(id);
    doc.context_mut()
        .mount_view_root(id, move || {
            widget(
                Stack::row(0.)
                    .width(LengthSpec::Px(w as f32))
                    .height(LengthSpec::Px(h as f32))
                    .with_layout(move |l| {
                        l.background = Some([1.; 4]);
                        l.paint.background_material = Some(material);
                    }),
            )
        })
        .unwrap();
    let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
    session.set_clear(Some([0.; 4]));
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    session.screenshot_png(&out).unwrap();
    println!("{}", out.display());
}
