//! Skin parameter controls shared with NanaType Editor.
use crate::ui::Ui;
use nana_ui::runtime::view::AnyView;
use nanatype_theme::Skin;
use std::sync::Arc;
pub fn view(ui: Ui, id: &str, skin: Arc<Skin>) -> Option<AnyView> {
    nanatype_frame::ui::param_controls::view(*ui, id, skin)
}
