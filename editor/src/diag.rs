//! NanaType Editor's events (docs/logging.md).

use nanatype_diag::domains::EDITOR;
use nanatype_diag::{EventDescriptor, Severity};

/// Every error notice the author sees, as shown.
pub static ERROR_NOTICE: EventDescriptor =
    EventDescriptor::new(EDITOR, 1, "editor.error_notice", Severity::Warn, &[]);

pub static EVENTS: &[&EventDescriptor] = &[&ERROR_NOTICE];
