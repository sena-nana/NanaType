//! Optional node references for authoring. Geometry is projected by NanaUI from
//! the same layout, transforms and scrolling used to draw and hit-test the skin.
use super::Stage;
use nana_ui::runtime::view::{El, NodeRef, Signal, node_ref, on_cleanup};
use nana_ui::runtime::{AppContext, ComponentView};
use std::collections::BTreeMap;
pub type Nodes = Signal<BTreeMap<String, NodeRef>>;
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub key: String,
    pub rect: [f32; 4],
}
pub trait Capture {
    fn editor_part(self, ui: Stage, key: impl Into<String>) -> Self;
}
impl<C: ComponentView, K> Capture for El<C, K> {
    fn editor_part(self, ui: Stage, key: impl Into<String>) -> Self {
        if ui.editor_nodes.is_none() {
            return self;
        }
        let node = node_ref();
        track(ui, key, node);
        self.node_ref(node)
    }
}
/// Register an existing motion reference without replacing its mounted node.
pub fn track(ui: Stage, key: impl Into<String>, node: NodeRef) {
    let Some(nodes) = ui.editor_nodes else {
        return;
    };
    let key = key.into();
    nodes.update(|m| {
        m.insert(key.clone(), node);
    });
    on_cleanup(move || {
        nodes.update(|m| {
            if m.get(&key) == Some(&node) {
                m.remove(&key);
            }
        });
    });
}

pub fn collect(ui: Stage, cx: &AppContext) -> Vec<Part> {
    let Some(nodes) = ui.editor_nodes else {
        return Vec::new();
    };
    let entries = nodes.with_untracked(|m| {
        m.iter()
            .filter_map(|(k, n)| n.get_untracked().map(|id| (k.clone(), id)))
            .collect::<Vec<_>>()
    });
    let ids = entries.iter().map(|(_, id)| *id).collect::<Vec<_>>();
    let projected = cx.world().project_accessibility_nodes(&ids);
    entries
        .into_iter()
        .filter_map(|(key, id)| {
            let n = projected.iter().find(|n| n.id == id)?;
            let b = n.bounds;
            (b.width > 0. && b.height > 0.).then_some(Part {
                key,
                rect: [b.x, b.y, b.width, b.height],
            })
        })
        .collect()
}
