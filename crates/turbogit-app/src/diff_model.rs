use std::ops::Range;
use std::sync::Arc;

use turbogit_domain::model::{
    DiffHighlighting, DiffPolicies, Patch, PatchHeaderLine, PatchHunk, PatchLine, PatchLineKind,
};
use turbogit_services::diff_policy::{self, Chunk};
use turbogit_services::hunk_stats::HunkSpan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Meta,
    RenameHeader,
    Hunk,
    Context,
    Del,
    Add,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Highlight {
    Row,
    Runs(Vec<Range<usize>>),
    #[default]
    Off,
}

pub struct Row {
    pub kind: RowKind,
    pub text: String,
    pub hunk: usize,
    /// 0-based ordinal over the hunk's +/- lines in order (changed rows
    /// only), exactly [`turbogit_services::partial::HunkSelection::Lines`]
    /// semantics for sub-hunk selection.
    pub line_ord: usize,
    /// 1-based old-file line number (0 when not applicable).
    pub old_no: usize,
    /// 1-based new-file line number (0 when not applicable).
    pub new_no: usize,
    /// The hunk's span, present on a `Hunk` row and nowhere else. The paint
    /// path classifies a hunk's staged state from this.
    pub span: Option<HunkSpan>,
    pub highlight: Highlight,
}

fn rows_from(patch: &Patch) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut hunk = 0usize;
    let mut enclosing = 0usize;
    let mut changed;
    let mut old_no = 1usize;
    let mut new_no = 1usize;

    let meta = |text: String, rows: &mut Vec<Row>| {
        rows.push(Row {
            kind: RowKind::Meta,
            text,
            hunk: 0,
            line_ord: 0,
            old_no: 0,
            new_no: 0,
            span: None,
            highlight: Highlight::Off,
        });
    };

    for file in &patch.files {
        if patch.files.len() > 1 {
            meta(file.section_header_line(), &mut rows);
            for header in &file.headers {
                let painted_as_meta = matches!(
                    header,
                    PatchHeaderLine::Index { .. }
                        | PatchHeaderLine::Sources { .. }
                        | PatchHeaderLine::NewFile { .. }
                        | PatchHeaderLine::Binary
                );
                for line in header.lines(file) {
                    if painted_as_meta {
                        meta(line, &mut rows);
                        continue;
                    }
                    rows.push(Row {
                        kind: RowKind::Context,
                        text: line,
                        hunk: enclosing,
                        line_ord: 0,
                        old_no,
                        new_no,
                        span: None,
                        highlight: Highlight::Off,
                    });
                    old_no += 1;
                    new_no += 1;
                }
            }
        }

        for h in &file.hunks {
            rows.push(Row {
                kind: RowKind::Hunk,
                text: h.header_line(),
                hunk,
                line_ord: 0,
                old_no: 0,
                new_no: 0,
                span: Some(HunkSpan {
                    old_start: h.old_start,
                    old_lines: h.old_count,
                    new_start: h.new_start,
                    new_lines: h.new_count,
                }),
                highlight: Highlight::Off,
            });
            enclosing = hunk;
            hunk += 1;
            old_no = h.old_start.max(1);
            new_no = h.new_start.max(1);
            changed = 0;

            for line in &h.lines {
                let row_kind = match line.kind {
                    PatchLineKind::Added => RowKind::Add,
                    PatchLineKind::Removed => RowKind::Del,
                    PatchLineKind::Context => RowKind::Context,
                };
                let this_old = matches!(line.kind, PatchLineKind::Removed | PatchLineKind::Context);
                let this_new = matches!(line.kind, PatchLineKind::Added | PatchLineKind::Context);
                rows.push(Row {
                    kind: row_kind,
                    text: line.text.clone(),
                    hunk: enclosing,
                    line_ord: if row_kind == RowKind::Context {
                        0
                    } else {
                        changed
                    },
                    old_no: if this_old { old_no } else { 0 },
                    new_no: if this_new { new_no } else { 0 },
                    span: None,
                    highlight: Highlight::Off,
                });
                if row_kind != RowKind::Context {
                    changed += 1;
                }
                if this_old {
                    old_no += 1;
                }
                if this_new {
                    new_no += 1;
                }
                if line.no_newline {
                    meta(PatchLine::NO_NEWLINE_MARKER.to_string(), &mut rows);
                }
            }
        }
    }
    rows
}

/// The context run at one end of a hunk: the unchanged lines it prints before
/// its first change, or after its last.
fn context_run(hunk: &PatchHunk, leading: bool) -> usize {
    let is_context = |line: &&PatchLine| line.kind == PatchLineKind::Context;
    let lines = hunk.lines.iter();
    if leading {
        lines.take_while(is_context).count()
    } else {
        lines.rev().take_while(is_context).count()
    }
}

/// The width the patch was made at, read off the patch itself. A hard-coded 3
/// would be wrong for `git diff -U0`, which prints no context anywhere, and would
/// then declare every file's tail provably empty.
fn context_radius(patch: &Patch) -> usize {
    patch
        .files
        .iter()
        .flat_map(|file| &file.hunks)
        .map(|hunk| context_run(hunk, true).max(context_run(hunk, false)))
        .max()
        .unwrap_or_default()
}

/// One run of unchanged lines the patch did not print, inside one file section.
/// A run that hides nothing is never in the table, so "an affordance exists" and
/// "lines are hidden" are one fact rather than a check the UI can forget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gap {
    /// Index into [`DiffModel::files`], so `DiffTarget::pane_sides` can name the
    /// side this run belongs to.
    pub file: usize,
    /// Hunk whose body ends the top edge; `None` for a leading run.
    pub above: Option<usize>,
    /// Hunk whose header starts the bottom edge; `None` at end of file.
    pub below: Option<usize>,
    /// 1-based first hidden line on the old side.
    pub old_first: u32,
    /// 1-based first hidden line on the new side.
    pub new_first: u32,
    pub hidden: Hidden,
}

/// `Run` is decided by the headers alone. `ToEof` is a run that ends at the
/// file's end, a length only the file's own text knows, which is the single case
/// where an affordance can precede its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hidden {
    Run(u32),
    ToEof,
}

/// One entry per gap, plus the two lanes that place it. An interior or leading
/// gap is emitted only at its `below` hunk's header; only a file's trailing gap
/// is emitted after that file's last body row, so no gap is ever laid out twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GapTable {
    pub gaps: Vec<Gap>,
    before_hunk: Vec<Option<u32>>,
    eof_gap: Vec<Option<u32>>,
}

impl GapTable {
    /// The gap ordinal that lays out before this hunk's header row, in the same
    /// 0-based, runs-across-files space as [`Row::hunk`].
    pub fn before_hunk(&self, hunk: usize) -> Option<u32> {
        self.before_hunk.get(hunk).copied().flatten()
    }

    /// The gap ordinal that lays out after this file's last body row.
    pub fn eof_gap(&self, file: usize) -> Option<u32> {
        self.eof_gap.get(file).copied().flatten()
    }

    pub fn gap(&self, gap: u32) -> Option<&Gap> {
        self.gaps.get(gap as usize)
    }

    fn push_for_hunk(&mut self, hunk: usize, gap: Option<Gap>) {
        let ord = match gap {
            None => None,
            Some(gap) => {
                let ord = self.gaps.len() as u32;
                self.gaps.push(gap);
                Some(ord)
            }
        };
        if self.before_hunk.len() <= hunk {
            self.before_hunk.resize(hunk + 1, None);
        }
        self.before_hunk[hunk] = ord;
    }

    fn push_for_file(&mut self, file: usize, gap: Option<Gap>) {
        let ord = match gap {
            None => None,
            Some(gap) => {
                let ord = self.gaps.len() as u32;
                self.gaps.push(gap);
                Some(ord)
            }
        };
        if self.eof_gap.len() <= file {
            self.eof_gap.resize(file + 1, None);
        }
        self.eof_gap[file] = ord;
    }
}

/// How many lines a side hides above the hunk that starts at `start`, whose
/// `count` may be zero. git's count-omitted or explicit `0` spells the start as
/// an *anchor* — the unchanged line the change sits after, which the hunk does
/// not print either — so the run above a zero-count side is `start` lines, not
/// `start - 1`. `@@ -5,0 +6 @@` hides old lines 1 to 5.
fn hidden_above(start: usize, count: usize) -> u32 {
    let above = if count == 0 {
        start
    } else {
        start.saturating_sub(1)
    };
    u32::try_from(above).unwrap_or(u32::MAX)
}

/// The first line a side hides below a hunk: the line after its last printed
/// line, or after its anchor when it prints none.
fn first_below(start: usize, count: usize) -> u32 {
    u32::try_from(start + count.max(1)).unwrap_or(u32::MAX)
}

fn span(hunk: &PatchHunk, old: bool) -> (usize, usize) {
    if old {
        (hunk.old_start, hunk.old_count)
    } else {
        (hunk.new_start, hunk.new_count)
    }
}

/// How many lines one side hides between two consecutive hunks of one file. Both
/// sides answer this the same way for a diff git printed, because everything
/// between two hunks is unchanged lines.
fn hidden_between(prev: &PatchHunk, next: &PatchHunk, old: bool) -> u32 {
    let (prev_start, prev_count) = span(prev, old);
    let (next_start, _) = span(next, old);
    u32::try_from(next_start.saturating_sub(prev_start + prev_count)).unwrap_or(u32::MAX)
}

/// A run above a file's first hunk, or `None` when it hides nothing or the two
/// sides do not agree.
fn leading_gap(file: usize, below: usize, hunk: &PatchHunk) -> Option<Gap> {
    let old = hidden_above(hunk.old_start, hunk.old_count);
    let new = hidden_above(hunk.new_start, hunk.new_count);
    // Two disagreeing counts mean the run is not the same lines on both sides, so
    // it cannot be painted as paired context: the gap is dropped, not clamped to
    // the smaller, because a half-truth about which lines are hidden is worse
    // than no affordance.
    (old == new && old > 0).then_some(Gap {
        file,
        above: None,
        below: Some(below),
        old_first: 1,
        new_first: 1,
        hidden: Hidden::Run(old),
    })
}

/// A run between two consecutive hunks of one file, or `None` when it hides
/// nothing or the two sides do not agree.
fn interior_gap(
    file: usize,
    above: usize,
    below: usize,
    prev: &PatchHunk,
    next: &PatchHunk,
) -> Option<Gap> {
    let old = hidden_between(prev, next, true);
    let new = hidden_between(prev, next, false);
    (old == new && old > 0).then_some(Gap {
        file,
        above: Some(above),
        below: Some(below),
        old_first: first_below(prev.old_start, prev.old_count),
        new_first: first_below(prev.new_start, prev.new_count),
        hidden: Hidden::Run(old),
    })
}

/// The run below a file's last hunk. Its length is not in the patch, so it is
/// `ToEof` — except when the hunk's trailing context is shorter than the radius,
/// which proves git had reached the file's end and printed everything.
fn eof_gap(file: usize, above: usize, last: &PatchHunk, radius: usize) -> Option<Gap> {
    let trailing = context_run(last, false);
    (trailing >= radius).then_some(Gap {
        file,
        above: Some(above),
        below: None,
        old_first: first_below(last.old_start, last.old_count),
        new_first: first_below(last.new_start, last.new_count),
        hidden: Hidden::ToEof,
    })
}

/// The runs of unchanged lines this patch leaves unprinted. Geometry comes from
/// the hunk spans and never from the printed rows: `rows_from` resets its
/// `old_no`/`new_no` counters per hunk but not per file, so between two sections
/// of a multi-file patch the header rows carry the *previous* file's numbers, and
/// a gap derived from them would sit in the wrong file at the wrong line.
pub fn gap_table(patch: &Patch) -> GapTable {
    let mut table = GapTable::default();
    let radius = context_radius(patch);
    let mut hunk = 0usize;
    for (file, section) in patch.files.iter().enumerate() {
        table.push_for_file(file, None);
        // A creation, a deletion and a combined (`diff --cc`) view have no
        // unchanged lines to reveal: one side is `/dev/null`, or the section
        // compares one path against two parents at once.
        let revealable = !section.new_file() && !section.deleted_file() && !section.combined;
        let mut prev: Option<(usize, &PatchHunk)> = None;
        for next in &section.hunks {
            let gap = match (revealable, prev) {
                (true, None) => leading_gap(file, hunk, next),
                (true, Some((above, prev))) => interior_gap(file, above, hunk, prev, next),
                _ => None,
            };
            table.push_for_hunk(hunk, gap);
            prev = Some((hunk, next));
            hunk += 1;
        }
        if let (true, Some((above, last))) = (revealable, prev) {
            table.push_for_file(file, eof_gap(file, above, last, radius));
        }
    }
    table
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMeta {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub similarity: Option<u32>,
    pub renamed: bool,
    pub new_file: bool,
    pub deleted_file: bool,
    pub binary: bool,
}

pub(crate) fn files_from(patch: &Patch) -> Vec<FileMeta> {
    patch
        .files
        .iter()
        .map(|file| FileMeta {
            old_path: (!file.old_path.is_empty()).then(|| file.old_path.clone()),
            new_path: (!file.new_path.is_empty()).then(|| file.new_path.clone()),
            similarity: file.similarity().map(u32::from),
            renamed: file.renamed(),
            new_file: file.new_file(),
            deleted_file: file.deleted_file(),
            binary: file.binary(),
        })
        .collect()
}

fn rename_header_text(meta: &FileMeta) -> Option<String> {
    if !meta.renamed {
        return None;
    }
    let old = meta.old_path.as_deref().unwrap_or("(unknown)");
    Some(match meta.similarity {
        Some(pct) => format!("Renamed from {old} · {pct}% similar"),
        None => format!("Renamed from {old}"),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Text,
    Image,
    Binary,
}

/// Extensions decoded as images (CONTEXT.md "Image diff"). SVG is
/// deliberately absent, never decoded (ADR-0015); an SVG change renders
/// as whatever the patch says (text rows or binary placeholder).
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

fn is_image_path(path: &str) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    ext.is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))
}

pub fn pane_kind(files: &[FileMeta]) -> PaneKind {
    let [f] = files else {
        return PaneKind::Text;
    };
    let both_images = f.old_path.as_deref().is_some_and(is_image_path)
        && f.new_path.as_deref().is_some_and(is_image_path);
    if both_images {
        PaneKind::Image
    } else if f.binary {
        PaneKind::Binary
    } else {
        PaneKind::Text
    }
}

pub enum DisplayRow {
    Full(Row),
    Pair(Option<Row>, Option<Row>),
}

pub struct DiffModel {
    pub display: Vec<DisplayRow>,
    pub hunk_first_display: Vec<u32>,
    pub files: Vec<FileMeta>,
    pub rename_header: Option<String>,
    gap_table: GapTable,
}

impl DiffModel {
    pub fn hunk_count(&self) -> usize {
        self.hunk_first_display.len()
    }

    /// The unprinted runs, addressed by the lanes that lay them out. Derived from
    /// the patch's hunk spans beside `rows_from`, never from `display`.
    pub fn gaps(&self) -> &GapTable {
        &self.gap_table
    }

    pub fn pure_rename(&self) -> bool {
        self.hunk_count() == 0
            && self.files.first().is_some_and(|f| {
                f.renamed && f.similarity == Some(100) && !f.new_file && !f.deleted_file
            })
    }
}

/// (added, removed) changed-line counts of the display model, the diff
/// preview header's `+N −M` change-size stats. Changed rows always live in
/// pairs, but full-width Add/Del rows are counted too for robustness.
pub fn line_counts(model: &DiffModel) -> (usize, usize) {
    let mut added = 0usize;
    let mut removed = 0usize;
    for disp in &model.display {
        match disp {
            DisplayRow::Full(row) => match row.kind {
                RowKind::Add => added += 1,
                RowKind::Del => removed += 1,
                _ => {}
            },
            DisplayRow::Pair(d, a) => {
                if d.is_some() {
                    removed += 1;
                }
                if a.is_some() {
                    added += 1;
                }
            }
        }
    }
    (added, removed)
}

fn pair_highlight(hl: DiffHighlighting, old: &str, new: &str) -> (Highlight, Highlight) {
    match hl {
        DiffHighlighting::None => (Highlight::Off, Highlight::Off),
        DiffHighlighting::Lines | DiffHighlighting::SplitChanges => {
            (Highlight::Row, Highlight::Row)
        }
        g => (
            Highlight::Runs(diff_policy::highlight_runs(g, new, old)),
            Highlight::Runs(diff_policy::highlight_runs(g, old, new)),
        ),
    }
}

fn equalized_run(dels: Vec<Row>, adds: Vec<Row>) -> Vec<DisplayRow> {
    let shared_old = dels.len() == adds.len();
    adds.into_iter()
        .enumerate()
        .map(|(i, row)| {
            let old_no = if shared_old { dels[i].old_no } else { 0 };
            DisplayRow::Full(Row {
                kind: RowKind::Context,
                text: row.text,
                hunk: row.hunk,
                line_ord: 0,
                old_no,
                new_no: row.new_no,
                span: None,
                highlight: Highlight::Off,
            })
        })
        .collect()
}

fn pair_run(dels: Vec<Row>, adds: Vec<Row>, hl: DiffHighlighting) -> Vec<DisplayRow> {
    if hl == DiffHighlighting::SplitChanges {
        split_run(dels, adds)
    } else {
        positional_run(dels, adds, hl)
    }
}

fn split_run(dels: Vec<Row>, adds: Vec<Row>) -> Vec<DisplayRow> {
    let mut slots_d: Vec<Option<Row>> = dels.into_iter().map(Some).collect();
    let mut slots_a: Vec<Option<Row>> = adds.into_iter().map(Some).collect();
    let del_texts: Vec<&str> = slots_d
        .iter()
        .map(|s| s.as_ref().map_or("", |r| r.text.as_str()))
        .collect();
    let add_texts: Vec<&str> = slots_a
        .iter()
        .map(|s| s.as_ref().map_or("", |r| r.text.as_str()))
        .collect();
    let chunks = diff_policy::split_changes(&del_texts, &add_texts);
    let mut out = Vec::new();
    for chunk in chunks {
        match chunk {
            Chunk::Carried(d, a) => {
                let add = slots_a[a].take();
                if let Some(row) = slots_d[d].take() {
                    out.push(DisplayRow::Full(Row {
                        kind: RowKind::Context,
                        text: row.text,
                        hunk: row.hunk,
                        line_ord: 0,
                        old_no: row.old_no,
                        new_no: add.map_or(0, |r| r.new_no),
                        span: None,
                        highlight: Highlight::Off,
                    }));
                }
            }
            Chunk::Block(pairs) => {
                out.extend(pairs.into_iter().map(|(d, a)| {
                    DisplayRow::Pair(
                        d.and_then(|i| slots_d[i].take()),
                        a.and_then(|i| slots_a[i].take()),
                    )
                }));
            }
        }
    }
    debug_assert!(slots_d.iter().all(Option::is_none) && slots_a.iter().all(Option::is_none));
    out
}

fn positional_run(dels: Vec<Row>, adds: Vec<Row>, hl: DiffHighlighting) -> Vec<DisplayRow> {
    let pairs = dels.len().max(adds.len());
    let mut dels = dels.into_iter();
    let mut adds = adds.into_iter();
    (0..pairs)
        .map(|_| {
            let mut del = dels.next();
            let mut add = adds.next();
            let (dh, ah) = pair_highlight(
                hl,
                del.as_ref().map_or("", |r| r.text.as_str()),
                add.as_ref().map_or("", |r| r.text.as_str()),
            );
            if let Some(row) = del.as_mut() {
                row.highlight = dh;
            }
            if let Some(row) = add.as_mut() {
                row.highlight = ah;
            }
            DisplayRow::Pair(del, add)
        })
        .collect()
}

fn flush_changed_run(
    pending: &mut Vec<Row>,
    display: &mut Vec<DisplayRow>,
    policies: DiffPolicies,
) {
    if pending.is_empty() {
        return;
    }
    let mut dels: Vec<Row> = Vec::new();
    let mut adds: Vec<Row> = Vec::new();
    for row in pending.drain(..) {
        if row.kind == RowKind::Del {
            dels.push(row);
        } else {
            adds.push(row);
        }
    }
    let del_texts: Vec<&str> = dels.iter().map(|r| r.text.as_str()).collect();
    let add_texts: Vec<&str> = adds.iter().map(|r| r.text.as_str()).collect();
    if diff_policy::considered_equal(policies.whitespace, &del_texts, &add_texts) {
        display.extend(equalized_run(dels, adds));
        return;
    }
    display.extend(pair_run(dels, adds, policies.highlighting));
}

fn build_model(
    rows: Vec<Row>,
    files: Vec<FileMeta>,
    policies: DiffPolicies,
    gap_table: GapTable,
) -> DiffModel {
    let mut display = Vec::new();
    let mut hunk_first_display = Vec::new();

    let rename_header = files.iter().find_map(rename_header_text);
    if let Some(text) = &rename_header {
        display.push(DisplayRow::Full(Row {
            kind: RowKind::RenameHeader,
            text: text.clone(),
            hunk: 0,
            line_ord: 0,
            old_no: 0,
            new_no: 0,
            span: None,
            highlight: Highlight::Off,
        }));
    }

    let mut pending: Vec<Row> = Vec::new();
    for row in rows {
        match row.kind {
            RowKind::Del | RowKind::Add => pending.push(row),
            RowKind::Meta | RowKind::RenameHeader | RowKind::Context => {
                flush_changed_run(&mut pending, &mut display, policies);
                display.push(DisplayRow::Full(row));
            }
            RowKind::Hunk => {
                flush_changed_run(&mut pending, &mut display, policies);
                hunk_first_display.push(display.len() as u32);
                display.push(DisplayRow::Full(row));
            }
        }
    }
    flush_changed_run(&mut pending, &mut display, policies);
    DiffModel {
        display,
        hunk_first_display,
        files,
        rename_header,
        gap_table,
    }
}

/// The diff read's value: one comparison's patch and the display model built
/// from it, behind one handle. Cloning the handle is a refcount bump, once per
/// frame, never per row, which is what leaves `&mut AppState` free for a
/// painter to write the hunk cursor and collapse set (ADR-0021).
pub struct DiffValue {
    pub patch: Patch,
    pub model: DiffModel,
}

impl DiffValue {
    /// `policies` folds into the same pass. The patch stays the patch: what the
    /// policies change is which rows the viewer counts as different and how each
    /// difference is highlighted, never the bytes partial staging composes from.
    pub(crate) fn build(patch: Patch, policies: DiffPolicies) -> Arc<Self> {
        let rows = rows_from(&patch);
        let files = files_from(&patch);
        let gaps = gap_table(&patch);
        Arc::new(Self {
            model: build_model(rows, files, policies, gaps),
            patch,
        })
    }

    pub fn is_blank(&self) -> bool {
        self.patch.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(text: &str) -> Patch {
        turbogit_engine_api::patch::parse_patch(text)
    }

    fn parse(text: &str) -> Vec<Row> {
        rows_from(&patch(text))
    }

    fn scan_files(text: &str) -> Vec<FileMeta> {
        files_from(&patch(text))
    }

    /// One whole production path, exactly as `DiffValue::build` runs it, so a
    /// test can put the gap pass beside the row pass and compare the rows against
    /// the fingerprint taken before the table existed.
    fn built(text: &str) -> DiffModel {
        let p = patch(text);
        build_model(
            rows_from(&p),
            files_from(&p),
            DiffPolicies::default(),
            gap_table(&p),
        )
    }

    /// `(kind, hunk, old_no, new_no, text)` for every row, pairs flattened in
    /// del-then-add order: the shape a painter sees.
    fn fingerprint(model: &DiffModel) -> Vec<String> {
        let row = |r: &Row| {
            format!(
                "{:?} {} {} {} {}",
                r.kind, r.hunk, r.old_no, r.new_no, r.text
            )
        };
        model
            .display
            .iter()
            .map(|disp| match disp {
                DisplayRow::Full(r) => row(r),
                DisplayRow::Pair(del, add) => {
                    let mut parts = Vec::new();
                    if let Some(r) = del {
                        parts.push(row(r));
                    }
                    if let Some(r) = add {
                        parts.push(row(r));
                    }
                    parts.join(" | ")
                }
            })
            .collect()
    }

    const RENAME_WITH_SIMILARITY: &str = "diff --git a/src/old.rs b/src/new.rs\n\
         similarity index 92%\n\
         rename from src/old.rs\n\
         rename to src/new.rs\n\
         --- a/src/old.rs\n\
         +++ b/src/new.rs\n\
         @@ -1 +1 @@\n\
         -a\n\
         +b\n";

    #[test]
    fn diff_rename_header_extracts_similarity() {
        let files = scan_files(RENAME_WITH_SIMILARITY);
        assert_eq!(files.len(), 1);
        assert!(files[0].renamed);
        assert_eq!(files[0].similarity, Some(92));
        assert_eq!(files[0].old_path.as_deref(), Some("src/old.rs"));
        assert_eq!(files[0].new_path.as_deref(), Some("src/new.rs"));
        assert_eq!(
            rename_header_text(&files[0]).as_deref(),
            Some("Renamed from src/old.rs · 92% similar")
        );
    }

    #[test]
    fn diff_rename_header_omits_missing_similarity() {
        let text = "diff --git a/x.txt b/y.txt\n\
                    rename from x.txt\n\
                    rename to y.txt\n";
        let files = scan_files(text);
        assert_eq!(files.len(), 1);
        assert!(files[0].renamed);
        assert_eq!(files[0].similarity, None);
        assert_eq!(
            rename_header_text(&files[0]).as_deref(),
            Some("Renamed from x.txt")
        );
    }

    #[test]
    fn diff_pure_rename_renders_header_without_hunks() {
        let text = "diff --git a/old.rs b/new.rs\n\
                    similarity index 100%\n\
                    rename from old.rs\n\
                    rename to new.rs\n";
        let model = built(text);
        assert!(model.pure_rename());
        assert_eq!(pane_kind(&scan_files(text)), PaneKind::Text);
        assert_eq!(model.hunk_count(), 0);
        assert_eq!(model.display.len(), 1);
        let DisplayRow::Full(row) = &model.display[0] else {
            panic!("rename header must be a full-width row");
        };
        assert_eq!(row.kind, RowKind::RenameHeader);
        assert_eq!(row.text, "Renamed from old.rs · 100% similar");
    }

    #[test]
    fn diff_binary_section_detected_single_and_mixed() {
        let binary = "diff --git a/logo.png b/logo.png\n\
                      index 1111111..2222222 100644\n\
                      Binary files a/logo.png and b/logo.png differ\n";
        assert_eq!(pane_kind(&scan_files(binary)), PaneKind::Image);

        let bin_txt = "diff --git a/data.bin b/data.bin\n\
                       index 1111111..2222222 100644\n\
                       Binary files a/data.bin and b/data.bin differ\n";
        assert_eq!(pane_kind(&scan_files(bin_txt)), PaneKind::Binary);

        let mixed = format!("{bin_txt}diff --git a/x.rs b/x.rs\n@@ -1 +1 @@\n-a\n+b\n");
        let files = scan_files(&mixed);
        assert_eq!(files.len(), 2);
        assert!(files[0].binary);
        assert!(!files[1].binary);
        assert_eq!(pane_kind(&files), PaneKind::Text);
    }

    #[test]
    fn diff_rename_header_keeps_hunk_navigation_aligned() {
        let model = built(RENAME_WITH_SIMILARITY);
        assert_eq!(model.hunk_count(), 1);
        assert_eq!(model.hunk_first_display[0], 1);
    }

    #[test]
    fn diff_single_file_sections_drop_their_plumbing_rows() {
        let text = "diff --git a/added.txt b/added.txt\n\
                    new file mode 100644\n\
                    index 0000000..3b18e51\n\
                    --- /dev/null\n\
                    +++ b/added.txt\n\
                    @@ -0,0 +1,2 @@\n\
                    +one\n\
                    +two\n";
        let rows = parse(text);
        assert!(
            rows.iter().all(|r| r.kind != RowKind::Meta),
            "no meta row survives a single-file section"
        );
        assert!(
            !rows.iter().any(|r| {
                ["diff --git ", "new file mode ", "index ", "--- ", "+++ "]
                    .iter()
                    .any(|prefix| r.text.starts_with(prefix))
            }),
            "git's header block is gone in every spelling: {:?}",
            rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>()
        );
        let body: Vec<(RowKind, usize, usize)> = rows
            .iter()
            .filter(|r| r.text == "one" || r.text == "two")
            .map(|r| (r.kind, r.old_no, r.new_no))
            .collect();
        assert_eq!(
            body,
            vec![(RowKind::Add, 0, 1), (RowKind::Add, 0, 2)],
            "1-based new-side numbers, from the span"
        );
    }

    #[test]
    fn diff_multi_file_patches_keep_every_plumbing_row() {
        let text = "diff --git a/one.txt b/one.txt\n\
                    index 1111111..2222222 100644\n\
                    --- a/one.txt\n\
                    +++ b/one.txt\n\
                    @@ -1 +1 @@\n\
                    -a\n\
                    +b\n\
                    diff --git a/two.txt b/two.txt\n\
                    index 3333333..4444444 100644\n\
                    --- a/two.txt\n\
                    +++ b/two.txt\n\
                    @@ -1 +1 @@\n\
                    -c\n\
                    +d\n";
        let rows = parse(text);
        assert_eq!(
            rows.iter()
                .filter(|r| r.text.starts_with("diff --git "))
                .count(),
            2,
            "every section keeps its opening line"
        );
        assert_eq!(
            rows.iter().filter(|r| r.text.starts_with("--- ")).count(),
            2,
            "the source pairs stay in a multi-file patch"
        );
    }

    #[test]
    fn diff_sections_split_per_file_with_mode_flags() {
        let text = "commit header noise\n\
                    diff --git a/added.bin b/added.bin\n\
                    new file mode 100644\n\
                    diff --git a/gone.txt b/gone.txt\n\
                    deleted file mode 100644\n";
        let files = scan_files(text);
        assert_eq!(files.len(), 2);
        assert!(files[0].new_file && !files[0].deleted_file);
        assert!(files[1].deleted_file && !files[1].renamed);
        assert_eq!(files[1].old_path.as_deref(), Some("gone.txt"));
    }

    #[test]
    fn diff_git_paths_arrive_resolved_from_the_value() {
        let paths = |text: &str| {
            let files = scan_files(text);
            (files[0].old_path.clone(), files[0].new_path.clone())
        };
        assert_eq!(
            paths("diff --git a/src/lib.rs b/src/lib.rs\n"),
            (Some("src/lib.rs".to_owned()), Some("src/lib.rs".to_owned()))
        );
        assert_eq!(
            paths("diff --git \"a/we\\tird\" \"b/ot\\ther\"\n"),
            (Some("we\tird".to_owned()), Some("ot\ther".to_owned()))
        );
        let spaced = scan_files("diff --git a/keep me.txt b/kept me.txt\n");
        assert_eq!(
            spaced[0].new_path.as_deref(),
            Some("kept me.txt"),
            "a space in a name must not be read as a field separator"
        );
        assert!(
            scan_files("nothing at all\n").is_empty(),
            "no section, no file"
        );
    }

    #[test]
    fn the_gap_pass_leaves_the_display_rows_untouched() {
        let shapes = [
            RENAME_WITH_SIMILARITY,
            "diff --git a/one.txt b/one.txt\n\
             index 1111111..2222222 100644\n\
             --- a/one.txt\n\
             +++ b/one.txt\n\
             @@ -5,3 +5,3 @@\n\
              five\n\
             -six\n\
             +SIX\n\
              seven\n\
             diff --git a/two.txt b/two.txt\n\
             index 3333333..4444444 100644\n\
             --- a/two.txt\n\
             +++ b/two.txt\n\
             @@ -3,0 +4 @@\n\
             +inserted\n\
             @@ -7,0 +8 @@\n\
             +also\n",
        ];
        for text in shapes {
            let p = patch(text);
            let build = |gaps: GapTable| {
                fingerprint(&build_model(
                    rows_from(&p),
                    files_from(&p),
                    DiffPolicies::default(),
                    gaps,
                ))
            };
            let with = build(gap_table(&p));
            let without = build(GapTable::default());
            assert_eq!(
                with, without,
                "the gap pass may add a table and nothing else:\n{text}"
            );
            assert_eq!(
                built(text).gaps(),
                &gap_table(&p),
                "the model carries the one table its patch implies: {text}"
            );
        }
    }

    #[test]
    fn diff_pane_kind_sniffs_image_extensions() {
        let meta = |old: &str, new: &str| FileMeta {
            old_path: Some(old.to_owned()),
            new_path: Some(new.to_owned()),
            ..FileMeta::default()
        };
        assert_eq!(pane_kind(&[meta("x.PNG", "x.png")]), PaneKind::Image);
        let mut svg_bin = meta("logo.svg", "logo.svg");
        svg_bin.binary = true;
        assert_eq!(pane_kind(&[svg_bin]), PaneKind::Binary);
        assert_eq!(pane_kind(&[meta("logo.svg", "logo.svg")]), PaneKind::Text);
        assert_eq!(pane_kind(&[meta("x.rs", "x.rs")]), PaneKind::Text);
        assert_eq!(pane_kind(&[]), PaneKind::Text);
    }
}
