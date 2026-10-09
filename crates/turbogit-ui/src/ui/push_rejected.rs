//! The classified reply to a rejected push.
//!
//! "Your push was rejected" is one string for six situations that need six
//! different replies, so this surface paints ONE message per outcome class and
//! only the next step that class can honour. The classes come from
//! [`turbogit_services::sync_service::classify_push_failure`]; the wording is
//! TurboGit's own except where the reference states a remediation clause
//! directly (`push.notification.description.*`) — that phrasing carries the
//! distinction the surface exists to make.
//!
//! # Two hazards, both in the dismissal path
//!
//! A frame renders this surface whenever `state.ui.push_rejection` is `Some`,
//! including the frame in which a control has just cleared it. Hence:
//!
//! 1. **Never read state back off what was painted.** The next-step verb inside
//!    the message is derived from the outcome class and the stored `remember`
//!    flag, never from the label of the control that was just clicked.
//! 2. **A stale rejection must not be dismissable as current.** [`render`]
//!    clones the rejection out of `state` up front and drops state whose
//!    repositories are no longer registered, so no click arm can dispatch work
//!    at a path that is gone — and no arm borrows `state.ui` across the call
//!    that clears it.

use crate::theme::Palette;
use crate::ui::widgets;
use egui::{RichText, Ui};
use turbogit_app::operation::{RecoveryMode, RecoveryPlan};
use turbogit_app::state::{AppState, PushRejection};
use turbogit_domain::model::{RootId, UpdateMethod};
use turbogit_services::sync_service::{PushOutcome, UpdateChoice};

/// The reply one class offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextAction {
    /// Fetch, integrate with the chosen update method, then push again.
    UpdateAndRetry,
    /// Fetch only, then push again: the remote moved, and an integration is
    /// not what was asked for.
    FetchAndRetry,
    /// Hand off to the conflict resolver, then push again.
    ResolveConflicts,
}

impl NextAction {
    /// The primary control's label, and the verb the message names for the
    /// same reply — one constant per class, so the sentence and the control
    /// cannot drift apart.
    fn verb(self) -> &'static str {
        match self {
            Self::UpdateAndRetry => "Update and retry",
            Self::FetchAndRetry => "Fetch and retry",
            Self::ResolveConflicts => "Resolve conflicts",
        }
    }
}

/// Paint the rejection notification. Called unconditionally by `ui::render`
/// like every other floating surface: the Push dialog closes the moment it
/// dispatches, so the reply to a rejection cannot live inside it.
pub fn render(ui: &mut Ui, state: &mut AppState) {
    let Some(rejection) = state.ui.push_rejection.clone() else {
        return;
    };
    // Staleness is judged on the STORED request, never on a galley from the
    // previous frame — the frame that dismisses still holds the old outcome.
    if is_stale(state, &rejection) {
        state.ui.push_rejection = None;
        return;
    }
    let title = window_title(&rejection.outcome);
    let message = outcome_message(&rejection, &state.settings);
    let mut open = true;
    egui::Window::new(&title)
        .id(egui::Id::new("push_rejected"))
        .open(&mut open)
        .default_width(480.0)
        .show(&ui.ctx().clone(), |ui| {
            ui.colored_label(
                Palette::STATE_ERROR,
                RichText::new(&message).color(Palette::STATE_ERROR),
            );
            controls(ui, state, &rejection);
        });
    if !open {
        state.ui.push_rejection = None;
    }
}

/// The notification's controls: one filled next-step action plus a dismissal
/// for the four classes that have a next step, and the bare dismissal for the
/// two that have none. An empty-handed notification offers no button it cannot
/// honour.
fn controls(ui: &mut Ui, state: &mut AppState, rejection: &PushRejection) {
    if let Some(action) = next_step(&rejection.outcome)
        && widgets::compact_primary_button(ui, action.verb()).clicked()
    {
        // Cloned out of `state` BEFORE anything mutates: the dispatch
        // rewrites `push_in_flight` and the dismissal drops the rejection,
        // and neither can happen through a live borrow of the other.
        run_next_step(state, rejection, action);
        state.ui.push_rejection = None;
        return;
    }
    if widgets::compact_button(ui, "Dismiss").clicked() {
        state.ui.push_rejection = None;
        return;
    }
    // The recovery's two choices belong to the class whose reply IS an update:
    // the fetch classes integrate nothing, and the resolve class runs no work
    // at all, so neither has a scope or a method to remember.
    if matches!(rejection.outcome, PushOutcome::Rejected { .. }) {
        update_choices(ui, state, rejection);
    }
}

/// The recovery's scope and memory.
///
/// The scope control defaults to affected-only — the conservative reading of a
/// reference default the docs never state, because a rejection in one repository
/// must not integrate other repositories' branches behind the user.
///
/// The checkbox is the reference's settings round-trip rather than a per-push
/// choice: ticking writes `auto_update_on_push_rejected` at once and un-ticking
/// clears it again, so the opt-in reverses in place as well as in Settings. It
/// remembers the METHOD by reference — the update reads `update_method` from
/// settings when it runs, so there is no second copy of it to keep in sync.
fn update_choices(ui: &mut Ui, state: &mut AppState, rejection: &PushRejection) {
    ui.separator();
    ui.label(RichText::new("UPDATE SCOPE").small().strong());
    ui.horizontal(|ui| {
        if ui
            .selectable_label(!rejection.update_all, UpdateChoice::AffectedOnly.label())
            .clicked()
        {
            set_update_all(state, false);
        }
        if ui
            .selectable_label(rejection.update_all, UpdateChoice::AllRepositories.label())
            .clicked()
        {
            set_update_all(state, true);
        }
    });
    let Some(current) = state.ui.push_rejection.as_mut() else {
        return;
    };
    let changed = ui
        .checkbox(
            &mut current.remember,
            "Remember the update method choice and silently update in the future",
        )
        .changed();
    if changed {
        let enabled = current.remember;
        // The flag's only write site, and it mirrors the checkbox exactly: a
        // tick that took effect only once an action followed would leave the
        // message above promising silent updates the settings do not yet
        // authorise. That message is built from the rejection `render` cloned
        // at the top of the frame — the standard one-frame lag for a control
        // that changes other content, not a bug to paper over.
        set_auto_update(state, enabled);
    }
}

/// Write the silent-update opt-in to the live settings, the open Settings
/// draft, and disk.
///
/// The draft too: `settings_modal` copies it over `state.settings` when the
/// modal is applied, so a draft opened before this tick would silently undo it.
fn set_auto_update(state: &mut AppState, enabled: bool) {
    state.settings.auto_update_on_push_rejected = enabled;
    if let Some(draft) = state.ui.settings_draft.as_mut() {
        draft.auto_update_on_push_rejected = enabled;
    }
    let _ = turbogit_app::persistence::save_settings(&state.project_dir, &state.settings);
}

/// Widen or narrow the recovery's scope on the stored rejection.
fn set_update_all(state: &mut AppState, all: bool) {
    if let Some(rejection) = state.ui.push_rejection.as_mut() {
        rejection.update_all = all;
    }
}

/// The reply a class offers, or `None` for the two classes that have none.
///
/// The two fetch classes share one reply because their reply IS the same —
/// refresh what the remote holds, then push again — while their MESSAGES
/// differ, which is the point of the split: the message is what the user reads
/// before choosing. The ordinary rejection differs from both, because it needs
/// the branch integrated, not merely refreshed.
fn next_step(outcome: &PushOutcome) -> Option<NextAction> {
    match outcome {
        PushOutcome::Rejected { .. } => Some(NextAction::UpdateAndRetry),
        PushOutcome::RejectedByRemote { .. } | PushOutcome::LeaseRejected { .. } => {
            Some(NextAction::FetchAndRetry)
        }
        PushOutcome::UpdateConflicted => Some(NextAction::ResolveConflicts),
        // Neither has a reply this surface can honour: one is git's own error to
        // describe, the other is a conflict state that predates the push and is
        // already the conflict resolver's to show.
        PushOutcome::UpdateUnresolved | PushOutcome::UpdateErrored { .. } => None,
    }
}

/// Run the chosen next step.
fn run_next_step(state: &mut AppState, rejection: &PushRejection, action: NextAction) {
    match action {
        NextAction::ResolveConflicts => {
            // The resolver is the app's conflict surface; opening it IS the
            // reply, and the re-push waits for the user to finish in it.
            state.ui.conflict_resolver_open = true;
        }
        NextAction::UpdateAndRetry | NextAction::FetchAndRetry => {
            let mode = match action {
                NextAction::UpdateAndRetry => RecoveryMode::Update {
                    method: state.settings.update_method,
                },
                _ => RecoveryMode::Fetch,
            };
            // No settings write here: `remember` is written the moment the
            // checkbox changes (see [`update_choices`]), so by the time an
            // action carries it the opt-in is already on disk, and a second
            // writer would be a second place for it to drift.
            let roots = recovery_roots(state, rejection);
            let request = rejection.request.clone();
            state.dispatch_push(request, Some(RecoveryPlan { mode, roots }));
        }
    }
}

/// The roots the recovery updates: the failed push's own roots, or — when the
/// user widened the choice — every registered root.
fn recovery_roots(state: &AppState, rejection: &PushRejection) -> Vec<RootId> {
    if rejection.update_all {
        state.multi.roots.iter().map(|r| r.id.clone()).collect()
    } else {
        rejection.affected_roots.clone()
    }
}

/// A rejection is stale once none of the repositories it describes is still
/// registered: its message is about a root the user cannot act on, and its
/// controls would dispatch work at a path that is gone.
fn is_stale(state: &AppState, rejection: &PushRejection) -> bool {
    !rejection
        .affected_roots
        .iter()
        .any(|id| state.multi.roots.iter().any(|r| &r.id == id))
}

/// The outcome's title. Each class names what failed, because one bare "Push
/// rejected" would collapse six different failures into one reply.
fn window_title(outcome: &PushOutcome) -> String {
    match outcome {
        PushOutcome::Rejected { .. } => "Push rejected".to_string(),
        PushOutcome::RejectedByRemote { .. } => "Push rejected by the remote".to_string(),
        PushOutcome::LeaseRejected { .. } => "Force-with-lease push rejected".to_string(),
        PushOutcome::UpdateConflicted => "Push canceled — update conflicted".to_string(),
        PushOutcome::UpdateUnresolved => "Push canceled — conflicts unresolved".to_string(),
        PushOutcome::UpdateErrored { .. } => "Push rejected — update failed".to_string(),
    }
}

/// The one sentence that says what happened, and the next step the class
/// offers. Where a class has a next step the sentence names it too, so the
/// message and the control agree — but the verb is written from the class,
/// never read back off the painted control.
fn outcome_message(
    rejection: &PushRejection,
    settings: &turbogit_domain::model::VcsSettings,
) -> String {
    match &rejection.outcome {
        // The reference's no-target-named case names the REMOTE only: what the
        // user needs is which remote refused the fast-forward. With the
        // opt-in on, the next step is the silent update, not an instruction to
        // do it by hand — a different SENTENCE, not an appended suffix.
        PushOutcome::Rejected { remote, branch } => {
            let head = format!("Push{} was rejected.", named(remote, " to ", ""));
            if rejection.remember {
                format!(
                    "{head} Future pushes of {branch} will update silently using{}.",
                    method_phrase(settings.update_method)
                )
            } else {
                format!(
                    "{head} Update {branch} to incorporate the remote's changes, then push \
                     again."
                )
            }
        }
        PushOutcome::RejectedByRemote { remote, branch } => {
            format!(
                "Push {} was rejected by the remote. Fetch to refresh what the remote holds, \
                 then push again.",
                branch_to(branch, remote)
            )
        }
        PushOutcome::LeaseRejected { remote, branch } => {
            format!(
                "Force-with-lease push {} was rejected: the lease, not the push, is stale. \
                 Fetch to refresh it, then push again.",
                branch_to(branch, remote)
            )
        }
        PushOutcome::UpdateConflicted => {
            "Push was canceled, because there were conflicts during the update. Check that \
             conflicts were resolved correctly, and invoke push again."
                .to_string()
        }
        PushOutcome::UpdateUnresolved => {
            "Push was canceled, because not all conflicts were resolved during the update. \
             Resolve the conflicts and try pushing again."
                .to_string()
        }
        PushOutcome::UpdateErrored { detail } => {
            format!("Push was rejected, and update failed with an error: {detail}")
        }
    }
}

/// The remembered update method as prose. Read from the settings the update
/// itself will use, so the sentence cannot promise a method the recovery does
/// not apply.
fn method_phrase(method: UpdateMethod) -> String {
    match method {
        UpdateMethod::Rebase => " a rebase".to_string(),
        UpdateMethod::Merge => " a merge".to_string(),
    }
}

/// `"{prefix}{value}{suffix}"` when `value` is known, and nothing at all when
/// it is not — so an unnamed target leaves no dangling preposition behind.
fn named(value: &str, prefix: &str, suffix: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("{prefix}{value}{suffix}")
    }
}

fn branch_to(branch: &str, remote: &str) -> String {
    match (branch.is_empty(), remote.is_empty()) {
        (false, false) => format!("{branch} to {remote}"),
        (false, true) => branch.to_string(),
        _ => String::new(),
    }
}
