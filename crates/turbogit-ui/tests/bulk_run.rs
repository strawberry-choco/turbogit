//! Issue 10 — cascade run monitor: the UI seam.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories and assert on public surfaces: painted
//! labels, accessible widget labels, and `AppState` transitions. The
//! monitor (screen 03) renders live per-repo states under an overall
//! progress header with Stop remaining / Retry skipped / Resolve actions.

use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::Duration;
use test_support::harness::{assert_not_painted, assert_painted, settle};
use turbogit_app::state::AppState;
use turbogit_services::bulk_run::RowState;

/// Run `git <args>` in `repo`; `ok=false` tolerates failure (conflict
/// seeding). Returns stdout.
fn git(repo: &Path, args: &[&str], ok: bool) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git invocation");
    assert!(
        !ok || out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

fn git_ok(repo: &Path, args: &[&str]) -> String {
    git(repo, args, true)
}

/// Create an initialized temp repository with one base commit on `main`
/// plus an `origin` remote so upstream reads can be exercised.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    git_ok(&path, &["init", "-q", "-b", "main"]);
    git_ok(&path, &["config", "user.email", "test@example.com"]);
    git_ok(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git_ok(&path, &["add", "."]);
    git_ok(&path, &["commit", "-q", "-m", "init"]);
    let bare = parent.join(format!("{name}.origin"));
    let _ = std::fs::remove_dir_all(&bare);
    git_ok(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git_ok(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git_ok(&path, &["push", "-q", "origin", "main"]);
    git_ok(&path, &["branch", "--set-upstream-to=origin/main", "main"]);
    path
}

/// A project with both repos under one group so a single group click
/// selects the fleet. `ui` starts with a dirty tracked file so skip
/// classification has something to bite on.
fn two_repo_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/bulk-run-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    std::fs::write(ui.join("base.txt"), "uncommitted\n").unwrap();
    (project, alpha, ui)
}

/// Headless harness driving the full app UI (mirrors `bulk_operations`).
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

/// Step until the named repo's row is terminal (Done / Failed / Skipped).
fn wait_row_terminal(h: &mut Harness<'_, AppState>, name: &str) {
    wait_for(h, |s| {
        s.ui.bulk_run.as_ref().is_some_and(|v| {
            v.rows.iter().any(|r| {
                r.name == name && !matches!(r.state, RowState::Queued { .. } | RowState::Running)
            })
        })
    });
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

fn alpha_id(alpha: &Path) -> turbogit_domain::model::RootId {
    turbogit_domain::model::RootId(alpha.to_path_buf().into())
}

#[test]
fn the_monitor_tracks_the_fleet_and_closes_on_demand() {
    let (project, alpha, _ui) = two_repo_project("render");
    let state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Pull all").click();
    settle(&mut h);
    h.get_by_label("Run on 1 of 1").click();
    settle(&mut h);

    // The monitor replaced the preflight modal: per-repo rows under the
    // progress header with the live footer actions.
    wait_row_terminal(&mut h, "alpha");
    settle(&mut h);
    assert_painted(&h, "alpha");
    assert_painted(&h, "Done");
    assert_painted(&h, "1 done");
    assert_painted(&h, "Stop remaining");

    // Close dismisses the monitor.
    h.get_by_label("Close").click();
    settle(&mut h);
    assert!(h.state().ui.bulk_run.is_none());
    assert_not_painted(&h, "Stop remaining");
}

#[test]
fn a_failed_row_exposes_resolve_that_jumps_to_the_repo() {
    let (project, alpha, _ui) = two_repo_project("resolve");
    // Break alpha's remote so its fetch fails deterministically.
    git_ok(
        &alpha,
        &["remote", "set-url", "origin", "/nonexistent/broken"],
    );
    let state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Fetch all").click();
    settle(&mut h);
    h.get_by_label("Run on 1 of 1").click();
    wait_row_terminal(&mut h, "alpha");
    settle(&mut h);

    assert_painted(&h, "Failed");
    h.get_by_label("Resolve alpha").click();
    settle(&mut h);

    // Resolve navigates: the repo becomes the selected root and the
    // monitor closes.
    assert_eq!(h.state().selected_root, Some(alpha_id(&alpha)));
    assert!(h.state().ui.bulk_run.is_none());
    assert_not_painted(&h, "Stop remaining");
}

#[test]
fn resolve_on_a_conflicted_repo_opens_the_conflict_resolver() {
    let (project, alpha, _ui) = two_repo_project("resolver-link");
    // Seed an in-progress merge conflict on alpha's tracked file.
    git_ok(&alpha, &["checkout", "-q", "-b", "other"]);
    std::fs::write(alpha.join("base.txt"), "theirs\n").unwrap();
    git_ok(&alpha, &["commit", "-qam", "theirs"]);
    git_ok(&alpha, &["checkout", "-q", "main"]);
    std::fs::write(alpha.join("base.txt"), "ours\n").unwrap();
    git_ok(&alpha, &["commit", "-qam", "ours"]);
    git(&alpha, &["merge", "other"], false);
    // Break the remote so the fetch fails and the row exposes Resolve.
    git_ok(
        &alpha,
        &["remote", "set-url", "origin", "/nonexistent/broken"],
    );

    let state = AppState::for_roots(&project, std::slice::from_ref(&alpha));
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Fetch all").click();
    settle(&mut h);
    h.get_by_label("Run on 1 of 1").click();
    wait_row_terminal(&mut h, "alpha");
    settle(&mut h);

    h.get_by_label("Resolve alpha").click();
    settle(&mut h);

    // The deep link jumps past repo selection straight into the resolver.
    assert_eq!(h.state().selected_root, Some(alpha_id(&alpha)));
    assert!(h.state().ui.conflict_open.is_some(), "resolver opened");
    assert_painted(&h, "Merge: base.txt");
}

#[test]
fn retry_skipped_redispatches_the_skipped_repo_from_the_monitor() {
    let (project, alpha, _ui) = two_repo_project("retry");
    let state = AppState::for_roots(&project, &[alpha.clone(), _ui.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    // Stash all runs on the dirty repo and skips the clean one.
    h.get_by_label("Stash all").click();
    settle(&mut h);
    h.get_by_label("Run on 1 of 2").click();
    wait_row_terminal(&mut h, "alpha");
    wait_row_terminal(&mut h, "ui");
    settle(&mut h);

    assert_painted(&h, "Skipped (clean tree)");
    assert_painted(&h, "Retry 1 skipped");
    h.get_by_label("Retry 1 skipped").click();
    wait_for(&mut h, |s| {
        s.ui.bulk_run
            .as_ref()
            .map(|v| v.tally() == (2, 0, 0, 0, 0))
            .unwrap_or(false)
    });
    settle(&mut h);
    assert_painted(&h, "2 done · 0 running · 0 queued · 0 skipped · 0 failed");
}
