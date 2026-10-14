//! Settlement (CONTEXT.md "Settlement"): what the application does once a
//! dispatched [`Operation`](crate::operation::Operation) has finished.
//!
//! One completion, one owner. This module owns the post-completion policy —
//! the toasts, the refresh, the activity entries, the undo armings, the
//! conflict hand-off and the bulk run's record — and
//! [`crate::state::AppState::drain_events`] routes each completion event to
//! exactly one of its two entry points, [`op_completed`] and
//! [`bulk_completed`]. It is shaped like [`granular`]: the rules move here,
//! the storage stays in [`crate::state::UiState`].
//!
//! It does **not** cover the completion events the other arms of the drain
//! settle — a diff answer, a log batch, a worktree listing. Those are reads
//! arriving, not an [`Operation`](crate::operation::Operation) finishing, and
//! they have no [`OpKind`] to route on.
//!
//! One sentence per load-bearing difference between the two sides of a
//! completion:
//!
//! - **The apply's outcome is read before the two-way split.** The completion
//!   event carries `TgResult<()>`, while an apply that failed can fail *and*
//!   fail to roll back — three states, one slot, armed by the dispatch and read
//!   here, so the failure arm can name the paths the rollback failed on.
//! - **The conflict count is read after the refresh.** A merge/rebase that
//!   landed mid-conflict is a hand-off rather than a failure, and the only
//!   reliable signal is the worktree's fresh conflict list; the count therefore
//!   comes from the refreshed snapshots, never from git's stderr text and never
//!   from the pre-refresh cache. The activity entry's Warning case is correct
//!   for exactly this reason.
//! - **`Continue` is a value, not a label.** Today's conflict hand-off uses a
//!   bare `continue` in the drain loop; [`Control`] keeps that expressible
//!   without threading a label through a loop that 17 other arms share.
//! - **The rules are named; the routing is one table.** What a completion does
//!   is a sequence of [`settle_ok`]'s and [`settle_err`]'s rules, each in its
//!   own function, and which of them a kind adds is decided once, by the total
//!   `match` in [`rules_for`] — never by a `kind ==` test at the call site.

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{DisplayName, RootId};
use turbogit_services::bulk_run;

use crate::activity::{ActivityEntry, ActivityKind};
use crate::bulk_history::BulkRunRecord;
use crate::granular;
use crate::operation::{OpKind, PushRequest};
use crate::root_caches::Affected;
use crate::state::{
    AppState, BranchDeleteUndo, BranchDivergence, DivergentRoot, PushRejection, Toast, ToastKind,
};

/// What a completion leaves the drain loop to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Resume the loop. Today's conflict hand-off used a bare `continue`, and it
    /// must stay expressible without a label on a loop nineteen other arms share.
    Continue,
    /// The loop's ordinary next step.
    Done,
}

/// Every [`OpKind`] this module routes, in declaration order.
///
/// The value-level enumeration [`rules_for`] and [`report_for`] are held to: a
/// new operation has to say here how it settles before it can be dispatched.
/// `settle_routed_kinds` compares this set with the one `Operation::kind()`
/// actually produces.
pub const ROUTED_KINDS: &[OpKind] = &[
    OpKind::Fetch,
    OpKind::Merge,
    OpKind::Rebase,
    OpKind::DeleteBranch,
    OpKind::Shelve,
    OpKind::Unshelve,
    OpKind::WorktreeAdd,
    OpKind::WorktreeRemove,
    OpKind::DropCommit,
    OpKind::RewordCommit,
    OpKind::Push,
    OpKind::PushRecovery,
    OpKind::Resolve,
    OpKind::GetFileFromBranch,
    OpKind::ApplyPatch,
    OpKind::Other,
];

/// The rules a completion runs beyond the ones every operation shares.
///
/// Most of what settlement does is not per-kind: the success toast, the refresh
/// and the granular staging settlement run for whatever finished. What is
/// per-kind used to be six `kind ==` tests and two `matches!` tests scattered
/// through two inline arms, which is why `operation.rs`'s module doc could not
/// keep its promise. These flags are the single enumeration of that half of the
/// policy, chosen by the total `match` in [`rules_for`] — so a new [`OpKind`]
/// is a compile error there rather than a verb that quietly takes the generic
/// path.
#[derive(Clone, Copy)]
struct RuleSet {
    /// Arm the post-hoc undo for a rewrite that landed.
    rewrite_undo: bool,
    /// Re-read the rungs a take rewrote under the pane that offered it.
    reload_get_from_branch_rows: bool,
    /// The fetch's own plain "what changed" toast.
    fetch_report: bool,
    /// Arm the branch-delete undo from the record the confirm verb captured.
    branch_delete_undo: bool,
    /// `Unshelve` alone: on the success side the shelf record goes, on the
    /// failure side the removal it armed is dropped.
    shelf_removal: bool,
    /// Classify a rejected push instead of reporting one failure.
    push_rejection: bool,
    /// Record an apply's titled outcome, which git never learned itself.
    apply_outcome: bool,
}

impl RuleSet {
    /// The generic path: the shared rules and nothing else.
    const NONE: Self = Self {
        rewrite_undo: false,
        reload_get_from_branch_rows: false,
        fetch_report: false,
        branch_delete_undo: false,
        shelf_removal: false,
        push_rejection: false,
        apply_outcome: false,
    };
}

/// Every rule one kind adds to the shared ones — one total `match`, and the
/// point of the module's interface.
///
/// The order the rules run in is the call sequence in [`settle_ok`] and
/// [`settle_err`], not the order of these arms: `refresh` interleaves with the
/// conflict count, so the sequence is what preserves it.
fn rules_for(kind: OpKind) -> RuleSet {
    match kind {
        OpKind::Fetch => RuleSet {
            fetch_report: true,
            ..RuleSet::NONE
        },
        // The conflict hand-off is `hands_off_conflicts`' to decide — both
        // halves ask it, so the two cannot disagree about which verbs hand off.
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
        OpKind::Shelve | OpKind::WorktreeAdd | OpKind::WorktreeRemove => RuleSet::NONE,
        // `Operation::Custom` settles on the shared path alone: its label,
        // its scope and its work are the dispatcher's, and there is no rule
        // the application can apply to it. Spelled out because an implicit
        // tail is where a future kind would disappear.
        OpKind::Other => RuleSet::NONE,
    }
}

/// Settle one completed [`Operation`](crate::operation::Operation).
///
/// `busy` and the repository-changed guard are the drain loop's own
/// bookkeeping and stay in [`crate::state::AppState::drain_events`]; everything
/// from the apply's pre-result read onwards is settlement.
pub fn op_completed(
    state: &mut AppState,
    kind: OpKind,
    label: String,
    affected: Affected,
    result: TgResult<()>,
) -> Control {
    // The apply's outcome is three-way and the event it travels on
    // is two-way, so the record comes off the slot its dispatch
    // armed — read before either arm below, because `report_for`
    // asks the window what actually happened and the failure arm
    // has to name the paths the rollback failed on.
    if kind == OpKind::ApplyPatch {
        state.settle_apply_patch();
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

/// Settle one completed bulk run (issue 12): the aggregate toast, the durable
/// record, and the run's own entry in the history list.
pub fn bulk_completed(state: &mut AppState, label: String, results: Vec<(RootId, TgResult<()>)>) {
    state.refresh(Affected::All);
    let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
    // **One read of the run's rows.** The skip reasons, the guard refusals and
    // the history record are three accounts of the same monitor's final truth,
    // and they used to be taken from three separate walks of it — so a row that
    // changed between two walks could be counted in one account and missing
    // from another. Everything derived from the view is derived here, before
    // the first mutation, and the steps that consume it keep today's order.
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
    // An empty result set would satisfy `ok == results.len()`
    // and settle as a success — but a run that planned no
    // roots ran nothing, so the zero case is decided here,
    // loudly, from the monitor's own skip rows.
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
    // The remotes manager renders the per-repo outcomes of
    // its apply (issue 33): display-ready rows in selection
    // order. Harmless for the bulk grid's own runs.
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
    // The durable record of the fleet run (issue #04
    // semantics): one entry with the aggregate outcome.
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
    // An execution-time guard refusal (a Retry forcing a
    // protected root lands in `run_step`, which refuses) is
    // durable per root too, naming the branch the guard
    // named — the aggregate above only names the repos.
    for (name, error) in refusals {
        state.ui.activity.push(ActivityEntry {
            at: chrono::Local::now(),
            repo: Some(name),
            message: format!("{label}: {error}"),
            kind: ActivityKind::Error,
        });
    }
    // Recent bulk operations (issue 12, screen 04): the
    // completed run lands in the history list — merged into
    // the same record on a retry pass. The monitor's rows
    // are the run's final truth (done/failed/skipped).
    //
    // The record is built out above, and only now handed to the
    // list, because the divergence offer is read from those
    // same per-root rows: the run's monitor is the one place
    // that knows which repositories moved and which did not, so
    // a second account of the outcome could disagree with the
    // surface the developer just watched.
    if let Some(record) = record {
        let diverged = branch_divergence_for(&record);
        if let Some(diverged) = &diverged {
            // The divergence is a durable feed entry for ANY
            // run that moved some roots and failed others —
            // reversible or not. The offer below can be declined
            // or dismissed, but the feed is what makes a
            // multi-root operation auditable later, so it arms
            // even when no rollback can be offered (Custom).
            state.ui.activity.push(ActivityEntry {
                at: chrono::Local::now(),
                repo: Some("project".to_string()),
                message: divergence_report(diverged, "diverged"),
                kind: ActivityKind::Warning,
            });
        }
        // The offer alone stays gated on `reversible()`: the
        // record names what diverged for every run, but a
        // `Rollback` the app cannot honour (arbitrary git under
        // Custom) is a false promise, so the control must not
        // appear. One offer at a time, always the newest run's —
        // a later run that lands everywhere replaces a stale
        // divergence rather than stacking a second modal on it.
        state.ui.branch_divergence = if record.reversible() { diverged } else { None };
        crate::bulk_history::record_into(&mut state.ui.bulk_history, record);
        state.save_durable_ui();
    }
}

/// The display-ready reason each row the monitor skipped carries, in row order.
/// The caller sorts and de-duplicates them: the *count* is roots, the list is
/// distinct reasons, and collapsing the two would misreport the fleet.
fn skip_reasons(view: &crate::bulk_run_view::BulkRunView) -> Vec<String> {
    view.rows
        .iter()
        .filter_map(|r| match &r.state {
            bulk_run::RowState::Skipped { reason } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

/// The guard refusals the monitor recorded, as `(root name, git's own reason)`.
/// The `protected_branch` flag is decided while the step's error is still a
/// type; this is the only place it is read, and a refusal settlement owes it a
/// durable per-root entry rather than re-reading the row's prose.
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

/// The success-side post-completion rules, in the order they run.
///
/// The order is the behaviour, not the formatting: `refresh` interleaves with
/// the conflict count further down, so a line moved here is a behaviour
/// changed and not a refactor.
fn settle_ok(state: &mut AppState, rules: RuleSet, kind: OpKind, label: &str, affected: &Affected) {
    report_success(state, label);
    clear_push_in_flight(state);
    refresh_affected(state, affected);
    arm_landed_rewrite_undo(state, rules);
    granular::settle(state);
    reload_get_from_branch_rows(state, rules);
    // Rules seven to nine all read what rules three to six left behind, which
    // is why they take this arm's results as arguments rather than re-reading
    // the state each one happens to want.
    let (conflicts, message) = record_completion_in_activity(state, kind, label, affected);
    report_what_the_fetch_changed(state, rules, &message);
    open_conflict_handoff(state, kind, conflicts);
    arm_branch_delete_undo(state, rules);
    remove_shelf_record(state, rules);
}

/// Rule ten (issue 12): a confirmed local-branch deletion arms the short-window
/// undo with the captured tip. The disclosure the notification carries — what
/// was unmerged, and what remote was left tracking nothing — is the pending
/// record's, because the branch it describes no longer exists to be read.
///
/// The pending is taken whatever the kind, exactly as it always was: a
/// confirmation the user opened and never completed must not leave one behind.
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

/// Rule eleven (surface 07): `Unshelve and Remove` is the only verb on this
/// surface whose removal spans both sides of the boundary — the stash entry goes
/// in git, the shelf record goes here. The record drops only on this arm: a pop
/// that failed left the shelved content in the stash list, and the row that
/// points at it has to stay painted.
fn remove_shelf_record(state: &mut AppState, rules: RuleSet) {
    if rules.shelf_removal
        && let Some((root, name)) = state.ui.shelf_removal_pending.take()
    {
        state.remove_shelf(&root, &name);
    }
}

/// Rule six: a take rewrote one working-tree file, so the changes window that
/// offered it re-reads its rung list — a path whose contents now match is no
/// longer a row, and a window still listing it would offer the same overwrite
/// again. The re-read is the app's, through the service, never the pane's, and
/// it follows the refresh above so both see the taken file.
fn reload_get_from_branch_rows(state: &mut AppState, rules: RuleSet) {
    if rules.reload_get_from_branch_rows {
        state.reload_get_from_branch_rows();
    }
}

/// Rule seven: the activity log (issue #04) — the durable record the toast only
/// summarizes. An op that leaves conflicts behind is a warning, not a success.
///
/// **The count is read here, after `refresh_affected` ran, and that is the whole
/// point of this rule.** The snapshots are the only reliable statement of what
/// conflicts the operation left; git's stderr text is not stable and the
/// pre-refresh cache says nothing about this operation at all. Moving the read
/// above the refresh compiles and passes most of the suite, and makes the
/// Warning case wrong.
///
/// Returns the count it read and the sentence it wrote, because the two rules
/// below consume both rather than recomputing them.
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

/// Rule eight (issue 13): a fetch reports plainly what changed — never a bare
/// success, never a silent no-op. It quotes the sentence rule seven wrote, so
/// the toast and the feed can never name two different outcomes for one fetch.
fn report_what_the_fetch_changed(state: &mut AppState, rules: RuleSet, message: &str) {
    if rules.fetch_report {
        state.show_toast(Toast::success(message.to_string()));
    }
}

/// Rule nine (issue 09): a merge/rebase that landed mid-conflict hands off
/// cleanly to the conflict experience with an explicit mid-operation state that
/// survives switching tabs. The count is rule seven's, for the same reason the
/// activity entry's Warning case is.
fn open_conflict_handoff(state: &mut AppState, kind: OpKind, conflicts: usize) {
    if conflicts > 0 && hands_off_conflicts(kind) {
        state.ui.merge_in_progress = true;
        state.ui.conflict_resolver_open = true;
    }
}

/// Rule one: the success toast, under the operation's own label. The rules
/// after it are what the label cannot say.
fn report_success(state: &mut AppState, label: &str) {
    state.show_toast(Toast::success(label.to_string()));
}

/// Rule two: a push that landed needs no replay request kept around — what it
/// recorded is only ever read by the rejection arm.
fn clear_push_in_flight(state: &mut AppState) {
    state.ui.push_in_flight = None;
}

/// Rule three: the invalidation every completed operation runs.
fn refresh_affected(state: &mut AppState, affected: &Affected) {
    state.refresh(affected.clone());
}

/// Rule four: arm the post-hoc undo while the rewrite's own tip is still the
/// fact to compare against. A pending that does not match this completion's
/// kind is dropped with it.
fn arm_landed_rewrite_undo(state: &mut AppState, rules: RuleSet) {
    if let Some(pending) = state.ui.rewrite_undo_pending.take()
        && rules.rewrite_undo
    {
        state.arm_rewrite_undo(pending);
    }
}

/// The failure-side post-completion rules, in the order they run.
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
    // Rule four is the only one that can end the arm early: a merge/rebase that
    // stopped on a conflict is not a failure, so nothing after it runs. The
    // control value is what lets the drain loop's own `continue` stay where it
    // is instead of becoming a labelled `break` on a loop nineteen arms share.
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

/// Rule one: a failed delete armed nothing worth keeping. The pending record and
/// the unmerged-commit disclosure go together, because the disclosure only
/// means anything while the confirmation is still on screen.
fn clear_delete_pending(state: &mut AppState) {
    state.ui.branches_delete_pending = None;
    state.ui.branches_delete_unmerged = None;
}

/// Rule two: a rewrite that failed arms no undo — the offer exists for a landed
/// rewrite only.
fn clear_rewrite_undo_pending(state: &mut AppState) {
    state.ui.rewrite_undo_pending = None;
}

/// Rule three: a rewrite that failed moved nothing, so the commit the developer
/// was looking at is still in the history and still selected, and nothing
/// follows it.
fn clear_reselect(state: &mut AppState, affected: &Affected) {
    state.clear_pending_reselect(affected);
}

/// Rule four (issue 09): a merge/rebase that hits conflicts is not a failure —
/// it is a hand-off to the conflict experience, with an explicit
/// mid-operation state that survives switching tabs. Returns whether it handed
/// off, which is the caller its only way to stop.
///
/// **The probe refreshes before it counts**, for the same reason the success
/// arm's rule seven does: the worktree's fresh conflict list is the reliable
/// signal and git's own stderr text is not stable here. It compiles and passes
/// either way, which is why it is written down.
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

/// Rule five: a rejected push is not one failure, it is six, and they need six
/// different replies. The toast still says what git said — this is the surface
/// that says what to DO about it.
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
    // Same rule the merge/rebase arm uses: the
    // refreshed worktree, not git's stderr text,
    // is what says conflicts are really there.
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
    // The opt-in the rejection notification's
    // checkbox writes: an ordinary rejection then
    // updates silently instead of asking a second
    // time. Gated on `OpKind::Push` — a failed
    // RECOVERY always shows its reply, or the two
    // would re-dispatch each other forever. The
    // fetch classes are out too: the user opted
    // into an update, not into an integration they
    // did not ask for.
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
            // The documented fallback for an
            // undocumented reference default: touch
            // only what the failed push touched.
            update_all: false,
            remember: state.settings.auto_update_on_push_rejected,
        });
    }
}

/// Rule six: the error toast, and the same sentence kept for whatever reads
/// `last_error` after the toast has gone.
fn report_failure(state: &mut AppState, label: &str, e: &turbogit_domain::error::TgError) {
    state.show_toast(Toast::error(format!("{label}: {e}")));
    state.last_error = Some(e.to_string());
}

/// Rule seven: an unshelve that never landed keeps its shelf row, so the record
/// it armed for removal is dropped here rather than left to a later, unrelated
/// success.
fn drop_shelf_removal(state: &mut AppState, rules: RuleSet) {
    if rules.shelf_removal {
        state.ui.shelf_removal_pending = None;
    }
}

/// Rule eight (issue #04): failures are entries too — the toast vanishes, the
/// feed remembers.
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

/// Rule nine: an apply that ended in one of its two titled states is recorded a
/// second time, by that state's own name: the line above carries git's apply
/// error, and git never learned what the rollback then did.
fn record_apply_outcome(state: &mut AppState, rules: RuleSet) {
    if rules.apply_outcome {
        state.record_apply_outcome();
    }
}

/// Repo label for one activity entry (issue #04): the affected root's
/// display name, or `None` when the op spanned every root of the project.
pub(crate) fn activity_repo_label(affected: &Affected) -> Option<String> {
    match affected {
        Affected::All => None,
        Affected::Root(id) => {
            let path = id.as_path();
            Some(path.display_name_or(&path.display().to_string()))
        }
    }
}

/// The roots a completed run left in disagreement, or `None` when nothing
/// diverged.
///
/// Two facts must hold together, and each one is the reason a near-miss must
/// not fire:
/// 1. the run CHANGED at least one root — a run that failed everywhere moved
///    nothing, so there is nothing to report;
/// 2. it FAILED in at least one other root — a run that landed everywhere left
///    the roots agreeing, which is the outcome everybody wanted.
///
/// This is the *record* of a divergence and deliberately says nothing about
/// whether a rollback can be *honoured* — that is `reversible()`'s call, made
/// at the offer's arm site, never here. A `Custom` run moves arbitrary git and
/// is not reversible, so it must still produce this record (the roots disagree
/// and nothing repairs them) while producing no offer (the app cannot undo what
/// it did not arrange). Fusing the two decisions is the bug: it would either
/// promise a rollback that fails on the spot or leave a divergence unrecorded.
///
/// A `Skipped` row is in none of the lists deliberately. The preflight matrix
/// refused it before anything ran and the monitor says why, which is the
/// multi-root *skip* policy the sync family runs on; this record is the other,
/// opposite theory of failure and the two are not to be unified.
fn branch_divergence_for(record: &BulkRunRecord) -> Option<BranchDivergence> {
    use crate::bulk_history::RepoOutcome;

    let mut changed: Vec<DivergentRoot> = Vec::new();
    let mut failed: Vec<DivergentRoot> = Vec::new();
    for row in &record.repos {
        let root = RootId(row.root.clone().into());
        match &row.outcome {
            RepoOutcome::Done => {
                // A root the run found already on the target branch was not
                // changed by it, so it is not one side of the divergence — and
                // a rollback that acted on it would move a repository the run
                // never moved.
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
                    // The run never landed here, so this is still the branch the
                    // root was on when the run started — the other half of the
                    // disagreement the sentence has to state.
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

/// The feed's line for one side of a divergence: `alpha (feature/x)`.
fn divergence_sides(roots: &[DivergentRoot]) -> String {
    roots
        .iter()
        .map(|r| format!("{} ({})", r.name, r.branch))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The durable sentence for a divergence and for the answer to the offer.
///
/// One builder for both records because both must name the same two halves —
/// the roots the run changed and the roots it failed in, each with the branch it
/// is on — and a feed entry that said only "partially failed" would leave a
/// later reader unable to tell which repository disagrees.
pub(crate) fn divergence_report(offer: &BranchDivergence, verb: &str) -> String {
    format!(
        "{} {verb} · {} · changed: {} · failed: {}",
        offer.operation,
        offer.branch,
        divergence_sides(&offer.changed),
        divergence_sides(&offer.failed)
    )
}

/// Unresolved conflict files across `affected`'s registered roots, read
/// from the (refreshed) snapshots.
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

/// The `(remote, branch)` a rejection notification names.
///
/// The `ThisRepo` path states both in its own fields. A scoped push resolves
/// the remote per root inside the service, so this reads the same answer back
/// through `push_remote` for the first pushed root — a multi-root rejection
/// then names one target in the notification while the toast and the activity
/// feed name every failing root.
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

/// Which operations hand the developer over to the conflict experience rather
/// than reporting a clean finish.
///
/// The original merge/rebase and an in-flight resolution
/// share one rule: git stops on the NEXT conflict with a non-zero exit and a
/// worktree full of unmerged paths, and the answer to that is the resolver, not
/// a failure toast. Stated once because the two settlement arms must not be able
/// to disagree about which verbs hand off.
fn hands_off_conflicts(kind: OpKind) -> bool {
    matches!(kind, OpKind::Merge | OpKind::Rebase | OpKind::Resolve)
}

/// The success message a completed operation reports, chosen by the
/// operation's identity (ADR-0020). The match is exhaustive on purpose: a new
/// [`OpKind`] has to say here how it reports before it can be dispatched.
fn report_for(state: &mut AppState, kind: OpKind, affected: &Affected, label: &str) -> String {
    match kind {
        OpKind::Fetch => state.fetch_report(affected),
        OpKind::Merge | OpKind::Rebase => push_report(state, affected, label),
        // A resolution reports the verb it ran and nothing more: an `Abort` that
        // landed has no ahead/behind story, and appending "nothing to push" to
        // "Abort merge" would describe the repository rather than the act.
        OpKind::Resolve => label.to_string(),
        // A rewrite reports the work it did and nothing more: there is no
        // ahead/behind story to tell, and a reword's new message never reaches
        // the feed.
        OpKind::DeleteBranch
        | OpKind::Shelve
        // The unshelve's label already names which half of the keep/remove pair
        // ran, which is the whole of what this verb has to report.
        | OpKind::Unshelve
        | OpKind::WorktreeAdd
        | OpKind::WorktreeRemove
        | OpKind::DropCommit
        | OpKind::RewordCommit
        // A take reports the verb it ran, and the verb's own label carries the
        // two facts that make it more than a word: the path and the branch it came
        // from. There is no ahead/behind story to append — nothing was
        // integrated, one file was overwritten — and the durable record, not the
        // toast, is where that has to be readable later.
        | OpKind::GetFileFromBranch
        // A push reports the verb it ran. Its SUBSTANCE is the failure path —
        // the classified rejection — and a push that succeeded has
        // nothing to explain, so the label is the whole message.
        | OpKind::Push
        | OpKind::PushRecovery
        | OpKind::Other => label.to_string(),
        // An apply reports the working tree it just wrote, in the outcome's own
        // words: the file count and the fact that nothing was staged are the two
        // things a developer needs from a patch verb, and the window's titled
        // state reads the same record rather than a second copy of it.
        OpKind::ApplyPatch => state
            .ui
            .dlg
            .apply_patch_outcome
            .as_ref()
            .map(|outcome| format!("{} — {}", outcome.title(), outcome.message()))
            .unwrap_or_else(|| label.to_string()),
    }
}

/// Success message for a completed merge/rebase (issue 09): the label already
/// carries how many commits came in when the dialog knew; the push state is
/// read from the freshly refreshed ahead/behind cache — "3 commits to push"
/// or "nothing to push".
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
