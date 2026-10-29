use crate::theme::Palette;
use crate::ui::widgets;
use egui::{RichText, Ui};
use turbogit_app::operation::{RecoveryMode, RecoveryPlan};
use turbogit_app::state::{AppState, PushRejection};
use turbogit_app::sync_service::{PushOutcome, UpdateChoice};
use turbogit_domain::model::{RootId, UpdateMethod};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextAction {
    UpdateAndRetry,
    FetchAndRetry,
    ResolveConflicts,
}

impl NextAction {
    fn verb(self) -> &'static str {
        match self {
            Self::UpdateAndRetry => "Update and retry",
            Self::FetchAndRetry => "Fetch and retry",
            Self::ResolveConflicts => "Resolve conflicts",
        }
    }
}

pub fn render(ui: &mut Ui, state: &mut AppState) {
    let Some(rejection) = state.ui.push_rejection.clone() else {
        return;
    };
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

fn controls(ui: &mut Ui, state: &mut AppState, rejection: &PushRejection) {
    if let Some(action) = next_step(&rejection.outcome)
        && widgets::compact_primary_button(ui, action.verb()).clicked()
    {
        run_next_step(state, rejection, action);
        state.ui.push_rejection = None;
        return;
    }
    if widgets::compact_button(ui, "Dismiss").clicked() {
        state.ui.push_rejection = None;
        return;
    }
    if matches!(rejection.outcome, PushOutcome::Rejected { .. }) {
        update_choices(ui, state, rejection);
    }
}

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
        set_auto_update(state, enabled);
    }
}

fn set_auto_update(state: &mut AppState, enabled: bool) {
    state.settings.auto_update_on_push_rejected = enabled;
    if let Some(draft) = state.ui.settings_draft.as_mut() {
        draft.auto_update_on_push_rejected = enabled;
    }
    let _ = turbogit_app::persistence::save_settings(&state.project_dir, &state.settings);
}

fn set_update_all(state: &mut AppState, all: bool) {
    if let Some(rejection) = state.ui.push_rejection.as_mut() {
        rejection.update_all = all;
    }
}

fn next_step(outcome: &PushOutcome) -> Option<NextAction> {
    match outcome {
        PushOutcome::Rejected { .. } => Some(NextAction::UpdateAndRetry),
        PushOutcome::RejectedByRemote { .. } | PushOutcome::LeaseRejected { .. } => {
            Some(NextAction::FetchAndRetry)
        }
        PushOutcome::UpdateConflicted => Some(NextAction::ResolveConflicts),
        PushOutcome::UpdateUnresolved | PushOutcome::UpdateErrored { .. } => None,
    }
}

fn run_next_step(state: &mut AppState, rejection: &PushRejection, action: NextAction) {
    match action {
        NextAction::ResolveConflicts => {
            state.ui.conflict_resolver_open = true;
        }
        NextAction::UpdateAndRetry | NextAction::FetchAndRetry => {
            let mode = match action {
                NextAction::UpdateAndRetry => RecoveryMode::Update {
                    method: state.settings.update_method,
                },
                _ => RecoveryMode::Fetch,
            };
            let roots = recovery_roots(state, rejection);
            let request = rejection.request.clone();
            state.dispatch_push(request, Some(RecoveryPlan { mode, roots }));
        }
    }
}

fn recovery_roots(state: &AppState, rejection: &PushRejection) -> Vec<RootId> {
    if rejection.update_all {
        state.multi.roots.iter().map(|r| r.id.clone()).collect()
    } else {
        rejection.affected_roots.clone()
    }
}

fn is_stale(state: &AppState, rejection: &PushRejection) -> bool {
    !rejection
        .affected_roots
        .iter()
        .any(|id| state.multi.roots.iter().any(|r| &r.id == id))
}

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

fn outcome_message(
    rejection: &PushRejection,
    settings: &turbogit_domain::model::VcsSettings,
) -> String {
    match &rejection.outcome {
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

fn method_phrase(method: UpdateMethod) -> String {
    match method {
        UpdateMethod::Rebase => " a rebase".to_string(),
        UpdateMethod::Merge => " a merge".to_string(),
    }
}

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
