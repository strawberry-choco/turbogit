//! Issue 18 — Blame view: reachable from the changed-files pane footer
//! ("Open diff · Blame · Full path history", screen 09) and from the
//! changed-file context menu; per-line commit/author/age attribution with
//! the blamed-at commit's lines highlighted; clicking a line's commit
//! navigates to it in the log.
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] over
//! the seeded single-focus project from `git_log.rs`'s fixture shape, with
//! the worker-event channel drained every frame (production parity).

use std::path::{Path, PathBuf};

use egui::{Color32, Pos2, Rect, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use tempfile::TempDir;
use turbogit_app::events::AppEvent;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::{configure_style, install_fonts};

// --- helpers (mirrors tests/git_log.rs) --------------------------------------

fn painted_text(harness: &Harness<'_, AppState>) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

#[track_caller]
fn assert_not_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        !texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was unexpectedly painted; painted text:\n{texts:#?}"
    );
}

fn filled_rects(harness: &Harness<'_, AppState>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect_shape) if rect_shape.fill != Color32::TRANSPARENT => {
                Some((rect_shape.rect, rect_shape.fill))
            }
            _ => None,
        })
        .collect()
}

/// Step frames until the painted output stabilizes and no async blame/diff
/// load is pending (mirrors tests/diff_viewer.rs's settle).
pub fn settle(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prev = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|diff={}|blame={}",
            painted_text(harness),
            harness.state().ui.diff_loading,
            harness.state().ui.blame_loading,
        );
        if fingerprint == prev
            && !harness.state().ui.diff_loading
            && !harness.state().ui.blame_loading
        {
            return;
        }
        prev = fingerprint;
    }
    panic!("log/blame layout did not settle within 15s");
}

// --- fixture -------------------------------------------------------------------

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    /// HEAD~1 — rewrote `file.txt` to one line ("alpha: second commit").
    c2: String,
    /// HEAD~2 — created `file.txt` ("alpha: initial commit").
    c1: String,
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).expect("writing work file");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", msg]);
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// One root `alpha`: c1 creates file.txt (one line), c2 rewrites it to two
/// lines, c3 is docs-only. Blame at c2 attributes line 1 → c1, line 2 → c2.
fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    std::fs::create_dir_all(&alpha).expect("alpha dir");
    run_git(&alpha, &["init", "-b", "main"]);
    run_git(&alpha, &["config", "user.email", "test@example.com"]);
    run_git(&alpha, &["config", "user.name", "Test"]);
    let c1 = commit_file(
        &alpha,
        "file.txt",
        "alpha: initial commit\n",
        "alpha: initial commit",
    );
    let c2 = commit_file(
        &alpha,
        "file.txt",
        "alpha: initial commit\nalpha: second commit\n",
        "alpha: second commit",
    );
    let _c3 = commit_file(
        &alpha,
        "README.md",
        "alpha: docs commit",
        "alpha: docs commit",
    );
    Seed {
        _tmp: tmp,
        project,
        c2,
        c1,
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
    settle(&mut harness);
    harness
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// Select the commit row, then its changed-file entry, landing on the
/// changed-files pane with the footer links enabled.
fn select_commit_and_file(harness: &mut Harness<'_, AppState>, seed: &Seed) {
    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    settle(harness);
    harness.get_by_label("file.txt").click();
    settle(harness);
}

// --- Cycle 3: footer entry + per-line attribution ------------------------------

#[test]
fn blame_opens_from_the_footer_and_paints_per_line_attribution() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    select_commit_and_file(&mut harness, &seed);

    // Footer links (screen 09) sit under the changed-files pane, acting on
    // the selected file.
    assert_painted(&harness, "Open diff");
    assert_painted(&harness, "Blame");
    assert_painted(&harness, "Full path history");

    harness.get_by_label("Blame").click();
    settle(&mut harness);

    // The blame surface replaced the graph pane and records its target.
    assert!(harness.state().ui.blame.is_some(), "blame target recorded");
    assert_painted(&harness, "Close blame");
    assert_painted(&harness, "file.txt");

    // Per-line attribution: both introducing commits' hashes, the author,
    // a relative age, and every line's content.
    assert_painted(&harness, &short(&seed.c1));
    assert_painted(&harness, &short(&seed.c2));
    assert_painted(&harness, "Test");
    assert!(
        painted_text(&harness).iter().any(|t| t.ends_with("s ago")),
        "a relative age must be painted; painted text:\n{:#?}",
        painted_text(&harness)
    );
    assert_painted(&harness, "alpha: initial commit");
    assert_painted(&harness, "alpha: second commit");

    // Closing blame returns to the commit graph.
    harness.get_by_label("Close blame").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.blame, None, "close drops the target");
    assert_not_painted(&harness, "Close blame");
    assert_painted(&harness, "HASH");
}

// --- Cycle 4: current commit's lines highlighted -------------------------------

/// Every paint-time origin of a galley painting exactly `text` — the same
/// string can appear in several panes (blame row + details message).
fn galley_origins(harness: &Harness<'_, AppState>, text: &str) -> Vec<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => Some(shape.pos),
            _ => None,
        })
        .collect()
}

#[test]
fn lines_from_the_blamed_commit_are_highlighted() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    harness.get_by_label("Blame").click();
    settle(&mut harness);

    // Line 2 was introduced by the blamed commit (c2): it carries the
    // translucent selection tint; line 1 (c1's) does not.
    let tint = turbogit_ui::theme::Palette::selection_bg();
    let in_tint = |harness: &Harness<'_, AppState>, text: &str| {
        let origins = galley_origins(harness, text);
        assert!(!origins.is_empty(), "`{text}` must be painted");
        filled_rects(harness)
            .iter()
            .any(|(r, c)| *c == tint && origins.iter().any(|p| r.contains(*p)))
    };
    assert!(
        in_tint(&harness, "alpha: second commit"),
        "line 2 must sit on a selection-tinted rect"
    );
    assert!(
        !in_tint(&harness, "alpha: initial commit"),
        "line 1 (a different commit's line) must stay unhighlighted"
    );
}

// --- Cycle 5: clicking a line's commit navigates to the log --------------------

#[test]
fn clicking_a_blamed_line_navigates_to_its_commit_in_the_log() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    harness.get_by_label("Blame").click();
    settle(&mut harness);

    // Click line 1 (introduced by c1) → the log selects c1 and the blame
    // view closes.
    let row_label = format!("{} alpha: initial commit", short(&seed.c1));
    harness.get_by_label(&row_label).click();
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c1.as_str()),
        "the line's commit must become the log selection"
    );
    assert_eq!(harness.state().ui.blame, None, "blame view closed");
    assert_painted(&harness, "HASH");
}

// --- Cycle 6: context-menu entry + path-scope respect --------------------------

#[test]
fn context_menu_opens_blame_and_the_path_scope_survives_it() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    // Scope the log to file.txt via its changed-file context menu.
    let c2_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&c2_label).click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click_secondary();
    settle(&mut harness);
    harness.get_by_label("Show history for file...").click();
    settle(&mut harness);
    assert!(harness.state().ui.log_path_scope.is_some());
    assert_not_painted(&harness, "alpha: docs commit");

    // Open blame on the scoped file from the footer, on a scoped commit.
    harness.get_by_label(&c2_label).click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click();
    settle(&mut harness);
    harness.get_by_label("Blame").click();
    settle(&mut harness);
    assert!(harness.state().ui.blame.is_some());
    assert_painted(&harness, "alpha: second commit");

    // Back to the log: the scope is untouched — the graph still lists only
    // commits touching file.txt.
    harness.get_by_label("Close blame").click();
    settle(&mut harness);
    assert!(harness.state().ui.log_path_scope.is_some());
    assert_painted(&harness, "alpha: initial commit");
    assert_not_painted(&harness, "alpha: docs commit");

    // The context menu offers blame directly on a changed file.
    harness.get_by_label(&c2_label).click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click_secondary();
    settle(&mut harness);
    harness.get_by_label("Show blame").click();
    settle(&mut harness);
    assert!(
        harness.state().ui.blame.is_some(),
        "context menu opens blame"
    );
}
