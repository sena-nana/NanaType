//! Per-window theme scoping, locale/reduced-motion propagation, animation clock
//! waking, and toast stack management.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use nana_ui::CompiledTheme;
use nana_ui::runtime::{Entity, RuntimeDocument, Toast, ToastStack};
use nana_ui_platform::WindowId;

use crate::app::App;
use crate::msg::{HostMsg, Msg};
use crate::ui::feedback::ToastRequest;
use crate::windows::WindowKind;

#[derive(Default)]
pub(super) struct ThemeEnv {
    toasts: HashMap<WindowId, Entity<ToastStack>>,
    /// Language and forced reduced motion each document was last given;
    /// re-sent only when they change (or to a new window), not per message.
    doc_env: HashMap<WindowId, (nanatype_i18n::Locale, bool)>,
    /// The app theme each output window last held back from: an output
    /// window keeps the plain theme whatever the app's own windows wear.
    output_held: HashMap<WindowId, Arc<CompiledTheme>>,
    /// The scene host's animation clock, to bring documents up to now.
    animation_clock: Option<nana_ui::RuntimeAnimationClock>,
}

impl ThemeEnv {
    pub fn register_toasts(&mut self, id: WindowId, stack: Entity<ToastStack>) {
        self.toasts.insert(id, stack);
    }

    pub fn remove_window(&mut self, id: WindowId) {
        self.doc_env.remove(&id);
        self.output_held.remove(&id);
        self.toasts.remove(&id);
    }

    pub fn invalidate_doc_env(&mut self, id: WindowId) {
        self.doc_env.remove(&id);
    }

    pub fn clear_doc_env(&mut self) {
        self.doc_env.clear();
    }

    pub fn init_window_theme(
        &mut self,
        id: WindowId,
        kind: WindowKind,
        doc: &mut RuntimeDocument,
        app: &App,
    ) {
        let _ = doc
            .context_mut()
            .set_theme_tokens(if kind.canvas().is_some() {
                self.output_held.insert(id, app.appearance.theme.clone());
                app.appearance.output_theme.clone()
            } else {
                app.appearance.theme.clone()
            });
    }

    /// A new app theme reaches every window's document (the scene host
    /// installs it); an output window takes the plain one back before it
    /// paints, so high contrast and the UI scale never reach the broadcast.
    pub fn hold_output_theme(
        &mut self,
        id: WindowId,
        docs: &mut HashMap<WindowId, RuntimeDocument>,
        app: &App,
    ) {
        if !WindowKind::from_id(id).is_some_and(|k| k.canvas().is_some()) {
            return;
        }
        let theme = &app.appearance.theme;
        if self
            .output_held
            .get(&id)
            .is_some_and(|held| Arc::ptr_eq(held, theme))
        {
            return;
        }
        if let Some(doc) = docs.get_mut(&id) {
            let _ = doc
                .context_mut()
                .set_theme_tokens(app.appearance.output_theme.clone());
            self.output_held.insert(id, theme.clone());
        }
    }

    pub fn sync_doc_env(&mut self, docs: &mut HashMap<WindowId, RuntimeDocument>, app: &App) {
        let locale = app.ui.locale.get_untracked();
        // The app's reduce-motion setting wins over the system's in every
        // window's runtime, so view transitions and toasts land at once too.
        let forced = app.settings.playback.reduced_motion;
        self.doc_env.retain(|id, _| docs.contains_key(id));
        for (id, doc) in docs {
            if self.doc_env.get(id) == Some(&(locale, forced)) {
                continue;
            }
            self.doc_env.insert(*id, (locale, forced));
            let cx = doc.context_mut();
            nanatype_frame::apply_locale(cx, locale);
            if forced {
                cx.set_reduced_motion(true);
            } else {
                cx.follow_system_reduced_motion();
            }
            // The app paints its own high contrast (`AppearanceState::theme`), honouring
            // the setting over the system's; NanaUI's overlay would replace
            // it in every window, the broadcast ones too.
            cx.set_high_contrast(false);
        }
    }

    pub fn sync_animation_clock(&mut self, epoch: Instant) {
        self.animation_clock = Some(nana_ui::RuntimeAnimationClock::new(epoch));
    }

    pub fn wake_animation_clock(&self, doc: &mut RuntimeDocument) {
        if let Some(clock) = self.animation_clock {
            clock.wake(doc.context_mut(), Instant::now());
        }
    }

    /// Push the app's new toasts into the settings window (when open) and
    /// the input window (while it is visible).
    pub fn show_toasts(
        &self,
        docs: &mut HashMap<WindowId, RuntimeDocument>,
        pending: &mut Vec<ToastRequest>,
        input_visible: bool,
    ) {
        let requests = std::mem::take(pending);
        if requests.is_empty() {
            return;
        }
        for request in requests {
            let mut shown = false;
            for kind in [WindowKind::Settings, WindowKind::Input] {
                let wanted = match kind {
                    WindowKind::Input => input_visible,
                    _ => request.settings,
                };
                let id = kind.id();
                let (true, Some(stack), Some(doc)) =
                    (wanted, self.toasts.get(&id).copied(), docs.get_mut(&id))
                else {
                    continue;
                };
                shown |= request.push(doc, stack, move |toast| {
                    Msg::Host(HostMsg::ToastDismissed(kind, toast))
                });
            }
            request.retry_unless(shown, pending);
        }
    }

    pub fn dismiss_toast(
        &self,
        docs: &mut HashMap<WindowId, RuntimeDocument>,
        kind: WindowKind,
        toast: Entity<Toast>,
    ) {
        let id = kind.id();
        if let (Some(stack), Some(doc)) = (self.toasts.get(&id).copied(), docs.get_mut(&id)) {
            let _ = doc.context_mut().remove_toast(stack, toast);
        }
    }
}
