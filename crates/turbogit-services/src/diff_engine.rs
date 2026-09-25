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
use turbogit_domain::model::{
    DiffOpts, Patch, PatchFile, PatchHeaderLine, PatchHunk, PatchLine, PatchLineKind,
};
use turbogit_engine_api::GitExecutor;

/// Context lines around each change cluster — git's default.
const CONTEXT_RADIUS: usize = 3;

// --- unified patch production -------------------------------------------------

/// The diff `opts` describes, as a [`Patch`]: computed in-process from the two
/// file versions when they are readable through the engine seam, otherwise the
/// executor's own answer (same errors, same shape).
///
/// In-process requires a single-path, non-stat patch with no explicit commit
/// target; see [`in_process`] for the exact side resolution and fallback
/// rules.
pub fn patch(exec: &dyn GitExecutor, root: &Path, opts: &DiffOpts) -> TgResult<Patch> {
    match in_process(exec, root, opts) {
        Some(patch) => patch,
        None => exec.diff_patch(root, opts),
    }
}

/// In-process patch for `opts`, or `None` when the CLI path must stay
/// authoritative.
///
/// Side resolution mirrors exactly how the viewer requests a diff:
///
/// | comparison       | old side   | new side     |
/// |------------------|------------|--------------|
/// | Repo (HEAD↔wt)   | `HEAD`     | worktree fs  |
/// | Staged (HEAD↔ix) | `HEAD`     | index side   |
/// | Local (ix↔wt)    | index side | worktree fs  |
/// | explicit l..r    | `<left>`   | `<right>`    |
///
/// An unreadable old side means git knows something this module cannot
/// reconstruct in-process (rename sources, newly added files carry rename /
/// new-file metadata we would have to guess), so those fall back. A new-side
/// read error is not proof of deletion: a path the index holds no entry for,
/// an invalid revision, and filesystem failures all look the same here.
/// Fall back for those errors too, letting git distinguish actual deletions.
fn in_process(exec: &dyn GitExecutor, root: &Path, opts: &DiffOpts) -> Option<TgResult<Patch>> {
    // Single-path full patches only; whole-tree, stat, and commit-scoped
    // requests keep their CLI semantics.
    let path = opts
        .path
        .as_ref()
        .filter(|_| !opts.stat && opts.commit.is_none())?;
    let rel = forward_slashes(path);

    let old = match (&opts.left, opts.staged) {
        (Some(rev), _) => exec.show_file_bytes(root, rev, path),
        (None, true) => exec.show_file_bytes(root, "HEAD", path),
        (None, false) => exec.index_file_bytes(root, path),
    };
    let old = match old {
        Ok(bytes) => bytes,
        Err(_) => return None,
    };

    let new = match &opts.right {
        Some(rev) => exec.show_file_bytes(root, rev, path).ok()?,
        None if opts.staged => exec.index_file_bytes(root, path).ok()?,
        None => std::fs::read(root.join(path)).ok()?,
    };

    // Non-UTF-8 content stays CLI territory: git's own rendering of it
    // (binary detection quirks included) is what parity is measured against.
    let old = String::from_utf8(old).ok()?;
    let new = String::from_utf8(new).ok()?;
    Some(Ok(file_patch(
        &rel,
        Some(old.as_str()),
        Some(new.as_str()),
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

/// The in-process answer for one file section, as a patch **value**. A `None`
/// side is a creation or a deletion; equal text contents produce no section at
/// all, exactly like `git diff` printing nothing for an unchanged path.
///
/// The hunk spans are computed from `similar`'s own ranges — git's 1-based
/// start, with a zero-length side pointing at the line it lands after — and the
/// section heading comes from the same funcname heuristic the text producer
/// used. Both are fields of the value now rather than a string arranged to
/// match git byte for byte, which is what `ADR-0022` raised this seam for.
pub fn file_patch(
    rel: &str,
    old: Option<&str>,
    new: Option<&str>,
    ignore_whitespace: bool,
) -> Patch {
    if old.is_none() && new.is_none() {
        return Patch::default();
    }
    let mut headers = Vec::new();
    match (old, new) {
        (None, Some(_)) => headers.push(PatchHeaderLine::NewFile {
            mode: "100644".to_string(),
        }),
        (Some(_), None) => headers.push(PatchHeaderLine::DeletedFile {
            mode: "100644".to_string(),
        }),
        _ => {}
    }
    let source = |side: Option<&str>, prefix: &str| {
        if side.is_some() {
            format!("{prefix}{rel}")
        } else {
            "/dev/null".to_string()
        }
    };
    let (a, b) = (source(old, "a/"), source(new, "b/"));
    if old.is_some_and(is_binary) || new.is_some_and(is_binary) {
        headers.push(PatchHeaderLine::Binary);
        return one_file(rel, headers, Vec::new());
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

    let mut hunks: Vec<PatchHunk> = Vec::new();
    for hunk in unified.iter_hunks() {
        let ops = hunk.ops();
        let old_at = ops.first().map_or(0, |op| op.old_range().start);
        let new_at = ops.first().map_or(0, |op| op.new_range().start);
        let mut lines: Vec<PatchLine> = Vec::new();
        let mut old_count = 0usize;
        let mut new_count = 0usize;
        for change in hunk.iter_changes() {
            let kind = match change.tag() {
                ChangeTag::Equal => {
                    old_count += 1;
                    new_count += 1;
                    PatchLineKind::Context
                }
                ChangeTag::Delete => {
                    old_count += 1;
                    PatchLineKind::Removed
                }
                ChangeTag::Insert => {
                    new_count += 1;
                    PatchLineKind::Added
                }
            };
            lines.push(PatchLine {
                kind,
                text: change
                    .value()
                    .strip_suffix('\n')
                    .unwrap_or(change.value())
                    .to_string(),
                // git terminates the bare record first, then flags it on its
                // own line — here the flag is a property of the line.
                no_newline: diff.newline_terminated() && change.missing_newline(),
            });
        }
        if lines.is_empty() {
            continue;
        }
        hunks.push(PatchHunk {
            old_start: if old_count == 0 { old_at } else { old_at + 1 },
            old_count,
            new_start: if new_count == 0 { new_at } else { new_at + 1 },
            new_count,
            heading: section_heading(old.unwrap_or(""), old_at),
            lines,
        });
    }
    if hunks.is_empty() {
        // No textual changes — git prints nothing at all for the path.
        return Patch::default();
    }
    headers.push(PatchHeaderLine::Sources { old: a, new: b });
    one_file(rel, headers, hunks)
}

/// One section, with its paths recorded repo-relative — the value's form. The
/// `a/`/`b/` prefixes live in the source pair, not baked into a name.
fn one_file(rel: &str, headers: Vec<PatchHeaderLine>, hunks: Vec<PatchHunk>) -> Patch {
    Patch {
        files: vec![PatchFile {
            old_path: rel.to_string(),
            new_path: rel.to_string(),
            combined: false,
            headers,
            hunks,
        }],
    }
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
    fn unreadable_worktree_falls_back_instead_of_synthesizing_deletion() {
        let root = tempfile::tempdir().unwrap();
        let path = Path::new("file.txt");
        let exec = turbogit_engine::fake::FakeExecutor::new();
        exec.files
            .lock()
            .unwrap()
            .insert(path.into(), "old\n".into());
        // Reading a directory fails, but that is not evidence of deletion.
        std::fs::create_dir(root.path().join(path)).unwrap();
        let opts = DiffOpts {
            path: Some(path.into()),
            ..DiffOpts::default()
        };
        assert!(in_process(&exec, root.path(), &opts).is_none());
        // Falling back means the engine's own answer, unchanged: no synthesized
        // deletion section.
        assert_eq!(
            patch(&exec, root.path(), &opts).unwrap(),
            exec.diff_patch(root.path(), &opts).unwrap()
        );
    }

    #[test]
    fn unreadable_revision_falls_back_and_real_deletions_keep_cli_semantics() {
        let root = tempfile::tempdir().unwrap();
        let exec = turbogit_engine::cli::CliExecutor {
            settings: Default::default(),
        };
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(root.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        std::fs::write(root.path().join("file.txt"), "old\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "base",
        ]);
        let opts = DiffOpts {
            path: Some("file.txt".into()),
            left: Some("HEAD".into()),
            right: Some("missing-revision".into()),
            ..DiffOpts::default()
        };
        assert!(in_process(&exec, root.path(), &opts).is_none());
        assert!(patch(&exec, root.path(), &opts).is_err());

        std::fs::remove_file(root.path().join("file.txt")).unwrap();
        for staged in [false, true] {
            if staged {
                git(&["add", "-u"]);
            }
            let opts = DiffOpts {
                path: Some("file.txt".into()),
                staged,
                ..DiffOpts::default()
            };
            let answer = patch(&exec, root.path(), &opts).unwrap();
            let file = &answer.files[0];
            assert!(file.deleted_file(), "a real deletion says so: {answer}");
            assert!(
                file.hunks
                    .iter()
                    .flat_map(|h| &h.lines)
                    .any(|l| l.kind == PatchLineKind::Removed && l.text == "old"),
                "the removed line reaches the value: {answer}"
            );
        }
    }

    #[test]
    fn file_patch_renders_git_shaped_modify() {
        let patch = file_patch("src/x.txt", Some("a\nb\nc\n"), Some("a\nB\nc\n"), false);
        assert_eq!(
            patch.to_string(),
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
        let hunk = &patch.files[0].hunks[0];
        assert_eq!((hunk.old_count, hunk.new_count), (1, 1));
        // The span is fields; the spelling git uses for them is derived.
        assert_eq!(hunk.header_line(), "@@ -1 +1 @@");
    }

    #[test]
    fn file_patch_new_and_deleted_files_use_dev_null() {
        let added = file_patch("n.txt", None, Some("hi\n"), false);
        let text = added.to_string();
        assert!(
            text.starts_with("diff --git a/n.txt b/n.txt\nnew file mode 100644\n"),
            "{text}"
        );
        assert!(text.contains("--- /dev/null\n+++ b/n.txt\n"), "{text}");
        assert!(text.contains("@@ -0,0 +1 @@\n+hi\n"), "{text}");
        assert!(added.files[0].new_file());

        let deleted = file_patch("n.txt", Some("hi\n"), None, false);
        let text = deleted.to_string();
        assert!(
            text.starts_with("diff --git a/n.txt b/n.txt\ndeleted file mode 100644\n"),
            "{text}"
        );
        assert!(text.contains("--- a/n.txt\n+++ /dev/null\n"), "{text}");
        assert!(text.contains("@@ -1 +0,0 @@\n-hi\n"), "{text}");
        assert!(deleted.files[0].deleted_file());
    }

    #[test]
    fn file_patch_binary_sides_render_the_marker_section() {
        let patch = file_patch("b.dat", Some("\0old"), Some("\0new"), false);
        assert!(patch.files[0].binary());
        assert_eq!(
            patch.to_string(),
            "diff --git a/b.dat b/b.dat\nBinary files a/b.dat and b/b.dat differ\n"
        );
        // A binary deletion keeps both the mode header and /dev/null wording.
        let deleted = file_patch("b.dat", Some("\0old"), None, false);
        let text = deleted.to_string();
        assert!(
            text.contains("deleted file mode 100644\n")
                && text.ends_with("Binary files a/b.dat and /dev/null differ\n"),
            "{text}"
        );
    }

    #[test]
    fn file_patch_equal_contents_is_empty_like_git() {
        assert!(file_patch("x.txt", Some("same\n"), Some("same\n"), false).is_empty());
        assert!(file_patch("x.txt", None, None, false).is_empty());
    }

    #[test]
    fn file_patch_marks_missing_trailing_newlines_like_git() {
        // Losing the final newline: only the new side carries the hint.
        let patch = file_patch("x.txt", Some("end\n"), Some("end"), false);
        assert_eq!(flagged(&patch), vec!["+end"]);
        assert!(
            patch
                .to_string()
                .contains("-end\n+end\n\\ No newline at end of file\n"),
            "{patch}"
        );
        // Two unterminated sides: git marks both.
        let patch = file_patch("x.txt", Some("end"), Some("other"), false);
        assert_eq!(flagged(&patch), vec!["-end", "+other"]);
        assert!(
            patch.to_string().contains(
                "-end\n\\ No newline at end of file\n+other\n\\ No newline at end of file\n"
            ),
            "{patch}"
        );
    }

    /// The body lines a section flags as unterminated, with git's prefix.
    fn flagged(patch: &Patch) -> Vec<String> {
        patch
            .files
            .iter()
            .flat_map(|f| &f.hunks)
            .flat_map(|h| &h.lines)
            .filter(|l| l.no_newline)
            .map(|l| {
                format!(
                    "{}{}",
                    match l.kind {
                        PatchLineKind::Added => '+',
                        PatchLineKind::Removed => '-',
                        PatchLineKind::Context => ' ',
                    },
                    l.text
                )
            })
            .collect()
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
