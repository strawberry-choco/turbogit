use turbogit_domain::error::TgResult;
use turbogit_domain::model::{DisplayName, RootId};
use turbogit_services::bulk_run;

use crate::activity::{ActivityEntry, ActivityKind};
use crate::bulk_history::BulkRunRecord;
use crate::granular;
use crate::operation::{OpKind, PushRequest};
use crate::root_caches::Affected;
use crate::state::{
    AppState, BranchDeleteUndo, BranchDivergence, Dialog, DivergentRoot, PushRejection, Toast,
    ToastKind,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Continue,
    Done,
}

pub const ROUTED_KINDS: &[OpKind] = &[
    OpKind::Fetch,
    OpKind::Merge,
    OpKind::Rebase,
    OpKind::DeleteBranch,
    OpKind::Shelve,
    OpKind::Unshelve,
    OpKind::WorktreeAdd,
    OpKind::WorktreeRemove,
    OpKind::Commit,
    OpKind::DropCommit,
    OpKind::RewordCommit,
    OpKind::Push,
    OpKind::PushRecovery,
    OpKind::Resolve,
    OpKind::GetFileFromBranch,
    OpKind::ApplyPatch,
    OpKind::Other,
];

#[derive(Clone, Copy)]
struct RuleSet {
    rewrite_undo: bool,
    reload_get_from_branch_rows: bool,
    fetch_report: bool,
    branch_delete_undo: bool,
    shelf_removal: bool,
    push_rejection: bool,
    apply_outcome: bool,
    open_push_dialog: bool,
}

impl RuleSet {
    const NONE: Self = Self {
        rewrite_undo: false,
        reload_get_from_branch_rows: false,
        fetch_report: false,
        branch_delete_undo: false,
        shelf_removal: false,
        push_rejection: false,
        apply_outcome: false,
        open_push_dialog: false,
    };
}

fn rules_for(kind: OpKind) -> RuleSet {
    match kind {
        OpKind::Fetch => RuleSet {
            fetch_report: true,
            ..RuleSet::NONE
        },
        OpKind::Merge | OpKind::Rebase | OpKind::Resolve => RuleSet::NONE,
        OpKind::DeleteBranch => RuleSet {
            branch_delete_undo: true,
            ..RuleSet::NONE
        },
        OpKind::DropCommit | OpKind::RewordCommit => RuleSet {
            rewrite_undo: true,
            ..RuleSet::NONE
        },
        OpKind::Push | OpKind::PushRecovery => RuleSet {
            push_rejection: true,
            ..RuleSet::NONE
        },
        OpKind::GetFileFromBranch => RuleSet {
            reload_get_from_branch_rows: true,
            ..RuleSet::NONE
        },
        OpKind::ApplyPatch => RuleSet {
            apply_outcome: true,
            ..RuleSet::NONE
        },
        OpKind::Unshelve => RuleSet {
            shelf_removal: true,
            ..RuleSet::NONE
        },
        OpKind::Commit => RuleSet {
            open_push_dialog: true,
            ..RuleSet::NONE
        },
        OpKind::Shelve | OpKind::WorktreeAdd | OpKind::WorktreeRemove => RuleSet::NONE,
        OpKind::Other => RuleSet::NONE,
    }
}

pub fn op_completed(
    state: &mut AppState,
    kind: OpKind,
    label: String,
    affected: Affected,
    result: TgResult<()>,
) -> Control {
    if kind == OpKind::ApplyPatch {
        crate::apply_patch::settle(state);
    }
    let rules = rules_for(kind);
    match result {
        Ok(()) => {
            settle_ok(state, rules, kind, &label, &affected);
            Control::Done
        }
        Err(e) => settle_err(state, rules, kind, &label, &affected, e),
    }
}

pub fn bulk_completed(state: &mut AppState, label: String, results: Vec<(RootId, TgResult<()>)>) {
    state.refresh(Affected::All);
    let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
    let (mut reasons, refusals, record) = match state.ui.bulk_run.as_ref() {
        Some(view) => (
            skip_reasons(view),
            guard_refusals(view),
            Some(crate::bulk_history::from_run_view(
                view,
                chrono::Utc::now().timestamp_millis(),
            )),
        ),
        None => (Vec::new(), Vec::new(), None),
    };
    let (kind, message) = if results.is_empty() {
        let skipped = reasons.len();
        if skipped == 0 {
            (ToastKind::Error, format!("{label} · nothing ran"))
        } else {
            reasons.sort();
            reasons.dedup();
            (
                ToastKind::Warning,
                format!(
                    "{label} · nothing ran — {skipped} root(s) skipped ({})",
                    reasons.join(", ")
                ),
            )
        }
    } else {
        let kind = if ok == results.len() {
            ToastKind::Success
        } else if ok > 0 {
            ToastKind::Warning
        } else {
            ToastKind::Error
        };
        (kind, crate::state::bulk_report_message(&label, &results))
    };
    state.ui.toast = Some(Toast {
        kind,
        message: message.clone(),
    });
    state.ui.dlg.remotes_apply_results = Some(
        results
            .iter()
            .map(|(id, r)| {
                (
                    id.display_name_or("?"),
                    r.clone().map_err(|e| e.to_string()),
                )
            })
            .collect(),
    );
    state.ui.activity.push(ActivityEntry {
        at: chrono::Local::now(),
        repo: Some("project".to_string()),
        message,
        kind: match kind {
            ToastKind::Success => ActivityKind::Success,
            ToastKind::Warning => ActivityKind::Warning,
            _ => ActivityKind::Error,
        },
    });
    for (name, error) in refusals {
        state.ui.activity.push(ActivityEntry {
            at: chrono::Local::now(),
            repo: Some(name),
            message: format!("{label}: {error}"),
            kind: ActivityKind::Error,
        });
    }
    if let Some(record) = record {
        let diverged = branch_divergence_for(&record);
        if let Some(diverged) = &diverged {
            state.ui.activity.push(ActivityEntry {
                at: chrono::Local::now(),
                repo: Some("project".to_string()),
                message: divergence_report(diverged, "diverged"),
                kind: ActivityKind::Warning,
            });
        }
        state.ui.branch_divergence = if record.reversible() { diverged } else { None };
        crate::bulk_history::record_into(&mut state.ui.bulk_history, record);
        state.save_durable_ui();
    }
}

fn skip_reasons(view: &crate::bulk_run_view::BulkRunView) -> Vec<String> {
    view.rows
        .iter()
        .filter_map(|r| match &r.state {
            bulk_run::RowState::Skipped { reason } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

fn guard_refusals(view: &crate::bulk_run_view::BulkRunView) -> Vec<(String, String)> {
    view.rows
        .iter()
        .filter_map(|r| match &r.state {
            bulk_run::RowState::Failed {
                error,
                protected_branch: true,
            } => Some((r.name.clone(), error.clone())),
            _ => None,
        })
        .collect()
}

fn settle_ok(state: &mut AppState, rules: RuleSet, kind: OpKind, label: &str, affected: &Affected) {
    report_success(state, label);
    clear_push_in_flight(state);
    refresh_affected(state, affected);
    arm_landed_rewrite_undo(state, rules);
    granular::settle(state);
    reload_get_from_branch_rows(state, rules);
    if kind == OpKind::Commit {
        state.ui.commit_no_verify = false;
    }
    let (conflicts, message) = record_completion_in_activity(state, kind, label, affected);
    report_what_the_fetch_changed(state, rules, &message);
    open_conflict_handoff(state, kind, conflicts);
    arm_branch_delete_undo(state, rules);
    remove_shelf_record(state, rules);
    open_push_dialog_owed(state, rules, affected);
}

fn open_push_dialog_owed(state: &mut AppState, rules: RuleSet, affected: &Affected) {
    if rules.open_push_dialog && state.ui.commit_push_dialog_pending.as_ref() == affected.root() {
        state.ui.commit_push_dialog_pending = None;
        state.ui.dialog = Some(Dialog::Push);
    }
}

fn arm_branch_delete_undo(state: &mut AppState, rules: RuleSet) {
    if let Some(pending) = state.ui.branches_delete_pending.take()
        && rules.branch_delete_undo
    {
        state.ui.branches_undo = Some(BranchDeleteUndo {
            root: pending.root,
            name: pending.name,
            tip_sha: pending.tip_sha,
            created_at: std::time::Instant::now(),
            upstream: pending.upstream,
            unmerged: pending.unmerged,
        });
        state.ui.branches_delete_unmerged = None;
    }
}

fn remove_shelf_record(state: &mut AppState, rules: RuleSet) {
    if rules.shelf_removal
        && let Some((root, name)) = state.ui.shelf_removal_pending.take()
    {
        state.remove_shelf(&root, &name);
    }
}

fn reload_get_from_branch_rows(state: &mut AppState, rules: RuleSet) {
    if rules.reload_get_from_branch_rows {
        state.reload_get_from_branch_rows();
    }
}

fn record_completion_in_activity(
    state: &mut AppState,
    kind: OpKind,
    label: &str,
    affected: &Affected,
) -> (usize, String) {
    let conflicts = conflicted_count(state, affected);
    let (entry, message) = if conflicts > 0 {
        (
            ActivityKind::Warning,
            format!("{label} · {conflicts} unresolved conflicts"),
        )
    } else {
        (
            ActivityKind::Success,
            report_for(state, kind, affected, label),
        )
    };
    state.ui.activity.push(ActivityEntry {
        at: chrono::Local::now(),
        repo: activity_repo_label(affected),
        message: message.clone(),
        kind: entry,
    });
    (conflicts, message)
}

fn report_what_the_fetch_changed(state: &mut AppState, rules: RuleSet, message: &str) {
    if rules.fetch_report {
        state.show_toast(Toast::success(message.to_string()));
    }
}

fn open_conflict_handoff(state: &mut AppState, kind: OpKind, conflicts: usize) {
    if conflicts > 0 && hands_off_conflicts(kind) {
        state.ui.merge_in_progress = true;
        state.ui.conflict_resolver_open = true;
    }
}

fn report_success(state: &mut AppState, label: &str) {
    state.show_toast(Toast::success(label.to_string()));
}

fn clear_push_in_flight(state: &mut AppState) {
    state.ui.push_in_flight = None;
}

fn refresh_affected(state: &mut AppState, affected: &Affected) {
    state.refresh(affected.clone());
}

fn arm_landed_rewrite_undo(state: &mut AppState, rules: RuleSet) {
    if let Some(pending) = state.ui.rewrite_undo_pending.take()
        && rules.rewrite_undo
    {
        state.arm_rewrite_undo(pending);
    }
}

fn settle_err(
    state: &mut AppState,
    rules: RuleSet,
    kind: OpKind,
    label: &str,
    affected: &Affected,
    e: turbogit_domain::error::TgError,
) -> Control {
    clear_delete_pending(state);
    clear_rewrite_undo_pending(state);
    clear_reselect(state, affected);
    drop_push_dialog_owed(state, rules, affected);
    if hand_off_failed_conflicts(state, kind, label, affected) {
        return Control::Continue;
    }
    classify_push_rejection(state, rules, kind, affected, &e);
    report_failure(state, label, &e);
    drop_shelf_removal(state, rules);
    record_failure_in_activity(state, label, affected, &e);
    record_apply_outcome(state, rules);
    granular::on_op_failed(state);
    Control::Done
}

fn clear_delete_pending(state: &mut AppState) {
    state.ui.branches_delete_pending = None;
    state.ui.branches_delete_unmerged = None;
}

fn clear_rewrite_undo_pending(state: &mut AppState) {
    state.ui.rewrite_undo_pending = None;
}

fn clear_reselect(state: &mut AppState, affected: &Affected) {
    state.clear_pending_reselect(affected);
}

fn drop_push_dialog_owed(state: &mut AppState, rules: RuleSet, affected: &Affected) {
    if rules.open_push_dialog && state.ui.commit_push_dialog_pending.as_ref() == affected.root() {
        state.ui.commit_push_dialog_pending = None;
    }
}

fn hand_off_failed_conflicts(
    state: &mut AppState,
    kind: OpKind,
    label: &str,
    affected: &Affected,
) -> bool {
    let conflict = if hands_off_conflicts(kind) {
        state.refresh(affected.clone());
        conflicted_count(state, affected) > 0
    } else {
        false
    };
    if !conflict {
        return false;
    }
    granular::settle(state);
    state.ui.merge_in_progress = true;
    state.ui.conflict_resolver_open = true;
    let conflicts = conflicted_count(state, affected);
    let report = format!("{label} · {conflicts} unresolved conflicts");
    state.show_toast(Toast::warning(report.clone()));
    state.ui.activity.push(ActivityEntry {
        at: chrono::Local::now(),
        repo: activity_repo_label(affected),
        message: report,
        kind: ActivityKind::Warning,
    });
    true
}

fn classify_push_rejection(
    state: &mut AppState,
    rules: RuleSet,
    kind: OpKind,
    affected: &Affected,
    e: &turbogit_domain::error::TgError,
) {
    if !rules.push_rejection {
        return;
    }
    let Some(request) = state.ui.push_in_flight.take() else {
        return;
    };
    state.refresh(affected.clone());
    let conflicts = conflicted_count(state, affected) > 0;
    let phase = if kind == OpKind::PushRecovery {
        turbogit_services::sync_service::PushPhase::Recovery
    } else {
        turbogit_services::sync_service::PushPhase::Push
    };
    let (remote, branch) = push_target_names(state, &request);
    let outcome = turbogit_services::sync_service::classify_push_failure(
        e, conflicts, phase, &remote, &branch,
    );
    let affected_roots = request.roots();
    let silent_update = state.settings.auto_update_on_push_rejected
        && kind == OpKind::Push
        && matches!(
            outcome,
            turbogit_services::sync_service::PushOutcome::Rejected { .. }
        );
    if silent_update {
        let plan = crate::operation::RecoveryPlan {
            mode: crate::operation::RecoveryMode::Update {
                method: state.settings.update_method,
            },
            roots: affected_roots,
        };
        state.dispatch_push(request, Some(plan));
    } else {
        state.ui.push_rejection = Some(PushRejection {
            outcome,
            request,
            affected_roots,
            update_all: false,
            remember: state.settings.auto_update_on_push_rejected,
        });
    }
}

fn report_failure(state: &mut AppState, label: &str, e: &turbogit_domain::error::TgError) {
    state.show_toast(Toast::error(format!("{label}: {e}")));
    state.last_error = Some(e.to_string());
}

fn drop_shelf_removal(state: &mut AppState, rules: RuleSet) {
    if rules.shelf_removal {
        state.ui.shelf_removal_pending = None;
    }
}

fn record_failure_in_activity(
    state: &mut AppState,
    label: &str,
    affected: &Affected,
    e: &turbogit_domain::error::TgError,
) {
    state.ui.activity.push(ActivityEntry {
        at: chrono::Local::now(),
        repo: activity_repo_label(affected),
        message: format!("{label}: {e}"),
        kind: ActivityKind::Error,
    });
}

fn record_apply_outcome(state: &mut AppState, rules: RuleSet) {
    if rules.apply_outcome {
        crate::apply_patch::record_outcome(state);
    }
}

pub(crate) fn activity_repo_label(affected: &Affected) -> Option<String> {
    match affected {
        Affected::All => None,
        Affected::Root(id) => {
            let path = id.as_path();
            Some(path.display_name_or(&path.display().to_string()))
        }
    }
}

fn branch_divergence_for(record: &BulkRunRecord) -> Option<BranchDivergence> {
    use crate::bulk_history::RepoOutcome;

    let mut changed: Vec<DivergentRoot> = Vec::new();
    let mut failed: Vec<DivergentRoot> = Vec::new();
    for row in &record.repos {
        let root = RootId(row.root.clone().into());
        match &row.outcome {
            RepoOutcome::Done => {
                if row.prior_branch.as_deref() == Some(record.branch.as_str()) {
                    continue;
                }
                changed.push(DivergentRoot {
                    root,
                    name: row.name.clone(),
                    branch: record.branch.clone(),
                    reason: None,
                });
            }
            RepoOutcome::Failed { error } => {
                failed.push(DivergentRoot {
                    root,
                    name: row.name.clone(),
                    branch: row
                        .prior_branch
                        .clone()
                        .unwrap_or_else(|| "detached HEAD".to_string()),
                    reason: Some(error.clone()),
                });
            }
            RepoOutcome::Skipped { .. } => {}
        }
    }
    if changed.is_empty() || failed.is_empty() {
        return None;
    }
    Some(BranchDivergence {
        record_id: record.id,
        operation: record.op.label().to_string(),
        branch: record.branch.clone(),
        changed,
        failed,
    })
}

fn divergence_sides(roots: &[DivergentRoot]) -> String {
    roots
        .iter()
        .map(|r| format!("{} ({})", r.name, r.branch))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn divergence_report(offer: &BranchDivergence, verb: &str) -> String {
    format!(
        "{} {verb} · {} · changed: {} · failed: {}",
        offer.operation,
        offer.branch,
        divergence_sides(&offer.changed),
        divergence_sides(&offer.failed)
    )
}

fn conflicted_count(state: &AppState, affected: &Affected) -> usize {
    state
        .multi
        .roots
        .iter()
        .filter(|r| match affected {
            Affected::All => true,
            Affected::Root(id) => &r.id == id,
        })
        .map(|r| r.status.conflicted.len())
        .sum()
}

fn push_target_names(state: &AppState, request: &PushRequest) -> (String, String) {
    use crate::operation::PushTargets;
    match &request.targets {
        PushTargets::Single { remote, branch, .. } => (remote.clone(), branch.clone()),
        PushTargets::Scope(roots) => {
            let named = state
                .multi
                .roots
                .iter()
                .find(|r| roots.iter().any(|s| s.id == r.id) && r.current_branch.is_some());
            match named {
                Some(root) => (
                    turbogit_services::sync_service::push_remote(root),
                    root.current_branch.clone().unwrap_or_default(),
                ),
                None => (String::new(), String::new()),
            }
        }
    }
}

fn hands_off_conflicts(kind: OpKind) -> bool {
    matches!(kind, OpKind::Merge | OpKind::Rebase | OpKind::Resolve)
}

fn report_for(state: &mut AppState, kind: OpKind, affected: &Affected, label: &str) -> String {
    match kind {
        OpKind::Fetch => state.fetch_report(affected),
        OpKind::Merge | OpKind::Rebase => push_report(state, affected, label),
        OpKind::Resolve => label.to_string(),
        OpKind::DeleteBranch
        | OpKind::Shelve
        | OpKind::Unshelve
        | OpKind::WorktreeAdd
        | OpKind::WorktreeRemove
        | OpKind::Commit
        | OpKind::DropCommit
        | OpKind::RewordCommit
        | OpKind::GetFileFromBranch
        | OpKind::Push
        | OpKind::PushRecovery
        | OpKind::Other => label.to_string(),
        OpKind::ApplyPatch => state
            .ui
            .dlg
            .apply_patch_outcome
            .as_ref()
            .map(|outcome| format!("{} — {}", outcome.title(), outcome.message()))
            .unwrap_or_else(|| label.to_string()),
    }
}

fn push_report(state: &AppState, affected: &Affected, label: &str) -> String {
    let ahead = state
        .multi
        .roots
        .iter()
        .filter(|r| match affected {
            Affected::All => true,
            Affected::Root(id) => &r.id == id,
        })
        .filter_map(|r| state.caches.ahead_behind(&r.id))
        .map(|(a, _)| a)
        .sum::<usize>();
    if ahead > 0 {
        format!("{label} · {ahead} commit(s) to push")
    } else {
        format!("{label} · nothing to push")
    }
}
