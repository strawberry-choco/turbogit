//! Issue #02 — Rich discard confirmation: affected-file list + Shelve-first.
//!
//! Contract (issue #02):
//! - The Discard confirmation dialog lists every affected file by path.
//! - Three buttons appear: `Shelve first` (alternative to destructive
//!   discard), `Discard N-file changes` (destructive), and `Cancel`.
//! - `Shelve first` stashes the changes (via `git stash push`) and closes
//!   the confirm without performing the discard; surfaces a toast.
//! - `Discard` runs the original destructive op (no change vs. pre-issue).
//! - `Cancel` closes the confirm without any side effect.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::harness::{assert_not_painted, assert_painted, painted_text};
use turbogit_app::state::{AppState, PendingConfirm};
use turbogit_domain::model::{Change, ChangeStatus};

// --- git fixture ---------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
}

/// A repo with three modified tracked files: `a.txt`, `b.txt`, `c.txt`.
/// Each gets a tracked baseline plus an uncommitted modification.
fn repo_with_modified_files() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    let alpha = project.join("alpha");
    // Keep worktree line endings identical to the committed blobs so
    // post-discard reverts compare byte-for-byte on Windows.
    git(&alpha, &["config", "core.autocrlf", "false"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        write(&alpha.join(name), "baseline\n");
    }
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-m", "init"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        write(&alpha.join(name), "modified\n");
    }
    (tmp, project)
}

// --- harness -------------------------------------------------------------------

fn feedback_harness(project_dir: std::path::PathBuf) -> Harness<'static, AppState> {
    let state = AppState::new(project_dir);
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            state.drain_events();
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    harness
}

fn settle_quiet(harness: &mut Harness<'_, AppState>) {
    let mut stable = 0;
    let mut prev = String::new();
    for _ in 0..300 {
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
        let cur = format!("{:?}", painted_text(harness));
        if cur == prev {
            stable += 1;
            if stable >= 3 {
                return;
            }
        } else {
            stable = 0;
            prev = cur;
        }
    }
    panic!("feedback layout did not settle within 300 frames");
}

// --- helpers -------------------------------------------------------------------

/// Step frames until the confirm is cleared AND the async op's success
/// toast lands (the stash/discard worker completed) or the deadline passes.
fn wait_confirm_and_success(harness: &mut Harness<'_, AppState>, what: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        harness.step();
        // Stepping paints the toast too; poll the state behind the harness.
        if harness.state().ui.confirm.is_none()
            && harness
                .state()
                .ui
                .toast
                .as_ref()
                .is_some_and(|t| matches!(t.kind, turbogit_app::state::ToastKind::Success))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "timed out waiting for the {what} op to complete; confirm={:?} toast={:?}",
        harness.state().ui.confirm.is_none(),
        harness.state().ui.toast
    );
}

fn make_changes(paths: &[&str]) -> Vec<Change> {
    paths
        .iter()
        .map(|p| Change {
            path: p.into(),
            status: ChangeStatus::Modified,
            chunks: vec![],
            staged: false,
            unstaged: true,
            orig_path: None,
        })
        .collect()
}

fn plant_discard(harness: &mut Harness<'_, AppState>) {
    let st = harness.state_mut();
    st.ui.confirm = Some(PendingConfirm::Discard {
        changes: make_changes(&["a.txt", "b.txt", "c.txt"]),
    });
}

/// Wait until `state.ui.confirm` is cleared (Cancel/OK/Discard closed it).
fn wait_confirm_cleared(harness: &mut Harness<'_, AppState>, what: &str) {
    for _ in 0..200 {
        harness.step();
        if harness.state().ui.confirm.is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for confirm to clear after {what}; last_error={:?}",
        harness.state().last_error
    );
}

// --- tests ---------------------------------------------------------------------

/// Contract: the Discard confirm dialog lists every affected file path.
#[test]
fn discard_confirm_lists_every_affected_file() {
    let (_tmp, project) = repo_with_modified_files();
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    for path in ["a.txt", "b.txt", "c.txt"] {
        assert_painted(&harness, path);
    }
    // Sanity: a path NOT in the list must not appear.
    assert_not_painted(&harness, "d.txt");
}

/// Contract: the Discard confirm dialog shows three buttons: Shelve first,
/// Discard N-file changes, Cancel.
#[test]
fn discard_confirm_offers_shelve_first_alongside_discard_and_cancel() {
    let (_tmp, project) = repo_with_modified_files();
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    assert_painted(&harness, "Shelve first");
    assert_painted(&harness, "Discard 3-file changes");
    assert_painted(&harness, "Cancel");
}

/// Contract: clicking Cancel closes the confirm without performing any
/// side effect. The files remain modified (no `git stash`, no discard).
#[test]
fn discard_confirm_cancel_keeps_changes_intact() {
    let (_tmp, project) = repo_with_modified_files();
    let alpha = project.join("alpha");
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    harness.get_by_label("Cancel").click();
    wait_confirm_cleared(&mut harness, "Cancel click");

    // No stash entry was created.
    let out = Command::new("git")
        .args(["stash", "list"])
        .current_dir(&alpha)
        .output()
        .unwrap();
    let stash_list = String::from_utf8_lossy(&out.stdout);
    assert!(
        stash_list.trim().is_empty(),
        "Cancel must not create a stash; got: {stash_list}"
    );
    // The files remain modified.
    for name in ["a.txt", "b.txt", "c.txt"] {
        let body = std::fs::read_to_string(alpha.join(name)).unwrap();
        assert_eq!(body, "modified\n", "{name} should remain modified");
    }
}

/// Contract: clicking Discard runs the destructive op (clears the
/// modifications).
#[test]
fn discard_confirm_discard_button_runs_the_destructive_op() {
    let (_tmp, project) = repo_with_modified_files();
    let alpha = project.join("alpha");
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    harness.get_by_label("Discard 3-file changes").click();
    // The discard runs on the worker; wait for its success toast so the
    // worktree assertions below see the finished revert.
    wait_confirm_and_success(&mut harness, "Discard");
    for name in ["a.txt", "b.txt", "c.txt"] {
        let body = std::fs::read_to_string(alpha.join(name)).unwrap();
        assert_eq!(body, "baseline\n", "{name} should revert to baseline");
    }
}

/// Contract: clicking Shelve first stashes the listed changes, closes
/// the confirm — and does NOT perform the destructive discard. After
/// the stash the worktree is clean.
#[test]
fn discard_confirm_shelve_first_stashes_and_keeps_worktree_clean() {
    let (_tmp, project) = repo_with_modified_files();
    let alpha = project.join("alpha");
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    harness.get_by_label("Shelve first").click();
    // The stash runs on the worker; wait for its success toast so the
    // worktree assertions below see the finished stash.
    wait_confirm_and_success(&mut harness, "Shelve first");

    // The worktree is clean: the stash captured every modification.
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&alpha)
        .output()
        .unwrap();
    let porcelain = String::from_utf8_lossy(&out.stdout);
    assert!(
        porcelain.trim().is_empty(),
        "worktree must be clean after Shelve first; got: {porcelain}"
    );
    // A stash entry exists.
    let out = Command::new("git")
        .args(["stash", "list"])
        .current_dir(&alpha)
        .output()
        .unwrap();
    let stash_list = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stash_list.trim().is_empty(),
        "Shelve first must create a stash entry; got: {stash_list}"
    );
}

/// Contract: a Shelve-first toast surfaces after the action.
#[test]
fn discard_confirm_shelve_first_surfaces_a_toast() {
    let (_tmp, project) = repo_with_modified_files();
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    harness.get_by_label("Shelve first").click();

    // Wait for a Shelve-related toast.
    let mut saw = false;
    for _ in 0..200 {
        harness.step();
        if let Some(t) = &harness.state().ui.toast
            && t.message.to_lowercase().contains("shelv")
        {
            saw = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        saw,
        "Shelve first should surface a shelve-related toast; current toast: {:?}",
        harness.state().ui.toast
    );
}
