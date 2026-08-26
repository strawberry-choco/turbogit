//! Diff viewer (spec §8.4, issue #13).
//!
//! Restyled onto the central [`crate::theme::Palette`] tokens and the shared
//! widget vocabulary — behavior preserved, visual migration only:
//!
//! - **Async + cached** engine access through the [`GitExecutor`] seam
//!   (Epic E7/J1): diffs are computed on a worker thread and cached, so no
//!   `git diff` runs synchronously per frame.
//! - **Virtualized rendering (ADR-0014)**: rows paint through
//!   [`ScrollArea::show_rows`] over a memoized display-row model built once
//!   per diff beside `diff_cache` — parsing and side-by-side pairing never
//!   run per frame, and hunk navigation scrolls by row index so unrealized
//!   rows stay reachable.
//! - **Segmented control** toggles Side-by-Side | Unified rendering.
//! - **Revision chips** select the working-tree comparison pair:
//!   Repo = HEAD↔worktree, Staged = HEAD↔index, Local = index↔worktree.
//!   Explicit commit-to-commit targets (Git Log) keep their fixed pair and
//!   hide the chips.
//! - **Hunk navigation** ‹ n/N › steps between parsed hunks.
//! - **Ignore whitespace** feeds `DiffOpts::ignore_whitespace` into the
//!   engine call and the cache key.
//! - Add/del lines paint token-exact backgrounds (`DIFF_ADD_BG` /
//!   `DIFF_DEL_BG`) with muted `INK_3` gutter numbers; hunk headers sit on
//!   SURFACE (spec §2.3).
//! - **Gutter staging (spec R2)**: every hunk-header band carries compact
//!   "+" / "−" controls that stage / unstage that whole hunk by composing a
//!   patch from the cached raw diff (ADR-0013) and applying it through the
//!   async op seam. Conflicted files keep the controls visible but inert.

use crate::core::granular::{self, comparison_triple, diff_key};
use crate::engine::AppEvent;
use crate::model::{ChangeStatus, DiffOpts};
use crate::state::{AppState, DiffComparison};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, Response, ScrollArea,
    Sense, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

// --- metrics -----------------------------------------------------------------

/// Rendered height of one diff line.
const ROW_H: f32 = 22.0;
/// Side-by-side pane header height (spec §8.4).
const PANE_HEADER_H: f32 = 28.0;
/// Width of the +/- sign column in a unified row.
const SIGN_W: f32 = 16.0;
/// Width of the line-number gutter column.
const NUM_W: f32 = 40.0;
/// X offset of the code text within a unified row.
const TEXT_X: f32 = SIGN_W + NUM_W + 12.0;

fn mono_font() -> FontId {
    FontId::new(12.0, FontFamily::Monospace)
}

// --- row model ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Meta,
    Hunk,
    Context,
    Del,
    Add,
}

struct Row {
    kind: RowKind,
    text: String,
    /// Owning hunk index — the header's own ordinal for `Hunk` rows, and
    /// the enclosing hunk for body rows (aiming the current hunk from the
    /// pointer, spec R2). Meta rows own no hunk.
    hunk: usize,
    /// 0-based ordinal over the hunk's +/- lines in order (changed rows
    /// only) — exactly [`crate::core::partial::HunkSelection::Lines`]
    /// semantics for sub-hunk selection (spec R2 story 3).
    line_ord: usize,
    /// 1-based old-file line number (0 when not applicable).
    old_no: usize,
    /// 1-based new-file line number (0 when not applicable).
    new_no: usize,
    /// Ordinal of this row in parse order — the unified-mode paging index
    /// (ADR-0014): unified windows are ranges over these ordinals.
    ord: usize,
}

/// `@@ -a,b +c,d @@` → `(a, c)` (defaults to 1 when absent).
fn hunk_starts(header: &str) -> (usize, usize) {
    let mut old = 1usize;
    let mut new = 1usize;
    for tok in header.split_whitespace() {
        if let Some(rest) = tok.strip_prefix('-') {
            if let Some(n) = rest.split(',').next().and_then(|v| v.parse().ok()) {
                old = n;
            }
        } else if let Some(rest) = tok.strip_prefix('+')
            && let Some(n) = rest.split(',').next().and_then(|v| v.parse().ok())
        {
            new = n;
        }
    }
    (old, new)
}

/// Parse unified-diff text into renderable rows, tracking 1-based line
/// numbers from each hunk header so gutters can show real positions, and
/// per-hunk changed-line ordinals so rows can carry sub-hunk selection
/// (spec R2 story 3).
fn parse(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut hunk = 0usize;
    let mut enclosing = 0usize;
    let mut changed = 0usize;
    let mut old_no = 1usize;
    let mut new_no = 1usize;
    for line in text.lines() {
        if line.starts_with("@@") {
            let (o, n) = hunk_starts(line);
            old_no = o;
            new_no = n;
            changed = 0;
            rows.push(Row {
                kind: RowKind::Hunk,
                text: line.to_string(),
                hunk,
                line_ord: 0,
                old_no: 0,
                new_no: 0,
                ord: 0,
            });
            enclosing = hunk;
            hunk += 1;
        } else if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("---")
            || line.starts_with("+++")
            || line.starts_with("new file")
            || line.starts_with("Binary")
            || line.starts_with('\\')
        {
            rows.push(Row {
                kind: RowKind::Meta,
                text: line.to_string(),
                hunk: 0,
                line_ord: 0,
                old_no: 0,
                new_no: 0,
                ord: 0,
            });
        } else if let Some(body) = line.strip_prefix('+') {
            rows.push(Row {
                kind: RowKind::Add,
                text: body.to_string(),
                hunk: enclosing,
                line_ord: changed,
                old_no: 0,
                new_no,
                ord: 0,
            });
            changed += 1;
            new_no += 1;
        } else if let Some(body) = line.strip_prefix('-') {
            rows.push(Row {
                kind: RowKind::Del,
                text: body.to_string(),
                hunk: enclosing,
                line_ord: changed,
                old_no,
                new_no: 0,
                ord: 0,
            });
            changed += 1;
            old_no += 1;
        } else {
            let body = line.strip_prefix(' ').unwrap_or(line);
            rows.push(Row {
                kind: RowKind::Context,
                text: body.to_string(),
                hunk: enclosing,
                line_ord: 0,
                old_no,
                new_no,
                ord: 0,
            });
            old_no += 1;
            new_no += 1;
        }
    }
    for (ord, row) in rows.iter_mut().enumerate() {
        row.ord = ord;
    }
    rows
}

// --- display-row model (ADR-0014) --------------------------------------------

/// One virtualized display row: the unit [`ScrollArea::show_rows`] pages
/// over. Side-by-side mode renders each element as one [`ROW_H`] band;
/// unified mode ignores pairing and flattens every contained row back into
/// its own band — both modes read from this one cached vector.
enum DisplayRow {
    /// Full-width row: meta, hunk header, or context.
    Full(Row),
    /// A side-by-side pair sharing one [`ROW_H`] band: one Del and/or one
    /// Add (`None` fills the missing half with an empty cell).
    Pair(Option<Row>, Option<Row>),
}

/// Everything rendering needs from one diff, built once per diff text and
/// memoized beside `diff_cache` (ADR-0014): the display-row vector plus the
/// index maps hunk navigation — and future R7 keyboard nav — scroll by.
struct DiffModel {
    display: Vec<DisplayRow>,
    /// Underlying parsed-row count: the unified-mode `show_rows` total
    /// (pairing ignored, each underlying row = one paging slot).
    raw_count: usize,
    /// Underlying-row ordinal → display index, so a unified window over raw
    /// ordinals finds the (possibly mid-pair) display elements covering it.
    raw_to_display: Vec<u32>,
    /// Hunk ordinal → first display index of its header row.
    hunk_first_display: Vec<u32>,
}

impl DiffModel {
    /// Number of parsed hunks.
    fn hunk_count(&self) -> usize {
        self.hunk_first_display.len()
    }

    /// First display-row index aiming at `hunk`: the paired-model index in
    /// side-by-side mode, otherwise the underlying-row ordinal of the header
    /// (unified pages over underlying rows). This is the shared hunk→row map
    /// ADR-0014 commits to — R7 keyboard nav (`F7`/`Shift+F7`) reuses it.
    fn first_row_for_hunk(&self, hunk: usize, side_by_side: bool) -> Option<usize> {
        let disp = *self.hunk_first_display.get(hunk)? as usize;
        Some(if side_by_side {
            disp
        } else {
            match &self.display[disp] {
                DisplayRow::Full(row) => row.ord,
                // Changed rows are always paired; headers never are.
                DisplayRow::Pair(..) => disp,
            }
        })
    }
}

/// Fold a buffered run of consecutive Del/Add rows into paired display rows,
/// recording each underlying row's display index in parse order.
fn flush_changed_run(
    pending: &mut Vec<Row>,
    display: &mut Vec<DisplayRow>,
    raw_to_display: &mut Vec<u32>,
) {
    if pending.is_empty() {
        return;
    }
    let start = display.len() as u32;
    let mut dels: Vec<Row> = Vec::new();
    let mut adds: Vec<Row> = Vec::new();
    let mut slots: Vec<u32> = Vec::with_capacity(pending.len());
    for row in pending.drain(..) {
        if row.kind == RowKind::Del {
            slots.push(dels.len() as u32);
            dels.push(row);
        } else {
            slots.push(adds.len() as u32);
            adds.push(row);
        }
    }
    let pairs = dels.len().max(adds.len());
    let mut dels = dels.into_iter();
    let mut adds = adds.into_iter();
    for _ in 0..pairs {
        display.push(DisplayRow::Pair(dels.next(), adds.next()));
    }
    raw_to_display.extend(slots.iter().map(|p| start + p));
}

/// Fold parsed rows into the cached display-row model (ADR-0014):
/// consecutive Del/Add runs collapse into paired display rows while
/// meta/hunk/context stay full-width. Also records the index maps so
/// per-frame work is O(visible rows).
fn build_model(rows: Vec<Row>) -> DiffModel {
    let raw_count = rows.len();
    let mut display = Vec::new();
    let mut raw_to_display = Vec::with_capacity(raw_count);
    let mut hunk_first_display = Vec::new();
    let mut pending: Vec<Row> = Vec::new();
    for row in rows {
        match row.kind {
            RowKind::Del | RowKind::Add => pending.push(row),
            RowKind::Meta | RowKind::Context => {
                flush_changed_run(&mut pending, &mut display, &mut raw_to_display);
                raw_to_display.push(display.len() as u32);
                display.push(DisplayRow::Full(row));
            }
            RowKind::Hunk => {
                flush_changed_run(&mut pending, &mut display, &mut raw_to_display);
                hunk_first_display.push(display.len() as u32);
                raw_to_display.push(display.len() as u32);
                display.push(DisplayRow::Full(row));
            }
        }
    }
    flush_changed_run(&mut pending, &mut display, &mut raw_to_display);
    DiffModel {
        display,
        raw_count,
        raw_to_display,
        hunk_first_display,
    }
}

// Single-entry memo of the last parsed diff (plan §2.2): UI rendering runs
// on one thread, so the display model of the most recently rendered diff
// lives in a thread-local slot beside the raw-text cache instead of being
// rebuilt from scratch every frame. Keyed by the raw text itself — not the
// diff cache key — because ops invalidate and refetch the cache under an
// unchanged key with changed bytes.
thread_local! {
    static PARSED_ROWS: RefCell<Option<(String, Rc<DiffModel>)>> = const { RefCell::new(None) };
}

/// Display model for the cached diff `text`, parsed and folded into the memo
/// only when the text changed (plan §2.2, ADR-0014). Hands back an [`Rc`]
/// handle — a refcount bump, never a row-by-row copy — so nothing here holds
/// a borrow of [`AppState`] past this expression.
fn diff_model(text: &str) -> Rc<DiffModel> {
    PARSED_ROWS.with(|slot| {
        let mut memo = slot.borrow_mut();
        if !matches!(&*memo, Some((existing, _)) if existing == text) {
            *memo = Some((text.to_owned(), Rc::new(build_model(parse(text)))));
        }
        Rc::clone(&memo.as_ref().expect("memo filled above").1)
    })
}

// --- engine access -----------------------------------------------------------

/// Synthesize a creation unified-diff for an untracked preview (spec R2):
/// `git diff` cannot see untracked files, so the worktree content is dressed
/// as a whole-file addition and fed through the normal cache/parse path —
/// rows, gutters, hover tracking, and hunk nav all behave unchanged. This is
/// the exact shape `partial_stage_cli.rs` proves appliable. `None` falls back
/// to engine behavior (unreadable, binary, or empty files).
fn synthetic_untracked_diff(root: &std::path::Path, rel: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(root.join(rel)).ok()?;
    // Binary (NUL byte) or empty content has no meaningful granular diff.
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    let content = String::from_utf8(bytes).ok()?;
    // Patch headers need slash-separated repo-relative paths.
    let display = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let n = content.lines().count();
    let mut out = format!(
        "diff --git a/{display} b/{display}\n\
         new file mode 100644\n\
         --- /dev/null\n\
         +++ b/{display}\n\
         @@ -0,0 +1,{n} @@\n"
    );
    for line in content.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    if !content.ends_with('\n') {
        out.push_str("\\ No newline at end of file\n");
    }
    Some(out)
}

/// Trigger an async diff load if the cache is missing/stale and not already
/// loading. Resets hunk navigation whenever a fresh load starts so the ‹n/N›
/// counter always describes the content being displayed.
fn ensure_diff(
    state: &mut AppState,
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    ignore_whitespace: bool,
    path: &Option<std::path::PathBuf>,
) {
    let key = diff_key(root, left, right, staged, ignore_whitespace, path);
    let stale = state
        .ui
        .diff_cache
        .as_ref()
        .map(|(k, _)| k != &key)
        .unwrap_or(true);
    if stale && !state.ui.diff_loading {
        state.ui.diff_error = None;
        state.ui.diff_current_hunk = 0;
        // The sub-hunk line selections refer to the outgoing content; the
        // granular module drops them with the rest of the per-diff
        // navigation state (spec R2, story 3) — the current hunk was reset
        // to the first hunk above.
        granular::on_diff_changed(state, path.as_deref());

        // Untracked previews never reach `git diff`; synthesize a creation
        // diff from worktree content synchronously and populate the cache
        // under the same key — no worker, no loading spinner (spec R2).
        let untracked = preview_status(state, path.as_deref()) == ChangeStatus::Unversioned;
        if untracked
            && let Some(rel) = path
            && let Some(text) = synthetic_untracked_diff(root, rel)
        {
            state.ui.diff_cache = Some((key, text));
            return;
        }

        state.ui.diff_loading = true;
        let executor: Arc<dyn crate::engine::GitExecutor> = state.executor.clone();
        let tx = state.tx.clone();
        let root = root.to_path_buf();
        let opts = DiffOpts {
            staged,
            ignore_whitespace,
            left: left.clone(),
            right: right.clone(),
            path: path.clone(),
            ..DiffOpts::default()
        };
        std::thread::spawn(move || {
            let res = executor.diff(&root, &opts);
            let _ = tx.send(AppEvent::DiffReady { key, result: res });
        });
    }
}

// --- entry point -------------------------------------------------------------

pub fn render_diff(
    ui: &mut Ui,
    state: &mut AppState,
    left: &Option<String>,
    right: &Option<String>,
    path: &Option<std::path::PathBuf>,
) {
    let root = match state.selected_path() {
        Some(r) => r,
        None => {
            ui.label("No repository selected.");
            return;
        }
    };

    // Working-tree comparisons expose the revision chips (spec §8.4);
    // explicit commit-to-commit targets keep their fixed revision pair.
    let working_tree = left.is_none() && right.is_none();
    let comparison = state.ui.diff_comparison;
    let ignore_ws = state.ui.diff_ignore_whitespace;
    let (eff_left, eff_right, staged) = comparison_triple(left, right, comparison);

    ensure_diff(state, &root, &eff_left, &eff_right, staged, ignore_ws, path);
    let key = diff_key(&root, &eff_left, &eff_right, staged, ignore_ws, path);

    // Cached display model (absent while loading / before first load).
    // Borrowed, not cloned (plan §1.1): both probes below end the immutable
    // borrow of `state.ui` immediately — before any `&mut state` use below —
    // and the model itself is memoized beside the raw cache (ADR-0014).
    let cached = state
        .ui
        .diff_cache
        .as_ref()
        .filter(|(k, _)| k == &key)
        .is_some();
    let model = state
        .ui
        .diff_cache
        .as_ref()
        .filter(|(k, _)| k == &key)
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(_, t)| diff_model(t));
    let total_hunks = model.as_ref().map_or(0, |m| m.hunk_count());

    // Toolbar chrome (spec §8.4): mode · chips · hunk nav · whitespace.
    // Wrapped so narrow panes (commit preview) push the toggle to a second
    // row instead of clipping it out of reach.
    ui.horizontal_wrapped(|ui| {
        let selected = if state.ui.diff_side_by_side { 0 } else { 1 };
        if let Some(idx) = segmented_control(ui, &["Side-by-Side", "Unified"], selected) {
            state.ui.diff_side_by_side = idx == 0;
        }
        if working_tree {
            ui.separator();
            comparison_chips(ui, state);
        }
        ui.separator();
        hunk_nav(ui, state, total_hunks);
        ui.separator();
        ui.checkbox(&mut state.ui.diff_ignore_whitespace, "Ignore whitespace");
    });

    if let Some(err) = &state.ui.diff_error {
        ui.colored_label(Palette::STATE_ERROR, err);
        return;
    }

    let model = match model {
        Some(model) => model,
        None if cached => {
            ui.label("(no differences)");
            return;
        }
        None => {
            if state.ui.diff_loading {
                ui.spinner();
                ui.label("Computing diff…");
            } else {
                ui.label("(no diff)");
            }
            return;
        }
    };

    // Gutter staging controls need the previewed file's status (spec R2);
    // resolved once per frame from the cached changelists.
    let status = preview_status(state, path.as_deref());

    let side_by_side = state.ui.diff_side_by_side;
    // Paging total (ADR-0014): side-by-side walks the paired display rows,
    // unified ignores pairing and pages one slot per underlying row.
    let total_rows = if side_by_side {
        model.display.len()
    } else {
        model.raw_count
    };

    // Side-by-side pane header band (spec §8.4), pinned above the paged
    // rows: `show_rows` reserves exactly the uniform-height row window, so
    // the fixed-height band leaves the scrolled content. Floating scrollbars
    // reserve no width, so the panes below still measure the same `half`.
    if side_by_side {
        let width = ui.available_width();
        let gap = ui.style().spacing.item_spacing.x;
        let half = ((width - gap * 2.0) / 2.0).max(120.0);
        let (left_label, right_label) = pane_labels(comparison);
        ui.horizontal(|ui| {
            header_band(ui, half, &format!("Before {left_label}"));
            header_band(ui, half, &format!("After {right_label}"));
        });
    }

    // Named id salt: the commit window's `ui.columns` panes share one stable
    // child id, and egui's default ScrollArea salt is constant — an unnamed
    // area here would share persisted scrollbar state with the changelist
    // pane's area and flip-flop visibility (a zero-delay repaint loop).
    ScrollArea::vertical().id_salt("diff_viewer").show_rows(
        ui,
        ROW_H,
        total_rows,
        |ui, visible| {
            // Hunk navigation (ADR-0014): index-based — aim the aimed hunk's
            // first display row at the viewport center instead of relying on
            // a realized widget (`resp.scroll_to_me` cannot reach rows this
            // window didn't build). Issued inside the closure because egui
            // consumes scroll targets set by an area's content; ones set
            // before the area begins are stashed for outer areas. At most
            // once per (diff, hunk) — re-issuing every frame would keep the
            // ScrollArea repainting forever.
            if state.ui.diff_current_hunk > 0
                && let Some(row_idx) =
                    model.first_row_for_hunk(state.ui.diff_current_hunk, side_by_side)
                && hunk_needs_scroll(ui, &key, state.ui.diff_current_hunk)
            {
                let pitch = ROW_H + ui.spacing().item_spacing.y;
                let y = ui.max_rect().top() + (row_idx as f32 - visible.start as f32) * pitch;
                let rect = Rect::from_min_size(
                    Pos2::new(ui.max_rect().left(), y),
                    Vec2::new(ui.max_rect().width(), ROW_H),
                );
                ui.scroll_to_rect(rect, Some(Align::Center));
            }
            // The match above guarantees the rendered diff is `key`, so hunk
            // scroll-dedup state can be namespaced per diff with it (issue #11).
            if side_by_side {
                render_side_by_side(ui, state, &model, visible, &key, status, path);
            } else {
                render_unified(ui, state, &model, visible, &key, status, path);
            }
        },
    );
}

/// Issue the hunk scroll request at most once per (diff, hunk): re-issuing
/// the index-based scroll every frame would keep the ScrollArea repainting
/// forever.
fn hunk_needs_scroll(ui: &Ui, diff_key: &str, idx: usize) -> bool {
    let id = egui::Id::new(("diff_hunk_scrolled", diff_key, idx));
    let done = ui.ctx().memory(|m| m.data.get_temp::<bool>(id)) == Some(true);
    if !done {
        ui.ctx().memory_mut(|m| m.data.insert_temp(id, true));
    }
    !done
}

// --- partial staging (spec R2) ----------------------------------------------

/// Resolve the previewed file's [`ChangeStatus`] from the selected root's
/// cached changelists (read-only per frame). The commit window previews
/// root-relative paths; absolute paths match too so other callers stay safe.
/// Unlisted paths fall back to [`ChangeStatus::Modified`] — controls stay
/// enabled and the engine seam remains the final authority. Also serves the
/// palette's Stage/Unstage Hunk verbs.
pub(crate) fn preview_status(state: &AppState, path: Option<&std::path::Path>) -> ChangeStatus {
    let Some(path) = path else {
        return ChangeStatus::Modified;
    };
    if let Some(id) = &state.selected_root
        && let Some(root) = state.multi.by_id(id)
        && let Some(c) = root.resolve_change(path)
    {
        return c.status;
    }
    ChangeStatus::Modified
}

/// Hunk count of the diff the Commit window's preview would render right
/// now — 0 while nothing is selected, still loading, errored, or the text
/// parses to no hunks (binary). Reads the memoized display model beside the
/// cache (ADR-0014), so F7/Shift+F7 (spec R7) can consult it per keypress
/// without rebuilding any row map.
pub(crate) fn preview_hunk_count(state: &AppState) -> usize {
    let Some(root) = state.selected_path() else {
        return 0;
    };
    let Some(path) = state.ui.preview_change.clone() else {
        return 0;
    };
    let (eff_left, eff_right, staged) = comparison_triple(&None, &None, state.ui.diff_comparison);
    let key = diff_key(
        &root,
        &eff_left,
        &eff_right,
        staged,
        state.ui.diff_ignore_whitespace,
        &Some(path),
    );
    state
        .ui
        .diff_cache
        .as_ref()
        .filter(|(k, _)| k == &key)
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(_, t)| diff_model(t).hunk_count())
        .unwrap_or(0)
}

/// Whether one changed line currently sits in the accumulated sub-hunk
/// selection (spec R2 story 3).
fn line_selected(
    state: &AppState,
    path: &Option<std::path::PathBuf>,
    hunk: usize,
    ord: usize,
) -> bool {
    path.as_ref()
        .and_then(|p| state.ui.line_selections.get(p))
        .and_then(|m| m.get(&hunk))
        .is_some_and(|s| s.contains(&ord))
}

/// Selected-line marker (spec R2 story 3): a BRAND edge bar on the row's
/// left — the IDE-gutter convention, readable over both diff band tints.
fn paint_selection_bar(painter: &egui::Painter, rect: &Rect) {
    painter.rect_filled(
        Rect::from_min_size(rect.left_top(), Vec2::new(2.0, rect.height())),
        CornerRadius::ZERO,
        Palette::BRAND,
    );
}

/// The accumulated line selection for one hunk, when any.
fn line_selection_for(
    state: &AppState,
    path: &Option<std::path::PathBuf>,
    hunk: usize,
) -> Option<BTreeSet<usize>> {
    let lines = path
        .as_ref()
        .and_then(|p| state.ui.line_selections.get(p))
        .and_then(|m| m.get(&hunk))?;
    (!lines.is_empty()).then(|| lines.clone())
}

/// Dispatch granular stage/unstage of one whole hunk or the accumulated
/// sub-hunk line selection (spec R2): pure intent — the core granular module
/// resolves the cached patch text (ADR-0013), status, routing, label, and
/// scope.
fn dispatch_hunk_action(
    state: &mut AppState,
    hunk: usize,
    stage: bool,
    path: &Option<std::path::PathBuf>,
) {
    let target = match line_selection_for(state, path, hunk) {
        // Story 3: an accumulated sub-hunk selection narrows the patch to
        // exactly the toggled lines; otherwise the whole hunk applies.
        Some(lines) => granular::HunkTarget::Lines(hunk, lines),
        None => granular::HunkTarget::Whole(hunk),
    };
    let Some(path) = path.clone() else {
        return;
    };
    granular::dispatch(state, path, target, stage);
}

// --- toolbar widgets ---------------------------------------------------------

/// Compact segmented control (spec §8.4): SURFACE_2 track, the selected
/// segment sits on SURFACE_3 with INK ink. Returns the clicked option index.
fn segmented_control(ui: &mut Ui, options: &[&str], selected: usize) -> Option<usize> {
    const SEGMENT_H: f32 = 24.0;
    const PAD_X: f32 = 10.0;
    let font_id = FontId::new(12.0, FontFamily::Proportional);

    let widths: Vec<f32> = options
        .iter()
        .map(|o| {
            let g = ui
                .painter()
                .layout_no_wrap((*o).to_owned(), font_id.clone(), Color32::WHITE);
            g.size().x + PAD_X * 2.0
        })
        .collect();
    let track_w: f32 = widths.iter().sum();

    let (track, _) = ui.allocate_exact_size(Vec2::new(track_w, SEGMENT_H), Sense::hover());
    ui.painter()
        .rect_filled(track, CornerRadius::same(4), Palette::SURFACE_2);

    let mut clicked = None;
    let mut x = track.left();
    for (i, option) in options.iter().enumerate() {
        let seg = Rect::from_min_size(Pos2::new(x, track.top()), Vec2::new(widths[i], SEGMENT_H));
        let id = ui.id().with(("diff-segment", i));
        let resp = ui.interact(seg, id, Sense::click());
        let is_selected = i == selected;
        if is_selected {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(3), Palette::SURFACE_3);
        }
        let ink = if is_selected || resp.hovered() {
            Palette::INK
        } else {
            Palette::INK_2
        };
        paint_centered(ui.painter(), seg, option, font_id.clone(), ink);
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, *option));
        widgets::focus_ring(ui, &resp);
        if resp.clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    clicked
}

/// Revision chips (spec §8.4): Repo/Staged/Local select the documented
/// working-tree comparison pair.
fn comparison_chips(ui: &mut Ui, state: &mut AppState) {
    for (cmp, label) in [
        (DiffComparison::Repo, "Repo"),
        (DiffComparison::Staged, "Staged"),
        (DiffComparison::Local, "Local"),
    ] {
        let selected = state.ui.diff_comparison == cmp;
        if chip_button(ui, label, selected).clicked() {
            state.ui.diff_comparison = cmp;
        }
    }
}

/// Pill-shaped selectable chip: selected = solid BRAND with brand ink,
/// unselected = SURFACE_3 with muted ink that brightens on hover.
fn chip_button(ui: &mut Ui, label: &str, selected: bool) -> Response {
    const CHIP_H: f32 = 18.0;
    const PAD_X: f32 = 10.0;
    let font_id = FontId::new(11.0, FontFamily::Proportional);

    let idle_fg = if selected {
        Palette::BRAND_INK
    } else {
        Palette::INK_2
    };
    let measured = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font_id.clone(), idle_fg);
    let size = Vec2::new(measured.size().x + PAD_X * 2.0, CHIP_H);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let id = ui.id().with(("diff-chip", label));
    let response = ui.interact(rect, id, Sense::click());

    let bg = if selected {
        Palette::BRAND
    } else {
        Palette::SURFACE_3
    };
    let fg = if selected || response.hovered() {
        if selected {
            Palette::BRAND_INK
        } else {
            Palette::INK
        }
    } else {
        Palette::INK_2
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(CHIP_H as u8 / 2), bg);
    paint_centered(ui.painter(), rect, label, font_id, fg);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    widgets::focus_ring(ui, &response);
    response
}

/// Hunk navigation ‹ n/N › (spec §8.4). Stepping clamps to [0, total):
/// Previous never goes above the first hunk, Next never past the last.
fn hunk_nav(ui: &mut Ui, state: &mut AppState, total_hunks: usize) {
    let enabled = total_hunks > 0;
    let prev = nav_button(ui, Icon::CHEVRON_LEFT, "Previous hunk", enabled);
    if enabled {
        let current = state.ui.diff_current_hunk.min(total_hunks - 1);
        ui.label(format!("{}/{}", current + 1, total_hunks));
    }
    let next = nav_button(ui, Icon::CHEVRON_RIGHT, "Next hunk", enabled);

    if prev.clicked() {
        state.ui.diff_current_hunk = state.ui.diff_current_hunk.saturating_sub(1);
    }
    if next.clicked() && enabled {
        state.ui.diff_current_hunk = (state.ui.diff_current_hunk + 1).min(total_hunks - 1);
    }
}

/// Square ghost icon button with an explicit accessibility label.
fn nav_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    const SIZE: f32 = 24.0;
    const ICON_SIZE: f32 = 14.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::hover());
    let id = ui.id().with(("diff-nav", label));
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if response.is_pointer_button_down_on() {
        Palette::SURFACE_3
    } else if response.hovered() {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    }
    let ink = if !enabled {
        Palette::INK_3
    } else if response.hovered() || response.is_pointer_button_down_on() {
        Palette::INK
    } else {
        Palette::INK_2
    };
    paint_icon_at(ui, icon, rect.center(), ICON_SIZE, ink);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    widgets::focus_ring(ui, &response);
    response
}

/// Compact ghost action button painted inside an already-allocated row rect
/// (gutter scale, 18px): transparent at rest, SURFACE_2 hover fill with
/// INK_2→INK glyph ink, SURFACE_3 while pressed — the [`nav_button`] ladder
/// shrunk onto the hunk band. A real interactable widget carrying labeled
/// Button accessibility info, so kittest and screen readers can find it.
fn gutter_button(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    glyph: &str,
    label: &str,
    tooltip: &str,
    enabled: bool,
) -> Response {
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if response.is_pointer_button_down_on() {
        Palette::SURFACE_3
    } else if response.hovered() {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    }
    let ink = if !enabled {
        Palette::INK_3
    } else if response.hovered() || response.is_pointer_button_down_on() {
        Palette::INK
    } else {
        Palette::INK_2
    };
    paint_centered(ui.painter(), rect, glyph, mono_font(), ink);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    widgets::focus_ring(ui, &response);
    if enabled {
        response.on_hover_text(tooltip)
    } else {
        response.on_hover_text("Resolve the conflict first")
    }
}

/// Stage/unstage gutter pair on a hunk-header band (spec R2): two compact
/// buttons at the band's left edge — "+" stages that whole hunk forward,
/// "−" reverse-applies it out of the index (1-based numbering in labels).
/// Conflicted files keep the pair visible but inert; conflicts resolve
/// through the conflict modal.
fn hunk_gutter_actions(
    ui: &mut Ui,
    state: &mut AppState,
    band: Rect,
    diff_key: &str,
    hunk: usize,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
) {
    const BTN: f32 = 18.0;
    const PAD_X: f32 = 6.0;
    const GAP: f32 = 4.0;
    let enabled = status != ChangeStatus::Conflicted;

    let y = band.center().y - BTN / 2.0;
    let stage_rect = Rect::from_min_size(Pos2::new(band.left() + PAD_X, y), Vec2::splat(BTN));
    let unstage_rect = Rect::from_min_size(
        Pos2::new(band.left() + PAD_X + BTN + GAP, y),
        Vec2::splat(BTN),
    );

    let base_id = ui.id().with(("diff-gutter", diff_key));
    let n = hunk + 1;
    let stage_label = format!("Stage hunk {n}");
    let stage = gutter_button(
        ui,
        stage_rect,
        base_id.with(("stage", hunk)),
        "+",
        &stage_label,
        "Stage this hunk",
        enabled,
    );
    let unstage_label = format!("Unstage hunk {n}");
    let unstage = gutter_button(
        ui,
        unstage_rect,
        base_id.with(("unstage", hunk)),
        "-",
        &unstage_label,
        "Unstage this hunk",
        enabled,
    );

    if stage.clicked() {
        dispatch_hunk_action(state, hunk, true, path);
    }
    if unstage.clicked() {
        dispatch_hunk_action(state, hunk, false, path);
    }
}

/// Paint one icon primitive centered at `origin` without disturbing layout.
fn paint_icon_at(ui: &mut Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let origin = Pos2::new(center.x - size / 2.0, center.y - size / 2.0);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(origin, Vec2::splat(size)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    icons::icon(&mut child, icon, size, color);
}

/// Paint a string centered inside `rect`.
fn paint_centered(painter: &egui::Painter, rect: Rect, text: &str, font: FontId, color: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    painter.galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

// --- rendering ---------------------------------------------------------------

/// Pane-header labels documenting the active pair (spec §8.4).
fn pane_labels(comparison: DiffComparison) -> (&'static str, &'static str) {
    match comparison {
        DiffComparison::Repo => ("HEAD", "Working tree"),
        DiffComparison::Staged => ("HEAD", "Index"),
        DiffComparison::Local => ("Index", "Working tree"),
    }
}

/// One vertically-centered text cell painted at absolute x inside `rect`.
fn paint_cell(
    painter: &egui::Painter,
    x: f32,
    rect: &Rect,
    text: &str,
    color: Color32,
    font: &FontId,
) {
    let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), color);
    painter.galley(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// Unified-mode sign + line-number gutter cells (muted INK_3 numbers).
fn paint_gutter(
    painter: &egui::Painter,
    rect: &Rect,
    sign: &str,
    num: usize,
    sign_color: Color32,
    font: &FontId,
) {
    paint_cell(painter, rect.left() + 4.0, rect, sign, sign_color, font);
    let num_text = num.to_string();
    let galley = painter.layout_no_wrap(num_text, font.clone(), Palette::INK_3);
    let x = rect.left() + SIGN_W + NUM_W - galley.size().x - 6.0;
    painter.galley(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        Palette::INK_3,
    );
}

/// Aim the current hunk (CONTEXT.md "Current hunk") at the row under the
/// pointer — but only when the pointer genuinely rests on the rendered diff
/// rows AND moved this frame. A stationary pointer must not fight keyboard
/// or button navigation that just scrolled a different hunk underneath it
/// (spec R7: one canonical selection). Elsewhere — other panes, floating
/// popups, headless state injection — the previous value stays authoritative,
/// so navigation and the palette verbs operate on the hunk last aimed at.
fn commit_current_hunk(
    state: &mut AppState,
    ui: &Ui,
    rows_rect: Option<Rect>,
    frame_hover: Option<usize>,
) {
    let moved = ui.input(|i| i.pointer.motion().is_some_and(|d| d != Vec2::ZERO));
    let inside = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| rows_rect.is_some_and(|r| r.contains(p)));
    if moved
        && inside
        && let Some(hunk) = frame_hover
    {
        state.ui.diff_current_hunk = hunk;
    }
}

/// Unified mode (spec §8.4): one full-width band per underlying row, paging
/// only the visible window of the cached display model (ADR-0014). Pairs are
/// flattened back into their constituent rows, so output is pixel-identical
/// to the pre-virtualization loop.
fn render_unified(
    ui: &mut Ui,
    state: &mut AppState,
    model: &DiffModel,
    visible: Range<usize>,
    diff_key: &str,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
) {
    let width = ui.available_width();
    let painter = ui.painter().clone();
    let font = mono_font();

    let end = visible.end.min(model.raw_count);
    let start = visible.start.min(end);
    if start < end {
        // Hover tracking (spec R2): which hunk sits under the pointer this
        // frame — visible rows only, which is correct under virtualization.
        let mut frame_hover: Option<usize> = None;
        let mut rows_rect: Option<Rect> = None;
        // The underlying-row window may open or close mid-pair; visit every
        // display element touching it and keep only the rows inside it.
        let first = model.raw_to_display[start] as usize;
        let last = model.raw_to_display[end - 1] as usize;
        for disp in &model.display[first..=last] {
            match disp {
                DisplayRow::Full(row) => {
                    if visible.contains(&row.ord) {
                        unified_row(
                            ui,
                            state,
                            row,
                            width,
                            &painter,
                            &font,
                            diff_key,
                            status,
                            path,
                            &mut frame_hover,
                            &mut rows_rect,
                        );
                    }
                }
                DisplayRow::Pair(del, add) => {
                    for row in [del.as_ref(), add.as_ref()].into_iter().flatten() {
                        if visible.contains(&row.ord) {
                            unified_row(
                                ui,
                                state,
                                row,
                                width,
                                &painter,
                                &font,
                                diff_key,
                                status,
                                path,
                                &mut frame_hover,
                                &mut rows_rect,
                            );
                        }
                    }
                }
            }
        }
        commit_current_hunk(state, ui, rows_rect, frame_hover);
    }
}

/// One unified band: allocation, hover tracking, click toggle, and the
/// token-exact paint for a single underlying row (spec §8.4/R2). Hunk
/// scrolling is handled centrally by the [`ScrollArea::show_rows`] caller.
#[allow(clippy::too_many_arguments)]
fn unified_row(
    ui: &mut Ui,
    state: &mut AppState,
    row: &Row,
    width: f32,
    painter: &egui::Painter,
    font: &FontId,
    diff_key: &str,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
    frame_hover: &mut Option<usize>,
    rows_rect: &mut Option<Rect>,
) {
    // Changed lines are clickable toggles (spec R2 story 3); everything
    // else stays hover-only.
    let toggleable = matches!(row.kind, RowKind::Del | RowKind::Add);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, ROW_H),
        if toggleable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    *rows_rect = Some(match *rows_rect {
        Some(union) => union.union(rect),
        None => rect,
    });
    if row.kind != RowKind::Meta && resp.hovered() {
        *frame_hover = Some(row.hunk);
    }
    if toggleable {
        // Accessibility: the row is labeled by its content text so
        // tooling can target individual changed lines.
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, row.text.as_str()));
        if resp.clicked() {
            granular::toggle_line_selection(state, path, row.hunk, row.line_ord);
        }
    }
    match row.kind {
        RowKind::Meta => {
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::INK_3,
                font,
            );
        }
        RowKind::Hunk => {
            painter.rect_filled(rect, CornerRadius::ZERO, Palette::SURFACE);
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::INK_3,
                font,
            );
            hunk_gutter_actions(ui, state, rect, diff_key, row.hunk, status, path);
        }
        RowKind::Context => {
            paint_gutter(painter, &rect, " ", row.new_no, Palette::INK_3, font);
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::INK,
                font,
            );
        }
        RowKind::Del => {
            painter.rect_filled(rect, CornerRadius::ZERO, Palette::DIFF_DEL_BG);
            if line_selected(state, path, row.hunk, row.line_ord) {
                paint_selection_bar(painter, &rect);
            }
            paint_gutter(
                painter,
                &rect,
                "-",
                row.old_no,
                Palette::DIFF_DEL_TEXT,
                font,
            );
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::DIFF_DEL_TEXT,
                font,
            );
        }
        RowKind::Add => {
            painter.rect_filled(rect, CornerRadius::ZERO, Palette::DIFF_ADD_BG);
            if line_selected(state, path, row.hunk, row.line_ord) {
                paint_selection_bar(painter, &rect);
            }
            paint_gutter(
                painter,
                &rect,
                "+",
                row.new_no,
                Palette::DIFF_ADD_TEXT,
                font,
            );
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::DIFF_ADD_TEXT,
                font,
            );
        }
    }
}

/// Side-by-side pane header band (SURFACE, spec §8.4).
fn header_band(ui: &mut Ui, width: f32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, PANE_HEADER_H), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), Palette::SURFACE);
    paint_centered(
        ui.painter(),
        rect,
        label,
        FontId::new(12.0, FontFamily::Proportional),
        Palette::INK,
    );
}

/// One side-by-side cell: optional token background band, muted gutter
/// number, sign marker, and code text. Changed cells are clickable line
/// toggles labeled by their content (spec R2 story 3); `selected` paints
/// the BRAND edge bar. Returns the cell's response so the row loop can
/// track hover and toggle clicks.
#[allow(clippy::too_many_arguments)]
fn cell_band(
    ui: &mut Ui,
    width: f32,
    fill: Option<Color32>,
    num: usize,
    sign: char,
    text: &str,
    text_color: Color32,
    painter: &egui::Painter,
    font: &FontId,
    toggleable: bool,
    selected: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, ROW_H),
        if toggleable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if let Some(bg) = fill {
        painter.rect_filled(rect, CornerRadius::ZERO, bg);
    }
    if selected {
        paint_selection_bar(painter, &rect);
    }
    if toggleable {
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, text));
    }
    if num > 0 {
        let galley = painter.layout_no_wrap(num.to_string(), font.clone(), Palette::INK_3);
        painter.galley(
            Pos2::new(
                rect.left() + NUM_W - galley.size().x - 6.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::INK_3,
        );
    }
    paint_cell(
        painter,
        rect.left() + NUM_W + 6.0,
        &rect,
        &sign.to_string(),
        text_color,
        font,
    );
    paint_cell(
        painter,
        rect.left() + NUM_W + 20.0,
        &rect,
        text,
        text_color,
        font,
    );
    resp
}

/// Side-by-side mode (spec §8.4): paired Del/Add bands plus full-width
/// context/hunk/meta rows, paging only the visible window of the cached
/// display model (ADR-0014). The pane header band is pinned by the caller.
/// Hover tracking and toggles apply to visible rows only — correct under
/// virtualization.
fn render_side_by_side(
    ui: &mut Ui,
    state: &mut AppState,
    model: &DiffModel,
    visible: Range<usize>,
    diff_key: &str,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
) {
    let width = ui.available_width();
    let spacing = ui.style().spacing.item_spacing.x;
    let half = ((width - spacing * 2.0) / 2.0).max(120.0);

    let painter = ui.painter().clone();
    let font = mono_font();
    // Hover tracking (spec R2): which hunk sits under the pointer this frame.
    let mut frame_hover: Option<usize> = None;
    let mut rows_rect: Option<Rect> = None;

    let end = visible.end.min(model.display.len());
    let start = visible.start.min(end);
    for disp in &model.display[start..end] {
        match disp {
            DisplayRow::Full(row) => match row.kind {
                RowKind::Meta => {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                    rows_rect = Some(match rows_rect {
                        Some(union) => union.union(rect),
                        None => rect,
                    });
                    paint_cell(
                        &painter,
                        rect.left() + TEXT_X,
                        &rect,
                        &row.text,
                        Palette::INK_3,
                        &font,
                    );
                }
                RowKind::Hunk => {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                    rows_rect = Some(match rows_rect {
                        Some(union) => union.union(rect),
                        None => rect,
                    });
                    if resp.hovered() {
                        frame_hover = Some(row.hunk);
                    }
                    painter.rect_filled(rect, CornerRadius::ZERO, Palette::SURFACE);
                    paint_cell(
                        &painter,
                        rect.left() + TEXT_X,
                        &rect,
                        &row.text,
                        Palette::INK_3,
                        &font,
                    );
                    hunk_gutter_actions(ui, state, rect, diff_key, row.hunk, status, path);
                }
                RowKind::Context => {
                    let hunk = row.hunk;
                    ui.horizontal(|ui| {
                        let old_cell = cell_band(
                            ui,
                            half,
                            None,
                            row.old_no,
                            ' ',
                            &row.text,
                            Palette::INK,
                            &painter,
                            &font,
                            false,
                            false,
                        );
                        let new_cell = cell_band(
                            ui,
                            half,
                            None,
                            row.new_no,
                            ' ',
                            &row.text,
                            Palette::INK,
                            &painter,
                            &font,
                            false,
                            false,
                        );
                        if old_cell.hovered() || new_cell.hovered() {
                            frame_hover = Some(hunk);
                        }
                    });
                }
                // Changed rows always live in pairs (build_model invariant).
                RowKind::Del | RowKind::Add => unreachable!("changed rows are always paired"),
            },
            DisplayRow::Pair(d, a) => {
                let hunk = d.as_ref().map(|r| r.hunk).or(a.as_ref().map(|r| r.hunk));
                ui.horizontal(|ui| {
                    let del_cell = if let Some(d) = d {
                        let selected = line_selected(state, path, d.hunk, d.line_ord);
                        let cell = cell_band(
                            ui,
                            half,
                            Some(Palette::DIFF_DEL_BG),
                            d.old_no,
                            '-',
                            &d.text,
                            Palette::DIFF_DEL_TEXT,
                            &painter,
                            &font,
                            true,
                            selected,
                        );
                        if cell.clicked() {
                            granular::toggle_line_selection(state, path, d.hunk, d.line_ord);
                        }
                        cell
                    } else {
                        cell_band(
                            ui,
                            half,
                            None,
                            0,
                            ' ',
                            "",
                            Palette::INK,
                            &painter,
                            &font,
                            false,
                            false,
                        )
                    };
                    let add_cell = if let Some(a) = a {
                        let selected = line_selected(state, path, a.hunk, a.line_ord);
                        let cell = cell_band(
                            ui,
                            half,
                            Some(Palette::DIFF_ADD_BG),
                            a.new_no,
                            '+',
                            &a.text,
                            Palette::DIFF_ADD_TEXT,
                            &painter,
                            &font,
                            true,
                            selected,
                        );
                        if cell.clicked() {
                            granular::toggle_line_selection(state, path, a.hunk, a.line_ord);
                        }
                        cell
                    } else {
                        cell_band(
                            ui,
                            half,
                            None,
                            0,
                            ' ',
                            "",
                            Palette::INK,
                            &painter,
                            &font,
                            false,
                            false,
                        )
                    };
                    if (del_cell.hovered() || add_cell.hovered())
                        && let Some(h) = hunk
                    {
                        frame_hover = Some(h);
                    }
                });
            }
        }
    }
    commit_current_hunk(state, ui, rows_rect, frame_hover);
}
