//! The diff viewer. A read-only comparison is nsh-base's `DiffView`, which
//! owns row virtualization, line selection, hunk navigation and gap
//! expansion. A comparison that accepts staging (the working tree against
//! HEAD, from the Commit tab's preview) uses the N0-proven fallback: a
//! virtual list of rows with a canvas gutter, a per-line checkbox, the hunk
//! verbs, the staged-line oracle from `turbogit-app`, and the reveal bars.
//!
//! The oracle is never re-derived here: every staged answer and every hunk
//! ordinal comes from `turbogit_app::staged_lines` / `granular`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nsh_base::{DiffLayout, DiffViewApi, diff_view, theme};
use nsh_core::prelude::*;
use nsh_core::{TouchEvent, TouchPhase};
use turbogit_app::diff_model::{
    DiffValue, DisplayRow, FileMeta, Hidden, PaneKind, RowKind, line_counts, pane_kind,
};
use turbogit_app::granular::{self, Direction, HunkTarget};
use turbogit_app::keyed_read::{ContextTarget, DiffTarget, Keyed, Read};
use turbogit_app::reveals::{Ask, Edge};
use turbogit_app::staged_lines::{self, FileLineSets};
use turbogit_app::state::AppState;
use turbogit_domain::model::{ChangeRef, ChangeStatus, DiffHighlighting, DiffWhitespace};

use crate::bridge::AppHandle;
use crate::screens::image_diff;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogAction, DialogProps, dialog};
use crate::ui::icon::{Icon, icon};
use crate::ui::icon_button::{IconButtonProps, icon_button};
use crate::ui::popover::{PopoverProps, popover};

const TICK_MS: u64 = 120;
const ROW_HEIGHT: f32 = 22.0;
/// The staging rows' gutter: the staging checkbox, the line number, and the
/// sign. Every row reserves all three cells, so a number sits in one column
/// whether or not its own row carries a checkbox.
const CHECK_CELL: f32 = 18.0;
const NUMBER_CELL: f32 = 40.0;
const SIGN_CELL: f32 = 14.0;
const CELL_GAP: f32 = 4.0;
/// Where a code line's text starts: the gutter's cells, the gaps between
/// them, and the gap that keeps the text off the sign. The char-drag anchor
/// and caret map a press's x to a character with it.
pub const TEXT_X: f32 = CHECK_CELL + NUMBER_CELL + SIGN_CELL + 3.0 * CELL_GAP;
/// The monospaced face's advance at `TYPE_BODY`.
pub const CHAR_WIDTH: f32 = 7.2;

/// One painted row of the staging fallback.
#[derive(Clone, Debug, PartialEq)]
enum RowData {
    Meta {
        text: String,
    },
    Hunk {
        text: String,
        hunk: usize,
        suffix: Option<&'static str>,
        changed: usize,
    },
    Context {
        text: String,
        hunk: usize,
        number: usize,
    },
    Changed {
        text: String,
        hunk: usize,
        ord: usize,
        old_no: usize,
        new_no: usize,
        staged: Option<bool>,
        add: bool,
    },
    Gap {
        gap: u32,
        file: usize,
        hidden_left: Option<u32>,
        label: String,
    },
    Revealed {
        gap: u32,
        file: usize,
        hunk: usize,
        number: usize,
        text: String,
    },
}

/// One hunk's staged-ness, accumulated over its changed rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum HunkStage {
    /// No changed row of the hunk answered the oracle.
    #[default]
    Unset,
    /// The comparison cannot answer (an unmerged path).
    Unknown,
    NotStaged,
    Part,
    Staged,
}

impl HunkStage {
    fn saw(self, staged: bool) -> Self {
        match (self, staged) {
            (Self::Unknown, _) => Self::Unknown,
            (Self::Unset, true) => Self::Staged,
            (Self::Unset, false) => Self::NotStaged,
            (Self::Staged, true) | (Self::NotStaged, false) => self,
            _ => Self::Part,
        }
    }

    fn suffix(self) -> Option<&'static str> {
        match self {
            Self::Staged => Some("· staged"),
            Self::NotStaged => Some("· not staged"),
            Self::Part => Some("· part"),
            Self::Unset | Self::Unknown => None,
        }
    }
}

/// One settled comparison as the rows and toolbar read it.
#[derive(Clone)]
struct DiffWindow(Arc<DiffData>);

struct DiffData {
    key: String,
    subject: String,
    target: DiffTarget,
    change: Option<ChangeRef>,
    pane_kind: PaneKind,
    files: Vec<FileMeta>,
    patch_text: String,
    rows: Vec<RowData>,
    hunk_rows: Vec<usize>,
    hunk_count: usize,
    stats: (usize, usize),
    waiting: bool,
    empty: bool,
    failure: Option<String>,
    staged_live: bool,
    conflicted: bool,
}

impl PartialEq for DiffWindow {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::ops::Deref for DiffWindow {
    type Target = DiffData;

    fn deref(&self) -> &DiffData {
        &self.0
    }
}

/// The line the user last pressed, keyed by comparison: hunk and ordinal in
/// the appliable patch, plus the displayed number for the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSel {
    pub hunk: usize,
    pub ord: usize,
    pub line: usize,
    pub add: bool,
}

fn short_sha(id: &str) -> String {
    id.chars().take(7).collect()
}

fn subject_of(target: &DiffTarget) -> String {
    match &target.path {
        Some(path) => format!(
            "{} @ {}",
            path.display(),
            short_sha(target.right.as_deref().unwrap_or_default())
        ),
        None => match (target.left.as_deref(), target.right.as_deref()) {
            (Some(left), Some(right)) => {
                format!("{}..{}", short_sha(left), short_sha(right))
            }
            (_, Some(right)) => short_sha(right),
            _ => "Working tree".to_owned(),
        },
    }
}

fn plan(state: &mut AppState, target: &DiffTarget, change: Option<ChangeRef>) -> DiffData {
    let key = target.key();
    if target.stages()
        && let Some(change) = change.as_ref()
    {
        state.ensure_hunk_stats(&change.root);
    }
    let verdict = state.read(target.clone());
    let mut data = DiffData {
        key,
        subject: subject_of(target),
        target: target.clone(),
        change: change.clone(),
        pane_kind: PaneKind::Text,
        files: Vec::new(),
        patch_text: String::new(),
        rows: Vec::new(),
        hunk_rows: Vec::new(),
        hunk_count: 0,
        stats: (0, 0),
        waiting: false,
        empty: false,
        failure: None,
        staged_live: target.stages(),
        conflicted: change.as_ref().is_some_and(|change| {
            granular::preview_status(state, Some(change)) == ChangeStatus::Conflicted
        }),
    };
    match verdict {
        Read::Waiting => data.waiting = true,
        Read::Empty => data.empty = true,
        Read::Failed(message) => data.failure = Some(message),
        Read::Fresh(value) => {
            data.pane_kind = pane_kind(&value.model.files);
            data.files = value.model.files.clone();
            data.hunk_count = value.model.hunk_count();
            data.stats = line_counts(&value.model);
            data.empty = value.model.display.is_empty();
            data.patch_text = value.patch.to_string();
            if data.pane_kind == PaneKind::Text {
                let rows = rows_for(state, &value, &data.key, change.as_ref());
                data.rows = rows;
                data.hunk_rows = data
                    .rows
                    .iter()
                    .enumerate()
                    .filter_map(|(at, row)| matches!(row, RowData::Hunk { .. }).then_some(at))
                    .collect();
            }
        }
    }
    data
}

fn rows_for(
    state: &AppState,
    value: &DiffValue,
    key: &str,
    change: Option<&ChangeRef>,
) -> Vec<RowData> {
    let model = &value.model;
    let oracle = change.and_then(|change| {
        let root = state.multi.by_id(&change.root)?;
        let entry = root.resolve_change(&change.path)?;
        let stats = state.caches.hunk_stats(&root.id)?;
        Some((entry.path.as_path(), stats.line_sets.as_slice()))
    });
    let sets = oracle;
    let conflicted = change.is_some_and(|change| {
        granular::preview_status(state, Some(change)) == ChangeStatus::Conflicted
    });
    let mut rows: Vec<RowData> = Vec::new();
    let mut hunk_staging: Vec<HunkStage> = vec![HunkStage::Unset; model.hunk_count()];
    let mut hunk_lines: Vec<usize> = vec![0; model.hunk_count()];
    let gaps = model.gaps();
    let gap_before: HashMap<usize, u32> = gaps
        .gaps
        .iter()
        .enumerate()
        .filter_map(|(at, gap)| gap.below.map(|hunk| (hunk, at as u32)))
        .collect();
    let push_gap = |rows: &mut Vec<RowData>, gap: u32| {
        push_gap_rows(rows, state, value, key, gap);
    };
    for disp in &model.display {
        match disp {
            DisplayRow::Full(row) => match row.kind {
                RowKind::Hunk => {
                    if let Some(gap) = gap_before.get(&row.hunk) {
                        push_gap(&mut rows, *gap);
                    }
                    rows.push(RowData::Hunk {
                        text: row.text.clone(),
                        hunk: row.hunk,
                        suffix: None,
                        changed: 0,
                    });
                }
                RowKind::Context => rows.push(RowData::Context {
                    text: row.text.clone(),
                    hunk: row.hunk,
                    number: row.new_no,
                }),
                RowKind::Meta | RowKind::RenameHeader => rows.push(RowData::Meta {
                    text: row.text.clone(),
                }),
                RowKind::Del | RowKind::Add => {
                    push_changed(
                        &mut rows,
                        &mut hunk_staging,
                        &mut hunk_lines,
                        row,
                        sets,
                        conflicted,
                    );
                }
            },
            DisplayRow::Pair(del, add) => {
                for row in [del.as_ref(), add.as_ref()].into_iter().flatten() {
                    push_changed(
                        &mut rows,
                        &mut hunk_staging,
                        &mut hunk_lines,
                        row,
                        sets,
                        conflicted,
                    );
                }
            }
        }
    }
    for (at, gap) in gaps.gaps.iter().enumerate() {
        if gap.below.is_none() {
            push_gap(&mut rows, at as u32);
        }
    }
    // The headers paint before their bodies are walked, so their state is
    // settled in one pass once every row of every hunk has answered.
    for row in &mut rows {
        if let RowData::Hunk {
            hunk,
            suffix,
            changed,
            ..
        } = row
        {
            *suffix = hunk_staging
                .get(*hunk)
                .copied()
                .unwrap_or_default()
                .suffix();
            *changed = hunk_lines.get(*hunk).copied().unwrap_or(0);
        }
    }
    rows
}

fn push_changed(
    rows: &mut Vec<RowData>,
    hunk_staging: &mut [HunkStage],
    hunk_lines: &mut [usize],
    row: &turbogit_app::diff_model::Row,
    sets: Option<(&std::path::Path, &[FileLineSets])>,
    conflicted: bool,
) {
    let staged = if conflicted {
        None
    } else {
        sets.and_then(|(path, sets)| {
            staged_lines::line_is_staged(sets, path, row.kind, row.old_no, row.new_no)
        })
    };
    if let Some(entry) = hunk_staging.get_mut(row.hunk) {
        *entry = match staged {
            Some(value) => entry.saw(value),
            None => HunkStage::Unknown,
        };
    }
    if let Some(lines) = hunk_lines.get_mut(row.hunk) {
        *lines += 1;
    }
    rows.push(RowData::Changed {
        text: row.text.clone(),
        hunk: row.hunk,
        ord: row.line_ord,
        old_no: row.old_no,
        new_no: row.new_no,
        staged,
        add: row.kind == RowKind::Add,
    });
}

fn push_gap_rows(
    rows: &mut Vec<RowData>,
    state: &AppState,
    value: &DiffValue,
    key: &str,
    gap: u32,
) {
    let model = &value.model;
    let Some(info) = model.gaps().gap(gap) else {
        return;
    };
    let edges = state.ui.reveals.edges(key, gap as usize);
    let ask = state.ui.reveals.side(key, info.file);
    let total = match info.hidden {
        Hidden::Run(n) => Some(n),
        Hidden::ToEof => match &ask {
            Ask::Have(text) => Some(
                text.lines()
                    .saturating_add(1)
                    .saturating_sub(info.new_first),
            ),
            _ => None,
        },
    };
    let top = match total {
        Some(total) => edges[0].min(total),
        None => 0,
    };
    let hunk = info.below.or(info.above).unwrap_or_default();
    for off in 0..top {
        let number = info.new_first + off;
        let text = match &ask {
            Ask::Have(side) => side.line(number).unwrap_or_default().to_owned(),
            _ => String::new(),
        };
        rows.push(RowData::Revealed {
            gap,
            file: info.file,
            hunk,
            number: number as usize,
            text,
        });
    }
    let hidden_left = total.map(|total| total.saturating_sub(top));
    if hidden_left == Some(0) {
        return;
    }
    let label = match (hidden_left, &ask) {
        (Some(n), _) => counted(
            n as usize,
            "unchanged line hidden",
            "unchanged lines hidden",
        ),
        (None, Ask::Waiting) => "Reading the file…".to_owned(),
        (None, _) => "Unchanged lines hidden".to_owned(),
    };
    rows.push(RowData::Gap {
        gap,
        file: info.file,
        hidden_left,
        label,
    });
}

fn counted(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The diff route: the target the shell holds, or an empty state.
pub fn diff_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let has_target = handle.peek(|s| s.ui.diff.is_some() || s.ui.preview_change.is_some());
    let body_handle = handle;
    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .test_id(&test_ids::route("diff_view"))
        .child(if has_target {
            target_body(body_handle).into_element()
        } else {
            text("Open a diff to see its rows.")
                .text_size(px(data::TYPE_BODY))
                .text_color(t.muted_foreground)
                .p(px(data::PANEL_PADDING))
                .into_element()
        })
}

/// The shell's window target, asked as the read layer's comparison.
pub fn keyed_target(
    target: &turbogit_app::state::DiffTarget,
    policies: turbogit_domain::model::DiffPolicies,
) -> DiffTarget {
    DiffTarget::new(
        target.root.as_path().to_path_buf(),
        target.left.clone(),
        target.right.clone(),
        policies,
        target.path.clone(),
    )
}

fn target_body(handle: AppHandle) -> impl IntoElement {
    let (target, change) = handle.peek(|s| {
        if let Some(target) = s.ui.diff.clone() {
            let change = target
                .path
                .clone()
                .map(|path| ChangeRef::new(target.root.clone(), path));
            (Some(keyed_target(&target, s.ui.diff_policies)), change)
        } else if let Some(change) = s.ui.preview_change.clone() {
            (
                Some(DiffTarget::working_tree(
                    change.root.as_path().to_path_buf(),
                    s.ui.diff_policies,
                    Some(change.path.clone()),
                )),
                Some(change),
            )
        } else {
            (None, None)
        }
    });
    match target {
        Some(target) => render_target(handle, target, change).into_element(),
        None => view().into_element(),
    }
}

/// The Commit tab's preview column: the comparison the list's selection
/// names, re-read as the selection moves.
pub fn preview_pane(handle: AppHandle) -> impl IntoElement {
    let current = signal(handle.peek(preview_target));
    let tick = handle.clone();
    let tick_current = current;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let next = tick.peek(preview_target);
        if next != tick_current.get() {
            tick_current.set(next);
        }
    });
    let t = theme();
    let body_handle = handle;
    dynamic(move || match current.get() {
        Some((target, change)) => view()
            .flex_1()
            .flex_col()
            .test_id(&test_ids::route("diff_view"))
            .child(render_target(body_handle.clone(), target, change))
            .into_element(),
        None => view()
            .flex_1()
            .items_center()
            .justify_center()
            .test_id(&test_ids::diff("preview"))
            .child(
                view()
                    .flex_col()
                    .items_center()
                    .gap(px(data::SPACING_X))
                    .child(icon(Icon::File, data::ICON_XL, t.muted_foreground))
                    .child(
                        text("Select a changed file to preview its unified diff.")
                            .text_size(px(data::TYPE_BODY))
                            .text_color(t.muted_foreground),
                    ),
            )
            .into_element(),
    })
}

/// The comparison the preview column pins: the selected change against the
/// working tree.
fn preview_target(state: &AppState) -> Option<(DiffTarget, Option<ChangeRef>)> {
    let change = state.ui.preview_change.clone()?;
    Some((
        DiffTarget::working_tree(
            change.root.as_path().to_path_buf(),
            state.ui.diff_policies,
            Some(change.path.clone()),
        ),
        Some(change),
    ))
}

/// One comparison: the toolbar over `DiffView`, the staging fallback, or the
/// image and binary panes.
pub fn render_target(
    handle: AppHandle,
    target: DiffTarget,
    change: Option<ChangeRef>,
) -> impl IntoElement {
    let t = theme();
    let window = signal(DiffWindow(Arc::new({
        let probe_handle = handle.clone();
        probe_handle.update_with(|s| plan(s, &target, change.clone()))
    })));
    let tick = handle.clone();
    let tick_target = target.clone();
    let tick_change = change.clone();
    let tick_window = window;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let next = tick.update_with(|s| plan(s, &tick_target, tick_change.clone()));
        if next.key != tick_window.get().key
            || next.rows != tick_window.get().rows
            || next.stats != tick_window.get().stats
            || next.waiting != tick_window.get().waiting
            || next.empty != tick_window.get().empty
            || next.failure != tick_window.get().failure
            || next.conflicted != tick_window.get().conflicted
        {
            tick_window.set(DiffWindow(Arc::new(next)));
        }
    });

    let split = signal(false);
    let selection = signal(HashMap::<String, LineSel>::new());
    let range = signal(None::<(u32, u32)>);
    let focus = signal(HashMap::<String, usize>::new());
    let list = element_ref();
    let read_ref = component_ref::<nsh_base::DiffView>();

    let toolbar_handle = handle.clone();
    let toolbar_window = window;
    let toolbar_split = split;
    let toolbar_selection = selection;
    let toolbar_range = range;
    let toolbar_focus = focus;
    let toolbar_list = list;
    let toolbar_ref = read_ref;
    let toolbar = diff_toolbar(
        toolbar_handle,
        toolbar_window,
        toolbar_split,
        toolbar_selection,
        toolbar_range,
        toolbar_focus,
        toolbar_list,
        toolbar_ref,
    );

    let body_handle = handle.clone();
    let body_window = window;
    let body_split = split;
    let body_selection = selection;
    let body_range = range;
    let body_focus = focus;
    let body_list = list;
    let body_ref = read_ref;
    let body = dynamic(move || {
        let data = body_window.get();
        match data.pane_kind {
            PaneKind::Image | PaneKind::Binary => {
                return image_diff::diff_target_pane(
                    body_handle.clone(),
                    data.target.clone(),
                    data.pane_kind,
                )
                .into_element();
            }
            PaneKind::Text => {}
        }
        if let Some(message) = data.failure.clone() {
            return view()
                .flex_col()
                .p(px(data::TYPE_BODY))
                .child(
                    text(message)
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground),
                )
                .into_element();
        }
        if data.waiting {
            return text("Computing diff…")
                .text_size(px(data::TYPE_BODY))
                .text_color(t.muted_foreground)
                .p(px(data::PANEL_PADDING))
                .into_element();
        }
        if data.empty {
            return text("(no differences)")
                .text_size(px(data::TYPE_BODY))
                .text_color(t.muted_foreground)
                .p(px(data::PANEL_PADDING))
                .into_element();
        }
        if data.staged_live {
            staging_rows(
                body_handle.clone(),
                body_window,
                body_selection,
                body_focus,
                body_list,
            )
            .into_element()
        } else {
            read_only_diff(
                body_handle.clone(),
                body_window,
                body_split,
                body_range,
                body_ref,
            )
            .into_element()
        }
    });

    view()
        .flex_1()
        .flex_col()
        .child(selection_line(window, selection, range, focus))
        .child(toolbar)
        .child(body)
}

/// The observable selection: the fallback's pressed line, or `DiffView`'s
/// reported range.
fn selection_line(
    window: Signal<DiffWindow>,
    selection: Signal<HashMap<String, LineSel>>,
    range: Signal<Option<(u32, u32)>>,
    focus: Signal<HashMap<String, usize>>,
) -> impl IntoElement {
    let t = theme();
    let selected = move || -> Option<String> {
        let data = window.get();
        if data.staged_live {
            selection
                .get()
                .get(&data.key)
                .map(|sel| format!("selection: line {} (hunk {})", sel.line, sel.hunk + 1))
        } else {
            range
                .get()
                .map(|(start, end)| format!("selection: {start}..{end}"))
        }
    };
    let focused = move || {
        let data = window.get();
        let hunk = focus.get().get(&data.key).copied().unwrap_or(0);
        format!("hunk {}/{}", hunk + 1, data.hunk_count.max(1))
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .px(px(data::PANEL_PADDING))
        .py(px(data::SPACING_Y))
        .child(
            text(move || window.get().subject.clone())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground)
                .flex_1(),
        )
        .child(
            text(focused)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground)
                .test_id(&test_ids::diff("hunk-label")),
        )
        .child(dynamic(move || match selected() {
            Some(label) => text(label)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::ACCENT_TEXT)
                .test_id(&test_ids::diff("selection"))
                .into_element(),
            None => view().into_element(),
        }))
}

#[allow(clippy::too_many_arguments)]
fn diff_toolbar(
    handle: AppHandle,
    window: Signal<DiffWindow>,
    split: Signal<bool>,
    selection: Signal<HashMap<String, LineSel>>,
    range: Signal<Option<(u32, u32)>>,
    focus: Signal<HashMap<String, usize>>,
    list: ElementRef,
    read_ref: ComponentRef<nsh_base::DiffView>,
) -> impl IntoElement {
    let split_handle = handle.clone();
    let _ = split_handle;
    let _ = selection;
    let mode_split = split;
    let mode_handle = split;
    let prev_handle = handle.clone();
    let prev_window = window;
    let prev_focus = focus;
    let prev_list = list;
    let prev_ref = read_ref;
    let next_handle = handle.clone();
    let next_window = window;
    let next_focus = focus;
    let next_list = list;
    let next_ref = read_ref;
    let _ = range;
    let whitespace_handle = handle.clone();
    let highlight_handle = handle.clone();
    let counts = move || {
        let data = window.get();
        (data.stats.0, data.stats.1)
    };
    let total_hunks = move || window.get().hunk_count;
    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .px(px(data::PANEL_PADDING))
        .py(px(data::SPACING_Y))
        .child(segmented(
            "Side-by-Side",
            "Unified",
            mode_split,
            move || mode_handle.set(!mode_handle.get()),
            &test_ids::diff("mode"),
        ))
        .child(diff_options(whitespace_handle, highlight_handle))
        .child(view().flex_1())
        .child(
            text(move || format!("+{}", counts().0))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::DIFF_ADD_ACCENT),
        )
        .child(
            text(move || format!("−{}", counts().1))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::DIFF_DEL_ACCENT),
        )
        .child(icon_button(
            IconButtonProps::new("‹", "Previous hunk")
                .disabled(total_hunks() == 0)
                .test_id(&test_ids::diff("prev"))
                .on_press(move || {
                    step_hunk(
                        &prev_handle,
                        &prev_window,
                        prev_focus,
                        &prev_list,
                        prev_ref,
                        -1,
                    )
                }),
        ))
        .child(icon_button(
            IconButtonProps::new("›", "Next hunk")
                .disabled(total_hunks() == 0)
                .test_id(&test_ids::diff("next"))
                .on_press(move || {
                    step_hunk(
                        &next_handle,
                        &next_window,
                        next_focus,
                        &next_list,
                        next_ref,
                        1,
                    )
                }),
        ))
        .test_id(&test_ids::diff("toolbar"))
}

/// The mode switch: two press targets inside one band, the selected one
/// wearing the accent.
fn segmented(
    left_label: &'static str,
    right_label: &'static str,
    value: Signal<bool>,
    toggle: impl Fn() + 'static,
    test_id: &str,
) -> impl IntoElement {
    let t = theme();
    let accent = t.accent;
    let accent_ink = t.accent_foreground;
    let ink = t.foreground;
    let muted = t.muted;
    let toggle = Arc::new(toggle);
    let left_toggle = toggle.clone();
    let right_toggle = toggle;
    let band = |label: &'static str, index: bool, run: Arc<dyn Fn()>| {
        view()
            .role(AccessibilityRole::Button)
            .label(label)
            .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
            .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
            .rounded(px(data::CONTROL_RADIUS))
            .bg(move || {
                if value.get() == index {
                    accent
                } else {
                    AdaptiveColor::TRANSPARENT
                }
            })
            .text_size(px(data::TYPE_CONTROL))
            .on_press(move || run())
            .child(
                text(label)
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(move || {
                        if value.get() == index {
                            accent_ink
                        } else {
                            ink
                        }
                    }),
            )
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(1.0))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(muted)
        .test_id(test_id)
        .child(band(left_label, true, Arc::new(move || left_toggle())))
        .child(band(right_label, false, Arc::new(move || right_toggle())))
}

fn diff_options(whitespace_handle: AppHandle, highlight_handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let whitespace_value = signal(whitespace_handle.peek(|s| s.ui.diff_policies.whitespace));
    let highlight_value = signal(highlight_handle.peek(|s| s.ui.diff_policies.highlighting));
    let ws = whitespace_handle;
    let hl = highlight_handle;
    let mut props = PopoverProps::new(button(
        ButtonProps::new("Diff options")
            .variant(ButtonVariant::Secondary)
            .size(crate::ui::ControlSize::Compact)
            .test_id(&test_ids::diff("options")),
    ))
    .width(280.0)
    .test_id(&test_ids::diff("options-popover"));
    props = props.child(
        text("Ignore Differences")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    for value in DiffWhitespace::ALL {
        let ws = ws.clone();
        props = props.child(option_row(
            value.label(),
            whitespace_value,
            value,
            move || {
                ws.update(move |s| {
                    s.ui.diff_policies.whitespace = value;
                    s.save_durable_ui();
                });
                whitespace_value.set(value);
            },
            &test_ids::diff(format!("whitespace:{}", value.label())),
        ));
    }
    props = props.child(
        text("Highlighting Differences")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    for value in DiffHighlighting::ALL {
        let hl = hl.clone();
        props = props.child(option_row(
            value.label(),
            highlight_value,
            value,
            move || {
                hl.update(move |s| {
                    s.ui.diff_policies.highlighting = value;
                    s.save_durable_ui();
                });
                highlight_value.set(value);
            },
            &test_ids::diff(format!("highlighting:{}", value.label())),
        ));
    }
    popover(props)
}

/// One policy the popover offers. The chosen value is read live, so the fill
/// and the mark follow a press instead of the menu reopening. Both inks are
/// theme roles: a text with no color of its own paints in the renderer's
/// default, not in the surface's ink.
fn option_row<T: PartialEq + Copy + 'static>(
    label: &'static str,
    chosen: Signal<T>,
    value: T,
    on_press: impl Fn() + 'static,
    test_id: &str,
) -> AnyElement {
    let t = theme();
    let fill = t.accent;
    let selected_ink = t.accent_foreground;
    let ink = t.popover_foreground;
    let is_chosen = move || chosen.get() == value;
    view()
        .role(AccessibilityRole::Button)
        .label(label)
        .px(px(data::DENSITY_DENSE_BUTTON_PAD_X))
        .py(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(move || {
            if is_chosen() {
                fill
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .on_press(on_press)
        .test_id(test_id)
        .child(
            text(move || {
                if is_chosen() {
                    format!("✓ {label}")
                } else {
                    label.to_owned()
                }
            })
            .text_size(px(data::TYPE_CONTROL))
            .text_color(move || if is_chosen() { selected_ink } else { ink }),
        )
        .into_element()
}

/// Moves the hunk focus one step, the fallback scrolling its list and
/// `DiffView` scrolling itself.
fn step_hunk(
    handle: &AppHandle,
    window: &Signal<DiffWindow>,
    focus: Signal<HashMap<String, usize>>,
    list: &ElementRef,
    read_ref: ComponentRef<nsh_base::DiffView>,
    delta: isize,
) {
    let data = window.get();
    if data.hunk_count == 0 {
        return;
    }
    if data.staged_live {
        let key = data.key.clone();
        let current = focus.get().get(&key).copied().unwrap_or(0);
        let next = (current as isize + delta).clamp(0, data.hunk_count as isize - 1) as usize;
        focus.update(|map| {
            map.insert(key, next);
        });
        if let Some(row) = data.hunk_rows.get(next) {
            list.scroll_to_index(*row as u32, percentage(0.5), px(0.0), false);
        }
        let _ = handle;
    } else if let Some(api) = read_ref.api::<DiffViewApi>() {
        let key = data.key.clone();
        let current = focus.get().get(&key).copied().unwrap_or(0);
        let next = (current as isize + delta).clamp(0, data.hunk_count as isize - 1) as usize;
        focus.update(|map| {
            map.insert(key, next);
        });
        if delta > 0 {
            api.next_hunk();
        } else {
            api.previous_hunk();
        }
    }
}

/// The read-only body: `DiffView` over the patch.
fn read_only_diff(
    _handle: AppHandle,
    window: Signal<DiffWindow>,
    split: Signal<bool>,
    range: Signal<Option<(u32, u32)>>,
    read_ref: ComponentRef<nsh_base::DiffView>,
) -> impl IntoElement {
    let patch_window = window;
    let select_range = range;
    view().flex_1().test_id(&test_ids::diff("viewer")).child(
        diff_view()
            .patch(move || SharedString::from(patch_window.get().patch_text.clone()))
            .layout(move || {
                if split.get() {
                    DiffLayout::Split
                } else {
                    DiffLayout::Unified
                }
            })
            .on_line_selection_change(move |selected: &Option<nsh_base::DiffLineRange>| {
                select_range.set(selected.map(|r| (r.start + 1, r.end + 1)));
            })
            .component_ref(&read_ref)
            .flex_1(),
    )
}

/// The staging fallback: the virtual list of rows, each changed row carrying
/// its checkbox, the hunk verbs, the reveal bar, and the caret.
fn staging_rows(
    handle: AppHandle,
    window: Signal<DiffWindow>,
    selection: Signal<HashMap<String, LineSel>>,
    focus: Signal<HashMap<String, usize>>,
    list: ElementRef,
) -> impl IntoElement {
    let count_window = window;
    let count = move || count_window.get().rows.len();
    let row_handle = handle.clone();
    let row_window = window;
    let row_selection = selection;
    let row_focus = focus;
    // The rows rebuild from the window signal whenever it changes: the
    // keyed reconciliation duplicates the cell before a mid-list insertion
    // (a revealed gap), and a stale cell keeps a hunk's disabled gutter
    // live. The list still mounts only the rows in view.
    virtual_list(count, move |at: Position| {
        staging_row(row_handle.clone(), row_window, row_selection, row_focus, at)
    })
    .element_ref(&list)
    .estimated_item_size(px(ROW_HEIGHT))
    .flex_1()
    .test_id(&test_ids::diff("list"))
}

fn staging_row(
    handle: AppHandle,
    window: Signal<DiffWindow>,
    selection: Signal<HashMap<String, LineSel>>,
    focus: Signal<HashMap<String, usize>>,
    at: Position,
) -> AnyElement {
    // A cell reused at the same index rebinds to the comparison it now
    // holds (a hunk's staged-ness, a conflict's gates), not the frame it
    // first mounted with.
    dynamic(move || staging_row_at(handle.clone(), window, selection, focus, at)).into_element()
}

fn staging_row_at(
    handle: AppHandle,
    window: Signal<DiffWindow>,
    selection: Signal<HashMap<String, LineSel>>,
    focus: Signal<HashMap<String, usize>>,
    at: Position,
) -> AnyElement {
    let t = theme();
    let row = window.with(|data| data.rows.get(at.index()).cloned());
    let Some(row) = row else {
        return view().into_element();
    };
    let key = window.with(|data| data.key.clone());
    let change = window.with(|data| data.change.clone());
    let conflicted = window.with(|data| data.conflicted);
    match row {
        RowData::Meta { text: label } => {
            row_text(&label, t.muted_foreground, data::TYPE_CONTROL).into_element()
        }
        RowData::Hunk {
            text: label,
            hunk,
            suffix,
            changed,
        } => {
            let concat = match suffix {
                Some(suffix) => {
                    format!("{label}   {suffix}   {}", counted(changed, "line", "lines"))
                }
                None => format!("{label}   {}", counted(changed, "line", "lines")),
            };
            let text_row = row_text(&concat, t.muted_foreground, data::TYPE_CONTROL);
            let stage_handle = handle.clone();
            let stage_change = change.clone();
            let unstage_handle = handle.clone();
            let unstage_change = change.clone();
            view()
                .flex_row()
                .items_center()
                .h(px(ROW_HEIGHT))
                .child(text_row)
                .child(view().flex_1())
                .child(button(
                    ButtonProps::new("Stage hunk")
                        .variant(ButtonVariant::Ghost)
                        .size(crate::ui::ControlSize::Compact)
                        .disabled(conflicted || change.is_none())
                        .test_id(&test_ids::diff(format!("stage-hunk:{hunk}")))
                        .on_press(move || {
                            let Some(change) = stage_change.clone() else {
                                return;
                            };
                            stage_handle.update(move |s| {
                                granular::dispatch(
                                    s,
                                    change,
                                    HunkTarget::Whole(hunk, Direction::Stage),
                                );
                            });
                        }),
                ))
                .child(button(
                    ButtonProps::new("Unstage hunk")
                        .variant(ButtonVariant::Ghost)
                        .size(crate::ui::ControlSize::Compact)
                        .disabled(conflicted || change.is_none())
                        .test_id(&test_ids::diff(format!("unstage-hunk:{hunk}")))
                        .on_press(move || {
                            let Some(change) = unstage_change.clone() else {
                                return;
                            };
                            unstage_handle.update(move |s| {
                                granular::dispatch(
                                    s,
                                    change,
                                    HunkTarget::Whole(hunk, Direction::Unstage),
                                );
                            });
                        }),
                ))
                .into_element()
        }
        RowData::Gap {
            gap,
            file,
            hidden_left,
            label: caption,
        } => {
            let tap_handle = handle.clone();
            let tap_window = window;
            view()
                .flex_row()
                .items_center()
                .gap(px(data::SPACING_X))
                .h(px(ROW_HEIGHT))
                .px(px(8.0))
                .bg(t.muted)
                .test_id(&test_ids::diff(format!("gap:{gap}")))
                .role(AccessibilityRole::Button)
                .label(caption.clone())
                .on_press(move || {
                    let data = tap_window.get();
                    let target = data.target.clone();
                    let key = data.key.clone();
                    let hidden = hidden_left;
                    tap_handle.update(move |s| {
                        if !s.ui.reveals.tap(&key, gap as usize, Edge::Top, hidden) {
                            return;
                        }
                        if s.ui.reveals.admit(&key, file) {
                            s.read(ContextTarget::new(target.clone(), file));
                        }
                    });
                })
                .child(
                    text(caption)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                )
                .into_element()
        }
        RowData::Context {
            text: label,
            number,
            hunk,
        } => {
            let press_handle = handle.clone();
            let press_key = key.clone();
            let press_selection = selection;
            view()
                .flex_row()
                .items_center()
                .h(px(ROW_HEIGHT))
                .test_id(&test_ids::diff(format!("context:{number}")))
                .on_press(move || {
                    press_selection.update(|map| {
                        map.insert(
                            press_key.clone(),
                            LineSel {
                                hunk,
                                ord: 0,
                                line: number,
                                add: false,
                            },
                        );
                    });
                    let _ = &press_handle;
                })
                .child(gutter(None, number, t.muted_foreground, " "))
                .child(
                    text(label)
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.foreground)
                        .flex_1(),
                )
                .into_element()
        }
        RowData::Revealed {
            text: label,
            number,
            hunk,
            ..
        } => {
            let press_selection = selection;
            let press_key = key.clone();
            view()
                .flex_row()
                .items_center()
                .h(px(ROW_HEIGHT))
                .test_id(&test_ids::diff(format!("revealed:{number}")))
                .on_press(move || {
                    press_selection.update(|map| {
                        map.insert(
                            press_key.clone(),
                            LineSel {
                                hunk,
                                ord: 0,
                                line: number,
                                add: false,
                            },
                        );
                    });
                })
                .child(gutter(None, number, t.muted_foreground, " "))
                .child(
                    text(label)
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.foreground)
                        .flex_1(),
                )
                .into_element()
        }
        RowData::Changed {
            text: label,
            hunk,
            ord,
            old_no,
            new_no,
            staged,
            add,
        } => changed_row(
            handle, window, selection, focus, key, change, conflicted, label, hunk, ord, old_no,
            new_no, staged, add,
        ),
    }
}

/// The gutter every staging row paints before its text: the checkbox slot,
/// the line number, and the sign. One function owns the geometry, so the
/// number column cannot drift by the width of a row's checkbox.
fn gutter(
    check: Option<AnyElement>,
    number: usize,
    ink: AdaptiveColor,
    sign: &'static str,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(CELL_GAP))
        .pr(px(CELL_GAP))
        .child(
            view()
                .w(px(CHECK_CELL))
                .flex_row()
                .items_center()
                .child(check),
        )
        .child(
            view().w(px(NUMBER_CELL)).flex_row().justify_end().child(
                text(number.to_string())
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(ink),
            ),
        )
        .child(
            view()
                .w(px(SIGN_CELL))
                .flex_row()
                .items_center()
                .child(text(sign).text_size(px(data::TYPE_CONTROL)).text_color(ink)),
        )
}

fn row_text(label: &str, ink: AdaptiveColor, size: f32) -> impl IntoElement {
    text(label.to_owned())
        .flex_1()
        .text_size(px(size))
        .text_color(ink)
}

#[allow(clippy::too_many_arguments)]
fn changed_row(
    handle: AppHandle,
    _window: Signal<DiffWindow>,
    selection: Signal<HashMap<String, LineSel>>,
    _focus: Signal<HashMap<String, usize>>,
    key: String,
    change: Option<ChangeRef>,
    conflicted: bool,
    label: String,
    hunk: usize,
    ord: usize,
    old_no: usize,
    new_no: usize,
    staged: Option<bool>,
    add: bool,
) -> AnyElement {
    let t = theme();
    let accent = t.accent;
    let accent_ink = t.muted_foreground;
    let number = if add { new_no } else { old_no };
    let label_len = label.chars().count();
    let staged_ink = match (add, staged) {
        (true, Some(false)) => data::DIFF_ADD_UNSTAGED_TEXT,
        (true, _) => data::DIFF_ADD_TEXT,
        (false, Some(false)) => data::DIFF_DEL_UNSTAGED_TEXT,
        (false, _) => data::DIFF_DEL_TEXT,
    };
    let band = match (add, staged) {
        (true, Some(false)) => data::DIFF_ADD_UNSTAGED_BG,
        (true, _) => data::DIFF_ADD_BG,
        (false, Some(false)) => data::DIFF_DEL_UNSTAGED_BG,
        (false, _) => data::DIFF_DEL_BG,
    };
    let bg_selection = selection;
    let bg_key = key.clone();
    let row_label = format!(
        "{label} ({})",
        match staged {
            Some(true) => "staged",
            Some(false) => "unstaged",
            None => "staged-ness unknown",
        }
    );
    let check_handle = handle.clone();
    let check_change = change.clone();
    let check: Option<AnyElement> = if add {
        let checked = match staged {
            Some(true) => CheckboxState::Checked,
            _ => CheckboxState::Unchecked,
        };
        Some(
            checkbox(
                CheckboxProps::new(move || checked)
                    .disabled(conflicted || change.is_none())
                    .accessibility_label(format!("Stage line {number}"))
                    .test_id(&test_ids::diff(format!("check:{number}")))
                    .on_change(move |_| {
                        let Some(change) = check_change.clone() else {
                            return;
                        };
                        check_handle.update(move |s| {
                            granular::dispatch(
                                s,
                                change,
                                HunkTarget::Row(
                                    if add { RowKind::Add } else { RowKind::Del },
                                    old_no,
                                    new_no,
                                ),
                            );
                        });
                    }),
            )
            .into_element(),
        )
    } else {
        None
    };

    let press_handle = handle.clone();
    let press_change = change.clone();
    let press_key = key.clone();
    let press_sel = selection;
    let press_label = row_label.clone();

    let down_handle = handle.clone();
    let down_change = change.clone();
    let up_handle = handle.clone();
    let up_change = change.clone();

    let mut row = view()
        .flex_row()
        .items_center()
        .h(px(ROW_HEIGHT))
        .bg(move || {
            let caret = bg_selection
                .get()
                .get(&bg_key)
                .copied()
                .is_some_and(|sel| sel.line == number && sel.hunk == hunk && sel.add == add);
            if caret { accent } else { band }
        })
        .test_id(&test_ids::diff(format!(
            "{}:{}",
            if add { "add" } else { "del" },
            number
        )))
        .role(AccessibilityRole::Button)
        .label(press_label)
        .on_press(move |_: &PressEvent| {
            if add {
                press_handle.update(|s| {
                    let draft = s.ui.char_drag.take();
                    let plain_click = draft.is_none_or(|draft| {
                        let (start, end) = draft.range();
                        start >= end
                    });
                    if plain_click && let Some(change) = press_change.clone() {
                        granular::toggle_row(
                            s,
                            &Some(change),
                            &line_row(add, hunk, ord, old_no, new_no),
                        );
                    }
                });
            } else if let Some(change) = press_change.clone() {
                press_handle.update(move |s| {
                    granular::toggle_row(
                        s,
                        &Some(change.clone()),
                        &line_row(add, hunk, ord, old_no, new_no),
                    );
                });
            }
            let target = press_key.clone();
            press_sel.update(|map| {
                map.insert(
                    target,
                    LineSel {
                        hunk,
                        ord,
                        line: number,
                        add,
                    },
                );
            });
        })
        .child(gutter(
            check,
            number,
            staged_ink,
            if add { "+" } else { "−" },
        ))
        .child(
            text(label)
                .text_size(px(data::TYPE_BODY))
                .text_color(staged_ink)
                .flex_1(),
        );
    if let Some(suffix) = match staged {
        Some(true) => Some("staged"),
        Some(false) => Some("unstaged"),
        None => None,
    } {
        row = row.child(
            text(suffix)
                .text_size(px(data::TYPE_CHIP))
                .text_color(accent_ink),
        );
    }
    if add {
        row = row
            .on_press_down(move |event: &PressEvent| {
                let Some(change) = down_change.clone() else {
                    return;
                };
                let anchor = char_at(event.position.x.0).min(label_len);
                down_handle.update(move |s| {
                    granular::begin_char_drag(s, &Some(change.clone()), hunk, ord, new_no, anchor);
                });
            })
            .on_press_up(move |event: &PressEvent| {
                let Some(change) = up_change.clone() else {
                    return;
                };
                let cursor = char_at(event.position.x.0).min(label_len);
                up_handle.update(move |s| {
                    granular::extend_char_drag(s, &Some(change.clone()), hunk, ord, cursor);
                    let dragged = s.ui.char_drag.as_ref().is_some_and(|draft| {
                        let (start, end) = draft.range();
                        start < end
                    });
                    if dragged {
                        granular::finish_char_drag(s, &Some(change.clone()), hunk, ord);
                    }
                });
            })
            .on_touch({
                let touch_handle = handle.clone();
                let touch_change = change.clone();
                move |event: &TouchEvent| {
                    let Some(change) = touch_change.clone() else {
                        return;
                    };
                    let Some(point) = event.points.iter().find(|point| point.changed) else {
                        return;
                    };
                    let x = point.location.x.0;
                    let char = char_at(x).min(label_len);
                    touch_handle.update(move |s| match event.phase {
                        TouchPhase::Start => {
                            granular::begin_char_drag(
                                s,
                                &Some(change.clone()),
                                hunk,
                                ord,
                                new_no,
                                char,
                            );
                        }
                        TouchPhase::Move => {
                            granular::extend_char_drag(s, &Some(change.clone()), hunk, ord, char);
                        }
                        TouchPhase::End | TouchPhase::Cancel => {
                            granular::extend_char_drag(s, &Some(change.clone()), hunk, ord, char);
                            let dragged = s.ui.char_drag.as_ref().is_some_and(|draft| {
                                let (start, end) = draft.range();
                                start < end
                            });
                            if dragged {
                                granular::finish_char_drag(s, &Some(change.clone()), hunk, ord);
                            }
                        }
                    });
                }
            });
    }
    row.into_element()
}

fn line_row(
    add: bool,
    hunk: usize,
    ord: usize,
    old_no: usize,
    new_no: usize,
) -> turbogit_app::diff_model::Row {
    turbogit_app::diff_model::Row {
        kind: if add { RowKind::Add } else { RowKind::Del },
        text: String::new(),
        hunk,
        line_ord: ord,
        old_no,
        new_no,
        span: None,
        highlight: Default::default(),
    }
}

/// The character a press at `x` names, from the monospaced advance.
pub fn char_at(x: f32) -> usize {
    ((x - TEXT_X).max(0.0) / CHAR_WIDTH).round() as usize
}

/// The window `DiffView` opens in: the comparison the shell holds, closed by
/// the dialog's own Close verb.
pub fn diff_layer(handle: AppHandle) -> impl IntoElement {
    let show_handle = handle.clone();
    when(
        move || show_handle.read(|s| s.ui.diff.is_some()),
        move || {
            let close_handle = handle.clone();
            let body_handle = handle.clone();
            let cancel_handle = handle.clone();
            let subject = handle.peek(|s| {
                s.ui.diff
                    .as_ref()
                    .map(|target| subject_of(&keyed_target(target, s.ui.diff_policies)))
                    .unwrap_or_else(|| "Diff".to_owned())
            });
            dialog(
                DialogProps::new("diff", subject)
                    .on_cancel(move || cancel_handle.update(|s| s.ui.diff = None))
                    .with_action(DialogAction::new(
                        "Close",
                        ButtonVariant::Ghost,
                        move || close_handle.update(|s| s.ui.diff = None),
                    ))
                    .child(
                        view()
                            .w(px(920.0))
                            .h(px(560.0))
                            .flex_col()
                            .test_id(&test_ids::diff("window"))
                            .child(target_body(body_handle)),
                    ),
            )
        },
    )
}
