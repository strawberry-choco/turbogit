use egui::Key;
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::Duration;
use test_support::git_seed::git;
use test_support::harness::{
    assert_matrix_geometry, assert_painted, settle, shell_harness_over_unstyled,
};
use turbogit_app::state::AppState;
use turbogit_services::bulk_ops::BulkOp;
use turbogit_services::bulk_run::RowState;

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
    let bare = parent.join(format!("{name}.origin"));
    let _ = std::fs::remove_dir_all(&bare);
    git(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&path, &["push", "-q", "origin", "main"]);
    git(&path, &["branch", "--set-upstream-to=origin/main", "main"]);
    path
}

fn two_repo_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = test_support::scratch_root(&format!("cascade-branch-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    std::fs::write(ui.join("base.txt"), "uncommitted\n").unwrap();
    (project, alpha, ui)
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024)
}

fn wait_for(harness: &mut Harness<'_, AppState>, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..1000 {
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
        if pred(harness.state()) {
            return;
        }
    }
    panic!("condition was not met within 1000 frames");
}

#[test]
fn the_cascade_modal_predicts_outcomes_and_runs_only_on_confirmation() {
    let (project, alpha, ui) = two_repo_project("modal");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Create & checkout branch").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::CreateBranch));

    assert_painted(&h, "Pattern: type / owner / value");
    assert_painted(&h, "Existing matches are checked out");
    assert_painted(&h, "Apply broadly");

    assert_painted(&h, "Branch name");

    h.get_by_label("Run on 1 of 2").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_none(), "no run without a name");
    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::CreateBranch));

    h.get_by_label("Branch name input").focus();
    h.get_by_label("Branch name input").type_text("feature/x");
    settle(&mut h);
    assert_painted(&h, "TARGET");
    assert_painted(&h, "Create from HEAD, checkout");
    assert_painted(&h, "Skipped (dirty worktree)");

    assert_matrix_geometry(
        &h,
        "TARGET",
        &[
            ("REPO", 0.0),
            ("CURRENT BRANCH", 150.0),
            ("TARGET", 290.0),
            ("STATUS", 410.0),
            ("OUTCOME", 520.0),
        ],
        &["alpha", "ui"],
        22.0,
        30.0,
    );

    h.get_by_label("Run on 1 of 2").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_op, None);
    assert!(h.state().ui.bulk_run.is_some(), "the monitor opened");

    wait_for(&mut h, |s| {
        s.ui.bulk_run.as_ref().is_some_and(|v| {
            v.rows.len() == 2
                && v.rows
                    .iter()
                    .all(|r| matches!(r.state, RowState::Done | RowState::Skipped { .. }))
        })
    });
    let view = h.state().ui.bulk_run.as_ref().unwrap();
    let by_name = |name: &str| view.rows.iter().find(|r| r.name == name).unwrap();
    assert_eq!(by_name("alpha").state, RowState::Done);
    assert!(matches!(by_name("ui").state, RowState::Skipped { .. }));

    let on_branch = git(&alpha, &["branch", "--show-current"]);
    assert_eq!(on_branch.trim(), "feature/x");
    let ui_branch = git(&ui, &["branch", "--show-current"]);
    assert_eq!(ui_branch.trim(), "main");
}

#[test]
fn esc_cancels_the_cascade_before_anything_runs() {
    let (project, alpha, ui) = two_repo_project("esc");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Create & checkout branch").click();
    settle(&mut h);
    h.get_by_label("Branch name input").focus();
    h.get_by_label("Branch name input").type_text("feature/x");
    settle(&mut h);

    h.key_press(Key::Escape);
    settle(&mut h);

    assert_eq!(h.state().ui.bulk_op, None);
    assert!(h.state().ui.bulk_run.is_none(), "no run was started");
    assert_eq!(git(&alpha, &["branch", "--show-current"]).trim(), "main");
    assert_eq!(git(&ui, &["branch", "--show-current"]).trim(), "main");
}
