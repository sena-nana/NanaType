//! One module per workspace of the activity rail; [`super::shell`] lays
//! them out.
pub mod data;
pub mod design;
pub mod files;
pub mod lua;
pub mod package;
pub mod signing;
pub mod sounds;

use std::sync::Arc;

use nana_ui::runtime::{SelectionState, TreeSelectionRequested, TreeView};

/// A single-selection tree's selection: the row `id`, or none. The editor's
/// state owns what is selected; the tree only shows it.
fn tree_selection(id: Option<&str>) -> SelectionState<Arc<str>> {
    let mut selection = SelectionState::single();
    if let Some(id) = id {
        selection.select_only(Arc::from(id));
    }
    selection
}

/// The row a tree's selection request selects, when it changes the
/// selection to one row.
fn requested_row(tree: &TreeView, request: &TreeSelectionRequested) -> Option<Arc<str>> {
    let mut selection = SelectionState::clone(&tree.selection);
    if !request.apply_to(&mut selection, &tree.nodes) {
        return None;
    }
    selection.iter().next().cloned()
}
