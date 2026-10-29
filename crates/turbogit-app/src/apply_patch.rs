use std::collections::HashMap;
use std::path::PathBuf;

use turbogit_domain::model::{DisplayName, RootId};
use turbogit_engine_api::patch::{PatchProvenance, parse_patch, read_provenance};
use turbogit_services::patch_apply::{self, ApplyOutcome};

use crate::operation::{ApplyReport, Operation};
use crate::root_caches::Affected;
use crate::state::{AppState, Dialog, PatchSource, Toast};

#[derive(Default)]
pub struct ApplyRecovery {
    rollback_recovery: HashMap<RootId, ApplyOutcome>,
}

impl ApplyRecovery {
    pub fn hold(&mut self, root: RootId, outcome: ApplyOutcome) {
        self.rollback_recovery.insert(root, outcome);
    }

    pub fn held(&self, root: &RootId) -> Option<ApplyOutcome> {
        self.rollback_recovery.get(root).cloned()
    }

    pub fn release(&mut self, root: &RootId) {
        self.rollback_recovery.remove(root);
    }
}

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
    reseed(state);
}

pub fn set_root(state: &mut AppState, root: &RootId) {
    if state.ui.dlg.apply_patch_root.as_ref() == Some(root) {
        return;
    }
    hold(state);
    state.ui.dlg.apply_patch_root = Some(root.clone());
    state.ui.dlg.apply_patch_preflight = None;
    state.ui.dlg.apply_patch_outcome = None;
    reseed(state);
}

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

fn held(state: &AppState) -> Option<ApplyOutcome> {
    let root = state.ui.dlg.apply_patch_root.as_ref()?;
    state.apply_recovery.held(root)
}

fn reseed(state: &mut AppState) {
    if let Some(held) = held(state) {
        state.ui.dlg.apply_patch_outcome = Some(held);
    }
}

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

pub fn sync(state: &mut AppState) {
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
        Some("this text holds no diff section — nothing to apply".to_string())
    } else {
        None
    };
    state.ui.dlg.apply_patch_patch = (!patch.is_empty()).then_some(patch);
    seed_fields(state, &provenance);
    state.ui.dlg.apply_patch_provenance = provenance;
    reseed(state);
}

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

pub(crate) fn settle(state: &mut AppState) {
    let outcome = state
        .ui
        .dlg
        .apply_patch_report
        .as_ref()
        .and_then(|report| report.take());
    state.ui.dlg.apply_patch_report = None;
    state.ui.dlg.apply_patch_outcome = outcome.or(state.ui.dlg.apply_patch_outcome.take());
    hold(state);
}

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
