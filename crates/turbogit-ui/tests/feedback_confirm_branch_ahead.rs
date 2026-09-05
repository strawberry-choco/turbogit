//! Issue #02 — Delete-branch confirmation warns when ahead of main and
//! unmerged.
//!
//! Contract (issue #02):
//! - The Delete local branch confirm dialog adds a "branch is ahead of
//!   `<upstream>` and not merged" warning line when the branch is ahead
//!   of its tracking upstream and that upstream is NOT an ancestor of
//!   the branch.
//! - The warning is omitted when the branch is fully merged (the
//!   default safe-delete path).

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use egui_kittest::Harness;
use tempfile::TempDir;
use test_support::harness::{assert_not_painted, assert_painted, painted_text};
use turbogit_app::state::{AppState, PendingConfirm};

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

/// A repo on `main` with a local `feature` branch that has commits
/// ahead of `main` (i.e. the upstream is NOT an ancestor of feature).
fn repo_with_unmerged_feature() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    let alpha = project.join("alpha");
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-m", "init"]);
    // Branch from main, add a commit on top, and set main as the
    // upstream. `feature` is now ahead of main and main is not an
    // ancestor of `feature`.
    git(&alpha, &["checkout", "-b", "feature"]);
    std::fs::write(alpha.join("feature.txt"), "y\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-m", "feature work"]);
    git(&alpha, &["branch", "--set-upstream-to=main", "feature"]);
    (tmp, project)
}

/// A repo on `main` with a `feature` branch whose commits are all on
/// main too (i.e. main IS an ancestor of feature — safe to delete).
fn repo_with_merged_feature() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    let alpha = project.join("alpha");
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-m", "init"]);
    git(&alpha, &["checkout", "-b", "feature"]);
    // No new commit: feature points at the same commit as main.
    git(&alpha, &["checkout", "main"]);
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

// --- tests ---------------------------------------------------------------------

/// Contract: when the branch is ahead of upstream and upstream is not
/// an ancestor, the Delete local branch confirm dialog paints an
/// "ahead of main · not merged" warning.
#[test]
fn delete_branch_confirm_warns_when_ahead_and_unmerged() {
    let (_tmp, project) = repo_with_unmerged_feature();
    let mut harness = feedback_harness(project);

    {
        let st = harness.state_mut();
        st.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
            name: "feature".into(),
        });
    }
    settle_quiet(&mut harness);

    // Standard confirm body.
    assert_painted(
        &harness,
        "Delete local branch 'feature'? This cannot be undone.",
    );
    // Ahead-of-main warning.
    assert_painted(&harness, "ahead");
    assert_painted(&harness, "not merged");
}

/// Contract: when the branch is fully merged into main, the warning is
/// OMITTED (the safe-delete path is the default behavior).
#[test]
fn delete_branch_confirm_omits_warning_when_merged() {
    let (_tmp, project) = repo_with_merged_feature();
    let mut harness = feedback_harness(project);

    {
        let st = harness.state_mut();
        st.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
            name: "feature".into(),
        });
    }
    settle_quiet(&mut harness);

    // The standard confirm body is present.
    assert_painted(
        &harness,
        "Delete local branch 'feature'? This cannot be undone.",
    );
    // The ahead-of-main warning is NOT painted.
    assert_not_painted(&harness, "not merged");
}
