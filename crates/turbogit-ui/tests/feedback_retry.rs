//! Issue #02 — Retry action on error toasts.
//!
//! The contract: a toast carrying a `retry: Some(RetryAction)` paints a
//! `Retry` button alongside `Dismiss`; clicking it clears the toast and
//! re-dispatches the action through `AppState::retry`, which runs the
//! equivalent of the original `run_git` call.
//!
//! Headless egui_kittest harness driving the production
//! [`turbogit_ui::ui::render`] over a real temp git repository. Assertions
//! are on painted text and on public `AppState` transitions
//! (toast, busy, last_error) — never on internal calls.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::harness::{assert_painted, painted_text};
use turbogit_app::state::{AppState, RetryAction, Toast, ToastKind};
use turbogit_ui::ui::popups::Action as PopupAction;

// --- git fixture ---------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_AUTHOR_COMMITTER_NAME", "t")
        .env("GIT_AUTHOR_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn commit_readme(repo: &Path) {
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "init"]);
}

/// One bare local remote (`origin`) plus one local repo `alpha` on `main`
/// so `Fetch { remote: Some("origin") }` is a real, valid retry action.
fn repo_project() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let remote = project.join("origin.git");
    git(
        &project,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    let alpha = project.join("alpha");
    commit_readme(&alpha);
    git(
        &alpha,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&alpha, &["push", "-u", "origin", "main"]);
    (tmp, project, alpha)
}

// --- harness -------------------------------------------------------------------

fn feedback_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
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
    panic!("feedback layout did not settle within 300 frames; last painted:\n{prev}");
}

/// Wait for the toast to disappear (Retry/Dismiss clears it).
fn wait_toast_cleared(harness: &mut Harness<'_, AppState>, what: &str) {
    for _ in 0..200 {
        harness.step();
        if harness.state().ui.toast.is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let s = harness.state();
    panic!(
        "timed out waiting for toast to clear after {what}; toast={:?} last_error={:?}",
        s.ui.toast, s.last_error
    );
}

// --- tests ---------------------------------------------------------------------

/// Contract: an error toast that carries a retry action paints a `Retry`
/// button. A toast without retry must NOT paint a Retry button.
#[test]
fn error_toast_with_retry_paints_retry_button() {
    let (_tmp, project, alpha) = repo_project();
    let mut harness = feedback_harness(project);

    // Plant a fetch-failure toast with a retry handle. The real fetch is
    // never dispatched during this assertion — the only thing we check is
    // that the toast paints the Retry affordance.
    {
        let st = harness.state_mut();
        st.ui.toast = Some(Toast {
            kind: ToastKind::Error,
            message: "Fetch from origin failed: network unreachable".into(),
            retry: Some(RetryAction::Fetch {
                root: alpha.clone(),
                remote: Some("origin".into()),
            }),
        });
        st.ui.toast_shown_at = None;
    }
    settle_quiet(&mut harness);

    assert_painted(&harness, "Retry");
    assert_painted(&harness, "Fetch from origin failed");

    // Sanity: a toast with retry=None must NOT paint the Retry button
    // (so the contract is genuinely conditional on the field).
    {
        let st = harness.state_mut();
        st.ui.toast = Some(Toast::error("Plain failure, no retry."));
        st.ui.toast_shown_at = None;
    }
    settle_quiet(&mut harness);
    assert_painted(&harness, "Plain failure, no retry.");

    let painted = painted_text(&harness).join("\n");
    assert!(
        !painted.split_whitespace().any(|tok| tok == "Retry"),
        "toast without retry must not paint a Retry button; painted:\n{painted}"
    );
}

/// Contract: clicking the Retry button clears the toast and re-dispatches
/// the action through `AppState::retry`. We observe the public side
/// effect: the toast goes away AND a fresh op runs (busy briefly, then a
/// success toast appears).
#[test]
fn clicking_retry_dispatches_the_action() {
    let (_tmp, project, alpha) = repo_project();
    let mut harness = feedback_harness(project);

    {
        let st = harness.state_mut();
        st.ui.toast = Some(Toast {
            kind: ToastKind::Error,
            message: "Fetch from origin failed".into(),
            retry: Some(RetryAction::Fetch {
                root: alpha.clone(),
                remote: Some("origin".into()),
            }),
        });
        st.ui.toast_shown_at = None;
    }
    settle_quiet(&mut harness);
    assert_painted(&harness, "Retry");

    // Click Retry. The kittest label `Retry` is unique on this frame.
    harness.get_by_label("Retry").click();
    wait_toast_cleared(&mut harness, "Retry click");

    // The fetch over the local bare remote succeeds: a success toast
    // should appear that names the op label.
    let mut saw_success = false;
    for _ in 0..200 {
        harness.step();
        if let Some(t) = &harness.state().ui.toast
            && t.kind == ToastKind::Success
            && t.message.contains("Fetch")
        {
            saw_success = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        saw_success,
        "Retry click should re-dispatch the fetch and surface a success toast; \
         current toast: {:?}",
        harness.state().ui.toast
    );
}

/// Contract: clicking Retry when the action's root no longer exists does
/// not leave the toast stuck. The retry runs, fails, and the resulting
/// error toast replaces the previous one (no leaked state).
#[test]
fn retry_with_missing_root_reports_a_fresh_error() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = feedback_harness(project);

    let missing = PathBuf::from("/nonexistent/root/path/that/never/existed");

    {
        let st = harness.state_mut();
        st.ui.toast = Some(Toast {
            kind: ToastKind::Error,
            message: "Fetch failed".into(),
            retry: Some(RetryAction::Fetch {
                root: missing,
                remote: Some("origin".into()),
            }),
        });
        st.ui.toast_shown_at = None;
    }
    settle_quiet(&mut harness);

    harness.get_by_label("Retry").click();

    // After Retry, the worker thread posts OpCompleted and a fresh error
    // toast replaces the previous one. Poll a bounded number of steps
    // (the toast is auto-dismissed after ~4s of egui-time, which elapses
    // quickly here because settle_quiet already advanced the clock).
    let mut seen_fresh_error = false;
    for i in 0..200 {
        harness.step();
        let s = harness.state();
        if let Some(t) = &s.ui.toast {
            // "Fetch from origin:" is the retry() label when remote is
            // Some; "Fetch:" is the label when remote is None. Either
            // means a fresh retry toast arrived (replacing the plant).
            if t.message.starts_with("Fetch from origin:") || t.message.starts_with("Fetch:") {
                seen_fresh_error = true;
                break;
            }
        }
        // Avoid log spam: only print the first 5 / last 5 steps.
        if !(5..=195).contains(&i) {
            let toast_msg =
                s.ui.toast
                    .as_ref()
                    .map(|t| (t.message.clone(), t.retry.is_some()));
            eprintln!(
                "step={i} toast={toast_msg:?} last_error={:?} busy={}",
                s.last_error, s.ui.busy
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        seen_fresh_error,
        "after retry against a missing root, a fresh error toast must appear"
    );
}
// Reference the popup Action so the use isn't dead (parallel to feedback_chrome).
#[allow(dead_code)]
fn _action_anchor(_: PopupAction) {}
