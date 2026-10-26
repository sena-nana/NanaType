//! Helpers shared by the integration tests: `mod common;` in each test file.
//! Each test binary uses a different subset, so unused items are expected.
#![allow(dead_code)]

use nana_ui::runtime::{DocumentId, RuntimeDocument, view::Mount};
use nana_ui_devtools::agent::RuntimeAgentSession;

/// Builds a [`RuntimeAgentSession`] around one mounted view.
pub struct SessionBuilder {
    id: u64,
    dark: Option<bool>,
}

/// Document 1, without theme tokens.
pub fn session() -> SessionBuilder {
    SessionBuilder { id: 1, dark: None }
}

impl SessionBuilder {
    /// Installs the application theme tokens (light or dark).
    pub fn themed(mut self, dark: bool) -> Self {
        self.dark = Some(dark);
        self
    }

    /// Mounts `view` in a `width`×`height` logical window. Not yet flushed.
    pub fn mount<M: Mount>(
        self,
        width: u32,
        height: u32,
        view: impl FnOnce() -> M,
    ) -> RuntimeAgentSession {
        let id = DocumentId::new(self.id).unwrap();
        let mut doc = RuntimeDocument::new(id);
        if let Some(dark) = self.dark {
            doc.context_mut()
                .set_theme_tokens(nanatype_frame::ui::kit::theme::compile(dark))
                .unwrap();
        }
        doc.context_mut().mount_view_root(id, view).unwrap();
        let mut session = RuntimeAgentSession::new(doc, width, height).unwrap();
        session.set_gpu_renderers(nanatype_frame::ui::sweep::renderers());
        session
    }
}
