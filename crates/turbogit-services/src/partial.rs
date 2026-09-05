//! Pure patch composition for partial staging (ADR-0013).
//!
//! Composes a stageable patch by filtering raw unified-diff text: git's
//! original `@@` headers and file meta lines are preserved (with line
//! counts recounted to match the kept body) while unselected hunks are
//! dropped. The engine applies patches with `git apply --recount`; the
//! recount here keeps libgit2 — which has no `--recount` knob —
//! honest about the body it ships.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::hunk_stats;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::ChangeStatus;
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

/// Compose `diff` down to the selected hunks of `selection`.
///
/// Returns an empty string when nothing is selected; callers treat that as a
/// no-op.
pub fn compose_patch(diff: &str, selection: &Selection) -> String {
    let mut meta = String::new();
    let mut hunks: Vec<(&str, String)> = Vec::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("@@") {
            hunks.push((line, String::new()));
        } else if let Some((_, body)) = hunks.last_mut() {
            body.push_str(line);
        } else {
            meta.push_str(line);
        }
    }

    let mut out = String::new();
    for (idx, (header, body)) in hunks.iter().enumerate() {
        let kept: Option<Cow<'_, str>> = match selection.hunks.get(&idx) {
            Some(HunkSelection::Whole) => Some(Cow::Borrowed(body.as_str())),
            Some(HunkSelection::Lines(lines)) if !lines.is_empty() => {
                Some(Cow::Owned(filter_body(body, &Filter::Lines(lines))))
            }
            Some(HunkSelection::Chars { ord, start, end }) => Some(Cow::Owned(filter_body(
                body,
                &Filter::Chars {
                    ord: *ord,
                    start: *start,
                    end: *end,
                },
            ))),
            _ => None,
        };
        if let Some(body) = kept {
            if out.is_empty() {
                out.push_str(&meta);
            }
            let header: Cow<'_, str> =
                if matches!(selection.hunks.get(&idx), Some(HunkSelection::Whole)) {
                    Cow::Borrowed(*header)
                } else {
                    Cow::Owned(recount_header(header, &body))
                };
            out.push_str(&header);
            out.push_str(&body);
        }
    }
    out
}

/// Count the old-side and new-side lines of a filtered hunk body so the
/// header can be rewritten to match. Context and kept `-` lines count on
/// the old side; context and kept `+` lines count on the new side. `\`
/// markers (`\ No newline at end of file`) ride on the changed line above
/// them and contribute nothing themselves.
fn body_line_counts(body: &str) -> (usize, usize) {
    let mut old = 0usize;
    let mut new = 0usize;
    for line in body.split_inclusive('\n') {
        if line.starts_with('+') {
            new += 1;
        } else if line.starts_with('-') {
            old += 1;
        } else if line.starts_with('\\') {
            // Marker for the changed line above; not counted.
        } else {
            old += 1;
            new += 1;
        }
    }
    (old, new)
}

/// Rewrite `header` so `-old_count` / `+new_count` match the lines actually
/// present in `body` — the count-1 default that the parser drops when
/// omitted is restored, and the optional trailing heading (`@@ … @@ fn alpha`)
/// is preserved verbatim.
///
/// Compose only drops additions and demotes deletions to context, never
/// removes context lines, so the first kept line is always context and
/// both start positions stay put. When the body genuinely shrinks on a
/// side (added line gone, no replacement), the count follows.
fn recount_header(header: &str, body: &str) -> String {
    let Some(span) = hunk_stats::parse_hunk_header(header) else {
        return header.to_owned();
    };
    let (old_count, new_count) = body_line_counts(body);
    // Keep the trailing heading after `@@ … @@` verbatim: empty, a
    // function label like `fn alpha`, or the diff-context marker. The
    // string after the last `@` is either the heading + `\n`, or just the
    // `\n` (no heading); treat any pure-whitespace remainder as no heading.
    // Header shape: `@@ -a,b +c,d @@ [<heading>]\n`.
    let tail = header
        .rsplit('@')
        .next()
        .map(|t| t.trim_start_matches(' ').trim_end_matches('\n'))
        .filter(|t| !t.is_empty())
        .unwrap_or("");
    let old = if old_count == 1 {
        format!("-{}", span.old_start)
    } else {
        format!("-{},{}", span.old_start, old_count)
    };
    let new = if new_count == 1 {
        format!("+{}", span.new_start)
    } else {
        format!("+{},{}", span.new_start, new_count)
    };
    if tail.is_empty() {
        format!("@@ {old} {new} @@\n")
    } else {
        format!("@@ {old} {new} @@ {tail}\n")
    }
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

/// Stage the selected hunks/lines of one file's diff into the index.
///
/// Composes the patch from raw diff text (ADR-0013) and applies it forward.
/// An empty selection is a no-op — nothing touches the engine. Conflicted
/// files are rejected before anything reaches the engine.
pub fn stage_selection(
    vcs: &dyn GitExecutor,
    root: &Path,
    diff: &str,
    selection: &Selection,
    status: ChangeStatus,
) -> TgResult<()> {
    ensure_granular_allowed(status)?;
    let patch = compose_patch(diff, selection);
    if patch.is_empty() {
        return Ok(());
    }
    retry_on_transient_lock(|| vcs.apply_patch_to_index(root, &patch, ApplyDirection::Forward))
}

/// Unstage the selected hunks/lines of one file's diff from the index.
///
/// Composes the patch from raw diff text (ADR-0013) and reverse-applies it
/// against the index (`git apply --cached --reverse`), matching git's own
/// unstage semantics. An empty selection is a no-op. Conflicted files are
/// rejected before anything reaches the engine.
pub fn unstage_selection(
    vcs: &dyn GitExecutor,
    root: &Path,
    diff: &str,
    selection: &Selection,
    status: ChangeStatus,
) -> TgResult<()> {
    ensure_granular_allowed(status)?;
    let patch = compose_patch(diff, selection);
    if patch.is_empty() {
        return Ok(());
    }
    retry_on_transient_lock(|| vcs.apply_patch_to_index(root, &patch, ApplyDirection::Reverse))
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
    diff: &str,
    selection: &Selection,
    status: ChangeStatus,
) -> TgResult<()> {
    ensure_granular_allowed(status)?;
    let patch = compose_patch(diff, selection);
    if patch.is_empty() {
        return Ok(());
    }
    // Both calls take the index lock (`add -N` writes the index too), so
    // each gets the transient-collision retry independently.
    retry_on_transient_lock(|| vcs.add_intent_to_add(root, paths))?;
    retry_on_transient_lock(|| vcs.apply_patch_to_index(root, &patch, ApplyDirection::Forward))
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
fn filter_body(body: &str, filter: &Filter) -> String {
    /// The selected slice of one addition body (char-indexed, clamped,
    /// end exclusive), or None when the range selects nothing.
    fn char_slice(text: &str, start: usize, end: usize) -> Option<String> {
        let chars: Vec<char> = text.chars().collect();
        let start = start.min(chars.len());
        let end = end.min(chars.len());
        (start < end).then(|| chars[start..end].iter().collect::<String>())
    }

    let mut out = String::new();
    let mut changed = 0usize;
    let mut prev_kept = true;
    for line in body.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix('-') {
            let keep = match filter {
                Filter::Lines(selected) => selected.contains(&changed),
                // A Chars selection never stages a deletion: the old line
                // stays in the index and only the selected bytes are added.
                Filter::Chars { .. } => false,
            };
            changed += 1;
            prev_kept = true;
            if keep {
                out.push_str(line);
            } else {
                out.push(' ');
                out.push_str(rest);
            }
        } else if let Some(rest) = line.strip_prefix('+') {
            let kept_line: Option<String> = match filter {
                Filter::Lines(selected) => selected.contains(&changed).then(|| line.to_owned()),
                Filter::Chars { ord, start, end } => {
                    // Char offsets index the line's text without its newline;
                    // the newline is re-added uniformly below.
                    let text = rest.strip_suffix('\n').unwrap_or(rest);
                    (changed == *ord)
                        .then(|| char_slice(text, *start, *end))
                        .flatten()
                        .map(|s| format!("+{s}\n"))
                }
            };
            changed += 1;
            prev_kept = kept_line.is_some();
            if let Some(kept_line) = kept_line {
                out.push_str(&kept_line);
            }
        } else if line.starts_with('\\') {
            if prev_kept {
                out.push_str(line);
            }
        } else {
            prev_kept = true;
            out.push_str(line);
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
            TWO_HUNK_DIFF,
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
            TWO_HUNK_DIFF,
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
            TWO_HUNK_DIFF,
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
            TWO_HUNK_DIFF,
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
            UNTRACKED_DIFF,
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
            UNTRACKED_DIFF,
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
            TWO_HUNK_DIFF,
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
