//! Passive feedback shared by the windows: toasts in the corner of the
//! settings, input and chat windows, and the empty states of lists that have
//! nothing in them yet (a ✦ glyph, one line, and a quiet way forward).

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, EntityRef, IntoView, widget};
use nana_ui::runtime::{Stack, ToastStack, ToastTone};
use nana_ui::theme::space;
use nana_ui_core::{AlignSpec, LengthSpec, PositionSpec};

/// A toast the host is asked to show.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastRequest {
    pub title: String,
    pub tone: ToastTone,
    /// How long it stays, in milliseconds.
    pub ms: u64,
    /// Also show it in the settings window (when open); the input window
    /// shows every toast while it is visible.
    pub settings: bool,
    /// When it was asked for; one no window could show yet waits this long.
    pub at: std::time::Instant,
}

impl ToastRequest {
    /// A notice: an error or information, for [`NOTICE_MS`].
    pub fn notice(text: String, error: bool) -> Self {
        let tone = if error {
            ToastTone::Danger
        } else {
            ToastTone::Info
        };
        Self::new(text, tone, NOTICE_MS, true)
    }

    /// Still worth showing in a window that opens now.
    pub fn fresh(&self) -> bool {
        self.at.elapsed() < std::time::Duration::from_millis(self.ms)
    }

    /// Push it into `stack`; closing or timing out sends `dismissed(toast)`.
    pub fn push<M: Send + 'static>(
        &self,
        doc: &mut nana_ui::runtime::RuntimeDocument,
        stack: nana_ui::runtime::Entity<ToastStack>,
        dismissed: impl Fn(nana_ui::runtime::Entity<nana_ui::runtime::Toast>) -> M
        + Send
        + Sync
        + 'static,
    ) -> bool {
        let toast = nana_ui::runtime::Toast::new(self.title.as_str(), self.tone)
            .timeout(std::time::Duration::from_millis(self.ms))
            .dismissible(true);
        let cx = doc.context_mut();
        let pushed = cx.push_toast(stack, toast).and_then(|toast| {
            cx.on(
                toast,
                move |_, _: &nana_ui::runtime::ToastDismissed, vcx| {
                    vcx.dispatch_program_all(dismissed(toast));
                },
            )
        });
        if let Err(e) = &pushed {
            crate::debug_log(&format!("toast: {e}"));
        }
        pushed.is_ok()
    }

    pub fn new(title: String, tone: ToastTone, ms: u64, settings: bool) -> Self {
        Self {
            title,
            tone,
            ms,
            settings,
            at: std::time::Instant::now(),
        }
    }
}

/// How long a notice stays as a toast.
pub const NOTICE_MS: u64 = 6000;
/// How long "Sent ✓" stays.
pub const SENT_MS: u64 = 1200;

/// A toast stack in the bottom-right corner of a positioned window root,
/// `bottom` px above its bottom edge. The host pushes toasts into it.
pub fn toast_stack(stack: EntityRef<ToastStack>, bottom: f32, max_visible: usize) -> AnyView {
    let mut st = nana_ui::runtime::NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.position = PositionSpec::Absolute;
        l.offset_right = Some(LengthSpec::Px(space::XL));
        l.offset_bottom = Some(LengthSpec::Px(bottom));
        l.width = Some(LengthSpec::Px(crate::ui::kit::TOAST_W));
        l.max_width = Some(LengthSpec::Percent(90.0));
        l.max_height = Some(LengthSpec::Px(crate::ui::kit::TOAST_MAX_H));
        l.align_items = AlignSpec::End;
        l.z_index = Some(100);
    }
    widget(ToastStack::new().max_visible(max_visible).style(st))
        .entity_ref(stack)
        .into_any()
}

/// Lay `view` out with a toast stack over it.
pub fn with_toasts(view: impl IntoView, stack: EntityRef<ToastStack>) -> AnyView {
    with_toasts_limit(view, stack, 3)
}

pub fn with_toasts_limit(
    view: impl IntoView,
    stack: EntityRef<ToastStack>,
    max_visible: usize,
) -> AnyView {
    widget(Stack::fill_column(0.0).with_layout(|l| {
        l.position = PositionSpec::Relative;
        l.width = Some(LengthSpec::Fill);
        l.height = Some(LengthSpec::Fill);
    }))
    .children((view, toast_stack(stack, space::XXXL, max_visible)))
    .into_any()
}
