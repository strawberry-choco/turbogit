use std::path::{Path, PathBuf};
use test_support::git_seed::{commit as git_commit, git, pin_eol_lf};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{Patch, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::patch::parse_patch;
use turbogit_services::patch_apply::{self, ApplyOutcome, Preflight, Rollback, Worktree};

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
    git_commit(&dir, "first.txt", "one\ntwo\nthree\n", "first.txt");
    git_commit(&dir, "second.txt", "alpha\nbeta\n", "second.txt");
    dir
}

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

struct Refusing;

impl Rollback for Refusing {
    fn put(&mut self, path: &Path, _content: Option<&[u8]>) -> TgResult<()> {
        Err(TgError::Other(format!(
            "injected: `{}` refused the write",
            path.display()
        )))
    }
}

#[test]
fn a_failed_apply_leaves_the_working_tree_byte_identical_to_what_it_held() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let before_first = bytes(&dir, "first.txt").expect("first.txt");
    let before_second = bytes(&dir, "second.txt").expect("second.txt");
    assert_eq!(before_first, b"one\ntwo\nthree\n");

    let outcome = patch_apply::apply_patch(&engine(), &dir, &first_ok_second_conflicts());

    let ApplyOutcome::Aborted { restored, .. } = &outcome else {
        panic!("the second section cannot match; expected Aborted, got {outcome:?}");
    };
    assert_eq!(outcome.title(), ApplyOutcome::ABORTED_TITLE);
    assert_eq!(outcome.message(), ApplyOutcome::ABORTED_MESSAGE);

    assert_eq!(bytes(&dir, "first.txt"), Some(before_first.clone()));
    assert_eq!(bytes(&dir, "second.txt"), Some(before_second.clone()));
    assert_eq!(
        restored.len(),
        2,
        "both named paths are accounted for: {restored:?}"
    );
    assert!(outcome.tree_is_restored());
}

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

#[test]
fn a_patch_that_applies_changes_every_section_and_stages_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let outcome = patch_apply::apply_patch(&engine(), &dir, &clean());

    let ApplyOutcome::Applied { paths } = &outcome else {
        panic!("expected Applied, got {outcome:?}");
    };
    assert_eq!(paths.len(), 2);
    assert_eq!(
        bytes(&dir, "first.txt"),
        Some(b"one\nTWO\nthree\n".to_vec())
    );
    assert_eq!(bytes(&dir, "second.txt"), Some(b"alpha\nBETA\n".to_vec()));
    let staged = git(&dir, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.trim().is_empty(),
        "`Apply patch` staged nothing; the index moved: {staged}"
    );
}

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
    assert!(
        report.contains("second.txt"),
        "the report should name the file that conflicts: {report}"
    );
    assert_eq!(
        paths.len(),
        2,
        "the patch's own paths travel with the verdict"
    );
    assert_eq!(bytes(&dir, "first.txt"), Some(before_first));
    assert_eq!(bytes(&dir, "second.txt"), Some(before_second));
}

#[test]
fn a_clean_preflight_says_clean_and_still_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let preflight = patch_apply::preflight(&engine(), &dir, &clean());

    assert!(
        matches!(preflight, Preflight::Clean { files: 2 }),
        "expected a clean answer for two sections, got {preflight:?}"
    );
    assert_eq!(
        bytes(&dir, "first.txt"),
        Some(b"one\ntwo\nthree\n".to_vec())
    );
    assert_eq!(bytes(&dir, "second.txt"), Some(b"alpha\nbeta\n".to_vec()));
}

#[test]
fn a_rollback_that_cannot_write_is_named_separately_from_one_that_did() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let before_first = bytes(&dir, "first.txt").expect("first.txt");

    let mut refusing = Refusing;
    let outcome =
        patch_apply::apply_patch_with(&engine(), &dir, &first_ok_second_conflicts(), &mut refusing);

    let ApplyOutcome::RollbackFailed {
        unrestored, reason, ..
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
    assert!(
        reason.contains("second.txt"),
        "git's reason travels: {reason}"
    );
    let first = unrestored
        .iter()
        .find(|u| u.path == Path::new("first.txt"))
        .expect("first.txt is named");
    assert_eq!(
        first.recorded.as_deref(),
        Some(before_first.as_slice()),
        "the recorded bytes are the ones the file held before the apply"
    );
}

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
    assert_eq!(
        bytes(&dir, "first.txt"),
        Some(b"one\nTWO\nthree\n".to_vec())
    );

    let mut real = Worktree;
    let still = patch_apply::retry_restore(&dir, unrestored, &mut real);

    assert!(
        still.is_empty(),
        "the retry should have written every path: {still:?}"
    );
    assert_eq!(
        bytes(&dir, "first.txt"),
        Some(b"one\ntwo\nthree\n".to_vec())
    );
    assert_eq!(bytes(&dir, "second.txt"), Some(b"alpha\nbeta\n".to_vec()));
}

#[test]
fn a_patch_naming_a_path_outside_the_root_is_refused_before_anything_is_written() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());

    let outcome = patch_apply::apply_patch(&engine(), &dir, &escapes_the_root());

    let ApplyOutcome::Aborted { reason, restored } = &outcome else {
        panic!("expected a refusal, got {outcome:?}");
    };
    assert!(
        restored.is_empty(),
        "nothing was ever written: {restored:?}"
    );
    assert!(
        reason.contains("outside the repository"),
        "the refusal says why: {reason}"
    );
    assert_eq!(
        bytes(&dir, "first.txt"),
        Some(b"one\ntwo\nthree\n".to_vec())
    );
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
