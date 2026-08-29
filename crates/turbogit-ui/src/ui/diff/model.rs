//! Diff display model: row parsing, per-file section metadata, and the
//! memoized display-row model the view pages over (ADR-0014, spec R8).

use egui::{FontFamily, FontId};
use std::cell::RefCell;
use std::rc::Rc;

// --- metrics -----------------------------------------------------------------

/// Rendered height of one diff line.
pub(super) const ROW_H: f32 = 22.0;
/// Side-by-side pane header height (spec §8.4).
pub(super) const PANE_HEADER_H: f32 = 28.0;
/// Width of the +/- sign column in a unified row.
pub(super) const SIGN_W: f32 = 16.0;
/// Width of the line-number gutter column.
pub(super) const NUM_W: f32 = 40.0;
/// X offset of the code text within a unified row.
pub(super) const TEXT_X: f32 = SIGN_W + NUM_W + 12.0;

pub(super) fn mono_font() -> FontId {
    FontId::new(12.0, FontFamily::Monospace)
}
// --- row model ---------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowKind {
    Meta,
    /// Rename metadata (CONTEXT.md "Rename header"), synthesized from the
    /// section scan: leads the content as its own full-width display row.
    RenameHeader,
    Hunk,
    Context,
    Del,
    Add,
}

pub(super) struct Row {
    pub(super) kind: RowKind,
    pub(super) text: String,
    /// Owning hunk index — the header's own ordinal for `Hunk` rows, and
    /// the enclosing hunk for body rows (aiming the current hunk from the
    /// pointer, spec R2). Meta rows own no hunk.
    pub(super) hunk: usize,
    /// 0-based ordinal over the hunk's +/- lines in order (changed rows
    /// only) — exactly [`turbogit_services::partial::HunkSelection::Lines`]
    /// semantics for sub-hunk selection (spec R2 story 3).
    pub(super) line_ord: usize,
    /// 1-based old-file line number (0 when not applicable).
    pub(super) old_no: usize,
    /// 1-based new-file line number (0 when not applicable).
    pub(super) new_no: usize,
    /// Raw ordinal in the display model — parse order plus any leading
    /// rename-header row — the unified-mode paging index (ADR-0014):
    /// unified windows are ranges over these ordinals.
    pub(super) ord: usize,
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
    // Raw paging ordinals are assigned by [`build_model`] over the
    // header-prefixed space (ADR-0014, spec R8).
    rows
}

/// Normalized view of one parsed row (Phase L1 parity seam): the kind tag,
/// display text, and gutter numbers — exactly what rendering keys on,
/// flattened for cross-module comparison.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowSummary {
    /// `"meta" | "hunk" | "context" | "del" | "add"`.
    pub kind: &'static str,
    pub text: String,
    pub old_no: usize,
    pub new_no: usize,
}

/// Row stream [`parse`] builds from unified-diff text, normalized into
/// [`RowSummary`]s. Doc-hidden test seam: lets integration tests assert
/// in-process (`similar`) vs CLI (`git diff`) row-stream equality without
/// exposing the renderer's private model.
#[doc(hidden)]
pub fn parsed_rows(text: &str) -> Vec<RowSummary> {
    parse(text)
        .into_iter()
        .map(|row| RowSummary {
            kind: match row.kind {
                RowKind::Meta | RowKind::RenameHeader => "meta",
                RowKind::Hunk => "hunk",
                RowKind::Context => "context",
                RowKind::Del => "del",
                RowKind::Add => "add",
            },
            text: row.text,
            old_no: row.old_no,
            new_no: row.new_no,
        })
        .collect()
}
// --- per-file section metadata (R8) ------------------------------------------

/// Metadata for one `diff --git` file section (spec R8): paths, rename
/// detection, file-mode changes, and binary-ness — derived purely from the
/// patch text, never the engine. Rename detection rides on git's own
/// defaults, so rename/similarity headers may be absent; absence is handled
/// gracefully.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FileMeta {
    /// Path before the change, from the `diff --git a/… b/…` line.
    pub(super) old_path: Option<String>,
    /// Path after the change, from the same line.
    pub(super) new_path: Option<String>,
    /// `similarity index N%` when git emitted a rename estimate.
    pub(super) similarity: Option<u32>,
    /// `rename from`/`rename to` headers were present.
    pub(super) renamed: bool,
    /// `new file mode` header was present.
    pub(super) new_file: bool,
    /// `deleted file mode` header was present.
    pub(super) deleted_file: bool,
    /// The section body carries `Binary files … differ` (no textual hunks).
    pub(super) binary: bool,
}

/// Split the remainder of a `diff --git` line into `(old, new)` paths.
/// Handles git's quoted form for unusual paths; the plain `a/… b/…` form
/// splits at the first ` b/` separator.
fn split_git_paths(rest: &str) -> (Option<String>, Option<String>) {
    if let Some(first) = rest.strip_prefix('"')
        && let Some(end) = first.find('"')
    {
        let old = &first[..end];
        let tail = first[end + 1..].trim_start();
        if let Some(new) = tail.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
            return (Some(old.to_owned()), Some(new.to_owned()));
        }
    }
    match rest.find(" b/") {
        Some(sep) => (
            Some(rest[..sep].to_owned()),
            Some(rest[sep + 1..].to_owned()),
        ),
        None => (None, None),
    }
}

/// Scan the patch text into per-file section metadata: each `diff --git`
/// line opens a section that following headers refine. Lines before the
/// first section (e.g. `git log -p` commit headers) are ignored.
fn scan_files(text: &str) -> Vec<FileMeta> {
    let mut files = Vec::new();
    let mut current: Option<FileMeta> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(done) = current.take() {
                files.push(done);
            }
            let (old_path, new_path) = split_git_paths(rest);
            current = Some(FileMeta {
                old_path,
                new_path,
                ..FileMeta::default()
            });
        } else if let Some(meta) = current.as_mut() {
            if let Some(pct) = line
                .strip_prefix("similarity index ")
                .and_then(|v| v.trim().strip_suffix('%'))
                .and_then(|v| v.parse().ok())
            {
                meta.similarity = Some(pct);
            } else if line.starts_with("rename from ") || line.starts_with("rename to ") {
                meta.renamed = true;
            } else if line.starts_with("new file mode") {
                meta.new_file = true;
            } else if line.starts_with("deleted file mode") {
                meta.deleted_file = true;
            } else if line.starts_with("Binary files ") && line.ends_with(" differ") {
                meta.binary = true;
            }
        }
    }
    files.extend(current);
    files
}

/// Formatted rename-header line (CONTEXT.md "Rename header") for a file
/// section carrying rename metadata; `None` otherwise. The similarity
/// percentage is omitted when git emitted none.
fn rename_header_text(meta: &FileMeta) -> Option<String> {
    if !meta.renamed {
        return None;
    }
    let old = meta
        .old_path
        .as_deref()
        .map(repo_rel_path)
        .unwrap_or("(unknown)");
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
pub(super) enum PaneKind {
    Text,
    Image,
    Binary,
}

/// Extensions decoded as images (CONTEXT.md "Image diff"). SVG is
/// deliberately absent — never decoded (ADR-0015); an SVG change renders
/// as whatever the patch says (text rows or binary placeholder).
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// Per-image raw-byte cap: over-cap sides fall back to the binary change.
pub(super) const IMAGE_CAP_BYTES: u64 = 20 * 1024 * 1024;

/// Decoded-pixel guard: a small blob can still explode into hundreds of MB
/// of RGBA; beyond this a side counts as undecodable (binary fallback).
pub(super) const IMAGE_MAX_PIXELS: u64 = 80_000_000;

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
pub(super) fn pane_kind(files: &[FileMeta]) -> PaneKind {
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

/// Repo-relative form of a diff-side path: drops git's `a/`/`b/` prefix.
pub(super) fn repo_rel_path(path: &str) -> &str {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
}
// --- display-row model (ADR-0014) --------------------------------------------

/// One virtualized display row: the unit `ScrollArea::show_rows` pages
/// over. Side-by-side mode renders each element as one [`ROW_H`] band;
/// unified mode ignores pairing and flattens every contained row back into
/// its own band — both modes read from this one cached vector.
pub(super) enum DisplayRow {
    /// Full-width row: meta, hunk header, or context.
    Full(Row),
    /// A side-by-side pair sharing one [`ROW_H`] band: one Del and/or one
    /// Add (`None` fills the missing half with an empty cell).
    Pair(Option<Row>, Option<Row>),
}

/// Everything rendering needs from one diff, built once per diff text and
/// memoized beside `diff_cache` (ADR-0014): the display-row vector plus the
/// index maps hunk navigation — and R7 keyboard nav — scroll by, and the
/// per-file section metadata R8 renders from.
pub(super) struct DiffModel {
    pub(super) display: Vec<DisplayRow>,
    /// Underlying parsed-row count: the unified-mode `show_rows` total
    /// (pairing ignored, each underlying row = one paging slot). Includes
    /// any leading rename-header row.
    pub(super) raw_count: usize,
    /// Underlying-row ordinal → display index, so a unified window over raw
    /// ordinals finds the (possibly mid-pair) display elements covering it.
    pub(super) raw_to_display: Vec<u32>,
    /// Hunk ordinal → first display index of its header row.
    pub(super) hunk_first_display: Vec<u32>,
    /// Per-file section metadata (spec R8), scanned once beside the rows.
    pub(super) files: Vec<FileMeta>,
    /// Formatted rename header (CONTEXT.md "Rename header") of the first
    /// renamed section, when any — rendered as the leading display row.
    pub(super) rename_header: Option<String>,
}

impl DiffModel {
    /// Number of parsed hunks.
    pub(super) fn hunk_count(&self) -> usize {
        self.hunk_first_display.len()
    }

    /// Whether the open diff is a pure rename: a detected rename with 100%
    /// similarity, no content hunks, and no file-mode creation/deletion —
    /// the header plus a "no content changes" note stands in for the empty
    /// scroller (spec R8).
    pub(super) fn pure_rename(&self) -> bool {
        self.hunk_count() == 0
            && self.files.first().is_some_and(|f| {
                f.renamed && f.similarity == Some(100) && !f.new_file && !f.deleted_file
            })
    }

    /// First display-row index aiming at `hunk`: the paired-model index in
    /// side-by-side mode, otherwise the underlying-row ordinal of the header
    /// (unified pages over underlying rows). This is the shared hunk→row map
    /// ADR-0014 commits to — R7 keyboard nav (`F7`/`Shift+F7`) reuses it.
    pub(super) fn first_row_for_hunk(&self, hunk: usize, side_by_side: bool) -> Option<usize> {
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

/// Fold parsed rows plus the per-file section scan (spec R8) into the
/// cached display-row model (ADR-0014): consecutive Del/Add runs collapse
/// into paired display rows while meta/hunk/context stay full-width, and a
/// rename header — when the scan found one — leads as its own full-width
/// row. Also records the index maps so per-frame work is O(visible rows).
fn build_model(mut rows: Vec<Row>, files: Vec<FileMeta>) -> DiffModel {
    let mut display = Vec::new();
    let mut raw_to_display = Vec::with_capacity(rows.len() + 1);
    let mut hunk_first_display = Vec::new();

    // The rename header (CONTEXT.md) leads the content as its own display
    // row, so every index below — raw ordinals, raw_to_display, and
    // hunk_first_display — accounts for it and hunks still point at their
    // own content rows (spec R8, ADR-0014).
    let rename_header = files.iter().find_map(rename_header_text);
    if let Some(text) = &rename_header {
        raw_to_display.push(display.len() as u32);
        display.push(DisplayRow::Full(Row {
            kind: RowKind::RenameHeader,
            text: text.clone(),
            hunk: 0,
            line_ord: 0,
            old_no: 0,
            new_no: 0,
            ord: 0,
        }));
    }
    let offset = usize::from(rename_header.is_some());
    for (raw, row) in rows.iter_mut().enumerate() {
        row.ord = raw + offset;
    }
    let raw_count = rows.len() + offset;

    let mut pending: Vec<Row> = Vec::new();
    for row in rows {
        match row.kind {
            RowKind::Del | RowKind::Add => pending.push(row),
            RowKind::Meta | RowKind::RenameHeader | RowKind::Context => {
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
        files,
        rename_header,
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
pub(super) fn diff_model(text: &str) -> Rc<DiffModel> {
    PARSED_ROWS.with(|slot| {
        let mut memo = slot.borrow_mut();
        if !matches!(&*memo, Some((existing, _)) if existing == text) {
            *memo = Some((
                text.to_owned(),
                Rc::new(build_model(parse(text), scan_files(text))),
            ));
        }
        Rc::clone(&memo.as_ref().expect("memo filled above").1)
    })
}
#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(files[0].old_path.as_deref(), Some("a/src/old.rs"));
        assert_eq!(files[0].new_path.as_deref(), Some("b/src/new.rs"));
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
        let model = build_model(parse(text), scan_files(text));
        assert!(model.pure_rename());
        assert_eq!(pane_kind(&scan_files(text)), PaneKind::Text);
        assert_eq!(model.hunk_count(), 0);
        // The header is the sole leading display row; raw paging counts it.
        assert_eq!(model.raw_count, 5);
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
        );
        assert_eq!(model.hunk_count(), 1);
        // The header occupies raw ordinal 0 and display row 0; the hunk
        // header follows it in both index spaces.
        assert_eq!(model.raw_to_display[0], 0);
        assert_eq!(model.hunk_first_display[0], 7);
        assert_eq!(model.first_row_for_hunk(0, true), Some(7));
        assert_eq!(model.first_row_for_hunk(0, false), Some(7));

        // Every raw ordinal lands in the display model exactly once — the
        // invariant unified-mode paging relies on (ADR-0014).
        let mut ords = Vec::with_capacity(model.raw_count);
        for disp in &model.display {
            match disp {
                DisplayRow::Full(row) => ords.push(row.ord),
                DisplayRow::Pair(del, add) => {
                    for row in del.iter().chain(add.iter()) {
                        ords.push(row.ord);
                    }
                }
            }
        }
        ords.sort_unstable();
        assert_eq!(ords, (0..model.raw_count).collect::<Vec<_>>());
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
        assert_eq!(files[1].old_path.as_deref(), Some("a/gone.txt"));
    }

    #[test]
    fn diff_git_paths_parse_quoted_and_plain() {
        assert_eq!(
            split_git_paths("a/src/lib.rs b/src/lib.rs"),
            (
                Some("a/src/lib.rs".to_owned()),
                Some("b/src/lib.rs".to_owned())
            )
        );
        assert_eq!(
            split_git_paths("\"a/we ird\" \"b/ot her\""),
            (Some("a/we ird".to_owned()), Some("b/ot her".to_owned()))
        );
        assert_eq!(split_git_paths("nothing"), (None, None));
    }

    #[test]
    fn diff_pane_kind_sniffs_image_extensions() {
        let meta = |old: &str, new: &str| FileMeta {
            old_path: Some(old.to_owned()),
            new_path: Some(new.to_owned()),
            ..FileMeta::default()
        };
        // Case-insensitive sniff on both sides.
        assert_eq!(pane_kind(&[meta("a/x.PNG", "b/x.png")]), PaneKind::Image);
        // SVG never decodes: a binary-flagged SVG change stays binary…
        let mut svg_bin = meta("a/logo.svg", "b/logo.svg");
        svg_bin.binary = true;
        assert_eq!(pane_kind(&[svg_bin]), PaneKind::Binary);
        // …and a textual SVG diff keeps its text rows.
        assert_eq!(
            pane_kind(&[meta("a/logo.svg", "b/logo.svg")]),
            PaneKind::Text
        );
        // A non-image text change stays text.
        assert_eq!(pane_kind(&[meta("a/x.rs", "b/x.rs")]), PaneKind::Text);
        // No section metadata at all → text rows.
        assert_eq!(pane_kind(&[]), PaneKind::Text);
    }
}
