//! The engine's three patch verbs, and which side of the index each one writes.
//!
//! The distinction is the whole of what `Apply patch` promises the developer: the
//! patch lands in the **working tree** as an ordinary local change, and the index
//! keeps holding what was committed. `git apply --cached` and `git apply` are one
//! flag apart and opposite in effect, so the flag is pinned here against a real
//! repository rather than trusted from a shared helper.
//!
//! The three verbs now share one stdin writer in the adapter, which is also what
//! makes this a regression suite: a refactor that put `--cached` back on the
//! worktree call, or dropped `--check` from the preflight, fails here on git's own
//! behaviour rather than on this file's prose.

use std::path::{Path, PathBuf};
use test_support::git_seed::{commit as git_commit, git, pin_eol_lf};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_engine_api::patch::parse_patch;

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn repo(parent: &Path) -> PathBuf {
    let dir = parent.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    pin_eol_lf(&dir);
    git_commit(&dir, "note.txt", "one\ntwo\nthree\n", "note.txt");
    git_commit(&dir, "other.txt", "alpha\nbeta\n", "other.txt");
    dir
}

fn raises_note() -> turbogit_domain::model::Patch {
    parse_patch(
        r#"diff --git a/note.txt b/note.txt
--- a/note.txt
+++ b/note.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
"#,
    )
}

/// The verb `Apply patch` runs: the file on disk changes, `git status` calls it a
/// local modification, and nothing is staged.
#[test]
fn a_worktree_apply_writes_the_file_and_leaves_the_index_to_itself() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    engine()
        .apply_patch_to_worktree(&dir, &raises_note())
        .expect("the section's context matches the file");

    assert_eq!(
        std::fs::read_to_string(dir.join("note.txt")).unwrap(),
        "one\nTWO\nthree\n",
        "the working tree holds what the patch says"
    );
    let staged = git(&dir, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.trim().is_empty(),
        "a worktree apply stages nothing; the index moved: {staged}"
    );
    let unstaged = git(&dir, &["diff", "--name-only"]);
    assert_eq!(unstaged.trim(), "note.txt", "git sees it as a local change");
}

/// The sibling verb, pinned in the same file because the two are one flag apart:
/// `--cached` moves the index and must leave the file on disk exactly as it was.
#[test]
fn an_index_apply_stages_the_change_without_touching_the_working_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    engine()
        .apply_patch_to_index(
            &dir,
            &raises_note(),
            turbogit_engine_api::ApplyDirection::Forward,
        )
        .expect("the section applies against the index");

    assert_eq!(
        git(&dir, &["diff", "--cached", "--name-only"]).trim(),
        "note.txt",
        "the index holds the change"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("note.txt")).unwrap(),
        "one\ntwo\nthree\n",
        "the file on disk is untouched by an index apply"
    );
}

/// `Show Diff`'s verdict comes from `git apply --check`, which is the only one of
/// the three that must not write even when the patch is fine.
#[test]
fn the_check_verdict_answers_for_both_halves_and_writes_neither() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    engine().check_patch(&dir, &raises_note()).expect("clean");
    assert_eq!(
        std::fs::read_to_string(dir.join("note.txt")).unwrap(),
        "one\ntwo\nthree\n",
        "a check that passes still wrote nothing"
    );

    let broken = parse_patch(
        r#"diff --git a/note.txt b/note.txt
--- a/note.txt
+++ b/note.txt
@@ -1,2 +1,2 @@
-absolutely-not-in-this-file
+nope
"#,
    );
    let err = engine()
        .check_patch(&dir, &broken)
        .expect_err("git must refuse a hunk whose context is not there");
    assert!(
        err.to_string().contains("note.txt"),
        "git's own stderr names the file, which is what the preflight paints: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("note.txt")).unwrap(),
        "one\ntwo\nthree\n"
    );
}

/// A refusal of one section says nothing about the others, which is the fact the
/// service's per-section apply and its rollback depend on: `git apply` given a
/// two-file patch where one file fails writes neither, and the same patch handed
/// over section by section writes the good one.
#[test]
fn one_whole_call_is_all_or_nothing_where_the_per_section_calls_are_not() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let good = raises_note();
    let bad = parse_patch(
        r#"diff --git a/other.txt b/other.txt
--- a/other.txt
+++ b/other.txt
@@ -1,4 +1,4 @@
 alpha
-beta
+BETA
 gamma
 delta
"#,
    );
    let mut both = good.clone();
    both.files.extend(bad.files.clone());

    // The single call: git rejects the whole thing and the tree is untouched.
    let err = engine()
        .apply_patch_to_worktree(&dir, &both)
        .expect_err("one failing section must refuse the whole patch");
    assert!(err.to_string().contains("other.txt"), "{err}");
    assert_eq!(
        std::fs::read_to_string(dir.join("note.txt")).unwrap(),
        "one\ntwo\nthree\n",
        "git's own atomicity: nothing was written"
    );

    // The same two sections handed over one at a time: the first lands, the second
    // is refused. THIS is the shape a total rollback has to be built for.
    engine()
        .apply_patch_to_worktree(&dir, &good)
        .expect("the first section applies on its own");
    assert_eq!(
        std::fs::read_to_string(dir.join("note.txt")).unwrap(),
        "one\nTWO\nthree\n"
    );
    engine()
        .apply_patch_to_worktree(&dir, &bad)
        .expect_err("the second still cannot");
    assert_eq!(
        std::fs::read_to_string(dir.join("other.txt")).unwrap(),
        "alpha\nbeta\n",
        "the failing section wrote nothing of its own"
    );
}
