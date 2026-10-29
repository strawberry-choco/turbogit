use super::actions::{
    commit_current_hunk, diff_options_button, hunk_gutter_actions, hunk_header_extras, hunk_nav,
    hunk_nav_width,
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
use turbogit_domain::model::{ChangeRef, ChangeStatus, DisplayName, RootId};

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
    fn resolve(state: &'a AppState, change: &Option<ChangeRef>) -> Option<Self> {
        let change = change.as_ref()?;
        let root = state.multi.by_id(&change.root)?;
        let entry = root.resolve_change(&change.path)?;
        let stats = state.caches.hunk_stats(&root.id)?;
        Some(Self {
            path: &entry.path,
            sets: &stats.line_sets,
        })
    }

    fn answer(&self, kind: RowKind, old_no: usize, new_no: usize) -> Option<bool> {
        staged_lines::line_is_staged(self.sets, self.path, kind, old_no, new_no)
    }
}

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum HunkStaging {
    Staged,
    Unstaged,
    Mixed,
    NoAnswer,
}

impl HunkStaging {
    fn fold(self, answer: Option<bool>) -> Self {
        match (self, answer) {
            (s, None) => s,
            (Self::NoAnswer, Some(true)) => Self::Staged,
            (Self::NoAnswer, Some(false)) => Self::Unstaged,
            (Self::Staged, Some(false)) | (Self::Unstaged, Some(true)) => Self::Mixed,
            (s, _) => s,
        }
    }

    fn suffix(self) -> Option<&'static str> {
        match self {
            Self::Staged => Some("· staged"),
            Self::Mixed => Some("· part"),
            Self::Unstaged => Some("· not staged"),
            Self::NoAnswer => None,
        }
    }
}

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

fn row_label(text: &str, staged: Option<bool>) -> String {
    match staged {
        Some(true) => format!("{text} (staged)"),
        Some(false) => format!("{text} (unstaged)"),
        None => format!("{text} (staged-ness unknown)"),
    }
}

fn gutter_ink(hovered: bool) -> Color32 {
    if hovered {
        Palette::INK_2
    } else {
        Palette::INK_3
    }
}

pub fn show_diff(ui: &mut Ui, state: &mut AppState) {
    let Some(target) = state.ui.diff.clone() else {
        return;
    };
    let mut open = true;
    egui::Window::new("Diff")
        .open(&mut open)
        .vscroll(false)
        .default_size(egui::vec2(920.0, 620.0))
        .show(ui.ctx(), |ui| diff_body(ui, state, &target));
    if !open {
        state.ui.diff = None;
    }
}

fn diff_body(ui: &mut Ui, state: &mut AppState, target: &turbogit_app::state::DiffTarget) {
    ui.label(
        RichText::new(subject_line(target))
            .font(FontId::new(
                crate::theme::TYPE_CONTROL,
                egui::FontFamily::Proportional,
            ))
            .color(Palette::BRAND),
    );
    let change = target
        .path
        .clone()
        .map(|path| ChangeRef::new(target.root.clone(), path));
    render_diff(
        ui,
        state,
        &target.root,
        &target.left,
        &target.right,
        &change,
    );
    ui.add_space(6.0);
    widgets::dialog_footer(ui, |ui| {
        if widgets::compact_button(ui, "Close").clicked() {
            state.ui.diff = None;
        }
    });
}

fn subject_line(target: &turbogit_app::state::DiffTarget) -> String {
    match &target.path {
        Some(path) => format!(
            "{} @ {}",
            path.display(),
            widgets::short_commit_ref(target.right.as_deref().unwrap_or_default()),
        ),
        None => match (target.left.as_deref(), target.right.as_deref()) {
            (Some(left), Some(right)) => format!(
                "{}..{}",
                widgets::short_commit_ref(left),
                widgets::short_commit_ref(right),
            ),
            (_, Some(right)) => widgets::short_commit_ref(right),
            _ => String::new(),
        },
    }
}

fn toolbar_counts_width(ui: &Ui, plan: Option<&PaintPlan>) -> f32 {
    let Some((added, removed)) = plan.and_then(PaintPlan::toolbar_counts) else {
        return 0.0;
    };
    let font = crate::theme::chrome_font(crate::theme::TYPE_BODY);
    let stat = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
    };
    let gap = ui.spacing().item_spacing.x;
    stat(&format!("+{added}")) + gap + stat(&format!("\u{2212}{removed}")) + gap
}

pub fn render_diff(
    ui: &mut Ui,
    state: &mut AppState,
    root: &RootId,
    left: &Option<String>,
    right: &Option<String>,
    change: &Option<ChangeRef>,
) {
    let path = change.as_ref().map(|change| change.path.clone());

    let target = match (left, right) {
        (None, None) => {
            DiffTarget::working_tree(root.as_path().to_path_buf(), state.ui.diff_policies, path)
        }
        _ => DiffTarget::new(
            root.as_path().to_path_buf(),
            left.clone(),
            right.clone(),
            state.ui.diff_policies,
            path,
        ),
    };
    let verdict = state.read(target.clone());
    let staging_live = target.stages();
    let view_failure = match &verdict {
        Read::Fresh(_) | Read::Empty => state.failed_view_message(&target),
        _ => None,
    };
    let total_hunks = match &verdict {
        Read::Fresh(diff) if view_failure.is_none() => diff.model.hunk_count(),
        _ => 0,
    };

    let plan = match &verdict {
        Read::Fresh(diff)
            if view_failure.is_none() && pane_kind(&diff.model.files) == PaneKind::Text =>
        {
            Some(paint_plan(state, &diff.model, change, staging_live))
        }
        _ => None,
    };

    ui.horizontal_wrapped(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let selected = if state.ui.diff_side_by_side { 0 } else { 1 };
        if let Some(idx) = widgets::segmented_control(ui, &["Side-by-Side", "Unified"], selected) {
            state.ui.diff_side_by_side = idx == 0;
        }
        ui.add_space(gap);
        diff_options_button(ui, state);

        // The right-hand group is right-aligned by ONE spacer sized from the
        // row's remaining width. `available_width` on a wrapping layout
        // answers the row's FULL width, so the remaining width on THIS row
        // comes from the before-wrap rect; a row too short for the group ends
        // here, and the group paints at the next row's leading edge. The
        // group itself paints in a non-wrapped scope because a wrapped-layout
        // label lays its galley out from the ROW's left edge with a leading
        // space, a statistic that never wraps wants its own cursor position,
        // not the row's.
        let right_w = toolbar_counts_width(ui, plan.as_ref())
            + hunk_nav_width(ui, state.ui.diff_current_hunk, total_hunks);
        let remaining = ui.available_size_before_wrap().x;
        if remaining >= right_w {
            ui.add_space(remaining - right_w);
        } else {
            ui.end_row();
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            if let Some((added, removed)) = plan.as_ref().and_then(PaintPlan::toolbar_counts) {
                let stat_font = crate::theme::chrome_font(crate::theme::TYPE_BODY);
                ui.label(
                    RichText::new(format!("+{added}"))
                        .font(stat_font.clone())
                        .color(Palette::DIFF_ADD_ACCENT),
                );
                ui.label(
                    RichText::new(format!("\u{2212}{removed}"))
                        .font(stat_font)
                        .color(Palette::DIFF_DEL_ACCENT),
                );
            }
            hunk_nav(ui, state, total_hunks);
        });
    });

    if let Some(message) = &view_failure {
        file_header(ui, change);
        widgets::keyed_read_presentation(
            ui,
            widgets::KeyedReadPresentation::Failed(message.as_str()),
        );
        return;
    }
    let diff = match verdict {
        Read::Fresh(diff) => diff,
        Read::Empty => {
            file_header(ui, change);
            ui.label("(no differences)");
            return;
        }
        Read::Waiting => {
            file_header(ui, change);
            widgets::keyed_read_presentation(
                ui,
                widgets::KeyedReadPresentation::Waiting("Computing diff…"),
            );
            return;
        }
        Read::Failed(message) => {
            file_header(ui, change);
            widgets::keyed_read_presentation(ui, widgets::KeyedReadPresentation::Failed(&message));
            return;
        }
    };
    let model = &diff.model;

    match pane_kind(&model.files) {
        PaneKind::Text => {}
        PaneKind::Binary => {
            let pane = PaneTarget::new(
                target.clone(),
                PaneFlavour::Binary,
                model.files[0].clone(),
                false,
            );
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

    if model.pure_rename() {
        if let Some(text) = &model.rename_header {
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
            paint_rename_header(ui.painter(), &rect, text, &mono_font());
        }
        widgets::empty_state(ui, "No content changes.");
        return;
    }

    if model.display.is_empty() {
        widgets::empty_state(ui, "No content changes.");
        return;
    }

    let status = granular::preview_status(state, change.as_ref());

    let side_by_side = state.ui.diff_side_by_side;
    let plan = plan.expect("a settled text diff computed the paint plan");
    let total_rows = if side_by_side {
        plan.sbs.len()
    } else {
        plan.unified.len()
    };

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

    let paint = egui::Id::new(("diff-view", &target));

    // Named id salt: the commit window's `ui.columns` panes share one stable
    // child id, and egui's default ScrollArea salt is constant, an unnamed
    // area here would share persisted scrollbar state with the changelist
    // pane's area and flip-flop visibility (a zero-delay repaint loop).
    ScrollArea::vertical().id_salt("diff_viewer").show_rows(
        ui,
        ROW_H,
        total_rows,
        |ui, visible| {
            // Hunk navigation (ADR-0014): index-based, aim the aimed hunk's
            // header slot at the viewport center instead of relying on
            // a realized widget (`resp.scroll_to_me` cannot reach rows this
            // window didn't build). Issued inside the closure because egui
            // consumes scroll targets set by an area's content; ones set
            // before the area begins are stashed for outer areas. At most
            // once per (diff, hunk), re-issuing every frame would keep the
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
            if side_by_side {
                render_side_by_side(ui, state, model, &plan, visible, paint, status, change);
            } else {
                render_unified(ui, state, model, &plan, visible, paint, status, change);
            }
        },
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HunkSlot {
    sbs: usize,
    unified: usize,
}

impl HunkSlot {
    const UNSET: Self = Self { sbs: 0, unified: 0 };

    fn slot(&self, side_by_side: bool) -> usize {
        if side_by_side { self.sbs } else { self.unified }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileRowStats {
    display: Option<usize>,
    added: usize,
    removed: usize,
}

struct PaintPlan {
    sbs: Vec<usize>,
    unified: Vec<(usize, u8)>,
    hunk_slots: Vec<HunkSlot>,
    hunk_hidden: Vec<usize>,
    hunk_lines: Vec<usize>,
    file_rows: Vec<FileRowStats>,
    caret: Option<usize>,
    staging: Vec<PairStaging>,
    hunk_staging: Vec<HunkStaging>,
    staging_live: bool,
}

impl PaintPlan {
    fn file_counts(&self, display: usize) -> (usize, usize) {
        self.file_rows
            .iter()
            .find(|f| f.display == Some(display))
            .map_or((0, 0), |f| (f.added, f.removed))
    }

    fn toolbar_counts(&self) -> Option<(usize, usize)> {
        match &self.file_rows[..] {
            [only] => Some((only.added, only.removed)),
            _ => None,
        }
    }

    fn staged(&self, display: usize, kind: RowKind) -> Option<bool> {
        self.staging
            .get(display)
            .and_then(|entry| entry.for_kind(kind))
    }

    fn hunk_suffix(&self, hunk: usize) -> Option<&'static str> {
        self.hunk_staging.get(hunk).and_then(|s| s.suffix())
    }
}

fn paint_plan(
    state: &AppState,
    model: &DiffModel,
    change: &Option<ChangeRef>,
    staging_live: bool,
) -> PaintPlan {
    let hunk_count = model.hunk_count();
    let want_add = matches!(state.ui.diff_pane, DiffPane::New);
    let oracle = if staging_live {
        StagedRows::resolve(state, change)
    } else {
        None
    };
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
        staging_live,
    };
    let mut open: Option<FileRowStats> = None;
    if model.files.len() == 1 {
        open = Some(FileRowStats {
            display: None,
            added: 0,
            removed: 0,
        });
    }
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
                            display: Some(di),
                            added: 0,
                            removed: 0,
                        });
                    }
                    plan.sbs.push(di);
                    plan.unified.push((di, 0));
                }
                RowKind::Hunk => {
                    if row.hunk < hunk_count {
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

fn hunk_needs_scroll(ui: &Ui, paint: egui::Id, idx: usize) -> bool {
    let id = egui::Id::new((paint, "diff_hunk_scrolled", idx));
    let done = ui.ctx().memory(|m| m.data.get_temp::<bool>(id)) == Some(true);
    if !done {
        ui.ctx().memory_mut(|m| m.data.insert_temp(id, true));
    }
    !done
}

fn holds_caret(state: &AppState, plan: &PaintPlan, display: usize, kind: RowKind) -> bool {
    plan.caret == Some(display)
        && match state.ui.diff_pane {
            DiffPane::New => kind == RowKind::Add,
            DiffPane::Old => kind == RowKind::Del,
        }
}

fn text_end(painter: &egui::Painter, text_origin_x: f32, text: &str, font: &FontId) -> f32 {
    text_origin_x
        + painter
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
}

fn pane_labels(target: &DiffTarget) -> (String, String) {
    let label = |rev: &Option<String>, working_tree_side: &str| {
        rev.clone().unwrap_or_else(|| working_tree_side.to_owned())
    };
    (
        label(&target.left, "Index"),
        label(&target.right, "Working tree"),
    )
}

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
/// already staged, otherwise one drag would stage the range *and* the whole
/// line.
fn handle_char_drag(
    ui: &mut Ui,
    state: &mut AppState,
    resp: &Response,
    text_origin_x: f32,
    row: &Row,
    change: &Option<ChangeRef>,
) -> bool {
    if resp.drag_started()
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
    {
        let anchor = char_at_x(ui, &row.text, pos.x - text_origin_x);
        granular::begin_char_drag(state, change, row.hunk, row.line_ord, row.new_no, anchor);
    }
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let cur = char_at_x(ui, &row.text, pos.x - text_origin_x);
        granular::extend_char_drag(state, change, row.hunk, row.line_ord, cur);
    }
    if resp.drag_stopped() {
        return granular::finish_char_drag(state, change, row.hunk, row.line_ord);
    }
    false
}

fn paint_char_highlight(
    painter: &egui::Painter,
    state: &AppState,
    rect: &Rect,
    text_origin_x: f32,
    row: &Row,
    change: &Option<ChangeRef>,
) {
    let Some(c) = change else {
        return;
    };
    let Some(draft) = state
        .ui
        .char_drag
        .clone()
        .filter(|d| d.change == *c && d.hunk == row.hunk && d.ord == row.line_ord)
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
#[allow(clippy::too_many_arguments)]
fn render_unified(
    ui: &mut Ui,
    state: &mut AppState,
    model: &DiffModel,
    plan: &PaintPlan,
    visible: Range<usize>,
    paint: egui::Id,
    status: ChangeStatus,
    change: &Option<ChangeRef>,
) {
    let width = ui.available_width();
    let painter = ui.painter().clone();
    let font = mono_font();

    let end = visible.end.min(plan.unified.len());
    let start = visible.start.min(end);
    if start < end {
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
                        change,
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
                        change,
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
                        change,
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
    change: &Option<ChangeRef>,
    plan: &PaintPlan,
    hovered: bool,
) -> bool {
    match row.kind {
        RowKind::Meta if is_file_row(row) => file_row(
            ui,
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
            if plan.staging_live {
                hunk_gutter_actions(ui, state, *rect, paint, row.hunk, status, change);
            }
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
    change: &Option<ChangeRef>,
    plan: &PaintPlan,
    frame_hover: &mut Option<usize>,
    rows_rect: &mut Option<Rect>,
) {
    let toggleable = plan.staging_live && matches!(row.kind, RowKind::Del | RowKind::Add);
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
        let label = row_label(&row.text, plan.staged(display, row.kind));
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label.as_str()));
        let drag_staged = if draggable {
            handle_char_drag(ui, state, &resp, rect.left() + TEXT_X, row, change)
        } else {
            false
        };
        if resp.clicked() && !drag_staged {
            granular::toggle_row(state, change, row);
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
        change,
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
            paint_char_highlight(painter, state, &rect, text_origin, row, change);
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

fn file_row(
    ui: &mut Ui,
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
    let trailing = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right() - crate::theme::BUTTON_PADDING.x, rect.bottom()),
    );
    let mut zone = ui.new_child(
        UiBuilder::new()
            .max_rect(trailing)
            .layout(Layout::right_to_left(Align::Center)),
    );
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

fn file_header(ui: &mut Ui, change: &Option<ChangeRef>) {
    let Some(change) = change else {
        return;
    };
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
    let label = change
        .path
        .display_name_lossy_or(&change.path.display().to_string());
    file_row(ui, &rect, &label, (0, 0), &mono_font(), false);
}

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

#[allow(clippy::too_many_arguments)]
fn render_side_by_side(
    ui: &mut Ui,
    state: &mut AppState,
    model: &DiffModel,
    plan: &PaintPlan,
    visible: Range<usize>,
    paint: egui::Id,
    status: ChangeStatus,
    change: &Option<ChangeRef>,
) {
    let width = ui.available_width();
    let spacing = ui.style().spacing.item_spacing.x;
    let half = ((width - spacing * 2.0) / 2.0).max(120.0);

    let painter = ui.painter().clone();
    let font = mono_font();
    let mut frame_hover: Option<usize> = None;
    let mut rows_rect: Option<Rect> = None;

    let end = visible.end.min(plan.sbs.len());
    let start = visible.start.min(end);
    for &di in &plan.sbs[start..end] {
        match &model.display[di] {
            DisplayRow::Full(row) => match row.kind {
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
                        change,
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
                RowKind::Del | RowKind::Add => unreachable!("changed rows are always paired"),
            },
            DisplayRow::Pair(d, a) => {
                let hunk = d.as_ref().map(|r| r.hunk).or(a.as_ref().map(|r| r.hunk));
                let caret_row = plan.caret == Some(di);
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
                            plan.staging_live,
                            false,
                        );
                        if cell.clicked() {
                            granular::toggle_row(state, change, d);
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
                            plan.staging_live,
                            plan.staging_live,
                        );
                        let text_origin = cell.rect.left() + NUM_W + 20.0;
                        let drag_staged =
                            handle_char_drag(ui, state, &cell, text_origin, a, change);
                        if cell.clicked() && !drag_staged {
                            granular::toggle_row(state, change, a);
                        }
                        paint_char_highlight(&painter, state, &cell.rect, text_origin, a, change);
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
