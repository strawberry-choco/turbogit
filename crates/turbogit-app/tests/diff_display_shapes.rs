//! Issue 10 (deepen-git-engine) — every display shape, with no `git` binary.
//!
//! The diff view used to reach a rendered row through four private readers of
//! git's text (a `@@` header parser, a `diff --git` scanner, a path-pair
//! splitter and a prefix strip) plus a second parse of the same header at paint
//! time. They all read the patch value now, so a suite can hand the display
//! model every shape it has to survive by seeding the substitutable adapter with
//! one canned patch.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::repo_with_one_commit;
use turbogit_app::diff_model::{DisplayRow, RowKind};
use turbogit_app::keyed_read::{DiffTarget, Read};
use turbogit_app::state::{AppState, DiffComparison};
use turbogit_engine::fake::FakeExecutor;
use turbogit_services::hunk_stats::HunkSpan;

/// One committed file, so the headless harness has a root to read against.
///
/// The recipe's committed file is `README.md` rather than this builder's, and
/// nothing reads it: the patch every shape is rendered from is the canned
/// `EVERY_SHAPE` string handed to `FakeExecutor`, never read off the worktree.
fn repo(dir: &Path) -> PathBuf {
    repo_with_one_commit(dir, "work")
}

/// A patch holding every shape the row model has to render: an edit with a
/// funcname heading, a **Binary change**, a mode-only change, and a rename of a
/// path containing a space.
const EVERY_SHAPE: &str = "diff --git a/README.md b/README.md\n\
     index 1234567..89abcde 100644\n\
     --- a/README.md\n\
     +++ b/README.md\n\
     @@ -1,3 +1,3 @@ fn keep() {\n\
      one\n\
     -two\n\
     +TWO\n\
      three\n\
     diff --git a/logo.png b/logo.png\n\
     index aaa..bbb 100644\n\
     Binary files a/logo.png and b/logo.png differ\n\
     diff --git a/script.sh b/script.sh\n\
     old mode 100644\n\
     new mode 100755\n\
     diff --git a/keep me.txt b/kept me.txt\n\
     index ccc..ddd 100644\n\
     similarity index 100%\n\
     rename from keep me.txt\n\
     rename to kept me.txt\n";

fn settled_diff(work: &PathBuf) -> Arc<turbogit_app::diff_model::DiffValue> {
    let mut fake = FakeExecutor::default();
    fake.diffs
        .insert(work.to_path_buf(), EVERY_SHAPE.to_string());
    let mut state = AppState::for_roots(work.parent().unwrap(), std::slice::from_ref(work))
        .with_executor(Arc::new(fake));
    while state.drain_events() > 0 {}
    let target = DiffTarget::new(
        work.to_path_buf(),
        None,
        None,
        DiffComparison::Repo,
        false,
        Some(PathBuf::from("README.md")),
    );
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    state.drain_events();
    let Read::Fresh(diff) = state.read(target) else {
        panic!("the canned patch settles as the diff's answer");
    };
    assert!(
        diff.patch.files.iter().any(|f| f
            .hunks
            .iter()
            .any(|h| h.heading.as_deref() == Some("fn keep() {"))),
        "the read carries git's funcname heading as a field, not as text to \
         search: {}",
        diff.patch
    );
    diff
}

fn hunk_rows(model: &turbogit_app::diff_model::DiffModel) -> Vec<&str> {
    model
        .display
        .iter()
        .filter_map(|row| match row {
            DisplayRow::Full(r) if r.kind == RowKind::Hunk => Some(r.text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn every_shape_builds_rows_without_a_git_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let diff = settled_diff(&work);
    let model = &diff.model;

    assert_eq!(model.hunk_count(), 1, "only the edit has a hunk");
    assert_eq!(hunk_rows(model), vec!["@@ -1,3 +1,3 @@ fn keep() {"]);
    assert_eq!(
        model.files.len(),
        4,
        "one section per file, whatever shape it is"
    );

    let labeled: Vec<(&str, &str)> = model
        .files
        .iter()
        .map(|f| {
            (
                f.new_path.as_deref().unwrap_or("?"),
                if f.binary {
                    "binary"
                } else if f.renamed {
                    "renamed"
                } else if f.new_file {
                    "new"
                } else {
                    "changed"
                },
            )
        })
        .collect();
    assert_eq!(
        labeled,
        vec![
            ("README.md", "changed"),
            ("logo.png", "binary"),
            ("script.sh", "changed"),
            ("kept me.txt", "renamed"),
        ],
        "the space in a renamed path must not be read as a field boundary"
    );

    let rename = model.rename_header.as_deref();
    assert_eq!(
        rename,
        Some("Renamed from keep me.txt · 100% similar"),
        "the rename header names the repo-relative old side"
    );

    let (added, removed) = turbogit_app::diff_model::line_counts(model);
    assert_eq!((added, removed), (1, 1), "one line each way in the edit");
}

/// The ticket's visible win: a hunk header is read once, and the answer the
/// paint path needed — is this hunk staged? — arrives with the row.
#[test]
fn a_hunk_row_carries_its_span_so_the_paint_path_never_reparses_the_header() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let model = &settled_diff(&work).model;

    let spans: Vec<Option<HunkSpan>> = model
        .display
        .iter()
        .filter_map(|row| match row {
            DisplayRow::Full(r) if r.kind == RowKind::Hunk => Some(r.span),
            _ => None,
        })
        .collect();
    assert_eq!(
        spans,
        vec![Some(HunkSpan {
            old_start: 1,
            old_lines: 3,
            new_start: 1,
            new_lines: 3,
        })],
        "the span is on the row, not something to parse back out of its label"
    );

    // Gutter numbers come from the same place: the removed row knows its old
    // line, the added row its new one, and neither is counted from a prefix.
    // Side-by-side pairing lives in the display row, so read both shapes.
    let gutters: Vec<(RowKind, usize, usize)> = model
        .display
        .iter()
        .flat_map(|row| match row {
            DisplayRow::Full(r) => vec![(r.text.as_str(), r.kind, r.old_no, r.new_no)],
            DisplayRow::Pair(del, add) => {
                let mut sides = Vec::new();
                if let Some(d) = del {
                    sides.push((d.text.as_str(), d.kind, d.old_no, d.new_no));
                }
                if let Some(a) = add {
                    sides.push((a.text.as_str(), a.kind, a.old_no, a.new_no));
                }
                sides
            }
        })
        // The hunk's own body, identified by content rather than by position.
        .filter(|(text, _, _, _)| matches!(*text, "one" | "two" | "TWO" | "three"))
        .map(|(_, kind, old, new)| (kind, old, new))
        .collect();
    assert_eq!(
        gutters,
        vec![
            (RowKind::Context, 1, 1),
            (RowKind::Del, 2, 0),
            (RowKind::Add, 0, 2),
            (RowKind::Context, 3, 3),
        ],
        "1-based gutter numbers, the way the painters read them"
    );
}
