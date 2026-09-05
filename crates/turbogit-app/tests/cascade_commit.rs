//! Issue 21 — cascade commit ("Also commit on N selected repos"): the app
//! seam.
//!
//! Headless tests over real temporary repositories through the production
//! registration path ([`AppState::for_roots`]): the cascade-commit rail
//! surfaces a "Commit M hunks" footer over the multi-repo selection; the
//! dispatch fans out across the selection, skipping repos with nothing
//! staged and reporting per-repo outcomes through the cascade-run monitor +
//! completion toast + activity feed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

/// Run `git <args>` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// A freshly-initialized repo with one base commit.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("seed.txt"), "seed\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "seed"]);
    path
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

/// Step the event pump until the completion toast lands (the dispatch runs
/// on a worker thread even in the headless harness).
fn wait_for_toast(state: &mut AppState) {
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

// -- Commit-rail footer text ---------------------------------------------------

#[test]
fn commit_rail_footer_counts_staged_hunks_across_the_selection() {
    // The "Commit 4 hunks" footer must add up the staged-change count of
    // every selected root whose index is non-empty — and signal "skips repos
    // with nothing staged" when at least one selected repo is clean. The
    // text is the contract the spec calls out (issue 21).
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("p");
    let alpha = temp_repo(&project, "alpha");
    let ui = temp_repo(&project, "ui");
    let lib = temp_repo(&project, "lib");
    let state = app_with_selection(&project, &[alpha.clone(), ui.clone(), lib.clone()]);

    // alpha: 2 staged files. ui: clean (skip). lib: 1 staged file.
    stage(&alpha, "a.txt", "alpha a\n");
    stage(&alpha, "b.txt", "alpha b\n");
    stage(&lib, "c.txt", "lib c\n");

    let footer = state.commit_rail_footer("Commit shared message");
    assert!(
        footer.starts_with("Commit 3 hunks"),
        "footer should sum the staged changes of every selected repo: {footer:?}"
    );
    assert!(
        footer.contains("skips repos with nothing staged"),
        "footer should warn about skipped repos: {footer:?}"
    );
}

#[test]
fn commit_rail_footer_is_a_no_op_label_when_nothing_is_selected() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("p");
    let alpha = temp_repo(&project, "alpha");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha));
    state.ui.repo_selection.clear();

    let footer = state.commit_rail_footer("msg");
    // The button is hidden / disabled in this case; the footer text just
    // surfaces "no selection" so a stray render does not mislead.
    assert!(
        footer.to_lowercase().contains("no selected")
            || footer.to_lowercase().contains("nothing to commit"),
        "footer should reflect the empty selection: {footer:?}"
    );
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
    wait_for_toast(&mut state);

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
    wait_for_toast(&mut state);

    // Stage a second edit and amend with a new message — fan out again.
    stage(&alpha, "amend2.txt", "amend body 2\n");
    state.run_commit_across("amended message", true);
    wait_for_toast(&mut state);

    let after = head(&alpha);
    assert_ne!(before, after, "amend produced a new HEAD");

    let log = git(&alpha, &["log", "-1", "--format=%s"]);
    assert_eq!(log.trim(), "amended message");
}
