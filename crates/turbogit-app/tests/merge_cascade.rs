//! Issue 28 — the merge dialog's cascade hand-off, the app seam.
//!
//! Headless tests over real temporary repositories through the production
//! registration path ([`AppState::for_roots`]): "View plan →" scopes the
//! cascade preflight to the sibling repos that share the branch being merged
//! into, and a confirmed cascade-merge plan runs those repos through the
//! cascade-run monitor (issue 10's pipeline) with the dialog's own options.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use turbogit_app::state::AppState;
use turbogit_domain::model::{MergeStrategy, RootId};
use turbogit_services::bulk_ops::{BulkOp, BulkPlan};
use turbogit_services::bulk_run::RowState;

/// Run `git <args>` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit(repo: &Path, name: &str, text: &str) {
    let file = repo.join(name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", text]);
}

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    commit(&path, "base.txt", "base");
    path
}

/// A two-repo project where both repos are on `main` and both carry a
/// `feature` branch one commit ahead (the same change, per repo).
fn sibling_project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let focused = temp_repo(&project, "focused");
    let sibling = temp_repo(&project, "sibling");
    for repo in [&focused, &sibling] {
        git(repo, &["checkout", "-q", "-b", "feature"]);
        commit(repo, "feature.txt", "feature-1");
        git(repo, &["checkout", "-q", "main"]);
    }
    (tmp, project, focused, sibling)
}

fn rid(path: &Path) -> RootId {
    RootId(path.to_path_buf().into())
}

/// Step the event pump until every monitor row is terminal or the deadline
/// passes.
fn wait_for_run_end(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        state.drain_events();
        let Some(view) = state.ui.bulk_run.as_ref() else {
            panic!("monitor closed before the run ended");
        };
        let busy_row = view
            .rows
            .iter()
            .any(|r| matches!(r.state, RowState::Queued { .. } | RowState::Running));
        if !busy_row {
            return;
        }
        assert!(Instant::now() < deadline, "run did not end within 10s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn view_plan_scopes_the_cascade_preflight_to_the_sibling_repos() {
    let (_tmp, project, focused, sibling) = sibling_project("mc-plan");
    let mut state = AppState::for_roots(&project, &[focused.clone(), sibling.clone()]);
    state.selected_root = Some(rid(&focused));
    state.ui.dlg.merge_target = "feature".to_string();

    state.open_merge_cascade_plan();

    assert_eq!(state.ui.bulk_op, Some(BulkOp::Merge));
    assert_eq!(
        state.ui.repo_selection.iter().cloned().collect::<Vec<_>>(),
        vec![rid(&sibling)],
        "the plan covers the sibling repos, not the focused one"
    );
}

#[test]
fn confirmed_merge_cascade_merges_the_source_branch_into_the_siblings() {
    let (_tmp, project, focused, sibling) = sibling_project("mc-run");
    let mut state = AppState::for_roots(&project, &[focused.clone(), sibling.clone()]);
    state.selected_root = Some(rid(&focused));
    state.ui.dlg.merge_target = "feature".to_string();
    state.ui.dlg.merge_strategy = MergeStrategy::Commit;

    state.open_merge_cascade_plan();
    let plan = BulkPlan {
        op: BulkOp::Merge,
        roots: state.ui.repo_selection.iter().cloned().collect(),
        rebase: false,
        branch: state.ui.dlg.merge_target.clone(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);
    wait_for_run_end(&mut state);

    // The sibling's main now contains the feature commit; the focused repo
    // was never touched by the cascade.
    assert_eq!(
        git(&sibling, &["branch", "--show-current"]).trim(),
        "main",
        "the sibling stayed on main"
    );
    assert_eq!(
        git(&sibling, &["rev-parse", "main"]).trim(),
        git(&sibling, &["rev-parse", "feature"]).trim(),
        "the sibling's main fast-forwarded onto feature"
    );
    assert_ne!(
        git(&focused, &["rev-parse", "main"]).trim(),
        git(&focused, &["rev-parse", "feature"]).trim(),
        "the focused repo is not part of its own cascade"
    );

    // The run landed in the recent-bulk-operations history as a merge.
    let last = state.ui.bulk_history.last().expect("history entry");
    assert_eq!(last.op, BulkOp::Merge);
}
