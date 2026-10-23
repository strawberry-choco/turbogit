//! The Log view's diff verbs: the changed-files pane's row click and its
//! `Open diff` footer link both name one file in one commit, and the
//! `View in Diff` escape names a whole commit.

use std::path::{Path, PathBuf};

use egui_kittest::{Harness, kittest::Queryable};
use tempfile::TempDir;
use test_support::git_seed::{commit, git, pin_eol_lf};
use test_support::harness::{assert_not_painted, assert_painted, painted_text, settle_reads};
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::{configure_style, install_fonts};

// --- fixture -------------------------------------------------------------------

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    /// HEAD~2 — added a second line to `tracked.txt`, so its diff is one
    /// addition and nothing else.
    c2: String,
}

/// The line only the newest commit's version of the file has. No other widget
/// in the window can paint it, unlike the commit messages, which the graph row
/// and the details pane both paint.
const ADDED_LINE: &str = "zz-added-by-second-commit";

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    commit(dir, name, body, msg);
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// One root `alpha`: c1 creates tracked.txt, c2 appends one line to it, c3 is a
/// docs-only commit.
fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    std::fs::create_dir_all(&alpha).expect("alpha dir");
    git(&alpha, &["init", "-b", "main"]);
    git(&alpha, &["config", "user.email", "test@example.com"]);
    git(&alpha, &["config", "user.name", "Test"]);
    pin_eol_lf(&alpha);
    let _c1 = commit_file(
        &alpha,
        "tracked.txt",
        "first line\n",
        "message of the first commit",
    );
    let c2 = commit_file(
        &alpha,
        "tracked.txt",
        &format!("first line\n{ADDED_LINE}\n"),
        "message of the second commit",
    );
    let _c3 = commit_file(&alpha, "README.md", "docs", "message of the docs commit");
    Seed {
        _tmp: tmp,
        project,
        c2,
    }
}

fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    assert_eq!(state.multi.roots.len(), 1, "one root discovered");
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

    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events(); // production parity with app.rs
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(
        1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
        800.0,
    ));
    settle_reads(&mut harness);
    harness
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// Select the commit row and its changed-file entry, landing on the
/// changed-files pane with the footer links enabled.
fn select_commit_and_file(harness: &mut Harness<'_, AppState>, seed: &Seed) {
    let row_label = format!("{} message of the second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    settle_reads(harness);
    harness.get_by_label("tracked.txt").click();
    settle_reads(harness);
}

/// **Clicking a changed file in the Log view shows that file's diff at that
/// commit.**
#[test]
fn a_changed_file_row_click_paints_its_diff_at_that_commit() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    select_commit_and_file(&mut harness, &seed);

    assert_painted(&harness, ADDED_LINE);
}

/// **A diff opened from the Log view is history, not the worktree: it offers
/// no staging.**
///
/// The granular verbs compose against the index, and a commit-to-commit pair
/// has no index on either side — so the surface paints read-only: no hunk
/// staging pair, and no row that claims a staged-ness it cannot honour.
#[test]
fn a_diff_opened_from_the_log_offers_no_staging() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    select_commit_and_file(&mut harness, &seed);

    for label in ["Stage hunk 1", "Unstage hunk 1"] {
        assert!(
            harness.query_by_label(label).is_none(),
            "a historical diff must not offer `{label}`"
        );
    }
    assert!(
        harness
            .query_by_label(&format!("{ADDED_LINE} (staged-ness unknown)"))
            .is_none(),
        "a historical row must not answer the staged-ness question at all"
    );
}

/// **Closing the diff window closes the window.**
#[test]
fn closing_the_diff_window_closes_the_window() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    select_commit_and_file(&mut harness, &seed);
    assert_painted(&harness, ADDED_LINE);
    harness.get_by_label("Close").click();
    settle_reads(&mut harness);

    assert_not_painted(&harness, ADDED_LINE);
}

/// **A root commit's changed file diffs the commit against the empty tree,
/// not the working tree.**
///
/// The worktree is dirtied after the commit on purpose: a comparison that
/// fell back to plain `git diff` would paint the dirty line as part of the
/// commit's change.
#[test]
fn a_root_commit_file_click_paints_the_commit_s_own_diff() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    std::fs::create_dir_all(&alpha).expect("alpha dir");
    git(&alpha, &["init", "-b", "main"]);
    git(&alpha, &["config", "user.email", "test@example.com"]);
    git(&alpha, &["config", "user.name", "Test"]);
    pin_eol_lf(&alpha);
    let root_added = "zz-added-by-root-commit";
    let worktree_added = "zz-dirty-worktree-line";
    let cid = commit_file(
        &alpha,
        "tracked.txt",
        &format!("first line\n{root_added}\n"),
        "message of the only commit",
    );
    let file = alpha.join("tracked.txt");
    let mut body = std::fs::read_to_string(&file).expect("read");
    body.push_str(worktree_added);
    body.push('\n');
    std::fs::write(&file, body).expect("dirty the worktree");

    let seed = Seed {
        _tmp: tmp,
        project,
        c2: String::new(),
    };
    let mut harness = log_harness(&seed);
    harness
        .get_by_label(&format!("{} message of the only commit", short(&cid)))
        .click();
    settle_reads(&mut harness);
    harness.get_by_label("tracked.txt").click();
    settle_reads(&mut harness);

    assert_painted(&harness, root_added);
    assert!(
        !painted_text(&harness)
            .iter()
            .any(|line| line.contains(worktree_added)),
        "the worktree's own dirt is not part of the commit's change"
    );
}
