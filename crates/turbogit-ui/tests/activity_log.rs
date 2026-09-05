//! Issue #04 — Activity log panel: the collapsible bottom-of-shell panel.
//!
//! Headless egui_kittest harness driving the production
//! [`turbogit_ui::ui::render`] over a real temp git repository. Entries are
//! planted through the public [`turbogit_app::activity::ActivityLog`] API
//! (the same surface `AppState` appends through); the end-to-end test drives
//! a real fetch from the topbar button. Assertions are on painted text and
//! public state transitions — never on internals.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::harness::{assert_not_painted, assert_painted, painted_text};
use turbogit_app::activity::{ActivityEntry, ActivityKind, TimeWindow};
use turbogit_app::state::AppState;

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
/// with `origin` configured, so a topbar Fetch is a real, valid operation.
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

fn activity_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    let state = turbogit_app::state::AppState::new(project_dir);
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

fn settle_quiet(harness: &mut Harness<'_, turbogit_app::state::AppState>) {
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
    panic!("activity layout did not settle within 300 frames; last painted:\n{prev}");
}

fn entry(minutes_ago: i64, repo: Option<&str>, message: &str, kind: ActivityKind) -> ActivityEntry {
    ActivityEntry {
        at: chrono::Local::now() - chrono::Duration::minutes(minutes_ago),
        repo: repo.map(str::to_string),
        message: message.to_string(),
        kind,
    }
}

/// Plant entries through the public ActivityLog API, expand the panel (the
/// same click a user performs — the panel defaults collapsed so short
/// displays keep their tool-window space), and settle.
fn plant(harness: &mut Harness<'_, turbogit_app::state::AppState>, entries: Vec<ActivityEntry>) {
    {
        let st = harness.state_mut();
        st.ui.activity.expanded = true;
        for e in entries {
            st.ui.activity.push(e);
        }
    }
    settle_quiet(harness);
}

// --- tests ---------------------------------------------------------------------

/// Contract: the expanded panel paints each entry's timestamp, repo, and
/// human-readable outcome; the header carries the ACTIVITY title and the
/// filter/clear controls.
#[test]
fn panel_paints_entries_with_timestamp_repo_and_outcome() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![
            entry(0, Some("alpha"), "Fetch from origin", ActivityKind::Success),
            entry(
                1,
                Some("alpha"),
                "Push origin/main: rejected",
                ActivityKind::Error,
            ),
        ],
    );

    assert_painted(&harness, "ACTIVITY");
    assert_painted(&harness, "Fetch from origin");
    assert_painted(&harness, "Push origin/main: rejected");
    assert_painted(&harness, "alpha");
    // Filter + clear controls of the header.
    assert_painted(&harness, "All repos");
    assert_painted(&harness, "All time");
    assert_painted(&harness, "Clear feed");

    // Severity mapping is a design-token contract: each activity kind
    // paints with its STATE_* token.
    assert_eq!(
        turbogit_ui::ui::activity_panel::kind_color(ActivityKind::Success),
        turbogit_ui::theme::Palette::STATE_SUCCESS
    );
    assert_eq!(
        turbogit_ui::ui::activity_panel::kind_color(ActivityKind::Warning),
        turbogit_ui::theme::Palette::STATE_WARNING
    );
    assert_eq!(
        turbogit_ui::ui::activity_panel::kind_color(ActivityKind::Error),
        turbogit_ui::theme::Palette::STATE_ERROR
    );
}

/// Contract: an empty expanded feed shows a quiet empty hint, not a blank
/// strip.
#[test]
fn empty_feed_paints_a_hint() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);
    harness.state_mut().ui.activity.expanded = true;
    settle_quiet(&mut harness);
    assert_painted(&harness, "ACTIVITY");
    assert_painted(&harness, "No activity yet");
}

/// Contract: collapsing hides the entries but keeps them; expanding shows
/// them again. The collapse/expand toggle is the "Activity log" control.
#[test]
fn collapse_and_expand_retain_entries() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![entry(
            0,
            Some("alpha"),
            "Fetch from origin",
            ActivityKind::Success,
        )],
    );
    assert!(harness.state().ui.activity.expanded);
    assert_painted(&harness, "Fetch from origin");

    harness.get_by_label("Collapse activity").click();
    settle_quiet(&mut harness);
    assert!(!harness.state().ui.activity.expanded);
    assert_not_painted(&harness, "Fetch from origin");
    // Collapsed header strip is still visible.
    assert_painted(&harness, "ACTIVITY");

    // The entries survive the collapse (session-durable feed).
    assert_eq!(harness.state().ui.activity.entries.len(), 1);

    harness.get_by_label("Expand activity").click();
    settle_quiet(&mut harness);
    assert!(harness.state().ui.activity.expanded);
    assert_painted(&harness, "Fetch from origin");
}

/// Contract: Clear empties the feed — the painted entries disappear and the
/// store is empty.
#[test]
fn clear_empties_the_feed() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![entry(
            0,
            Some("alpha"),
            "Fetch from origin",
            ActivityKind::Success,
        )],
    );
    assert_painted(&harness, "Fetch from origin");

    harness.get_by_label("Clear feed").click();
    settle_quiet(&mut harness);

    assert!(harness.state().ui.activity.entries.is_empty());
    assert_not_painted(&harness, "Fetch from origin");
    assert_painted(&harness, "No activity yet");
}

/// Contract: the repo filter cycles (All repos → alpha → …) and narrows the
/// visible feed to the chosen repo's entries.
#[test]
fn repo_filter_narrows_the_painted_feed() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![
            entry(0, Some("alpha"), "Fetch from origin", ActivityKind::Success),
            entry(0, Some("beta"), "Pull upstream", ActivityKind::Success),
        ],
    );
    assert_painted(&harness, "Fetch from origin");
    assert_painted(&harness, "Pull upstream");

    harness.get_by_label("All repos").click();
    settle_quiet(&mut harness);

    // Cycled to the only registered root (alpha): alpha's entries stay,
    // beta's disappear from the feed (the store still holds them).
    assert_eq!(
        harness.state().ui.activity.repo_filter.as_deref(),
        Some("alpha")
    );
    assert_painted(&harness, "Fetch from origin");
    assert_not_painted(&harness, "Pull upstream");
    assert_eq!(harness.state().ui.activity.entries.len(), 2);
}

/// Contract: the time-window filter cycles (All time → Last 30 min) and
/// hides entries older than the window.
#[test]
fn time_window_filter_hides_old_entries() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);

    plant(
        &mut harness,
        vec![
            entry(0, Some("alpha"), "fresh fetch", ActivityKind::Success),
            entry(40, Some("alpha"), "old fetch", ActivityKind::Success),
        ],
    );
    assert_painted(&harness, "fresh fetch");
    assert_painted(&harness, "old fetch");

    harness.get_by_label("All time").click();
    settle_quiet(&mut harness);

    assert_eq!(harness.state().ui.activity.window, TimeWindow::Last30Min);
    assert_painted(&harness, "fresh fetch");
    assert_not_painted(&harness, "old fetch");
}

/// Contract (end to end): a real fetch dispatched from the topbar lands in
/// the activity panel — the OpCompleted → entry → paint pipeline.
#[test]
fn real_fetch_lands_in_the_panel() {
    let (_tmp, project, _alpha) = repo_project();
    let mut harness = activity_harness(project);
    settle_quiet(&mut harness);

    harness.state_mut().ui.activity.expanded = true;
    settle_quiet(&mut harness);
    harness.get_by_label("Fetch").click();

    // The topbar dispatches the op labeled "Fetch"; wait for its entry to
    // land in the store and be painted in the feed.
    let mut landed = false;
    for _ in 0..300 {
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
        let stored = harness
            .state()
            .ui
            .activity
            .entries
            .iter()
            .any(|e| e.message == "Fetch" && e.kind == ActivityKind::Success);
        if stored {
            landed = true;
            break;
        }
    }
    assert!(
        landed,
        "real fetch should be recorded as an activity entry; entries: {:?}",
        harness.state().ui.activity.entries
    );
    // The new entry is visible in the painted feed: its repo label (alpha)
    // appears more than once on screen — topbar breadcrumb + feed row.
    let alpha_rows = painted_text(&harness)
        .iter()
        .filter(|t| t.trim() == "alpha")
        .count();
    assert!(
        alpha_rows >= 2,
        "the fetch entry's repo row should be painted; painted:\n{:?}",
        painted_text(&harness)
    );
}
