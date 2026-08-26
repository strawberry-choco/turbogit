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
//! - **Non-text diffs (spec R8, ADR-0015)**: per-file section metadata is
//!   scanned beside the rows; rename metadata renders as a leading header
//!   row (a pure 100% rename shows "No content changes." instead of an
//!   empty scroller), an image pair renders as decoded pictures with
//!   dimension/size captions, and a lone binary change renders as a
//!   one-line description with byte sizes. Non-text bytes fetch off the
//!   frame path through the same async-event seam as diffs and cache
//!   decoded results beside them.

use crate::core::diff_engine;
use crate::core::granular::{self, comparison_triple, diff_key};
use crate::engine::{AppEvent, DecodedImage, FetchedBlob, GitExecutor};
use crate::model::{ChangeStatus, DiffOpts};
use crate::state::{AppState, DiffComparison};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, Response, ScrollArea,
    Sense, TextureOptions, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, VecDeque};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    Meta,
    /// Rename metadata (CONTEXT.md "Rename header"), synthesized from the
    /// section scan: leads the content as its own full-width display row.
    RenameHeader,
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
    /// Raw ordinal in the display model — parse order plus any leading
    /// rename-header row — the unified-mode paging index (ADR-0014):
    /// unified windows are ranges over these ordinals.
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
struct FileMeta {
    /// Path before the change, from the `diff --git a/… b/…` line.
    old_path: Option<String>,
    /// Path after the change, from the same line.
    new_path: Option<String>,
    /// `similarity index N%` when git emitted a rename estimate.
    similarity: Option<u32>,
    /// `rename from`/`rename to` headers were present.
    renamed: bool,
    /// `new file mode` header was present.
    new_file: bool,
    /// `deleted file mode` header was present.
    deleted_file: bool,
    /// The section body carries `Binary files … differ` (no textual hunks).
    binary: bool,
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
enum PaneKind {
    Text,
    Image,
    Binary,
}

/// Extensions decoded as images (CONTEXT.md "Image diff"). SVG is
/// deliberately absent — never decoded (ADR-0015); an SVG change renders
/// as whatever the patch says (text rows or binary placeholder).
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// Per-image raw-byte cap: over-cap sides fall back to the binary change.
const IMAGE_CAP_BYTES: u64 = 20 * 1024 * 1024;

/// Decoded-pixel guard: a small blob can still explode into hundreds of MB
/// of RGBA; beyond this a side counts as undecodable (binary fallback).
const IMAGE_MAX_PIXELS: u64 = 80_000_000;

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
fn pane_kind(files: &[FileMeta]) -> PaneKind {
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
fn repo_rel_path(path: &str) -> &str {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
}

/// Where one side's bytes come from — mirroring exactly how the viewer's
/// diff text is requested ([`comparison_triple`] + `CliExecutor::diff`):
///
/// | comparison       | old side   | new side     |
/// |------------------|------------|--------------|
/// | Repo (HEAD↔wt)   | `HEAD`     | worktree fs  |
/// | Staged (HEAD↔ix) | `HEAD`     | index `:0`   |
/// | Local (ix↔wt)    | index `:0` | worktree fs  |
/// | explicit l..r    | `<left>`   | `<right>`    |
///
/// Index revs use git's stage syntax (`:<n>:<path>`), the same style the
/// conflict reader uses for `:1`/`:2`/`:3`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SideSpec {
    Rev(String),
    Worktree,
    Missing,
}

/// Byte sources for `(old, new)` of one file section. New files have no old
/// side; deleted files no new side — those render as the single-image case.
fn byte_side_specs(
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    meta: &FileMeta,
) -> (SideSpec, SideSpec) {
    let old = if meta.new_file {
        SideSpec::Missing
    } else {
        match left {
            Some(l) => SideSpec::Rev(l.clone()),
            None if staged => SideSpec::Rev("HEAD".to_owned()),
            None => SideSpec::Rev(":0".to_owned()),
        }
    };
    let new = if meta.deleted_file {
        SideSpec::Missing
    } else {
        match right {
            Some(r) => SideSpec::Rev(r.clone()),
            None if staged => SideSpec::Rev(":0".to_owned()),
            None => SideSpec::Worktree,
        }
    };
    (old, new)
}

/// Human-readable byte size ("0 B", "512 B", "1.2 KB", "12 MB") — decimal
/// units; one decimal below 10 of a unit, whole numbers from there.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    // Rounding must never print "1000.0 KB" — promote once more instead.
    if value.round() >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    let name = UNITS[unit];
    if unit == 0 || value >= 10.0 {
        format!("{value:.0} {name}")
    } else {
        format!("{value:.1} {name}")
    }
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
/// index maps hunk navigation — and R7 keyboard nav — scroll by, and the
/// per-file section metadata R8 renders from.
struct DiffModel {
    display: Vec<DisplayRow>,
    /// Underlying parsed-row count: the unified-mode `show_rows` total
    /// (pairing ignored, each underlying row = one paging slot). Includes
    /// any leading rename-header row.
    raw_count: usize,
    /// Underlying-row ordinal → display index, so a unified window over raw
    /// ordinals finds the (possibly mid-pair) display elements covering it.
    raw_to_display: Vec<u32>,
    /// Hunk ordinal → first display index of its header row.
    hunk_first_display: Vec<u32>,
    /// Per-file section metadata (spec R8), scanned once beside the rows.
    files: Vec<FileMeta>,
    /// Formatted rename header (CONTEXT.md "Rename header") of the first
    /// renamed section, when any — rendered as the leading display row.
    rename_header: Option<String>,
}

impl DiffModel {
    /// Number of parsed hunks.
    fn hunk_count(&self) -> usize {
        self.hunk_first_display.len()
    }

    /// Whether the open diff is a pure rename: a detected rename with 100%
    /// similarity, no content hunks, and no file-mode creation/deletion —
    /// the header plus a "no content changes" note stands in for the empty
    /// scroller (spec R8).
    fn pure_rename(&self) -> bool {
        self.hunk_count() == 0
            && self.files.first().is_some_and(|f| {
                f.renamed && f.similarity == Some(100) && !f.new_file && !f.deleted_file
            })
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
fn diff_model(text: &str) -> Rc<DiffModel> {
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

// --- non-text pane bytes & textures (R8) -------------------------------------

/// One resolved side of a non-text pane: byte length plus the decoded
/// image when the side was decodable within the cap. The GPU texture is
/// built lazily at first paint and kept here so re-showing a file never
/// re-uploads. Constructed off the frame path via [`PaneSide::from_blob`].
pub struct PaneSide {
    pub byte_len: u64,
    pub image: Option<DecodedImage>,
    texture: Option<egui::TextureHandle>,
}

impl PaneSide {
    pub(crate) fn from_blob(blob: FetchedBlob) -> Self {
        Self {
            byte_len: blob.byte_len,
            image: blob.decoded,
            texture: None,
        }
    }
}

/// Both sides of one non-text pane result, keyed by load key.
#[derive(Default)]
pub struct PaneEntry {
    pub old: Option<PaneSide>,
    pub new: Option<PaneSide>,
}

/// Cache bound: a few entries cover back-and-forth file switching without
/// pinning every visited image in memory.
const PANE_CACHE_CAP: usize = 4;

/// Bounded cache of non-text pane results (CONTEXT.md "Root caches"
/// philosophy): invalidated wholesale with root refreshes through
/// [`crate::state::AppState::refresh`], never poked field-by-field; evicts
/// oldest beyond [`PANE_CACHE_CAP`].
#[derive(Default)]
pub struct PaneCache {
    map: HashMap<String, PaneEntry>,
    order: VecDeque<String>,
}

impl PaneCache {
    pub fn get(&self, key: &str) -> Option<&PaneEntry> {
        self.map.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut PaneEntry> {
        self.map.get_mut(key)
    }

    /// Insert (or replace), evicting the oldest entry past the cap.
    pub fn store(&mut self, key: String, entry: PaneEntry) {
        if !self.map.contains_key(&key) {
            self.order.push_back(key.clone());
            while self.order.len() > PANE_CACHE_CAP
                && let Some(evicted) = self.order.pop_front()
            {
                self.map.remove(&evicted);
            }
        }
        self.map.insert(key, entry);
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

/// Decode raw bytes into a [`DecodedImage`] when they are an in-cap image.
/// SVG can never reach this — the extension sniff gates decoding — and a
/// blob over [`IMAGE_CAP_BYTES`] or with more than [`IMAGE_MAX_PIXELS`]
/// counts as undecodable so the pane falls back to the binary change.
fn decode_image(bytes: &[u8]) -> Option<DecodedImage> {
    if bytes.len() as u64 > IMAGE_CAP_BYTES {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let (width, height) = (img.width(), img.height());
    if width as u64 * height as u64 > IMAGE_MAX_PIXELS {
        return None;
    }
    Some(DecodedImage {
        width,
        height,
        rgba: img.to_rgba8().into_raw(),
    })
}

/// Fetch one side's bytes (worker-thread only): engine blob for rev specs,
/// filesystem for the worktree — metadata-only when no decode is needed,
/// since the binary caption wants lengths, not content.
fn fetch_side(
    exec: &dyn GitExecutor,
    root: &std::path::Path,
    spec: &SideSpec,
    rel: &str,
    decode: bool,
) -> Option<FetchedBlob> {
    let bytes = match spec {
        SideSpec::Missing => return None,
        SideSpec::Worktree => {
            let full = root.join(rel);
            if !decode {
                let len = std::fs::metadata(full).ok()?.len();
                return Some(FetchedBlob {
                    byte_len: len,
                    decoded: None,
                });
            }
            std::fs::read(full).ok()?
        }
        SideSpec::Rev(rev) => exec
            .show_file_bytes(root, rev, std::path::Path::new(rel))
            .ok()?,
    };
    let byte_len = bytes.len() as u64;
    let decoded = decode.then(|| decode_image(&bytes)).flatten();
    Some(FetchedBlob { byte_len, decoded })
}

/// Dispatch the off-frame byte load for a non-text pane when nothing is in
/// flight and the result is not cached — one in-flight load per target,
/// mirroring `ensure_diff`. Returns whether an entry for `pane_key` is
/// already cached.
#[allow(clippy::too_many_arguments)]
fn ensure_pane_bytes(
    state: &mut AppState,
    pane_key: String,
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    meta: &FileMeta,
    decode: bool,
) -> bool {
    if state.ui.pane_bytes.get(&pane_key).is_some() {
        return true;
    }
    if state.ui.pane_bytes_loading.is_none() {
        let (old_spec, new_spec) = byte_side_specs(left, right, staged, meta);
        // Owned: the worker closure is 'static, the metadata borrow is not.
        let old_rel = meta
            .old_path
            .as_deref()
            .map(|p| repo_rel_path(p).to_owned())
            .unwrap_or_default();
        let new_rel = meta
            .new_path
            .as_deref()
            .map(|p| repo_rel_path(p).to_owned())
            .unwrap_or_default();
        let root = root.to_path_buf();
        let executor = state.executor.clone();
        let tx = state.tx.clone();
        state.ui.pane_bytes_loading = Some(pane_key.clone());
        std::thread::spawn(move || {
            let old = fetch_side(executor.as_ref(), &root, &old_spec, &old_rel, decode);
            let new = fetch_side(executor.as_ref(), &root, &new_spec, &new_rel, decode);
            let _ = tx.send(AppEvent::FileBytesReady {
                key: pane_key,
                old,
                new,
            });
        });
    }
    false
}

/// Resolved byte lengths for the binary caption, when both sides resolved.
fn pane_byte_lens(state: &AppState, pane_key: &str) -> Option<(u64, u64)> {
    let entry = state.ui.pane_bytes.get(pane_key)?;
    Some((entry.old.as_ref()?.byte_len, entry.new.as_ref()?.byte_len))
}

/// Fit `tex` inside `max`, preserving aspect ratio, never upscaling past
/// the natural pixel size (crisp beats blurry).
fn fitted(tex: Vec2, max: Vec2) -> Vec2 {
    if tex.x <= 0.0 || tex.y <= 0.0 {
        return Vec2::ZERO;
    }
    let scale = (max.x / tex.x).min(max.y / tex.y).min(1.0);
    tex * scale
}

/// Per-side caption under an image pane: `1920×1080 · 1.2 MB`.
fn image_caption(side: &PaneSide) -> String {
    match &side.image {
        Some(img) => format!(
            "{}×{} · {}",
            img.width,
            img.height,
            human_size(side.byte_len)
        ),
        None => human_size(side.byte_len),
    }
}

/// One image cell: the texture fitted into `max`, caption centered below.
fn image_cell(ui: &mut Ui, side: &PaneSide, max: Vec2) {
    let tex = side
        .texture
        .as_ref()
        .expect("texture built before painting");
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(6.0);
        ui.add(egui::Image::new(tex).fit_to_exact_size(fitted(tex.size_vec2(), max)));
        ui.add_space(2.0);
        ui.colored_label(Palette::INK_2, image_caption(side));
    });
}

/// Image-diff pane (CONTEXT.md "Image diff", ADR-0015): the two decoded
/// versions side by side with dimension/size captions, replacing the row
/// view entirely — the mode toggle has no second layout to switch to and
/// no hunk affordances exist. Bytes fetch off-frame; while loading a
/// lightweight note stands in. Over-cap / undecodable / unreadable sides
/// fall back to the binary-change rendering with whatever sizes resolved;
/// a new/deleted file renders its single existing side.
#[allow(clippy::too_many_arguments)]
fn render_image_pane(
    ui: &mut Ui,
    state: &mut AppState,
    pane_key: &str,
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    meta: &FileMeta,
) {
    if !ensure_pane_bytes(
        state,
        pane_key.to_owned(),
        root,
        left,
        right,
        staged,
        meta,
        true,
    ) {
        centered_note(ui, "Loading image…");
        return;
    }
    let entry = state
        .ui
        .pane_bytes
        .get_mut(pane_key)
        .expect("entry cached above");
    let present: Vec<&PaneSide> = [entry.old.as_ref(), entry.new.as_ref()]
        .into_iter()
        .flatten()
        .collect();
    if present.is_empty() || present.iter().any(|s| s.image.is_none()) {
        // Nothing usable decoded: the binary-change fallback (sizes that
        // did resolve still show).
        let sizes = match (&entry.old, &entry.new) {
            (Some(o), Some(n)) => Some((o.byte_len, n.byte_len)),
            _ => None,
        };
        binary_placeholder(ui, sizes);
        return;
    }
    // Lazy GPU upload: decoding happened on the worker; each texture is
    // built once at first paint and kept in the cache entry.
    for (i, side) in [entry.old.as_mut(), entry.new.as_mut()]
        .into_iter()
        .flatten()
        .enumerate()
    {
        if side.texture.is_none()
            && let Some(img) = &side.image
        {
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [img.width as usize, img.height as usize],
                &img.rgba,
            );
            side.texture = Some(ui.ctx().load_texture(
                format!("diff-pane-{pane_key}-{i}"),
                color,
                TextureOptions::LINEAR,
            ));
        }
    }

    const CAPTION_H: f32 = 24.0;
    let avail_h = (ui.available_height() - CAPTION_H).max(ROW_H * 2.0);
    match (entry.old.as_ref(), entry.new.as_ref()) {
        (Some(old), Some(new)) => {
            ui.columns(2, |cols| {
                let w0 = cols[0].available_width();
                let w1 = cols[1].available_width();
                image_cell(&mut cols[0], old, Vec2::new(w0, avail_h));
                image_cell(&mut cols[1], new, Vec2::new(w1, avail_h));
            });
        }
        // Single-sided (new / deleted file): the lone image, centered.
        // Covers (None, None) too — the empty-entry fallback above already
        // returned.
        (side, None) | (None, side) => {
            if let Some(side) = side {
                let width = ui.available_width();
                image_cell(ui, side, Vec2::new(width, avail_h));
            }
        }
    }
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
        // Phase L1: when the setting asks for in-process diffs, the worker
        // computes the patch with `similar`; `diff_text` itself falls back to
        // this same CLI call whenever the in-process path cannot produce it
        // (multi-file targets, unreadable sides, non-UTF-8 content).
        let in_process = state.settings.in_process_diffs;
        let opts = DiffOpts {
            staged,
            ignore_whitespace,
            left: left.clone(),
            right: right.clone(),
            path: path.clone(),
            ..DiffOpts::default()
        };
        std::thread::spawn(move || {
            let res = if in_process {
                diff_engine::diff_text(executor.as_ref(), &root, &opts)
            } else {
                executor.diff(&root, &opts)
            };
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

    // Non-text diffs render outside the display-row model (spec R8,
    // ADR-0015): a lone binary change or an image pair replaces the rows
    // entirely — the mode toggle has no second layout to switch to, and
    // with no hunks there is nothing to navigate or stage. The pane key
    // inherits the diff cache key, so any reload of the patch text (ops
    // invalidate it) also refetches the pane bytes.
    match pane_kind(&model.files) {
        PaneKind::Text => {}
        PaneKind::Binary => {
            let meta = &model.files[0];
            let pane_key = format!("{key}#bin");
            ensure_pane_bytes(
                state,
                pane_key.clone(),
                &root,
                &eff_left,
                &eff_right,
                staged,
                meta,
                false,
            );
            // While loading (or when a side is unreadable) the sizes are
            // unresolved and the bare description shows — the graceful
            // fallback text.
            let sizes = pane_byte_lens(state, &pane_key);
            binary_placeholder(ui, sizes);
            return;
        }
        PaneKind::Image => {
            let meta = &model.files[0];
            let pane_key = format!("{key}#img");
            render_image_pane(
                ui, state, &pane_key, &root, &eff_left, &eff_right, staged, meta,
            );
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
        ui.colored_label(Palette::INK_2, "No content changes.");
        return;
    }

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
    if !matches!(row.kind, RowKind::Meta | RowKind::RenameHeader) && resp.hovered() {
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
        RowKind::RenameHeader => paint_rename_header(painter, &rect, &row.text, font),
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

/// Paint the rename-header text inside a row-shaped band — shared by the
/// in-scroller display row and the pure-rename static band (spec R8).
fn paint_rename_header(painter: &egui::Painter, rect: &Rect, text: &str, font: &FontId) {
    paint_cell(
        painter,
        rect.left() + TEXT_X,
        rect,
        text,
        Palette::INK_2,
        font,
    );
}

/// Centered muted one-line note filling the remaining pane height.
fn centered_note(ui: &mut Ui, text: &str) {
    let width = ui.available_width();
    let height = ui.available_height().max(ROW_H * 3.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    paint_centered(
        ui.painter(),
        rect,
        text,
        FontId::new(12.0, FontFamily::Proportional),
        Palette::INK_2,
    );
}

/// Binary-change placeholder (CONTEXT.md "Binary change", ADR-0015):
/// replaces the row-based view for a single binary file section. `sizes`
/// carries the resolved `(before, after)` byte lengths from the same
/// off-frame sourcing path the image pane uses; `None` (still loading, or
/// a side unreadable) renders the bare description.
fn binary_placeholder(ui: &mut Ui, sizes: Option<(u64, u64)>) {
    let text = match sizes {
        Some((before, after)) => format!(
            "Binary file changed · {} → {}",
            human_size(before),
            human_size(after)
        ),
        None => "Binary file changed".to_owned(),
    };
    centered_note(ui, &text);
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
                RowKind::RenameHeader => {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                    rows_rect = Some(match rows_rect {
                        Some(union) => union.union(rect),
                        None => rect,
                    });
                    paint_rename_header(&painter, &rect, &row.text, &font);
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

    #[test]
    fn diff_byte_side_specs_mirror_diff_invocation() {
        let plain = FileMeta::default();
        let added = FileMeta {
            new_file: true,
            ..FileMeta::default()
        };
        let deleted = FileMeta {
            deleted_file: true,
            ..FileMeta::default()
        };

        // Repo chip (HEAD↔worktree): `git diff HEAD`.
        assert_eq!(
            byte_side_specs(&Some("HEAD".to_owned()), &None, false, &plain),
            (SideSpec::Rev("HEAD".to_owned()), SideSpec::Worktree)
        );
        // Staged chip (HEAD↔index): `git diff --cached`; index via stage-0.
        assert_eq!(
            byte_side_specs(&None, &None, true, &plain),
            (
                SideSpec::Rev("HEAD".to_owned()),
                SideSpec::Rev(":0".to_owned())
            )
        );
        // Local chip (index↔worktree): plain `git diff`.
        assert_eq!(
            byte_side_specs(&None, &None, false, &plain),
            (SideSpec::Rev(":0".to_owned()), SideSpec::Worktree)
        );
        // Explicit commit-to-commit targets pass their revs through.
        assert_eq!(
            byte_side_specs(
                &Some("abc123".to_owned()),
                &Some("def456".to_owned()),
                false,
                &plain
            ),
            (
                SideSpec::Rev("abc123".to_owned()),
                SideSpec::Rev("def456".to_owned())
            )
        );
        // New files have no old side; deleted files no new side.
        assert_eq!(
            byte_side_specs(&None, &None, false, &added).0,
            SideSpec::Missing
        );
        assert_eq!(
            byte_side_specs(&Some("HEAD".to_owned()), &None, false, &deleted).1,
            SideSpec::Missing
        );
    }

    #[test]
    fn diff_human_size_formats_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1000), "1.0 KB");
        assert_eq!(human_size(1500), "1.5 KB");
        // Rounding promotes instead of printing "1000.0 KB".
        assert_eq!(human_size(999_999), "1.0 MB");
        assert_eq!(human_size(900_000), "900 KB");
        assert_eq!(human_size(1_200_000), "1.2 MB");
        assert_eq!(human_size(12_000_000), "12 MB");
    }

    #[test]
    fn diff_image_caption_formats_dimensions() {
        let side = PaneSide {
            byte_len: 5,
            image: Some(DecodedImage {
                width: 1920,
                height: 1080,
                rgba: Vec::new(),
            }),
            texture: None,
        };
        assert_eq!(image_caption(&side), "1920×1080 · 5 B");
    }

    #[test]
    fn diff_decode_image_rejects_garbage_and_over_cap() {
        assert!(decode_image(b"not an image").is_none());
        let over_cap = vec![0u8; IMAGE_CAP_BYTES as usize + 1];
        assert!(decode_image(&over_cap).is_none());
    }

    #[test]
    fn diff_fetch_side_reads_rev_blob_and_worktree_file() {
        let png = {
            let img = image::DynamicImage::new_rgb8(2, 3);
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
            buf.into_inner()
        };
        let exec = crate::engine::fake::FakeExecutor::new();
        exec.files_bytes
            .lock()
            .unwrap()
            .insert(std::path::PathBuf::from("art.png"), png.clone());

        // Rev side through the engine seam (`<rev>:<path>`).
        let blob = fetch_side(
            &exec,
            std::path::Path::new("/irrelevant"),
            &SideSpec::Rev("HEAD".to_owned()),
            "art.png",
            true,
        )
        .expect("rev side readable");
        assert_eq!(blob.byte_len, png.len() as u64);
        let decoded = blob.decoded.expect("png decodes");
        assert_eq!((decoded.width, decoded.height), (2, 3));

        // Worktree side reads the file when decoding…
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("art.png"), &png).unwrap();
        let blob = fetch_side(&exec, dir.path(), &SideSpec::Worktree, "art.png", true)
            .expect("worktree side readable");
        assert_eq!(blob.decoded.as_ref().map(|d| d.width), Some(2));

        // …and uses fs metadata only when lengths suffice (binary caption).
        let blob = fetch_side(&exec, dir.path(), &SideSpec::Worktree, "art.png", false)
            .expect("worktree side measurable");
        assert_eq!(blob.byte_len, png.len() as u64);
        assert!(blob.decoded.is_none());

        // A missing side (new/deleted file) yields nothing.
        assert!(fetch_side(&exec, dir.path(), &SideSpec::Missing, "art.png", true).is_none());
    }

    #[test]
    fn diff_pane_cache_evicts_oldest_beyond_cap() {
        let mut cache = PaneCache::default();
        for i in 0..6 {
            cache.store(format!("k{i}"), PaneEntry::default());
        }
        assert_eq!(cache.len(), PANE_CACHE_CAP);
        assert!(cache.get("k0").is_none());
        assert!(cache.get("k1").is_none());
        for i in 2..6 {
            assert!(cache.get(&format!("k{i}")).is_some());
        }
        cache.clear();
        assert_eq!(cache.len(), 0);
    }
}
