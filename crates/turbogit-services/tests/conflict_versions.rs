//! Issue 04 (deepen-git-engine) — a **Conflict**'s versions as three answers.
//!
//! The resolver used to ask the **Git engine** for index stage revisions by
//! string (`:1`, `:2`, `:3`), and the substitutable adapter served one content
//! per path whatever it was asked for — so no test in the repo could tell ours
//! from theirs. These run through that adapter, with no `git` binary, and name
//! the scenario the old seam could not express.

use std::path::{Path, PathBuf};

use turbogit_domain::model::ConflictVersions;
use turbogit_engine::fake::FakeExecutor;
use turbogit_services::conflict;

fn root() -> PathBuf {
    PathBuf::from("/repo")
}

fn merged() -> PathBuf {
    PathBuf::from("merged.txt")
}

/// One conflicted path whose three index versions hold three different bodies.
fn fake_with_three_sides() -> (FakeExecutor, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let mut v = FakeExecutor::new();
    v.conflicts.insert(
        merged(),
        ConflictVersions {
            base: Some("ancestor\n".to_string()),
            ours: Some("our side\n".to_string()),
            theirs: Some("their side\n".to_string()),
        },
    );
    (v, tmp)
}

#[test]
fn each_side_arrives_as_its_own_answer_not_one_content_three_times() {
    let (v, _tmp) = fake_with_three_sides();
    let versions = conflict::read_versions(&v, &root(), &merged()).expect("three sides");
    assert_eq!(versions.base.as_deref(), Some("ancestor\n"));
    assert_eq!(versions.ours.as_deref(), Some("our side\n"));
    assert_eq!(versions.theirs.as_deref(), Some("their side\n"));
    assert_ne!(
        versions.ours, versions.theirs,
        "ours and theirs must be distinguishable, or the resolver can show \
         the user the same content twice"
    );
}

#[test]
fn accepting_ours_writes_our_side() {
    let (v, tmp) = fake_with_three_sides();
    let repo = tmp.path().to_path_buf();
    conflict::accept_ours(&v, &repo, &merged()).expect("resolving our side");
    assert_eq!(
        std::fs::read_to_string(repo.join(merged())).unwrap(),
        "our side\n",
        "`Ours` must write the side it names"
    );
}

#[test]
fn accepting_theirs_writes_their_side() {
    let (v, tmp) = fake_with_three_sides();
    let repo = tmp.path().to_path_buf();
    conflict::accept_theirs(&v, &repo, &merged()).expect("resolving their side");
    assert_eq!(
        std::fs::read_to_string(repo.join(merged())).unwrap(),
        "their side\n",
        "`Theirs` must write the side it names — the old seam served both from \
         one content per path, so nothing could tell these two apart"
    );
}

#[test]
fn an_absent_side_is_answered_as_absent_not_as_an_empty_one() {
    // An add/add conflict has no ancestor: `git show :1:<path>` fails, and a
    // resolver that read that as an empty base merges against a phantom.
    let mut v = FakeExecutor::new();
    v.conflicts.insert(
        merged(),
        ConflictVersions {
            base: None,
            ours: Some("ours\n".to_string()),
            theirs: Some("theirs\n".to_string()),
        },
    );
    let versions = conflict::read_versions(&v, &root(), &merged()).expect("two sides");
    assert_eq!(versions.base, None, "no ancestor is not an empty ancestor");
    assert_eq!(versions.ours.as_deref(), Some("ours\n"));
}

#[test]
fn a_path_with_no_conflict_is_an_error_not_three_empties() {
    let v = FakeExecutor::new();
    assert!(
        conflict::read_versions(&v, &root(), Path::new("clean.txt")).is_err(),
        "asking for a conflict that is not there must not answer with blanks"
    );
}
