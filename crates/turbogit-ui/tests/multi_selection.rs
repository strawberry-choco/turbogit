//! Issue #08 — Multi-repo selection.
//!
//! The workspace tree grows tri-state checkboxes (repo rows toggle
//! independently; a group checkbox selects/clears its descendant repos),
//! a bottom selection bar (count / quick actions / Clear), and a central
//! summary surface (aggregate stats + a per-repo table) with a
//! "Pin as view" affordance whose views restore the selection.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories and assert only on public surfaces:
//! painted labels, accessible widget labels, and `AppState` transitions.
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::harness::{assert_not_painted, assert_painted, painted_text, settle};
use turbogit_app::state::AppState;

/// Run `git` in `repo`, asserting success, and return stdout.
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

/// Create an initialized temp repository with one base commit on `main`
/// plus an `origin` remote so upstream reads can be exercised.
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

/// A two-group project (`frontend`/alpha, `oss`/lib) under
/// `.scratch/msm-<tag>/wsb`.
fn two_repo_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/msm-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    let oss = project.join("oss");
    std::fs::create_dir_all(&frontend).unwrap();
    std::fs::create_dir_all(&oss).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let lib = temp_repo(&oss, "lib");
    (project, alpha, lib)
}

/// A four-repo project with two repos per folder — `frontend/{alpha, ui}`
/// and `oss/{cli, lib}` — so both folder checkboxes display in the
/// recursive tree. Returns the project dir and the repo paths.
fn four_repo_project(tag: &str) -> (PathBuf, Vec<PathBuf>) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/msf-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    let oss = project.join("oss");
    std::fs::create_dir_all(&frontend).unwrap();
    std::fs::create_dir_all(&oss).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    let cli = temp_repo(&oss, "cli");
    let lib = temp_repo(&oss, "lib");
    (project, vec![alpha, ui, cli, lib])
}

/// Headless harness driving the full app UI (mirrors `workspace_sidebar`).
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

/// Assert some painted galley is exactly `text` (counts paint as their own
/// galleys, so exact matching keeps "2" distinct from "2 selected / 2").
#[track_caller]
fn assert_galley(harness: &Harness<'_, AppState>, text: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t == text),
        "`{text}` was not painted as an exact galley; painted text:\n{texts:#?}"
    );
}

/// Step frames, yielding to the worker thread a quick action dispatches,
/// until `needle` is painted (the feedback tests' wait pattern). A worker
/// op needs a few frames to land; settling on stability alone returns
/// during the async gap before the completion toast appears.
fn wait_painted(harness: &mut Harness<'_, AppState>, needle: &str) {
    for _ in 0..1000 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        if painted_text(harness).iter().any(|t| t.contains(needle)) {
            return;
        }
    }
    panic!("`{needle}` was not painted within 1000 frames");
}

// -- Checkboxes and the selection bar ----------------------------------------

#[test]
fn checking_a_repo_checkbox_selects_it_and_opens_the_summary() {
    let (project, alpha, lib) = two_repo_project("repo-check");
    let state = AppState::for_roots(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo alpha").click();
    settle(&mut h);

    // The summary surface replaces the tool window while a selection is live.
    assert_painted(&h, "Multi-repo selection");
    assert_painted(&h, "REPOS");
    assert_painted(&h, "AHEAD");
    assert_painted(&h, "BEHIND");
    assert_painted(&h, "DIRTY FILES");
    // The per-repo table with the selected repo.
    assert_painted(&h, "REPO");
    assert_painted(&h, "BRANCH");
    assert_painted(&h, "SYNC");
    assert_painted(&h, "DIRTY");
    assert_painted(&h, "LAST COMMIT");
    assert_painted(&h, "alpha");
    // The bottom selection bar: count, quick actions, Clear.
    assert_galley(&h, "1 selected / 2");
    assert_painted(&h, "Fetch");
    assert_painted(&h, "Pull");
    assert_painted(&h, "Branch…");
    assert_painted(&h, "Clear");
    // The Pin as view affordance.
    assert_painted(&h, "Pin as view");
}

#[test]
fn checking_a_group_selects_every_descendant_without_collapsing() {
    let (project, repos) = four_repo_project("group-check");
    let state = AppState::for_roots(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);

    assert_galley(&h, "2 selected / 4");
    assert_painted(&h, "alpha"); // the select click must not collapse the group

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    assert_not_painted(&h, "Multi-repo selection");
    assert_painted(&h, "alpha"); // the clear click must not collapse either
}

#[test]
fn the_selection_bar_clear_button_empties_the_selection() {
    let (project, alpha, lib) = two_repo_project("clear");
    let state = AppState::for_roots(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo alpha").click();
    settle(&mut h);
    assert_painted(&h, "Multi-repo selection");

    h.get_by_label("Clear selection").click();
    settle(&mut h);
    assert_not_painted(&h, "Multi-repo selection");
    assert_not_painted(&h, "1 selected / 2");
}

// -- Quick actions ------------------------------------------------------------

#[test]
fn fetch_and_pull_quick_actions_dispatch_over_the_selection() {
    let (project, repos) = four_repo_project("quick-ops");
    let state = AppState::for_roots(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Select group oss").click();
    settle(&mut h);

    h.get_by_label("Fetch selection").click();
    wait_painted(&mut h, "Fetch · 4 repos");

    h.get_by_label("Pull selection").click();
    wait_painted(&mut h, "Pull · 4 repos");
}

#[test]
fn the_branch_quick_action_opens_the_new_branch_dialog() {
    let (project, alpha, lib) = two_repo_project("branch-op");
    let state = AppState::for_roots(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo alpha").click();
    settle(&mut h);
    h.get_by_label("New branch for selection").click();
    settle(&mut h);

    assert_painted(&h, "New Branch");
}

// -- Pin as view --------------------------------------------------------------

#[test]
fn pin_as_view_saves_the_selection_and_a_pinned_view_restores_it() {
    let (project, repos) = four_repo_project("pin");
    let state = AppState::for_roots(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select group frontend").click();
    settle(&mut h);
    h.get_by_label("Select group oss").click();
    settle(&mut h);
    h.get_by_label("Pin as view").click();
    settle(&mut h);

    // The pinned view renders as a recallable chip.
    assert_painted(&h, "View 1");

    // Clearing drops the surface…
    h.get_by_label("Clear selection").click();
    settle(&mut h);
    assert_not_painted(&h, "Multi-repo selection");

    // …and recalling the view restores the selection.
    h.get_by_label("Restore view View 1").click();
    settle(&mut h);
    assert_painted(&h, "Multi-repo selection");
    assert_galley(&h, "4 selected / 4");
}

#[test]
fn the_summary_table_shows_the_last_commit_subject_and_age() {
    let (project, alpha, lib) = two_repo_project("last-commit");
    let state = AppState::for_roots(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Select repo alpha").click();
    settle(&mut h);

    // alpha's cached log carries the fixture's base commit.
    assert_painted(&h, "init");
}
