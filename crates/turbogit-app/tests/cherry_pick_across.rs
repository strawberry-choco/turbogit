//! Issue 16 — Cherry-pick across repositories, app seam: the dialog's
//! forecast over the current multi-repo selection and the run itself,
//! dispatched through the cascade pool and reported through the run
//! monitor, with conflicts held and never auto-resolved.
//!
//! Headless tests over real temporary repositories through the production
//! registration path ([`AppState::for_roots`]), events pumped through the
//! real drain loop.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use turbogit_app::state::AppState;
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
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit_file(repo: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(repo.join(name), body).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", msg]);
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

fn init_repo(repo: &Path) {
    std::fs::create_dir_all(repo).unwrap();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.email", "t@t"]);
    git(repo, &["config", "user.name", "t"]);
}

/// A three-repo project: `alpha` carries c1..c3, `beta` is a clean target,
/// `gamma` diverged on `b.txt` so picking c2 into it conflicts. Returns
/// (guard, project, alpha, beta, gamma, [c1, c2, c3]).
#[allow(clippy::type_complexity)]
fn project(
    tag: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
    Vec<String>,
) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let alpha = project.join("alpha");
    let beta = project.join("beta");
    let gamma = project.join("gamma");
    init_repo(&alpha);
    let c1 = commit_file(&alpha, "a.txt", "one\n", "c1");
    let c2 = commit_file(&alpha, "b.txt", "two\n", "c2");
    let c3 = commit_file(&alpha, "c.txt", "three\n", "c3");
    init_repo(&beta);
    commit_file(&beta, "base.txt", "base\n", "beta base");
    init_repo(&gamma);
    commit_file(&gamma, "b.txt", "conflicting\n", "gamma diverges b.txt");
    (tmp, project, alpha, beta, gamma, vec![c1, c2, c3])
}

fn rid(path: &Path) -> turbogit_domain::model::RootId {
    turbogit_domain::model::RootId(path.to_path_buf().into())
}

/// Step the event pump until every cherry-run monitor row is terminal.
fn wait_for_run_end(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        state.drain_events();
        let Some(view) = state.ui.cherry_run.as_ref() else {
            panic!("cherry monitor closed before the run ended");
        };
        let busy = view
            .rows
            .iter()
            .any(|r| matches!(r.state, RowState::Queued { .. } | RowState::Running));
        if !busy {
            return;
        }
        assert!(Instant::now() < deadline, "run did not end within 10s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// -- Forecast + default targets -------------------------------------------------

#[test]
fn the_dialog_defaults_to_the_current_multi_repo_selection_as_targets() {
    let (_tmp, project, alpha, beta, gamma, _commits) = project("cpa-default");
    let mut state = AppState::for_roots(&project, &[alpha.clone(), beta.clone(), gamma.clone()]);
    state.ui.repo_selection = [rid(&beta), rid(&gamma)].into_iter().collect();

    state.open_cherry_across();

    let dlg = &state.ui.dlg;
    assert_eq!(dlg.cherry_source.as_ref(), Some(&rid(&alpha)));
    assert_eq!(dlg.cherry_targets, vec![rid(&beta), rid(&gamma)]);
    assert!(dlg.cherry_stop_on_conflict, "the policy defaults to on");
}

#[test]
fn the_forecast_predicts_applies_risk_and_outcome_per_target() {
    let (_tmp, project, alpha, beta, gamma, commits) = project("cpa-forecast");
    let mut state = AppState::for_roots(&project, &[alpha.clone(), beta.clone(), gamma.clone()]);
    state.ui.repo_selection = [rid(&beta), rid(&gamma)].into_iter().collect();
    state.open_cherry_across();
    state.cherry_toggle_commit(commits[1].clone());
    state.cherry_toggle_commit(commits[2].clone());

    let forecast = state
        .ui
        .dlg
        .cherry_forecast
        .as_ref()
        .expect("forecast computed");
    assert_eq!(forecast.len(), 2, "one row per target");
    let beta_row = forecast.iter().find(|t| t.root == rid(&beta)).unwrap();
    assert_eq!((beta_row.applies, beta_row.total), (2, 2));
    assert_eq!(
        beta_row.risk,
        turbogit_services::cherry_across::TargetRisk::Low
    );
    let gamma_row = forecast.iter().find(|t| t.root == rid(&gamma)).unwrap();
    assert_eq!(
        gamma_row.risk,
        turbogit_services::cherry_across::TargetRisk::High,
        "gamma diverged on b.txt — the conflict is predicted before running"
    );
}

// -- The run --------------------------------------------------------------------

#[test]
fn running_applies_the_commits_in_order_to_every_target_and_reports_through_the_monitor() {
    let (_tmp, project, alpha, beta, _gamma, commits) = project("cpa-run");
    let mut state = AppState::for_roots(&project, &[alpha.clone(), beta.clone()]);
    state.ui.repo_selection = [rid(&beta)].into_iter().collect();
    state.open_cherry_across();
    state.cherry_toggle_commit(commits[1].clone());
    state.cherry_toggle_commit(commits[2].clone());

    state.run_cherry_across();
    wait_for_run_end(&mut state);

    let view = state.ui.cherry_run.as_ref().expect("monitor stays open");
    assert!(view.rows.iter().all(|r| r.state == RowState::Done));
    let subjects = git(&beta, &["log", "--format=%s", "--reverse"]);
    assert!(
        subjects.contains("c2") && subjects.contains("c3"),
        "{beta:?} received both picks"
    );
    // Applied in order: c3's copy sits on c2's copy.
    let subjects: Vec<&str> = subjects.lines().collect();
    let c2_pos = subjects.iter().position(|s| *s == "c2").unwrap();
    let c3_pos = subjects.iter().position(|s| *s == "c3").unwrap();
    assert!(c2_pos < c3_pos, "c2 applied before c3");
    assert!(
        state.ui.toast.is_some(),
        "the completed run surfaces feedback"
    );
}

#[test]
fn a_conflicted_target_is_marked_failed_and_left_held_for_manual_resolution() {
    let (_tmp, project, alpha, beta, gamma, commits) = project("cpa-conflict");
    let mut state = AppState::for_roots(&project, &[alpha.clone(), beta.clone(), gamma.clone()]);
    state.ui.repo_selection = [rid(&beta), rid(&gamma)].into_iter().collect();
    state.open_cherry_across();
    state.cherry_toggle_commit(commits[1].clone());
    state.cherry_toggle_commit(commits[2].clone());

    state.run_cherry_across();
    wait_for_run_end(&mut state);

    let view = state.ui.cherry_run.as_ref().expect("monitor stays open");
    let gamma_row = view.rows.iter().find(|r| r.root == rid(&gamma)).unwrap();
    assert!(
        matches!(gamma_row.state, RowState::Failed { .. }),
        "the conflicted repo is marked failed: {:?}",
        gamma_row.state
    );
    let beta_row = view.rows.iter().find(|r| r.root == rid(&beta)).unwrap();
    assert_eq!(
        beta_row.state,
        RowState::Done,
        "other targets are unaffected"
    );

    // Held, never auto-resolved: the conflict is still open in gamma.
    assert!(
        turbogit_services::integrate_service::in_progress(&gamma),
        "the conflicted pick must stay held"
    );
    let conflicted = git(&gamma, &["diff", "--name-only", "--diff-filter=U"]);
    assert!(
        conflicted.contains("b.txt"),
        "the conflicted file is visible for the Resolve deep link: {conflicted}"
    );
}
