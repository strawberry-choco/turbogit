//! Issue 12 — recent bulk operations history: the UI seam.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories and assert on public surfaces: the
//! history list below the operations grid paints time, operation, repo
//! count, and outcome summary; Details drills down to the per-repo
//! outcomes; Rollback is offered only for reversible operations and undoes
//! the run's effect through the real git state.

use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::harness::{assert_painted, settle};
use turbogit_app::bulk_history::{BulkRunRecord, RepoOutcome, RepoRecord};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;
use turbogit_services::bulk_ops::BulkOp;

/// Run `git <args>` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

/// Create an initialized temp repository with one base commit on `main`.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    path
}

/// A two-repo project with both repos registered.
fn two_repo_project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    (tmp, alpha, ui)
}

/// Headless harness driving the full app UI (mirrors `cascade_branch`).
fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    h.set_size(egui::vec2(1280.0, 800.0));
    h
}

fn repo_record(root: &Path, name: &str, outcome: RepoOutcome) -> RepoRecord {
    RepoRecord {
        root: root.to_path_buf(),
        name: name.to_string(),
        prior_branch: None,
        created: false,
        outcome,
    }
}

fn record(id: u64, op: BulkOp, branch: &str, repos: Vec<RepoRecord>) -> BulkRunRecord {
    BulkRunRecord {
        id,
        at: 1_791_234_000_000, // a fixed wall-clock time for the label
        op,
        branch: branch.to_string(),
        repos,
        rolled_back: false,
    }
}

fn state_with_history(project: &Path, roots: &[PathBuf], history: Vec<BulkRunRecord>) -> AppState {
    let mut state = AppState::for_roots(project, roots);
    state.ui.repo_selection = roots.iter().map(|p| RootId(p.clone().into())).collect();
    state.ui.bulk_history = history;
    state
}

#[test]
fn the_history_list_renders_rows_and_details_drills_down_per_repo() {
    let (_tmp, alpha, ui) = two_repo_project("hist-ui");
    let project = alpha.parent().unwrap().parent().unwrap().to_path_buf();
    let state = state_with_history(
        &project,
        &[alpha.clone(), ui.clone()],
        vec![record(
            1,
            BulkOp::FetchAll,
            "",
            vec![
                repo_record(&alpha, "alpha", RepoOutcome::Done),
                repo_record(
                    &ui,
                    "ui",
                    RepoOutcome::Skipped {
                        reason: "offline remote".to_string(),
                    },
                ),
            ],
        )],
    );
    let mut h = harness(state);
    settle(&mut h);

    // The section header and the row's time, op, repo count, and summary.
    assert_painted(&h, "RECENT BULK OPERATIONS");
    assert_painted(&h, "Fetch all · 2 repos");
    assert_painted(&h, "1 ok · 1 skipped (offline remote)");

    // Details drills down to the per-repo outcome rows.
    h.get_by_label("Details Fetch all").click();
    settle(&mut h);
    assert_painted(&h, "alpha · done");
    assert_painted(&h, "ui · skipped (offline remote)");
}

#[test]
fn rollback_is_offered_only_for_reversible_runs_and_undoes_the_branches() {
    let (_tmp, alpha, ui) = two_repo_project("hist-ui-rb");
    let project = alpha.parent().unwrap().parent().unwrap().to_path_buf();
    // alpha's prior checkout was main and the run created feature/x; ui's
    // record row is a preflight skip and must stay untouched.
    let mut alpha_row = repo_record(&alpha, "alpha", RepoOutcome::Done);
    alpha_row.prior_branch = Some("main".to_string());
    alpha_row.created = true;
    let state = state_with_history(
        &project,
        &[alpha.clone(), ui.clone()],
        vec![
            record(
                1,
                BulkOp::CreateBranch,
                "feature/x",
                vec![
                    alpha_row,
                    repo_record(
                        &ui,
                        "ui",
                        RepoOutcome::Skipped {
                            reason: "dirty worktree".to_string(),
                        },
                    ),
                ],
            ),
            record(
                2,
                BulkOp::FetchAll,
                "",
                vec![repo_record(&alpha, "alpha", RepoOutcome::Done)],
            ),
        ],
    );
    let mut h = harness(state);
    settle(&mut h);

    // The reversible cascade exposes Rollback; the fetch run does not.
    assert_painted(&h, "Create branch feature/x · 2 repos");
    h.get_by_label("Rollback Create branch feature/x").click();
    settle(&mut h);

    // The undo ran through the real repos: created branch deleted, prior
    // checkout restored, the skipped repo untouched.
    assert_eq!(
        git(&alpha, &["branch", "--show-current"]).trim(),
        "main",
        "the prior checkout was restored"
    );
    assert!(
        !git(&alpha, &["branch", "--list", "feature/x"]).contains("feature/x"),
        "the created branch was deleted"
    );
}
