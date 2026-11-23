//! The alert dialog over nsh-base's alert_dialog(): the platform alert
//! (NSAlert on macOS) for a simple yes-and-no. A confirm that carries
//! disclosure actions is a dialog, not this.

use nsh_base::alert_dialog as base_alert;
use nsh_core::prelude::*;

/// An alert's answer: the named action, or dismissed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertAnswer {
    Action(SharedString),
    Dismissed,
}

/// Shows the alert and answers with the action's key (`discard`, `cancel`)
/// or [`AlertAnswer::Dismissed`]. The platform renders it; the await ends
/// when the user answers.
pub async fn alert_dialog(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    cancel: (&'static str, &'static str),
    destructive: (&'static str, &'static str),
) -> AlertAnswer {
    let choice = base_alert()
        .title(title)
        .description(description)
        .cancel(cancel.0, cancel.1)
        .destructive(destructive.0, destructive.1)
        .show()
        .await;
    match choice.ok().and_then(|r| r.action) {
        Some(action) => AlertAnswer::Action(action),
        None => AlertAnswer::Dismissed,
    }
}
