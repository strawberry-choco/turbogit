//! Issue #02 — Inline banner component (severity + deep-link actions).
//!
//! The contract: a [`Banner`] is a strip rendered at the top of a tool
//! surface that carries a semantic severity, a one-line message, and zero
//! or more clickable action buttons. Each action's label is painted and
//! clicking it invokes the action's effect through `AppState`. Severity
//! drives the strip's accent color (issue #02).
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] over
//! a real temp git repository so the banner participates in the same
//! frame paint order as the rest of the shell. Assertions are on
//! painted text and on public `AppState` transitions.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::harness::{assert_not_painted, assert_painted, painted_text};
use turbogit_app::banner::{Banner, BannerAction, BannerSeverity};
use turbogit_app::state::AppState;

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

/// One local repo on `main` (no remote — the banner tests don't push).
fn repo_project() -> (TempDir, std::path::PathBuf) {
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
    panic!("feedback layout did not settle within 300 frames; last painted:\n{prev}");
}

// --- tests ---------------------------------------------------------------------

/// Contract: a banner paints its message AND every action label. The
/// "Shelf restored" use case in the issue (deep link to diff) is the
/// shape we assert — two actions on one banner, both clickable through
/// the accessibility tree.
#[test]
fn banner_paints_message_and_every_action_label() {
    let (_tmp, project) = repo_project();
    let mut harness = feedback_harness(project);

    // Show a banner with two deep-link actions. The actions are inert
    // closures; the assertion only requires the labels to be painted and
    // reachable through kittest's accessibility tree.
    let banner = Banner::new(BannerSeverity::Info, "Stash restored")
        .action(BannerAction::new("View diff", |_state| {}))
        .action(BannerAction::new("Dismiss", |_state| {}));
    harness.state_mut().ui.banner = Some(banner);
    settle_quiet(&mut harness);

    assert_painted(&harness, "Stash restored");
    assert_painted(&harness, "View diff");
    assert_painted(&harness, "Dismiss");
}

/// Contract: a banner with a single action still renders the message and
/// the action button. Sanity for the "single deep link" case.
#[test]
fn banner_with_single_action_renders() {
    let (_tmp, project) = repo_project();
    let mut harness = feedback_harness(project);

    let banner = Banner::new(BannerSeverity::Warning, "2 repos rejected the cascade push")
        .action(BannerAction::new("Review", |_state| {}));
    harness.state_mut().ui.banner = Some(banner);
    settle_quiet(&mut harness);

    assert_painted(&harness, "2 repos rejected the cascade push");
    assert_painted(&harness, "Review");
}

/// Contract: when no banner is set on the state, no banner chrome paints.
/// Sanity that the component is genuinely conditional on the field.
#[test]
fn no_banner_paints_no_banner_chrome() {
    let (_tmp, project) = repo_project();
    let mut harness = feedback_harness(project);

    // Make sure no banner is set.
    harness.state_mut().ui.banner = None;
    settle_quiet(&mut harness);

    // The text the banner would carry must NOT be painted when the banner
    // is absent (it lives in the banner message, not in the shell).
    assert_not_painted(&harness, "Stash restored");
    assert_not_painted(&harness, "View diff");
}

/// Contract: clicking a banner action's label invokes the action against
/// `AppState`. The action can mutate state (e.g. set a toast) and the
/// change is visible after a few frames.
#[test]
fn clicking_a_banner_action_invokes_it_against_appstate() {
    let (_tmp, project) = repo_project();
    let mut harness = feedback_harness(project);

    let banner = Banner::new(BannerSeverity::Info, "Stash restored").action(BannerAction::new(
        "Acknowledge",
        |state| {
            state.ui.toast = Some(turbogit_app::state::Toast::info("acknowledged"));
        },
    ));
    harness.state_mut().ui.banner = Some(banner);
    settle_quiet(&mut harness);
    assert_painted(&harness, "Acknowledge");

    harness.get_by_label("Acknowledge").click();

    // Wait for the action's effect to surface as a toast.
    let mut saw = false;
    for _ in 0..200 {
        harness.step();
        if let Some(t) = &harness.state().ui.toast
            && t.message == "acknowledged"
        {
            saw = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        saw,
        "clicking a banner action should run its closure against AppState; \
         current toast: {:?}",
        harness.state().ui.toast
    );
}
