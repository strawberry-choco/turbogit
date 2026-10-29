use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use test_support::git_seed::{git, repo_with_one_commit};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(parent).unwrap();
    repo_with_one_commit(parent, name)
}

fn stage(repo: &Path, file: &str, content: &str) {
    std::fs::write(repo.join(file), content).unwrap();
    git(repo, &["add", "--", file]);
}

fn app_with_selection(project: &Path, roots: &[PathBuf]) -> AppState {
    let mut state = AppState::for_roots(project, roots);
    state.ui.repo_selection = roots.iter().map(|p| RootId((*p).clone().into())).collect();
    state
}

fn settle_bulk_run(state: &mut AppState) {
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

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

#[test]
fn cascade_commit_fans_out_across_the_selection_and_reports_per_repo_outcomes() {
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

    let toast = state.ui.toast.as_ref().expect("toast");
    assert!(
        toast.message.contains("2 of 2") || toast.message.contains("2 of 3"),
        "toast reports aggregate outcome: {toast:?}"
    );

    let last = state.ui.activity.entries.last().expect("activity");
    assert!(
        last.message.contains("commit") || last.message.contains("Commit"),
        "activity names the operation: {last:?}"
    );
}

#[test]
fn cascade_commit_amend_amends_only_repos_that_have_a_committable_index() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("p");
    let alpha = temp_repo(&project, "alpha");
    let ui = temp_repo(&project, "ui");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()]);

    stage(&alpha, "amend.txt", "amend body\n");
    let before = head(&alpha);
    state.run_commit_across("first message", false);
    settle_bulk_run(&mut state);

    stage(&alpha, "amend2.txt", "amend body 2\n");
    state.run_commit_across("amended message", true);
    settle_bulk_run(&mut state);

    let after = head(&alpha);
    assert_ne!(before, after, "amend produced a new HEAD");

    let log = git(&alpha, &["log", "-1", "--format=%s"]);
    assert_eq!(log.trim(), "amended message");
}

#[test]
fn commit_targets_are_the_staged_repos_in_registration_order() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("p");
    let alpha = temp_repo(&project, "alpha");
    let ui = temp_repo(&project, "ui");
    let lib = temp_repo(&project, "lib");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone(), lib.clone()]);

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

    let roots = state.commit_targets();
    assert_eq!(roots.len(), 2, "two staged targets");
    state.run_commit_across_roots(roots, "roots message", false);
    settle_bulk_run(&mut state);

    assert_ne!(head(&alpha), alpha_before, "alpha advanced");
    assert_ne!(head(&lib), lib_before, "lib advanced");
    assert_eq!(head(&ui), ui_before, "ui was not passed, untouched");
}

#[test]
fn prefill_amend_message_answers_from_a_fresh_read_when_no_log_cache_was_warmed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("p");
    let alpha = temp_repo(&project, "alpha");
    let ui = temp_repo(&project, "ui");

    git(
        &ui,
        &["commit", "-q", "--allow-empty", "-m", "diverged head"],
    );
    stage(&alpha, "feature.txt", "alpha\n");
    stage(&ui, "feature.txt", "ui\n");

    let mut state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    assert_eq!(
        state.commit_targets().len(),
        2,
        "both staged repos are checked and vote"
    );
    assert!(
        state.caches.log(&RootId(alpha.clone().into())).is_none(),
        "nothing fetched a log for alpha, so only a fresh read can answer"
    );

    state.prefill_amend_message();
    assert_eq!(
        state.ui.commit_message, "",
        "differing heads withhold the seed"
    );

    git(&ui, &["reset", "-q", "--soft", "HEAD~1"]);
    state.prefill_amend_message();
    assert_eq!(state.ui.commit_message, "init", "equal heads seed");
}
