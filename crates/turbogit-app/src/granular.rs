//! The granular staging protocol (spec R2 stories 3/8/9) as one deep module.
//!
//! Callers pass pure intent — a file path plus a [`HunkTarget`] and a
//! direction — and [`dispatch`] resolves every remaining input itself: the
//! selected root, the change status, the cached raw diff text (ADR-0013),
//! untracked routing, the op label, and the [`Affected`] scope. Missing
//! inputs make the op a silent no-op; both call sites (the diff viewer's
//! gutter controls and the command palette's Stage/Unstage Hunk verbs)
//! depend on that. The module also owns completion settlement ([`settle`])
//! and the selection-lifetime rules ([`on_diff_changed`],
//! [`toggle_line_selection`], [`prune_on_refresh`]); the maps themselves stay
//! physically in [`crate::state::UiState`].

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::root_caches::Affected;
use crate::state::{AppState, DiffComparison};
use turbogit_domain::model::ChangeStatus;
use turbogit_services::partial::{self, HunkSelection, Selection};

// --- diff-cache addressing ---------------------------------------------------

/// Build a cache key that uniquely identifies this diff request.
pub fn diff_key(
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    ignore_whitespace: bool,
    path: &Option<std::path::PathBuf>,
) -> String {
    format!("{root:?}|{left:?}|{right:?}|staged={staged}|ws={ignore_whitespace}|{path:?}")
}

/// Effective comparison triple for a diff target: the revision chips only
/// apply to working-tree comparisons (left/right both unset, spec §8.4);
/// explicit commit-to-commit targets pass through untouched. Shared by the
/// viewer and [`dispatch`] so both address the same cache entry.
pub fn comparison_triple(
    left: &Option<String>,
    right: &Option<String>,
    comparison: DiffComparison,
) -> (Option<String>, Option<String>, bool) {
    if left.is_none() && right.is_none() {
        match comparison {
            DiffComparison::Repo => (Some("HEAD".to_owned()), None, false),
            DiffComparison::Staged => (None, None, true),
            DiffComparison::Local => (None, None, false),
        }
    } else {
        (left.clone(), right.clone(), false)
    }
}

/// Raw unified-diff text the viewer currently renders for `path` (the
/// commit window's preview target), or None when nothing is cached. Granular
/// ops compose their patches from exactly these bytes (ADR-0013).
fn cached_preview_diff(state: &AppState, path: &std::path::Path) -> Option<String> {
    let root = state.selected_path()?;
    let (eff_left, eff_right, staged) = comparison_triple(&None, &None, state.ui.diff_comparison);
    let key = diff_key(
        &root,
        &eff_left,
        &eff_right,
        staged,
        state.ui.diff_ignore_whitespace,
        &Some(path.to_path_buf()),
    );
    state
        .ui
        .diff_cache
        .as_ref()
        .filter(|(k, _)| k == &key)
        .map(|(_, t)| t.clone())
}

// --- dispatch ----------------------------------------------------------------

/// What part of the diff a granular op addresses: one whole hunk, or an
/// accumulated sub-hunk line selection (story 3).
pub enum HunkTarget {
    Whole(usize),
    Lines(usize, BTreeSet<usize>),
}

/// Dispatch one granular stage/unstage op (spec R2): resolve the diff text,
/// change status, untracked routing, op label, and [`Affected`] scope here,
/// then apply the composed patch through the async op seam. Missing inputs →
/// silent no-op (the palette verbs' contract).
pub fn dispatch(state: &mut AppState, path: PathBuf, target: HunkTarget, stage: bool) {
    let Some(root) = state.selected_path() else {
        return;
    };
    let Some(diff_text) = cached_preview_diff(state, &path) else {
        return;
    };
    let status = change_status(state, &path);
    let selection = selection_for(&target);
    let label = if stage { "Stage hunk" } else { "Unstage hunk" };
    // Only staging reroutes for untracked files (intent-to-add + forward
    // apply using the repo-relative path — the only form git accepts there);
    // unstage keeps the plain reverse-apply so both paths stay predictable.
    let untracked = stage && status == ChangeStatus::Unversioned;
    // Post-op the viewer settles on the remaining unstaged changes (story 8);
    // called right before `run_git`, so no-op paths never move the mode.
    settle_preview_on_unstaged(state);
    // Story 9: remember which file the op targeted so completion can decide
    // exclusions/focus with fresh status.
    state.ui.pending_granular = Some(path.clone());
    state.run_git(
        label.to_owned(),
        Affected::from_optional_root(Some(root.as_path())),
        move |v| {
            if untracked {
                partial::stage_untracked_selection(
                    v,
                    &root,
                    std::slice::from_ref(&path),
                    &diff_text,
                    &selection,
                    status,
                )
            } else if stage {
                partial::stage_selection(v, &root, &diff_text, &selection, status)
            } else {
                partial::unstage_selection(v, &root, &diff_text, &selection, status)
            }
        },
    );
}

fn selection_for(target: &HunkTarget) -> Selection {
    match target {
        HunkTarget::Whole(hunk) => Selection {
            hunks: [(*hunk, HunkSelection::Whole)].into_iter().collect(),
        },
        HunkTarget::Lines(hunk, lines) => Selection {
            hunks: [(*hunk, HunkSelection::Lines(lines.clone()))]
                .into_iter()
                .collect(),
        },
    }
}

/// The path's [`ChangeStatus`] via the canonical resolver; unlisted paths
/// fall back to [`ChangeStatus::Modified`] — controls stay enabled and the
/// engine seam remains the final authority (same rule as the viewer's
/// resolution).
fn change_status(state: &AppState, path: &Path) -> ChangeStatus {
    state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .and_then(|root| root.resolve_change(path))
        .map(|c| c.status)
        .unwrap_or(ChangeStatus::Modified)
}

/// IntelliJ-style post-op preview focus (spec R2 story 8): after a granular
/// stage/unstage the viewer lands on the remaining UNSTAGED changes — the
/// Local (index↔worktree) comparison.
fn settle_preview_on_unstaged(state: &mut AppState) {
    state.ui.diff_comparison = DiffComparison::Local;
}

// --- completion settlement (spec R2 story 9) ----------------------------------

/// Story 9 completion: called by `drain_events` AFTER `refresh` — the pending
/// granular op's file just got its post-op status, and only the refreshed
/// snapshot can say whether anything unstaged remains. When nothing unstaged
/// remains, the file leaves the changelist: its exclusion is stored under the
/// absolute bucket-key form the bucket builders compare (root join), whatever
/// form `path` arrived in. The follow-up preview is computed BEFORE inserting
/// the exclusion — the search already skips `key` itself via its
/// `just_finished` comparisons, so the ordering is equivalent and `key` can
/// then move. If the finished file was being previewed, focus advances to the
/// next changed file in display order (Default → Unversioned → Conflicts,
/// skipping exclusions) or clears.
pub(crate) fn settle(state: &mut AppState) {
    let Some(path) = state.ui.pending_granular.take() else {
        return;
    };
    let Some(key) = granular_key(state, &path) else {
        return;
    };
    if !is_fully_staged(state, &path) {
        return;
    }
    let next_preview = next_preview_candidate(state, &key);
    state.ui.granularly_completed.insert(key);
    if state.ui.preview_change.as_ref() == Some(&path) {
        state.ui.preview_change = next_preview;
    }
}

/// A granular op failed: nothing settled, so the pending marker must not
/// survive and misattribute the next op's completion.
pub(crate) fn on_op_failed(state: &mut AppState) {
    state.ui.pending_granular = None;
}

/// The absolute bucket-key form of `path` (repo-relative or absolute),
/// when it currently appears in the selected root's changes.
fn granular_key(state: &AppState, path: &Path) -> Option<PathBuf> {
    let root = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))?;
    root.resolve_change(path).map(|c| root.canonical_key(c))
}

/// Whether `path` (repo-relative or absolute, matching the viewer's
/// resolution) currently has staged content and no unstaged counterpart.
fn is_fully_staged(state: &AppState, path: &Path) -> bool {
    let Some(root) = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
    else {
        return false;
    };
    root.resolve_change(path)
        .is_some_and(|c| c.staged && !c.unstaged)
}

/// First changed file of the selected root that is still listed — same
/// bucket order as the Commit window (Default → Unversioned → Conflicts)
/// — skipping excluded paths and `just_finished`.
fn next_preview_candidate(state: &AppState, just_finished: &Path) -> Option<PathBuf> {
    let root = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))?;
    let rank = |s: ChangeStatus| match s {
        ChangeStatus::Conflicted => 2,
        ChangeStatus::Unversioned => 1,
        _ => 0,
    };
    for target in 0..=2u8 {
        if let Some(c) = root
            .status
            .changes
            .iter()
            .filter(|c| c.status != ChangeStatus::Ignored)
            .find(|c| {
                rank(c.status) == target
                    && !state
                        .ui
                        .granularly_completed
                        .contains(&root.canonical_key(c))
                    && c.path != just_finished
                    && root.canonical_key(c) != just_finished
            })
        {
            return Some(c.path.clone());
        }
    }
    None
}

// --- selection lifetimes (spec R2 stories 3/8) ---------------------------------

/// Selection-lifetime rule: the diff cache changed, so the per-diff
/// selections describing the outgoing content die with it — the accumulated
/// sub-hunk line selections refer to content no longer shown. The current
/// hunk itself is the viewer's own fresh-load reset (`ensure_diff`), which
/// calls this on the same path.
pub fn on_diff_changed(state: &mut AppState, path: Option<&Path>) {
    if let Some(p) = path {
        state.ui.line_selections.remove(p);
    }
}

/// Toggle one changed line's membership in the accumulated sub-hunk
/// selection (story 3). Empty sets are pruned so a fully deselected hunk
/// falls back to whole-hunk semantics.
pub fn toggle_line_selection(
    state: &mut AppState,
    path: &Option<std::path::PathBuf>,
    hunk: usize,
    ord: usize,
) {
    let Some(p) = path else {
        return;
    };
    let hunks = state.ui.line_selections.entry(p.clone()).or_default();
    let lines = hunks.entry(hunk).or_default();
    if !lines.insert(ord) {
        lines.remove(&ord);
    }
    if lines.is_empty() {
        hunks.remove(&hunk);
    }
}

/// Selection-lifetime rule (refresh scope): exclusions only hold while the
/// path is still fully staged; a new unstaged edit (or unstage, or deletion)
/// puts it back in the list.
pub(crate) fn prune_on_refresh(state: &mut AppState) {
    let still_fully_staged: HashSet<PathBuf> = state
        .ui
        .granularly_completed
        .iter()
        .filter(|p| is_fully_staged(state, p))
        .cloned()
        .collect();
    state.ui.granularly_completed = still_fully_staged;
}
