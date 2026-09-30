//! Issue 16 — Cherry-pick across repositories: the dialog (screen 05) and
//! its run, driven end-to-end through [`turbogit_ui::ui::render`] with the
//! kittest harness over a seeded two-repo project (real git, tempdir).
//!
//! Assertions use only public surfaces: painted output, public `AppState`
//! transitions, and the real git state of the seeded repos.

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::accesskit::Role;
use egui_kittest::kittest::NodeT as _;
use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::harness::{
    assert_painted, click_menu_item, right_click_row, settle_quiet, shell_harness_over,
};
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::bulk_run::RowState;

// --- git fixture ---------------------------------------------------------------

/// A `git` runner that pins the commit identity on every invocation.
///
/// Not `test_support::git_seed::git`, which takes no per-call env: `init_repo` writes the
/// identity into the repository too, and the dialog rows carry the author line, so
/// the two repos have to agree on it.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@t"]);
    git(dir, &["config", "user.name", "t"]);
}

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    #[expect(dead_code)]
    alpha: PathBuf,
    beta: PathBuf,
    #[expect(dead_code)]
    c1: String,
    c2: String,
    #[expect(dead_code)]
    c3: String,
}

/// `alpha` with commits c1..c3; `beta` a clean unrelated target.
fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    let beta = project.join("beta");
    init_repo(&alpha);
    let c1 = commit_file(&alpha, "a.txt", "one\n", "alpha: first commit");
    let c2 = commit_file(&alpha, "b.txt", "two\n", "alpha: second commit");
    let c3 = commit_file(&alpha, "c.txt", "three\n", "alpha: third commit");
    init_repo(&beta);
    commit_file(&beta, "base.txt", "base\n", "beta: base commit");
    Seed {
        _tmp: tmp,
        project,
        alpha,
        beta,
        c1,
        c2,
        c3,
    }
}

// --- harness -------------------------------------------------------------------

fn harness_with_log(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    for root in state.multi.roots.clone() {
        let commits = engine.log(&root.path, &LogOpts::default()).expect("log");
        state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.id.clone(),
                commits: Ok(commits),
                mode: LogBatchMode::Replace,
            })
            .expect("send LogLoaded");
    }
    state.drain_events();
    state.ui.tab = Tab::Log;

    // The shared shell launcher at this suite's box, and the shared `settle_quiet`
    // behind it: 300 frames and three consecutive stable fingerprints.
    let mut harness = shell_harness_over(
        state,
        egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    );
    settle_quiet(&mut harness);
    harness
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// Wait until `pred` holds on the harness state.
fn wait_for(harness: &mut Harness<'_, AppState>, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..300 {
        harness.step();
        if pred(harness.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("condition not met within 300 frames");
}

/// Open the cross-repo cherry-pick dialog from `c2`'s commit menu. The dialog is
/// unchanged; only the way in moved, with the action (ADR-0024).
fn open_dialog(harness: &mut Harness<'_, AppState>, c2: &str) {
    right_click_row(harness, &format!("{} alpha: second commit", short(c2)));
    click_menu_item(harness, "Copy hash", "Cherry-pick across…");
    settle_quiet(harness);
}

// --- the dialog ----------------------------------------------------------------

#[test]
fn the_dialog_lists_commits_targets_and_predictions_with_a_preview_rail() {
    let seed = seeded_project();
    let mut harness = harness_with_log(&seed);

    open_dialog(&mut harness, &seed.c2);

    // The commit checklist carries every candidate in application order,
    // and the targets table defaults to the other registered repo.
    assert_painted(&harness, "alpha: first commit");
    assert_painted(&harness, "alpha: second commit");
    assert_painted(&harness, "alpha: third commit");
    assert_painted(&harness, "will apply in order");
    assert_painted(&harness, "APPLIES");
    assert_painted(&harness, "RISK");
    assert_painted(&harness, "OUTCOME");
    assert_painted(&harness, "beta");
    assert_painted(&harness, "Stop on first conflict");

    // Checking a commit updates the prediction and focuses the patch
    // preview rail.
    harness
        .get_by_role_and_label(Role::CheckBox, "alpha: second commit")
        .click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "1/1");
    assert_painted(&harness, "PATCH PREVIEW");
    assert_painted(&harness, "b.txt");
    assert_painted(&harness, "+two");

    // Both selected commits apply cleanly at low risk into an unrelated
    // repo.
    harness.get_by_label("alpha: third commit").click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "2/2");
    assert_painted(&harness, "clean apply");
    assert_painted(&harness, "Low");
}

#[test]
fn the_dialog_search_filters_the_commit_checklist() {
    let seed = seeded_project();
    let mut harness = harness_with_log(&seed);
    open_dialog(&mut harness, &seed.c2);

    harness
        .get_by_label("Search commits by message, author, or hash")
        .click();
    harness
        .get_by_label("Search commits by message, author, or hash")
        .type_text("second");
    settle_quiet(&mut harness);

    // The filtered checklist keeps the matching commit's checkbox and drops
    // the others (the graph behind the dialog still paints its rows).
    assert!(
        harness
            .get_all_by_role(egui::accesskit::Role::CheckBox)
            .any(|n| n.accesskit_node().label() == Some("alpha: second commit".to_string())),
        "the matching commit's checkbox remains"
    );
    assert!(
        !harness
            .get_all_by_role(egui::accesskit::Role::CheckBox)
            .any(|n| n.accesskit_node().label() == Some("alpha: first commit".to_string())),
        "non-matching commits are filtered out of the checklist"
    );
}

#[test]
fn executing_queues_the_run_and_reports_through_the_monitor() {
    let seed = seeded_project();
    let mut harness = harness_with_log(&seed);
    open_dialog(&mut harness, &seed.c2);
    harness
        .get_by_role_and_label(Role::CheckBox, "alpha: second commit")
        .click();
    settle_quiet(&mut harness);

    harness.get_by_label("Cherry-pick to 1 repo").click();
    settle_quiet(&mut harness);

    // The monitor replaces the dialog and runs to completion.
    assert!(harness.state().ui.dialog.is_none(), "the dialog closed");
    assert!(
        harness.state().ui.cherry_run.is_some(),
        "the run monitor opened"
    );
    assert_painted(&harness, "Cherry-pick across — live");
    wait_for(&mut harness, |s| {
        s.ui.cherry_run
            .as_ref()
            .is_some_and(|v| v.rows.iter().all(|r| r.state == RowState::Done))
    });
    assert_painted(&harness, "Done");

    // The pick really landed in the target repo.
    let subjects = git(&seed.beta, &["log", "--format=%s"]);
    assert!(
        subjects.contains("alpha: second commit"),
        "the target gained the picked commit: {subjects}"
    );
}
