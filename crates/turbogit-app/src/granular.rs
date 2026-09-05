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
use crate::state::{AppState, CharSelection, DiffComparison, Granularity};
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

/// What part of the diff a granular op addresses: the whole file, one whole
/// hunk, an accumulated sub-hunk line selection (story 3), or one line's
/// character range (issue 19) — `(hunk, ord, start, end)` with the
/// [`turbogit_services::partial::HunkSelection::Chars`] semantics.
pub enum HunkTarget {
    File,
    Whole(usize),
    Lines(usize, BTreeSet<usize>),
    Chars(usize, usize, usize, usize),
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
    let selection = selection_for(&target, &diff_text);
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

fn selection_for(target: &HunkTarget, diff_text: &str) -> Selection {
    match target {
        HunkTarget::File => file_selection(diff_text),
        HunkTarget::Whole(hunk) => Selection {
            hunks: [(*hunk, HunkSelection::Whole)].into_iter().collect(),
        },
        HunkTarget::Lines(hunk, lines) => Selection {
            hunks: [(*hunk, HunkSelection::Lines(lines.clone()))]
                .into_iter()
                .collect(),
        },
        HunkTarget::Chars(hunk, ord, start, end) => Selection {
            hunks: [(
                *hunk,
                HunkSelection::Chars {
                    ord: *ord,
                    start: *start,
                    end: *end,
                },
            )]
            .into_iter()
            .collect(),
        },
    }
}

/// Every hunk of `diff_text`, whole (File granularity): the selection that
/// stages the file's entire cached diff through the same patch pipeline as
/// every other granular op.
fn file_selection(diff_text: &str) -> Selection {
    Selection {
        hunks: (0..diff_text.lines().filter(|l| l.starts_with("@@")).count())
            .map(|h| (h, HunkSelection::Whole))
            .collect(),
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
        if state
            .ui
            .char_selection
            .as_ref()
            .is_some_and(|s| s.path == p)
        {
            state.ui.char_selection = None;
        }
    }
}

/// Switch the staging granularity (issue 19): accumulated line and char
/// selections refer to the outgoing semantics' content, so both die with
/// the switch — the same lifetime rule as a diff-cache change.
pub fn set_granularity(state: &mut AppState, granularity: Granularity) {
    state.ui.diff_granularity = granularity;
    state.ui.line_selections.clear();
    state.ui.char_selection = None;
    state.ui.char_drag_anchor = None;
}

/// Esc (issue 19): the drag-selected character range clears without staging,
/// along with any in-flight drag draft.
pub fn clear_char_selection(state: &mut AppState) {
    state.ui.char_selection = None;
    state.ui.char_drag_anchor = None;
}

/// Enter (issue 19): stage the active char selection through the granular
/// pipeline. A silent no-op without one. The selection is consumed before
/// dispatch — the diff it referred to is about to be replaced by the post-op
/// reload.
pub fn stage_char_selection(state: &mut AppState) {
    let Some(sel) = state.ui.char_selection.take() else {
        return;
    };
    dispatch(
        state,
        sel.path,
        HunkTarget::Chars(sel.hunk, sel.ord, sel.start, sel.end),
        true,
    );
}

// --- char-range drag protocol (issue 19) --------------------------------------

/// Arm a fresh char-range drag on one changed line (UI computes the anchor
/// char index from the pointer). The selection starts collapsed at the
/// anchor; a collapsed selection at drag end is a click, not a range.
pub fn begin_char_selection(
    state: &mut AppState,
    path: &Option<PathBuf>,
    hunk: usize,
    ord: usize,
    anchor: usize,
) {
    let Some(p) = path else {
        return;
    };
    state.ui.char_drag_anchor = Some((p.clone(), hunk, ord, anchor));
    state.ui.char_selection = Some(CharSelection {
        path: p.clone(),
        hunk,
        ord,
        start: anchor,
        end: anchor,
    });
}

/// Extend the active drag with the pointer's current char index: the range
/// normalizes around the anchor so dragging either way widens it. Drag
/// events that left the anchor row are ignored (the UI only forwards
/// in-row positions).
pub fn update_char_selection(
    state: &mut AppState,
    path: &Option<PathBuf>,
    hunk: usize,
    ord: usize,
    cur: usize,
) {
    let Some(p) = path else {
        return;
    };
    let Some(anchor) = state
        .ui
        .char_drag_anchor
        .clone()
        .filter(|a| a.0 == *p && a.1 == hunk && a.2 == ord)
        .map(|a| a.3)
    else {
        return;
    };
    let sel = CharSelection {
        path: p.clone(),
        hunk,
        ord,
        start: anchor.min(cur),
        end: anchor.max(cur),
    };
    state.ui.char_selection = Some(sel);
}

/// Close the drag: a collapsed range was a plain click (the line toggle
/// handles that), so it clears; a real range stays armed for Enter. The
/// anchor draft dies either way.
pub fn end_char_selection(state: &mut AppState, path: &Option<PathBuf>, hunk: usize, ord: usize) {
    let Some(p) = path else {
        return;
    };
    if !matches!(state.ui.char_drag_anchor, Some((ref a, ah, ao, _)) if *a == *p && ah == hunk && ao == ord)
    {
        return;
    }
    state.ui.char_drag_anchor = None;
    if state
        .ui
        .char_selection
        .as_ref()
        .is_some_and(|s| s.start >= s.end)
    {
        state.ui.char_selection = None;
    }
}

/// The selection readout (issue 19): how many lines and chars are selected
/// and at what granularity, plus the Enter/Esc hints while a char range is
/// armed. None while nothing is selected.
pub fn selection_readout(state: &AppState) -> Option<String> {
    let path = state.ui.preview_change.as_ref()?;
    let mut lines = state
        .ui
        .line_selections
        .get(path)
        .map_or(0, |hunks| hunks.values().map(|s| s.len()).sum());
    let mut chars = None;
    if let Some(sel) = &state.ui.char_selection
        && &sel.path == path
    {
        lines += 1;
        chars = Some(sel.end.saturating_sub(sel.start));
    }
    if lines == 0 {
        return None;
    }
    let granularity = match state.ui.diff_granularity {
        Granularity::File => "file",
        Granularity::Hunk => "hunk",
        Granularity::Line => "line",
    };
    let mut out = if lines == 1 {
        "1 line selection".to_owned()
    } else {
        format!("{lines} line selections")
    };
    if let Some(c) = chars {
        out.push_str(&format!(" · {c} chars"));
    }
    out.push_str(&format!(" · granularity: {granularity}"));
    if chars.is_some() {
        out.push_str(" · ↵ stage · Esc clear");
    }
    Some(out)
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
