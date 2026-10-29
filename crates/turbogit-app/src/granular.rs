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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Stage,
    Unstage,
}

impl Direction {
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

pub enum HunkTarget {
    Whole(usize, Direction),
    Row(RowKind, usize, usize),
    /// A character range inside one displayed addition row:
    /// `(new gutter, start, end)`. Always a stage.
    Chars(usize, usize, usize),
}

struct Planned {
    patch: Patch,
    selection: Selection,
    label: &'static str,
    stage: bool,
    untracked: bool,
}

fn plan(state: &AppState, change: &ChangeRef, target: &HunkTarget) -> Vec<Planned> {
    let (status, keyed) = match change_of(state, &change.root, &change.path) {
        Some(found) => (found.status, found.path.clone()),
        None => (ChangeStatus::Modified, change.path.clone()),
    };
    if status == ChangeStatus::Unversioned && target_is_a_stage(target) {
        return plan_untracked(state, change, target);
    }
    let Some(stats) = state.caches.hunk_stats(&change.root) else {
        return Vec::new();
    };
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
                "Stage characters",
            )
        }
        HunkTarget::Whole(hunk, dir) => {
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

/// Missing inputs make the verb a silent no-op, which is the palette contract.
pub fn dispatch(state: &mut AppState, change: ChangeRef, target: HunkTarget) {
    let ops = plan(state, &change, &target);
    if ops.is_empty() {
        return;
    }
    let status = status_of(state, &change.root, &change.path);
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

pub fn preview_status(state: &AppState, change: Option<&ChangeRef>) -> ChangeStatus {
    let Some(change) = change else {
        return ChangeStatus::Modified;
    };
    status_of(state, &change.root, &change.path)
}

pub fn status_of(state: &AppState, root: &RootId, path: &Path) -> ChangeStatus {
    change_of(state, root, path)
        .map(|change| change.status)
        .unwrap_or(ChangeStatus::Modified)
}

fn change_of<'a>(state: &'a AppState, root: &RootId, path: &Path) -> Option<&'a Change> {
    state.multi.by_id(root)?.resolve_change(path)
}

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

pub(crate) fn on_op_failed(state: &mut AppState) {
    state.ui.pending_granular = None;
}

fn is_fully_staged(state: &AppState, change: &ChangeRef) -> bool {
    change_of(state, &change.root, &change.path).is_some_and(|c| c.staged && !c.unstaged)
}

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

pub fn toggle_row(state: &mut AppState, change: &Option<ChangeRef>, row: &Row) {
    let Some(p) = change.clone() else {
        return;
    };
    dispatch(state, p, HunkTarget::Row(row.kind, row.old_no, row.new_no));
}

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

/// Called *after* the snapshots are re-registered, which is the only point
/// where "no longer holds" is true, pruning against the outgoing scan would
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
