//! The diff display model: the **Git engine**'s patch value in, renderable rows
//! out.
//!
//! Plain data owned by the application layer, imported back up by the UI
//! modules (the same DDD split that put [`crate::diff_data`]'s pane types
//! here): nothing in this file references egui, and every layout constant and
//! typeface stayed with the painters.
//!
//! It reads a [`Patch`], so git's unified-diff syntax is interpreted in exactly
//! one place — the engine's parser — and no line of this file tests a string
//! prefix to decide what a line is. It is built once per diff *content*, beside
//! the patch text, and handed out as the display model the keyed read's diff
//! answer carries (ADR-0014, ADR-0021).

use std::ops::Range;
use std::sync::Arc;

use turbogit_domain::model::{
    DiffHighlighting, DiffPolicies, Patch, PatchHeaderLine, PatchLine, PatchLineKind,
};
use turbogit_services::diff_policy::{self, Chunk};
use turbogit_services::hunk_stats::HunkSpan;

// --- row model ---------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Meta,
    /// Rename metadata (CONTEXT.md "Rename header"), synthesized from the
    /// section scan: leads the content as its own full-width display row.
    RenameHeader,
    Hunk,
    Context,
    Del,
    Add,
}

/// How one changed row is highlighted, decided by the viewer's
/// [`DiffHighlighting`] before the model is built.
///
/// The three shapes are what a painter has to be able to tell apart: a band
/// across the whole row, one band per changed run, or no band at all. A row the
/// model does not consider a difference carries [`Highlight::Off`] and is never
/// asked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Highlight {
    /// The whole row is the highlight — TurboGit's pre-existing rendering.
    Row,
    /// Char-offset runs of the row's own text, one band each.
    Runs(Vec<Range<usize>>),
    /// Nothing is highlighted.
    #[default]
    Off,
}

pub struct Row {
    pub kind: RowKind,
    pub text: String,
    /// Owning hunk index — the header's own ordinal for `Hunk` rows, and
    /// the enclosing hunk for body rows (aiming the current hunk from the
    /// pointer, spec R2). Meta rows own no hunk.
    pub hunk: usize,
    /// 0-based ordinal over the hunk's +/- lines in order (changed rows
    /// only) — exactly [`turbogit_services::partial::HunkSelection::Lines`]
    /// semantics for sub-hunk selection (spec R2 story 3).
    pub line_ord: usize,
    /// 1-based old-file line number (0 when not applicable).
    pub old_no: usize,
    /// 1-based new-file line number (0 when not applicable).
    pub new_no: usize,
    /// The hunk's span, present on a `Hunk` row and nowhere else. The paint
    /// path classifies a hunk's staged state from this, which is what retired
    /// the second read of git's `@@` header.
    pub span: Option<HunkSpan>,
    /// This row's highlighting under the viewer's [`DiffHighlighting`]. Only a
    /// changed row is ever asked.
    pub highlight: Highlight,
}

/// Build the renderable rows from the patch value: 1-based gutter numbers from
/// each hunk's span, per-hunk changed-line ordinals for sub-hunk selection
/// (spec R2 story 3), and each hunk's span carried on the row so the paint path
/// asks the row rather than re-reading git's header.
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
        // A pane showing exactly one file section has no plumbing block: the
        // pane's own chrome names the file, so git's header lines would only
        // repeat it. Multi-file patches keep every row — there the section
        // headers are what tell the sections apart. Hunk-body numbering is
        // untouched either way: it resets from each hunk's own span, never
        // from the header rows the walk advances past.
        if patch.files.len() > 1 {
            meta(file.section_header_line(), &mut rows);
            for header in &file.headers {
                // Which of these the viewer paints as section metadata is a fact
                // about the header, not about the text it renders to.
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
                    // A header the row model does not name — a similarity percent, a
                    // rename pair, a mode change — reaches the viewer as an ordinary
                    // line, advancing both gutters like the context it sits among.
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
    // Raw paging ordinals are assigned by [`build_model`] over the
    // header-prefixed space (ADR-0014, spec R8).
    rows
}

// --- per-file section metadata (R8) ------------------------------------------

/// Metadata for one `diff --git` file section (spec R8): paths, rename
/// detection, file-mode changes, and binary-ness — derived purely from the
/// patch text, never the engine. Rename detection rides on git's own
/// defaults, so rename/similarity headers may be absent; absence is handled
/// gracefully.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMeta {
    /// Path before the change, from the `diff --git a/… b/…` line.
    pub old_path: Option<String>,
    /// Path after the change, from the same line.
    pub new_path: Option<String>,
    /// `similarity index N%` when git emitted a rename estimate.
    pub similarity: Option<u32>,
    /// `rename from`/`rename to` headers were present.
    pub renamed: bool,
    /// `new file mode` header was present.
    pub new_file: bool,
    /// `deleted file mode` header was present.
    pub deleted_file: bool,
    /// The section body carries `Binary files … differ` (no textual hunks).
    pub binary: bool,
}

/// Per-file section metadata, read off the patch value — paths, rename
/// detection, file-mode changes and binary-ness are fields the engine answered,
/// not a scan of git's syntax. Rename detection rides on git's own defaults, so
/// rename/similarity headers may be absent; absence is handled gracefully.
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

/// Formatted rename-header line (CONTEXT.md "Rename header") for a file
/// section carrying rename metadata; `None` otherwise. The similarity
/// percentage is omitted when git emitted none.
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

// --- non-text panes (R8, ADR-0015) -------------------------------------------

/// Which pane renders the open diff: text rows, an image pair, or the
/// binary-change placeholder. Only single-file sections leave the row
/// model; multi-file texts keep their inline rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Text,
    Image,
    Binary,
}

/// Extensions decoded as images (CONTEXT.md "Image diff"). SVG is
/// deliberately absent — never decoded (ADR-0015); an SVG change renders
/// as whatever the patch says (text rows or binary placeholder).
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// Extension sniff (case-insensitive) deciding whether a side decodes.
fn is_image_path(path: &str) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    ext.is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))
}

/// Pane kind of the open diff: an image pair when the single section's
/// paths both sniff as images, else a lone binary change, else the normal
/// text rows.
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

// --- display-row model (ADR-0014) --------------------------------------------

/// One virtualized display row: the unit `ScrollArea::show_rows` pages
/// over. Side-by-side mode renders each element as one row band;
/// unified mode ignores pairing and flattens every contained row back into
/// its own band — both modes read from this one cached vector.
pub enum DisplayRow {
    /// Full-width row: meta, hunk header, or context.
    Full(Row),
    /// A side-by-side pair sharing one row band: one Del and/or one
    /// Add (`None` fills the missing half with an empty cell).
    Pair(Option<Row>, Option<Row>),
}

/// Everything rendering needs from one diff, built with the patch text it
/// came from (ADR-0014, ADR-0021): the display-row vector the per-frame paint
/// plan filters into slot streams (issue 20), and the per-file section
/// metadata R8 renders from.
pub struct DiffModel {
    pub display: Vec<DisplayRow>,
    /// Hunk ordinal → first display index of its header row.
    pub hunk_first_display: Vec<u32>,
    /// Per-file section metadata (spec R8), scanned once beside the rows.
    pub files: Vec<FileMeta>,
    /// Formatted rename header (CONTEXT.md "Rename header") of the first
    /// renamed section, when any — rendered as the leading display row.
    pub rename_header: Option<String>,
}

impl DiffModel {
    /// Number of parsed hunks.
    pub fn hunk_count(&self) -> usize {
        self.hunk_first_display.len()
    }

    /// Whether the open diff is a pure rename: a detected rename with 100%
    /// similarity, no content hunks, and no file-mode creation/deletion —
    /// the header plus a "no content changes" note stands in for the empty
    /// scroller (spec R8).
    pub fn pure_rename(&self) -> bool {
        self.hunk_count() == 0
            && self.files.first().is_some_and(|f| {
                f.renamed && f.similarity == Some(100) && !f.new_file && !f.deleted_file
            })
    }
}

/// (added, removed) changed-line counts of the display model — the diff
/// preview header's `+N −M` change-size stats (design doc §5). Changed rows
/// always live in pairs (build_model invariant), but full-width Add/Del rows
/// are counted too for robustness.
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

/// The two halves of one changed pair's highlighting, under `hl`.
///
/// The run list [`diff_policy::highlight_runs`] answers with is named on its
/// **second** argument's text, so the two sides are asked in opposite order —
/// which compiles identically either way and silently swaps the highlights onto
/// the wrong pane.
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

/// A changed run the whitespace policy considers equal, as the rows it leaves:
/// the new side's lines, standing as ordinary unchanged content.
///
/// A row the viewer does not consider a difference owns no changed-line ordinal
/// and takes no highlight — which is what keeps it out of the sub-hunk selection
/// the granular verbs compose, since a line the user was told to ignore cannot be
/// the line they meant to stage.
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

/// Pair a run that survived the whitespace policy, by the highlighting policy.
fn pair_run(dels: Vec<Row>, adds: Vec<Row>, hl: DiffHighlighting) -> Vec<DisplayRow> {
    if hl == DiffHighlighting::SplitChanges {
        split_run(dels, adds)
    } else {
        positional_run(dels, adds, hl)
    }
}

/// The run partitioned into the smaller changes it is made of: a line that
/// appears on both sides is carried through and belongs to no change, and every
/// remaining block is highlighted one line at a time.
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
    // The partition answers with run-relative indices only, so the borrow of the
    // slots ends here and the walk below can take rows out of them.
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
    // Every line of the run is covered by exactly one op, so both slot lists are
    // empty here; a leftover would be a bug in the partition, not a case to paint.
    debug_assert!(slots_d.iter().all(Option::is_none) && slots_a.iter().all(Option::is_none));
    out
}

/// One change across the whole run: the two sides paired line for line, each row
/// carrying the highlighting its own side earned.
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

/// Fold a buffered run of consecutive Del/Add rows into display rows.
///
/// **The order of the two policies is load-bearing** and compiles either way:
/// the whitespace policy decides whether the run is a difference at all, and only
/// a run that survives is cut up by the highlighting policy. Read the other way
/// and a change the viewer was told to ignore still gets its lines partitioned
/// into carried and changed rows — a difference that shows as nothing while
/// still moving rows around.
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

/// Fold parsed rows plus the per-file section scan (spec R8) into the
/// display-row model (ADR-0014): consecutive Del/Add runs collapse into
/// paired display rows while meta/hunk/context stay full-width, and a
/// rename header — when the scan found one — leads as its own full-width
/// row. The per-frame paint plan (issue 20) filters this stream into the
/// paged slot lists.
///
/// The viewer's policies are applied here and nowhere else: they are a property
/// of the model, built once per content *and policy*, so no frame re-parses and
/// no policy decision reaches the patch text the staging path composes from.
fn build_model(rows: Vec<Row>, files: Vec<FileMeta>, policies: DiffPolicies) -> DiffModel {
    let mut display = Vec::new();
    let mut hunk_first_display = Vec::new();

    // The rename header (CONTEXT.md) leads the content as its own display
    // row, before every hunk (spec R8).
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
    }
}

/// The diff read's value: one comparison's patch and the display model built
/// from it, behind one handle. Cloning the handle is a refcount bump — once per
/// frame, never per row — which is what leaves `&mut AppState` free for a
/// painter to write the hunk cursor and collapse set (ADR-0021).
pub struct DiffValue {
    pub patch: Patch,
    pub model: DiffModel,
}

impl DiffValue {
    /// Fold the patch value once, at the moment the content is settled — which
    /// is the whole point of the read owning it: the paint path never parses,
    /// and nothing compares patch text to decide (`ADR-0022`).
    ///
    /// `policies` folds into the same pass. The patch stays the patch: what the
    /// policies change is which rows the viewer counts as different and how each
    /// difference is highlighted, never the bytes partial staging composes from.
    pub(crate) fn build(patch: Patch, policies: DiffPolicies) -> Arc<Self> {
        Arc::new(Self {
            model: build_model(rows_from(&patch), files_from(&patch), policies),
            patch,
        })
    }

    /// A cached entry with no visible rows is a settled answer of "nothing to
    /// show", not a missing one (spec R2).
    pub fn is_blank(&self) -> bool {
        self.patch.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two readers under test, reached the only way they are reachable now:
    /// through the engine's one patch parser.
    fn patch(text: &str) -> Patch {
        turbogit_engine_api::patch::parse_patch(text)
    }

    fn parse(text: &str) -> Vec<Row> {
        rows_from(&patch(text))
    }

    fn scan_files(text: &str) -> Vec<FileMeta> {
        files_from(&patch(text))
    }

    /// Rename with content edits: headers plus one hunk (spec R8).
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
        // Rename detection rides on git defaults; a patch may carry the
        // rename pair without a similarity estimate.
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
        let model = build_model(parse(text), scan_files(text), DiffPolicies::default());
        assert!(model.pure_rename());
        assert_eq!(pane_kind(&scan_files(text)), PaneKind::Text);
        assert_eq!(model.hunk_count(), 0);
        // One file section: the plumbing is suppressed, so the synthesized
        // rename header is the section's only row.
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
        // A .png section is an image pair by sniff, not a bare binary pane.
        assert_eq!(pane_kind(&scan_files(binary)), PaneKind::Image);

        let bin_txt = "diff --git a/data.bin b/data.bin\n\
                       index 1111111..2222222 100644\n\
                       Binary files a/data.bin and b/data.bin differ\n";
        assert_eq!(pane_kind(&scan_files(bin_txt)), PaneKind::Binary);

        // A binary section among text sections stays inline: the full-pane
        // replacement is only for the single-file case (ADR-0015).
        let mixed = format!("{bin_txt}diff --git a/x.rs b/x.rs\n@@ -1 +1 @@\n-a\n+b\n");
        let files = scan_files(&mixed);
        assert_eq!(files.len(), 2);
        assert!(files[0].binary);
        assert!(!files[1].binary);
        assert_eq!(pane_kind(&files), PaneKind::Text);
    }

    #[test]
    fn diff_rename_header_keeps_hunk_navigation_aligned() {
        let model = build_model(
            parse(RENAME_WITH_SIMILARITY),
            scan_files(RENAME_WITH_SIMILARITY),
            DiffPolicies::default(),
        );
        assert_eq!(model.hunk_count(), 1);
        // One file section: the plumbing is suppressed and the synthesized
        // rename header leads as display row 0, so the hunk header is the very
        // next row — the slot the per-frame paint plan aims hunk navigation at
        // (issue 20).
        assert_eq!(model.hunk_first_display[0], 1);
    }

    /// The rule is a fact about the display model, not the painter: a pane
    /// showing exactly one file section has no plumbing block, because the
    /// pane's own chrome names the file. Every spelling git's header block can
    /// take is asserted gone, and the hunk bodies keep their span-derived
    /// gutter numbers.
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
        // Numbering derives from the hunk span, not from the header rows the
        // walk used to advance past: the 1-based new-side numbers stand.
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
        // Lines before the first `diff --git` (e.g. `git log -p` commit
        // headers) belong to no section.
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

    /// The pair arrives as two repo-relative names. Four readers
    /// used to split that pair out of the header themselves, and only one of
    /// them handled git's quoted form — which is why a name git quotes (a tab,
    /// a byte above ASCII) is the case to pin here rather than a name with a
    /// plain space, which git does not quote at all.
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
    fn diff_pane_kind_sniffs_image_extensions() {
        let meta = |old: &str, new: &str| FileMeta {
            old_path: Some(old.to_owned()),
            new_path: Some(new.to_owned()),
            ..FileMeta::default()
        };
        // Case-insensitive sniff on both sides.
        assert_eq!(pane_kind(&[meta("x.PNG", "x.png")]), PaneKind::Image);
        // SVG never decodes: a binary-flagged SVG change stays binary…
        let mut svg_bin = meta("logo.svg", "logo.svg");
        svg_bin.binary = true;
        assert_eq!(pane_kind(&[svg_bin]), PaneKind::Binary);
        // …and a textual SVG diff keeps its text rows.
        assert_eq!(pane_kind(&[meta("logo.svg", "logo.svg")]), PaneKind::Text);
        // A non-image text change stays text.
        assert_eq!(pane_kind(&[meta("x.rs", "x.rs")]), PaneKind::Text);
        // No section metadata at all → text rows.
        assert_eq!(pane_kind(&[]), PaneKind::Text);
    }
}
