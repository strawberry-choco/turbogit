//! Hunk-span statistics over the **Git engine**'s patch value.
//!
//! Pure classification: which files a whole-root diff touches, and the span of
//! each hunk. No engine access; callers read the patch value through the
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

#[cfg(test)]
mod tests {
    use super::*;

    /// git's diff syntax has exactly one reader, and it lives with the
    /// adapters. A services test may build a value with it; nothing in this
    /// crate's production code does.
    fn patch(text: &str) -> Patch {
        turbogit_engine_api::patch::parse_patch(text)
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
}
