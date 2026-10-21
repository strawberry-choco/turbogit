//! The per-line staged oracle, and the cross-patch coordinate translation.
//!
//! This is the only module in the app allowed to reason about line identity
//! across two patches. [`derive_line_sets`] turns the two index-facing patch
//! values `ensure_hunk_stats` already holds into per-file line sets plus the two
//! appliable per-file sections, [`row_is_staged`] answers the one product
//! question a row click needs (is the line this row reports in the index?), and
//! [`row_source`] answers the operation question that follows it: which hunk and
//! which changed line of which appliable patch a displayed row is. Pure
//! derivation, no git call, and no answer for a line the app is not showing.
//!
//! ## The two numbering spaces
//!
//! A `+` line's number belongs to the patch that printed it, not to "the file".
//! `index ↔ worktree` numbers its `+` lines in **worktree** space, because the
//! worktree is that patch's post-image. `HEAD ↔ index` numbers its `+` lines in
//! **index** space, because the index is *its* post-image. Those two integers
//! describe the same text only until some earlier hunk added or removed a line,
//! and nothing at a call site makes one usable as the other. So every set here
//! is documented with the side whose image numbers it, and the sets are never
//! merged into one number line. [`LineSets::added`] is post-image space and
//! [`LineSets::removed`] is pre-image space, always, which is what lets the same
//! two fields carry three different spaces across the two patches.
//!
//! ## The translation is a hop, a locate, and one table
//!
//! [`FileLineSets::map`] moves one line number between two numbering spaces
//! using nothing but [`LineRuns`], and [`locate_line`] turns a number in one of
//! a retained section's two images into the `(hunk, ord)` pair
//! [`turbogit_services::partial::HunkSelection::Lines`] wants. [`row_source`]
//! is the entry point a click makes: it reads the row's kind and gutter, the
//! comparison the rows came from, and the toggle, hops when the two spaces
//! differ, and locates in the patch the toggle names.
//!
//! Nothing outside this module composes those steps, and nothing here reads
//! `Row::hunk` or `Row::line_ord`. Those two address the *displayed* patch, and
//! the displayed patch is never the appliable one. That separation is the
//! program's invariant, and it is why the answer is a `(hunk, ord)` of a
//! [`Source`] rather than a position in the rows the user is looking at.
//!
//! ## The ordinal, established by reading `partial.rs`
//!
//! `HunkSelection::Lines` ordinals and `Row::line_ord` are the same count, and
//! `diff_model`'s documentation of it survives reading the code that applies it.
//! In `partial.rs`'s `filter_lines` one `changed` counter is incremented for
//! every body line that is not context, in body order, across both sides at
//! once: a hunk holding `-a +A +B` numbers `a` as 0, `A` as 1, `B` as 2. So an
//! ordinal is not "the k-th deletion" and not "the k-th addition"; a `-` line
//! and a `+` line share one enumeration, and a context line consumes nothing.
//! `rows_from` in `diff_model.rs` keeps exactly that counter, which is why a
//! row's `line_ord` and a selection's ordinal agree with no translation between
//! them, and why [`locate_line`] must advance its counter for a changed line of
//! the other side too.
//!
//! `\ No newline at end of file` consumes no ordinal in either count. The parser
//! attaches it to the line it qualifies as `PatchLine::no_newline`, so it never
//! reaches a body walk as a line of its own. `rows_from` does paint an extra
//! `Meta` row after the qualified line, and that row is a painting artifact: it
//! is not a changed line, so it does not advance `changed`. A click's
//! `(hunk, ord)` therefore never mentions the marker, and a file whose last line
//! has no newline is not off by one.
//!
//! The hunk index is the other half, and here the documented claim is wrong.
//! `Selection`'s own comment says its keys are "the same order as the diff row
//! model's `Row.hunk`". `compose` indexes `file.hunks.iter().enumerate()`, so
//! its hunk index restarts at 0 for every file section of the patch handed to
//! it, while `rows_from`'s `hunk` counter is declared outside the file loop and
//! runs on across every file of the patch. The two coincide exactly when the
//! patch has one file section, which is true of every path-scoped pane read and
//! false of any whole-root patch. So [`locate_line`] reports the per-section
//! index `compose` wants, and a caller must not feed `compose` a row's
//! `Row::hunk` from a multi-file patch.
//!
//! ## Three answers, none of them `Option<usize>`
//!
//! [`Mapping`] is [`Mapping::Both`] or [`Mapping::OneImage`]: a number that
//! translates, versus a number whose line exists in only one of the hop's two
//! images, so the far space has no number for it. [`Ordinal`] is
//! [`Ordinal::At`], [`Ordinal::Context`], [`Ordinal::Untouched`] or
//! [`Ordinal::NoSection`]: a changed line with a real position, a line this
//! patch prints but changes nowhere, a line this patch never prints at all, and
//! a path this patch has no section for. Those are four different things a
//! caller can say, and one `Option<usize>` would make the first two
//! indistinguishable and the last two silent. [`NoSource`] is the row-level
//! reason and names which half declined.
//!
//! [`Space`] and [`Side`] keep the spaces honest without a runtime check: a
//! number is only ever handed to [`FileLineSets::map`], which picks the patch
//! that defines the hop itself (the `staged` sets for HEAD/index, the `local`
//! sets for index/worktree), so a caller cannot ask a staged patch to translate
//! a worktree number. [`Side`] is a patch's own pre-image or post-image and is
//! always legal, because which revision that is follows from the [`Source`].
//!
//! ## What retention costs, and why it is retention
//!
//! [`FileLineSets`] carries the `PatchFile` section of each appliable patch.
//! The alternative was a click that re-reads git: ticket 01 measured one
//! `git apply --cached` round trip at about 14.5 ms, 6.8 times the batched cost
//! for six lines, subprocess-spawn dominated. A click cannot compose from the
//! displayed value either, which is the point of this program. So the text the
//! root's cold fill already parsed is kept rather than thrown away.
//!
//! The cost is the diff text of every changed file of every root, in both
//! index-facing patches, for exactly as long as one `RootHunkStats` entry lives.
//! Measured on this tree's working diff (30 files, 139 hunks, 3,474 body lines,
//! nothing staged, so only the `local` sections are held): 136,300 bytes of line
//! text plus 32 bytes per `PatchLine`, plus the hunk and file records
//! (`PatchHunk` 80, `PatchFile` 104), which is about 265 KB against the 11.6 KB
//! of runs and 11.8 KB of hunk spans ticket 03 reported. That is 71 bytes per
//! body line counting only the text and the line structs, so a 200,000-line diff
//! would hold about 14 MB per patch where the runs held 1.4 MB, and a tree with
//! both a staged and an unstaged half pays it twice. It is bounded by the working
//! diff's own size, which is the text the user is reading; the pane's own
//! `DiffValue` already holds one file's `Patch` outright, so what is genuinely
//! new is the other changed files. The whole entry is dropped as a unit by every
//! refresh, so this is a cold-fill cost and never a growing one.
//!
//! Sections live behind [`Arc`] because `RootHunkStats` derives `Clone` and is
//! handed out by reference from the caches, so cloning the entry must not clone
//! a diff. [`PatchFile`] derives `PartialEq`, and `Arc`'s `PartialEq`
//! dereferences, so [`FileLineSets`] equality still compares section **content**:
//! the unit tests assert it across two separately parsed sections, which a
//! pointer-equality comparison would fail. A retained section is `None` when the
//! patch has no section for the path and when the path is unmerged, because a
//! `diff --cc` body numbers nothing and `git apply` refuses it, so there is no
//! such thing as an appliable section for a conflicted file.
//!
//! The hand-off shape: `partial::compose` takes `&Patch`, so a click that
//! composes from one retained section assembles a single-file `Patch` around it
//! (see [`section_as_patch`]). That is one deep copy of one file's text per
//! click and no git spawn. `Patch` holds `Vec<PatchFile>`, not
//! `Vec<Arc<PatchFile>>`, so the copy is the price of the current `services`
//! signature; a `compose_file(&PatchFile, &Selection)` would remove it.
//!
//! One hand-off hazard the tests found rather than the plan named: a located
//! `(hunk, ord)` is a correct address, not a promise that git takes the composed
//! patch, and what it takes depends on the direction. A forward stage composes
//! against the pre-image, which demotions and drops never change, so any single
//! ordinal is appliable and a stage keeps row granularity. A reverse unstage
//! composes against the post-image, where a dropped `+` erases a line the index
//! holds and a `-` demoted to HEAD's old text claims a line that is no longer
//! there. With `filter_lines`' reverse branch keeping an unselected staged line as
//! post-image context instead, single staged changes reverse cleanly; the one
//! thing git will not take is half of a replacement pair. So an unstage composes
//! the clicked line's whole change unit via [`unstage_ords`], never a whole hunk,
//! and every other change in that hunk stays individually reversible. This module
//! reports positions and the unit; the click that composes decides in
//! [`crate::granular`], and the accepted sets there are measured, not assumed.
//!
//! ## The answer's lifetime
//!
//! One [`RootHunkStats`](crate::root_caches::RootHunkStats) entry, and nothing
//! longer. The sets are computed in the same breath as the hunk spans, from the
//! three whole-root `diff_patch` reads of a cold root, and they go when that
//! entry goes: [`RootCaches::invalidate`](crate::root_caches::RootCaches::invalidate)
//! drops `hunk_stats` as one unit for
//! [`Affected::Root`](crate::root_caches::Affected::Root), which is the first
//! thing [`AppState::refresh`](crate::state::AppState::refresh) does for every
//! completed operation, a line stage included. The next `ensure_hunk_stats`
//! re-reads git and re-derives. So the answer is exactly as fresh as the index,
//! and its life is one operation.
//!
//! It must not ride along in the displayed diff value. Staging a line leaves
//! `git diff HEAD` byte-identical, so the display model that ADR-0014 folds at
//! settle time is built from the same bytes before and after the click. An
//! answer folded into that value could only be recomputed by re-folding
//! identical input, which makes it stale by construction, not by timing.
//!
//! ## Two states only, and a line staged and then edited again
//!
//! A line can be staged and then edited again in the worktree, so that the file
//! holds both the index's version and a further unstaged edit. The user-facing
//! model has two states, so the answer is never a third one. The constraint:
//! **a row whose displayed content is what the index holds reports staged**, and
//! the click that follows reverse-applies its staged occurrence. The further
//! worktree edit then stays unstaged, which is exactly what the combined view
//! shows for that line afterwards. This reads like a bug in ten months, so the
//! mechanism is spelled out: an addition row is unstaged iff its worktree number
//! appears in `index ↔ worktree`'s added set, and a line whose index content did
//! not change since the stage does not appear there however many neighbours of
//! it did. Conversely, a staged addition whose text the worktree then rewrote
//! *is* in that set, and correctly answers unstaged, because the row shows the
//! rewritten text, which the index does not hold.
//!
//! ## What is not a per-line question
//!
//! - **Unmerged paths** get no answer. The flag is git's own
//!   [`PatchFile::combined`], never path text, and it comes from `git diff`
//!   (index↔worktree), which prints `diff --cc <path>` for such a path. Two
//!   hazards live here. `git diff --cached` prints `* Unmerged path <path>` and
//!   no section at all, so the staged patch contributes nothing for the file.
//!   And `git diff HEAD`, the patch a row actually comes from, prints an
//!   unmerged path as an ordinary two-sided diff whose "added" lines are the
//!   conflict markers, so the displayed value carries no sign of the conflict
//!   and this answer is the only thing that can keep the controls inert.
//! - **Binary, mode-only and pure-rename sections** carry no hunks, so no line
//!   of them has a number in any space and no row can ask. A section present
//!   with four empty sets answers nothing, rather than the "staged" an empty
//!   added set would imply by accident. A rename *with* content changes does
//!   have lines, is keyed by the `b/` side path like every other set, and
//!   answers per line exactly as an ordinary modification does.
//! - **Untracked paths** are in no diff at all, so they are absent and get no
//!   answer. Intent-to-add changes that: the path then appears in
//!   `index ↔ worktree` with every worktree line as a `+`, and each row answers
//!   unstaged, which is the truth about a file the index holds only as a
//!   placeholder.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use turbogit_domain::model::{Patch, PatchFile, PatchLineKind, forward_slash_path};
use turbogit_services::hunk_stats::HunkSpan;

use crate::diff_model::{Row, RowKind};

/// Sorted, non-overlapping, merged inclusive runs of 1-based line numbers in
/// one patch's one numbering space.
///
/// Runs rather than a number per line: git emits a `+` line and its neighbour in
/// the same hunk as consecutive numbers, and the walk below visits them in
/// ascending order, so one run costs 16 bytes for however many lines it covers.
/// Measured over this repository's own working diff, 744 added lines sit in 218
/// runs and 1370 removed lines in 269 runs, so a run covers three to five lines
/// and the representation is about a third of what one number per line costs,
/// while losing nothing: [`LineRuns::contains`] is the same binary search a
/// sorted set gives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineRuns {
    /// Ascending inclusive `(first, last)` pairs, never touching or overlapping.
    runs: Vec<(usize, usize)>,
}

impl LineRuns {
    /// Add one line number, extending the last run when it is contiguous.
    ///
    /// Public because it is the only way to build a `LineRuns` outside this
    /// module, and because the merge rule is the derivation's own: a second copy
    /// of it elsewhere would be a second chance to get it wrong. The line-identity
    /// translation turned out to read these sets rather than add to them, so the
    /// first reason is the live one.
    pub fn push(&mut self, line: usize) {
        match self.runs.last_mut() {
            Some(last) if last.1 + 1 >= line => last.1 = last.1.max(line),
            _ => self.runs.push((line, line)),
        }
    }

    /// Whether `line` is covered. Binary search over the ascending run ends.
    pub fn contains(&self, line: usize) -> bool {
        let at = self.runs.partition_point(|(_, end)| *end < line);
        self.runs
            .get(at)
            .is_some_and(|(start, end)| *start <= line && *end >= line)
    }

    /// How many covered numbers are strictly below `line`.
    ///
    /// The translation's rank arithmetic needs the count rather than the
    /// membership test, and merged runs make it a sum over a handful of pairs
    /// instead of a walk of one number per line.
    fn count_below(&self, line: usize) -> usize {
        self.runs
            .iter()
            .map(|&(start, end)| {
                if start >= line {
                    0
                } else {
                    line.saturating_sub(start).min(end - start + 1)
                }
            })
            .sum()
    }

    /// The `nth` (1-based) positive integer this set does *not* cover.
    ///
    /// Walks the runs in ascending order, spending the gaps between them: the
    /// gap below a run holds every number between the previous run's end and
    /// this run's start, and past the last run every number above it is
    /// uncovered. Linear in the runs of one file, which is a handful, and it is
    /// asked once per click rather than once per line.
    fn nth_outside(&self, nth: usize) -> usize {
        let mut after = 0usize;
        let mut remaining = nth;
        for &(start, end) in &self.runs {
            let gap = start.saturating_sub(1).saturating_sub(after);
            if remaining <= gap {
                return after + remaining;
            }
            remaining -= gap;
            after = end;
        }
        after + remaining
    }

    /// The runs themselves, ascending.
    pub fn runs(&self) -> &[(usize, usize)] {
        &self.runs
    }

    /// How many line numbers are covered, which is not how many runs are.
    pub fn len(&self) -> usize {
        self.runs.iter().map(|(start, end)| end - start + 1).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

/// One file's changed line numbers within one patch, each side kept in the
/// numbering space of the image it names.
///
/// The pair is per patch, never per file overall: [`FileLineSets::local`]'s two
/// spaces are index and worktree, and [`FileLineSets::staged`]'s are HEAD and
/// index, so the same integer names a different line in each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineSets {
    /// Pre-image line numbers of the section's `-` lines.
    pub removed: LineRuns,
    /// Post-image line numbers of the section's `+` lines.
    pub added: LineRuns,
}

impl LineSets {
    fn is_blank(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// One file's per-line staged-ness facts, merged over the two patches that
/// reach the index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileLineSets {
    /// Repo-relative path with forward slashes, git's `b/` side: the same key
    /// [`turbogit_services::hunk_stats::FileHunks::path`] uses, so a renamed
    /// file is found under the name the pane shows.
    pub path: String,
    /// `index ↔ worktree` (`git diff`): `removed` numbers index lines, `added`
    /// numbers worktree lines. Empty when the file has no unstaged change.
    pub local: LineSets,
    /// `HEAD ↔ index` (`git diff --cached`): `removed` numbers HEAD lines,
    /// `added` numbers index lines. Empty when nothing about this file is
    /// staged.
    pub staged: LineSets,
    /// git's own flag for an unmerged path: no line of this file is
    /// classifiable, and the caller keeps its controls inert.
    pub conflicted: bool,
    /// The retained `index ↔ worktree` section: the body [`LineSets`] were
    /// counted out of, kept so a click can address a hunk and a changed line
    /// instead of spawning git. `None` when the patch has no section for this
    /// path, and always `None` for an unmerged path.
    pub local_section: Option<Arc<PatchFile>>,
    /// The retained `HEAD ↔ index` section, same rules as
    /// [`FileLineSets::local_section`], different numbering spaces.
    pub staged_section: Option<Arc<PatchFile>>,
}

impl FileLineSets {
    /// Whether this section carries any numbered line at all. A binary,
    /// mode-only or pure-rename section does not, and answering from its empty
    /// sets would be an accident.
    fn has_lines(&self) -> bool {
        !(self.local.is_blank() && self.staged.is_blank())
    }
}

/// Which of the two index-facing patches a section came from, and so which of
/// the two spaces its numbers live in.
#[derive(Clone, Copy)]
enum Origin {
    Staged,
    Local,
}

/// The per-file line sets and appliable sections of the two index-facing
/// patches of one root.
///
/// Ordered ascending by path, which makes the value say the same thing whatever
/// order the two patches listed their files in, and is the invariant [`find`]
/// searches against.
///
/// Both patches arrive by value, because each section is moved into the entry
/// rather than copied out of a borrowed patch. A tree's whole working diff
/// duplicated once more at fill time is the one part of retention here that is
/// avoidable, and the caller drops its `Patch` values the moment this returns.
pub fn derive_line_sets(staged: Patch, local: Patch) -> Vec<FileLineSets> {
    let mut by_path: HashMap<String, FileLineSets> = HashMap::new();
    collect(&mut by_path, local.files, Origin::Local);
    collect(&mut by_path, staged.files, Origin::Staged);
    let mut sets: Vec<FileLineSets> = by_path.into_values().collect();
    sets.sort_by(|a, b| a.path.cmp(&b.path));
    sets
}

/// Fold every file section of one patch into the entry for its path.
fn collect(by_path: &mut HashMap<String, FileLineSets>, files: Vec<PatchFile>, origin: Origin) {
    for file in files {
        let path = file.new_path.replace('\\', "/");
        let conflicted = file.combined;
        let entry = by_path.entry(path.clone()).or_insert_with(|| FileLineSets {
            path,
            ..Default::default()
        });
        if conflicted {
            entry.conflicted = true;
            continue;
        }
        number_section(
            &file,
            match origin {
                Origin::Staged => &mut entry.staged,
                Origin::Local => &mut entry.local,
            },
        );
        let section = Some(Arc::new(file));
        match origin {
            Origin::Staged => entry.staged_section = section,
            Origin::Local => entry.local_section = section,
        }
    }
}

/// The `-` and `+` line numbers of one section, counted exactly the way the
/// pane's own row builder counts them: each hunk restarts both counters from
/// its own `@@` starts, and each body line advances only the side it belongs
/// to, which is what makes a row's gutter number and this set the same integer.
fn number_section(file: &PatchFile, out: &mut LineSets) {
    for hunk in &file.hunks {
        let mut old_no = hunk.old_start.max(1);
        let mut new_no = hunk.new_start.max(1);
        for line in &hunk.lines {
            match line.kind {
                PatchLineKind::Added => {
                    out.added.push(new_no);
                    new_no += 1;
                }
                PatchLineKind::Removed => {
                    out.removed.push(old_no);
                    old_no += 1;
                }
                PatchLineKind::Context => {
                    old_no += 1;
                    new_no += 1;
                }
            }
        }
    }
}

/// The entry for `path`, asked in the repo-relative spelling the caches use.
///
/// A binary search, because [`derive_line_sets`] sorts what it emits and a row
/// ask happens per row, not per file: the linear scan
/// [`RootHunkStats::file`](crate::root_caches::RootHunkStats::file) does would
/// be paid once per visible line.
pub fn find<'a>(sets: &'a [FileLineSets], path: &Path) -> Option<&'a FileLineSets> {
    let want = forward_slash_path(path);
    let at = sets.partition_point(|f| f.path < want);
    sets.get(at).filter(|f| f.path == want)
}

/// Whether the line `row` reports has its content in the index, or nothing to
/// say about it.
///
/// The row is read from the patch of a `HEAD`-based comparison, which is the
/// pair the preview pins: an `Add` row's `new_no` is then a
/// **worktree** line number and a `Del` row's `old_no` is a **HEAD** line
/// number, and those are exactly the two spaces the rules below ask about.
///
/// The behaviour difference a later reader has to know: an addition row is
/// correct in either working-tree comparison, because both of their post-images
/// are the worktree, while a deletion row is not. Under an `index ↔ worktree`
/// read, a `Del` row's `old_no` is an *index* number, and
/// no rule here can tell the two apart, because a row carries a number and not a
/// space. Its content is in the index by definition there, so an `index ↔
/// worktree` pane has no staged removals to report. That is why this rule is only
/// right for rows read from the pinned `HEAD ↔ worktree` comparison, and why
/// [`Displayed::IndexVsWorktree`] exists for the tests that ask the other one.
///
/// - An **addition** row is unstaged iff its worktree line is one that
///   `index ↔ worktree` adds, since that patch's post-image *is* the worktree:
///   every worktree line appears in it either as context, meaning the index
///   holds it, or as a `+`, meaning it does not.
/// - A **deletion** row's removal is staged iff its HEAD line is one
///   `HEAD ↔ index` removes.
///
/// Pairing is positional, never by content: a line is its number in one space,
/// so a file that adds the same text twice in a row gets two independent
/// answers.
#[must_use]
pub fn row_is_staged(sets: &[FileLineSets], path: &Path, row: &Row) -> Option<bool> {
    line_is_staged(sets, path, row.kind, row.old_no, row.new_no)
}

/// [`row_is_staged`] asked of a row's three numbers instead of a `Row`.
///
/// The staging path addresses a displayed row by its kind and its two gutters,
/// because those are what survives leaving the display model, and it must not
/// fabricate a `Row` to ask the question. `Row` is not `Clone`, so a caller that
/// holds a row across a refresh holds a copy of these numbers instead.
#[must_use]
pub fn line_is_staged(
    sets: &[FileLineSets],
    path: &Path,
    kind: RowKind,
    old_no: usize,
    new_no: usize,
) -> Option<bool> {
    let file = find(sets, path)?;
    if file.conflicted || !file.has_lines() {
        return None;
    }
    match kind {
        RowKind::Add => Some(!file.local.added.contains(new_no)),
        RowKind::Del => Some(file.staged.removed.contains(old_no)),
        // A context row is in every side at once, and a meta, rename or hunk row
        // is not a line: neither is a staging question.
        _ => None,
    }
}

// --- coordinate translation --------------------------------------------------

/// Which revision a line number names.
///
/// Nothing else: a number is meaningless until one of these is named for it,
/// which is why [`FileLineSets::map`] and not arithmetic at the call site is the
/// only way to move one between spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// The commit `HEAD` points at. A root with no commit has no Head space to
    /// name at all, because the `HEAD` read is refused there, so no patch that
    /// numbers a line against it can ever be held.
    Head,
    /// The index.
    Index,
    /// The working tree.
    Worktree,
}

/// Which image of one appliable patch a numbering question is about.
///
/// Legal for every [`Source`] without exception, because every patch has exactly
/// two images; which revision each one is follows from the source, see
/// [`Source::space`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The side the patch removes from.
    Pre,
    /// The side the patch adds to.
    Post,
}

/// Which of the two appliable patches a click composes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `HEAD ↔ index` (`git diff --cached`), reverse-applied to take a line out
    /// of the index.
    Staged,
    /// `index ↔ worktree` (`git diff`), forward-applied to put a line into it.
    Local,
}

impl Source {
    /// Which space one image of this patch numbers.
    pub fn space(self, side: Side) -> Space {
        match (self, side) {
            (Source::Staged, Side::Pre) => Space::Head,
            (Source::Staged, Side::Post) => Space::Index,
            (Source::Local, Side::Pre) => Space::Index,
            (Source::Local, Side::Post) => Space::Worktree,
        }
    }
}

/// One line number of one patch space, asked for in the other.
///
/// Two outcomes, and the second is not a failure of the translation: it is the
/// answer for a line the patch changed, which has no number on the far side
/// because the far image does not contain the line. A `Result` would make the
/// ordinary case of clicking a changed line look like an error, and an
/// `Option<usize>` would say nothing about which side holds the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mapping {
    /// The same text, at this number on the far side.
    Both(usize),
    /// The line exists in exactly one of the hop's two images, so the far space
    /// holds no number for it. Asked pre to post it is one of the patch's `+`
    /// lines; asked post to pre, one of its `-` lines. Which one follows from
    /// the direction the caller asked, so the variant names neither.
    OneImage,
}

/// Move one line number between the two images of one patch.
///
/// The rank trick is the whole correctness of this function, so it is written
/// out. A line present in both images is the k-th line its own side does **not**
/// change, and its far-side number is the k-th number the far side does not
/// change either. Counting positions rather than offsets is what makes it land,
/// because the obvious shift `line + added - removed` answers a survivor sitting
/// under an insertion with the insertion's own number, and that number is a
/// member of `added`, so it names no post-image line at all.
///
/// A 0 `line` is the pane's "not applicable" gutter. It names no line in any
/// space, so it answers [`Mapping::OneImage`], which is the honest direction of
/// the lie: [`row_source`] reports [`NoSource::RowNumbersNothing`] before ever
/// asking, so a caller normally never reaches this arm.
#[must_use]
pub fn map_side(sets: &LineSets, from: Side, line: usize) -> Mapping {
    let (near, far) = match from {
        Side::Pre => (&sets.removed, &sets.added),
        Side::Post => (&sets.added, &sets.removed),
    };
    if line == 0 || near.contains(line) {
        return Mapping::OneImage;
    }
    let rank = (line - 1) - near.count_below(line);
    Mapping::Both(far.nth_outside(rank + 1))
}

/// Where one numbered line sits in a section, in the addressing
/// [`turbogit_services::partial::HunkSelection::Lines`] takes.
///
/// The four variants are the four things a caller has to be able to say, and the
/// pair that a `usize` option would collapse is `Context` against `Untouched`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ordinal {
    /// Changed line `ord` of hunk `hunk`, counted over the hunk's `+` and `-`
    /// lines in body order exactly as `partial.rs`'s `filter_lines` counts them.
    At { hunk: usize, ord: usize },
    /// The line is inside hunk `hunk` of this section and is context there: it
    /// has a number on both sides, and this patch changes nothing about it, so
    /// there is no ordinal to select.
    Context { hunk: usize },
    /// No hunk of this section covers the number. The line may well exist in the
    /// file; this patch never prints it. Kept distinct from
    /// [`Ordinal::Context`] on purpose, because a click on a line the appliable
    /// patch does not change and a click on a line it prints unchanged are
    /// different statements about the index.
    Untouched,
    /// This path has no section in this patch at all.
    NoSection,
}

/// The `(hunk, ord)` of line `line` of one image of one section.
///
/// Walks the retained bodies, restarting both counters at each hunk's own `@@`
/// starts exactly as `number_section` and the display model do, so the numbers
/// it compares against are the numbers the sets were counted out of. `hunk` is
/// the index inside *this* section, which is what `compose` enumerates; see the
/// module doc for why that is not `Row::hunk` of a multi-file patch.
#[must_use]
pub fn locate_line(file: &PatchFile, side: Side, line: usize) -> Ordinal {
    for (hunk, h) in file.hunks.iter().enumerate() {
        let mut old_no = h.old_start.max(1);
        let mut new_no = h.new_start.max(1);
        let mut ord = 0usize;
        for l in &h.lines {
            // Only the side being asked has a number for this line: a `-` line
            // is unnumbered on the post image and a `+` line unnumbered on the
            // pre. Its ordinal is spent either way, because one counter
            // enumerates both sides.
            let numbered = match (l.kind, side) {
                (PatchLineKind::Removed, Side::Pre) => Some(old_no),
                (PatchLineKind::Added, Side::Post) => Some(new_no),
                (PatchLineKind::Context, Side::Pre) => Some(old_no),
                (PatchLineKind::Context, Side::Post) => Some(new_no),
                _ => None,
            };
            if numbered == Some(line) {
                return if l.kind == PatchLineKind::Context {
                    Ordinal::Context { hunk }
                } else {
                    Ordinal::At { hunk, ord }
                };
            }
            if l.kind != PatchLineKind::Context {
                ord += 1;
            }
            match l.kind {
                PatchLineKind::Added => new_no += 1,
                PatchLineKind::Removed => old_no += 1,
                PatchLineKind::Context => {
                    old_no += 1;
                    new_no += 1;
                }
            }
        }
    }
    Ordinal::Untouched
}

impl FileLineSets {
    /// The retained section a click composes from, if that patch has one.
    pub fn section(&self, pick: Source) -> Option<&PatchFile> {
        match pick {
            Source::Staged => self.staged_section.as_deref(),
            Source::Local => self.local_section.as_deref(),
        }
    }

    /// One line number of one space, named in another.
    ///
    /// The pair picks the patch that defines the hop, so no wrong combination is
    /// expressible: HEAD and index are the `staged` patch's two spaces, index
    /// and worktree are `local`'s, and the HEAD-to-worktree pair is the
    /// composition of the two through the index.
    ///
    /// A space asked of itself is the identity, which is what lets
    /// [`row_source`] call this unconditionally: a row whose gutter already
    /// names the space the source patch wants hops through here and changes
    /// nothing. An empty set is a real answer in the same way, not a missing
    /// one: a file the stage did not touch holds identical HEAD and index
    /// numbers even when [`FileLineSets::staged_section`] is `None`, and the
    /// identity is the truth about it.
    ///
    /// Composition has one consequence a single hop does not: a HEAD line the
    /// staged patch removes has no index number, so it has no worktree number
    /// either, and the answer is [`Mapping::OneImage`] rather than a guess about
    /// where the text went. The same reads back from the other end for a worktree
    /// line the local patch added.
    #[must_use]
    pub fn map(&self, from: Space, to: Space, line: usize) -> Mapping {
        match (from, to) {
            (Space::Head, Space::Head)
            | (Space::Index, Space::Index)
            | (Space::Worktree, Space::Worktree) => Mapping::Both(line),
            (Space::Head, Space::Index) => map_side(&self.staged, Side::Pre, line),
            (Space::Index, Space::Head) => map_side(&self.staged, Side::Post, line),
            (Space::Index, Space::Worktree) => map_side(&self.local, Side::Pre, line),
            (Space::Worktree, Space::Index) => map_side(&self.local, Side::Post, line),
            (Space::Head, Space::Worktree) => match map_side(&self.staged, Side::Pre, line) {
                Mapping::Both(index) => map_side(&self.local, Side::Pre, index),
                Mapping::OneImage => Mapping::OneImage,
            },
            (Space::Worktree, Space::Head) => match map_side(&self.local, Side::Post, line) {
                Mapping::Both(index) => map_side(&self.staged, Side::Post, index),
                Mapping::OneImage => Mapping::OneImage,
            },
        }
    }

    /// Where one of this file's lines sits inside the patch a click composes
    /// from, in that patch's own numbering.
    #[must_use]
    pub fn locate(&self, pick: Source, side: Side, line: usize) -> Ordinal {
        match self.section(pick) {
            None => Ordinal::NoSection,
            Some(file) => locate_line(file, side, line),
        }
    }
}

/// The comparison the displayed rows were built from, which is what decides what
/// a row's gutter number names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Displayed {
    /// `HEAD ↔ worktree`, the comparison the preview pins: a
    /// `Del` row's `old_no` is a HEAD line and an `Add` row's `new_no` is a
    /// worktree line.
    HeadVsWorktree,
    /// `index ↔ worktree`, the unstaged working-tree diff, which is a staging
    /// source here and not what the pane shows: `old_no` is an INDEX line, and
    /// that is the gap [`row_is_staged`]'s doc says makes its deletion rule wrong
    /// for rows read this way. A `Del` row here still gets a translation, and the
    /// translation is honest about the index; the staged-ness ANSWER for such a
    /// row is the part that cannot be right.
    IndexVsWorktree,
}

/// Which way the row's toggle moves the line, and so which patch it composes
/// from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    /// The line is not in the index: forward-apply `index ↔ worktree`.
    Stage,
    /// The line is in the index: reverse-apply `HEAD ↔ index`.
    Unstage,
}

/// Why one row click has no operation to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoSource {
    /// The path is unmerged: git's own flag, and no line of it is classifiable
    /// or appliable.
    Conflicted,
    /// The path is in neither index-facing patch: untracked, or not changed.
    PathAbsent,
    /// The path is in one patch but absent from the one this toggle names, so
    /// the direction asked has no source here at all.
    SourceAbsent,
    /// A meta, hunk, rename or context row: not a line, so not a staging
    /// question.
    RowIsNotALine,
    /// The row kind's own gutter is 0, so it names no line in any space.
    RowNumbersNothing,
    /// The hop this click needs has no far-side number for the line: the row
    /// shows text that lives in only one image, so the patch it would compose
    /// from does not hold it.
    NoFarSide,
    /// The line is context at hunk `hunk` of the appliable patch: that patch
    /// changes nothing about it, so it has no ordinal.
    Context { hunk: usize },
    /// No hunk of the appliable patch covers the line's number in that patch.
    Untouched,
}

/// What one row click needs in order to compose its operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowSource {
    /// Select changed line `ord` of hunk `hunk` of [`Source`]'s retained section
    /// and apply it in the toggle's direction. Both numbers are per section, as
    /// `compose` enumerates them, and `source` says which section to hand it.
    Located {
        source: Source,
        hunk: usize,
        ord: usize,
    },
    /// No operation, for the reason named. The reason does not carry the source,
    /// because the toggle already decided it.
    Missing(NoSource),
}

/// Where a displayed row's line lives, in the patch a click can apply.
///
/// The one entry point a row click makes, and the only place in the app that
/// joins a displayed row to an appliable patch. It reads the row's kind and
/// gutter, never its `hunk` or `line_ord`: those address the displayed patch,
/// and the displayed patch is a reading value, not the operation's source.
///
/// The table below is the whole policy, and the rule behind it is one sentence:
/// a stage moves the worktree's text into the index, an unstage moves the
/// index's text back out. So under `Stage` an addition is a worktree line and a
/// deletion is an index line the worktree dropped, and under `Unstage` both rows
/// are lines the index holds. Under a HEAD-based pane the `Del` + `Unstage` and
/// `Add` + `Stage` pairs need no hop, because the row's gutter already names the
/// space the appliable patch numbers.
///
/// [`Toggle`] is the consequence of [`row_is_staged`], not a second question: the
/// oracle answers staged or unstaged, `Unstage` is the name for the staged answer,
/// and this function never re-derives it. A caller that asks the other toggle gets
/// a [`RowSource::Missing`], which is a statement about the index rather than a
/// bug in the arithmetic.
#[must_use]
pub fn row_source(
    sets: &[FileLineSets],
    path: &Path,
    row: &Row,
    displayed: Displayed,
    toggle: Toggle,
) -> RowSource {
    line_source(
        sets, path, row.kind, row.old_no, row.new_no, displayed, toggle,
    )
}

/// [`row_source`] asked of a row's three numbers instead of a `Row`.
///
/// Same reason as [`line_is_staged`]: the staging path holds the numbers a row
/// leaves behind, not the row, and it must not invent a display model to ask the
/// one question it has to ask.
#[must_use]
pub fn line_source(
    sets: &[FileLineSets],
    path: &Path,
    kind: RowKind,
    old_no: usize,
    new_no: usize,
    displayed: Displayed,
    toggle: Toggle,
) -> RowSource {
    let Some(file) = find(sets, path) else {
        return RowSource::Missing(NoSource::PathAbsent);
    };
    if file.conflicted {
        return RowSource::Missing(NoSource::Conflicted);
    }
    let displayed_pre = match displayed {
        Displayed::HeadVsWorktree => Space::Head,
        Displayed::IndexVsWorktree => Space::Index,
    };
    let (row_space, number, source, side) = match (kind, toggle) {
        (RowKind::Add, Toggle::Stage) => (Space::Worktree, new_no, Source::Local, Side::Post),
        (RowKind::Add, Toggle::Unstage) => (Space::Worktree, new_no, Source::Staged, Side::Post),
        (RowKind::Del, Toggle::Stage) => (displayed_pre, old_no, Source::Local, Side::Pre),
        (RowKind::Del, Toggle::Unstage) => (displayed_pre, old_no, Source::Staged, Side::Pre),
        _ => return RowSource::Missing(NoSource::RowIsNotALine),
    };
    if number == 0 {
        return RowSource::Missing(NoSource::RowNumbersNothing);
    }
    let want = source.space(side);
    let line = match file.map(row_space, want, number) {
        Mapping::Both(line) => line,
        Mapping::OneImage => return RowSource::Missing(NoSource::NoFarSide),
    };
    match file.locate(source, side, line) {
        Ordinal::At { hunk, ord } => RowSource::Located { source, hunk, ord },
        Ordinal::Context { hunk } => RowSource::Missing(NoSource::Context { hunk }),
        Ordinal::Untouched => RowSource::Missing(NoSource::Untouched),
        Ordinal::NoSection => RowSource::Missing(NoSource::SourceAbsent),
    }
}

/// The ordinals of the single change unit a reverse unstage must move together.
///
/// A row click that stages takes exactly the one located ordinal: git forward-
/// applies any subset out of the `index <-> worktree` section, so two identical
/// adjacent additions stage one at a time. A row click that *unstages* is
/// different, because `git apply --cached --reverse` matches the composed patch's
/// new side against the index, and the partial patch that a lone half of a
/// replacement pair produces is not a true statement about the index: selecting
/// only the `+` of `old -> NEW` reverse-deletes the line instead of restoring
/// HEAD's `old`, and selecting only the `-` leaves both texts in the index. The
/// smallest set that reverses a modification cleanly is its `-`/`+` pair.
///
/// A pair is a `Removed` line immediately followed by an `Added` line in the hunk
/// body, matched greedily left to right, so `-a +A -b +B` (two modifications
/// touching, no context between) is two pairs and each reverse-applies alone. A
/// run of same-kind lines is NOT a pair: an adjacent `+ +` or `- -` reverse-applies
/// one at a time (measured: reversing a single `+dup` of two identical staged
/// additions leaves the other staged), so per-row control survives wherever git
/// actually grants it, and the widening never reaches past one `-`/`+` pair into a
/// neighbouring change.
///
/// An input this function cannot locate declines: no section for `source`, no
/// hunk at `hunk`, and an `ord` outside that hunk's changed lines each answer
/// `None`. Returning `ord` alone would be the exact selection this doc exists to
/// refuse, a lone half of a pair, and it would reach git as a patch that rewrites
/// the index to the wrong text, so a caller gets no operation rather than a
/// plausible one. The reason this decline is not named the way the module's others
/// are (`Ordinal`, [`NoSource`]) is that every caller maps all of them to the same
/// no-op; a wider answer type would only invent a distinction nothing reads.
#[must_use]
pub fn unstage_ords(
    sets: &FileLineSets,
    source: Source,
    hunk: usize,
    ord: usize,
) -> Option<BTreeSet<usize>> {
    // Ordinals counted over non-context lines in body order, exactly as
    // `partial.rs`'s `filter_lines` counts them.
    let groups = |lines: &[turbogit_domain::model::PatchLine]| -> Vec<BTreeSet<usize>> {
        let changed: Vec<PatchLineKind> = lines
            .iter()
            .filter(|l| l.kind != PatchLineKind::Context)
            .map(|l| l.kind)
            .collect();
        let mut out: Vec<BTreeSet<usize>> = Vec::new();
        let mut i = 0usize;
        while i < changed.len() {
            let mut group = BTreeSet::new();
            if changed[i] == PatchLineKind::Removed
                && i + 1 < changed.len()
                && changed[i + 1] == PatchLineKind::Added
            {
                group.insert(i);
                group.insert(i + 1);
                i += 2;
            } else {
                group.insert(i);
                i += 1;
            }
            out.push(group);
        }
        out
    };
    let section = sets.section(source)?;
    let located = section.hunks.get(hunk)?;
    groups(&located.lines)
        .into_iter()
        .find(|g| g.contains(&ord))
}

/// Whether the located section hunk is a pure creation, the one reverse shape
/// that still needs a whole-hunk selection.
///
/// A staged new file prints a `--- /dev/null` section whose body is every line an
/// `Added`: its pre-image holds nothing. `git apply --cached --reverse` matches the
/// patch's new side against the index and rewrites to the old side, so reversing
/// part of a creation would have to leave a non-empty old side (the context the
/// other lines become) where git expects the empty pre-image, and it refuses with
/// `new file <path> depends on old contents`. Reversing the whole hunk empties the
/// file cleanly. That is the exact shape where the fine-grained reverse rule
/// cannot hold, and the only one: a section hunk with any context or removal line
/// has a real pre-image to reverse into and takes a single change.
#[must_use]
pub fn section_hunk_is_creation(sets: &FileLineSets, source: Source, hunk: usize) -> bool {
    sets.section(source)
        .and_then(|section| section.hunks.get(hunk))
        .is_some_and(|h| {
            !h.lines.is_empty() && h.lines.iter().all(|l| l.kind == PatchLineKind::Added)
        })
}

/// The `Patch` to hand `partial::compose` for one retained section.
///
/// `Patch` owns its sections as a `Vec<PatchFile>`, so nothing that borrows the
/// retained `Arc` can be handed to `compose`, and the only value that works is a
/// single-file patch built around a clone. One deep copy of one file's diff text
/// per click, and no git spawn: the cheaper error by the 14.5 ms ticket 01
/// measured. Named here so a caller cannot invent a worse hand-off, and because
/// the copy is the price of the current `services` signature, not of this
/// module's choice to retain `Arc`s. A `compose_file(&PatchFile, &Selection)`
/// would remove it.
#[must_use]
pub fn section_as_patch(sets: &FileLineSets, source: Source) -> Option<Patch> {
    Some(Patch {
        files: vec![sets.section(source)?.clone()],
    })
}

/// The hunks of one appliable section that a displayed hunk's line range covers.
///
/// A whole-hunk verb is addressed in the displayed patch, where one hunk of
/// `HEAD ↔ worktree` can hold rows that belong to several hunks of either
/// index-facing section, so the hunk the reader pointed at has to be resolved to
/// section hunks before anything is composed. That resolution is line identity
/// across two patches and lives here, beside the rest of it.
///
/// The two ranges compared are already in one space, which is why the pair is
/// picked by [`Source`] and not asked of the caller: a `Staged` section numbers
/// its pre-image in HEAD, and so does the displayed hunk's `old_start`; a
/// `Local` section numbers its post-image in the worktree, and so does the
/// displayed hunk's `new_start`. The other pair of numbers names a different
/// revision on each side and would compare an index line against a HEAD one.
///
/// Zero-length ranges stand for the single point at their start line, so a pure
/// insertion still touches the hunk it was added inside.
#[must_use]
pub fn covered_hunks(
    sets: &[FileLineSets],
    path: &Path,
    pick: Source,
    anchor: &HunkSpan,
) -> Vec<usize> {
    let Some(section) = find(sets, path).and_then(|file| file.section(pick)) else {
        return Vec::new();
    };
    section
        .hunks
        .iter()
        .enumerate()
        .filter(|(_, h)| {
            let (a_start, a_len, s_start, s_len) = match pick {
                Source::Staged => (anchor.old_start, anchor.old_lines, h.old_start, h.old_count),
                Source::Local => (anchor.new_start, anchor.new_lines, h.new_start, h.new_count),
            };
            let end = |start: usize, len: usize| if len == 0 { start } else { start + len - 1 };
            a_start <= end(s_start, s_len) && s_start <= end(a_start, a_len)
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one reader of git's diff syntax, the engine's, as every services
    /// unit test reaches it.
    fn patch(text: &str) -> Patch {
        turbogit_engine_api::patch::parse_patch(text)
    }

    fn row(kind: RowKind, old_no: usize, new_no: usize) -> Row {
        Row {
            kind,
            text: String::new(),
            hunk: 0,
            line_ord: 0,
            old_no,
            new_no,
            span: None,
            highlight: Default::default(),
        }
    }

    fn runs_of(lines: &[usize]) -> LineRuns {
        let mut out = LineRuns::default();
        for line in lines {
            out.push(*line);
        }
        out
    }

    /// `index ↔ worktree`: a removal whose index number is 6 and an addition
    /// whose worktree number is 20, chosen so the two spaces of one patch
    /// cannot be confused for one another.
    const LOCAL: &str = "\
diff --git a/f.txt b/f.txt
--- a/f.txt
+++ b/f.txt
@@ -5,2 +5,1 @@
 gone1
-gone2
@@ -20,1 +19,2 @@
 anchor
+added
";

    /// `HEAD ↔ index`: HEAD line 1 goes, index lines 1 and 2 arrive. Its `+`
    /// numbers are index-space, so `1` here is not the `1` of [`LOCAL`].
    const STAGED: &str = "\
diff --git a/f.txt b/f.txt
--- a/f.txt
+++ b/f.txt
@@ -1 +1,2 @@
-first
+here
+there
";

    #[test]
    fn line_runs_merge_contiguous_numbers_and_search_them() {
        let runs = runs_of(&[10, 11, 12, 20, 21, 40]);
        assert_eq!(runs.runs(), &[(10, 12), (20, 21), (40, 40)]);
        assert_eq!(runs.len(), 6);
        for covered in [10, 12, 20, 21, 40] {
            assert!(runs.contains(covered), "{covered} is covered");
        }
        for gap in [0, 1, 9, 13, 19, 22, 39, 41] {
            assert!(!runs.contains(gap), "{gap} is a gap");
        }
        assert_eq!(runs_of(&[7, 7]).runs(), &[(7, 7)], "a repeat adds nothing");
        assert!(LineRuns::default().is_empty());
        assert!(!LineRuns::default().contains(1));
    }

    #[test]
    fn derive_numbers_each_side_in_its_own_space() {
        let sets = derive_line_sets(patch(STAGED), patch(LOCAL));
        assert_eq!(sets.len(), 1);
        let one = &sets[0];
        assert_eq!(one.path, "f.txt");
        assert_eq!(one.local.removed.runs(), &[(6, 6)], "index-space 6");
        assert_eq!(one.local.added.runs(), &[(20, 20)], "worktree-space 20");
        assert_eq!(one.staged.removed.runs(), &[(1, 1)], "HEAD-space 1");
        assert_eq!(one.staged.added.runs(), &[(1, 2)], "index-space 1..2");
        assert!(!one.conflicted);
    }

    #[test]
    fn derive_merges_two_patches_and_keeps_a_one_sided_file_apart() {
        let staged_only = derive_line_sets(patch(STAGED), Patch::default());
        assert_eq!(staged_only.len(), 1);
        assert!(staged_only[0].local.is_blank(), "nothing unstaged");
        assert_eq!(staged_only[0].staged.added.runs(), &[(1, 2)]);

        let local_only = derive_line_sets(Patch::default(), patch(LOCAL));
        assert_eq!(local_only[0].staged, LineSets::default());
        assert_eq!(local_only[0].local.added.runs(), &[(20, 20)]);
        // Adjacent additions collapse into one run of the right length.
        let twice = patch(
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +1,3 @@\n base\n+here\n+here\n",
        );
        let sets = derive_line_sets(Patch::default(), twice);
        assert_eq!(sets[0].local.added.runs(), &[(2, 3)]);
        assert_eq!(sets[0].local.added.len(), 2, "two lines in one run");
    }

    #[test]
    fn a_combined_section_is_conflicted_and_numbers_nothing() {
        let text = "diff --cc conf.txt\nindex ba2906d,2299c37..0000000\n--- a/conf.txt\n+++ b/conf.txt\n@@@ -1,1 -1,1 +1,5 @@@\n+<<<<<<< HEAD\n +main\n++=======\n+ side\n++>>>>>>> side\n";
        let sets = derive_line_sets(Patch::default(), patch(text));
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].path, "conf.txt");
        assert!(sets[0].conflicted, "git's own flag, never path text");
        assert!(
            sets[0].local.added.is_empty() && sets[0].local.removed.is_empty(),
            "a two-column combined body is not a numbering of anything"
        );
        assert_eq!(
            row_is_staged(&sets, Path::new("conf.txt"), &row(RowKind::Add, 0, 2)),
            None
        );
    }

    #[test]
    fn a_section_without_lines_answers_nothing() {
        // A mode-only change: git's section carries headers and no hunks.
        let text = "diff --git a/script.sh b/script.sh\nold mode 100644\nnew mode 100755\n";
        let sets = derive_line_sets(patch(text), patch(text));
        assert!(!sets[0].has_lines());
        assert_eq!(
            row_is_staged(&sets, Path::new("script.sh"), &row(RowKind::Add, 0, 1)),
            None,
            "an empty added set must not read as staged"
        );
    }

    #[test]
    fn row_kind_chooses_its_own_gutter_and_the_two_rules_answer_apart() {
        let sets = derive_line_sets(patch(STAGED), patch(LOCAL));
        let ask = |kind, old_no, new_no| {
            row_is_staged(&sets, Path::new("f.txt"), &row(kind, old_no, new_no))
        };
        // Worktree line 20 is a `+` of `index ↔ worktree`: not in the index.
        assert_eq!(ask(RowKind::Add, 0, 20), Some(false));
        // Worktree line 19 is context there: the index holds it.
        assert_eq!(ask(RowKind::Add, 0, 19), Some(true));
        // HEAD line 1 is removed by `HEAD ↔ index`: its removal is staged.
        assert_eq!(ask(RowKind::Del, 1, 0), Some(true));
        // HEAD line 2 is not.
        assert_eq!(ask(RowKind::Del, 2, 0), Some(false));
        // Index line 6 answers nothing useful and is never HEAD line 6: the
        // deletion rule reads HEAD, and 6 is not in that set.
        assert_eq!(ask(RowKind::Del, 6, 0), Some(false));
        for kind in [
            RowKind::Context,
            RowKind::Meta,
            RowKind::Hunk,
            RowKind::RenameHeader,
        ] {
            assert_eq!(ask(kind, 6, 20), None, "{kind:?} is not a staging question");
        }
    }

    #[test]
    fn duplicate_adjacent_additions_are_paired_by_position() {
        let text = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +1,3 @@\n base\n+here\n+here\n";
        let both = derive_line_sets(patch(text), patch(text));
        let staged_only = derive_line_sets(patch(text), Patch::default());
        let ask = |sets: &[FileLineSets], new_no| {
            row_is_staged(sets, Path::new("f.txt"), &row(RowKind::Add, 0, new_no))
        };
        assert_eq!(ask(&both, 2), Some(false), "first copy is unstaged");
        assert_eq!(
            ask(&both, 3),
            Some(false),
            "the second copy is its own line, not a repeat of the first"
        );
        assert_eq!(
            ask(&both, 4),
            Some(true),
            "a third copy the index does hold"
        );
        assert_eq!(
            ask(&staged_only, 2),
            Some(true),
            "both copies staged: absent from the working-tree patch"
        );
        assert_eq!(ask(&staged_only, 3), Some(true));
    }

    #[test]
    fn paths_are_keyed_by_the_b_side_in_one_spelling() {
        let text = "diff --git a/src/old.rs b/src/new.rs\nsimilarity index 92%\nrename from src/old.rs\nrename to src/new.rs\n--- a/src/old.rs\n+++ b/src/new.rs\n@@ -1 +1 @@\n-a\n+b\n";
        let sets = derive_line_sets(patch(text), Patch::default());
        assert_eq!(sets.len(), 1, "one entry per path, not one per patch");
        assert_eq!(sets[0].path, "src/new.rs");
        assert_eq!(
            find(&sets, Path::new("src/new.rs")).map(|f| f.staged.added.runs()),
            Some(&[(1, 1)][..]),
            "the renamed file's added line, in index space"
        );
        assert_eq!(find(&sets, Path::new("src/old.rs")), None);
    }

    #[test]
    fn derive_sorts_by_path_because_find_searches_against_that_order() {
        // Listed in the patch in descending order, and merged from two patches.
        let text =
            "diff --git a/z.txt b/z.txt\n--- a/z.txt\n+++ b/z.txt\n@@ -1 +1,2 @@\n keep\n+zed\n";
        let other =
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,2 @@\n keep\n+alpha\n";
        let sets = derive_line_sets(patch(other), patch(text));
        assert_eq!(
            sets.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["a.txt", "z.txt"]
        );
        assert_eq!(
            find(&sets, Path::new("a.txt")).map(|f| f.staged.added.len()),
            Some(1)
        );
        assert_eq!(
            find(&sets, Path::new("z.txt")).map(|f| f.local.added.len()),
            Some(1)
        );
        assert_eq!(
            find(&sets, Path::new("m.txt")).map(|f| f.path.as_str()),
            None,
            "a path between two keys is not a hit on its neighbour"
        );
    }

    #[test]
    fn an_unknown_path_or_an_empty_derivation_answers_nothing() {
        let sets = derive_line_sets(patch(STAGED), patch(LOCAL));
        assert_eq!(
            row_is_staged(&sets, Path::new("other.txt"), &row(RowKind::Add, 0, 20)),
            None
        );
        assert_eq!(
            row_is_staged(&[], Path::new("f.txt"), &row(RowKind::Del, 1, 0)),
            None
        );
        assert!(derive_line_sets(Patch::default(), Patch::default()).is_empty());
    }

    // --- the coordinate translation ------------------------------------------

    /// A three-revision story the two patches agree about, so that one HEAD
    /// number, one index number and one worktree number name lines of one file:
    /// HEAD holds 8 lines (`alpha` to `hotel`), the index holds 7 (HEAD's
    /// `bravo` modified to `BRAVO` and `delta` deleted, both staged), and the
    /// worktree holds 9 (`echo` modified to `ECHO` and `ink`, `juliet` added,
    /// none of it staged).
    const RICH_STAGED: &str = "\
diff --git a/f.txt b/f.txt
--- a/f.txt
+++ b/f.txt
@@ -1,5 +1,4 @@
 alpha
-bravo
+BRAVO
 charlie
-delta
 echo
";

    /// The unstaged half of [`RICH_STAGED`]'s story: `index ↔ worktree`, whose
    /// pre-image is the index the staged patch just produced.
    const RICH_LOCAL: &str = "\
diff --git a/f.txt b/f.txt
--- a/f.txt
+++ b/f.txt
@@ -3,5 +3,7 @@
 charlie
-echo
+ECHO
 foxtrot
 golf
 hotel
+ink
+juliet
";

    /// git's own flag for an unmerged path, in the one form that carries it.
    const COMBINED: &str = "diff --cc conf.txt\nindex ba2906d,2299c37..0000000\n--- a/conf.txt\n+++ b/conf.txt\n@@@ -1,1 -1,1 +1,5 @@@\n+<<<<<<< HEAD\n +main\n++=======\n+ side\n++>>>>>>> side\n";

    /// The file entry of both [`RICH_STAGED`] and [`RICH_LOCAL`].
    fn rich_file() -> FileLineSets {
        find(
            &derive_line_sets(patch(RICH_STAGED), patch(RICH_LOCAL)),
            Path::new("f.txt"),
        )
        .expect("both patches list f.txt")
        .clone()
    }

    /// One hunk's changed-line ordinal as a `Selection`, the way a click uses it.
    fn select(hunk: usize, ord: usize) -> turbogit_services::partial::Selection {
        turbogit_services::partial::Selection {
            hunks: [(
                hunk,
                turbogit_services::partial::HunkSelection::Lines([ord].into_iter().collect()),
            )]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn map_moves_a_head_line_to_its_index_number_and_back() {
        let file = rich_file();
        // Lines above the staged edit keep their number in both spaces.
        assert_eq!(file.map(Space::Head, Space::Index, 1), Mapping::Both(1));
        assert_eq!(file.map(Space::Head, Space::Index, 3), Mapping::Both(3));
        // `echo` is HEAD 5 and index 4, and everything below shifts by the same
        // one line: HEAD 8 `hotel` is index 7.
        assert_eq!(file.map(Space::Head, Space::Index, 5), Mapping::Both(4));
        assert_eq!(file.map(Space::Head, Space::Index, 6), Mapping::Both(5));
        assert_eq!(file.map(Space::Head, Space::Index, 8), Mapping::Both(7));
        // The two HEAD lines the stage removes have no index number, which is
        // the answer a click on a staged removal has to be able to see.
        for head in [2, 4] {
            assert_eq!(
                file.map(Space::Head, Space::Index, head),
                Mapping::OneImage,
                "HEAD {head} is a removed line"
            );
        }
        // Back the other way: index 2 is `BRAVO`, an added line, so HEAD holds
        // no number for it, while index 7 is HEAD's 8.
        assert_eq!(file.map(Space::Index, Space::Head, 2), Mapping::OneImage);
        assert_eq!(file.map(Space::Index, Space::Head, 4), Mapping::Both(5));
        assert_eq!(file.map(Space::Index, Space::Head, 7), Mapping::Both(8));
        // A space asked of itself is the identity, which is what lets
        // [`row_source`] hop unconditionally.
        assert_eq!(file.map(Space::Index, Space::Index, 7), Mapping::Both(7));
    }

    #[test]
    fn map_moves_a_worktree_line_to_its_index_number_and_back() {
        let file = rich_file();
        assert_eq!(file.map(Space::Worktree, Space::Index, 1), Mapping::Both(1));
        assert_eq!(file.map(Space::Worktree, Space::Index, 5), Mapping::Both(5));
        assert_eq!(file.map(Space::Worktree, Space::Index, 7), Mapping::Both(7));
        // `ECHO` replaced an index line and `ink`, `juliet` are new text, so
        // none of the three has an index number.
        for worktree in [4, 8, 9] {
            assert_eq!(
                file.map(Space::Worktree, Space::Index, worktree),
                Mapping::OneImage,
                "worktree {worktree} is an added line"
            );
        }
        assert_eq!(
            file.map(Space::Index, Space::Worktree, 4),
            Mapping::OneImage,
            "index 4 is `echo`, which the worktree no longer holds"
        );
        assert_eq!(file.map(Space::Index, Space::Worktree, 5), Mapping::Both(5));
        assert_eq!(file.map(Space::Index, Space::Worktree, 7), Mapping::Both(7));
    }

    /// The round trip the plan asks for, over every line of both patches and of
    /// the composition: a line that maps forward maps back to the same number.
    #[test]
    fn every_line_that_exists_on_both_sides_maps_forward_and_back_to_itself() {
        let file = rich_file();
        for (from, to, last) in [
            (Space::Head, Space::Index, 8),
            (Space::Index, Space::Head, 7),
            (Space::Worktree, Space::Index, 9),
            (Space::Index, Space::Worktree, 7),
            (Space::Head, Space::Worktree, 8),
            (Space::Worktree, Space::Head, 9),
        ] {
            for line in 1..=last {
                if let Mapping::Both(far) = file.map(from, to, line) {
                    assert_eq!(
                        file.map(to, from, far),
                        Mapping::Both(line),
                        "{from:?} {line} -> {to:?} {far} -> back"
                    );
                }
            }
        }
    }

    #[test]
    fn map_composes_head_to_worktree_through_the_index() {
        let file = rich_file();
        // HEAD 6 `foxtrot`: the stage removed a line above it and the worktree
        // replaced one above it, so its three numbers are 6, 5 and 5.
        assert_eq!(file.map(Space::Head, Space::Index, 6), Mapping::Both(5));
        assert_eq!(file.map(Space::Index, Space::Worktree, 5), Mapping::Both(5));
        assert_eq!(file.map(Space::Head, Space::Worktree, 6), Mapping::Both(5));
        assert_eq!(file.map(Space::Worktree, Space::Head, 5), Mapping::Both(6));
        assert_eq!(file.map(Space::Head, Space::Worktree, 8), Mapping::Both(7));
        // One hop declining declines the composition: HEAD 5 `echo` is gone from
        // the worktree, HEAD 2 `bravo` is gone from the index, and worktree 9
        // `juliet` was never in the index.
        assert_eq!(file.map(Space::Head, Space::Worktree, 5), Mapping::OneImage);
        assert_eq!(file.map(Space::Head, Space::Worktree, 2), Mapping::OneImage);
        assert_eq!(file.map(Space::Worktree, Space::Head, 9), Mapping::OneImage);
    }

    #[test]
    fn map_declines_a_zero_gutter_instead_of_underflowing() {
        let file = rich_file();
        assert_eq!(file.map(Space::Head, Space::Index, 0), Mapping::OneImage);
        assert_eq!(map_side(&file.local, Side::Pre, 0), Mapping::OneImage);
    }

    #[test]
    fn unstage_ords_pairs_a_replacement_but_not_a_same_kind_run() {
        // `-a +A -b +B`: two touching modifications in one hunk, ordinals 0..3.
        // A `Removed` immediately followed by an `Added` is one replacement pair;
        // the next `Removed` opens the next, so each modification reverse-applies
        // alone while its own pair stays together.
        let staged = patch(
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,4 @@\n-a\n+A\n-b\n+B\n",
        );
        let sets = derive_line_sets(staged, patch(""));
        let file = find(&sets, Path::new("f.txt")).expect("staged section");
        let pair = |a, b| Some(BTreeSet::from([a, b]));
        assert_eq!(unstage_ords(file, Source::Staged, 0, 0), pair(0, 1));
        assert_eq!(unstage_ords(file, Source::Staged, 0, 1), pair(0, 1));
        assert_eq!(unstage_ords(file, Source::Staged, 0, 2), pair(2, 3));
        assert_eq!(unstage_ords(file, Source::Staged, 0, 3), pair(2, 3));

        // A same-kind run (`+y +z`) is not a pair: each addition reverse-applies
        // on its own, so per-row control survives where git grants it.
        let adds = patch(
            "diff --git a/g.txt b/g.txt\n--- a/g.txt\n+++ b/g.txt\n@@ -1,2 +1,4 @@\n x\n+y\n+z\n w\n",
        );
        let sets = derive_line_sets(adds, patch(""));
        let g = find(&sets, Path::new("g.txt")).expect("adds section");
        assert_eq!(
            unstage_ords(g, Source::Staged, 0, 0),
            Some(BTreeSet::from([0]))
        );
        assert_eq!(
            unstage_ords(g, Source::Staged, 0, 1),
            Some(BTreeSet::from([1]))
        );
    }

    #[test]
    fn unstage_ords_declines_an_input_it_cannot_locate() {
        // One unstaged addition, so the `local` section exists and hunk 0 has
        // exactly one changed line.
        const LOCAL_UNSTAGED: &str = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,5 @@\n A\n+local\n b\n c\n d\n";
        // Every arm that cannot name a change unit answers nothing. A lone
        // ordinal is the selection this function exists to refuse: reverse-
        // applying one half of a replacement pair rewrites the index to the
        // wrong text, so an unlocatable `ord` must produce no operation rather
        // than a patch built from a guess.
        let staged = patch(
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,4 @@\n-a\n+A\n-b\n+B\n",
        );
        let sets = derive_line_sets(staged, patch(LOCAL_UNSTAGED));
        let file = find(&sets, Path::new("f.txt")).expect("both sections");

        // An ordinal past the hunk's changed lines: hunk 0 has exactly four.
        assert_eq!(unstage_ords(file, Source::Staged, 0, 4), None);
        // A hunk index the section does not have.
        assert_eq!(unstage_ords(file, Source::Staged, 1, 0), None);
        // A section the path has no entry for: `f.txt` is staged and unstaged,
        // so ask for the one it is absent from.
        let sets = derive_line_sets(patch(""), patch(LOCAL_UNSTAGED));
        let only_local = find(&sets, Path::new("f.txt")).expect("local section");
        assert_eq!(unstage_ords(only_local, Source::Staged, 0, 0), None);
        // And the same input located in the section that does exist answers its
        // pair, so the three declines above are the missing location and not
        // the pairing.
        assert_eq!(
            unstage_ords(only_local, Source::Local, 0, 0),
            Some(BTreeSet::from([0]))
        );
    }

    #[test]
    fn a_new_file_section_is_detected_as_a_creation() {
        // The one reverse shape that still needs a whole-hunk selection.
        let created = patch(
            "diff --git a/n.txt b/n.txt\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n",
        );
        let sets = derive_line_sets(created, patch(""));
        let n = find(&sets, Path::new("n.txt")).expect("creation section");
        assert!(section_hunk_is_creation(n, Source::Staged, 0));

        // A hunk with context has a real pre-image to reverse into and takes a
        // fine-grained reverse, so it is not a creation.
        let mixed = patch(
            "diff --git a/m.txt b/m.txt\n--- a/m.txt\n+++ b/m.txt\n@@ -1,3 +1,4 @@\n ctx\n+new\n end\n",
        );
        let sets = derive_line_sets(mixed, patch(""));
        let m = find(&sets, Path::new("m.txt")).expect("mixed section");
        assert!(!section_hunk_is_creation(m, Source::Staged, 0));
    }

    #[test]
    fn locate_counts_one_ordinal_over_both_sides_of_a_hunk_in_body_order() {
        let staged_patch = patch(RICH_STAGED);
        let local_patch = patch(RICH_LOCAL);
        let staged = staged_patch.files.first().unwrap();
        let local = local_patch.files.first().unwrap();
        // One counter over `-` and `+` together, in body order, exactly as
        // `filter_lines` keeps it: `bravo` is 0, the `BRAVO` replacing it is 1,
        // and the later `-delta` is 2, not 0 of a second deletion run.
        assert_eq!(
            locate_line(staged, Side::Pre, 2),
            Ordinal::At { hunk: 0, ord: 0 }
        );
        assert_eq!(
            locate_line(staged, Side::Post, 2),
            Ordinal::At { hunk: 0, ord: 1 }
        );
        assert_eq!(
            locate_line(staged, Side::Pre, 4),
            Ordinal::At { hunk: 0, ord: 2 }
        );
        // `-echo` is 0 and `+ECHO` is 1 even though they number different
        // spaces, and the two trailing additions are 2 and 3 with four context
        // lines spent between them.
        assert_eq!(
            locate_line(local, Side::Pre, 4),
            Ordinal::At { hunk: 0, ord: 0 }
        );
        assert_eq!(
            locate_line(local, Side::Post, 4),
            Ordinal::At { hunk: 0, ord: 1 }
        );
        assert_eq!(
            locate_line(local, Side::Post, 8),
            Ordinal::At { hunk: 0, ord: 2 }
        );
        assert_eq!(
            locate_line(local, Side::Post, 9),
            Ordinal::At { hunk: 0, ord: 3 }
        );
    }

    #[test]
    fn locate_tells_a_context_line_from_a_line_the_patch_never_prints() {
        let file = rich_file();
        assert_eq!(
            file.locate(Source::Staged, Side::Pre, 3),
            Ordinal::Context { hunk: 0 },
            "HEAD 3 `charlie` is inside the staged hunk and unchanged by it"
        );
        assert_eq!(
            file.locate(Source::Staged, Side::Pre, 6),
            Ordinal::Untouched,
            "the staged patch prints no HEAD line 6 at all"
        );
        assert_eq!(
            file.locate(Source::Staged, Side::Post, 5),
            Ordinal::Untouched,
            "and no index line 5 either, though the file has one"
        );
        assert_eq!(
            file.locate(Source::Local, Side::Pre, 1),
            Ordinal::Untouched,
            "the unstaged hunk starts at index 3"
        );
        assert_eq!(
            file.locate(Source::Local, Side::Pre, 5),
            Ordinal::Context { hunk: 0 },
            "index 5 `foxtrot` is the hunk's own context"
        );
    }

    #[test]
    fn a_no_newline_marker_costs_no_ordinal_and_adds_no_line() {
        let text = "diff --git a/g.txt b/g.txt\n--- a/g.txt\n+++ b/g.txt\n@@ -1,2 +1,2 @@\n keep\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n";
        let parsed = patch(text);
        let file = parsed.files.first().unwrap();
        assert_eq!(
            locate_line(file, Side::Pre, 2),
            Ordinal::At { hunk: 0, ord: 0 }
        );
        assert_eq!(
            locate_line(file, Side::Post, 2),
            Ordinal::At { hunk: 0, ord: 1 }
        );
        assert_eq!(
            locate_line(file, Side::Post, 3),
            Ordinal::Untouched,
            "the marker annotates line 2 and is never a body line of its own"
        );
        let derived = derive_line_sets(Patch::default(), patch(text));
        assert_eq!(
            map_side(&derived.first().unwrap().local, Side::Pre, 2),
            Mapping::OneImage,
            "and the sets count the same two lines the walk counts"
        );
    }

    #[test]
    fn row_source_hops_a_head_gutter_before_naming_an_index_position() {
        let sets = derive_line_sets(patch(RICH_STAGED), patch(RICH_LOCAL));
        let ask = |kind, old_no, new_no, displayed, toggle| {
            row_source(
                &sets,
                Path::new("f.txt"),
                &row(kind, old_no, new_no),
                displayed,
                toggle,
            )
        };
        use Displayed::*;
        use Toggle::*;
        // The trap this exists for: HEAD 5 `echo` is index 4, because the stage
        // deleted `delta` above it. The pane's gutter therefore names nothing the
        // unstaged patch changes: asking with 5 answers `Context`, a different
        // line (`foxtrot`) declined, while the click's real target has ordinal 0.
        assert_eq!(
            ask(RowKind::Del, 5, 0, HeadVsWorktree, Stage),
            RowSource::Located {
                source: Source::Local,
                hunk: 0,
                ord: 0
            }
        );
        let file = find(&sets, Path::new("f.txt")).expect("listed");
        assert_eq!(
            file.locate(Source::Local, Side::Pre, 5),
            Ordinal::Context { hunk: 0 },
            "the untranslated answer, which is a different line entirely"
        );
        // No hop needed, and none taken: the two pairs where the gutter already
        // names the space the appliable patch numbers.
        assert_eq!(
            ask(RowKind::Del, 4, 0, HeadVsWorktree, Unstage),
            RowSource::Located {
                source: Source::Staged,
                hunk: 0,
                ord: 2
            },
            "staged removal of `delta`, straight out of `HEAD <-> index`"
        );
        assert_eq!(
            ask(RowKind::Add, 0, 8, HeadVsWorktree, Stage),
            RowSource::Located {
                source: Source::Local,
                hunk: 0,
                ord: 2
            },
            "unstaged addition of `ink`, straight out of `index <-> worktree`"
        );
        // One hop each way for the cross pairs.
        assert_eq!(
            ask(RowKind::Add, 0, 2, HeadVsWorktree, Unstage),
            RowSource::Located {
                source: Source::Staged,
                hunk: 0,
                ord: 1
            },
            "worktree 2 `BRAVO` is index 2, the staged patch's own added line"
        );
        assert_eq!(
            ask(RowKind::Del, 4, 0, IndexVsWorktree, Stage),
            RowSource::Located {
                source: Source::Local,
                hunk: 0,
                ord: 0
            },
            "the same line carries gutter 5 under a HEAD read and gutter 4 under \
             today's index read, and each reaches this one ordinal"
        );
    }

    #[test]
    fn row_source_names_which_half_declined() {
        let sets = derive_line_sets(patch(RICH_STAGED), patch(RICH_LOCAL));
        let ask = |kind, old_no, new_no, displayed, toggle| {
            row_source(
                &sets,
                Path::new("f.txt"),
                &row(kind, old_no, new_no),
                displayed,
                toggle,
            )
        };
        use Displayed::*;
        use NoSource::*;
        use Toggle::*;
        // A staged removal is not in the index, so the unstaged patch cannot
        // carry its stage: the hop, not the locate, declined.
        assert_eq!(
            ask(RowKind::Del, 4, 0, HeadVsWorktree, Stage),
            RowSource::Missing(NoFarSide)
        );
        // An unstaged addition is not in the index either, so it has nothing to
        // unstage.
        assert_eq!(
            ask(RowKind::Add, 0, 8, HeadVsWorktree, Unstage),
            RowSource::Missing(NoFarSide)
        );
        // In the right patch but context there: the index equals HEAD for this
        // line, so an unstage of it would move nothing.
        assert_eq!(
            ask(RowKind::Del, 5, 0, HeadVsWorktree, Unstage),
            RowSource::Missing(Context { hunk: 0 }),
            "HEAD 5 `echo` is unstaged content the staged patch prints unchanged"
        );
        // And outside every hunk of it, which is a different answer about a
        // different line: the staged patch never prints HEAD 6 at all.
        assert_eq!(
            ask(RowKind::Del, 6, 0, HeadVsWorktree, Unstage),
            RowSource::Missing(Untouched)
        );
        // Not in any hunk of the right patch: worktree line 1 is above every
        // hunk the unstaged patch prints, so no selection exists to make.
        assert_eq!(
            ask(RowKind::Add, 0, 1, HeadVsWorktree, Stage),
            RowSource::Missing(Untouched)
        );
        // The row-level declines, each its own answer.
        assert_eq!(
            ask(RowKind::Context, 5, 5, HeadVsWorktree, Stage),
            RowSource::Missing(RowIsNotALine)
        );
        assert_eq!(
            ask(RowKind::Hunk, 0, 0, HeadVsWorktree, Stage),
            RowSource::Missing(RowIsNotALine)
        );
        assert_eq!(
            ask(RowKind::Add, 0, 0, HeadVsWorktree, Stage),
            RowSource::Missing(RowNumbersNothing)
        );
        assert_eq!(
            row_source(
                &sets,
                Path::new("g.txt"),
                &row(RowKind::Add, 0, 1),
                HeadVsWorktree,
                Stage
            ),
            RowSource::Missing(PathAbsent),
            "a path in neither patch"
        );
        // The toggle can name a patch that has no section for a path the other
        // one does list.
        let local_only = derive_line_sets(Patch::default(), patch(RICH_LOCAL));
        assert_eq!(
            row_source(
                &local_only,
                Path::new("f.txt"),
                &row(RowKind::Add, 0, 5),
                HeadVsWorktree,
                Unstage
            ),
            RowSource::Missing(SourceAbsent)
        );
        // An unmerged path is inert before any of the arithmetic.
        let conflicted = derive_line_sets(Patch::default(), patch(COMBINED));
        assert_eq!(
            row_source(
                &conflicted,
                Path::new("conf.txt"),
                &row(RowKind::Add, 0, 2),
                HeadVsWorktree,
                Stage
            ),
            RowSource::Missing(Conflicted)
        );
    }

    #[test]
    fn the_retained_sections_compare_by_content_not_by_pointer() {
        let sets = derive_line_sets(patch(RICH_STAGED), patch(RICH_LOCAL));
        let twin = derive_line_sets(patch(RICH_STAGED), patch(RICH_LOCAL));
        let held = sets.first().unwrap().staged_section.as_ref().unwrap();
        let other = twin.first().unwrap().staged_section.as_ref().unwrap();
        assert!(
            !Arc::ptr_eq(held, other),
            "two derivations allocate two sections, so the equality below cannot be pointer identity"
        );
        assert_eq!(sets, twin, "sections included");
        // And content really is what is compared: one word changed in one body
        // makes two otherwise equal entries unequal.
        let mutated = derive_line_sets(
            patch(&RICH_STAGED.replace("charlie", "CHARLIE")),
            patch(RICH_LOCAL),
        );
        assert_ne!(sets, mutated);
        // Equality reaches the same text the patch carried.
        let again = patch(RICH_LOCAL);
        assert_eq!(
            sets.first().unwrap().local_section.as_deref(),
            again.files.first(),
            "the retained section is the parsed section, not a summary of it"
        );
    }

    #[test]
    fn derive_retains_a_section_only_for_the_patch_that_printed_it() {
        let both = derive_line_sets(patch(RICH_STAGED), patch(RICH_LOCAL));
        let one = find(&both, Path::new("f.txt")).expect("listed");
        assert!(one.staged_section.is_some() && one.local_section.is_some());
        let staged_only = derive_line_sets(patch(RICH_STAGED), Patch::default());
        let one = find(&staged_only, Path::new("f.txt")).expect("listed");
        assert!(one.staged_section.is_some() && one.local_section.is_none());
        assert_eq!(
            one.locate(Source::Local, Side::Post, 4),
            Ordinal::NoSection,
            "the absent patch has no section to walk"
        );
        // A conflicted path retains nothing: a `diff --cc` body numbers no
        // image and `git apply` refuses it, so there is no appliable section.
        let conflict = derive_line_sets(Patch::default(), patch(COMBINED));
        let one = find(&conflict, Path::new("conf.txt")).expect("listed");
        assert!(one.local_section.is_none() && one.staged_section.is_none());
        assert!(one.conflicted);
    }

    #[test]
    fn retention_leaves_every_existing_answer_where_it_was() {
        let sets = derive_line_sets(patch(RICH_STAGED), patch(RICH_LOCAL));
        let one = find(&sets, Path::new("f.txt")).expect("listed");
        assert_eq!(
            (one.staged.removed.runs(), one.staged.added.runs()),
            (&[(2, 2), (4, 4)][..], &[(2, 2)][..]),
            "the runs are still counted out of the same bodies the sections keep"
        );
        assert_eq!(
            (one.local.removed.runs(), one.local.added.runs()),
            (&[(4, 4)][..], &[(4, 4), (8, 9)][..])
        );
        assert_eq!(
            row_is_staged(&sets, Path::new("f.txt"), &row(RowKind::Del, 4, 0)),
            Some(true),
            "a staged removal answers as it did before retention"
        );
        assert_eq!(
            row_is_staged(&sets, Path::new("f.txt"), &row(RowKind::Add, 0, 8)),
            Some(false)
        );
        assert_eq!(
            row_is_staged(&sets, Path::new("f.txt"), &row(RowKind::Add, 0, 5)),
            Some(true),
            "worktree 5 `foxtrot` is in the index"
        );
    }

    #[test]
    fn the_located_position_selects_exactly_that_line_of_the_source_section() {
        let sets = derive_line_sets(patch(RICH_STAGED), patch(RICH_LOCAL));
        let one = find(&sets, Path::new("f.txt")).expect("listed");
        // Stage the unstaged deletion of `echo`, addressed through the index.
        let RowSource::Located { source, hunk, ord } = row_source(
            &sets,
            Path::new("f.txt"),
            &row(RowKind::Del, 5, 0),
            Displayed::HeadVsWorktree,
            Toggle::Stage,
        ) else {
            panic!("the click has an answer");
        };
        let composed = turbogit_services::partial::compose(
            &section_as_patch(one, source).expect("a retained section"),
            &select(hunk, ord),
            turbogit_engine_api::ApplyDirection::Forward,
        )
        .to_string();
        assert!(composed.contains("-echo"), "{composed}");
        assert!(!composed.contains("+ECHO"), "{composed}");
        assert!(!composed.contains("+ink"), "{composed}");
        // Stage the addition of `ink` and only that line.
        let RowSource::Located { source, hunk, ord } = row_source(
            &sets,
            Path::new("f.txt"),
            &row(RowKind::Add, 0, 8),
            Displayed::HeadVsWorktree,
            Toggle::Stage,
        ) else {
            panic!("the click has an answer");
        };
        let composed = turbogit_services::partial::compose(
            &section_as_patch(one, source).expect("a retained section"),
            &select(hunk, ord),
            turbogit_engine_api::ApplyDirection::Forward,
        );
        assert_eq!(composed.files.len(), 1, "one section, as retained");
        assert_eq!(composed.files[0].hunks.len(), 1);
        let text = composed.to_string();
        assert!(text.contains("+ink") && !text.contains("+juliet"), "{text}");
        assert!(!text.contains("-echo"), "{text}");
    }

    #[test]
    fn a_two_line_hunk_locates_the_ordinal_that_distinguishes_them() {
        // Adjacent identical additions are the case content cannot decide: one
        // hunk, two `+dup` lines, two ordinals, and the numbers say which is
        // which where the text cannot.
        let both = "diff --git a/dup.txt b/dup.txt\n--- a/dup.txt\n+++ b/dup.txt\n@@ -1 +1,3 @@\n base\n+dup\n+dup\n";
        let parsed = patch(both);
        let file = parsed.files.first().unwrap();
        assert_eq!(
            locate_line(file, Side::Post, 2),
            Ordinal::At { hunk: 0, ord: 0 }
        );
        assert_eq!(
            locate_line(file, Side::Post, 3),
            Ordinal::At { hunk: 0, ord: 1 }
        );
        // The same pair when only one copy is staged: the first copy is context
        // in the unstaged patch and the second is its only changed line, so each
        // row is addressed in the patch that actually holds it.
        let staged = "diff --git a/dup.txt b/dup.txt\n--- a/dup.txt\n+++ b/dup.txt\n@@ -1 +1,2 @@\n base\n+dup\n";
        let unstaged = "diff --git a/dup.txt b/dup.txt\n--- a/dup.txt\n+++ b/dup.txt\n@@ -1,2 +1,3 @@\n base\n dup\n+dup\n";
        let derived = derive_line_sets(patch(staged), patch(unstaged));
        let mixed = find(&derived, Path::new("dup.txt")).expect("listed");
        assert_eq!(
            mixed.locate(Source::Staged, Side::Post, 2),
            Ordinal::At { hunk: 0, ord: 0 },
            "the copy the index holds"
        );
        assert_eq!(
            mixed.locate(Source::Local, Side::Post, 2),
            Ordinal::Context { hunk: 0 },
            "the same worktree number, context in the other patch"
        );
        assert_eq!(
            mixed.locate(Source::Local, Side::Post, 3),
            Ordinal::At { hunk: 0, ord: 0 },
            "and the copy the worktree added"
        );
        assert_eq!(
            row_source(
                &derived,
                Path::new("dup.txt"),
                &row(RowKind::Add, 0, 3),
                Displayed::HeadVsWorktree,
                Toggle::Stage
            ),
            RowSource::Located {
                source: Source::Local,
                hunk: 0,
                ord: 0
            },
            "the second copy is the one the click can stage"
        );
    }
}
