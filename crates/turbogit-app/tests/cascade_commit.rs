//! Issue 21 — cascade commit (one message across the checked repos): the app
//! seam.
//!
//! Headless tests over real temporary repositories through the production
//! registration path ([`AppState::for_roots`]): `commit_targets` selects the
//! repos that hold staged content — every one is a target by default, and only
//! an explicit uncheck drops it — and the roots-driven dispatch fans the one
//! message out across them, skipping repos with nothing staged and reporting
//! per-repo outcomes through the cascade-run monitor + completion toast +
//! activity feed.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use test_support::git_seed::{git, repo_with_one_commit};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

/// A freshly-initialized repo with one base commit.
///
/// A thin composition over [`repo_with_one_commit`]; only the committed path differs
/// and no test here reads it. The recipe `git init`s *into* `parent`.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(parent).unwrap();
    repo_with_one_commit(parent, name)
}

/// Stage a new edit in `repo`.
fn stage(repo: &Path, file: &str, content: &str) {
    std::fs::write(repo.join(file), content).unwrap();
    git(repo, &["add", "--", file]);
}

/// Build an AppState over `roots` and select them all (the cascade-commit
/// button is a per-selection operation).
fn app_with_selection(project: &Path, roots: &[PathBuf]) -> AppState {
    let mut state = AppState::for_roots(project, roots);
    state.ui.repo_selection = roots.iter().map(|p| RootId((*p).clone().into())).collect();
    state
}

/// Step the event pump until a bulk run reports. A cascade commit fans out
/// over the same worker pool as a fleet run, which ADR-0019's admission slot
/// only means something in front of — so the dispatch seam leaves it threaded
/// even under the headless harness, and its answer really does arrive later.
///
/// Not a `test_support::harness` settle: those settle a *painted frame* by fingerprinting
/// `painted_text`, and this crate does not depend on `egui_kittest`. This pumps the
/// event pump and exits on a *state* predicate — a completion toast.
fn settle_bulk_run(state: &mut AppState) {
    // A completion toast survives into later runs; only a toast produced by
    // this dispatch is a valid completion signal.
    state.ui.toast = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        state.drain_events();
        if state.ui.toast.is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no completion toast within 10s");
}

/// HEAD SHA for the repo.
fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

#[test]
fn cascade_commit_fans_out_across_the_selection_and_reports_per_repo_outcomes() {
    // Three roots selected: alpha has staged content and commits; ui is
    // clean (skipped); lib has staged content and commits. After dispatch
    // alpha and lib advance HEAD, ui does not.
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("p");
    let alpha = temp_repo(&project, "alpha");
    let ui = temp_repo(&project, "ui");
    let lib = temp_repo(&project, "lib");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone(), lib.clone()]);

    stage(&alpha, "feature.txt", "alpha\n");
    stage(&lib, "feature.txt", "lib\n");

    let alpha_before = head(&alpha);
    let lib_before = head(&lib);
    let ui_before = head(&ui);

    state.run_commit_across("shared message", false);
    settle_bulk_run(&mut state);

    assert_ne!(head(&alpha), alpha_before, "alpha advanced");
    assert_ne!(head(&lib), lib_before, "lib advanced");
    assert_eq!(head(&ui), ui_before, "ui was skipped, untouched");

    // The toast names the run and counts ok vs planned.
    let toast = state.ui.toast.as_ref().expect("toast");
    assert!(
        toast.message.contains("2 of 2") || toast.message.contains("2 of 3"),
        "toast reports aggregate outcome: {toast:?}"
    );

    // Activity feed captures it as well.
    let last = state.ui.activity.entries.last().expect("activity");
    assert!(
        last.message.contains("commit") || last.message.contains("Commit"),
        "activity names the operation: {last:?}"
    );
}

#[test]
fn cascade_commit_amend_amends_only_repos_that_have_a_committable_index() {
    // Amend fans out the same way: each selected root commits its index with
    // --amend. A clean root is skipped rather than erroring.
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("p");
    let alpha = temp_repo(&project, "alpha");
    let ui = temp_repo(&project, "ui");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()]);

    stage(&alpha, "amend.txt", "amend body\n");
    // First commit normally, then amend with a new message.
    let before = head(&alpha);
    state.run_commit_across("first message", false);
    settle_bulk_run(&mut state);

    // Stage a second edit and amend with a new message — fan out again.
    stage(&alpha, "amend2.txt", "amend body 2\n");
    state.run_commit_across("amended message", true);
    settle_bulk_run(&mut state);

    let after = head(&alpha);
    assert_ne!(before, after, "amend produced a new HEAD");

    let log = git(&alpha, &["log", "-1", "--format=%s"]);
    assert_eq!(log.trim(), "amended message");
}

// -- The commit card's checklist targets ---------------------------------------

#[test]
fn commit_targets_are_the_staged_repos_in_registration_order() {
    // The commit card commits every repo that holds staged content, in
    // registration order. All staged repos are checked by default, so a clean
    // repo drops out with no gesture needed.
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("p");
    let alpha = temp_repo(&project, "alpha");
    let ui = temp_repo(&project, "ui");
    let lib = temp_repo(&project, "lib");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone(), lib.clone()]);

    // alpha and lib hold staged content; ui is clean.
    stage(&alpha, "a.txt", "alpha a\n");
    stage(&lib, "c.txt", "lib c\n");
    state.refresh(Affected::All);

    let alpha_id = RootId(alpha.clone().into());
    let lib_id = RootId(lib.clone().into());

    let targets: Vec<RootId> = state
        .commit_targets()
        .iter()
        .map(|root| root.id.clone())
        .collect();
    assert_eq!(
        targets,
        vec![alpha_id, lib_id],
        "staged & default-checked, in registration order — the clean ui is excluded"
    );
}

#[test]
fn run_commit_across_roots_commits_exactly_the_passed_roots() {
    // The roots-driven cascade commits each passed root that holds staged
    // content and leaves a root that is not passed untouched.
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("p");
    let alpha = temp_repo(&project, "alpha");
    let ui = temp_repo(&project, "ui");
    let lib = temp_repo(&project, "lib");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone(), lib.clone()]);

    stage(&alpha, "feature.txt", "alpha\n");
    stage(&lib, "feature.txt", "lib\n");
    state.refresh(Affected::All);

    let alpha_before = head(&alpha);
    let lib_before = head(&lib);
    let ui_before = head(&ui);

    // alpha and lib hold staged content, so both are targets by default (ui is
    // staged nowhere).
    let roots = state.commit_targets();
    assert_eq!(roots.len(), 2, "two staged targets");
    state.run_commit_across_roots(roots, "roots message", false);
    settle_bulk_run(&mut state);

    assert_ne!(head(&alpha), alpha_before, "alpha advanced");
    assert_ne!(head(&lib), lib_before, "lib advanced");
    assert_eq!(head(&ui), ui_before, "ui was not passed, untouched");
}
