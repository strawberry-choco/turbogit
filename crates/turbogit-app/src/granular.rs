//! The granular staging protocol (spec R2 stories 3/8/9) as one deep module.
//!
//! Callers pass pure intent — a [`ChangeRef`] plus a [`HunkTarget`] — and
//! [`dispatch`] resolves every remaining input itself: the repository that owns
//! the change, its status, the appliable patch the operation is composed from,
//! untracked routing, the op label, and the [`Affected`] scope. Missing inputs
//! make the op a silent no-op; both call sites (the diff viewer's gutter controls
//! and the command palette's Stage/Unstage Hunk verbs) depend on that. The module
//! also owns completion settlement ([`settle`]), the char-range drag draft, and
//! the refresh-scoped lifetimes ([`prune_on_refresh`],
//! [`prune_selection_on_refresh`]); the draft itself stays physically in
//! [`crate::state::UiState`].
//!
//! The root travels with the change rather than being read off the focused root,
//! because the changes list spans every registered repository: a click on a
//! non-focused root's row would otherwise stage in the focused one.
//!
//! # What an operation is composed from, and what the user read
//!
//! Not the same value, and deliberately so. The pane renders `HEAD ↔ worktree`,
//! which shows a file's staged and unstaged lines side by side and is
//! byte-identical before and after a click, because staging cannot change what
//! the worktree differs from HEAD by. That stability is what makes a click
//! immediate instead of armed.
//!
//! A `git diff HEAD` patch is never appliable to the index, though: its context
//! describes the worktree, and `git apply --cached` matches context against the
//! **index**, so a subset cut out of the displayed patch reaches git as
//! `error: patch failed`. That is why ADR-0013's surviving rule, restated by
//! ADR-0022, is read here as *the value the operation is composed from* rather
//! than the value the pane drew. So every operation composes from one of the two
//! index-facing sections [`crate::staged_lines`] retains per file, chosen by
//! direction:
//!
//! - a **stage** forward-applies the `index ↔ worktree` section, whose pre-image
//!   *is* the index;
//! - an **unstage** reverse-applies the `HEAD ↔ index` section, whose post-image
//!   *is* the index.
//!
//! [`crate::staged_lines::line_source`] turns the row the reader clicked, which
//! carries a HEAD or a worktree line number, into the `(hunk, ordinal)` of the
//! section the direction names. Nothing outside that module pairs a line of one
//! patch with a line of another.
//!
//! # What the smallest accepted selection turned out to be
//!
//! A located ordinal is an address, and which address git takes depends on the
//! direction the composed patch applies. Measured against real git over nine hunk
//! shapes (adjacent distinct additions, adjacent deletions, one replacement pair,
//! a pair plus a separate addition, a pair plus a trailing addition run at EOF,
//! two identical adjacent additions, a deletion pair plus a pair, two pairs in one
//! hunk, additions at EOF), handed to `git apply --cached` in the direction a
//! click would use it.
//!
//! - **Forward, out of the `local` section: every single ordinal is accepted.**
//!   `filter_lines` keeps context, re-emits an unselected `-` as context, and
//!   drops an unselected `+`, so it never removes a line from the pre-image git
//!   matches the index against. Any subset of ordinals is therefore appliable and
//!   a stage keeps row granularity: two identical adjacent lines stage one at a
//!   time.
//! - **Reverse, out of the `staged` section: the clicked line's change unit.**
//!   This direction needs its own `filter_lines` branch, and applying the forward
//!   one costs all of the granularity: it refuses every single ordinal across
//!   the nine shapes measured here, so the only selection that ever lands is
//!   `HunkSelection::Whole` for the located hunk. That is the filter, not git.
//!   `git apply --cached --reverse` matches the patch's new side against the
//!   index, so a staged change the user did not select must be described truly:
//!   it stays in the patch as a context line carrying its **post-image** text (the
//!   reverse branch of `filter_lines`), not dropped and not demoted to HEAD's old
//!   text. With that branch, single changes reverse cleanly. What git will not
//!   take is half of a replacement pair, because a lone `+` reverse-deletes a line
//!   instead of restoring its old text and a lone `-` leaves both texts in the
//!   index. So an unstage composes the clicked line's replacement pair if it has
//!   one (see [`staged_lines::unstage_ords`]) and nothing wider: a same-kind run
//!   like two identical adjacent additions reverse-applies one line at a time, so
//!   it stays per-row, and the widening never reaches into a neighbouring change.
//!
//! The rule widens in one direction and must not widen in the other. Widening a
//! stage would cost the per-row control git never restricted; a stage is one
//! ordinal. An unstage widens only to the change unit git requires, and keeps
//! every other change in the hunk individually reversible.
//!
//! # Cold inputs are a decision, not an accident
//!
//! Composition reads [`crate::root_caches::RootCaches::hunk_stats`], a peek at
//! what a root's cold fill already cost: no entry, no operation. A granular verb
//! therefore never spawns git to discover what it would have composed. That is
//! the same contract the palette verbs had while they leaned on the display
//! cache, now with a named source, and
//! `granular_dispatch_without_cached_diff_is_a_silent_no_op` pins it.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::diff_model::{Row, RowKind};
use crate::keyed_read::DiffTarget;
use crate::operation::Operation;
use crate::root_caches::{Affected, StatsView};
use crate::staged_lines::{self, Displayed, Ordinal, RowSource, Side, Source, Toggle};
use crate::state::{AppState, CharDrag};
use turbogit_domain::model::{Change, ChangeRef, ChangeStatus, Patch, RootId};
use turbogit_services::partial::{self, HunkSelection, Selection};

// --- dispatch ----------------------------------------------------------------

/// Which way a whole-hunk verb moves the index.
///
/// A row click names no direction, and that is the ticket's whole change: the
/// direction is the oracle's answer for that row, so [`HunkTarget::Row`] carries
/// none and a caller cannot ask a staged row to be staged again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Stage,
    Unstage,
}

impl Direction {
    /// Which appliable section this direction composes from.
    fn source(self) -> Source {
        match self {
            Direction::Stage => Source::Local,
            Direction::Unstage => Source::Staged,
        }
    }

    fn stages(self) -> bool {
        matches!(self, Direction::Stage)
    }
}

/// What part of the diff a granular op addresses.
///
/// `Whole` is addressed the way the pane numbered its hunks; `Row` and `Chars`
/// are addressed by the numbers a row carries, because those are the only part of
/// a row that survives translation into another patch.
pub enum HunkTarget {
    /// One displayed hunk, whole, moved in the direction the button names.
    Whole(usize, Direction),
    /// One displayed row: its kind, its old gutter, its new gutter. Toggled.
    Row(RowKind, usize, usize),
    /// A character range inside one displayed addition row:
    /// `(new gutter, start, end)`. Always a stage.
    Chars(usize, usize, usize),
}

/// One composed operation waiting to be dispatched.
struct Planned {
    patch: Patch,
    selection: Selection,
    label: &'static str,
    /// Forward or reverse apply.
    stage: bool,
    /// Intent-to-add first, which is the only form git accepts for a path the
    /// index does not hold yet.
    untracked: bool,
}

/// Resolve a target into the operations it means, or into nothing at all.
///
/// Every `None` on this path is the silent no-op a cold cache or an
/// unanswerable row is supposed to produce, not an error: a conflicted, untracked,
/// binary, rename-section or context row simply has no operation to make.
fn plan(state: &AppState, change: &ChangeRef, target: &HunkTarget) -> Vec<Planned> {
    let (status, keyed) = match change_of(state, &change.root, &change.path) {
        Some(found) => (found.status, found.path.clone()),
        // An unlisted path keeps the fallback the status resolver gives the
        // controls, and its own spelling for the caches to miss on.
        None => (ChangeStatus::Modified, change.path.clone()),
    };
    if status == ChangeStatus::Unversioned && target_is_a_stage(target) {
        return plan_untracked(state, change, target);
    }
    let Some(stats) = state.caches.hunk_stats(&change.root) else {
        return Vec::new();
    };
    // The retained sections are keyed by git's `b/` side, repo-relative with
    // forward slashes, while a ref's path can be either spelling: the changes list
    // hands the repo-relative form and an injected preview hands an absolute one.
    // Asking the caches with the absolute spelling would answer `PathAbsent` and
    // turn every click in that pane into a silent no-op, so the resolution above is
    // what the lookup is keyed on, not `change.path`.
    let path = keyed.as_path();
    let Some(file) = staged_lines::find(&stats.line_sets, path) else {
        return Vec::new();
    };
    let sets = stats.line_sets.as_slice();
    match target {
        HunkTarget::Row(kind, old_no, new_no) => {
            let staged = match staged_lines::line_is_staged(sets, path, *kind, *old_no, *new_no) {
                Some(staged) => staged,
                None => return Vec::new(),
            };
            let toggle = if staged {
                Toggle::Unstage
            } else {
                Toggle::Stage
            };
            let located = staged_lines::line_source(
                sets,
                path,
                *kind,
                *old_no,
                *new_no,
                Displayed::HeadVsWorktree,
                toggle,
            );
            let RowSource::Located { source, hunk, ord } = located else {
                return Vec::new();
            };
            // The selection each direction composes. A stage takes the single
            // located ordinal: git forward-applies any subset of the `local`
            // section, so a stage keeps row granularity. An unstage takes the
            // clicked line's replacement pair (a lone `-` or `+` where it has no
            // partner), and the reverse filter then keeps every *other* staged
            // change of the located hunk as post-image context, so the neighbours
            // stay staged. One shape still widens to the whole hunk: a pure
            // creation section, whose partial reverse git refuses, and an
            // ordinal the located section cannot name is no operation at all.
            // See the module doc and [`staged_lines::unstage_ords`] for what git
            // measures.
            let selection = match toggle {
                Toggle::Stage => select(hunk, HunkSelection::Lines(BTreeSet::from([ord]))),
                Toggle::Unstage if staged_lines::section_hunk_is_creation(file, source, hunk) => {
                    select(hunk, HunkSelection::Whole)
                }
                Toggle::Unstage => {
                    let Some(ords) = staged_lines::unstage_ords(file, source, hunk, ord) else {
                        return Vec::new();
                    };
                    select(hunk, HunkSelection::Lines(ords))
                }
            };
            one(
                file,
                source,
                selection,
                toggle == Toggle::Stage,
                if toggle == Toggle::Stage {
                    "Stage line"
                } else {
                    "Unstage line"
                },
            )
        }
        HunkTarget::Chars(new_no, start, end) => {
            // A character range is a stage of part of one addition. A row whose
            // text the index already holds has nothing left to stage, and the
            // lookup declines it instead of the caller guessing.
            let located = staged_lines::line_source(
                sets,
                path,
                RowKind::Add,
                0,
                *new_no,
                Displayed::HeadVsWorktree,
                Toggle::Stage,
            );
            let RowSource::Located { source, hunk, ord } = located else {
                return Vec::new();
            };
            one(
                file,
                source,
                select(
                    hunk,
                    HunkSelection::Chars {
                        ord,
                        start: *start,
                        end: *end,
                    },
                ),
                true,
                // A character range is not a line, and the op list is where a
                // user reads what happened.
                "Stage characters",
            )
        }
        HunkTarget::Whole(hunk, dir) => {
            // The hunk the reader pointed at is the pane's, and a `+` or `−` on it
            // means every row of it that this direction can move. Those rows live
            // in one appliable section each, and `covered_hunks` is the one place
            // that says which section hunks the displayed range reaches.
            let Some(anchor) = stats
                .file(StatsView::Repo, path)
                .and_then(|f| f.hunks.get(*hunk))
                .copied()
            else {
                return Vec::new();
            };
            let covered = staged_lines::covered_hunks(sets, path, dir.source(), &anchor);
            if covered.is_empty() {
                return Vec::new();
            }
            one(
                file,
                dir.source(),
                Selection {
                    hunks: covered
                        .into_iter()
                        .map(|index| (index, HunkSelection::Whole))
                        .collect(),
                },
                dir.stages(),
                if dir.stages() {
                    "Stage hunk"
                } else {
                    "Unstage hunk"
                },
            )
        }
    }
}

fn target_is_a_stage(target: &HunkTarget) -> bool {
    match target {
        HunkTarget::Whole(_, dir) => dir.stages(),
        HunkTarget::Row(..) | HunkTarget::Chars(..) => true,
    }
}

/// The one case that composes from the **displayed** value.
///
/// Not an exception to the invariant but an instance of it: an untracked path is
/// in neither index-facing patch, so the pane's synthesized creation diff is at
/// once the value the reader selected from and the only value that can be applied
/// to that path. Its rows are all `+`, so the ordinal is read out of the same
/// section by [`locate_line`], and staging it goes through intent-to-add exactly
/// as it did before the preview was pinned.
fn plan_untracked(state: &AppState, change: &ChangeRef, target: &HunkTarget) -> Vec<Planned> {
    let Some(root) = state.multi.by_id(&change.root) else {
        return Vec::new();
    };
    let Some(patch) = state
        .peek(DiffTarget::working_tree(
            root.id.as_path().to_path_buf(),
            state.ui.diff_policies,
            Some(change.path.clone()),
        ))
        .map(|value| value.patch.clone())
    else {
        return Vec::new();
    };
    let Some(section) = patch.files.first() else {
        return Vec::new();
    };
    let selection = match target {
        HunkTarget::Whole(hunk, _) => select(*hunk, HunkSelection::Whole),
        HunkTarget::Row(RowKind::Add, _, new_no) => {
            match staged_lines::locate_line(section, Side::Post, *new_no) {
                Ordinal::At { hunk, ord } => {
                    select(hunk, HunkSelection::Lines(BTreeSet::from([ord])))
                }
                _ => return Vec::new(),
            }
        }
        HunkTarget::Chars(new_no, start, end) => {
            match staged_lines::locate_line(section, Side::Post, *new_no) {
                Ordinal::At { hunk, ord } => select(
                    hunk,
                    HunkSelection::Chars {
                        ord,
                        start: *start,
                        end: *end,
                    },
                ),
                _ => return Vec::new(),
            }
        }
        // A deletion row of a file the index does not hold removes nothing.
        HunkTarget::Row(..) => return Vec::new(),
    };
    vec![Planned {
        patch,
        selection,
        label: "Stage line",
        stage: true,
        untracked: true,
    }]
}

/// Assemble one operation from a retained section, or nothing when that patch has
/// no section for this path.
fn one(
    file: &staged_lines::FileLineSets,
    source: Source,
    selection: Selection,
    stage: bool,
    label: &'static str,
) -> Vec<Planned> {
    let patch = staged_lines::section_as_patch(file, source);
    match patch {
        Some(patch) => vec![Planned {
            patch,
            selection,
            label,
            stage,
            untracked: false,
        }],
        None => Vec::new(),
    }
}

fn select(hunk: usize, body: HunkSelection) -> Selection {
    Selection {
        hunks: [(hunk, body)].into_iter().collect(),
    }
}

/// Dispatch every operation one target resolves into (spec R2).
///
/// A target resolves to one today. `Whole` could resolve to more if one button
/// meant both directions, and the shape is a list for that reason; the
/// alternative, widening `GitExecutor::apply_patch_to_index` to take a list of
/// patches, is refused by ADR-0022's one writer and one patch shape. Cost,
/// measured by ticket 01 and not assumed: one `git apply --cached` round trip is
/// 14.5 ms median over 30 reps, six line operations 87.1 ms against 12.8 ms
/// batched (6.8x), subprocess-spawn dominated and so linear in operations,
/// rising to 95.7 ms for six with a `git status` churner holding the index lock.
/// `retry_on_transient_lock` absorbs the collision; a second operation in one
/// click would cost one more 14.5 ms, which is what any two clicks already pay.
///
/// Missing inputs make the verb a silent no-op, which is the palette contract.
pub fn dispatch(state: &mut AppState, change: ChangeRef, target: HunkTarget) {
    let ops = plan(state, &change, &target);
    if ops.is_empty() {
        return;
    }
    let status = status_of(state, &change.root, &change.path);
    // Story 9: remember which change the op targeted so completion can decide
    // exclusions/focus with fresh status.
    state.ui.pending_granular = Some(change.clone());
    for op in ops {
        let Planned {
            patch,
            selection,
            label,
            stage,
            untracked,
        } = op;
        let root = change.root.clone();
        let path = change.path.clone();
        state.dispatch(Operation::custom(
            label,
            Affected::Root(root.clone()),
            move |v| {
                let root = root.as_path();
                // Only staging reroutes for untracked files (intent-to-add, then
                // a forward apply using the repo-relative path, which is the only
                // form git accepts there); unstage keeps the plain reverse-apply
                // so both paths stay predictable.
                if untracked {
                    partial::stage_untracked_selection(
                        v,
                        root,
                        std::slice::from_ref(&path),
                        &patch,
                        &selection,
                        status,
                    )
                } else if stage {
                    partial::stage_selection(v, root, &patch, &selection, status)
                } else {
                    partial::unstage_selection(v, root, &patch, &selection, status)
                }
            },
        ));
    }
}

/// The previewed change's [`ChangeStatus`] via the canonical resolver; an
/// unlisted or absent change falls back to [`ChangeStatus::Modified`] — controls
/// stay enabled and the engine seam remains the final authority.
pub fn preview_status(state: &AppState, change: Option<&ChangeRef>) -> ChangeStatus {
    let Some(change) = change else {
        return ChangeStatus::Modified;
    };
    status_of(state, &change.root, &change.path)
}

/// One named root's path's [`ChangeStatus`] via the canonical resolver; an
/// unlisted path falls back to [`ChangeStatus::Modified`] (same rule as the
/// viewer's resolution).
pub fn status_of(state: &AppState, root: &RootId, path: &Path) -> ChangeStatus {
    change_of(state, root, path)
        .map(|change| change.status)
        .unwrap_or(ChangeStatus::Modified)
}

/// The change entry `path` names inside `root`, in either of its spellings.
///
/// The caller may hand an absolute path (an injected preview) or a repo-relative
/// one (a row click), and only one of those is what the caches are keyed on.
fn change_of<'a>(state: &'a AppState, root: &RootId, path: &Path) -> Option<&'a Change> {
    state.multi.by_id(root)?.resolve_change(path)
}

// --- completion settlement (spec R2 story 9) ----------------------------------

/// Story 9 completion: called by `drain_events` AFTER `refresh`, so the op's file
/// has its post-op status and only the refreshed snapshot can say whether the
/// file is still one the reader has work left in.
///
/// "Leaves the changelist" is a **file-list** question, not a preview question:
/// the combined preview never empties, because a row click cannot change
/// `git diff HEAD`, so nothing about the pane tells the app the file is finished.
/// [`is_fully_staged`] asks git's status instead, which is what the flat list's
/// buckets are keyed on, and the exclusion is stored as the change's own
/// identity — root plus the repo-relative spelling the entry carries, whatever
/// spelling the click arrived in. The follow-up preview is computed BEFORE
/// inserting the exclusion — the search already skips the finished change via its
/// `just_finished` comparison, so the ordering is equivalent and the exclusion can
/// then move. If the finished file was being previewed, focus advances to the next
/// changed file of that root in display order (Default → Unversioned → Conflicts,
/// skipping exclusions) or clears.
pub(crate) fn settle(state: &mut AppState) {
    let Some(pending) = state.ui.pending_granular.take() else {
        return;
    };
    let Some(change) = change_of(state, &pending.root, &pending.path) else {
        return;
    };
    let change = ChangeRef::new(pending.root.clone(), change.path.clone());
    if !is_fully_staged(state, &change) {
        return;
    }
    let next_preview = next_preview_candidate(state, &change);
    state.ui.granularly_completed.insert(change.clone());
    if state.ui.preview_change.as_ref() == Some(&change) {
        state.ui.preview_change = next_preview;
    }
}

/// A granular op failed: nothing settled, so the pending marker must not
/// survive and misattribute the next op's completion.
pub(crate) fn on_op_failed(state: &mut AppState) {
    state.ui.pending_granular = None;
}

/// Whether the change, resolved through the root that owns it, currently has
/// staged content and no unstaged counterpart.
fn is_fully_staged(state: &AppState, change: &ChangeRef) -> bool {
    change_of(state, &change.root, &change.path).is_some_and(|c| c.staged && !c.unstaged)
}

/// First changed file of the finished change's root that is still listed — same
/// bucket order as the Commit window (Default → Unversioned → Conflicts)
/// — skipping exclusions and `just_finished`.
fn next_preview_candidate(state: &AppState, just_finished: &ChangeRef) -> Option<ChangeRef> {
    let root = state.multi.by_id(&just_finished.root)?;
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
                    && c.path != just_finished.path
                    && !state
                        .ui
                        .granularly_completed
                        .contains(&ChangeRef::new(root.id.clone(), c.path.clone()))
            })
        {
            return Some(ChangeRef::new(root.id.clone(), c.path.clone()));
        }
    }
    None
}

// --- char-range drag protocol (issue 19) --------------------------------------

/// Toggle one displayed row (spec R2 story 3): a row click's whole meaning.
///
/// The row is addressed by its kind and its two gutters, which is what
/// [`crate::staged_lines`] translates into whichever appliable section its
/// staged-ness names. A silent no-op without a previewed change, or for a row the
/// oracle cannot answer.
pub fn toggle_row(state: &mut AppState, change: &Option<ChangeRef>, row: &Row) {
    let Some(p) = change.clone() else {
        return;
    };
    dispatch(state, p, HunkTarget::Row(row.kind, row.old_no, row.new_no));
}

/// Arm a fresh char-range drag on one changed line (UI computes the anchor
/// char index from the pointer). The draft starts collapsed at the anchor; a
/// draft still collapsed at release is a click, not a range.
///
/// The draft keys itself on the pane's `(hunk, ord)` because those identify the
/// row the pointer is on, and carries the row's worktree gutter because that is
/// the number the operation is addressed by.
pub fn begin_char_drag(
    state: &mut AppState,
    change: &Option<ChangeRef>,
    hunk: usize,
    ord: usize,
    new_no: usize,
    anchor: usize,
) {
    let Some(p) = change else {
        return;
    };
    state.ui.char_drag = Some(CharDrag {
        change: p.clone(),
        hunk,
        ord,
        new_no,
        anchor,
        cursor: anchor,
    });
}

/// Move the draft's cursor to the pointer's current char index; the range
/// normalizes around the anchor when read, so dragging either way widens it.
/// Drags that left the anchor row are ignored (the UI only forwards in-row
/// positions).
pub fn extend_char_drag(
    state: &mut AppState,
    change: &Option<ChangeRef>,
    hunk: usize,
    ord: usize,
    cur: usize,
) {
    if let Some(draft) = state.ui.char_drag.as_mut()
        && Some(&draft.change) == change.as_ref()
        && draft.hunk == hunk
        && draft.ord == ord
    {
        draft.cursor = cur;
    }
}

/// Close the drag. A range that moved stages through the granular pipeline; a
/// collapsed one was a plain click, which the row's own toggle handles, and the
/// draft dies either way.
///
/// The return value is the whole point: one `Sense::click_and_drag` response
/// reports a finished drag and a click on the same frame, and both routes stage,
/// so the caller must know which one ran or a char drag also stages its line.
pub fn finish_char_drag(
    state: &mut AppState,
    change: &Option<ChangeRef>,
    hunk: usize,
    ord: usize,
) -> bool {
    let Some(draft) = state
        .ui
        .char_drag
        .clone()
        .filter(|d| Some(&d.change) == change.as_ref() && d.hunk == hunk && d.ord == ord)
    else {
        return false;
    };
    state.ui.char_drag = None;
    let (start, end) = draft.range();
    if start >= end {
        return false;
    }
    dispatch(
        state,
        draft.change,
        HunkTarget::Chars(draft.new_no, start, end),
    );
    true
}

/// Selection-lifetime rule (refresh scope): exclusions only hold while the
/// change is still fully staged; a new unstaged edit (or unstage, or deletion)
/// puts it back in the list.
pub(crate) fn prune_on_refresh(state: &mut AppState) {
    let still_fully_staged: HashSet<ChangeRef> = state
        .ui
        .granularly_completed
        .iter()
        .filter(|change| is_fully_staged(state, change))
        .cloned()
        .collect();
    state.ui.granularly_completed = still_fully_staged;
}

/// Selection-lifetime rule (refresh scope): `ui.selected` means "the files I
/// am looking at", so a path the rebuilt list no longer holds is not one
/// anymore.
///
/// Called *after* the snapshots are re-registered, which is the only point
/// where "no longer holds" is true — pruning against the outgoing scan would
/// leave every path a commit had just removed sitting in the set for one more
/// refresh.
pub(crate) fn prune_selection_on_refresh(state: &mut AppState) {
    let live: HashSet<PathBuf> = state
        .multi
        .roots
        .iter()
        .flat_map(|r| r.status.changes.iter().map(|c| r.canonical_key(c)))
        .collect();
    state.ui.selected.retain(|key| live.contains(key));
}
