//! log-view-scaling 03 — Load more inside a scope: the affordance, the
//! automatic trigger, the status line, and what a scope's batches cost when the
//! roots filter moves.
//!
//! `log_ux.rs` owns the in-place inversion (a path scope and a ref scope over a
//! project one batch longer than a batch). This suite owns the cases that need a
//! project shaped for them: a history long enough for a scope's SECOND batch, a
//! pickaxe term that matches more commits than one batch holds, a second root so
//! the combined listing is combined, a window tall enough for a whole scoped
//! batch to fit on screen, and a scrollbar to drag.
//!
//! The search scope is the newest member of that set and the one with a history:
//! a pickaxe batch cannot be positioned with `--skip`, which counts the
//! traversal rather than the matches, so a search that used a `skip` was torn
//! every time and could never leave its first batch. The app now asks for a
//! search from the front of its match stream and cuts the batch out of the tail,
//! and these tests are what say a term that matches more commits than one batch
//! holds is a listing that grows like the others.
//!
//! The fixture is one repository whose `f.txt` ALTERNATES a token from commit to
//! commit. That one choice buys the premises this suite needs:
//!
//! - a path scope over `f.txt` is the whole `f.txt` history, so it can be
//!   scoped to something longer than a batch;
//! - `git log -Sneedle` matches every one of those commits (the count of the
//!   term flips 1 → 0 → 1 on every commit), so the pickaxe scope also continues
//!   past a batch — while matching no message, hash or author, so what the pane
//!   shows under that term is exactly the union of the hits;
//! - `only.txt`'s single commit touches neither, so a path scope and a pickaxe
//!   scope both genuinely narrow the listing rather than restating it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Color32, Rect, Shape, vec2};
use egui_kittest::{Harness, kittest::Queryable};
use tempfile::TempDir;
use test_support::RecordingExecutor;
use test_support::harness::{click_menu_item, right_click_row};
use turbogit_app::root_caches::LogScope;
use turbogit_app::state::{AppState, LOG_BATCH_SIZE};
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_ui::theme::{configure_style, install_fonts};

/// The pickaxe term the fixture's content alternates, and which appears in no
/// commit message, hash or author name.
const TOKEN: &str = "needle";

// --- painted-output helpers (mirrors log_ux.rs) --------------------------------

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
fn assert_not_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        !texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was unexpectedly painted; painted text:\n{texts:#?}"
    );
}

#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

/// Every filled rectangle the last frame painted, as `(rect, fill)`.
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

/// Step frames until the painted output stabilizes.
fn settle(harness: &mut Harness<'_, AppState>) {
    let mut prev = String::new();
    for _ in 0..40 {
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("log layout did not settle within 40 frames");
}

#[track_caller]
fn settle_until(harness: &mut Harness<'_, AppState>, needle: &str) {
    for _ in 0..200 {
        harness.step();
        if painted_text(harness).iter().any(|t| t.contains(needle)) {
            settle(harness);
            return;
        }
    }
    panic!("`{needle}` never appeared within 200 frames");
}

/// Pump frames until the settled [`AppState`] satisfies `pred`, for a fetch
/// whose completion changes nothing paintable. Time-boxed because a batch fetch
/// is a real `git` subprocess.
#[track_caller]
fn settle_where(harness: &mut Harness<'_, AppState>, pred: impl Fn(&AppState) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        harness.step();
        if pred(harness.state()) {
            settle(harness);
            return;
        }
    }
    panic!("condition never held within 20s");
}

// --- Fixture --------------------------------------------------------------------

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

/// The row label `commit_row` publishes for the commit `n_from_head` rows below
/// HEAD: `<short hash> <subject>`. A row the list did not build has no such node,
/// which is how "that row is on screen" is asserted.
fn row_label(repo: &Path, n_from_head: usize) -> String {
    let line = git(
        repo,
        &[
            "log",
            "-1",
            "--format=%h %s",
            &format!("HEAD~{n_from_head}"),
        ],
    );
    line.trim().to_owned()
}

/// One commit in a `fast-import` stream: where it sits, what it says, what it
/// changes, and when. One value rather than seven arguments, because the seeding
/// loop below says all seven at every step.
struct Imported {
    branch: String,
    msg: String,
    from: Option<usize>,
    file: String,
    body: String,
    at: i64,
}

impl Imported {
    fn new(
        branch: &str,
        msg: impl Into<String>,
        from: Option<usize>,
        file: &str,
        body: impl Into<String>,
        at: i64,
    ) -> Self {
        Self {
            branch: branch.to_owned(),
            msg: msg.into(),
            from,
            file: file.to_owned(),
            body: body.into(),
            at,
        }
    }
}

/// Append one commit to a `fast-import` stream, changing the given file as its
/// only change. `mark` is the stream-wide mark counter fast-import requires.
fn fast_import_commit(stream: &mut String, mark: &mut usize, c: Imported) {
    let data = |stream: &mut String, s: &str| {
        stream.push_str(&format!("data {}\n{s}", s.len()));
    };
    *mark += 1;
    stream.push_str(&format!(
        "commit refs/heads/{}\nmark :{mark}\n\
         author t <t@t> {} +0000\n\
         committer t <t@t> {} +0000\n",
        c.branch, c.at, c.at
    ));
    data(stream, &c.msg);
    if let Some(parent) = c.from {
        stream.push_str(&format!("from :{parent}\n"));
    }
    stream.push_str(&format!("M 644 inline {}\n", c.file));
    data(stream, &c.body);
}

/// The subject a seeded commit carries, which carries the repository's name: the
/// same `c99` in two roots is `alpha: c99` in one and `beta: c99` in the other.
/// Assertions name whole subjects rather than bare ones, because a bare `c99` is
/// also a substring of somebody's commit hash.
fn subject(repo: &Path, what: &str) -> String {
    format!(
        "{}: {what}",
        repo.file_name()
            .and_then(|n| n.to_str())
            .expect("a repository directory name")
    )
}

/// Seed one repository with `commits` commits that each rewrite `f.txt` with a
/// token that alternates, one commit touching only `only.txt`, and a `topic`
/// branch carrying `commits` commits of its own that touch only `topic.txt`.
///
/// The three listings are shaped so each scope is testable on its own:
///
/// Every commit is salted with its repository's directory name — in its message
/// and in its blob — so two roots seeded by the same function have different
/// hashes and different subjects: a combined listing has to be able to say which
/// root a row came from.
///
/// - `main -- f.txt` is `commits` commits long, so a path scope on it continues
///   past a batch;
/// - `main -- only.txt` is one commit, so a path scope genuinely NARROWS;
/// - `topic` is `commits` commits long and shares no commit with `main`, so a ref
///   scope on it REPLACES the listing — and is itself longer than a batch;
/// - `-Sneedle` matches the `f.txt` commits and nothing else, so a pickaxe term
///   matches more than one batch holds while matching no message, hash or author:
///   what the pane shows under it is exactly the union of the hits.
fn seed_repo(dir: &Path, commits: usize) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "t@t"]);
    git(dir, &["config", "user.name", "t"]);
    // The salt is what makes two roots tellable apart: same fixture, different
    // repository, and the subjects and hashes have to differ with it.
    let salt = dir
        .file_name()
        .and_then(|n| n.to_str())
        .expect("a repository directory name")
        .to_owned();
    let mut stream = String::new();
    let mut mark = 0usize;
    let put = |stream: &mut String, mark: &mut usize, c: Imported| {
        fast_import_commit(stream, mark, c);
    };
    put(
        &mut stream,
        &mut mark,
        Imported::new(
            "main",
            format!("{salt}: c0"),
            None,
            "f.txt",
            format!("0 {salt} {TOKEN}\n"),
            1_000_000_000,
        ),
    );
    for i in 1..commits {
        // The token ALTERNATES, so its occurrence count changes in every one of
        // these commits: `-Sneedle` matches all of them, and a term that only
        // ever appeared once could not carry a scope past its first batch.
        let body = if i % 2 == 0 {
            format!("{i} {salt} {TOKEN}\n")
        } else {
            format!("{i} {salt} plain\n")
        };
        put(
            &mut stream,
            &mut mark,
            Imported::new(
                "main",
                format!("{salt}: c{i}"),
                Some(i),
                "f.txt",
                body,
                1_000_000_000 + i as i64,
            ),
        );
    }
    // One commit on main that touches nothing `f.txt` knows about.
    put(
        &mut stream,
        &mut mark,
        Imported::new(
            "main",
            format!("{salt}: only: off the path"),
            Some(commits),
            "only.txt",
            format!("off path {salt}\n"),
            1_000_000_000 + commits as i64,
        ),
    );
    // The side branch, off main's tip, of the same length and sharing no commit
    // with it.
    let base = mark;
    put(
        &mut stream,
        &mut mark,
        Imported::new(
            "topic",
            format!("{salt}: t0"),
            Some(base),
            "topic.txt",
            format!("side 0 {salt}\n"),
            1_000_000_100 + commits as i64,
        ),
    );
    for i in 1..commits {
        put(
            &mut stream,
            &mut mark,
            Imported::new(
                "topic",
                format!("{salt}: t{i}"),
                Some(base + i),
                "topic.txt",
                format!("side {i} {salt}\n"),
                1_000_000_100 + commits as i64 + i as i64,
            ),
        );
    }

    let mut child = std::process::Command::new("git")
        .arg("fast-import")
        .current_dir(dir)
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
        "git fast-import failed with {:?}; stderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    git(dir, &["reset", "-q", "--hard", "main"]);
}

/// One seeded project with two independent roots, each with `commits` commits
/// shaped by [`seed_repo`] — so the roots filter has something to narrow and the
/// combined listing is genuinely combined.
fn two_root_project(commits: usize) -> (TempDir, PathBuf, Vec<PathBuf>) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    let beta = project.join("beta");
    seed_repo(&alpha, commits);
    seed_repo(&beta, commits);
    (tmp, project, vec![alpha, beta])
}

/// One seeded project: `alpha` alone in its own project directory.
fn single_root_project(commits: usize) -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    seed_repo(&repo, commits);
    (tmp, project, repo)
}

/// The Log tool window over `state` at an explicit window size, pumping worker
/// events every frame exactly as `app.rs` does.
fn log_harness_sized(
    state: AppState,
    size: egui::Vec2,
) -> (Harness<'static, AppState>, Arc<RecordingExecutor>) {
    let recorder: Arc<RecordingExecutor> =
        Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
            settings: VcsSettings::default(),
        })));
    let mut state = state.with_executor(recorder.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(size);
    settle(&mut harness);
    (harness, recorder)
}

/// A window tall enough to build a whole scoped batch: a batch is 50 rows at the
/// log row's 30pt pitch, so 1500pt of list plus the toolbar, the micro-headers,
/// the status line and the shell's own chrome needs comfortably more than that.
const TALL: egui::Vec2 = egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 2400.0);

/// A harness over a project whose first unscoped batch has been fetched through
/// the production path, settled on the newest commit.
fn seeded_harness_sized(
    project: &Path,
    repos: &[PathBuf],
    size: egui::Vec2,
) -> (Harness<'static, AppState>, Arc<RecordingExecutor>) {
    let mut state = AppState::for_roots(project, repos);
    let roots: Vec<RootId> = state.multi.roots.iter().map(|r| r.id.clone()).collect();
    for id in roots {
        state.fetch_log(id);
    }
    let head = short(git(&repos[0], &["rev-parse", "HEAD"]).trim());
    let (mut harness, recorder) = log_harness_sized(state, size);
    settle_until(&mut harness, &head);
    (harness, recorder)
}

/// A harness at the production window size every other log suite uses.
fn seeded_harness(
    project: &Path,
    repos: &[PathBuf],
) -> (Harness<'static, AppState>, Arc<RecordingExecutor>) {
    seeded_harness_sized(
        project,
        repos,
        vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    )
}

// --- Scoped reads, from the tests' side -----------------------------------------

fn scoped_len(state: &AppState, root: &RootId, scope: &LogScope) -> usize {
    state.caches.scoped_log(root, scope).map_or(0, <[_]>::len)
}

fn has_more(state: &AppState, root: &RootId, scope: &LogScope) -> bool {
    state.caches.scoped_log_has_more(root, scope)
}

fn path_scope(file: &str) -> LogScope {
    LogScope::Path(PathBuf::from(file))
}

// --- A scope that fits its viewport never pages itself -------------------------

/// The automatic trigger is a settled bottom, and a list that FITS its viewport
/// is never at the bottom — a whole scoped batch on screen, with the gate open,
/// batches nothing.
///
/// This is the one place the two halves of the rule are checked against each
/// other rather than separately: the premise is asserted first (the scope's
/// window is a full batch, the scope says it continues, and a row forty places
/// down the list really is laid out — which it can only be if the viewport holds
/// the whole batch), and only then is the fetch count held still. Without the
/// premise, a closed gate would make the assertion vacuous.
#[test]
fn a_scope_that_fits_its_viewport_never_loads_a_second_batch() {
    // A window tall enough to build every row of a full scoped batch: 50 rows at
    // the log row's 30pt pitch is 1500pt, so the log body needs well over that.
    let (_tmp, project, repo) = single_root_project(2 * LOG_BATCH_SIZE);
    let (mut harness, recorder) = seeded_harness_sized(&project, std::slice::from_ref(&repo), TALL);
    let root_id = harness.state().multi.roots[0].id.clone();

    // Scoped to `f.txt`: 100 commits, one batch held, the gate open.
    let scope = path_scope("f.txt");
    harness.state_mut().ui.log_path_scope = Some(PathBuf::from("f.txt"));
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &scope) == LOG_BATCH_SIZE
    });
    assert!(has_more(harness.state(), &root_id, &scope));
    assert_painted(&harness, "Load more");

    // The premise that makes the next assertion mean something: the last rows of
    // the scoped batch are ON SCREEN, so the list is not overflowing and has no
    // bottom to settle at. A row 45 places down a 50-row window is a widget
    // only if the viewport is tall enough to have built it.
    assert!(
        harness.query_by_label(&row_label(&repo, 45)).is_some(),
        "the whole scoped batch fits the viewport, which is the premise here"
    );

    // Frames keep coming, the trigger is asked every one of them, and the gate
    // is open: nothing may be read.
    let held = recorder.log_call_count();
    for _ in 0..30 {
        harness.step();
    }
    assert_eq!(
        recorder.log_call_count(),
        held,
        "a scoped list that fits its viewport has no bottom to settle at, so it must \
         never page itself — the gate is open and the list does not overflow"
    );
    assert_eq!(
        scoped_len(harness.state(), &root_id, &scope),
        LOG_BATCH_SIZE,
        "and the scope still holds exactly the one batch it was given"
    );
    assert_painted(&harness, "Load more");
}

// --- The status line reports the scope's loaded window --------------------------

/// "N shown" is the scope's LOADED WINDOW in a scope, and specifically not a
/// count of the rows the list drew.
///
/// The fixture is shaped so the two candidates cannot be confused: the scope is
/// batched to 100 rows while the unscoped window behind it stays at its one batch,
/// so a line reporting the unscoped window would read "50" where the scope
/// reads "100". And at this window height the list builds only the rows in its
/// viewport, so a line counting drawn rows could not report 50 or 100 at all —
/// a row 45 places down the first batch, and later a row 60 places down the
/// second, are held by the window and have no widget on screen.
#[test]
fn the_status_line_reports_the_scopes_loaded_window_not_the_rows_drawn() {
    let (_tmp, project, repo) = single_root_project(2 * LOG_BATCH_SIZE);
    let (mut harness, _recorder) = seeded_harness(&project, std::slice::from_ref(&repo));
    let root_id = harness.state().multi.roots[0].id.clone();

    // Scoped to `f.txt`, whose history is 100 commits long. The unscoped window
    // also holds one batch here, so the two agree until the scope has paged.
    let scope = path_scope("f.txt");
    harness.state_mut().ui.log_path_scope = Some(PathBuf::from("f.txt"));
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &scope) == LOG_BATCH_SIZE
    });
    assert_painted(&harness, &format!("{LOG_BATCH_SIZE} shown"));
    assert!(
        harness.query_by_label(&row_label(&repo, 45)).is_none(),
        "the list builds only the rows in its viewport, so far fewer than 50 rows are \
         drawn — the premise that `50 shown` is a window, not a count of what is on screen"
    );

    // Page the SCOPE to its whole history. The unscoped window does not move:
    // `f.txt`'s history is longer than the unscoped window's first batch, and the
    // two are separate windows with separate flags.
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &scope) == 2 * LOG_BATCH_SIZE
    });
    assert_eq!(
        harness.state().caches.log(&root_id).map_or(0, <[_]>::len),
        LOG_BATCH_SIZE,
        "the unscoped window behind the scope is still one batch, so a line reporting \
         IT would say 50"
    );
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert!(
        harness.query_by_label(&row_label(&repo, 60)).is_none(),
        "and the second batch is mostly off screen, so `100 shown` is not a count of \
         the rows the list drew"
    );

    // The scope's history ends at 100 commits: the next batch asks past the end,
    // comes back with the anchor row alone, and the number does not move.
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| !has_more(s, &root_id, &scope));
    assert_eq!(
        scoped_len(harness.state(), &root_id, &scope),
        2 * LOG_BATCH_SIZE,
        "the scope's window is the whole of the scope's history now"
    );
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
}

// --- A scope shorter than one batch ---------------------------------------------

/// A scope whose whole history fits in one batch is exactly what it was before
/// scopes paged: one read, no affordance, and no batch asked for behind it.
///
/// Four commits touch `f.txt`, so the scoped listing is four rows — a short batch,
/// which is the answer "this listing ends here" and not a reason to ask again.
/// The read count is checked per scope rather than in total, because the harness
/// also fetches the unscoped window on the way in.
#[test]
fn a_scope_shorter_than_one_batch_reads_once_and_offers_nothing() {
    let (_tmp, project, repo) = single_root_project(4);
    let (mut harness, recorder) = seeded_harness(&project, std::slice::from_ref(&repo));
    let root_id = harness.state().multi.roots[0].id.clone();

    harness.state_mut().ui.log_path_scope = Some(PathBuf::from("f.txt"));
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &path_scope("f.txt")) == 4
    });
    let scope = path_scope("f.txt");

    // The premise, then the claim: the scope's whole history is loaded, and it
    // says so itself.
    assert_eq!(scoped_len(harness.state(), &root_id, &scope), 4);
    assert!(
        !has_more(harness.state(), &root_id, &scope),
        "a short batch IS the end of the scope's history"
    );
    let reads_of = || {
        recorder
            .log_opts()
            .iter()
            .filter(|opts| opts.path.as_deref() == Some(Path::new("f.txt")))
            .count()
    };
    assert_eq!(reads_of(), 1, "one read: the cold batch, and no second one");

    assert_not_painted(&harness, "Load more");
    // The whole scope is on the status line, and the commit that never touched
    // `f.txt` is not in it.
    assert_painted(&harness, "4 shown");
    for what in ["c0", "c1", "c2", "c3"] {
        assert_painted(&harness, &subject(&repo, what));
    }
    assert_not_painted(&harness, &subject(&repo, "only: off the path"));

    // Frames keep coming with the pane drawing; nothing may be asked for.
    for _ in 0..30 {
        harness.step();
    }
    assert_eq!(
        reads_of(),
        1,
        "a scope that ended inside its first batch must never be asked for a second one"
    );
    assert_not_painted(&harness, "Load more");
}

// --- Loaded batches survive the roots filter and re-entering the scope -----------

/// A scope's batches are held against `(root, path)`, so nothing that moves the
/// VIEW may read them again: narrowing the roots filter, widening it, and
/// leaving and re-entering the scope all have to come back to the window that
/// was already there.
///
/// The observable is the engine, not the picture. A refetch here would hand back
/// exactly the rows already on screen, so every rendering would look correct; the
/// thing that must not happen is the read. Each `log_opts` entry is counted, and
/// the cold reads of this scope (the ones with no `skip`) are counted separately
/// from its batches, because a scope that is re-read from the front is a scope
/// whose batches were thrown away.
#[test]
fn a_scopes_batches_survive_the_roots_filter_and_re_entering_it() {
    let (_tmp, project, repos) = two_root_project(2 * LOG_BATCH_SIZE);
    let (mut harness, recorder) = seeded_harness(&project, &repos);
    let alpha = harness.state().multi.roots[0].id.clone();
    let beta = harness.state().multi.roots[1].id.clone();
    let repo = repos[0].clone();
    assert_eq!(
        harness.state().selected_root.as_ref(),
        Some(&alpha),
        "the scope reads the selected root, which is alpha — the premise here"
    );
    let scope = path_scope("f.txt");
    let reads = |recorder: &RecordingExecutor| {
        recorder
            .log_opts()
            .iter()
            .filter(|opts| opts.path.as_deref() == Some(Path::new("f.txt")))
            .count()
    };
    let cold_reads = |recorder: &RecordingExecutor| {
        recorder
            .log_opts()
            .iter()
            .filter(|opts| opts.path.as_deref() == Some(Path::new("f.txt")) && opts.skip.is_none())
            .count()
    };

    // Enter the scope through the production gesture and batch it to the end of
    // `f.txt`'s history: one cold read, one batch read, 100 rows held.
    scope_history_to(&mut harness, &row_label(&repo, 1), "f.txt");
    settle_where(&mut harness, |s| {
        scoped_len(s, &alpha, &scope) == LOG_BATCH_SIZE
    });
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        scoped_len(s, &alpha, &scope) == 2 * LOG_BATCH_SIZE
    });
    assert_eq!(
        reads(&recorder),
        2,
        "the cold batch and one batch, and no more"
    );
    assert_eq!(
        cold_reads(&recorder),
        1,
        "one read from the front of the listing"
    );

    // Narrow the roots filter to this root, then widen it again. The scoped
    // window is keyed by the root the scope names, so neither move can reach it.
    harness.get_by_label("Root alpha").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_root_filter.as_ref(), Some(&alpha));
    assert_scope_untouched(
        &harness,
        &recorder,
        &alpha,
        &scope,
        "narrowing the roots filter to the scope's own root",
    );
    harness.get_by_label("All roots").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_root_filter, None);
    assert_scope_untouched(
        &harness,
        &recorder,
        &alpha,
        &scope,
        "widening the roots filter back to every root",
    );

    // Leave the scope, and come back to it through the same gesture.
    harness.get_by_label("Remove path filter").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_path_scope, None);
    scope_history_to(&mut harness, &row_label(&repo, 1), "f.txt");
    settle(&mut harness);
    assert_scope_untouched(&harness, &recorder, &alpha, &scope, "re-entering the scope");

    // The second root is a second listing throughout: its own window is still
    // untouched, and the engine was asked for exactly one batch of this scope and
    // nothing else.
    assert_eq!(
        scoped_len(harness.state(), &beta, &scope),
        0,
        "no root but the scoped one was ever read for this path"
    );
    assert_eq!(
        recorder
            .log_opts()
            .iter()
            .filter(|opts| opts.path.is_some() && opts.skip.is_some())
            .count(),
        1,
        "one batch read of the scope, which is the one Load more that was pressed"
    );
}

/// The scope's window is still the one that was batched, and the engine has not
/// been asked for it again — one helper, so all four steps of the test above
/// state the same two things.
#[track_caller]
fn assert_scope_untouched(
    harness: &Harness<'_, AppState>,
    recorder: &RecordingExecutor,
    root: &RootId,
    scope: &LogScope,
    after: &str,
) {
    assert_eq!(
        scoped_len(harness.state(), root, scope),
        2 * LOG_BATCH_SIZE,
        "{after} must not cost the scope its loaded batches"
    );
    assert_eq!(
        recorder
            .log_opts()
            .iter()
            .filter(|opts| opts.path.as_deref() == Some(Path::new("f.txt")))
            .count(),
        2,
        "{after} must not read the scope again"
    );
    assert!(
        harness
            .state()
            .caches
            .scoped_log(root, scope)
            .is_some_and(|w| w.len() == 2 * LOG_BATCH_SIZE),
        "{after} must leave the very same window in place"
    );
}

/// Drive the full user path to a file's history: select `row`, right-click its
/// changed-file entry, and activate "Show history for file…" on the shared menu
/// host. This is the only gesture that sets a path scope in production.
fn scope_history_to(harness: &mut Harness<'_, AppState>, row: &str, file: &str) {
    // The commit row is asked for by its full label, which the details pane's
    // parent link repeats once a commit is selected; the graph pane is built
    // first, so the first match is the row.
    harness
        .query_all_by_label(row)
        .next()
        .unwrap_or_else(|| panic!("no commit row labelled {row:?}"))
        .click();
    settle(harness);
    right_click_row(harness, file);
    click_menu_item(harness, "Show blame", "Show history for file…");
    settle(harness);
}

// --- Scoping still replaces or narrows the listing -----------------------------

/// The three scopes still say what they said before they paged — a path scope
/// narrows the listing to the commits touching that path, a ref scope REPLACES
/// it with that ref's history, and a pickaxe term still unions code-change hits
/// in — and each of them is checked with its own gate OPEN, so the assertions
/// are about a listing that has something to offer rather than about an idle
/// pane.
///
/// The fixture carries the evidence for each: `only.txt`'s single commit is in
/// `main` and not in `main -- f.txt`; `topic` shares no commit with `main`; and
/// `needle` is in no message, hash or author name, so under that term the visible
/// set can only be the hits.
#[test]
fn scoping_still_replaces_or_narrows_the_listing_with_the_gate_open() {
    let (_tmp, project, repo) = single_root_project(2 * LOG_BATCH_SIZE);
    let (mut harness, _recorder) = seeded_harness(&project, std::slice::from_ref(&repo));
    let root_id = harness.state().multi.roots[0].id.clone();
    assert_painted(&harness, &subject(&repo, "only: off the path"));

    // --- path scope: NARROWS. The commit that never touched `f.txt` goes, the
    // scope keeps going, and paging it brings in older `f.txt` commits.
    let path = path_scope("f.txt");
    harness.state_mut().ui.log_path_scope = Some(PathBuf::from("f.txt"));
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &path) == LOG_BATCH_SIZE
    });
    assert!(
        has_more(harness.state(), &root_id, &path),
        "the gate is open"
    );
    assert_painted(&harness, "Load more");
    assert_not_painted(&harness, &subject(&repo, "only: off the path"));
    assert_not_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &path) == 2 * LOG_BATCH_SIZE
    });
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_not_painted(&harness, &subject(&repo, "only: off the path"));

    // --- ref scope: REPLACES. `main` shares no commit with `topic`, so under a
    // ref scope not one of main's rows is left.
    harness.state_mut().ui.log_path_scope = None;
    harness.state_mut().ui.log_ref_scope = Some((root_id.clone(), "topic".to_owned()));
    let by_ref = LogScope::Ref("topic".to_owned());
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &by_ref) == LOG_BATCH_SIZE
    });
    assert!(
        has_more(harness.state(), &root_id, &by_ref),
        "`topic` is a batch longer than a batch too, so the gate is open here as well"
    );
    assert_painted(&harness, "Load more");
    assert_painted(&harness, &subject(&repo, "t99"));
    assert_not_painted(&harness, &subject(&repo, "c99"));
    assert_not_painted(&harness, &subject(&repo, "only: off the path"));
    assert_painted(&harness, &format!("{LOG_BATCH_SIZE} shown"));

    // --- pickaxe: UNIONS code-change hits. No message, hash or author contains
    // the term, so every visible row is a hit and the listing under the term is
    // the union of the hits with the unscoped window's rows.
    harness.state_mut().ui.log_ref_scope = None;
    harness.get_by_label("Search commits").click();
    harness.get_by_label("Search commits").type_text(TOKEN);
    let search = LogScope::Search(TOKEN.to_owned());
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &search) == LOG_BATCH_SIZE
    });
    assert!(
        has_more(harness.state(), &root_id, &search),
        "the pickaxe scope matches more commits than a batch holds"
    );
    assert_painted(&harness, "Load more");
    assert_painted(&harness, &format!("{LOG_BATCH_SIZE} shown"));
    assert_not_painted(&harness, &subject(&repo, "only: off the path"));
    assert_not_painted(&harness, &subject(&repo, "t99"));
    // Load more is offered, and pressing it pages BOTH windows behind this
    // listing: the unscoped union it is half of, and the search window the hits
    // come from. Nothing off-path enters the listing either way — under this
    // term the metadata filter matches nothing, so every visible row is a hit,
    // and the count is the hits.
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        s.caches.log(&root_id).map_or(0, <[_]>::len) == 2 * LOG_BATCH_SIZE
            && scoped_len(s, &root_id, &search) == 2 * LOG_BATCH_SIZE
    });
    assert_eq!(
        scoped_len(harness.state(), &root_id, &search),
        2 * LOG_BATCH_SIZE,
        "the search window is the whole match stream, so every visible row is a hit \\
         and the count is that window"
    );
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_painted(&harness, &subject(&repo, "c99"));
    assert_not_painted(&harness, &subject(&repo, "only: off the path"));
    assert_not_painted(&harness, &subject(&repo, "t99"));
}

/// A search scope batches and pages exactly as a path or a ref scope does: the
/// affordance is offered while its match stream continues, pressing it brings in
/// the next batch of hits, and the affordance goes once the stream is over.
///
/// This test used to pin the OPPOSITE — that a search is never batched, because
/// its batch could not be positioned. It can now: the app asks for a search from
/// the front of its match stream (everything held plus one batch) and cuts the
/// batch out of the tail, so the window grows by a real batch of matches.
///
/// The number the pane reports under a search term is the HITS, and that is
/// asserted here at every step rather than assumed: the term matches no message,
/// hash or author, so the live filter hides every unscoped row and the displayed
/// window is the union of the hits alone — whatever the unscoped window behind it
/// is doing.
#[test]
fn a_search_scope_batches_and_pages_like_a_path_or_ref_scope() {
    let (_tmp, project, repo) = single_root_project(2 * LOG_BATCH_SIZE);
    let (mut harness, _recorder) = seeded_harness(&project, std::slice::from_ref(&repo));
    let root_id = harness.state().multi.roots[0].id.clone();

    harness.get_by_label("Search commits").click();
    harness.get_by_label("Search commits").type_text(TOKEN);
    let search = LogScope::Search(TOKEN.to_owned());
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &search) == LOG_BATCH_SIZE
    });

    // git's own match stream, uncapped — the oracle the window is checked against.
    let matches = git(&repo, &["log", "--format=%s", &format!("-S{TOKEN}")]);
    let matches: Vec<&str> = matches.lines().collect();
    assert_eq!(
        matches.len(),
        2 * LOG_BATCH_SIZE,
        "the fixture's premise: two full batches of hits, and the traversal is \
         longer than the match stream by the one commit that touches neither file"
    );
    assert!(
        has_more(harness.state(), &root_id, &search),
        "one batch of the matches is held and more follow, so the gate is open"
    );
    assert_painted(&harness, "Load more");
    assert_painted(&harness, &format!("{LOG_BATCH_SIZE} shown"));
    assert_not_painted(&harness, &subject(&repo, matches[0]));

    // One press: the next batch of hits arrives, and the count is the hits.
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &search) == 2 * LOG_BATCH_SIZE
    });
    assert_eq!(
        harness
            .state()
            .caches
            .scoped_log(&root_id, &search)
            .map_or(0, <[_]>::len),
        2 * LOG_BATCH_SIZE,
        "the search window is the whole of the match stream now, newest first"
    );
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert!(
        harness.query_by_label(&row_label(&repo, 1)).is_some(),
        "and its oldest hit is the fixture's oldest commit, so the batch really \
         reached the end of the stream"
    );

    // The stream ends on a row limit — 100 matches is exactly what the second
    // batch asked for — so the affordance is still offered, and the batch past
    // the end is what ends it.
    assert!(has_more(harness.state(), &root_id, &search));
    assert_painted(&harness, "Load more");
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| !has_more(s, &root_id, &search));
    assert_eq!(
        scoped_len(harness.state(), &root_id, &search),
        2 * LOG_BATCH_SIZE,
        "the batch past the end brought the anchor row alone, so the window held still"
    );
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_not_painted(&harness, "Load more");
}

/// Settling at the bottom of a search listing pages it further, with nothing
/// clicked, and then stops — the automatic trigger is the list's resting
/// position, so a search reaches the end of its match stream the way a path
/// scope does.
///
/// The read count is the half that matters most here. A search batch re-reads
/// its own held prefix, so each arrival is a real read; what must never happen is
/// an unbounded number of them. While this test pinned the opposite behaviour it
/// measured 274 reads across ~120 frames of a torn-and-reread cycle, and the
/// bound below is what says that cycle cannot come back.
#[test]
fn settling_at_the_bottom_of_a_search_listing_pages_further_and_then_stops() {
    let (_tmp, project, repo) = single_root_project(2 * LOG_BATCH_SIZE);
    let (mut harness, recorder) = seeded_harness(&project, std::slice::from_ref(&repo));
    let root_id = harness.state().multi.roots[0].id.clone();

    harness.get_by_label("Search commits").click();
    harness.get_by_label("Search commits").type_text(TOKEN);
    let search = LogScope::Search(TOKEN.to_owned());
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &search) == LOG_BATCH_SIZE
    });
    let before = recorder.log_call_count();

    // One wheel to the bottom of the search listing, and no click anywhere: the
    // next batch of hits arrives on its own.
    wheel_log_to_bottom(&mut harness, &repo);
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &search) == 2 * LOG_BATCH_SIZE
    });
    assert_eq!(
        scoped_len(harness.state(), &root_id, &search),
        2 * LOG_BATCH_SIZE,
        "the search reached a second batch of hits from a wheel gesture"
    );
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert!(
        has_more(harness.state(), &root_id, &search),
        "and the match stream ends on a row limit, so the pane still believes \\
         there is one more batch — the same boundary a path scope stops at"
    );

    // The wheel has to be re-applied, because the list grew under it: that is
    // what ends the stream, exactly as it does for a path scope.
    wheel_log_to_bottom(&mut harness, &repo);
    settle_where(&mut harness, |s| !has_more(s, &root_id, &search));
    assert_eq!(
        scoped_len(harness.state(), &root_id, &search),
        2 * LOG_BATCH_SIZE,
        "the batch past the end brought the anchor row alone, so the window held \\
         still while the flag dropped"
    );
    assert_not_painted(&harness, "Load more");

    // And the bottom of a finished search is a resting place, not a trigger.
    let held = recorder.log_call_count();
    for _ in 0..40 {
        harness.step();
    }
    assert!(
        recorder.log_call_count() <= held,
        "a search resting at the bottom of its own history issued {} more reads",
        recorder.log_call_count() - held
    );
    assert!(
        recorder.log_call_count() > before,
        "and the wheels did reach the engine: a search that pages is one that reads"
    );
    assert_eq!(
        scoped_len(harness.state(), &root_id, &search),
        2 * LOG_BATCH_SIZE,
        "and the window is still the whole match stream"
    );
}

/// A multi-root project pages as the combined listing it is, the roots filter
/// still narrows what is drawn, and the root stripes are still painted.
///
/// "The combined listing" here is the unscoped union of two roots, which is what
/// a multi-root log list is, with both roots' batches moving together off one
/// press. The pickaxe union also spans roots and also batches, but it is folded
/// into the displayed rows rather than being the listing on its own — its
/// per-root behaviour is `a_search_scope_batches_and_pages_like_a_path_or_ref_scope`.
#[test]
fn a_combined_listing_batches_and_the_roots_filter_still_narrows_it() {
    let (_tmp, project, repos) = two_root_project(2 * LOG_BATCH_SIZE);
    let (mut harness, _recorder) = seeded_harness(&project, &repos);
    let (alpha_repo, beta_repo) = (&repos[0], &repos[1]);
    let alpha = harness.state().multi.roots[0].id.clone();
    let beta = harness.state().multi.roots[1].id.clone();
    let window = |s: &AppState, root: &RootId| s.caches.log(root).map_or(0, <[_]>::len);

    // Both roots' first batches are held and both say they continue past them, so
    // the union is the combined listing of two batched windows.
    assert_eq!(window(harness.state(), &alpha), LOG_BATCH_SIZE);
    assert_eq!(window(harness.state(), &beta), LOG_BATCH_SIZE);
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_painted(&harness, &subject(alpha_repo, "c99"));
    assert_painted(&harness, &subject(beta_repo, "c99"));
    assert_stripes(&harness);

    // One press pages every window behind the list: both roots' next batch, from
    // the same click, because the gate is the union of what the roots say.
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        window(s, &alpha) == 2 * LOG_BATCH_SIZE && window(s, &beta) == 2 * LOG_BATCH_SIZE
    });
    assert_painted(&harness, &format!("{} shown", 4 * LOG_BATCH_SIZE));
    assert_painted(&harness, &subject(alpha_repo, "c99"));
    assert_painted(&harness, &subject(beta_repo, "c99"));
    assert_stripes(&harness);

    // The roots filter narrows the drawn rows to the one root it names, and the
    // gate follows it: the visible root still has a batch to come, so the
    // affordance is still offered for that half alone.
    harness.get_by_label("Root alpha").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_root_filter.as_ref(), Some(&alpha));
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_painted(&harness, &subject(alpha_repo, "c99"));
    assert_not_painted(&harness, &subject(beta_repo, "c99"));
    assert!(harness.state().caches.log_has_more(&alpha));
    assert_painted(&harness, "Load more");

    // Each root's history is 101 commits, so this press brings the one commit
    // their two batches could not hold and ends both windows — the unscoped pager
    // pages every root that says it has more, not only the ones on screen. With
    // alpha the only root in view and its flag dropped, the affordance goes.
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        !s.caches.log_has_more(&alpha) && !s.caches.log_has_more(&beta)
    });
    assert_eq!(window(harness.state(), &alpha), 2 * LOG_BATCH_SIZE + 1);
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE + 1));
    assert_not_painted(&harness, "Load more");
    assert_not_painted(&harness, &subject(beta_repo, "c99"));

    // Widening brings the other root back at the batch it already held — the
    // filter changes what is drawn, not what has been read — the stripes come
    // back with it, and the combined listing has nothing left to offer.
    harness.get_by_label("All roots").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_root_filter, None);
    assert_eq!(
        window(harness.state(), &alpha) + window(harness.state(), &beta),
        2 * (2 * LOG_BATCH_SIZE + 1),
        "both roots' whole histories, 101 commits each, and neither was read twice"
    );
    assert_painted(&harness, &format!("{} shown", 2 * (2 * LOG_BATCH_SIZE + 1)));
    assert_painted(&harness, &subject(alpha_repo, "c99"));
    assert_painted(&harness, &subject(beta_repo, "c99"));
    assert_not_painted(&harness, "Load more");
    assert_stripes(&harness);
}

/// The log list's root stripes: one distinct colour per root, read as the thin
/// filled bars the rows carry, the same observation `git_log.rs` makes.
#[track_caller]
fn assert_stripes(harness: &Harness<'_, AppState>) {
    let mut colors: Vec<Color32> = filled_rects(harness)
        .into_iter()
        .filter(|(r, _)| r.width() <= 5.0 && r.height() >= 15.0)
        .map(|(_, c)| c)
        .collect();
    colors.sort_by_key(|c| c.r() as u32 * 1_000_000 + c.g() as u32 * 1_000 + c.b() as u32);
    colors.dedup();
    assert!(
        colors.len() >= 2,
        "expected a stripe per root in a combined listing, got {colors:?}"
    );
}

/// Scroll the commit list to its end with a wheel gesture, the gesture a
/// developer reaches the bottom of a long log with. The pointer lands on a
/// built row where there is one and just under the search box otherwise — above
/// every row position the list can be scrolled to — and a wheel delta far larger
/// than the list scrolls it to its last offset.
fn wheel_log_to_bottom(harness: &mut Harness<'_, AppState>, repo: &Path) {
    let head_label = row_label(repo, 0);
    let over = harness.query_by_label(&head_label).map_or_else(
        || {
            let toolbar = harness.get_by_label("Search commits").rect();
            egui::pos2(toolbar.center().x, toolbar.bottom() + 60.0)
        },
        |row| row.rect().center(),
    );
    harness.hover_at(over);
    harness.step();
    for phase in [
        egui::TouchPhase::Start,
        egui::TouchPhase::Move,
        egui::TouchPhase::End,
    ] {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -100_000.0),
            phase,
            modifiers: egui::Modifiers::default(),
        });
        harness.step();
    }
    settle(harness);
}

// --- The scrollbar route --------------------------------------------------------

/// The pointer never touches a row in this test: it grabs the commit list's OWN
/// scrollbar and drags it to its end, and a scoped list batches. That is the
/// scrollbar half of "settling at the bottom batches further automatically" — the
/// gesture is different from the wheel, the arrival at the bottom is the same
/// four numbers `settled_at_bottom` reads, and nothing in the pane can tell them
/// apart.
///
/// One gesture, more than one batch: a dragged list comes to rest at the bottom
/// of a window that is still growing, so the rule keeps asking until the scope's
/// history has ended. That is P6's own behaviour (the wheel test in `log_ux.rs`
/// is the one that pins "one arrival is one batch"), so what is asserted here is
/// the end state and the reads stopping, not the number of arrivals.
///
/// The keyboard is the one route of the three that is NOT drivable, and it is
/// not drivable because there is nothing to drive: egui's `ScrollArea` reads the
/// wheel delta and `Ui::scroll_with_delta` and nothing else, so no key is bound
/// to the log list's offset. `settled_at_bottom`'s own unit tests are what state
/// the route-agnostic half of the claim.
#[test]
fn dragging_the_scrollbar_to_the_end_loads_a_scoped_list_further() {
    let (_tmp, project, repo) = single_root_project(2 * LOG_BATCH_SIZE);
    let (mut harness, recorder) = seeded_harness(&project, std::slice::from_ref(&repo));
    let root_id = harness.state().multi.roots[0].id.clone();

    // Scoped to `f.txt`: 100 commits, one batch held, the gate open, the list
    // taller than its viewport so there is a scrollbar to drag at all.
    let scope = path_scope("f.txt");
    harness.state_mut().ui.log_path_scope = Some(PathBuf::from("f.txt"));
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &scope) == LOG_BATCH_SIZE
    });
    assert!(has_more(harness.state(), &root_id, &scope));
    assert_painted(&harness, "Load more");
    // The first row of the scoped listing — HEAD touched `only.txt`, so the
    // scope's newest row is one below HEAD.
    let first_row = harness
        .query_by_label(&row_label(&repo, 1))
        .unwrap_or_else(|| panic!("the scoped listing's first row is on screen"))
        .rect();
    // A row is laid out across the list's width, so its right edge is where the
    // list ends, and the scrollbar is the strip just inside that edge.
    let bar = first_row.right() - 6.0;

    harness.hover_at(egui::pos2(bar, first_row.top() + 2.0));
    harness.step();
    harness.drag_at(egui::pos2(bar, first_row.top() + 2.0));
    harness.step();
    harness.hover_at(egui::pos2(bar, first_row.top() + 5_000.0));
    harness.step();
    harness.drop_at(egui::pos2(bar, first_row.top() + 5_000.0));

    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &scope) == 2 * LOG_BATCH_SIZE && !has_more(s, &root_id, &scope)
    });
    assert_eq!(
        scoped_len(harness.state(), &root_id, &scope),
        2 * LOG_BATCH_SIZE,
        "the scope's whole history, paged by a scrollbar drag and nothing else"
    );
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_not_painted(&harness, "Load more");

    // The bottom of an exhausted scope is a resting place, not a trigger.
    let held = recorder.log_call_count();
    for _ in 0..30 {
        harness.step();
    }
    assert_eq!(
        recorder.log_call_count(),
        held,
        "a list resting at the bottom of a scope whose history has ended must not \
         keep reading"
    );
}
