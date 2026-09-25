//! Partial staging: composing a stageable patch from the **Git engine**'s patch
//! value (`ADR-0022`, superseding `ADR-0013`'s raw-text composition).
//!
//! A selection is applied to the value — hunks are its structure, a line's kind
//! is a field, and a kept hunk's counts are arithmetic — and the text the index
//! takes is rendered from the composed value at the moment of applying.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{ChangeStatus, Patch, PatchFile, PatchHunk, PatchLine, PatchLineKind};
use turbogit_engine_api::{ApplyDirection, GitExecutor};

/// What part of a single hunk is selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HunkSelection {
    /// The entire hunk: every changed line plus its context.
    Whole,
    /// Only the listed changed lines. Positions are 0-based ordinals counted
    /// over the hunk's `+`/`-` lines in order; context lines are not numbered.
    Lines(BTreeSet<usize>),
    /// A character range within one changed line (story: line-granularity
    /// staging, issue 19). `ord` follows the [`HunkSelection::Lines`]
    /// ordinal scheme; `start..end` are char offsets into that line's body
    /// (the text after the `+`/`-` marker), end exclusive and clamped to the
    /// line. Defined for addition lines: the staged index gains a line
    /// holding exactly the selected bytes while the old line's deletion
    /// stays unstaged. On a deletion line the range selects nothing.
    Chars {
        ord: usize,
        start: usize,
        end: usize,
    },
}

/// Partial-staging selection for one file's diff, keyed by 0-based hunk index
/// (same order as the diff row model's `Row.hunk`). An absent key means the
/// hunk is not selected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub hunks: BTreeMap<usize, HunkSelection>,
}

/// Compose `patch` down to the selected hunks of `selection` — as the value it
/// already is. Hunk boundaries are the value's structure, the counts in a
/// `@@` header are arithmetic on the kept lines, and a line's kind is a field,
/// not a prefix character to strip (`ADR-0022`, retiring `ADR-0013`'s
/// raw-text composition).
///
/// Empty when nothing is selected; callers treat that as a no-op.
pub fn compose(patch: &Patch, selection: &Selection) -> Patch {
    let files = patch
        .files
        .iter()
        .filter_map(|file| {
            let hunks: Vec<PatchHunk> = file
                .hunks
                .iter()
                .enumerate()
                .filter_map(|(idx, hunk)| {
                    let kept = match selection.hunks.get(&idx)? {
                        HunkSelection::Whole => hunk.lines.clone(),
                        HunkSelection::Lines(lines) if !lines.is_empty() => {
                            filter_lines(&hunk.lines, &Filter::Lines(lines))
                        }
                        HunkSelection::Chars { ord, start, end } => filter_lines(
                            &hunk.lines,
                            &Filter::Chars {
                                ord: *ord,
                                start: *start,
                                end: *end,
                            },
                        ),
                        HunkSelection::Lines(_) => return None,
                    };
                    if kept.is_empty() {
                        return None;
                    }
                    // The counts a header will carry are the kept lines' own
                    // totals, computed here rather than rewritten into a string.
                    let (old_count, new_count) = body_line_counts(&kept);
                    Some(PatchHunk {
                        old_start: hunk.old_start,
                        old_count,
                        new_start: hunk.new_start,
                        new_count,
                        heading: hunk.heading.clone(),
                        lines: kept,
                    })
                })
                .collect();
            if hunks.is_empty() {
                return None;
            }
            Some(PatchFile {
                old_path: file.old_path.clone(),
                new_path: file.new_path.clone(),
                combined: file.combined,
                headers: file.headers.clone(),
                hunks,
            })
        })
        .collect();
    Patch { files }
}

/// Old-side and new-side line counts of kept body lines. A context line counts
/// on both sides; the `\ No newline at end of file` marker rides on the changed
/// line above it and contributes nothing of its own.
fn body_line_counts(lines: &[PatchLine]) -> (usize, usize) {
    let mut old = 0usize;
    let mut new = 0usize;
    for line in lines {
        match line.kind {
            PatchLineKind::Context => {
                old += 1;
                new += 1;
            }
            PatchLineKind::Removed => old += 1,
            PatchLineKind::Added => new += 1,
        }
    }
    (old, new)
}

/// Granular operations are forbidden on conflicted files (spec R2): a patch
/// against merge state would corrupt it. Conflicts resolve through the
/// conflict modal instead.
fn ensure_granular_allowed(status: ChangeStatus) -> TgResult<()> {
    if status == ChangeStatus::Conflicted {
        return Err(turbogit_domain::error::TgError::Other(
            "Granular staging is unavailable for conflicted files; \
             resolve the conflict first"
                .into(),
        ));
    }
    Ok(())
}

/// Whether `err` is git's fail-fast index-lock collision: `git apply` (and
/// every index writer) takes `.git/index.lock` exclusively and aborts with
/// exit 128 when anything else — a status refresh over a racy index, an IDE,
/// a background fetch — holds or is creating it. The collision is momentary
/// by nature, so it is safe to retry; everything else is a real failure.
fn is_transient_index_lock(err: &TgError) -> bool {
    let msg = err.to_string();
    msg.contains("index.lock") && msg.contains("File exists")
}

/// Run `op`, retrying briefly while it fails with a transient index-lock
/// collision ([`is_transient_index_lock`]). Bounded: a handful of attempts
/// with a growing sub-second backoff, then the last error propagates — a
/// genuinely stale lock must still surface to the user.
pub(crate) fn retry_on_transient_lock<T>(mut op: impl FnMut() -> TgResult<T>) -> TgResult<T> {
    /// Total bounded wait stays well under a second: lock holders here are
    /// other short-lived git processes, not stuck ones.
    const ATTEMPTS: usize = 6;
    const BACKOFF_MS: u64 = 25;
    let mut attempt = 1usize;
    loop {
        match op() {
            ok @ Ok(_) => return ok,
            Err(e) if attempt < ATTEMPTS && is_transient_index_lock(&e) => {
                std::thread::sleep(Duration::from_millis(BACKOFF_MS * attempt as u64));
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Stage the selected hunks/lines of one file's patch into the index.
///
/// Composes the selection off the patch value and applies the rendered result
/// forward (`ADR-0022`, superseding `ADR-0013`'s raw-text composition). An
/// empty selection is a no-op — nothing touches the engine. Conflicted files
/// are rejected before anything reaches the engine.
pub fn stage_selection(
    vcs: &dyn GitExecutor,
    root: &Path,
    patch: &Patch,
    selection: &Selection,
    status: ChangeStatus,
) -> TgResult<()> {
    ensure_granular_allowed(status)?;
    let composed = compose(patch, selection);
    if composed.is_empty() {
        return Ok(());
    }
    retry_on_transient_lock(|| vcs.apply_patch_to_index(root, &composed, ApplyDirection::Forward))
}

/// Unstage the selected hunks/lines of one file's patch from the index.
///
/// Composes the selection off the patch value and reverse-applies the rendered
/// result against the index (`git apply --cached --reverse`), matching git's own
/// unstage semantics. An empty selection is a no-op. Conflicted files are
/// rejected before anything reaches the engine.
pub fn unstage_selection(
    vcs: &dyn GitExecutor,
    root: &Path,
    patch: &Patch,
    selection: &Selection,
    status: ChangeStatus,
) -> TgResult<()> {
    ensure_granular_allowed(status)?;
    let composed = compose(patch, selection);
    if composed.is_empty() {
        return Ok(());
    }
    retry_on_transient_lock(|| vcs.apply_patch_to_index(root, &composed, ApplyDirection::Reverse))
}

/// Stage part of an untracked file's content into the index.
///
/// The file is made intent-to-add first (`git add -N`) so the index can hold
/// a partial creation; only then is the composed patch applied forward. An
/// empty selection is a no-op — not even the intent-to-add marker happens.
pub fn stage_untracked_selection(
    vcs: &dyn GitExecutor,
    root: &Path,
    paths: &[PathBuf],
    patch: &Patch,
    selection: &Selection,
    status: ChangeStatus,
) -> TgResult<()> {
    ensure_granular_allowed(status)?;
    let composed = compose(patch, selection);
    if composed.is_empty() {
        return Ok(());
    }
    // Both calls take the index lock (`add -N` writes the index too), so
    // each gets the transient-collision retry independently.
    retry_on_transient_lock(|| vcs.add_intent_to_add(root, paths))?;
    retry_on_transient_lock(|| vcs.apply_patch_to_index(root, &composed, ApplyDirection::Forward))
}

/// Which changed lines of one hunk body survive composition, and how.
enum Filter<'a> {
    /// The listed line ordinals, whole.
    Lines(&'a BTreeSet<usize>),
    /// One line's char range (`Chars` selection): the selected slice of the
    /// addition becomes the staged line; every other changed line is
    /// unselected (deletions turn context, additions drop).
    Chars {
        ord: usize,
        start: usize,
        end: usize,
    },
}

/// Keep context lines and the selected changed lines of one hunk body.
///
/// An unselected addition is dropped entirely (it must not come into
/// existence in the staged result), while an unselected deletion survives on
/// both sides and is therefore re-emitted as a context line — removing it
/// outright would misalign every later old-side line of the hunk. A
/// `\ No newline at end of file` marker annotates the changed line above it,
/// so it survives only when that line does.
fn filter_lines(lines: &[PatchLine], filter: &Filter) -> Vec<PatchLine> {
    /// The selected slice of one addition body (char-indexed, clamped,
    /// end exclusive), or None when the range selects nothing.
    fn char_slice(text: &str, start: usize, end: usize) -> Option<String> {
        let chars: Vec<char> = text.chars().collect();
        let start = start.min(chars.len());
        let end = end.min(chars.len());
        (start < end).then(|| chars[start..end].iter().collect::<String>())
    }

    let mut out: Vec<PatchLine> = Vec::new();
    let mut changed = 0usize;
    for line in lines {
        match line.kind {
            PatchLineKind::Context => out.push(line.clone()),
            PatchLineKind::Removed => {
                let keep = match filter {
                    Filter::Lines(selected) => selected.contains(&changed),
                    // A Chars selection never stages a deletion: the old line
                    // stays in the index and only the selected bytes are added.
                    Filter::Chars { .. } => false,
                };
                changed += 1;
                out.push(PatchLine {
                    kind: if keep {
                        PatchLineKind::Removed
                    } else {
                        PatchLineKind::Context
                    },
                    text: line.text.clone(),
                    no_newline: keep && line.no_newline,
                });
            }
            PatchLineKind::Added => {
                let kept = match filter {
                    Filter::Lines(selected) => selected.contains(&changed).then(|| PatchLine {
                        kind: PatchLineKind::Added,
                        text: line.text.clone(),
                        no_newline: line.no_newline,
                    }),
                    Filter::Chars { ord, start, end } => (changed == *ord)
                        .then(|| char_slice(&line.text, *start, *end))
                        .flatten()
                        .map(|text| PatchLine {
                            kind: PatchLineKind::Added,
                            text,
                            no_newline: line.no_newline,
                        }),
                };
                changed += 1;
                out.extend(kept);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use turbogit_domain::model::ChangeStatus;
    use turbogit_engine::fake::{Call, FakeExecutor};
    use turbogit_engine_api::ApplyDirection;

    /// Two-hunk diff fixture: the `alpha` edit and the `omega` edit are far
    /// enough apart that git reports them as independent hunks.
    const TWO_HUNK_DIFF: &str = concat!(
        "diff --git a/words.txt b/words.txt\n",
        "--- a/words.txt\n",
        "+++ b/words.txt\n",
        "@@ -1,3 +1,3 @@\n",
        " alpha\n",
        "-bravo\n",
        "+BRAVO\n",
        "@@ -18,3 +18,3 @@\n",
        " oscar\n",
        "-papa\n",
        "+PAPA\n",
    );

    /// The staging path composes from the value, so that is what a test hands
    /// it. git's syntax has exactly one reader and it lives with the adapters;
    /// nothing in this crate's production code calls it.
    fn patch(text: &str) -> Patch {
        turbogit_engine::patch::parse_patch(text)
    }

    /// …and the composed value's *text* is the staging guarantee (ADR-0013 as
    /// superseded by issue 11), so a test states what those bytes are.
    fn compose_patch(text: &str, selection: &Selection) -> String {
        compose(&patch(text), selection).to_string()
    }

    fn whole_hunk(idx: usize) -> Selection {
        Selection {
            hunks: [(idx, HunkSelection::Whole)].into_iter().collect(),
        }
    }

    #[test]
    fn stage_selection_applies_whole_hunk_forward() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        stage_selection(
            &engine,
            &root,
            &patch(TWO_HUNK_DIFF),
            &whole_hunk(0),
            ChangeStatus::Modified,
        )
        .unwrap();

        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [Call::ApplyPatch {
                direction: ApplyDirection::Forward
            }],
            "granular stage must dispatch one forward patch application"
        );
    }

    #[test]
    fn stage_selection_with_nothing_selected_is_a_no_op() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        stage_selection(
            &engine,
            &root,
            &patch(TWO_HUNK_DIFF),
            &Selection::default(),
            ChangeStatus::Modified,
        )
        .unwrap();

        assert!(
            engine.calls.lock().unwrap().is_empty(),
            "empty selection must not touch the engine"
        );
    }

    #[test]
    fn unstage_selection_reverse_applies_whole_hunk() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        unstage_selection(
            &engine,
            &root,
            &patch(TWO_HUNK_DIFF),
            &whole_hunk(0),
            ChangeStatus::Modified,
        )
        .unwrap();

        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [Call::ApplyPatch {
                direction: ApplyDirection::Reverse
            }],
            "granular unstage must reverse-apply against the index"
        );
    }

    #[test]
    fn unstage_selection_with_nothing_selected_is_a_no_op() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        unstage_selection(
            &engine,
            &root,
            &patch(TWO_HUNK_DIFF),
            &Selection::default(),
            ChangeStatus::Modified,
        )
        .unwrap();

        assert!(
            engine.calls.lock().unwrap().is_empty(),
            "empty selection must not touch the engine"
        );
    }

    /// Creation diff for an untracked file: git reports it as a new-file
    /// addition once marked intent-to-add.
    const UNTRACKED_DIFF: &str = concat!(
        "diff --git a/new.txt b/new.txt\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/new.txt\n",
        "@@ -0,0 +1,3 @@\n",
        "+one\n",
        "+two\n",
        "+three\n",
    );

    #[test]
    fn stage_untracked_selection_intents_to_add_then_applies_forward() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        stage_untracked_selection(
            &engine,
            &root,
            &[PathBuf::from("new.txt")],
            &patch(UNTRACKED_DIFF),
            &whole_hunk(0),
            ChangeStatus::Unversioned,
        )
        .unwrap();

        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [
                Call::AddIntentToAdd(vec![PathBuf::from("new.txt")]),
                Call::ApplyPatch {
                    direction: ApplyDirection::Forward
                },
            ],
            "untracked files must be made intent-to-add before patch application"
        );
    }

    #[test]
    fn stage_untracked_selection_with_nothing_selected_is_a_no_op() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        stage_untracked_selection(
            &engine,
            &root,
            &[PathBuf::from("new.txt")],
            &patch(UNTRACKED_DIFF),
            &Selection::default(),
            ChangeStatus::Unversioned,
        )
        .unwrap();

        assert!(
            engine.calls.lock().unwrap().is_empty(),
            "empty selection must not intent-to-add the file"
        );
    }

    /// One-hunk diff whose changed pair edits one long line — the char-range
    /// staging fixture: only a slice of the addition should reach the index.
    const SINGLE_EDIT_DIFF: &str = concat!(
        "diff --git a/code.rs b/code.rs\n",
        "--- a/code.rs\n",
        "+++ b/code.rs\n",
        "@@ -1,3 +1,3 @@\n",
        " fn main() {\n",
        "-    let x = 1;\n",
        "+    let calculated_value = compute(x);\n",
        " }\n",
    );

    #[test]
    fn compose_patch_char_range_on_an_addition_keeps_exactly_the_selected_bytes() {
        // The addition is changed-line ordinal 1 (the deletion is 0). Chars
        // 8..24 of the addition body select `calculated_value` exactly (four
        // leading spaces precede it).
        let selection = Selection {
            hunks: [(
                0usize,
                HunkSelection::Chars {
                    ord: 1,
                    start: 8,
                    end: 24,
                },
            )]
            .into_iter()
            .collect(),
        };

        let patch = compose_patch(SINGLE_EDIT_DIFF, &selection);

        // The old line stays context (its deletion is not selected), and the
        // addition collapses to exactly the selected bytes.
        let expected = concat!(
            "diff --git a/code.rs b/code.rs\n",
            "--- a/code.rs\n",
            "+++ b/code.rs\n",
            "@@ -1,3 +1,4 @@\n",
            " fn main() {\n",
            "     let x = 1;\n",
            "+calculated_value\n",
            " }\n",
        );
        assert_eq!(patch, expected);
    }

    #[test]
    fn compose_patch_char_range_outside_the_line_clamps_to_the_line() {
        let selection = Selection {
            hunks: [(
                0usize,
                HunkSelection::Chars {
                    ord: 1,
                    start: 4,
                    end: 999,
                },
            )]
            .into_iter()
            .collect(),
        };

        let patch = compose_patch(SINGLE_EDIT_DIFF, &selection);

        assert!(
            patch.contains("+let calculated_value = compute(x);\n"),
            "an over-long range must clamp to the line's bytes, got:\n{patch}"
        );
    }

    #[test]
    fn granular_stage_is_blocked_on_conflicted_files() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        let result = stage_selection(
            &engine,
            &root,
            &patch(TWO_HUNK_DIFF),
            &whole_hunk(0),
            ChangeStatus::Conflicted,
        );

        assert!(
            result.is_err(),
            "conflicted files must not stage granularly"
        );
        assert!(
            engine.calls.lock().unwrap().is_empty(),
            "blocked operations must not reach the engine — \
             conflicts resolve through the conflict modal"
        );
    }

    // --- transient index-lock retry ------------------------------------------

    fn lock_error() -> TgError {
        TgError::Cli {
            code: 128,
            stderr: "fatal: Unable to create '/repo/.git/index.lock': File exists.\n\n\
                     Another git process seems to be running in this repository"
                .into(),
        }
    }

    #[test]
    fn retry_succeeds_after_a_transient_lock_collision() {
        let mut calls = 0usize;
        let result: TgResult<()> = retry_on_transient_lock(|| {
            calls += 1;
            if calls == 1 {
                Err(lock_error())
            } else {
                Ok(())
            }
        });
        assert!(result.is_ok(), "one lock collision must be retried away");
        assert_eq!(calls, 2, "exactly one retry after a transient collision");
    }

    #[test]
    fn retry_is_bounded_for_a_persistently_locked_index() {
        let mut calls = 0usize;
        let result: TgResult<()> = retry_on_transient_lock(|| {
            calls += 1;
            Err(lock_error())
        });
        assert!(result.is_err(), "a stale lock must surface as an error");
        assert!(calls > 1, "transient collisions are retried");
    }

    #[test]
    fn retry_does_not_mask_unrelated_failures() {
        let mut calls = 0usize;
        let result: TgResult<()> = retry_on_transient_lock(|| {
            calls += 1;
            Err(TgError::Cli {
                code: 1,
                stderr: "error: patch does not apply".into(),
            })
        });
        assert!(result.is_err());
        assert_eq!(calls, 1, "real patch failures must not be retried");
    }

    #[test]
    fn retry_classifier_matches_only_lock_collisions() {
        assert!(is_transient_index_lock(&lock_error()));
        assert!(!is_transient_index_lock(&TgError::Cli {
            code: 1,
            stderr: "error: patch does not apply".into(),
        }));
        assert!(!is_transient_index_lock(&TgError::Other("boom".into())));
    }
}
