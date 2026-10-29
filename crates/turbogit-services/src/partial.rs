use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{ChangeStatus, Patch, PatchFile, PatchHunk, PatchLine, PatchLineKind};
use turbogit_engine_api::{ApplyDirection, GitExecutor};

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
/// **within the patch handed to [`compose`]**. An absent key means the hunk is
/// not selected.
///
/// This is not the same numbering as the display model's `Row::hunk` for a
/// multi-file patch, whatever the older comment claimed: `compose` enumerates
/// `file.hunks` inside its per-file closure, so the index restarts at 0 for every
/// section, while `diff_model::rows_from` declares its `hunk` counter outside the
/// file loop and lets it run across every file of the patch. The two agree only
/// when the patch has one file section, which is every path-scoped read and no
/// whole-root read. The first caller to hand `compose` a multi-file patch and a
/// `Row::hunk` would select the wrong hunk of the wrong file, silently, because
/// both numbers are 0-based and both compile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub hunks: BTreeMap<usize, HunkSelection>,
}

/// Compose `patch` down to the selected hunks of `selection`, as the value it
/// already is. Hunk boundaries are the value's structure, the counts in a
/// `@@` header are arithmetic on the kept lines, and a line's kind is a field,
/// not a prefix character to strip (`ADR-0022`, retiring `ADR-0013`'s
/// raw-text composition).
///
/// `direction` is which way the composed patch will be applied to the index,
/// and it changes only how an *unselected* changed line survives: see
/// `filter_lines`. A forward stage composes against the pre-image, a reverse
/// unstage against the post-image, and the two need opposite treatment of the
/// same dropped/demoted line.
///
/// Empty when nothing is selected; callers treat that as a no-op.
pub fn compose(patch: &Patch, selection: &Selection, direction: ApplyDirection) -> Patch {
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
                            filter_lines(&hunk.lines, &Filter::Lines(lines), direction)
                        }
                        HunkSelection::Chars { ord, start, end } => filter_lines(
                            &hunk.lines,
                            &Filter::Chars {
                                ord: *ord,
                                start: *start,
                                end: *end,
                            },
                            direction,
                        ),
                        HunkSelection::Lines(_) => return None,
                    };
                    if kept.is_empty() {
                        return None;
                    }
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

/// Validate the change status, compose the selection off the patch value, and
/// return the composed patch, or `None` when an empty selection makes the op
/// the no-op it is (nothing touches the engine).
fn composed_or_noop(
    patch: &Patch,
    selection: &Selection,
    status: ChangeStatus,
    direction: ApplyDirection,
) -> TgResult<Option<Patch>> {
    ensure_granular_allowed(status)?;
    let composed = compose(patch, selection, direction);
    if composed.is_empty() {
        return Ok(None);
    }
    Ok(Some(composed))
}

/// Apply one already-composed patch to the index in the named direction,
/// retrying while it hits a transient index-lock collision.
fn apply_composed(
    vcs: &dyn GitExecutor,
    root: &Path,
    patch: &Patch,
    direction: ApplyDirection,
) -> TgResult<()> {
    retry_on_transient_lock(|| vcs.apply_patch_to_index(root, patch, direction))
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
/// exit 128 when anything else, a status refresh over a racy index, an IDE,
/// a background fetch, holds or is creating it. The collision is momentary
/// by nature, so it is safe to retry; everything else is a real failure.
fn is_transient_index_lock(err: &TgError) -> bool {
    let msg = err.to_string();
    msg.contains("index.lock") && msg.contains("File exists")
}

/// Run `op`, retrying briefly while it fails with a transient index-lock
/// collision ([`is_transient_index_lock`]). Bounded: a handful of attempts
/// with a growing sub-second backoff, then the last error propagates, a
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
/// empty selection is a no-op, nothing touches the engine. Conflicted files
/// are rejected before anything reaches the engine.
pub fn stage_selection(
    vcs: &dyn GitExecutor,
    root: &Path,
    patch: &Patch,
    selection: &Selection,
    status: ChangeStatus,
) -> TgResult<()> {
    match composed_or_noop(patch, selection, status, ApplyDirection::Forward)? {
        Some(composed) => apply_composed(vcs, root, &composed, ApplyDirection::Forward),
        None => Ok(()),
    }
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
    match composed_or_noop(patch, selection, status, ApplyDirection::Reverse)? {
        Some(composed) => apply_composed(vcs, root, &composed, ApplyDirection::Reverse),
        None => Ok(()),
    }
}

/// Stage part of an untracked file's content into the index.
///
/// The file is made intent-to-add first (`git add -N`) so the index can hold
/// a partial creation; only then is the composed patch applied forward. An
/// empty selection is a no-op, not even the intent-to-add marker happens.
pub fn stage_untracked_selection(
    vcs: &dyn GitExecutor,
    root: &Path,
    paths: &[PathBuf],
    patch: &Patch,
    selection: &Selection,
    status: ChangeStatus,
) -> TgResult<()> {
    match composed_or_noop(patch, selection, status, ApplyDirection::Forward)? {
        Some(composed) => {
            // Both calls take the index lock (`add -N` writes the index too),
            // so each gets the transient-collision retry independently.
            retry_on_transient_lock(|| vcs.add_intent_to_add(root, paths))?;
            apply_composed(vcs, root, &composed, ApplyDirection::Forward)
        }
        None => Ok(()),
    }
}

enum Filter<'a> {
    Lines(&'a BTreeSet<usize>),
    /// One line's char range (`Chars` selection): the selected slice of the
    /// addition becomes the staged line; every other changed line is
    /// unselected (deletions turn context, additions drop). A `Chars` selection
    /// is only ever composed forward, so its treatment never branches on
    /// direction.
    Chars {
        ord: usize,
        start: usize,
        end: usize,
    },
}

/// Keep context lines and the selected changed lines of one hunk body.
///
/// Which text an *unselected* changed line is written as is the whole hazard
/// here, and it flips with `direction`, because `git apply --cached --reverse`
/// never consults HEAD: it matches the patch's new side against the index and
/// rewrites the file to the patch's old side. So the two directions want
/// opposite treatment of the same line.
///
/// Forward (a stage), against `index <-> worktree`, whose pre-image *is* the
/// index: git matches the pre-image, which is `context + every '-' line`. An
/// unselected addition is dropped (it must not enter the index yet) and an
/// unselected deletion is demoted to a context line carrying HEAD's old text.
/// Neither move touches the pre-image git matches, so any subset is appliable.
///
/// Reverse (an unstage), against `HEAD <-> index`, whose post-image *is* the
/// index: git matches the *new* side, so every staged line the user did not
/// select must still be described truly. An unselected addition is a line the
/// index holds, so it survives as a context line carrying its **post-image**
/// text, and an unselected deletion is a line the index no longer holds, so it
/// is dropped outright. Reusing the forward treatment for a reverse apply is the
/// failure these two arms exist to prevent, and it is total: a demoted `-` claims
/// HEAD's line where the index holds its replacement, and a dropped `+` erases an
/// index line from the block git is searching for. Both self-inflict
/// `error: patch failed` on every partial selection, and the only reverse patch
/// that then lands is the whole hunk. A selected change
/// keeps its `-`/`+` marks in either direction, so a replacement row staged or
/// unstaged as one row still moves as its `-`/`+` pair.
///
/// The counts follow from the kept lines (`body_line_counts`), so the header
/// agrees with whichever body this produces. A `\ No newline at end of file`
/// marker annotates the changed line above it, so it survives only when that
/// line does; a reverse-demoted addition keeps its own marker because the index
/// line it stands for has exactly that trailing-newline state.
fn filter_lines(lines: &[PatchLine], filter: &Filter, direction: ApplyDirection) -> Vec<PatchLine> {
    /// The selected slice of one addition body (char-indexed, clamped,
    /// end exclusive), or None when the range selects nothing.
    fn char_slice(text: &str, start: usize, end: usize) -> Option<String> {
        let chars: Vec<char> = text.chars().collect();
        let start = start.min(chars.len());
        let end = end.min(chars.len());
        (start < end).then(|| chars[start..end].iter().collect::<String>())
    }

    let reverse_lines = direction == ApplyDirection::Reverse && matches!(filter, Filter::Lines(_));

    let mut out: Vec<PatchLine> = Vec::new();
    let mut changed = 0usize;
    for line in lines {
        match line.kind {
            PatchLineKind::Context => out.push(line.clone()),
            PatchLineKind::Removed => {
                let keep = match filter {
                    Filter::Lines(selected) => selected.contains(&changed),
                    Filter::Chars { .. } => false,
                };
                changed += 1;
                if keep {
                    out.push(PatchLine {
                        kind: PatchLineKind::Removed,
                        text: line.text.clone(),
                        no_newline: line.no_newline,
                    });
                } else if reverse_lines {
                } else {
                    out.push(PatchLine {
                        kind: PatchLineKind::Context,
                        text: line.text.clone(),
                        no_newline: false,
                    });
                }
            }
            PatchLineKind::Added => match filter {
                Filter::Lines(selected) => {
                    let keep = selected.contains(&changed);
                    changed += 1;
                    if keep {
                        out.push(PatchLine {
                            kind: PatchLineKind::Added,
                            text: line.text.clone(),
                            no_newline: line.no_newline,
                        });
                    } else if reverse_lines {
                        out.push(PatchLine {
                            kind: PatchLineKind::Context,
                            text: line.text.clone(),
                            no_newline: line.no_newline,
                        });
                    }
                }
                Filter::Chars { ord, start, end } => {
                    let kept = (changed == *ord)
                        .then(|| char_slice(&line.text, *start, *end))
                        .flatten()
                        .map(|text| PatchLine {
                            kind: PatchLineKind::Added,
                            text,
                            no_newline: line.no_newline,
                        });
                    changed += 1;
                    out.extend(kept);
                }
            },
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

    fn patch(text: &str) -> Patch {
        turbogit_engine_api::patch::parse_patch(text)
    }

    fn compose_patch(text: &str, selection: &Selection) -> String {
        compose(
            &patch(text),
            selection,
            turbogit_engine_api::ApplyDirection::Forward,
        )
        .to_string()
    }

    fn compose_reverse(text: &str, selection: &Selection) -> String {
        compose(
            &patch(text),
            selection,
            turbogit_engine_api::ApplyDirection::Reverse,
        )
        .to_string()
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

    #[test]
    fn forward_composition_is_byte_for_byte_what_it_was_before_the_reverse_rule() {
        for (name, text, ords, expected) in forward_fixture_set() {
            let selection = Selection {
                hunks: [(0usize, HunkSelection::Lines(ords.iter().copied().collect()))]
                    .into_iter()
                    .collect(),
            };
            let composed = compose(&patch(text), &selection, ApplyDirection::Forward).to_string();
            assert_eq!(
                composed, expected,
                "forward bytes drifted for fixture {name}"
            );
        }
    }

    fn forward_fixture_set() -> Vec<(&'static str, &'static str, Vec<usize>, String)> {
        let cases: [(&str, &str, &[usize], &str); 7] = [
            (
                "PURE_ADD",
                "diff --git a/p.txt b/p.txt\n--- a/p.txt\n+++ b/p.txt\n@@ -1,3 +1,5 @@\n one\n+new1\n+new2\n two\n",
                &[0],
                "diff --git a/p.txt b/p.txt\n--- a/p.txt\n+++ b/p.txt\n@@ -1,2 +1,3 @@\n one\n+new1\n two\n",
            ),
            (
                "PURE_DEL",
                "diff --git a/d.txt b/d.txt\n--- a/d.txt\n+++ b/d.txt\n@@ -1,4 +1,2 @@\n keep\n-x\n-y\n last\n",
                &[0],
                "diff --git a/d.txt b/d.txt\n--- a/d.txt\n+++ b/d.txt\n@@ -1,4 +1,3 @@\n keep\n-x\n y\n last\n",
            ),
            (
                "REPLACE",
                "diff --git a/r.txt b/r.txt\n--- a/r.txt\n+++ b/r.txt\n@@ -1,3 +1,3 @@\n lead\n-old\n+new\n trail\n",
                &[1],
                "diff --git a/r.txt b/r.txt\n--- a/r.txt\n+++ b/r.txt\n@@ -1,3 +1,4 @@\n lead\n old\n+new\n trail\n",
            ),
            (
                "MIXED",
                "diff --git a/m.txt b/m.txt\n--- a/m.txt\n+++ b/m.txt\n@@ -1,5 +1,6 @@\n a\n-b\n+B\n c\n+added\n d\n",
                &[0, 1],
                "diff --git a/m.txt b/m.txt\n--- a/m.txt\n+++ b/m.txt\n@@ -1,4 +1,4 @@\n a\n-b\n+B\n c\n d\n",
            ),
            (
                "TRAIL_EOF",
                "diff --git a/t.txt b/t.txt\n--- a/t.txt\n+++ b/t.txt\n@@ -1,2 +1,4 @@\n first\n second\n+tail1\n+tail2\n",
                &[0],
                "diff --git a/t.txt b/t.txt\n--- a/t.txt\n+++ b/t.txt\n@@ -1,2 +1,3 @@\n first\n second\n+tail1\n",
            ),
            (
                "DUP_ADD",
                "diff --git a/u.txt b/u.txt\n--- a/u.txt\n+++ b/u.txt\n@@ -1 +1,3 @@\n base\n+dup\n+dup\n",
                &[0],
                "diff --git a/u.txt b/u.txt\n--- a/u.txt\n+++ b/u.txt\n@@ -1 +1,2 @@\n base\n+dup\n",
            ),
            (
                "UNTRACKED_IA",
                "diff --git a/n.txt b/n.txt\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,3 @@\n+one\n+two\n+three\n",
                &[1],
                "diff --git a/n.txt b/n.txt\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1 @@\n+two\n",
            ),
        ];
        cases
            .into_iter()
            .map(|(name, text, ords, expected)| (name, text, ords.to_vec(), expected.to_string()))
            .collect()
    }

    fn lines_sel(ords: &[usize]) -> Selection {
        Selection {
            hunks: [(0usize, HunkSelection::Lines(ords.iter().copied().collect()))]
                .into_iter()
                .collect(),
        }
    }

    const TWO_STAGED_MODS: &str = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n-alpha\n+ALPHA\n-beta\n+BETA\n gamma\n";

    #[test]
    fn reverse_composition_keeps_an_unselected_modification_as_post_image_context() {
        let reverse = compose_reverse(TWO_STAGED_MODS, &lines_sel(&[2, 3]));
        let expected = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n ALPHA\n-beta\n+BETA\n gamma\n";
        assert_eq!(reverse, expected);
    }

    #[test]
    fn reverse_composition_of_a_staged_modification_and_addition_unstages_only_the_pair() {
        let staged = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,6 +1,7 @@\n one\n-two\n+TWO\n three\n four\n+extra\n five\n six\n";
        let reverse = compose_reverse(staged, &lines_sel(&[0, 1]));
        let expected = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,7 +1,7 @@\n one\n-two\n+TWO\n three\n four\n extra\n five\n six\n";
        assert_eq!(reverse, expected);
    }

    #[test]
    fn forward_filter_of_the_same_selection_is_unchanged_by_the_reverse_rule() {
        let forward = compose_patch(TWO_STAGED_MODS, &lines_sel(&[2, 3]));
        let expected = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n";
        assert_eq!(forward, expected);
    }
}
