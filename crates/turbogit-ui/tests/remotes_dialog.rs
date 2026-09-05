//! Issue 33 — Manage remotes dialog (screen 13 "Manage remotes…").
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` against
//! temporary git repositories, asserting painted labels, public `AppState`
//! transitions, and the exact engine calls handed to the executor boundary
//! (via [`RecordingExecutor`]).
//!
//! Covered behaviors:
//! - the branches popup footer opens the manager; the focused repo's remotes
//!   list with their fetch and push URLs
//! - add a remote dispatches through the engine seam and refreshes the list
//! - rename / edit-URL / remove row actions dispatch and refresh
//! - set upstream per branch dispatches through the engine seam
//! - in multi-root scope the pending change applies across the checked
//!   selection with per-repo outcomes reported

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable as _};
use test_support::{RecordedCall, RecordingExecutor};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;

// ---------------------------------------------------------------- helpers --

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Append a line to `file.txt`, stage, commit.
fn commit(dir: &Path, msg: &str) {
    let file = dir.join("file.txt");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{msg}").expect("appending work file");
    drop(f);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// A repo on `main` with local identity and one committed file; returns the
/// repo path.
fn fresh_repo(tmp: &Path, name: &str) -> PathBuf {
    let repo = tmp.join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "seed");
    repo
}

/// Add `origin` with distinct fetch and push URLs (the divergent-URL case the
/// manager must render).
fn add_divergent_origin(repo: &Path) {
    git(
        repo,
        &["remote", "add", "origin", "https://fetch.git/repo.git"],
    );
    git(
        repo,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://push.git/repo.git",
        ],
    );
}

/// AppState with a recording executor wrapped around the real CLI engine.
fn app_state_recording(project: &Path, roots: &[PathBuf]) -> (AppState, Arc<RecordingExecutor>) {
    let exec: Arc<RecordingExecutor> = Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })));
    let state = AppState::for_roots(project, roots)
        .with_executor(exec.clone())
        .with_settings(VcsSettings::default());
    (state, exec)
}

/// Headless harness driving the full app UI with event draining per frame.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    )
}

/// Step frames until painted button geometry is stable for 3 consecutive
/// frames.
fn settle(h: &mut Harness<'_, AppState>) {
    let mut stable = 0;
    let mut prev = String::new();
    for _ in 0..300 {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
        let fp = format!(
            "{:?}",
            h.query_all_by_role(egui::accesskit::Role::Button)
                .map(|n| (
                    n.accesskit_node().label().as_deref().map(str::to_owned),
                    n.rect(),
                ))
                .collect::<Vec<_>>()
        );
        if fp == prev {
            stable += 1;
            if stable >= 3 {
                return;
            }
        } else {
            stable = 0;
            prev = fp;
        }
    }
    panic!("layout did not settle within 300 frames");
}

/// Open the branches popup on the focused root and settle.
fn open_popup(h: &mut Harness<'_, AppState>) {
    h.state_mut().ui.branches_popup = true;
    settle(h);
}

/// Open the Manage Remotes dialog from the branches popup footer.
fn open_manager(h: &mut Harness<'_, AppState>) {
    open_popup(h);
    h.query_all_by_label("Manage remotes…")
        .next()
        .expect("footer button painted")
        .click();
    settle(h);
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::ManageRemotes),
        "footer click opens the manager"
    );
}

/// Type `text` into the field with the given accessible label.
fn type_into_field(h: &mut Harness<'_, AppState>, label: &str, text: &str) {
    let field = h.get_by_label(label);
    field.focus();
    field.type_text(text);
    settle(h);
}

/// Click a button by label and settle.
fn click_button(h: &mut Harness<'_, AppState>, label: &str) {
    h.query_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("no button labeled {label}"))
        .click();
    settle(h);
}

/// Step frames until `pred` holds on public state (async op completion).
fn pump_until(h: &mut Harness<'_, AppState>, what: &str, mut pred: impl FnMut(&AppState) -> bool) {
    for _ in 0..600 {
        if pred(h.state()) {
            return;
        }
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for: {what}");
}

/// Poll until `f` is true or the deadline elapses.
fn wait_until<F: Fn() -> bool>(ms: u64, f: F) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

// ------------------------------------------------------------------ tests --

#[test]
fn footer_opens_the_manager_listing_fetch_and_push_urls() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));

    open_manager(&mut h);

    // The remote row paints its name and both URLs.
    assert!(h.query_all_by_label("origin").next().is_some());
    assert!(
        h.query_all_by_label("fetch: https://fetch.git/repo.git")
            .next()
            .is_some()
    );
    assert!(
        h.query_all_by_label("push: https://push.git/repo.git")
            .next()
            .is_some()
    );
}

#[test]
fn add_remote_dispatches_and_refreshes_the_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    type_into_field(&mut h, "Remote name", "origin");
    type_into_field(&mut h, "Remote fetch URL", "https://fetch.git/repo.git");
    click_button(&mut h, "Add remote");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::AddRemote { name, url, .. }
                    if name == "origin" && url == "https://fetch.git/repo.git"
            )
        })
    });
    assert!(dispatched, "expected AddRemote, got {:?}", exec.recorded());

    // The completion refresh re-snapshots the root: the list paints it.
    pump_until(&mut h, "add refresh", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .map(|r| r.remotes.len())
            .unwrap_or(0)
            == 1
    });
    assert!(h.query_all_by_label("origin").next().is_some());
    assert!(
        h.query_all_by_label("fetch: https://fetch.git/repo.git")
            .next()
            .is_some()
    );
}

#[test]
fn rename_remote_dispatches_and_refreshes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    click_button(&mut h, "Rename");
    // The new-name field is pre-seeded with the old name (in-place editing);
    // clear it so typing replaces, not appends.
    h.state_mut().ui.dlg.remotes_rename_new.clear();
    settle(&mut h);
    type_into_field(&mut h, "New remote name", "upstream");
    click_button(&mut h, "Rename");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::RenameRemote { old, new, .. }
                    if old == "origin" && new == "upstream"
            )
        })
    });
    assert!(
        dispatched,
        "expected RenameRemote, got {:?}",
        exec.recorded()
    );

    pump_until(&mut h, "rename refresh", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .map(|r| r.remotes.iter().any(|r| r.name == "upstream"))
            .unwrap_or(false)
    });
    assert!(h.query_all_by_label("upstream").next().is_some());
}

#[test]
fn edit_url_dispatches_and_refreshes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    click_button(&mut h, "Edit URL");
    // Pre-seeded with the current URL; clear so typing replaces it.
    h.state_mut().ui.dlg.remotes_edit_fetch.clear();
    settle(&mut h);
    type_into_field(&mut h, "Edit fetch URL", "https://new.git/repo.git");
    click_button(&mut h, "Save");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::SetRemoteUrl { name, fetch_url, .. }
                    if name == "origin" && fetch_url.as_deref() == Some("https://new.git/repo.git")
            )
        })
    });
    assert!(
        dispatched,
        "expected SetRemoteUrl, got {:?}",
        exec.recorded()
    );

    pump_until(&mut h, "edit-url refresh", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.remotes.first())
            .map(|r| r.fetch_url.as_deref() == Some("https://new.git/repo.git"))
            .unwrap_or(false)
    });
    assert!(
        h.query_all_by_label("fetch: https://new.git/repo.git")
            .next()
            .is_some()
    );
}

#[test]
fn remove_remote_dispatches_and_refreshes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    click_button(&mut h, "Remove");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::RemoveRemote { name, .. } if name == "origin"
            )
        })
    });
    assert!(
        dispatched,
        "expected RemoveRemote, got {:?}",
        exec.recorded()
    );

    pump_until(&mut h, "remove refresh", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .map(|r| r.remotes.is_empty())
            .unwrap_or(false)
    });
    assert!(
        h.query_all_by_label("No remotes configured.")
            .next()
            .is_some()
    );
}

#[test]
fn set_upstream_per_branch_dispatches_through_the_engine_seam() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    // Provide the tracking ref `--set-upstream-to` requires, without a
    // network round trip.
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    click_button(&mut h, "Set upstream…");
    click_button(&mut h, "main → origin/main");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::SetBranchUpstream { branch, upstream, .. }
                    if branch == "main" && upstream == "origin/main"
            )
        })
    });
    assert!(
        dispatched,
        "expected SetBranchUpstream, got {:?}",
        exec.recorded()
    );
    // The tracking config landed for real.
    assert_eq!(
        git(&repo, &["config", "--get", "branch.main.remote"]).trim(),
        "origin"
    );
}

#[test]
fn apply_add_across_selection_reports_per_repo_outcomes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = fresh_repo(tmp.path(), "alpha");
    let beta = fresh_repo(tmp.path(), "beta");
    let (state, exec) = app_state_recording(tmp.path(), &[alpha.clone(), beta.clone()]);
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(alpha.clone().into()));
    open_manager(&mut h);

    type_into_field(&mut h, "Remote name", "origin");
    type_into_field(&mut h, "Remote fetch URL", "https://fetch.git/repo.git");
    // The focused root is checked by default; tick beta into the scope.
    click_button(&mut h, "beta");
    click_button(&mut h, "Apply add");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::AddRemote { name, .. } if name == "origin"
            )
        })
    });
    assert!(dispatched, "expected AddRemote, got {:?}", exec.recorded());

    // Both roots take the change; the per-repo outcomes render inline.
    pump_until(&mut h, "apply outcomes", |s| {
        s.multi
            .roots
            .iter()
            .all(|r| r.remotes.iter().any(|r| r.name == "origin"))
    });
    assert!(h.query_all_by_label("✓ alpha").next().is_some());
    assert!(h.query_all_by_label("✓ beta").next().is_some());
    assert!(
        h.state()
            .ui
            .toast
            .as_ref()
            .map(|t| t.message.contains("2 of 2 ok"))
            .unwrap_or(false),
        "toast aggregates the per-repo outcomes"
    );
}

#[test]
fn apply_add_reports_a_failing_repo_without_blocking_the_others() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = fresh_repo(tmp.path(), "alpha");
    let beta = fresh_repo(tmp.path(), "beta");
    // alpha already has origin, so its add must fail.
    git(
        &alpha,
        &["remote", "add", "origin", "https://old.git/repo.git"],
    );
    let (state, _exec) = app_state_recording(tmp.path(), &[alpha.clone(), beta.clone()]);
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(alpha.clone().into()));
    open_manager(&mut h);

    type_into_field(&mut h, "Remote name", "origin");
    type_into_field(&mut h, "Remote fetch URL", "https://new.git/repo.git");
    click_button(&mut h, "beta");
    click_button(&mut h, "Apply add");

    pump_until(&mut h, "mixed apply outcomes", |s| {
        s.ui.toast
            .as_ref()
            .map(|t| t.message.contains("failed: alpha"))
            .unwrap_or(false)
    });
    // beta still succeeded and its outcome row paints.
    assert!(
        h.state()
            .multi
            .by_id(&RootId(beta.clone().into()))
            .map(|r| r.remotes.iter().any(|r| r.name == "origin"))
            .unwrap_or(false)
    );
    assert!(h.query_all_by_label("✓ beta").next().is_some());
    assert!(
        h.query_all_by_label("✗ alpha").next().is_some(),
        "the failing repo's outcome row renders"
    );
}
