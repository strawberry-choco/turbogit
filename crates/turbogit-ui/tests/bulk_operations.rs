use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::Duration;
use test_support::git_seed::git;
use test_support::harness::{
    assert_matrix_geometry, assert_not_painted, assert_painted, painted_galleys, painted_text,
    settle, shell_harness_over_unstyled,
};
use turbogit_app::state::AppState;
use turbogit_services::bulk_ops::BulkOp;

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
    let base = test_support::scratch_root(&format!("bulk-{tag}"));
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

const PROTECTED_OUTCOME: &str = "Skipped (protected branch) — the run may not touch it";

fn protected_refusals(h: &Harness<'_, AppState>) -> Vec<String> {
    let galleys = painted_galleys(h);
    let same_line = |a: f32, b: f32| (a - b).abs() < 1.0;
    let mut headers: Vec<(f32, f32)> = Vec::new();
    for outcome in galleys.iter().filter(|g| g.text == "OUTCOME") {
        if let Some(repo) = galleys
            .iter()
            .find(|g| g.text == "REPO" && same_line(g.pos.y, outcome.pos.y))
        {
            headers.push((repo.pos.x, outcome.pos.x));
        }
    }
    assert_eq!(
        headers.len(),
        1,
        "exactly one matrix header row is painted; found {headers:?}"
    );
    let (repo_x, outcome_x) = headers.into_iter().next().expect("one matrix header row");

    let mut refusals = Vec::new();
    for row in galleys.iter().filter(|g| g.text == PROTECTED_OUTCOME) {
        assert!(
            same_line(row.pos.x, outcome_x),
            "`{PROTECTED_OUTCOME}` painted outside the matrix's OUTCOME column at {:?} \
             (column starts at {outcome_x})",
            row.pos
        );
        let repo = galleys
            .iter()
            .find(|g| same_line(g.pos.x, repo_x) && same_line(g.pos.y, row.pos.y))
            .unwrap_or_else(|| panic!("no repo cell shares the refusal's line at {:?}", row.pos));
        refusals.push(repo.text.clone());
    }
    refusals.sort();
    refusals
}

fn line_with(h: &Harness<'_, AppState>, needle: &str) -> Vec<String> {
    let galleys = painted_galleys(h);
    let line = galleys
        .iter()
        .find(|g| g.text == needle)
        .unwrap_or_else(|| panic!("`{needle}` was not painted; {:?}", painted_text(h)))
        .pos
        .y;
    let mut texts: Vec<(f32, String)> = galleys
        .iter()
        .filter(|g| (g.pos.y - line).abs() < 1.0)
        .map(|g| (g.pos.x, g.text.clone()))
        .collect();
    texts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    texts.into_iter().map(|(_, t)| t).collect()
}

#[test]
fn selecting_repos_offers_the_bulk_operations_grid() {
    let (project, alpha, ui) = two_repo_project("grid");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    assert!(
        !test_support::harness::painted_text(&h)
            .iter()
            .any(|t| t == "OPERATIONS")
    );

    h.get_by_label("Select group frontend").click();
    settle(&mut h);

    assert_painted(&h, "OPERATIONS");
    h.get_by_label("Fetch all").click();
    settle(&mut h);

    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::FetchAll));
    assert_painted(&h, "Run on 2 of 2");

    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert_eq!(h.state().ui.bulk_op, None);
    assert_not_painted(&h, "Run on 2 of 2");
}

#[test]
fn the_preflight_matrix_shows_per_repo_status_and_skip_reasons() {
    let (project, alpha, ui) = two_repo_project("matrix");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Pull all").click();
    settle(&mut h);

    assert_eq!(h.state().ui.bulk_op, Some(BulkOp::PullAll));
    assert_painted(&h, "1 of 2 will run");
    assert_painted(&h, "1 will be skipped");
    assert_painted(&h, "REPO");
    assert_painted(&h, "CURRENT BRANCH");
    assert_painted(&h, "STATUS");
    assert_painted(&h, "OUTCOME");
    assert_painted(&h, "Skipped (dirty worktree) — commit or stash first");
    assert_painted(&h, "Merge upstream");
    assert_painted(&h, "Dirty");
    assert_painted(&h, "Clean");
    assert_painted(&h, "Run on 1 of 2");
    assert!(!h.state().ui.bulk_rebase);
    assert_painted(&h, "Rebase onto upstream instead of merging");

    assert_matrix_geometry(
        &h,
        "OUTCOME",
        &[
            ("REPO", 0.0),
            ("CURRENT BRANCH", 170.0),
            ("STATUS", 300.0),
            ("OUTCOME", 390.0),
        ],
        &["alpha", "ui"],
        22.0,
        30.0,
    );
}

#[test]
fn a_rebasing_pull_refuses_every_protected_root_in_the_matrix() {
    let (project, alpha, ui) = two_repo_project("protected");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Pull all").click();
    settle(&mut h);

    assert_painted(&h, "Merge upstream");
    assert!(
        protected_refusals(&h).is_empty(),
        "a merging pull names no protected branch"
    );

    h.get_by_label("Rebase onto upstream instead of merging")
        .click();
    settle(&mut h);
    assert!(
        h.state().ui.bulk_rebase,
        "the policy control took the click"
    );

    assert_eq!(
        protected_refusals(&h),
        vec!["alpha".to_string(), "ui".to_string()],
        "one refusal row per root on the protected branch"
    );
    assert_not_painted(&h, "Skipped (dirty worktree)");
    let header: Vec<String> = line_with(&h, "0 of 2 will run")
        .into_iter()
        .filter(|t| t.ends_with("will run") || t.ends_with("will be skipped."))
        .collect();
    assert_eq!(
        header,
        vec![
            "Will run on 0 repositories. 2 will be skipped.".to_string(),
            "0 of 2 will run".to_string(),
        ],
        "the refusal count and the sentence explaining it are one line of the modal"
    );

    h.get_by_label("Run on 0 of 2").click();
    settle(&mut h);
    assert_eq!(
        h.state().ui.bulk_op,
        Some(BulkOp::PullAll),
        "the modal is still preflighting"
    );
    assert!(h.state().ui.bulk_run.is_none(), "nothing ran");
}

#[test]
fn confirming_the_preflight_runs_the_fleet_and_reports_outcomes() {
    let (project, alpha, ui) = two_repo_project("run");
    let state = AppState::for_roots(&project, &[alpha.clone(), ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Stash all").click();
    settle(&mut h);
    assert_painted(&h, "Run on 1 of 2");
    h.get_by_label("Run on 1 of 2").click();

    wait_for(&mut h, |s| s.ui.toast.is_some() && s.ui.bulk_op.is_none());
    assert_eq!(h.state().ui.bulk_op, None);
    let toast = h.state().ui.toast.as_ref().unwrap();
    assert_eq!(toast.message, "Stash all · 1 of 1 ok");

    let ui_status = git(&ui, &["status", "--porcelain"]);
    assert!(ui_status.trim().is_empty(), "ui was stashed: {ui_status:?}");
    let ui_stashes = git(&ui, &["stash", "list"]);
    assert!(
        !ui_stashes.trim().is_empty(),
        "ui's changes are in the stash"
    );
    let alpha_status = git(&alpha, &["status", "--porcelain"]);
    assert!(
        alpha_status.trim().is_empty(),
        "alpha was clean all along: {alpha_status:?}"
    );
    let alpha_stashes = git(&alpha, &["stash", "list"]);
    assert!(
        alpha_stashes.trim().is_empty(),
        "skipped root must not be stashed: {alpha_stashes:?}"
    );
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
