//! Hunk-span statistics over the **Git engine**'s patch value (issue 20,
//! screen 06).
//!
//! Pure classification: which files a whole-root diff touches, the span of each
//! hunk, and — by comparing the three working-tree views of one file
//! (HEAD↔worktree, HEAD↔index, index↔worktree) — how much of each hunk is
//! already staged. No engine access; callers read the patch value through the
//! [`crate::changes`] seam and cache the result per root.

use turbogit_domain::model::Patch;

/// One hunk's line-range span, parsed from its `@@ -a,b +c,d @@` header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HunkSpan {
    /// 1-based old-file start line.
    pub old_start: usize,
    /// Old-file line count (0 for pure insertions).
    pub old_lines: usize,
    /// 1-based new-file start line.
    pub new_start: usize,
    /// New-file line count (0 for pure deletions).
    pub new_lines: usize,
}

/// How much of a HEAD↔worktree hunk is already in the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagedState {
    /// Every changed line of the hunk is staged.
    Staged,
    /// Some but not all of the hunk's changed lines are staged.
    Partial,
    /// Nothing of the hunk is staged.
    Unstaged,
}

/// Per-file hunk spans of one multi-file unified diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHunks {
    /// Repo-relative path (git's `b/` side of the `diff --git` line).
    pub path: String,
    /// Spans in diff order.
    pub hunks: Vec<HunkSpan>,
}

/// The per-file hunk spans of a patch value — the same list one file section
/// answers with, so the whole-root listings and the display model read one
/// parser of one shape (`ADR-0022`). The four numbers a header carries are
/// fields here; nothing re-reads `@@ -a,b +c,d`.
pub fn file_hunks(patch: &Patch) -> Vec<FileHunks> {
    patch
        .files
        .iter()
        .map(|file| FileHunks {
            path: file.new_path.clone(),
            hunks: file
                .hunks
                .iter()
                .map(|h| HunkSpan {
                    old_start: h.old_start,
                    old_lines: h.old_count,
                    new_start: h.new_start,
                    new_lines: h.new_count,
                })
                .collect(),
        })
        .collect()
}

/// Whether two line ranges touch. Zero-length ranges (pure insertions) act
/// as the single point at their start line; git hunk ranges sharing the
/// HEAD old side overlap only when one contains the other's anchor.
fn ranges_touch(start_a: usize, len_a: usize, start_b: usize, len_b: usize) -> bool {
    let end_a = if len_a == 0 {
        start_a
    } else {
        start_a + len_a - 1
    };
    let end_b = if len_b == 0 {
        start_b
    } else {
        start_b + len_b - 1
    };
    start_a <= end_b && start_b <= end_a
}

/// Classify a HEAD↔worktree hunk against the HEAD↔index (staged) hunks of
/// the same file. Both diffs share the HEAD old side, so old-line ranges
/// compare directly: an equal span means fully staged, an overlapping one
/// partially staged.
pub fn hunk_staged_state(hunk: &HunkSpan, staged: &[HunkSpan]) -> StagedState {
    for s in staged {
        if s == hunk {
            return StagedState::Staged;
        }
    }
    for s in staged {
        if ranges_touch(s.old_start, s.old_lines, hunk.old_start, hunk.old_lines) {
            return StagedState::Partial;
        }
    }
    StagedState::Unstaged
}

/// Whether a staged hunk still has an unstaged remainder: an index↔worktree
/// hunk touching the staged hunk's new-side (index) line range.
pub fn staged_hunk_partial(staged: &HunkSpan, local: &[HunkSpan]) -> bool {
    local
        .iter()
        .any(|l| ranges_touch(staged.new_start, staged.new_lines, l.old_start, l.old_lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// git's diff syntax has exactly one reader, and it lives with the
    /// adapters. A services test may build a value with it; nothing in this
    /// crate's production code does.
    fn patch(text: &str) -> Patch {
        turbogit_engine::patch::parse_patch(text)
    }

    /// Two well-separated hunks in one file, as `git diff HEAD` emits.
    const TWO_HUNK_DIFF: &str = "\
diff --git a/src/a.rs b/src/a.rs
index 1111111..2222222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,4 +10,5 @@ fn alpha
 context
-removed
+added
+added2
 context
@@ -80,3 +81,3 @@ fn omega
 ctx
-old
+new
 ctx
diff --git a/src/b.rs b/src/b.rs
index 3333333..4444444 100644
--- a/src/b.rs
+++ b/src/b.rs
@@ -1 +1 @@
-a
+b
";

    #[test]
    fn file_hunks_split_per_file_with_repo_relative_paths() {
        let files = file_hunks(&patch(TWO_HUNK_DIFF));
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/a.rs");
        assert_eq!(
            files[0].hunks,
            vec![
                HunkSpan {
                    old_start: 10,
                    old_lines: 4,
                    new_start: 10,
                    new_lines: 5,
                },
                HunkSpan {
                    old_start: 80,
                    old_lines: 3,
                    new_start: 81,
                    new_lines: 3,
                },
            ]
        );
        assert_eq!(files[1].path, "src/b.rs");
        assert_eq!(files[1].hunks.len(), 1);
    }

    #[test]
    fn staged_state_matches_equal_spans_and_flags_overlap() {
        let hunk = HunkSpan {
            old_start: 10,
            old_lines: 4,
            new_start: 10,
            new_lines: 5,
        };
        // Hunk staged exactly: the staged diff carries the identical span.
        let staged = vec![HunkSpan {
            old_start: 10,
            old_lines: 4,
            new_start: 10,
            new_lines: 5,
        }];
        assert_eq!(hunk_staged_state(&hunk, &staged), StagedState::Staged);

        // Only part of the hunk staged: the staged hunk's old range is a
        // subset (one of two deletions staged).
        let partial = vec![HunkSpan {
            old_start: 10,
            old_lines: 2,
            new_start: 10,
            new_lines: 1,
        }];
        assert_eq!(hunk_staged_state(&hunk, &partial), StagedState::Partial);

        // Extra unstaged work merged into the same hunk: same old range but
        // a wider new side.
        let extended = vec![HunkSpan {
            old_start: 10,
            old_lines: 4,
            new_start: 10,
            new_lines: 5,
        }];
        let grown = HunkSpan {
            new_lines: 6,
            ..hunk
        };
        assert_eq!(hunk_staged_state(&grown, &extended), StagedState::Partial);

        // Nothing staged near the hunk.
        let far = vec![HunkSpan {
            old_start: 80,
            old_lines: 3,
            new_start: 81,
            new_lines: 3,
        }];
        assert_eq!(hunk_staged_state(&hunk, &far), StagedState::Unstaged);
        assert_eq!(hunk_staged_state(&hunk, &[]), StagedState::Unstaged);
    }

    #[test]
    fn staged_state_handles_pure_insertion_spans() {
        // A staged pure addition at the same insertion point is exact.
        let hunk = HunkSpan {
            old_start: 5,
            old_lines: 0,
            new_start: 6,
            new_lines: 2,
        };
        assert_eq!(
            hunk_staged_state(
                &hunk,
                &[HunkSpan {
                    old_start: 5,
                    old_lines: 0,
                    new_start: 6,
                    new_lines: 2,
                }]
            ),
            StagedState::Staged
        );
        // A different insertion point does not match.
        assert_eq!(
            hunk_staged_state(
                &hunk,
                &[HunkSpan {
                    old_start: 6,
                    old_lines: 0,
                    new_start: 7,
                    new_lines: 2,
                }]
            ),
            StagedState::Unstaged
        );
    }

    #[test]
    fn staged_hunk_partial_flags_local_overlap_only() {
        let staged = HunkSpan {
            old_start: 10,
            old_lines: 4,
            new_start: 10,
            new_lines: 5,
        };
        // An unstaged remainder inside the staged hunk's index range.
        let remainder = vec![HunkSpan {
            old_start: 12,
            old_lines: 2,
            new_start: 11,
            new_lines: 1,
        }];
        assert!(staged_hunk_partial(&staged, &remainder));
        // Unstaged work far away leaves the staged hunk complete.
        let elsewhere = vec![HunkSpan {
            old_start: 50,
            old_lines: 3,
            new_start: 50,
            new_lines: 3,
        }];
        assert!(!staged_hunk_partial(&staged, &elsewhere));
        assert!(!staged_hunk_partial(&staged, &[]));
    }
}
