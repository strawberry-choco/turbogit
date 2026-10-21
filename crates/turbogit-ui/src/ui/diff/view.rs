//! Diff rendering: the virtualized entry point plus the unified and
//! side-by-side row painters (spec §8.4, ADR-0014).
use super::actions::{
    commit_current_hunk, diff_options_menu, hunk_gutter_actions, hunk_header_extras, hunk_nav,
};
use super::model::{
    DiffModel, DisplayRow, Highlight, NUM_W, PANE_HEADER_H, PaneKind, ROW_H, Row, RowKind, SIGN_W,
    TEXT_X, is_file_row, mono_font, pane_kind,
};
use super::panes::{binary_placeholder, pane_byte_lens, render_image_pane};
use crate::theme::Palette;
use crate::ui::{components, widgets};
use egui::{
    Align, Color32, CornerRadius, FontId, Layout, Pos2, Rect, Response, RichText, ScrollArea,
    Sense, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::ops::Range;
use turbogit_app::granular;
use turbogit_app::keyed_read::{DiffTarget, PaneFlavour, PaneTarget, Read};
use turbogit_app::staged_lines::{self, FileLineSets};
use turbogit_app::state::{AppState, DiffPane};
use turbogit_domain::model::{ChangeStatus, DisplayName};

// --- staged-ness: the one per-line answer the painters read --------------------

/// The per-line oracle, resolved for the file this pane is previewing.
///
/// Every colour and every word about a row's staged-ness comes from here, and
/// nothing comes from the patch the rows were built from. That is the hazard the
/// whole pane turns on: for an unmerged path `git diff HEAD` prints an ordinary
/// two-sided diff whose added lines **are** the conflict markers, so a treatment
/// read off the displayed patch would paint a conflicted file's own markers as
/// staged additions. The oracle answers `None` for that path instead.
///
/// It is resolved once per frame, at the top of the walk that already reads every
/// row, because answering it per row would re-resolve the changelist entry per
/// painted line.
struct StagedRows<'a> {
    path: &'a std::path::Path,
    sets: &'a [FileLineSets],
}

impl<'a> StagedRows<'a> {
    /// The selected root's changelist entry for `path`, and the line sets keyed on
    /// git's own spelling of it.
    ///
    /// The pane hands an **absolute** path (its entry points take the previewed
    /// change as the shell knows it) while the caches key on the repo-relative `b/`
    /// side, so the changelist lookup is the one place the two spellings meet. A
    /// root with no `hunk_stats` entry yet answers nothing at all, which is `None`
    /// per row rather than a guess.
    fn resolve(state: &'a AppState, path: &Option<std::path::PathBuf>) -> Option<Self> {
        let root = state
            .selected_root
            .as_ref()
            .and_then(|id| state.multi.by_id(id))?;
        let change = root.resolve_change(path.as_deref()?)?;
        let stats = state.caches.hunk_stats(&root.id)?;
        Some(Self {
            path: &change.path,
            sets: &stats.line_sets,
        })
    }

    /// Whether this row's line is in the index, or has nothing to say about it.
    fn answer(&self, kind: RowKind, old_no: usize, new_no: usize) -> Option<bool> {
        staged_lines::line_is_staged(self.sets, self.path, kind, old_no, new_no)
    }
}

/// The oracle's answers for the two halves of one display row.
#[derive(Clone, Copy, Default)]
struct PairStaging {
    del: Option<bool>,
    add: Option<bool>,
}

impl PairStaging {
    fn for_kind(self, kind: RowKind) -> Option<bool> {
        match kind {
            RowKind::Add => self.add,
            RowKind::Del => self.del,
            _ => None,
        }
    }
}

/// What the per-line answers say about one hunk as a whole.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HunkStaging {
    /// Every answered row is staged.
    Staged,
    /// Every answered row is unstaged.
    Unstaged,
    /// The hunk holds both states.
    Mixed,
    /// No row of it was answered: a conflicted path, or a hunk with nothing in it
    /// that a stage can reach.
    NoAnswer,
}

impl HunkStaging {
    /// Take one more row's answer. A `None` contributes nothing, which is safe
    /// rather than generous: an unanswered *changed* row means the whole path went
    /// unanswered (a conflict, or a file absent from both appliable patches), and
    /// there is no shape where one row of a hunk is answered and its neighbour is
    /// not.
    fn fold(self, answer: Option<bool>) -> Self {
        match (self, answer) {
            (s, None) => s,
            (Self::NoAnswer, Some(true)) => Self::Staged,
            (Self::NoAnswer, Some(false)) => Self::Unstaged,
            (Self::Staged, Some(false)) | (Self::Unstaged, Some(true)) => Self::Mixed,
            (s, _) => s,
        }
    }

    /// The hunk header's suffix, and no suffix at all where the oracle said nothing.
    ///
    /// This text exists because muting cannot show a mixed hunk: a hunk holding
    /// both states looks like a hunk holding some quiet rows, and nobody can count
    /// those at a glance. Spelled `· part`, and it is **not** the staged-hunk chip's
    /// ` (part)` in `commit_window.rs`. That one labels a chip in the commit
    /// window's index list, this one labels a hunk header on the diff screen: two
    /// controls with two wordings, so one shared constant cannot serve both
    /// without renaming the label the other shows.
    fn suffix(self) -> Option<&'static str> {
        match self {
            Self::Staged => Some("· staged"),
            Self::Mixed => Some("· part"),
            Self::Unstaged => Some("· not staged"),
            Self::NoAnswer => None,
        }
    }
}

/// The band and ink one changed row wears, from the oracle's answer.
///
/// Staged is the foreground, so a row the oracle cannot answer keeps the full
/// treatment as well: muting it would report it unstaged, which is a claim about
/// the index nothing made. Only `Some(false)` quiets.
fn row_treatment(kind: RowKind, staged: Option<bool>) -> (Color32, Color32) {
    let (band, ink) = match (kind, staged) {
        (RowKind::Add, Some(false)) => (
            Palette::DIFF_ADD_UNSTAGED_BG,
            Palette::DIFF_ADD_UNSTAGED_TEXT,
        ),
        (RowKind::Add, _) => (Palette::DIFF_ADD_BG, Palette::DIFF_ADD_TEXT),
        (RowKind::Del, Some(false)) => (
            Palette::DIFF_DEL_UNSTAGED_BG,
            Palette::DIFF_DEL_UNSTAGED_TEXT,
        ),
        (RowKind::Del, _) => (Palette::DIFF_DEL_BG, Palette::DIFF_DEL_TEXT),
        _ => return (Palette::DIFF_ADD_BG, Palette::DIFF_ADD_TEXT),
    };
    (band, ink)
}

/// A changed row's accessibility label: its content, then its staged-ness.
///
/// The **state**, and not the consequence of the click, on purpose. The two
/// diverge in both directions: an unstage moves a replacement *pair* together,
/// because a `-` and the `+` under it are one change, while a row on a file git has
/// never had a version of reaches the whole *file*, because git refuses a partial
/// reverse there. A label that named the consequence would promise per-row control
/// the click does not always carry, which is a misleading mark of a new kind. The
/// state is true in every shape, and the parenthetical is the whole-file
/// checkbox's own vocabulary (`stage_label` in `commit_window.rs`), so the row and
/// the box beside it cannot disagree.
fn row_label(text: &str, staged: Option<bool>) -> String {
    match staged {
        Some(true) => format!("{text} (staged)"),
        Some(false) => format!("{text} (unstaged)"),
        None => format!("{text} (staged-ness unknown)"),
    }
}

/// The ink of a changed row's line numbers, which is where the pointer answers.
///
/// Not a fill: the app's hover rung is `SURFACE_2`, and against a muted band that
/// measures 1.01:1, i.e. an interaction fill laid over a muted row is invisible on
/// exactly the rows that most need it. The row's band and ink are now spent on
/// staged-ness, so the pointer's answer lives in the gutter column, which carries
/// no git state at all and steps one rung up the ink ramp for both states.
fn gutter_ink(hovered: bool) -> Color32 {
    if hovered {
        Palette::INK_2
    } else {
        Palette::INK_3
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
            widgets::empty_state(ui, "No repository selected.");
            return;
        }
    };

    // The two kinds of diff target, told apart by which constructor they ask
    // for: no revision named means the working-tree preview, and the working
    // tree is read as `HEAD ↔ worktree`. A caller that names both sides gets
    // exactly the pair it asked for.
    let target = match (left, right) {
        (None, None) => DiffTarget::working_tree(root, state.ui.diff_policies, path.clone()),
        _ => DiffTarget::new(
            root,
            left.clone(),
            right.clone(),
            state.ui.diff_policies,
            path.clone(),
        ),
    };
    // One ask, one verdict: the read answers from the cache, or starts the
    // load and says it is waiting. The widget-state salt below is derived from
    // the target, never from the cache key, which the caller cannot name.
    let verdict = state.read(target.clone());
    // A root-wide read of the view this pane displays failed: the whole file
    // counts as failed to read, content and all. Consulted on a `Fresh` or
    // `Empty` verdict because every other verdict already stops the rows, and
    // an `(no differences)` answer from the per-path read is a claim the dead
    // root read just took away. The rule and its exemptions live in
    // [`AppState::failed_view_message`].
    let view_failure = match &verdict {
        Read::Fresh(_) | Read::Empty => state.failed_view_message(&target),
        _ => None,
    };
    let total_hunks = match &verdict {
        Read::Fresh(diff) if view_failure.is_none() => diff.model.hunk_count(),
        _ => 0,
    };

    // Toolbar chrome (spec §8.4): view mode · hunk nav.
    // Wrapped so narrow panes (commit preview) push a control to a second row
    // instead of clipping it out of reach.
    ui.horizontal_wrapped(|ui| {
        // The toolbar's own item rhythm is the gap between its groups: the
        // frames are the separation, so there is no rule painted between them.
        let gap = ui.spacing().item_spacing.x;
        let selected = if state.ui.diff_side_by_side { 0 } else { 1 };
        if let Some(idx) = widgets::segmented_control(ui, &["Side-by-Side", "Unified"], selected) {
            state.ui.diff_side_by_side = idx == 0;
        }
        ui.add_space(gap);
        hunk_nav(ui, state, total_hunks);
    });

    // The verdict is the whole paint contract: which of these four the
    // surface shows is answered by the read, so there is no loading flag left
    // to forget to check and no error field that can describe some other
    // comparison (ADR-0021).
    //
    // Every state that is not `Fresh` still paints the file row, because the
    // file row is where the pane's comparison toggle lives: a comparison that
    // came back empty, is still computing, or failed must not take the control
    // that produces it off screen along with the rows it would apply to.
    if let Some(message) = &view_failure {
        file_header(ui, state, path.as_deref());
        widgets::keyed_read_presentation(
            ui,
            widgets::KeyedReadPresentation::Failed(message.as_str()),
        );
        return;
    }
    let diff = match verdict {
        Read::Fresh(diff) => diff,
        Read::Empty => {
            file_header(ui, state, path.as_deref());
            ui.label("(no differences)");
            return;
        }
        Read::Waiting => {
            file_header(ui, state, path.as_deref());
            widgets::keyed_read_presentation(
                ui,
                widgets::KeyedReadPresentation::Waiting("Computing diff…"),
            );
            return;
        }
        Read::Failed(message) => {
            file_header(ui, state, path.as_deref());
            widgets::keyed_read_presentation(ui, widgets::KeyedReadPresentation::Failed(&message));
            return;
        }
    };
    let model = &diff.model;

    // Non-text diffs render outside the display-row model (spec R8,
    // ADR-0015): a lone binary change or an image pair replaces the rows
    // entirely — the mode toggle has no second layout to switch to, and
    // with no hunks there is nothing to navigate or stage. The pane is
    // addressed by the same target, so any reload of the patch text (ops
    // invalidate it) also refetches the pane bytes.
    match pane_kind(&model.files) {
        PaneKind::Text => {}
        PaneKind::Binary => {
            let pane = PaneTarget::new(
                target.clone(),
                PaneFlavour::Binary,
                model.files[0].clone(),
                false,
            );
            // While loading (or when a side is unreadable) the sizes are
            // unresolved and the bare description shows — the graceful
            // fallback text.
            let sizes = match state.read(pane) {
                Read::Fresh(entry) => pane_byte_lens(&entry),
                _ => None,
            };
            binary_placeholder(ui, sizes);
            return;
        }
        PaneKind::Image => {
            render_image_pane(ui, state, &target, &model.files[0]);
            return;
        }
    }

    // Pure rename (100% similar, no content hunks): the header plus a note
    // instead of an empty scroller (spec R8).
    if model.pure_rename() {
        if let Some(text) = &model.rename_header {
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
            paint_rename_header(ui.painter(), &rect, text, &mono_font());
        }
        widgets::empty_state(ui, "No content changes.");
        return;
    }

    // Gutter staging controls need the previewed file's status (spec R2);
    // resolved once per frame from the cached changelists.
    let status = granular::preview_status(state, path.as_deref());

    let side_by_side = state.ui.diff_side_by_side;
    // Per-frame paint plan (issue 20): collapsed hunks drop their body rows
    // from the virtualized stream, so both modes page over filtered slot
    // lists built in one O(model) walk. Side-by-side slots are display
    // indices; unified slots are (display index, member) with a Pair member
    // flattened into one slot per underlying row (ADR-0014 paging, now over
    // the filtered stream).
    let plan = paint_plan(state, model, path);
    let total_rows = if side_by_side {
        plan.sbs.len()
    } else {
        plan.unified.len()
    };

    // Side-by-side pane header band (spec §8.4), pinned above the paged
    // rows: `show_rows` reserves exactly the uniform-height row window, so
    // the fixed-height band leaves the scrolled content. Floating scrollbars
    // reserve no width, so the panes below still measure the same `half`.
    if side_by_side {
        let width = ui.available_width();
        let gap = ui.style().spacing.item_spacing.x;
        let half = ((width - gap * 2.0) / 2.0).max(120.0);
        let (left_label, right_label) = pane_labels(&target);
        ui.horizontal(|ui| {
            header_band(ui, half, &format!("Before {left_label}"));
            header_band(ui, half, &format!("After {right_label}"));
        });
    }

    // Identity of the diff being painted. The scroll-dedup and gutter widget
    // states below must reset when the surface starts showing a different
    // comparison, and the target is what says so — the read's cache key would
    // do as well but is not a thing a caller can hold (ADR-0021).
    let paint = egui::Id::new(("diff-view", &target));

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
            // header slot at the viewport center instead of relying on
            // a realized widget (`resp.scroll_to_me` cannot reach rows this
            // window didn't build). Issued inside the closure because egui
            // consumes scroll targets set by an area's content; ones set
            // before the area begins are stashed for outer areas. At most
            // once per (diff, hunk) — re-issuing every frame would keep the
            // ScrollArea repainting forever.
            if state.ui.diff_current_hunk < plan.hunk_slots.len()
                && hunk_needs_scroll(ui, paint, state.ui.diff_current_hunk)
            {
                let row_idx = plan.hunk_slots[state.ui.diff_current_hunk].slot(side_by_side);
                let pitch = ROW_H + ui.spacing().item_spacing.y;
                let y = ui.max_rect().top() + (row_idx as f32 - visible.start as f32) * pitch;
                let rect = Rect::from_min_size(
                    Pos2::new(ui.max_rect().left(), y),
                    Vec2::new(ui.max_rect().width(), ROW_H),
                );
                ui.scroll_to_rect(rect, Some(Align::Center));
            }
            // The verdict above guarantees the rendered diff is `target`'s, so
            // hunk scroll-dedup state can be namespaced per diff with it
            // (issue #11).
            if side_by_side {
                render_side_by_side(ui, state, model, &plan, visible, paint, status, path);
            } else {
                render_unified(ui, state, model, &plan, visible, paint, status, path);
            }
        },
    );
}

/// Where one hunk's header band sits, per stream: the side-by-side slot and
/// the unified slot.
///
/// The two are NOT the same number, and the difference is the whole reason this
/// is a pair. A `DisplayRow::Pair` is ONE side-by-side row (the deletion and
/// the addition share a line) but TWO unified rows, so every pair before a
/// hunk's header shifts the unified slot one further along. A header is a full
/// row in both modes — that part was always true — but the slot NUMBER it
/// occupies is a property of the stream being paged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HunkSlot {
    sbs: usize,
    unified: usize,
}

impl HunkSlot {
    const UNSET: Self = Self { sbs: 0, unified: 0 };

    /// The slot in the stream this frame pages.
    fn slot(&self, side_by_side: bool) -> usize {
        if side_by_side { self.sbs } else { self.unified }
    }
}

/// One file section's change counts, carried on the paint plan beside the slot
/// streams (spec story 23): the statistics a file row paints are a property of
/// the file, so they are measured once per frame from the model the keyed read
/// already answered with — never reparsed from patch text, and never recounted
/// per row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileRowStats {
    /// Display index of the section's file row — the row these counts describe.
    display: usize,
    added: usize,
    removed: usize,
}

/// One frame's paint plan over the cached display model (issue 20): the
/// filtered slot streams of both modes plus the per-hunk metadata the hunk
/// header bands render (header slot for scroll aiming, hidden-row count
/// while collapsed, changed-line count while expanded).
struct PaintPlan {
    /// Side-by-side slots: display indices, one band per entry.
    sbs: Vec<usize>,
    /// Unified slots: `(display index, member)` with member 0 = full row,
    /// 1 = a Pair's deletion, 2 = a Pair's addition.
    unified: Vec<(usize, u8)>,
    /// Per hunk: the slot its header band occupies in EACH stream — see
    /// [`HunkSlot`], which is why one number is not enough.
    hunk_slots: Vec<HunkSlot>,
    /// Per hunk: body bands hidden by collapsing.
    hunk_hidden: Vec<usize>,
    /// Per hunk: changed-line count of its body.
    hunk_lines: Vec<usize>,
    /// Per file row: the change counts of the section that row opens.
    file_rows: Vec<FileRowStats>,
    /// Display index of the row the caret sits on — the first changed row of the
    /// current hunk **on the pane that holds the caret**, so `⌃⇧⇥` visibly moves
    /// it and `F4` can name the line it is standing on.
    caret: Option<usize>,
    /// Per display row: the per-line oracle's answer for each half of a pair, so a
    /// row's colour, its accessibility label and the header's suffix all come from
    /// one resolution of the changelist entry rather than from four lookups per
    /// painted line.
    staging: Vec<PairStaging>,
    /// Per hunk: those answers aggregated, which is what names the header's
    /// `· staged` / `· part` / `· not staged` suffix.
    hunk_staging: Vec<HunkStaging>,
}

impl PaintPlan {
    /// `(added, removed)` for the section whose file row is at `display`.
    /// `(0, 0)` for a row that is not a file row's section.
    fn file_counts(&self, display: usize) -> (usize, usize) {
        self.file_rows
            .iter()
            .find(|f| f.display == display)
            .map_or((0, 0), |f| (f.added, f.removed))
    }

    /// The oracle's answer for one row of the display model.
    fn staged(&self, display: usize, kind: RowKind) -> Option<bool> {
        self.staging
            .get(display)
            .and_then(|entry| entry.for_kind(kind))
    }

    /// The suffix this hunk's header names, or nothing where the oracle has no
    /// answer for any of its rows.
    fn hunk_suffix(&self, hunk: usize) -> Option<&'static str> {
        self.hunk_staging.get(hunk).and_then(|s| s.suffix())
    }
}

/// Walk the display model once, filtering collapsed hunks' bodies out of
/// both slot streams (issue 20). Meta rows between sections stay visible —
/// they belong to no hunk body. The same walk totals each file section's
/// changed lines for the file row, which is why the counts are carried here
/// rather than recomputed per row (ticket 12), and the same walk asks the
/// per-line oracle about every changed row once, which is why staged-ness is
/// painted from a resolved frame rather than a lookup per painted line.
fn paint_plan(state: &AppState, model: &DiffModel, path: &Option<std::path::PathBuf>) -> PaintPlan {
    let hunk_count = model.hunk_count();
    let want_add = matches!(state.ui.diff_pane, DiffPane::New);
    let oracle = StagedRows::resolve(state, path);
    let mut caret: Option<usize> = None;
    let mut plan = PaintPlan {
        sbs: Vec::with_capacity(model.display.len()),
        unified: Vec::new(),
        hunk_slots: vec![HunkSlot::UNSET; hunk_count],
        hunk_hidden: vec![0; hunk_count],
        hunk_lines: vec![0; hunk_count],
        file_rows: Vec::new(),
        caret: None,
        staging: vec![PairStaging::default(); model.display.len()],
        hunk_staging: vec![HunkStaging::NoAnswer; hunk_count],
    };
    // The section currently being totalled, and the counts of the one before
    // it — a file row opens its section, so the section's totals are only
    // complete when the NEXT file row (or the end of the model) arrives.
    let mut open: Option<FileRowStats> = None;
    let close = |plan: &mut PaintPlan, open: &mut Option<FileRowStats>| {
        if let Some(section) = open.take() {
            plan.file_rows.push(section);
        }
    };
    let collapsed = &state.ui.diff_collapsed;
    for (di, disp) in model.display.iter().enumerate() {
        match disp {
            DisplayRow::Full(row) => match row.kind {
                RowKind::Meta | RowKind::RenameHeader => {
                    if is_file_row(row) {
                        close(&mut plan, &mut open);
                        open = Some(FileRowStats {
                            display: di,
                            added: 0,
                            removed: 0,
                        });
                    }
                    plan.sbs.push(di);
                    plan.unified.push((di, 0));
                }
                RowKind::Hunk => {
                    if row.hunk < hunk_count {
                        // Each stream's OWN length before the header is pushed
                        // is the slot that header is about to occupy — the two
                        // are different numbers and are not interchangeable.
                        plan.hunk_slots[row.hunk] = HunkSlot {
                            sbs: plan.sbs.len(),
                            unified: plan.unified.len(),
                        };
                    }
                    plan.sbs.push(di);
                    plan.unified.push((di, 0));
                }
                RowKind::Context => {
                    if collapsed.contains(&row.hunk) {
                        plan.hunk_hidden[row.hunk] += 1;
                    } else {
                        plan.sbs.push(di);
                        plan.unified.push((di, 0));
                    }
                }
                // Changed rows always live in pairs (build_model invariant).
                RowKind::Del | RowKind::Add => {
                    if let Some(section) = open.as_mut() {
                        if row.kind == RowKind::Add {
                            section.added += 1;
                        } else {
                            section.removed += 1;
                        }
                    }
                }
            },
            DisplayRow::Pair(d, a) => {
                let rows = d.iter().chain(a.iter());
                let hunk = rows.clone().next().map(|r| r.hunk).unwrap_or_default();
                // The caret is the first changed row of the current hunk on the
                // side the active pane is showing. Only a Pair holds a changed
                // row (the model's own invariant), so only a Pair can carry it.
                if caret.is_none()
                    && hunk == state.ui.diff_current_hunk
                    && if want_add { a.is_some() } else { d.is_some() }
                {
                    caret = Some(di);
                }
                if collapsed.contains(&hunk) {
                    plan.hunk_hidden[hunk] += 1;
                } else {
                    plan.sbs.push(di);
                    if d.is_some() {
                        plan.unified.push((di, 1));
                    }
                    if a.is_some() {
                        plan.unified.push((di, 2));
                    }
                }
                plan.hunk_lines[hunk] += rows.count();
                // Staged-ness is answered per row here, once, and both the row's
                // paint and the header's suffix read the result. Collapsed or not,
                // a hunk's state describes the hunk.
                let entry = PairStaging {
                    del: d.as_ref().and_then(|r| {
                        oracle
                            .as_ref()
                            .and_then(|o| o.answer(r.kind, r.old_no, r.new_no))
                    }),
                    add: a.as_ref().and_then(|r| {
                        oracle
                            .as_ref()
                            .and_then(|o| o.answer(r.kind, r.old_no, r.new_no))
                    }),
                };
                plan.staging[di] = entry;
                plan.hunk_staging[hunk] = plan.hunk_staging[hunk].fold(entry.del).fold(entry.add);
                // Collapsed or not, a file's statistics describe the file.
                if let Some(section) = open.as_mut() {
                    if d.is_some() {
                        section.removed += 1;
                    }
                    if a.is_some() {
                        section.added += 1;
                    }
                }
            }
        }
    }
    close(&mut plan, &mut open);
    plan.caret = caret;
    plan
}

/// Issue the hunk scroll request at most once per (diff, hunk): re-issuing
/// the index-based scroll every frame would keep the ScrollArea repainting
/// forever.
fn hunk_needs_scroll(ui: &Ui, paint: egui::Id, idx: usize) -> bool {
    let id = egui::Id::new((paint, "diff_hunk_scrolled", idx));
    let done = ui.ctx().memory(|m| m.data.get_temp::<bool>(id)) == Some(true);
    if !done {
        ui.ctx().memory_mut(|m| m.data.insert_temp(id, true));
    }
    !done
}
// --- rendering ---------------------------------------------------------------

/// Whether this row carries the caret.
///
/// A unified stream shows the two panes as two rows of one pair, so the pane the
/// caret is on is named by the row's own kind: a deletion belongs to the `Before`
/// side, an addition to the `After` one.
fn holds_caret(state: &AppState, plan: &PaintPlan, display: usize, kind: RowKind) -> bool {
    plan.caret == Some(display)
        && match state.ui.diff_pane {
            DiffPane::New => kind == RowKind::Add,
            DiffPane::Old => kind == RowKind::Del,
        }
}

/// x of the end of `text` as painted from `text_origin_x` — where the caret sits.
fn text_end(painter: &egui::Painter, text_origin_x: f32, text: &str, font: &FontId) -> f32 {
    text_origin_x
        + painter
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
}

/// Pane-header labels naming the two sides this read actually diffs.
///
/// A side names itself when the target named it, and falls back to the pinned
/// working-tree pair's own side otherwise, so a commit-to-commit diff prints
/// its own revisions rather than HEAD/Index/Working tree.
fn pane_labels(target: &DiffTarget) -> (String, String) {
    let label = |rev: &Option<String>, working_tree_side: &str| {
        rev.clone().unwrap_or_else(|| working_tree_side.to_owned())
    };
    (
        label(&target.left, "Index"),
        label(&target.right, "Working tree"),
    )
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

// --- char-range drag protocol (issue 19) ---------------------------------------

/// Char offset of the glyph boundary nearest `x`, in row-text coordinates
/// (x relative to the text origin). Clamped into the line.
fn char_at_x(ui: &Ui, text: &str, x: f32) -> usize {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), mono_font(), Color32::WHITE);
    galley.rows.first().map_or(0, |r| r.char_at(x.max(0.0)).0)
}

/// The char-range drag protocol on an addition row (issue 19): the press
/// anchors the range, in-row pointer movement widens it around the anchor, and
/// release stages it. A range that never moved was a plain click, so nothing
/// happens here and the row's own line toggle runs instead.
///
/// The return value is whether this response staged anything. `clicked()` and
/// `drag_stopped()` both come off one `Sense::click_and_drag` response, and a
/// finished drag reports both, so the caller must skip its click when the drag
/// already staged — otherwise one drag would stage the range *and* the whole
/// line.
fn handle_char_drag(
    ui: &mut Ui,
    state: &mut AppState,
    resp: &Response,
    text_origin_x: f32,
    row: &Row,
    path: &Option<std::path::PathBuf>,
) -> bool {
    if resp.drag_started()
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
    {
        let anchor = char_at_x(ui, &row.text, pos.x - text_origin_x);
        granular::begin_char_drag(state, path, row.hunk, row.line_ord, row.new_no, anchor);
    }
    // Not an else: the drag's first frame counts as both started and
    // dragged, and the pointer is already at its moved position there.
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let cur = char_at_x(ui, &row.text, pos.x - text_origin_x);
        granular::extend_char_drag(state, path, row.hunk, row.line_ord, cur);
    }
    if resp.drag_stopped() {
        return granular::finish_char_drag(state, path, row.hunk, row.line_ord);
    }
    false
}

/// Translucent BRAND band over the character range under the pointer
/// (issue 19, screen 06): the live drag, and nothing after it. No-op unless the
/// draft is this exact row.
fn paint_char_highlight(
    painter: &egui::Painter,
    state: &AppState,
    rect: &Rect,
    text_origin_x: f32,
    row: &Row,
    path: &Option<std::path::PathBuf>,
) {
    let Some(p) = path else {
        return;
    };
    let Some(draft) = state
        .ui
        .char_drag
        .clone()
        .filter(|d| d.path == *p && d.hunk == row.hunk && d.ord == row.line_ord)
    else {
        return;
    };
    let (start, end) = draft.range();
    if start >= end {
        return;
    }
    let galley = painter.layout_no_wrap(row.text.clone(), mono_font(), Color32::WHITE);
    let Some(galley_row) = galley.rows.first() else {
        return;
    };
    let len = row.text.chars().count();
    let x0 = text_origin_x + galley_row.x_offset(egui::epaint::text::CharIndex(start.min(len)));
    let x1 = text_origin_x + galley_row.x_offset(egui::epaint::text::CharIndex(end.min(len)));
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(x0, rect.top()),
            Pos2::new(x1.max(x0), rect.bottom()),
        ),
        CornerRadius::same(crate::theme::MARK_RADIUS),
        Palette::BRAND.gamma_multiply(0.35),
    );
}

/// Paint one changed row's highlight, in whatever shape the highlighting policy
/// decided it.
///
/// Three shapes and no more: the whole row, one band per changed run, or nothing.
/// The run list is a property of the display model (`ADR-0014`) — the policy is
/// folded when the diff settles, so this measures glyphs and never re-compares
/// text. Measuring happens for realized rows only, which is exactly the set
/// `show_rows` handed this frame.
fn paint_highlight(
    painter: &egui::Painter,
    rect: &Rect,
    text: &str,
    text_origin_x: f32,
    highlight: &Highlight,
    bg: Color32,
    font: &FontId,
) {
    match highlight {
        Highlight::Row => {
            painter.rect_filled(*rect, CornerRadius::ZERO, bg);
        }
        Highlight::Runs(runs) => {
            let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE);
            let Some(row) = galley.rows.first() else {
                return;
            };
            let len = text.chars().count();
            for run in runs {
                let x0 =
                    text_origin_x + row.x_offset(egui::epaint::text::CharIndex(run.start.min(len)));
                let x1 =
                    text_origin_x + row.x_offset(egui::epaint::text::CharIndex(run.end.min(len)));
                if x1 <= x0 {
                    continue;
                }
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(x0, rect.top()), Pos2::new(x1, rect.bottom())),
                    CornerRadius::same(crate::theme::MARK_RADIUS),
                    bg,
                );
            }
        }
        Highlight::Off => {}
    }
}

/// The caret bar marking which pane of the diff holds the reader's position
/// (`Diff.FocusOppositePane`). It sits inside the text, at the caret line's last
/// glyph — not on the row's leading edge, which is the selection rail's place, and
/// the two are different tokens for that reason (`theme::CARET_WIDTH`).
fn paint_caret(painter: &egui::Painter, rect: &Rect, x: f32) {
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(x, rect.top() + 2.0),
            Pos2::new(x + crate::theme::CARET_WIDTH, rect.bottom() - 2.0),
        ),
        CornerRadius::ZERO,
        Palette::ACCENT,
    );
}

/// Unified-mode sign + line-number gutter cells. The sign wears the row's own
/// ink, which is now the staged-ness role's, and the number wears the gutter's.
fn paint_gutter(
    painter: &egui::Painter,
    rect: &Rect,
    sign: &str,
    num: usize,
    sign_color: Color32,
    num_color: Color32,
    font: &FontId,
) {
    paint_cell(painter, rect.left() + 4.0, rect, sign, sign_color, font);
    let num_text = num.to_string();
    let galley = painter.layout_no_wrap(num_text, font.clone(), num_color);
    let x = rect.left() + SIGN_W + NUM_W - galley.size().x - 6.0;
    painter.galley(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        num_color,
    );
}
/// Unified mode (spec §8.4): one full-width band per underlying row, paging
/// only the visible window of the filtered slot stream (ADR-0014, issue 20).
/// Pair members arrive flattened as their own slots, so output is
/// pixel-identical to the pre-virtualization loop.
#[allow(clippy::too_many_arguments)]
fn render_unified(
    ui: &mut Ui,
    state: &mut AppState,
    model: &DiffModel,
    plan: &PaintPlan,
    visible: Range<usize>,
    paint: egui::Id,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
) {
    let width = ui.available_width();
    let painter = ui.painter().clone();
    let font = mono_font();

    let end = visible.end.min(plan.unified.len());
    let start = visible.start.min(end);
    if start < end {
        // Hover tracking (spec R2): which hunk sits under the pointer this
        // frame — visible rows only, which is correct under virtualization.
        let mut frame_hover: Option<usize> = None;
        let mut rows_rect: Option<Rect> = None;
        for &(di, member) in &plan.unified[start..end] {
            match (&model.display[di], member) {
                (DisplayRow::Full(row), 0) => {
                    unified_row(
                        ui,
                        state,
                        di,
                        row,
                        width,
                        &painter,
                        &font,
                        paint,
                        status,
                        path,
                        plan,
                        &mut frame_hover,
                        &mut rows_rect,
                    );
                }
                (DisplayRow::Pair(Some(row), _), 1) => {
                    unified_row(
                        ui,
                        state,
                        di,
                        row,
                        width,
                        &painter,
                        &font,
                        paint,
                        status,
                        path,
                        plan,
                        &mut frame_hover,
                        &mut rows_rect,
                    );
                }
                (DisplayRow::Pair(_, Some(row)), 2) => {
                    unified_row(
                        ui,
                        state,
                        di,
                        row,
                        width,
                        &painter,
                        &font,
                        paint,
                        status,
                        path,
                        plan,
                        &mut frame_hover,
                        &mut rows_rect,
                    );
                }

                _ => {}
            }
        }
        commit_current_hunk(state, ui, rows_rect, frame_hover);
    }
}

/// Paint the full-width display row of one kind: the file row, the rename
/// header, and the hunk band. Both render modes paint these three identically,
/// so the paint is stated once here and a fourth full-width kind is one arm
/// below rather than an edit in two loops.
///
/// The caller owns the row shell and its per-mode bookkeeping — the unified
/// loop's hover plumbing, the side-by-side loop's `rows_rect` union and
/// `frame_hover` capture — and passes the `rect` it allocated plus the row's
/// `display` index. Returns whether the kind was one of the three; `false`
/// leaves the changed rows (`Context` / `Del` / `Add`) to the mode's own paint.
#[allow(clippy::too_many_arguments)]
fn paint_full_width_row(
    ui: &mut Ui,
    state: &mut AppState,
    rect: &Rect,
    row: &Row,
    display: usize,
    painter: &egui::Painter,
    font: &FontId,
    paint: egui::Id,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
    plan: &PaintPlan,
    hovered: bool,
) -> bool {
    match row.kind {
        RowKind::Meta if is_file_row(row) => file_row(
            ui,
            state,
            rect,
            &row.text,
            plan.file_counts(display),
            font,
            hovered,
        ),
        RowKind::Meta => paint_cell(
            painter,
            rect.left() + TEXT_X,
            rect,
            &row.text,
            Palette::INK_3,
            font,
        ),
        RowKind::RenameHeader => paint_rename_header(painter, rect, &row.text, font),
        RowKind::Hunk => {
            painter.rect_filled(*rect, CornerRadius::ZERO, Palette::SURFACE);
            // The header is the `@@` line git itself printed, painted as its own
            // galley, and the staged-ness suffix is a **second** galley beside it.
            // Two rather than one string because the suite that aims a hunk compares
            // the painted header against `git diff`'s own line: fold the suffix in
            // and the pane claims git printed words it did not.
            let header = painter.layout_no_wrap(row.text.clone(), font.clone(), Palette::INK_3);
            let header_x = rect.left() + TEXT_X;
            painter.galley(
                Pos2::new(header_x, rect.center().y - header.size().y / 2.0),
                header.clone(),
                Palette::INK_3,
            );
            if let Some(suffix) = plan.hunk_suffix(row.hunk) {
                paint_cell(
                    painter,
                    header_x + header.size().x + 8.0,
                    rect,
                    suffix,
                    Palette::INK_3,
                    font,
                );
            }
            hunk_gutter_actions(ui, state, *rect, paint, row.hunk, status, path);
            hunk_header_extras(
                ui,
                state,
                *rect,
                paint,
                row.hunk,
                plan.hunk_lines.get(row.hunk).copied().unwrap_or(0),
                state.ui.diff_collapsed.contains(&row.hunk),
                plan.hunk_hidden.get(row.hunk).copied().unwrap_or(0),
            );
        }
        RowKind::Context | RowKind::Del | RowKind::Add => return false,
    }
    true
}

/// One unified band: allocation, hover tracking, click toggle, and the
/// token-exact paint for a single underlying row (spec §8.4/R2). Hunk
/// scrolling is handled centrally by the [`ScrollArea::show_rows`] caller.
#[allow(clippy::too_many_arguments)]
fn unified_row(
    ui: &mut Ui,
    state: &mut AppState,
    display: usize,
    row: &Row,
    width: f32,
    painter: &egui::Painter,
    font: &FontId,
    paint: egui::Id,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
    plan: &PaintPlan,
    frame_hover: &mut Option<usize>,
    rows_rect: &mut Option<Rect>,
) {
    // Changed lines are clickable toggles (spec R2 story 3, issue 19);
    // additions additionally take char-range drags. Everything else stays
    // hover-only.
    let toggleable = matches!(row.kind, RowKind::Del | RowKind::Add);
    let draggable = toggleable && row.kind == RowKind::Add;
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, ROW_H),
        if draggable {
            Sense::click_and_drag()
        } else if toggleable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    *rows_rect = Some(match *rows_rect {
        Some(union) => union.union(rect),
        None => rect,
    });
    if !matches!(row.kind, RowKind::Meta | RowKind::RenameHeader) && resp.hovered() {
        *frame_hover = Some(row.hunk);
    }
    if toggleable {
        // Accessibility: the row is labeled by its content text so tooling can
        // target individual changed lines, and by the state the paint is showing so
        // the colour has a word beside it.
        let label = row_label(&row.text, plan.staged(display, row.kind));
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label.as_str()));
        let drag_staged = if draggable {
            handle_char_drag(ui, state, &resp, rect.left() + TEXT_X, row, path)
        } else {
            false
        };
        if resp.clicked() && !drag_staged {
            granular::toggle_row(state, path, row);
        }
    }
    if paint_full_width_row(
        ui,
        state,
        &rect,
        row,
        display,
        painter,
        font,
        paint,
        status,
        path,
        plan,
        resp.hovered(),
    ) {
        return;
    }
    match row.kind {
        RowKind::Context => {
            paint_gutter(
                painter,
                &rect,
                " ",
                row.new_no,
                Palette::INK_3,
                Palette::INK_3,
                font,
            );
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
            let caret = holds_caret(state, plan, display, RowKind::Del);
            let (band, ink) = row_treatment(RowKind::Del, plan.staged(display, RowKind::Del));
            paint_highlight(
                painter,
                &rect,
                &row.text,
                rect.left() + TEXT_X,
                &row.highlight,
                band,
                font,
            );
            paint_gutter(
                painter,
                &rect,
                "-",
                row.old_no,
                ink,
                gutter_ink(resp.hovered()),
                font,
            );
            let text_origin = rect.left() + TEXT_X;
            paint_cell(painter, text_origin, &rect, &row.text, ink, font);
            if caret {
                paint_caret(
                    painter,
                    &rect,
                    text_end(painter, text_origin, &row.text, font),
                );
            }
        }
        RowKind::Add => {
            let caret = holds_caret(state, plan, display, RowKind::Add);
            let (band, ink) = row_treatment(RowKind::Add, plan.staged(display, RowKind::Add));
            paint_highlight(
                painter,
                &rect,
                &row.text,
                rect.left() + TEXT_X,
                &row.highlight,
                band,
                font,
            );
            paint_gutter(
                painter,
                &rect,
                "+",
                row.new_no,
                ink,
                gutter_ink(resp.hovered()),
                font,
            );
            let text_origin = rect.left() + TEXT_X;
            paint_cell(painter, text_origin, &rect, &row.text, ink, font);
            paint_char_highlight(painter, state, &rect, text_origin, row, path);
            if caret {
                paint_caret(
                    painter,
                    &rect,
                    text_end(painter, text_origin, &row.text, font),
                );
            }
        }
        _ => {}
    }
}
/// Side-by-side pane header band (SURFACE, spec §8.4).
fn header_band(ui: &mut Ui, width: f32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, PANE_HEADER_H), Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(crate::theme::CONTROL_RADIUS),
        Palette::SURFACE,
    );
    widgets::paint_centered_text(
        ui.painter(),
        rect,
        label,
        crate::theme::chrome_font(crate::theme::TYPE_BODY),
        Palette::INK,
    );
}

/// The file row (spec story 23): a file section's own opening line, carrying
/// the statistics that describe that file and the comparison toggle that drives
/// it.
///
/// **The geometry here is the row shell's, not this function's.** The band is
/// the `ROW_H` band the caller allocated and the label sits at the shell's
/// [`TEXT_X`], exactly where every other row in this stream puts its text — the
/// two new occupants are placed *from that one rect*, right-aligned inside it,
/// and the row's own fill is the one row-fill decision
/// ([`widgets::paint_row`]). A file row is therefore a row like any other, and
/// gaining two occupants cannot give it a second selection treatment, a second
/// height, or a second text origin.
fn file_row(
    ui: &mut Ui,
    state: &mut AppState,
    rect: &Rect,
    label: &str,
    (added, removed): (usize, usize),
    font: &FontId,
    hovered: bool,
) {
    widgets::paint_row(ui, *rect, components::RowState::from_flags(false, hovered));
    paint_cell(
        ui.painter(),
        rect.left() + TEXT_X,
        rect,
        label,
        Palette::INK_3,
        font,
    );
    // The trailing zone, hung off the row shell's rect: the change counts and
    // then the toggle, right-to-left so the toggle is the one control that
    // always sits at the row's trailing edge. It is a child `Ui` over the
    // already-allocated rect, so nothing here can change the row's height.
    let trailing = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right() - crate::theme::BUTTON_PADDING.x, rect.bottom()),
    );
    let mut zone = ui.new_child(
        UiBuilder::new()
            .max_rect(trailing)
            .layout(Layout::right_to_left(Align::Center)),
    );
    // The diff viewer's settings menu (`⌃⇧D`) takes the trailing slot. A lone
    // "Ignore whitespace" checkbox there would ask one of the ten questions
    // answered here, and ask it of `git` rather than of the view. `⌃⇧D` opens this
    // from the keyboard by arming the flag the shell sets.
    let trigger = zone.button(
        RichText::new("Diff options").font(crate::theme::chrome_font(crate::theme::TYPE_BODY)),
    );
    let mut popup = egui::Popup::menu(&trigger);
    if state.ui.diff_options_open {
        popup = popup.open(true);
    }
    let mut policies_changed = false;
    match popup.show(|menu| diff_options_menu(menu, state)) {
        // The popup closed itself — by a click outside or by `Escape` — so the
        // keyboard route's arm goes with it, and `⌃⇧D` toggles cleanly next time.
        None => state.ui.diff_options_open = false,
        Some(shown) => policies_changed = shown.inner,
    }
    if policies_changed {
        // A policy is presentation the reader agrees on, and the agreement has to
        // survive the session it was picked in.
        state.save_durable_ui();
    }
    // The same `+N −M` stats treatment the commit window's preview header uses,
    // in the same accents: the counts are statistics, not diff row content.
    let stat_font = crate::theme::chrome_font(crate::theme::TYPE_BODY);
    zone.label(
        RichText::new(format!("\u{2212}{removed}"))
            .font(stat_font.clone())
            .color(Palette::DIFF_DEL_ACCENT),
    );
    zone.label(
        RichText::new(format!("+{added}"))
            .font(stat_font)
            .color(Palette::DIFF_ADD_ACCENT),
    );
}

/// The pane's header for one file, painted in every state of the comparison —
/// settled or not. It is the file row with a label that names the file even
/// when there is no `diff --git` line to show, and `+0 −0` for a comparison
/// with nothing in it, because the toggle it carries has to be reachable in every
/// state the comparison can be in.
fn file_header(ui: &mut Ui, state: &mut AppState, path: Option<&std::path::Path>) {
    let Some(path) = path else {
        return;
    };
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
    let label = path.display_name_lossy_or(&path.display().to_string());
    file_row(ui, state, &rect, &label, (0, 0), &mono_font(), false);
}

/// Paint the rename-header row as **one muted band** across the whole row —
/// shared by the in-scroller display row and the pure-rename static band
/// (spec R8).
///
/// The band is [`Palette::SECTION_BG`], the app's group-label band: a header is
/// scaffolding, and a header that is only a line of text reads as one more row
/// of context. It is painted with square corners like the hunk-header band
/// above it, and the text keeps the ink it always had — a band is added, no
/// diff text token moves.
fn paint_rename_header(painter: &egui::Painter, rect: &Rect, text: &str, font: &FontId) {
    painter.rect_filled(*rect, CornerRadius::ZERO, Palette::SECTION_BG);
    paint_cell(
        painter,
        rect.left() + TEXT_X,
        rect,
        text,
        Palette::INK_2,
        font,
    );
}
/// One side-by-side cell: the highlight the policy chose for it, a gutter number,
/// sign marker, and code text. A changed cell is a clickable line toggle (spec R2
/// story 3); the addition cell also takes a char-range drag. `selected` paints the
/// BRAND edge bar and `caret` the pane's caret bar. `bg` and `text_color` are the
/// cell's own staged-ness role, chosen by the caller, and `staged` is the oracle's
/// answer the accessibility label states. Returns the cell's response so the row
/// loop can track hover and toggle clicks.
#[allow(clippy::too_many_arguments)]
fn cell_band(
    ui: &mut Ui,
    width: f32,
    highlight: &Highlight,
    bg: Color32,
    caret: bool,
    num: usize,
    sign: char,
    text: &str,
    text_color: Color32,
    staged: Option<bool>,
    painter: &egui::Painter,
    font: &FontId,
    toggleable: bool,
    draggable: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, ROW_H),
        if toggleable && draggable {
            Sense::click_and_drag()
        } else if toggleable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let text_origin = rect.left() + NUM_W + 20.0;
    paint_highlight(painter, &rect, text, text_origin, highlight, bg, font);
    if toggleable {
        let label = row_label(text, staged);
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label.as_str()));
    }
    if num > 0 {
        let num_color = if toggleable {
            gutter_ink(resp.hovered())
        } else {
            Palette::INK_3
        };
        let galley = painter.layout_no_wrap(num.to_string(), font.clone(), num_color);
        painter.galley(
            Pos2::new(
                rect.left() + NUM_W - galley.size().x - 6.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            num_color,
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
    paint_cell(painter, text_origin, &rect, text, text_color, font);
    if caret {
        paint_caret(painter, &rect, text_end(painter, text_origin, text, font));
    }
    resp
}

/// Side-by-side mode (spec §8.4): paired Del/Add bands plus full-width
/// context/hunk/meta rows, paging only the visible window of the filtered
/// slot stream (ADR-0014, issue 20). The pane header band is pinned by the
/// caller. Hover tracking and toggles apply to visible rows only — correct
/// under virtualization.
#[allow(clippy::too_many_arguments)]
fn render_side_by_side(
    ui: &mut Ui,
    state: &mut AppState,
    model: &DiffModel,
    plan: &PaintPlan,
    visible: Range<usize>,
    paint: egui::Id,
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

    let end = visible.end.min(plan.sbs.len());
    let start = visible.start.min(end);
    for &di in &plan.sbs[start..end] {
        match &model.display[di] {
            DisplayRow::Full(row) => match row.kind {
                // The three full-width kinds share this row shell; the painter
                // below states their paint, so its "handled" answer is true here.
                RowKind::Meta | RowKind::RenameHeader | RowKind::Hunk => {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                    rows_rect = Some(match rows_rect {
                        Some(union) => union.union(rect),
                        None => rect,
                    });
                    if row.kind == RowKind::Hunk && resp.hovered() {
                        frame_hover = Some(row.hunk);
                    }
                    paint_full_width_row(
                        ui,
                        state,
                        &rect,
                        row,
                        di,
                        &painter,
                        &font,
                        paint,
                        status,
                        path,
                        plan,
                        resp.hovered(),
                    );
                }
                RowKind::Context => {
                    let hunk = row.hunk;
                    ui.horizontal(|ui| {
                        let old_cell = cell_band(
                            ui,
                            half,
                            &Highlight::Off,
                            Palette::DIFF_DEL_BG,
                            false,
                            row.old_no,
                            ' ',
                            &row.text,
                            Palette::INK,
                            None,
                            &painter,
                            &font,
                            false,
                            false,
                        );
                        let new_cell = cell_band(
                            ui,
                            half,
                            &Highlight::Off,
                            Palette::DIFF_ADD_BG,
                            false,
                            row.new_no,
                            ' ',
                            &row.text,
                            Palette::INK,
                            None,
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
                let caret_row = plan.caret == Some(di);
                // **Both cells answer for themselves.** A side-by-side pair is one
                // line's two halves, and in a combined view a replacement can be
                // staged on one side of the index and not the other (a staged
                // modification re-edited in the worktree answers its addition
                // unstaged and its deletion the other way). So each of the two cells
                // wears the treatment its own answer names, and neither borrows its
                // neighbour's.
                let del_staged = plan.staged(di, RowKind::Del);
                let add_staged = plan.staged(di, RowKind::Add);
                let (del_band, del_ink) = row_treatment(RowKind::Del, del_staged);
                let (add_band, add_ink) = row_treatment(RowKind::Add, add_staged);
                ui.horizontal(|ui| {
                    let del_cell = if let Some(d) = d {
                        let cell = cell_band(
                            ui,
                            half,
                            &d.highlight,
                            del_band,
                            caret_row && matches!(state.ui.diff_pane, DiffPane::Old),
                            d.old_no,
                            '-',
                            &d.text,
                            del_ink,
                            del_staged,
                            &painter,
                            &font,
                            true,
                            false,
                        );
                        if cell.clicked() {
                            granular::toggle_row(state, path, d);
                        }
                        cell
                    } else {
                        cell_band(
                            ui,
                            half,
                            &Highlight::Off,
                            Palette::DIFF_DEL_BG,
                            false,
                            0,
                            ' ',
                            "",
                            Palette::INK,
                            None,
                            &painter,
                            &font,
                            false,
                            false,
                        )
                    };
                    let add_cell = if let Some(a) = a {
                        let cell = cell_band(
                            ui,
                            half,
                            &a.highlight,
                            add_band,
                            caret_row && matches!(state.ui.diff_pane, DiffPane::New),
                            a.new_no,
                            '+',
                            &a.text,
                            add_ink,
                            add_staged,
                            &painter,
                            &font,
                            true,
                            true,
                        );
                        let text_origin = cell.rect.left() + NUM_W + 20.0;
                        let drag_staged = handle_char_drag(ui, state, &cell, text_origin, a, path);
                        if cell.clicked() && !drag_staged {
                            granular::toggle_row(state, path, a);
                        }
                        paint_char_highlight(&painter, state, &cell.rect, text_origin, a, path);
                        cell
                    } else {
                        cell_band(
                            ui,
                            half,
                            &Highlight::Off,
                            Palette::DIFF_ADD_BG,
                            false,
                            0,
                            ' ',
                            "",
                            Palette::INK,
                            None,
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
