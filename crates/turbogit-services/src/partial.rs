//! Pure patch composition for partial staging (ADR-0013).
//!
//! Composes a stageable patch by filtering raw unified-diff text: git's
//! original `@@` headers and file meta lines are preserved verbatim while
//! unselected hunks are dropped. Line counts inside kept headers are left
//! untouched; the engine applies patches with `git apply --recount`.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use std::path::{Path, PathBuf};
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
                Some(Cow::Owned(filter_body(body, lines)))
            }
            _ => None,
        };
        if let Some(body) = kept {
            if out.is_empty() {
                out.push_str(&meta);
            }
            out.push_str(header);
            out.push_str(&body);
        }
    }
    out
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

/// Keep context lines and the selected changed lines of one hunk body.
///
/// An unselected addition is dropped entirely (it must not come into
/// existence in the staged result), while an unselected deletion survives on
/// both sides and is therefore re-emitted as a context line — removing it
/// outright would misalign every later old-side line of the hunk. A
/// `\ No newline at end of file` marker annotates the changed line above it,
/// so it survives only when that line does.
fn filter_body(body: &str, selected: &BTreeSet<usize>) -> String {
    let mut out = String::new();
    let mut changed = 0usize;
    let mut prev_kept = true;
    for line in body.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix('-') {
            let keep = selected.contains(&changed);
            changed += 1;
            prev_kept = true;
            if keep {
                out.push_str(line);
            } else {
                out.push(' ');
                out.push_str(rest);
            }
        } else if line.starts_with('+') {
            let keep = selected.contains(&changed);
            changed += 1;
            prev_kept = keep;
            if keep {
                out.push_str(line);
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
    use turbogit_engine_api::ApplyDirection;
    use turbogit_engine_api::fake::{Call, FakeExecutor};

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
