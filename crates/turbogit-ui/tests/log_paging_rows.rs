//! Log-view-scaling 05 — the commit list lays out only the rows in its
//! viewport.
//!
//! The observable is which commit rows exist as widgets: a row's
//! [`egui::WidgetInfo`] label is the one handle every other log test already
//! drives rows by, and a row the list did not build has no label to find. So
//! "the row below the fold was not laid out" is a statement about the
//! accessibility tree, not about a private field.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use egui_kittest::{Harness, kittest::Queryable};
use test_support::RecordingExecutor;
use test_support::harness::{assert_painted, painted_text, shell_harness_over};
use turbogit_app::state::{AppState, LOG_BATCH_SIZE};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;

// --- painted-output helpers (mirrors log_ux.rs) -------------------------------

/// Step frames until the painted output stabilizes.
///
/// Not `test_support::harness::settle`'s 10 frames: the batch fetch behind the Log
/// window is a real `git` subprocess, and a frame taken before the worker posts is
/// byte-identical to a settled one. The shared settle has no frame-budget argument.
fn settle(harness: &mut Harness<'_, AppState>) {
    let mut prev = String::new();
    for _ in 0..30 {
        harness.step();
        if format!("{:?}", painted_text(harness)) == prev {
            return;
        }
        prev = format!("{:?}", painted_text(harness));
    }
    panic!("log layout did not settle within 30 frames");
}

#[track_caller]
fn settle_until(harness: &mut Harness<'_, AppState>, needle: &str) {
    for _ in 0..100 {
        harness.step();
        if painted_text(harness).iter().any(|t| t.contains(needle)) {
            settle(harness);
            return;
        }
    }
    panic!("`{needle}` never appeared within 100 frames");
}

// --- Fixture: one long history, seeded once for the whole binary ---------------

/// A `git` runner that pins the commit identity on every invocation.
///
/// Not `test_support::git_seed::git`, which takes no per-call env: the `rev-parse`
/// calls naming the head row run through this runner too, so handing them to the
/// shared one would commit as whatever the developer's global config names.
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

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// The row label `commit_row` publishes: `<short hash> <subject>`.
fn row_label(repo: &Path, nth_from_head: usize) -> String {
    let id = git(repo, &["rev-parse", &format!("HEAD~{}", nth_from_head)]);
    let head = git(repo, &["rev-list", "--count", "HEAD"]);
    let total: usize = head.trim().parse().expect("commit count");
    format!("{} c{}", short(id.trim()), total - nth_from_head - 1)
}

/// Seed a project with `n` commits on `main` in one `fast-import` batch — one
/// subprocess per commit would cost minutes across this suite.
fn seeded_project(n: usize) -> (PathBuf, PathBuf) {
    {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let repo = project.join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "t"]);
        let mut stream = String::new();
        let data = |stream: &mut String, s: &str| {
            stream.push_str(&format!("data {}\n{s}", s.len()));
        };
        stream.push_str("commit refs/heads/main\nmark :1\n");
        stream.push_str("author t <t@t> 1000000000 +0000\n");
        stream.push_str("committer t <t@t> 1000000000 +0000\n");
        data(&mut stream, "c0\n");
        stream.push_str("M 644 inline f.txt\n");
        data(&mut stream, "body 0\n");
        for i in 1..n {
            stream.push_str(&format!("commit refs/heads/main\nmark :{}\n", i + 1));
            stream.push_str(&format!("author t <t@t> {} +0000\n", 1_000_000_000 + i));
            stream.push_str(&format!("committer t <t@t> {} +0000\n", 1_000_000_000 + i));
            data(&mut stream, &format!("c{i}\n"));
            stream.push_str(&format!("from :{}\n", i));
            stream.push_str("M 644 inline f.txt\n");
            data(&mut stream, &format!("body {i}\n"));
        }
        let mut child = std::process::Command::new("git")
            .arg("fast-import")
            .current_dir(&repo)
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn git fast-import");
        use std::io::Write as _;
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(stream.as_bytes())
            .expect("write fast-import stream");
        let out = child.wait_with_output().expect("wait fast-import");
        assert!(
            out.status.success(),
            "git fast-import failed with {:?}",
            out.status
        );
        git(&repo, &["reset", "-q", "--hard", "main"]);
        // The temp dir outlives every test in this binary.
        std::mem::forget(dir);
        (project, repo)
    }
}

/// Two full batches of history — the list is asked to batch, and the window is
/// twice the height of anything the viewport can show.
fn long_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| seeded_project(2 * LOG_BATCH_SIZE))
}

/// Exactly one batch of history. The list's end is the window's end here, so
/// "scrolled to the end" has an unambiguous meaning: arriving at the bottom
/// batches once more (the row limit cannot tell a full batch from the end) and
/// then holds, without the window growing out from under the assertion.
fn one_batch_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| seeded_project(LOG_BATCH_SIZE))
}

/// Harness over the long project with the first batch fetched through the
/// production path and worker events pumped every frame.
fn log_harness_over(project: &Path, repo: &Path) -> Harness<'static, AppState> {
    let recorder: Arc<RecordingExecutor> =
        Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
            settings: VcsSettings::default(),
        })));
    let mut state = AppState::for_roots(project, std::slice::from_ref(&repo.to_path_buf()))
        .with_executor(recorder);
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    // The shared shell launcher at this suite's box; the `settle` after it is still
    // the local 30-frame one.
    let mut harness = shell_harness_over(
        state,
        egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    );
    settle(&mut harness);
    // The head commit's short hash is one painted galley; the full row label is
    // an accessibility label, not painted text, so the wait is on the hash.
    let head = git(repo, &["rev-parse", "HEAD"]);
    settle_until(&mut harness, &short(head.trim()));
    harness
}

fn log_harness() -> Harness<'static, AppState> {
    let (project, repo) = long_project();
    log_harness_over(project, repo)
}

/// Scroll the commit list to its end with a wheel gesture over a row, the way
/// a developer reaches the bottom of a long log: the pointer lands on a built
/// row, then a wheel delta far larger than the list scrolls it to its last
/// offset. No production scroll hook is involved — this is the real gesture the
/// settled-bottom rule reads.
fn wheel_to_bottom(harness: &mut Harness<'static, AppState>, repo: &Path) {
    let over_a_row = harness.get_by_label(&row_label(repo, 0)).rect().center();
    harness.hover_at(over_a_row);
    harness.step();
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -100_000.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::default(),
    });
    settle(harness);
}

// --- The list batches over the row count it is given ---------------------------

/// The list is given the whole window's row count and lays out only the rows in
/// its viewport: the top row exists, a row well below the fold does not. The
/// count the pane reports is the whole window's either way, so "N shown" is not
/// the number of rows the list built.
#[test]
fn only_the_rows_in_the_viewport_are_laid_out() {
    let (_project, repo) = long_project();
    let harness = log_harness();

    let top = row_label(repo.as_path(), 0);
    assert!(
        harness.query_by_label(&top).is_some(),
        "the first row of the window is in view and must exist"
    );
    // Row 40 is far below an 800px-tall list: it exists only if the list
    // builds every row it holds.
    let below_the_fold = row_label(repo.as_path(), 40);
    assert!(
        harness.query_by_label(&below_the_fold).is_none(),
        "a row 40 rows down was laid out — the list is not paging over its rows"
    );
    // The whole window is still what the pane reports.
    assert_painted(&harness, &format!("{LOG_BATCH_SIZE} shown"));
}

/// The row count reaches the scroll area, not just the label: the oldest loaded
/// row is unreachable from the top, and a wheel to the end of the list brings it
/// into view. Were the count wrong, the list would stop short of its own last
/// row and no amount of scrolling would ever show it.
#[test]
fn the_last_row_of_the_window_is_reachable_by_scrolling() {
    let (project, repo) = one_batch_project();
    let mut harness = log_harness_over(project, repo);

    let oldest = row_label(repo.as_path(), LOG_BATCH_SIZE - 1);
    assert!(
        harness.query_by_label(&oldest).is_none(),
        "the oldest row starts below the fold, which is the premise"
    );
    wheel_to_bottom(&mut harness, repo);
    assert!(
        harness.query_by_label(&oldest).is_some(),
        "scrolling to the end of the list must reach the last row it holds"
    );
    // The micro-headers are painted outside the scroll area, so they are still
    // there once the list is scrolled to its end.
    assert_painted(&harness, "HASH");
    assert_painted(&harness, "MESSAGE");
}

/// A log that matches no commits still says so. The message is the one
/// variable-height thing in the list, so it is drawn outside the paged rows: an
/// empty result must not have to claim a row slot it does not have, and it stays
/// where the pane has always shown it instead of scrolling away with a list that
/// has nothing in it.
#[test]
fn an_empty_window_says_so_outside_the_paged_rows() {
    let (project, repo) = one_batch_project();
    let mut harness = log_harness_over(project, repo);
    // The premise: the list has rows to begin with. (A row's full label is one
    // accessibility label, not one painted galley, so it is checked as a node.)
    assert!(
        harness
            .query_by_label(&row_label(repo.as_path(), 0))
            .is_some(),
        "the list starts populated"
    );

    // A search term nothing matches: the window is empty.
    harness.state_mut().ui.log_filter = "no-such-commit-anywhere".to_owned();
    settle(&mut harness);
    assert_painted(&harness, "No commits match.");
    // And it is not one of the list's rows: the list was handed an empty row
    // count, so it cannot have laid the message out as one.
    assert_painted(&harness, "0 shown");
}
