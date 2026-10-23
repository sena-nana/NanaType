//! Helpers shared by the integration tests: `mod common;` in each test file.
//! Each test binary uses a different subset, so unused items are expected.
#![allow(dead_code)]

use nana_ui::runtime::{DocumentId, RuntimeDocument, view::Mount};
use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{clock, ui::Ui};
use nanatype_theme::SkinRegistry;
use std::{sync::Arc, time::Duration};

/// The built-in skins, as this build loads them.
pub fn registry() -> SkinRegistry {
    SkinRegistry::new(None, env!("CARGO_PKG_VERSION"))
}

/// A built-in skin by id.
pub fn skin(id: &str) -> Arc<nanatype_theme::Skin> {
    registry().load(id).unwrap().skin
}

/// The RGBA pixel at `(x, y)` of a tightly packed `width`-wide image.
pub fn pixel(data: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = (y as usize * width as usize + x as usize) * 4;
    data[i..i + 4].try_into().unwrap()
}

/// Builds a [`RuntimeAgentSession`] around one mounted view.
pub struct SessionBuilder {
    id: u64,
    dark: Option<bool>,
    scale: f32,
    locale: Option<nanatype_i18n::Locale>,
}

/// Document 1, without theme tokens, at scale 1, in NanaUI's own language.
pub fn session() -> SessionBuilder {
    SessionBuilder {
        id: 1,
        dark: None,
        scale: 1.0,
        locale: None,
    }
}

impl SessionBuilder {
    /// Installs the application theme tokens (light or dark).
    pub fn themed(mut self, dark: bool) -> Self {
        self.dark = Some(dark);
        self
    }

    pub fn id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Puts the document in `locale` as the app does each window's.
    pub fn locale(mut self, locale: nanatype_i18n::Locale) -> Self {
        self.locale = Some(locale);
        self
    }

    /// Mounts `view` in a `width`×`height` logical window. Not yet flushed.
    pub fn mount<M: Mount>(
        self,
        width: u32,
        height: u32,
        view: impl FnOnce() -> M,
    ) -> RuntimeAgentSession {
        self.mount_output(width, height, view).0
    }

    /// Like [`Self::mount`], also returning what mounting `view` produced.
    pub fn mount_output<M: Mount>(
        self,
        width: u32,
        height: u32,
        view: impl FnOnce() -> M,
    ) -> (RuntimeAgentSession, M::Output) {
        let (doc, output) = self.build(view);
        (self.wrap(doc, width, height), output)
    }

    /// Like [`Self::mount`], with the view built at clock time `now` ms.
    pub fn mount_at<M: Mount>(
        self,
        now: u64,
        width: u32,
        height: u32,
        view: impl FnOnce() -> M,
    ) -> RuntimeAgentSession {
        let (doc, _) = clock::capture_at(now, || self.build(view));
        self.wrap(doc, width, height)
    }

    fn build<M: Mount>(&self, view: impl FnOnce() -> M) -> (RuntimeDocument, M::Output) {
        let id = DocumentId::new(self.id).unwrap();
        let mut doc = RuntimeDocument::new(id);
        if let Some(dark) = self.dark {
            doc.context_mut()
                .set_theme_tokens(nanatype::ui::kit::theme::compile(dark))
                .unwrap();
        }
        if let Some(locale) = self.locale {
            nanatype_frame::apply_locale(doc.context_mut(), locale);
        }
        let output = doc.context_mut().mount_view_root(id, view).unwrap();
        (doc, output)
    }

    fn wrap(&self, doc: RuntimeDocument, width: u32, height: u32) -> RuntimeAgentSession {
        RuntimeAgentSession::new_scaled(doc, width, height, self.scale).unwrap()
    }
}

/// Captures one frame at `now` ms with the presentation clock at `elapsed`.
/// When `capture_env` names a directory, the frame is saved there as `name.png`.
pub fn frame(
    session: &mut RuntimeAgentSession,
    ui: Ui,
    now: u64,
    elapsed: u64,
    name: &str,
    capture_env: &str,
) -> Vec<u8> {
    ui.now.set(now);
    session
        .document_mut()
        .sync_presentation_clock(Duration::from_millis(elapsed));
    clock::capture_at(now, || {
        let (size, rgba) = session.screenshot_rgba().unwrap();
        if let Ok(dir) = std::env::var(capture_env) {
            std::fs::create_dir_all(&dir).unwrap();
            image::save_buffer(
                std::path::Path::new(&dir).join(format!("{name}.png")),
                &rgba,
                size.width,
                size.height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        rgba
    })
}

/// Render until a frame neither waits on nor was refused a shader compile.
/// Compiles turned away while other work held both compiler slots are retried
/// on the next frame, as the application's warming loop does; waiting only for
/// `pending == 0` would miss them when tests run in parallel.
#[allow(dead_code)]
pub fn settle_shaders(mut render: impl FnMut()) {
    use nanatype::ui::shader_effects::cache_stats;
    let started = std::time::Instant::now();
    loop {
        let refused = cache_stats().refused;
        render();
        while cache_stats().pending > 0 {
            assert!(
                started.elapsed() < Duration::from_secs(15),
                "shader compilation stalled"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        if cache_stats().refused == refused {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "shader compiles kept being refused"
        );
    }
}
