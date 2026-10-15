//! The `Apply patch` verb's flow — sync, seed, preflight, dispatch, settle,
//! record, retry — as free functions over [`AppState`], the shape
//! [`crate::granular`] and [`crate::settle`] already take. The window's storage
//! stays in [`crate::state::DialogState`]; this module owns the policy over it.
//!
//! The one invariant this module exists to hold: **the rollback record belongs
//! to the tree, not to the window that paints it.** The window's own
//! transitions — a keystroke in the patch field, a route or root switch, a close
//! and reopen — drop what they are painting, correctly, for a patch that is
//! gone; none of them changes a byte on disk, so none of them may be the event
//! that decides whether the developer can still get their content back. A failed
//! rollback's recorded bytes therefore move onto a per-root holder
//! ([`ApplyRecovery`]) before any transition can drop them, and the retry that
//! restores every path is what releases them.
//!
//! Settlement reaches this module through [`settle`] — the outcome read that
//! happens *before* the two-way Ok/Err split — and [`record_outcome`], the
//! failure arm's recording rule: one function per hook, exactly as it calls
//! [`crate::granular`].

use std::collections::HashMap;
use std::path::PathBuf;

use turbogit_domain::model::{DisplayName, RootId};
use turbogit_engine_api::patch::{PatchProvenance, parse_patch, read_provenance};
use turbogit_services::patch_apply::{self, ApplyOutcome};

use crate::operation::{ApplyReport, Operation};
use crate::root_caches::Affected;
use crate::state::{AppState, Dialog, PatchSource, Toast};

/// A rollback the working tree is still waiting on, per root: the recorded
/// content of the paths a failed apply could not put back.
///
/// Held here rather than only in the apply window because the thing it is a
/// record OF is the tree, not the dialog. An entry leaves only when a retry
/// restores every path it holds.
#[derive(Default)]
pub struct ApplyRecovery {
    rollback_recovery: HashMap<RootId, ApplyOutcome>,
}

impl ApplyRecovery {
    /// Take a root's unresolved rollback under the holder.
    pub fn hold(&mut self, root: RootId, outcome: ApplyOutcome) {
        self.rollback_recovery.insert(root, outcome);
    }

    /// The recovery `root` is owed, if any.
    pub fn held(&self, root: &RootId) -> Option<ApplyOutcome> {
        self.rollback_recovery.get(root).cloned()
    }

    /// Release a root's record once the tree it belongs to is whole again.
    pub fn release(&mut self, root: &RootId) {
        self.rollback_recovery.remove(root);
    }
}

/// Open the `Apply patch` window on the focused root, by the route `source`
/// names. The two routes are the reference's two verbs: `Apply patch…` reaches
/// for a file, `Apply patch from Clipboard` skips the path entirely.
///
/// The apply group of the dialog payload is reset here rather than the whole
/// [`crate::state::DialogState`]: every other dialog's fields live in the same
/// struct and survive this window opening, and the window's own state is the
/// only thing that must not survive from one visit to the next.
pub fn open(state: &mut AppState, source: PatchSource) {
    let Some(root) = state.selected_root.clone() else {
        state.show_toast(Toast::info(
            "Apply patch needs a repository: open a workspace first.",
        ));
        return;
    };
    let d = &mut state.ui.dlg;
    d.apply_patch_root = Some(root);
    d.apply_patch_source = source;
    d.apply_patch_path.clear();
    d.apply_patch_text.clear();
    d.apply_patch_loaded.clear();
    d.apply_patch_loaded_path.clear();
    d.apply_patch_patch = None;
    d.apply_patch_error = None;
    d.apply_patch_provenance = None;
    d.apply_patch_author.clear();
    d.apply_patch_message.clear();
    d.apply_patch_preflight = None;
    d.apply_patch_outcome = None;
    d.apply_patch_report = None;
    state.ui.dialog = Some(Dialog::ApplyPatch);
    // A rollback this root has not finished is not the window's to forget.
    // Closing and reopening is the developer coming back for the recovery.
    reseed(state);
}

/// Point the window's apply at `root`. Exposed because the window is per root
/// and a multi-root workspace has more than one it could mean; leaving the
/// choice at whatever was focused when the palette item ran would be the
/// per-root admission rule reduced to a guess.
pub fn set_root(state: &mut AppState, root: &RootId) {
    if state.ui.dlg.apply_patch_root.as_ref() == Some(root) {
        return;
    }
    hold(state);
    state.ui.dlg.apply_patch_root = Some(root.clone());
    // The verdict the old root was measured against says nothing about this
    // one, and leaving it painted would be a preflight for a tree that is not
    // the tree about to be written.
    state.ui.dlg.apply_patch_preflight = None;
    state.ui.dlg.apply_patch_outcome = None;
    // The rollback is the exception to that rule: the old root keeps its
    // record here rather than in the window, and this root's own unfinished
    // rollback is exactly what pointing at this root should now show.
    reseed(state);
}

/// Switch the window's patch route (file ↔ clipboard).
///
/// The payload the other route read is dropped with it — a patch read from a
/// file is not the text the clipboard holds, and carrying one into the other
/// route would apply the wrong payload under a label that says otherwise. A
/// rollback the tree is still waiting on is not payload, so it is held here
/// and painted again by the same rule every other transition uses.
pub fn set_source(state: &mut AppState, source: PatchSource) {
    hold(state);
    let d = &mut state.ui.dlg;
    d.apply_patch_source = source;
    d.apply_patch_text.clear();
    d.apply_patch_loaded.clear();
    d.apply_patch_loaded_path.clear();
    d.apply_patch_patch = None;
    d.apply_patch_preflight = None;
    d.apply_patch_error = None;
    d.apply_patch_outcome = None;
    reseed(state);
}

/// Take the window's unresolved rollback into the per-root holder. Runs before
/// any transition that drops what the window paints, so the recorded content a
/// retry writes cannot be lost along with the frame that showed it.
fn hold(state: &mut AppState) {
    let Some(outcome) = state
        .ui
        .dlg
        .apply_patch_outcome
        .clone()
        .filter(|outcome| matches!(outcome, ApplyOutcome::RollbackFailed { .. }))
    else {
        return;
    };
    let Some(root) = state.ui.dlg.apply_patch_root.clone() else {
        return;
    };
    state.apply_recovery.hold(root, outcome);
}

/// The recovery the window's current root is owed, if any.
fn held(state: &AppState) -> Option<ApplyOutcome> {
    let root = state.ui.dlg.apply_patch_root.as_ref()?;
    state.apply_recovery.held(root)
}

/// Put the held recovery back on the window after a transition cleared it. Only
/// a rollback that never finished is re-painted: `Applied` and `Aborted` are
/// answers about the patch, and the patch is what the transition dropped.
fn reseed(state: &mut AppState) {
    if let Some(held) = held(state) {
        state.ui.dlg.apply_patch_outcome = Some(held);
    }
}

/// Run the file route's picker: the seam [`AppState::patch_reader`] installs,
/// whose absence is "this surface has no native dialog here" rather than an
/// error — the path field is the same choice made by typing.
pub fn choose_file(state: &mut AppState) {
    let Some(reader) = state.patch_reader.take() else {
        state.show_toast(Toast::info("Type the patch file's path instead."));
        return;
    };
    let chosen = reader();
    state.patch_reader = Some(reader);
    let Some(path) = chosen else {
        return;
    };
    state.ui.dlg.apply_patch_path = path.to_string_lossy().into_owned();
}

/// Bring the window's parsed state in line with what the developer has since
/// typed, chosen or pasted. Runs once per frame.
///
/// Two memos make that cheap and correct: the file is re-read only when the
/// path field has changed, and the patch is re-parsed only when the text has. A
/// third thing happens on any reparse — the preflight verdict and the last
/// outcome are dropped, because both were answers about a patch that is gone. A
/// rollback the tree never finished is the one exception, and [`reseed`] is what
/// puts it back on the window.
pub fn sync(state: &mut AppState) {
    // Held before anything below can drop it: a keystroke in the patch field
    // changes no byte of the working tree, so it cannot be what leaves the
    // developer holding a half-applied file with no recorded content to put
    // back into it.
    hold(state);
    if state.ui.dlg.apply_patch_source == PatchSource::File {
        let path = state.ui.dlg.apply_patch_path.trim().to_string();
        if !path.is_empty() && path != state.ui.dlg.apply_patch_loaded_path {
            state.ui.dlg.apply_patch_loaded_path = path.clone();
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    state.ui.dlg.apply_patch_text = text;
                    state.ui.dlg.apply_patch_error = None;
                }
                Err(e) => {
                    // An unreadable file is stated, not silently treated as no
                    // patch: the developer has to be able to tell a typo in a
                    // path from a patch that holds nothing.
                    state.ui.dlg.apply_patch_text.clear();
                    state.ui.dlg.apply_patch_loaded.clear();
                    state.ui.dlg.apply_patch_patch = None;
                    state.ui.dlg.apply_patch_provenance = None;
                    state.ui.dlg.apply_patch_preflight = None;
                    state.ui.dlg.apply_patch_error =
                        Some(format!("`{path}` could not be read — {e}"));
                    return;
                }
            }
        }
    }
    if state.ui.dlg.apply_patch_text == state.ui.dlg.apply_patch_loaded {
        return;
    }
    state.ui.dlg.apply_patch_loaded = state.ui.dlg.apply_patch_text.clone();
    state.ui.dlg.apply_patch_preflight = None;
    state.ui.dlg.apply_patch_outcome = None;
    let text = state.ui.dlg.apply_patch_text.clone();
    let patch = parse_patch(&text);
    let provenance = read_provenance(&text);
    state.ui.dlg.apply_patch_error = if patch.is_empty() && !text.trim().is_empty() {
        // Empty text is a window nobody has filled yet; text that holds no
        // `diff --git` section is a file that is not a patch.
        Some("this text holds no diff section — nothing to apply".to_string())
    } else {
        None
    };
    state.ui.dlg.apply_patch_patch = (!patch.is_empty()).then_some(patch);
    seed_fields(state, &provenance);
    state.ui.dlg.apply_patch_provenance = provenance;
    reseed(state);
}

/// Fill the destination's author and message fields from the patch's own
/// header — the reference's behaviour for a git-format patch, and the reason
/// the provenance is read at all rather than dropped with the `From:` lines.
///
/// Only an **untouched** field is re-seeded: a field still holding what the
/// previous patch put there is the patch's, and one holding anything else is
/// the developer's typing over it. The distinction is made by comparing to the
/// provenance actually seeded last, not by a "dirty" flag, so re-opening the
/// same patch cannot clobber an edit and a new patch cannot be ignored.
fn seed_fields(state: &mut AppState, next: &Option<PatchProvenance>) {
    let field = |p: Option<&PatchProvenance>, pick: fn(&PatchProvenance) -> Option<String>| {
        p.and_then(pick).unwrap_or_default()
    };
    let last = state.ui.dlg.apply_patch_provenance.as_ref();
    let author = field(next.as_ref(), |p| p.author.clone());
    let subject = field(next.as_ref(), |p| p.subject.clone());
    if state.ui.dlg.apply_patch_author == field(last, |p| p.author.clone()) {
        state.ui.dlg.apply_patch_author = author;
    }
    if state.ui.dlg.apply_patch_message == field(last, |p| p.subject.clone()) {
        state.ui.dlg.apply_patch_message = subject;
    }
}

/// `Show Diff` as a preflight, not a confirmation: ask git whether the patch
/// applies and paint the answer. Nothing is written, nothing is staged, and no
/// `Operation` is dispatched — the whole of the verb's value is that a
/// developer can use it as many times as they like and still be looking at the
/// tree they started with.
pub fn preflight(state: &mut AppState) {
    sync(state);
    let (Some(root), Some(patch)) = (
        state.ui.dlg.apply_patch_root.clone(),
        state.ui.dlg.apply_patch_patch.clone(),
    ) else {
        return;
    };
    state.ui.dlg.apply_patch_preflight = Some(patch_apply::preflight(
        state.executor.as_ref(),
        root.as_path(),
        &patch,
    ));
}

/// Dispatch the apply. The outcome slot is armed first, exactly as a push
/// records its request first: under an inline pump the completion event is
/// queued inside this call, and settlement must find a handle to read.
pub fn dispatch(state: &mut AppState) {
    sync(state);
    let (Some(root), Some(patch)) = (
        state.ui.dlg.apply_patch_root.clone(),
        state.ui.dlg.apply_patch_patch.clone(),
    ) else {
        return;
    };
    let report = ApplyReport::armed();
    state.ui.dlg.apply_patch_report = Some(report.clone());
    state.ui.dlg.apply_patch_outcome = None;
    state.dispatch(Operation::ApplyPatch {
        root,
        patch,
        report,
    });
}

/// Move a completed apply's outcome out of its slot and onto the window, so
/// the titled state paints from the record rather than from the toast's prose.
pub(crate) fn settle(state: &mut AppState) {
    let outcome = state
        .ui
        .dlg
        .apply_patch_report
        .as_ref()
        .and_then(|report| report.take());
    state.ui.dlg.apply_patch_report = None;
    state.ui.dlg.apply_patch_outcome = outcome.or(state.ui.dlg.apply_patch_outcome.take());
    // The window's painted outcome is the only place a failed rollback's
    // recorded bytes exist, so settling is where they move somewhere that
    // outlives the window's own transitions.
    hold(state);
}

/// The durable record of an apply's two titled outcomes: `Apply patch
/// aborted` with its guarantee, and `Rollback failed` with every path that
/// could not be restored, the reason for that one write and the action the
/// window offers beside it.
///
/// Recorded as its own entry rather than left to the generic failure line
/// because the generic one carries git's apply error, and git's error does
/// not know what the rollback then did — neither that every file came back,
/// nor which files are still wrong. The feed is where the developer looks
/// after the notification has gone, and [`ApplyOutcome::title`] names these
/// strings once so the window and this entry cannot drift.
pub(crate) fn record_outcome(state: &mut AppState) {
    let entry: Option<(crate::activity::ActivityKind, String)> =
        match state.ui.dlg.apply_patch_outcome.as_ref() {
            Some(
                outcome @ (ApplyOutcome::Aborted { .. } | ApplyOutcome::RollbackFailed { .. }),
            ) => {
                let kind = if outcome.tree_is_restored() {
                    crate::activity::ActivityKind::Warning
                } else {
                    crate::activity::ActivityKind::Error
                };
                let message = match outcome {
                    ApplyOutcome::Aborted { reason, .. } => {
                        format!(
                            "{} — {} · Why: {reason}",
                            outcome.title(),
                            outcome.message()
                        )
                    }
                    ApplyOutcome::RollbackFailed { unrestored, .. } => {
                        let stated: Vec<String> = unrestored.iter().map(|u| u.stated()).collect();
                        format!(
                            "{} · {} file(s) still hold what the apply wrote: {}. The \
                         content they held before is recorded and the window's Restore \
                         retry writes it.",
                            outcome.title(),
                            unrestored.len(),
                            stated.join("; ")
                        )
                    }
                    _ => return,
                };
                Some((kind, message))
            }
            _ => None,
        };
    let Some((kind, message)) = entry else {
        return;
    };
    let repo = state.ui.dlg.apply_patch_root.as_ref().map(|r| {
        let path = r.as_path();
        path.display_name_or(&path.display().to_string())
    });
    state.ui.activity.push(crate::activity::ActivityEntry {
        at: chrono::Local::now(),
        repo,
        message,
        kind,
    });
}

/// The `Rollback failed` state's one recovery action: write the recorded
/// content again, for exactly the paths that failed, and re-state the outcome
/// from what the second attempt answers.
///
/// Not a git call and not a fresh apply — no `Operation`, no dispatch, on the
/// same reasoning as the create-patch write, which is a file write and so has
/// never gone through the engine. The bytes are already in the outcome this
/// action reads, and the tree is already the one the apply left.
///
/// A retry that succeeds moves the window to `Apply patch aborted`, which is
/// then true: the patch did not land and every file changed by it is back. The
/// state does not vanish on a retry — it is replaced by the state the tree is
/// actually in.
pub fn retry_restore(state: &mut AppState) {
    let (Some(root), Some(outcome)) = (
        state.ui.dlg.apply_patch_root.clone(),
        state.ui.dlg.apply_patch_outcome.clone(),
    ) else {
        return;
    };
    let ApplyOutcome::RollbackFailed {
        reason,
        unrestored,
        restored,
    } = outcome
    else {
        return;
    };
    let still = patch_apply::retry_restore(root.as_path(), &unrestored, &mut patch_apply::Worktree);
    let restored_now: Vec<PathBuf> = unrestored
        .iter()
        .filter(|u| !still.iter().any(|s| s.path == u.path))
        .map(|u| u.path.clone())
        .chain(restored.clone())
        .collect();
    let restored_everything = still.is_empty();
    state.ui.dlg.apply_patch_outcome = Some(if still.is_empty() {
        ApplyOutcome::Aborted {
            reason,
            restored: restored_now,
        }
    } else {
        ApplyOutcome::RollbackFailed {
            reason,
            unrestored: still,
            restored,
        }
    });
    // The holder follows the tree, not the window: a retry that restored
    // every path releases the root's record, and one refused again keeps the
    // second refusal's reasons in place of the first one's.
    if restored_everything {
        state.apply_recovery.release(&root);
    } else {
        hold(state);
    }
    state.refresh(Affected::Root(root));
    if state
        .ui
        .dlg
        .apply_patch_outcome
        .as_ref()
        .is_some_and(|o| o.tree_is_restored())
    {
        state.show_toast(Toast::success(
            "Rollback complete · every file the apply changed is back to the content it held.",
        ));
    } else {
        record_outcome(state);
    }
}
