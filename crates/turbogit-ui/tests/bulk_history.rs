use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::git_seed::{git, repo_with_one_commit};
use test_support::harness::{assert_painted, settle, shell_harness_over_unstyled};
use turbogit_app::bulk_history::{BulkRunRecord, RepoOutcome, RepoRecord};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;
use turbogit_services::bulk_ops::BulkOp;

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    repo_with_one_commit(parent, name)
}

fn two_repo_project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    (tmp, alpha, ui)
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024)
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
        at: 1_791_234_000_000,
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

    assert_painted(&h, "RECENT BULK OPERATIONS");
    assert_painted(&h, "Fetch all · 2 repos");
    assert_painted(&h, "1 ok · 1 skipped (offline remote)");

    h.get_by_label("Details Fetch all").click();
    settle(&mut h);
    assert_painted(&h, "alpha · done");
    assert_painted(&h, "ui · skipped (offline remote)");
}

#[test]
fn rollback_is_offered_only_for_reversible_runs_and_undoes_the_branches() {
    let (_tmp, alpha, ui) = two_repo_project("hist-ui-rb");
    let project = alpha.parent().unwrap().parent().unwrap().to_path_buf();
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

    assert_painted(&h, "Create branch feature/x · 2 repos");
    h.get_by_label("Rollback Create branch feature/x").click();
    settle(&mut h);

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
