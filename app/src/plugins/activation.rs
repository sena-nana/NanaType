//! Content checks and request identity are separate: identical bytes in a new
//! dialog never accept the previous dialog's asynchronous confirmation.
use super::{ActivationReview, NEXT_TICKET};
use crate::ui::Ui;
use std::sync::{Arc, atomic::Ordering};

#[derive(Default)]
pub struct Activation {
    state: State,
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    Reviewing {
        ticket: u64,
        id: String,
    },
    Ready(Arc<ActivationReview>),
    Confirming {
        ticket: u64,
        review: Arc<ActivationReview>,
    },
}

impl Activation {
    pub fn request(&mut self, id: String, ui: Ui) -> u64 {
        self.cancel(ui);
        let ticket = NEXT_TICKET.fetch_add(1, Ordering::Relaxed);
        self.state = State::Reviewing { ticket, id };
        ui.plugin_activation_busy.set(true);
        ticket
    }

    pub fn confirm(&mut self, ui: Ui) -> Option<(u64, Arc<ActivationReview>)> {
        if !ui.plugin_source_trusted.get_untracked() {
            return None;
        }
        let State::Ready(review) = &self.state else {
            return None;
        };
        let review = review.clone();
        let ticket = NEXT_TICKET.fetch_add(1, Ordering::Relaxed);
        self.state = State::Confirming {
            ticket,
            review: review.clone(),
        };
        ui.plugin_activation_busy.set(true);
        Some((ticket, review))
    }

    /// Finish the current phase. Only a matching confirmation returns content
    /// that may be persisted as approval and submitted for startup.
    pub fn complete(
        &mut self,
        ticket: u64,
        result: Result<Arc<ActivationReview>, String>,
        ui: Ui,
    ) -> Result<Option<Arc<ActivationReview>>, String> {
        let (id, digest) = match &self.state {
            State::Reviewing { ticket: active, id } if *active == ticket => (id.as_str(), None),
            State::Confirming {
                ticket: active,
                review,
            } if *active == ticket => (review.id.as_str(), Some(review.digest.as_str())),
            _ => return Ok(None),
        };
        let confirming = digest.is_some();
        if confirming && !ui.plugin_source_trusted.get_untracked() {
            self.cancel(ui);
            return Ok(None);
        }
        let result = result.and_then(|current| {
            if current.id == id && digest.is_none_or(|digest| current.digest == digest) {
                Ok(current)
            } else {
                Err(ui.now_t("packages-changed-during-review"))
            }
        });
        self.cancel(ui);
        result
            .map(|review| {
                if confirming {
                    Some(review)
                } else {
                    ui.plugin_activation.set(Some(review.clone()));
                    self.state = State::Ready(review);
                    None
                }
            })
            .inspect_err(|error| ui.plugin_activation_error.set(Some(error.clone())))
    }

    pub fn cancel(&mut self, ui: Ui) {
        self.state = State::Idle;
        ui.plugin_activation.set(None);
        ui.plugin_source_trusted.set(false);
        ui.plugin_activation_busy.set(false);
        ui.plugin_activation_error.set(None);
    }

    pub fn cancel_for(&mut self, id: &str, ui: Ui) {
        let active = match &self.state {
            State::Idle => false,
            State::Reviewing { id: active, .. } => active == id,
            State::Ready(review) | State::Confirming { review, .. } => review.id == id,
        };
        if active {
            self.cancel(ui)
        }
    }
}
