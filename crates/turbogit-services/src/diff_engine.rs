//! In-process diff engine (library-migration plan Phase L1).
//!
//! Computes unified diffs with the `similar` crate instead of shelling out to
//! `git diff`, while every downstream consumer stays untouched: the producer
//! emits **git-shaped unified patch text**, so the cached raw text, the row
//! parser, the display model, per-file metadata scanning, and granular
//! staging — which composes its patches from the cached raw text (ADR-0013) —
//! all keep working verbatim. Whenever an in-process computation cannot be
//! performed confidently, [`diff_text`] delegates to the executor's CLI text
//! path unchanged: multi-file or stat targets, unreadable revisions (rename
//! sources, newly added files), and non-UTF-8 content all stay authoritative
//! CLI territory.
//!
//! The module also hosts the structured 3-way merge ([`merge_segments`])
//! backing the conflict editor: ordered `(ours, theirs, is_conflict)`
//! segments with the same shape as the raw-marker parser's tuples, built from
//! `similar`'s merge regions instead of parsing conflict markers.

use std::path::Path;

use similar::{ChangeTag, MergeResolution, TextDiffConfig, TextMerge, WhitespaceMode};

use turbogit_domain::error::TgResult;
use turbogit_domain::model::DiffOpts;
use turbogit_engine_api::GitExecutor;

/// Context lines around each change cluster — git's default.
const CONTEXT_RADIUS: usize = 3;

// --- unified patch production -------------------------------------------------

/// Unified diff text for `opts`: computed in-process from the two file
/// versions when they are readable through the engine seam, otherwise the
/// executor's CLI text verbatim (same errors, same shape).
///
/// In-process requires a single-path, non-stat patch with no explicit commit
/// target; see [`in_process`] for the exact side resolution and fallback
/// rules.
pub fn diff_text(exec: &dyn GitExecutor, root: &Path, opts: &DiffOpts) -> TgResult<String> {
    match in_process(exec, root, opts) {
        Some(patch) => patch,
        None => exec.diff(root, opts),
    }
}

/// In-process patch for `opts`, or `None` when the CLI path must stay
/// authoritative.
///
/// Side resolution mirrors exactly how the viewer requests diff text:
///
/// | comparison       | old side   | new side     |
/// |------------------|------------|--------------|
/// | Repo (HEAD↔wt)   | `HEAD`     | worktree fs  |
/// | Staged (HEAD↔ix) | `HEAD`     | index `:0`   |
/// | Local (ix↔wt)    | index `:0` | worktree fs  |
/// | explicit l..r    | `<left>`   | `<right>`    |
///
/// An unreadable old side means git knows something this module cannot
/// reconstruct in-process (rename sources, newly added files carry rename /
/// new-file metadata we would have to guess), so those fall back. An
/// unreadable new side is an unambiguous deletion — the status scan saw the
/// file absent there — and renders as a `/dev/null` patch like git does.
fn in_process(exec: &dyn GitExecutor, root: &Path, opts: &DiffOpts) -> Option<TgResult<String>> {
    // Single-path full patches only; whole-tree, stat, and commit-scoped
    // requests keep their CLI semantics.
    let path = opts
        .path
        .as_ref()
        .filter(|_| !opts.stat && opts.commit.is_none())?;
    let rel = forward_slashes(path);

    let old_rev = opts.left.clone().unwrap_or_else(|| {
        if opts.staged {
            "HEAD".to_owned()
        } else {
            ":0".to_owned()
        }
    });
    let old = match exec.show_file_bytes(root, &old_rev, path) {
        Ok(bytes) => bytes,
        Err(_) => return None,
    };

    let new = match &opts.right {
        Some(rev) => exec.show_file_bytes(root, rev, path).ok(),
        None if opts.staged => exec.show_file_bytes(root, ":0", path).ok(),
        None => std::fs::read(root.join(path)).ok(),
    };

    // Non-UTF-8 content stays CLI territory: git's own rendering of it
    // (binary detection quirks included) is what parity is measured against.
    let old = String::from_utf8(old).ok()?;
    let new = match new {
        Some(bytes) => Some(String::from_utf8(bytes).ok()?),
        None => None,
    };
    Some(Ok(file_patch(
        &rel,
        Some(old.as_str()),
        new.as_deref(),
        opts.ignore_whitespace,
    )))
}

/// Repo-relative display form for patch headers: forward slashes throughout,
/// matching how git spells paths in unified output on every platform.
fn forward_slashes(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// NUL-byte sniff — git's own binaryness test for blob contents.
fn is_binary(content: &str) -> bool {
    content.as_bytes().contains(&0)
}

/// Git-shaped unified patch for one file section from its two sides, `None`
/// being an absent side (`/dev/null`). Covers creation (`new file mode`),
/// deletion (`deleted file mode`), binary changes (the one-line
/// `Binary files … differ` section), and `\ No newline at end of file`
/// hints; equal text contents produce the empty patch, exactly like
/// `git diff` prints nothing for an unchanged path.
pub fn file_patch(
    rel: &str,
    old: Option<&str>,
    new: Option<&str>,
    ignore_whitespace: bool,
) -> String {
    if old.is_none() && new.is_none() {
        return String::new();
    }
    let mut out = format!("diff --git a/{rel} b/{rel}\n");
    match (old, new) {
        (None, Some(_)) => out.push_str("new file mode 100644\n"),
        (Some(_), None) => out.push_str("deleted file mode 100644\n"),
        _ => {}
    }
    let a = if old.is_some() {
        format!("a/{rel}")
    } else {
        "/dev/null".to_owned()
    };
    let b = if new.is_some() {
        format!("b/{rel}")
    } else {
        "/dev/null".to_owned()
    };
    if old.is_some_and(is_binary) || new.is_some_and(is_binary) {
        out.push_str(&format!("Binary files {a} and {b} differ\n"));
        return out;
    }

    let diff = TextDiffConfig::default()
        .whitespace_mode(if ignore_whitespace {
            WhitespaceMode::IgnoreAll
        } else {
            WhitespaceMode::Exact
        })
        .diff_lines(old.unwrap_or(""), new.unwrap_or(""));
    let mut unified = diff.unified_diff();
    unified.context_radius(CONTEXT_RADIUS);

    // Hunks are serialized by hand instead of via `UnifiedDiff`'s Display so
    // headers can carry the section heading git appends (`@@ … @@ name`),
    // which similar's formatter leaves out.
    let mut body = String::new();
    for hunk in unified.iter_hunks() {
        let old_start = hunk.ops().first().map_or(0, |op| op.old_range().start);
        body.push_str(&hunk.header().to_string());
        if let Some(heading) = section_heading(old.unwrap_or(""), old_start) {
            body.push(' ');
            body.push_str(&heading);
        }
        body.push('\n');
        for change in hunk.iter_changes() {
            body.push(match change.tag() {
                ChangeTag::Equal => ' ',
                ChangeTag::Delete => '-',
                ChangeTag::Insert => '+',
            });
            body.push_str(change.value());
            if !diff.newline_terminated() {
                body.push('\n');
            } else if change.missing_newline() {
                // git terminates the bare record first (`putc('\n')` in
                // DIFF_SYMBOL_NO_LF_EOF), then flags it on its own line.
                body.push('\n');
                body.push_str("\\ No newline at end of file\n");
            }
        }
    }
    if body.is_empty() {
        // No textual changes — git prints nothing at all for the path.
        return String::new();
    }
    out.push_str("--- ");
    out.push_str(&a);
    out.push('\n');
    out.push_str("+++ ");
    out.push_str(&b);
    out.push('\n');
    out.push_str(&body);
    out
}

/// Section heading git appends to a hunk header: the closest line strictly
/// before the hunk (in the old content) whose first byte is alphabetic, `_`,
/// or `$` — git's default funcname heuristic — truncated to 80 bytes with
/// trailing whitespace stripped. `next_old_line` is the hunk's first old-side
/// line index (0-based). No matching line means no heading, like git.
fn section_heading(old: &str, next_old_line: usize) -> Option<String> {
    /// git's `funcbuf[80]` heading cap.
    const HEADING_CAP: usize = 80;
    let mut heading = None;
    for line in old.lines().take(next_old_line) {
        if line
            .as_bytes()
            .first()
            .is_some_and(|&c| c.is_ascii_alphabetic() || c == b'_' || c == b'$')
        {
            heading = Some(line);
        }
    }
    heading.map(|line| {
        // Truncate bytes first, then strip trailing whitespace — git's
        // order — never splitting a UTF-8 code point.
        let mut cut = line.len().min(HEADING_CAP);
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line[..cut].trim_end().to_owned()
    })
}

// --- structured 3-way merge ---------------------------------------------------

/// Ordered merge segments from a structured 3-way merge of `base`, `ours`,
/// and `theirs` (via `similar::merge`): `(ours, theirs, is_conflict)` tuples
/// with the same shape the conflict editor's raw-marker parser produces —
/// normal segments carry their text in the first field, conflict segments
/// both sides. Non-overlapping edits fold into normal segments
/// automatically; only genuinely incompatible regions stay conflicts, so the
/// editor shows strictly fewer blocks than raw markers would.
///
/// Adjacent normal regions are flattened and empty ones dropped, mirroring
/// the marker parser's single-accumulating-normal-buffer behavior.
pub fn merge_segments(base: &str, ours: &str, theirs: &str) -> Vec<(String, String, bool)> {
    let merge = TextMerge::from_lines(base, ours, theirs);
    let mut segs: Vec<(String, String, bool)> = Vec::new();
    for region in merge.regions() {
        let mut ours_text = String::new();
        for i in region.ours_range() {
            if let Some(line) = merge.ours_line(i) {
                ours_text.push_str(line);
            }
        }
        let mut theirs_text = String::new();
        for i in region.theirs_range() {
            if let Some(line) = merge.theirs_line(i) {
                theirs_text.push_str(line);
            }
        }
        match region.resolution() {
            MergeResolution::Conflict => segs.push((ours_text, theirs_text, true)),
            MergeResolution::Theirs => push_normal(&mut segs, theirs_text),
            // Unchanged / Ours / Both all render our side (identical content
            // by definition for the first and last).
            _ => push_normal(&mut segs, ours_text),
        }
    }
    segs
}

/// Append a normal segment, flattening into a preceding normal segment and
/// skipping empties — the tuple stream equivalent of the marker parser's
/// accumulating normal buffer.
fn push_normal(segs: &mut Vec<(String, String, bool)>, text: String) {
    if text.is_empty() {
        return;
    }
    match segs.last_mut() {
        Some((normal, tail, false)) if tail.is_empty() => normal.push_str(&text),
        _ => segs.push((text, String::new(), false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_patch_renders_git_shaped_modify() {
        let patch = file_patch("src/x.txt", Some("a\nb\nc\n"), Some("a\nB\nc\n"), false);
        assert_eq!(
            patch,
            concat!(
                "diff --git a/src/x.txt b/src/x.txt\n",
                "--- a/src/x.txt\n",
                "+++ b/src/x.txt\n",
                "@@ -1,3 +1,3 @@\n",
                " a\n",
                "-b\n",
                "+B\n",
                " c\n",
            )
        );
    }

    #[test]
    fn file_patch_omits_count_one_hunk_ranges_like_git() {
        let patch = file_patch("x.txt", Some("a\n"), Some("b\n"), false);
        assert!(patch.contains("@@ -1 +1 @@\n"), "{patch}");
    }

    #[test]
    fn file_patch_new_and_deleted_files_use_dev_null() {
        let added = file_patch("n.txt", None, Some("hi\n"), false);
        assert!(
            added.starts_with("diff --git a/n.txt b/n.txt\nnew file mode 100644\n"),
            "{added}"
        );
        assert!(added.contains("--- /dev/null\n+++ b/n.txt\n"), "{added}");
        assert!(added.contains("@@ -0,0 +1 @@\n+hi\n"), "{added}");

        let deleted = file_patch("n.txt", Some("hi\n"), None, false);
        assert!(
            deleted.starts_with("diff --git a/n.txt b/n.txt\ndeleted file mode 100644\n"),
            "{deleted}"
        );
        assert!(
            deleted.contains("--- a/n.txt\n+++ /dev/null\n"),
            "{deleted}"
        );
        assert!(deleted.contains("@@ -1 +0,0 @@\n-hi\n"), "{deleted}");
    }

    #[test]
    fn file_patch_binary_sides_render_the_marker_section() {
        let patch = file_patch("b.dat", Some("\0old"), Some("\0new"), false);
        assert_eq!(
            patch,
            "diff --git a/b.dat b/b.dat\nBinary files a/b.dat and b/b.dat differ\n"
        );
        // A binary deletion keeps both the mode header and /dev/null wording.
        let deleted = file_patch("b.dat", Some("\0old"), None, false);
        assert!(
            deleted.contains("deleted file mode 100644\n")
                && deleted.ends_with("Binary files a/b.dat and /dev/null differ\n"),
            "{deleted}"
        );
    }

    #[test]
    fn file_patch_equal_contents_is_empty_like_git() {
        assert_eq!(
            file_patch("x.txt", Some("same\n"), Some("same\n"), false),
            ""
        );
        assert_eq!(file_patch("x.txt", None, None, false), "");
    }

    #[test]
    fn file_patch_marks_missing_trailing_newlines_like_git() {
        // Losing the final newline: only the new side carries the hint.
        let patch = file_patch("x.txt", Some("end\n"), Some("end"), false);
        assert!(
            patch.contains("-end\n+end\n\\ No newline at end of file\n"),
            "{patch}"
        );
        // Two unterminated sides: git marks both.
        let patch = file_patch("x.txt", Some("end"), Some("other"), false);
        assert!(
            patch.contains(
                "-end\n\\ No newline at end of file\n+other\n\\ No newline at end of file\n"
            ),
            "{patch}"
        );
    }

    #[test]
    fn merge_segments_conflict_matches_marker_shape() {
        let segs = merge_segments("one\ntwo\n", "one\nours\n", "one\ntheirs\n");
        assert_eq!(
            segs,
            vec![
                ("one\n".to_owned(), String::new(), false),
                ("ours\n".to_owned(), "theirs\n".to_owned(), true),
            ]
        );
    }

    #[test]
    fn merge_segments_autoresolves_non_overlapping_edits() {
        let base = "a\nb\nc\nd\ne\nf\ng\n";
        let ours = base.replace('b', "B");
        let theirs = base.replace('f', "F");
        let segs = merge_segments(base, &ours, &theirs);
        assert!(segs.iter().all(|(_, _, c)| !c), "{segs:?}");
        let composed: String = segs.iter().map(|(a, _, _)| a.as_str()).collect();
        assert_eq!(composed, "a\nB\nc\nd\ne\nF\ng\n");
    }

    #[test]
    fn merge_segments_flattens_adjacent_normals_and_skips_empties() {
        // Identical edits on both sides resolve as `Both` — one normal run.
        let segs = merge_segments("x\n", "y\n", "y\n");
        assert_eq!(segs, vec![("y\n".to_owned(), String::new(), false)]);
        // No regions at all when every input is empty.
        assert!(merge_segments("", "", "").is_empty());
    }
}
