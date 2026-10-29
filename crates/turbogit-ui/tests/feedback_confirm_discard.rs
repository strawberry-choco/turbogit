use std::path::Path;
use std::process::Command;
use std::time::Duration;

use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env, pin_eol_lf};
use test_support::harness::{assert_not_painted, assert_painted, settle_quiet, shell_harness_over};
use turbogit_app::state::{AppState, PendingConfirm};
use turbogit_domain::model::{Change, ChangeStatus};

// No repository `user.*` here, so every call is pinned per call through the
// fixture crate's runner, author AND committer: a headless machine has no global
// identity to borrow, and this suite used to pin only the author, so its `init`
// commit died here with `Committer identity unknown`.

fn write(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
}

fn repo_with_modified_files() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    let alpha = project.join("alpha");
    pin_eol_lf(&alpha);
    for name in ["a.txt", "b.txt", "c.txt"] {
        write(&alpha.join(name), "baseline\n");
    }
    git_with_env(&alpha, &["add", "."], &GitEnv::fixture());
    git_with_env(&alpha, &["commit", "-m", "init"], &GitEnv::fixture());
    for name in ["a.txt", "b.txt", "c.txt"] {
        write(&alpha.join(name), "modified\n");
    }
    (tmp, project)
}

fn feedback_harness(project_dir: std::path::PathBuf) -> Harness<'static, AppState> {
    shell_harness_over(AppState::new(project_dir), egui::vec2(1024.0, 768.0))
}

fn wait_confirm_and_success(harness: &mut Harness<'_, AppState>, what: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        harness.step();
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

#[test]
fn discard_confirm_lists_every_affected_file() {
    let (_tmp, project) = repo_with_modified_files();
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    for path in ["a.txt", "b.txt", "c.txt"] {
        assert_painted(&harness, path);
    }
    assert_not_painted(&harness, "d.txt");
}

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

#[test]
fn discard_confirm_cancel_keeps_changes_intact() {
    let (_tmp, project) = repo_with_modified_files();
    let alpha = project.join("alpha");
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    harness.get_by_label("Cancel").click();
    wait_confirm_cleared(&mut harness, "Cancel click");

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
    for name in ["a.txt", "b.txt", "c.txt"] {
        let body = std::fs::read_to_string(alpha.join(name)).unwrap();
        assert_eq!(body, "modified\n", "{name} should remain modified");
    }
}

#[test]
fn discard_confirm_discard_button_runs_the_destructive_op() {
    let (_tmp, project) = repo_with_modified_files();
    let alpha = project.join("alpha");
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    harness.get_by_label("Discard 3-file changes").click();
    wait_confirm_and_success(&mut harness, "Discard");
    for name in ["a.txt", "b.txt", "c.txt"] {
        let body = std::fs::read_to_string(alpha.join(name)).unwrap();
        assert_eq!(body, "baseline\n", "{name} should revert to baseline");
    }
}

#[test]
fn discard_confirm_shelve_first_stashes_and_keeps_worktree_clean() {
    let (_tmp, project) = repo_with_modified_files();
    let alpha = project.join("alpha");
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    harness.get_by_label("Shelve first").click();
    wait_confirm_and_success(&mut harness, "Shelve first");

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

#[test]
fn discard_confirm_shelve_first_surfaces_a_toast() {
    let (_tmp, project) = repo_with_modified_files();
    let mut harness = feedback_harness(project);
    plant_discard(&mut harness);
    settle_quiet(&mut harness);

    harness.get_by_label("Shelve first").click();

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
