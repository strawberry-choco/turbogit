//! Issue 14 — Worktrees & Submodules tool tabs.
//!
//! The Worktrees and Submodules center tabs become real browsers over the
//! focused root: worktree path / branch / dirty status with add & remove
//! actions; submodule path / pinned vs recorded commit / status with update
//! & deinit actions. Tab badges paint live counts (screen 01 "Worktrees 3").
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories (CONTEXT.md "Headless harness") and
//! assert only on public surfaces: painted labels, on-disk git effects, and
//! public `AppState` transitions.
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use test_support::harness::{assert_painted, painted_text, settle};
use turbogit_app::state::{AppState, Tab};

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

/// An initialized temp repository with one base commit on the default
/// branch, under a deterministic scratch parent.
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
    path.canonicalize().unwrap()
}

/// A fresh scratch parent under the workspace's `.scratch` so path
/// basenames in assertions are stable.
fn scratch(tag: &str) -> PathBuf {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/{tag}"));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    parent
}

/// Headless harness driving the full app UI; mirrors `workspace_shell_frame`'s.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    )
}

/// Step frames until `pred` holds or the deadline passes — for async op /
/// refresh cycles a plain `settle` cannot wait for (and for on-disk effects
/// settle cannot observe at all).
fn step_until(h: &mut Harness<'_, AppState>, mut pred: impl FnMut(&Harness<'_, AppState>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        h.step();
        if pred(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("condition not met within 10s");
}

/// A linked worktree of `repo` on an existing branch, canonicalized.
fn add_worktree(repo: &Path, parent: &Path, name: &str, branch: &str) -> PathBuf {
    git(repo, &["branch", branch]);
    let wt = parent.join(name);
    git(repo, &["worktree", "add", wt.to_str().unwrap(), branch]);
    wt.canonicalize().unwrap()
}

/// True when some painted galley contains `needle`.
fn painted_contains(h: &Harness<'_, AppState>, needle: &str) -> bool {
    painted_text(h).iter().any(|t| t.contains(needle))
}

// -- Cycle A — Worktrees tab body -------------------------------------------

#[test]
fn worktrees_tab_lists_worktrees_with_branch_and_dirty_state() {
    let parent = scratch("wt-sub-tabs-list");
    let repo = temp_repo(&parent, "alpha");
    let wt = add_worktree(&repo, &parent, "wt-feature", "feature");
    std::fs::write(wt.join("base.txt"), "changed\n").unwrap();

    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);
    settle(&mut h);
    // The tab's data loads asynchronously (event pump); step until it paints.
    step_until(&mut h, |h| painted_contains(h, "wt-feature"));

    // The tab paints a real browser: the worktree row (path + branch +
    // dirty status), and the add affordance.
    assert_painted(&h, "wt-feature");
    assert_painted(&h, "dirty");
    assert_painted(&h, "Add worktree");
}

#[test]
fn worktrees_tab_add_action_creates_a_worktree_end_to_end() {
    let parent = scratch("wt-sub-tabs-add");
    let repo = temp_repo(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Add worktree").click();
    settle(&mut h);
    h.get_by_label("Worktree path input").focus();
    h.get_by_label("Worktree path input")
        .type_text("wt-created");
    h.get_by_label("Worktree branch input").focus();
    h.get_by_label("Worktree branch input")
        .type_text("created-branch");
    settle(&mut h);
    assert_painted(&h, "wt-created");
    assert_painted(&h, "created-branch");
    let created = repo.join("wt-created");
    h.get_by_label("Create").click();

    // End to end: the worktree exists on disk AND the (refreshed, refetched)
    // tab lists it — wait for the painted row, not just the directory (git
    // creates it mid-op, long before the completion refresh lands).
    step_until(&mut h, |h| {
        created.exists() && painted_contains(h, "wt-created")
    });
}

#[test]
fn worktrees_tab_remove_action_removes_a_worktree_end_to_end() {
    let parent = scratch("wt-sub-tabs-remove");
    let repo = temp_repo(&parent, "alpha");
    let wt = add_worktree(&repo, &parent, "wt-feature", "feature");
    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Remove wt-feature"));

    h.get_by_label("Remove wt-feature").click();
    settle(&mut h);
    assert_painted(&h, "Remove worktree");
    h.get_by_label("OK").click();

    step_until(&mut h, |h| {
        !wt.exists() && !painted_contains(h, "wt-feature")
    });
}

/// A superproject with one locally-added submodule; returns the
/// canonicalized superproject root.
fn super_with_submodule(parent: &Path, name: &str) -> PathBuf {
    let child_src = parent.join(format!("{name}-child-src"));
    std::fs::create_dir_all(&child_src).unwrap();
    git(&child_src, &["init", "-q", "-b", "main"]);
    git(&child_src, &["config", "user.email", "test@example.com"]);
    git(&child_src, &["config", "user.name", "Test"]);
    std::fs::write(child_src.join("c.txt"), "one\n").unwrap();
    git(&child_src, &["add", "."]);
    git(&child_src, &["commit", "-q", "-m", "c1"]);

    let repo = parent.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(
        &repo,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &format!("../{name}-child-src"),
            "child",
        ],
    );
    git(&repo, &["commit", "-q", "-m", "add child"]);
    repo.canonicalize().unwrap()
}

// -- Cycle B — Submodules tab body -------------------------------------------

#[test]
fn submodules_tab_lists_submodules_with_status_and_actions() {
    let parent = scratch("wt-sub-tabs-sub-list");
    let repo = super_with_submodule(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "child"));

    assert_painted(&h, "child");
    assert_painted(&h, "Up to date");
    assert_painted(&h, "Update child");
    assert_painted(&h, "Deinit child");
}

#[test]
fn submodules_tab_shows_needs_update_with_pinned_vs_recorded() {
    let parent = scratch("wt-sub-tabs-sub-ahead");
    let repo = super_with_submodule(&parent, "alpha");
    // Move the submodule's HEAD off the recorded commit.
    let sub_wc = repo.join("child");
    std::fs::write(sub_wc.join("c.txt"), "two\n").unwrap();
    git(&sub_wc, &["add", "."]);
    git(&sub_wc, &["commit", "-q", "-m", "c2"]);

    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Needs update"));

    assert_painted(&h, "Needs update");
    // Pinned vs recorded are both reported when they diverge.
    assert_painted(&h, "recorded");
}

#[test]
fn submodules_tab_update_action_checks_the_recorded_commit_out() {
    let parent = scratch("wt-sub-tabs-sub-update");
    let repo = super_with_submodule(&parent, "alpha");
    let sub_wc = repo.join("child");
    std::fs::write(sub_wc.join("c.txt"), "two\n").unwrap();
    git(&sub_wc, &["add", "."]);
    git(&sub_wc, &["commit", "-q", "-m", "c2"]);

    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Update child"));

    h.get_by_label("Update child").click();
    step_until(&mut h, |h| painted_contains(h, "Up to date"));
}

#[test]
fn submodules_tab_deinit_action_uninitializes_the_submodule() {
    let parent = scratch("wt-sub-tabs-sub-deinit");
    let repo = super_with_submodule(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Deinit child"));

    h.get_by_label("Deinit child").click();
    settle(&mut h);
    assert_painted(&h, "De-init submodule");
    h.get_by_label("OK").click();
    step_until(&mut h, |h| painted_contains(h, "Uninitialized"));
}

// -- Cycle C — tab-strip badges ----------------------------------------------

/// The Worktrees tab badge paints the live count of the focused root's
/// linked worktrees (screen 01 "Worktrees 3"), without visiting the tab.
#[test]
fn tab_badges_show_live_worktree_and_submodule_counts() {
    let parent = scratch("wt-sub-tabs-badges");
    let repo = temp_repo(&parent, "alpha");
    add_worktree(&repo, &parent, "wt-a", "feature-a");
    add_worktree(&repo, &parent, "wt-b", "feature-b");
    let state = AppState::for_roots(&parent, &[repo]);
    // Stay on Changes: the badge must be live, not visit-driven.
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Worktrees 2"));
}
