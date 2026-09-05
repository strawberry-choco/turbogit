//! Hunk-span statistics over unified diff text (issue 20, screen 06).
//!
//! Pure parsing and classification: which files a whole-root diff touches,
//! the `@@ -a,b +c,d @@` span of each hunk, and — by comparing the three
//! working-tree views of one file (HEAD↔worktree, HEAD↔index,
//! index↔worktree) — how much of each hunk is already staged. No engine
//! access; callers fetch the diff text through the [`crate::changes`] seam
//! and cache the result per root.

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

/// Parse `@@ -a,b +c,d @@ …` into a [`HunkSpan`]; counts default to 1 when
/// omitted, and a header without both sides is `None`.
pub fn parse_hunk_header(header: &str) -> Option<HunkSpan> {
    let mut old: Option<(usize, usize)> = None;
    let mut new: Option<(usize, usize)> = None;
    for tok in header.split_whitespace() {
        if let Some(rest) = tok.strip_prefix('-') {
            old = Some(parse_range(rest));
        } else if let Some(rest) = tok.strip_prefix('+') {
            new = Some(parse_range(rest));
        }
    }
    // `-`/`+` also prefix diff body lines, so the parse only counts when the
    // line carried both a `-` and a `+` range (a lone `-` body line yields
    // None).
    let (o, n) = (old?, new?);
    Some(HunkSpan {
        old_start: o.0,
        old_lines: o.1,
        new_start: n.0,
        new_lines: n.1,
    })
}

/// Parse git's `-a,b` / `+c,d` range token; the count defaults to 1.
fn parse_range(rest: &str) -> (usize, usize) {
    let mut parts = rest.split(',');
    let start = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let lines = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1);
    (start, lines)
}

/// Split a multi-file unified diff into per-file hunk spans. Lines before
/// the first `diff --git` header belong to no file.
pub fn parse_file_hunks(diff: &str) -> Vec<FileHunks> {
    let mut files: Vec<FileHunks> = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = git_b_side(rest);
            files.push(FileHunks {
                path,
                hunks: Vec::new(),
            });
        } else if line.starts_with("@@")
            && let Some(file) = files.last_mut()
            && let Some(span) = parse_hunk_header(line)
        {
            file.hunks.push(span);
        }
    }
    files
}

/// Repo-relative path from the remainder of a `diff --git` line: git's
/// `b/` side (the post-change path), matching the viewer's row model.
fn git_b_side(rest: &str) -> String {
    match rest.find(" b/") {
        Some(sep) => rest[sep + 3..].to_owned(),
        None => rest.to_owned(),
    }
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
    fn hunk_header_parses_spans_with_default_counts() {
        assert_eq!(
            parse_hunk_header("@@ -10,4 +10,5 @@ fn alpha"),
            Some(HunkSpan {
                old_start: 10,
                old_lines: 4,
                new_start: 10,
                new_lines: 5,
            })
        );
        // Counts default to 1 when omitted.
        assert_eq!(
            parse_hunk_header("@@ -1 +1 @@"),
            Some(HunkSpan {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
            })
        );
        // Pure insertion: zero-length old range at the insertion point.
        assert_eq!(
            parse_hunk_header("@@ -5,0 +6,2 @@"),
            Some(HunkSpan {
                old_start: 5,
                old_lines: 0,
                new_start: 6,
                new_lines: 2,
            })
        );
        assert_eq!(parse_hunk_header("not a header"), None);
    }

    #[test]
    fn file_hunks_split_per_file_with_repo_relative_paths() {
        let files = parse_file_hunks(TWO_HUNK_DIFF);
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
