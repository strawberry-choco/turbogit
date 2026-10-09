//! Applying a patch to the working tree, and the guarantee the surface claims
//! while doing it.
//!
//! The suite runs against **real git in a temporary repository**, because the
//! assertion is about bytes on disk: a canned executor could only prove that this
//! file and the service agree about what a failed apply means, which is not a
//! statement about the working tree.
//!
//! The load-bearing assertion in every test here is the **content check**, not the
//! outcome name. `Apply patch aborted` is a claim the UI makes about itself; the
//! claim is `std::fs::read` of a path equal to what it held before the apply ran.
//! Every failure test therefore reads the tree back and compares bytes.
//!
//! The fixture patches are hand-written rather than produced by `git diff`, because
//! the second section of each one is the thing being tested: a hunk whose context
//! cannot match the file it names. Git's own output would have to be broken on
//! purpose anyway, and a patch written out here says which file fails and why.

use std::path::{Path, PathBuf};
use test_support::git_seed::{commit as git_commit, git};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{Patch, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine::patch::parse_patch;
use turbogit_services::patch_apply::{
    self, ApplyOutcome, Preflight, Rollback, Worktree,
};

// ------------------------------------------------------------------ fixtures --

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// One repository, two committed files with content that makes a matching hunk and
/// a non-matching one easy to state exactly.
fn repo(parent: &Path) -> PathBuf {
    let dir = parent.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    git(&dir, &["config", "core.autocrlf", "false"]);
    git_commit(&dir, "first.txt", "one\ntwo\nthree\n", "first.txt");
    git_commit(&dir, "second.txt", "alpha\nbeta\n", "second.txt");
    dir
}

/// The patch the ticket's verification scenario describes: a first section that
/// applies, and a second whose context names lines the file does not hold.
fn first_ok_second_conflicts() -> Patch {
    parse_patch(
        r#"diff --git a/first.txt b/first.txt
--- a/first.txt
+++ b/first.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
diff --git a/second.txt b/second.txt
--- a/second.txt
+++ b/second.txt
@@ -1,4 +1,4 @@
 alpha
-beta
+BETA
 gamma
 delta
"#,
    )
}

/// The same failure, one step further: the first section CREATES a file, so the
/// rollback's job on that path is a removal rather than a write.
fn create_then_conflict() -> Patch {
    parse_patch(
        r#"diff --git a/added.txt b/added.txt
new file mode 100644
--- /dev/null
+++ b/added.txt
@@ -0,0 +1,2 @@
+created by the patch
+second line
diff --git a/second.txt b/second.txt
--- a/second.txt
+++ b/second.txt
@@ -1,4 +1,4 @@
 alpha
-beta
+BETA
 gamma
 delta
"#,
    )
}

/// A patch that applies in full.
fn clean() -> Patch {
    parse_patch(
        r#"diff --git a/first.txt b/first.txt
--- a/first.txt
+++ b/first.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
diff --git a/second.txt b/second.txt
--- a/second.txt
+++ b/second.txt
@@ -1,2 +1,2 @@
 alpha
-beta
+BETA
"#,
    )
}

/// A section whose only header line names a path OUTSIDE the repository.
fn escapes_the_root() -> Patch {
    parse_patch(
        r#"diff --git a/../outside.txt b/../outside.txt
--- a/../outside.txt
+++ b/../outside.txt
@@ -1,1 +1,1 @@
-absent
+written
"#,
    )
}

fn bytes(dir: &Path, rel: &str) -> Option<Vec<u8>> {
    std::fs::read(dir.join(rel)).ok()
}

/// A [`Rollback`] that refuses every write, standing in for the device, the
/// permission and the locked file — the cases a `Rollback failed` state is named
/// for. Injecting it is what makes that state assertable rather than aspirational.
struct Refusing;

impl Rollback for Refusing {
    fn put(&mut self, path: &Path, _content: Option<&[u8]>) -> TgResult<()> {
        Err(TgError::Other(format!(
            "injected: `{}` refused the write",
            path.display()
        )))
    }
}

// ------------------------------------------------------- the content check ----

/// The ticket's scenario: a patch that fails on its SECOND file, after the first
/// has already been written. What has to be true is that the first file's content
/// is back to the bytes it held — not that anything says it is.
#[test]
fn a_failed_apply_leaves_the_working_tree_byte_identical_to_what_it_held() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let before_first = bytes(&dir, "first.txt").expect("first.txt");
    let before_second = bytes(&dir, "second.txt").expect("second.txt");
    assert_eq!(before_first, b"one\ntwo\nthree\n");

    let outcome = patch_apply::apply_patch(&engine(), &dir, &first_ok_second_conflicts());

    // The named state, which the tree then has to earn.
    let ApplyOutcome::Aborted { restored, .. } = &outcome else {
        panic!("the second section cannot match; expected Aborted, got {outcome:?}");
    };
    assert_eq!(outcome.title(), ApplyOutcome::ABORTED_TITLE);
    assert_eq!(outcome.message(), ApplyOutcome::ABORTED_MESSAGE);

    // THE assertion. Every touched path, compared to the bytes recorded before git
    // was asked to write anything.
    assert_eq!(bytes(&dir, "first.txt"), Some(before_first.clone()));
    assert_eq!(bytes(&dir, "second.txt"), Some(before_second.clone()));
    assert_eq!(restored.len(), 2, "both named paths are accounted for: {restored:?}");
    assert!(outcome.tree_is_restored());
}

/// A create is rolled back by REMOVING the file. Writing zero bytes over it would
/// leave an untracked empty file the developer never had, which is a tree that is
/// not "exactly as it was".
#[test]
fn a_path_the_patch_created_is_removed_again_rather_than_emptied() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    assert!(bytes(&dir, "added.txt").is_none(), "fixture guard");

    let outcome = patch_apply::apply_patch(&engine(), &dir, &create_then_conflict());

    assert!(
        matches!(outcome, ApplyOutcome::Aborted { .. }),
        "expected Aborted, got {outcome:?}"
    );
    assert_eq!(
        bytes(&dir, "added.txt"),
        None,
        "the created file is gone, not left behind as an empty one"
    );
    assert_eq!(bytes(&dir, "second.txt"), Some(b"alpha\nbeta\n".to_vec()));
}

/// The other half of the guarantee: when the patch applies, the sections land and
/// the outcome says how many paths changed. A rollback suite that could only ever
/// fail would prove nothing about the state it is protecting.
#[test]
fn a_patch_that_applies_changes_every_section_and_stages_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let outcome = patch_apply::apply_patch(&engine(), &dir, &clean());

    let ApplyOutcome::Applied { paths } = &outcome else {
        panic!("expected Applied, got {outcome:?}");
    };
    assert_eq!(paths.len(), 2);
    assert_eq!(bytes(&dir, "first.txt"), Some(b"one\nTWO\nthree\n".to_vec()));
    assert_eq!(bytes(&dir, "second.txt"), Some(b"alpha\nBETA\n".to_vec()));
    // The apply is a working-tree write: the index still holds what was committed,
    // which is what makes the changed files show up as local changes to commit.
    let staged = git(&dir, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.trim().is_empty(),
        "`Apply patch` staged nothing; the index moved: {staged}"
    );
}

// ------------------------------------------------------ preflight, no writes --

/// `Show Diff` is a preflight, not a confirmation. It has to answer the same
/// question the apply asks and leave the tree alone, because the developer is
/// expected to press it repeatedly and stay free to walk away afterwards.
#[test]
fn the_preflight_answers_for_a_conflicting_patch_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let before_first = bytes(&dir, "first.txt").expect("first.txt");
    let before_second = bytes(&dir, "second.txt").expect("second.txt");

    let preflight = patch_apply::preflight(&engine(), &dir, &first_ok_second_conflicts());

    let Preflight::Conflicts { report, paths } = &preflight else {
        panic!("expected Conflicts, got {preflight:?}");
    };
    // Git's own verdict, verbatim: the signal the surface highlights lines against.
    assert!(
        report.contains("second.txt"),
        "the report should name the file that conflicts: {report}"
    );
    assert_eq!(paths.len(), 2, "the patch's own paths travel with the verdict");
    assert_eq!(bytes(&dir, "first.txt"), Some(before_first));
    assert_eq!(bytes(&dir, "second.txt"), Some(before_second));
}

/// A clean preflight says clean — and still writes nothing. The second half is the
/// one a "preview that applies" would silently break.
#[test]
fn a_clean_preflight_says_clean_and_still_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let preflight = patch_apply::preflight(&engine(), &dir, &clean());

    assert!(
        matches!(preflight, Preflight::Clean { files: 2 }),
        "expected a clean answer for two sections, got {preflight:?}"
    );
    assert_eq!(bytes(&dir, "first.txt"), Some(b"one\ntwo\nthree\n".to_vec()));
    assert_eq!(bytes(&dir, "second.txt"), Some(b"alpha\nbeta\n".to_vec()));
}

// --------------------------------------------------- the named rollback state --

/// The state that makes the guarantee honest. When restoring fails, the outcome is
/// `RollbackFailed` — which is what the aborted message's promise would otherwise
/// assert falsely — and it names every path it could not put back, with the reason
/// for that one write and the content it is holding.
#[test]
fn a_rollback_that_cannot_write_is_named_separately_from_one_that_did() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let before_first = bytes(&dir, "first.txt").expect("first.txt");

    let mut refusing = Refusing;
    let outcome =
        patch_apply::apply_patch_with(&engine(), &dir, &first_ok_second_conflicts(), &mut refusing);

    let ApplyOutcome::RollbackFailed {
        unrestored,
        reason,
        ..
    } = &outcome
    else {
        panic!("expected RollbackFailed, got {outcome:?}");
    };
    assert_eq!(outcome.title(), ApplyOutcome::ROLLBACK_FAILED_TITLE);
    assert_ne!(outcome.title(), ApplyOutcome::ABORTED_TITLE);
    assert!(!outcome.tree_is_restored());
    assert!(
        !outcome.message().contains(ApplyOutcome::ABORTED_MESSAGE),
        "the rolled-back sentence cannot be said by this state: {}",
        outcome.message()
    );
    assert_eq!(
        unrestored.len(),
        2,
        "both paths refused: {}",
        unrestored
            .iter()
            .map(|u| u.stated())
            .collect::<Vec<_>>()
            .join("; ")
    );
    // The apply's own failure is reported alongside the rollback's, because the
    // developer needs to know which of the two went wrong.
    assert!(reason.contains("second.txt"), "git's reason travels: {reason}");
    // The content the rollback was trying to write is still in the record — that is
    // what the window's recovery action has to have in hand.
    let first = unrestored
        .iter()
        .find(|u| u.path == PathBuf::from("first.txt"))
        .expect("first.txt is named");
    assert_eq!(
        first.recorded.as_deref(),
        Some(before_first.as_slice()),
        "the recorded bytes are the ones the file held before the apply"
    );
}

/// A rollback that fails is not the end of the road: the state carries its own
/// recovery, which is the refused write attempted again. When the second attempt
/// gets through, the outcome becomes the aborted one — truthfully, because by then
/// the tree really is back.
#[test]
fn retrying_a_refused_restore_moves_the_state_to_aborted_once_it_gets_through() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let mut refusing = Refusing;
    let failed =
        patch_apply::apply_patch_with(&engine(), &dir, &first_ok_second_conflicts(), &mut refusing);
    let ApplyOutcome::RollbackFailed { unrestored, .. } = &failed else {
        panic!("expected RollbackFailed, got {failed:?}");
    };
    // The apply really did write `first.txt` before the rollback was refused, so the
    // tree is genuinely wrong and the retry genuinely has something to fix.
    assert_eq!(bytes(&dir, "first.txt"), Some(b"one\nTWO\nthree\n".to_vec()));

    let mut real = Worktree;
    let still = patch_apply::retry_restore(&dir, unrestored, &mut real);

    assert!(still.is_empty(), "the retry should have written every path: {still:?}");
    assert_eq!(bytes(&dir, "first.txt"), Some(b"one\ntwo\nthree\n".to_vec()));
    assert_eq!(bytes(&dir, "second.txt"), Some(b"alpha\nbeta\n".to_vec()));
}

// ------------------------------------------------------------- what it refuses --

/// An apply this module could not undo is an apply it does not start. A patch
/// naming a path outside the repository would hand the rollback — which is this
/// module's own write, not git's — a destination the repository has no claim on.
#[test]
fn a_patch_naming_a_path_outside_the_root_is_refused_before_anything_is_written() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let outcome = patch_apply::apply_patch(&engine(), &dir, &escapes_the_root());

    let ApplyOutcome::Aborted { reason, restored } = &outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert!(restored.is_empty(), "nothing was ever written: {restored:?}");
    assert!(
        reason.contains("outside the repository"),
        "the refusal says why: {reason}"
    );
    assert_eq!(bytes(&dir, "first.txt"), Some(b"one\ntwo\nthree\n".to_vec()));
}

/// A patch with no file sections is not a success that did nothing. git accepts an
/// empty patch silently; reporting `Applied { paths: 0 }` for that would let a
/// window say a patch landed when no patch was there.
#[test]
fn an_empty_patch_is_refused_rather_than_reported_as_applied() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let outcome = patch_apply::apply_patch(&engine(), &dir, &Patch::default());
    assert!(
        matches!(outcome, ApplyOutcome::Aborted { .. }),
        "expected a refusal, got {outcome:?}"
    );
    assert!(matches!(
        patch_apply::preflight(&engine(), &dir, &Patch::default()),
        Preflight::Refused { .. }
    ));
}

/// The rename case the path collection exists for: one section moves a file, so the
/// rollback has to put the old name back AND remove the new one. A reader that
/// collected only the destination would leave the source half-moved.
#[test]
fn a_failed_apply_rolls_back_both_sides_of_a_rename_section() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let patch = parse_patch(
        r#"diff --git a/first.txt b/renamed.txt
similarity index 100%
rename from first.txt
rename to renamed.txt
diff --git a/second.txt b/second.txt
--- a/second.txt
+++ b/second.txt
@@ -1,4 +1,4 @@
 alpha
-beta
+BETA
 gamma
 delta
"#,
    );
    // All three names are collected — both sides of the rename, plus the second
    // section's own path.
    assert_eq!(
        patch_apply::touched_paths(&patch),
        vec![
            PathBuf::from("first.txt"),
            PathBuf::from("renamed.txt"),
            PathBuf::from("second.txt"),
        ]
    );

    let outcome = patch_apply::apply_patch(&engine(), &dir, &patch);

    assert!(
        matches!(outcome, ApplyOutcome::Aborted { .. }),
        "expected Aborted, got {outcome:?}"
    );
    assert_eq!(
        bytes(&dir, "first.txt"),
        Some(b"one\ntwo\nthree\n".to_vec()),
        "the rename's source is back under its old name"
    );
    assert_eq!(
        bytes(&dir, "renamed.txt"),
        None,
        "the name the rename created is gone"
    );
}
