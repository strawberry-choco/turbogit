//! Issue 17 — Log UX upgrades, UI seam: Load-more batching, removable
//! path-filter chips, code-change search, upgraded commit details, and ref
//! state decorations. Headless egui_kittest harness over
//! [`turbogit_ui::ui::render`], mirroring `git_log.rs`'s painted-output
//! assertions plus public [`AppState`] transitions.
//!
//! Unlike `git_log.rs` this harness pumps worker events every frame (as
//! `app.rs` does in production) so batch fetches dispatched by the UI land
//! during `settle`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable},
};
use tempfile::TempDir;
use test_support::RecordingExecutor;
use test_support::harness::{
    assert_not_painted, assert_painted, click_menu_item, painted_text, right_click_row,
    shell_harness_over,
};
use turbogit_app::root_caches::LogScope;
use turbogit_app::state::{AppState, LOG_BATCH_SIZE};
use turbogit_domain::model::RootId;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;

// --- Painted-output helpers ----------------------------------------------------
// The painted queries are the shared `test_support::harness` ones. `settle` and the
// two pumps below are not; read the note on `settle` for why.

/// Step frames until the painted output stabilizes.
///
/// Not the shared `settle`'s 10 frames: this suite's shell starts a real `git` batch
/// fetch behind the Log window, and a frame where a worker has not posted looks
/// identical to a settled one. The shared settle has no frame-budget argument.
pub fn settle(harness: &mut Harness<'_, AppState>) {
    let mut prev = String::new();
    for _ in 0..30 {
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("log layout did not settle within 30 frames");
}

/// Step frames until `needle` is painted. Unlike [`settle`] this survives
/// stability races against in-flight worker fetches: a stable frame does
/// not mean the pending fetch has landed yet.
#[track_caller]
pub fn settle_until(harness: &mut Harness<'_, AppState>, needle: &str) {
    for _ in 0..100 {
        harness.step();
        if painted_text(harness).iter().any(|t| t.contains(needle)) {
            settle(harness);
            return;
        }
    }
    panic!("`{needle}` never appeared within 100 frames");
}

/// [`settle_until`]'s state-side sibling: pump until the settled [`AppState`]
/// satisfies `pred`, for a fetch whose completion changes nothing paintable.
/// Time-boxed rather than frame-boxed because a batch fetch is a real `git`
/// subprocess, and a kittest frame costs about a millisecond.
#[track_caller]
fn settle_where(harness: &mut Harness<'_, AppState>, pred: impl Fn(&AppState) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        harness.step();
        if pred(harness.state()) {
            settle(harness);
            return;
        }
    }
    panic!("condition never held within 15s");
}

// --- Fixture -------------------------------------------------------------------

/// A `git` runner that pins the commit identity on every invocation.
///
/// Not `test_support::git_seed::git`, which cannot express per-call env: the
/// `fast-import` stream gives every commit an explicit `1_000_000_000 + n` epoch
/// second, and pinning `GIT_AUTHOR_*`/`GIT_COMMITTER_*` keeps the `git commit`
/// subprocesses (`small_project`, `ref_project`) on the same clock.
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

/// Write `name` with `body`, stage everything, commit as `msg`, and return the
/// new HEAD's SHA.
///
/// Not `test_support::git_seed::commit`, which returns `()`: the row labels these
/// call sites look for are `"<short hash> <subject>"`, so the hash is the output.
fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    repo: PathBuf,
    /// HEAD — the first row of the first batch.
    newest: String,
    /// The root commit, `n - 1` rows below HEAD.
    oldest: String,
}
/// A single-root project with `LOG_BATCH_SIZE + 1` commits so exactly one
/// batch hides the oldest commit behind Load more.
fn seeded_project() -> Seed {
    seeded_project_with(LOG_BATCH_SIZE + 1)
}

/// A single-root project with `n` commits.
fn seeded_project_with(n: usize) -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    // Seed the history in ONE `git fast-import` batch instead of one
    // per-commit subprocess (each ~150ms under
    // Git-for-Windows; a 201-commit loop costs ~30s and starves the async
    // fetch budget in the harness). The commits are still real objects on
    // `main`, so the production `fetch_log` / `load_more_log` paths drive
    // them exactly as with a hand-seeded history.
    let mut stream = String::new();
    // Helper to emit a `data <len>\n<bytes>` block with the exact byte
    // length (multi-digit messages/files break hardcoded lengths).
    let data = |stream: &mut String, s: &str| {
        stream.push_str(&format!("data {}\n{s}", s.len()));
    };
    stream.push_str("commit refs/heads/main\n");
    stream.push_str("mark :1\n");
    stream.push_str("author t <t@t> 1000000000 +0000\n");
    stream.push_str("committer t <t@t> 1000000000 +0000\n");
    data(&mut stream, "c0\n");
    stream.push_str("M 644 inline f.txt\n");
    data(&mut stream, "body 0\n");
    let mut prev: u64 = 1;
    for i in 1..n {
        stream.push_str(&format!("commit refs/heads/main\nmark :{}\n", i + 1));
        stream.push_str(&format!("author t <t@t> {} +0000\n", 1_000_000_000 + i));
        stream.push_str(&format!("committer t <t@t> {} +0000\n", 1_000_000_000 + i));
        data(&mut stream, &format!("c{i}\n"));
        stream.push_str(&format!("from :{prev}\n"));
        stream.push_str("M 644 inline f.txt\n");
        data(&mut stream, &format!("body {i}\n"));
        prev = i as u64 + 1;
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
        "git fast-import failed with {:?}; stderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    // Point HEAD at the new main and re-read the boundary commits.
    git(&repo, &["reset", "-q", "--hard", "main"]);
    let mut oldest = String::new();
    let mut newest = String::new();
    for i in 0..n {
        let id = git(&repo, &["rev-parse", &format!("HEAD~{}", n - 1 - i)])
            .trim()
            .to_string();
        if i == 0 {
            oldest = id.clone();
        }
        if i == n - 1 {
            newest = id.clone();
        }
    }
    Seed {
        _tmp: tmp,
        project,
        repo,
        newest,
        oldest,
    }
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// Harness with the Log tool window active over `state`'s project. The log
/// is primed through the production batch-sized fetch path
/// (`AppState::fetch_log`); worker events are pumped every frame.
/// The shared shell launcher at this suite's box; the `settle` after it is still the
/// local 30-frame one.
fn harness_over(state: AppState) -> Harness<'static, AppState> {
    let mut harness = shell_harness_over(
        state,
        egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    );
    settle(&mut harness);
    harness
}

/// Harness with the Log tool window active over a single-root project. The
/// log is primed through the production batch-sized fetch path
/// (`AppState::fetch_log`); worker events are pumped every frame.
fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    assert_eq!(state.multi.roots.len(), 1, "one root registered");
    let root_id = state.multi.roots[0].id.clone();
    state.fetch_log(root_id);
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);
    // The batch fetch lands on a worker thread; await the newest commit so
    // every test starts from a settled, populated log.
    settle_until(&mut harness, &short(&seed.newest));
    harness
}

// --- Commit-log batching: the pane's affordance reads the window's flag --------

/// The row label the commit list publishes for `rev`: `<short hash> <subject>`.
/// The list only builds the rows in its viewport, so a row below the fold has no
/// label to find at all — which is why the paging tests below ask the window what
/// it holds rather than looking for a deep row on screen.
fn row_label(repo: &Path, rev: &str) -> String {
    let hash = git(repo, &["rev-parse", rev]);
    let subject = git(repo, &["log", "-1", "--format=%s", hash.trim()]);
    format!("{} {}", short(hash.trim()), subject.trim())
}

/// Scroll the commit list to its end with a wheel gesture: the pointer lands
/// inside the list — over a built row where there is one, or just under the
/// search box, which is above every row position the list can be scrolled to —
/// and a wheel delta far larger than the list scrolls it to its last offset.
/// This is the real gesture the settled-bottom rule reads: no production scroll
/// hook is involved, and the row the old version of these tests scrolled to by
/// name is never built at all.
///
/// The delta is wrapped in a touch phase because that is the one path egui
/// applies whole in a single frame; a bare mouse-wheel delta is spread over
/// several frames, and a list that batches while it is still scrolling would look
/// like two arrivals at the bottom rather than one.
fn wheel_log_to_bottom(harness: &mut Harness<'static, AppState>, repo: &Path) {
    let head_label = row_label(repo, "HEAD");
    let over = harness.query_by_label(&head_label).map_or_else(
        || {
            // Scrolled past the top row: aim under the search box, which sits
            // above every row the list can be showing.
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

/// Ids of `root`'s cached window, newest first.
fn cached_ids(state: &AppState, root: &turbogit_domain::model::RootId) -> Vec<String> {
    state
        .caches
        .log(root)
        .unwrap_or_default()
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

/// Ids of one scope's cached window in `root`, newest first.
fn scoped_ids(state: &AppState, root: &RootId, scope: &LogScope) -> Vec<String> {
    state
        .caches
        .scoped_log(root, scope)
        .unwrap_or_default()
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

/// How many rows `scope`'s window in `root` holds right now.
fn scoped_len(state: &AppState, root: &RootId, scope: &LogScope) -> usize {
    state.caches.scoped_log(root, scope).map_or(0, <[_]>::len)
}

#[test]
fn commit_list_loads_batches_via_load_more() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let root_id = harness.state().multi.roots[0].id.clone();

    // First batch: the newest commit is visible, the oldest is not, and the
    // status line + Load more affordance are painted.
    assert_painted(&harness, &short(&seed.newest));
    assert_not_painted(&harness, &short(&seed.oldest));
    assert_painted(&harness, &format!("{} shown", LOG_BATCH_SIZE));
    assert_painted(&harness, "Load more");
    // The window really does stop short of the oldest commit — the row above is
    // not painted because the first batch does not hold it (the list also builds
    // only the rows in its viewport, so "not painted" alone would not say which).
    assert!(
        !cached_ids(harness.state(), &root_id).contains(&seed.oldest),
        "the first batch must not hold the oldest commit yet"
    );

    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        s.caches.log(&root_id).map_or(0, |c| c.len()) == LOG_BATCH_SIZE + 1
    });

    // The second batch ADDED its one new row; the short batch ended the
    // history, so the affordance goes away on its own.
    assert!(
        cached_ids(harness.state(), &root_id).contains(&seed.oldest),
        "the batch brought the oldest commit into the window"
    );
    assert_painted(&harness, &format!("{} shown", LOG_BATCH_SIZE + 1));
    assert_not_painted(&harness, "Load more");
    assert!(
        !harness.state().caches.log_has_more(&root_id),
        "a short batch ends the window, so no further batch is claimed"
    );
}

/// A history of exactly two batches cannot end on a batch boundary the pager can
/// see, so it takes one more request than a shorter one — and then stops. The
/// old guess (`len() >= batch_limit`) offered a phantom third batch instead.
#[test]
fn a_history_of_exact_batches_stops_offering_more_without_showing_a_phantom_batch() {
    let seed = seeded_project_with(2 * LOG_BATCH_SIZE);
    let mut harness = log_harness(&seed);
    let root_id = harness.state().multi.roots[0].id.clone();

    assert_painted(&harness, &format!("{} shown", LOG_BATCH_SIZE));

    // Batch 2 fills its request, so the flag still says more follows.
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        s.caches.log(&root_id).map_or(0, |c| c.len()) == 2 * LOG_BATCH_SIZE
    });
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_painted(&harness, "Load more");

    // The third request brings back only the checksum row: no new rows appear,
    // and the affordance is gone rather than waiting for another click. The
    // flag — not the painted rows — is what says the batch has landed.
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| !s.caches.log_has_more(&root_id));
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_not_painted(&harness, "Load more");

    // And it stays gone: no further batch is claimed for the pane to chase.
    let settled = harness.state().caches.log(&root_id).map_or(0, |c| c.len());
    settle(&mut harness);
    assert_eq!(
        harness.state().caches.log(&root_id).map_or(0, |c| c.len()),
        settled,
        "the window is stable once history has ended"
    );
}

/// Reaching the bottom of the list loads the next batch with nothing clicked
/// (plan P6), and stops the moment history does. The engine is a recording
/// wrapper so the fetch count is checkable: an arrival at the bottom is one
/// batch, not a loop.
///
/// The scroll is a wheel gesture to the end of the list rather than a scroll to
/// a named row: the list pages over its rows (log-view-scaling 05), so the
/// last row of a 50-row window is not a widget until the list is scrolled to it
/// — which is the very thing being driven here. Every assertion is the one the
/// named-row version made: the next batch arrives on its own, one batch per
/// arrival, and the pane holds still once the history has ended.
#[test]
fn resting_at_the_bottom_loads_batches_until_history_ends() {
    let seed = seeded_project_with(2 * LOG_BATCH_SIZE);
    let recorder: Arc<RecordingExecutor> =
        Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
            settings: VcsSettings::default(),
        })));
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo))
        .with_executor(recorder.clone());
    let root_id = state.multi.roots[0].id.clone();
    state.fetch_log(root_id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);
    settle_until(&mut harness, &short(&seed.newest));
    assert_painted(&harness, &format!("{} shown", LOG_BATCH_SIZE));

    // Scrolling the 50 held rows to their end IS the bottom of the list, and no
    // click is involved. (It is the 50th row because HEAD is `c99`.)
    wheel_log_to_bottom(&mut harness, &seed.repo);
    settle_where(&mut harness, |s| {
        s.caches.log(&root_id).map_or(0, |c| c.len()) == 2 * LOG_BATCH_SIZE
    });
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_eq!(
        recorder.log_call_count(),
        2,
        "one arrival at the bottom is one batch"
    );

    // Still at the bottom after the batch landed — the list grew, so the wheel has
    // to be re-applied — the pane asks once more (the batch ends exactly on a row
    // limit) and then holds still.
    wheel_log_to_bottom(&mut harness, &seed.repo);
    settle_where(&mut harness, |s| !s.caches.log_has_more(&root_id));
    let settled = recorder.log_call_count();
    for _ in 0..20 {
        harness.step();
    }
    assert_eq!(
        recorder.log_call_count(),
        settled,
        "a settled bottom at the end of history must not re-issue fetches"
    );
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
}

/// The same automatic trigger, inside a scope (log-view-scaling 03): a path
/// scope over a file every commit touches is the whole 100-commit history, it
/// is batched the way the unscoped listing above is batched, and it holds still
/// once its own history has ended.
///
/// The wheel is the one gesture this harness can drive end-to-end. That is not
/// a narrower claim than it looks: the trigger is not a gesture, it is a
/// resting position ([`settled_at_bottom`] takes the offset the list came to
/// rest at and nothing else), so the scrollbar and the keyboard arrive at the
/// same branch through the same four numbers. What the harness cannot drive is
/// stated rather than faked: a key press does not move this list's offset —
/// `ScrollArea` reads the wheel delta and `scroll_with_delta` only, so there is
/// no key bound to the log list at all — and the scrollbar is a separate leg
/// in `log_scope_batching.rs`, which drags it.
#[test]
fn resting_at_the_bottom_of_a_scoped_list_loads_further_batches() {
    let seed = seeded_project_with(2 * LOG_BATCH_SIZE);
    let recorder: Arc<RecordingExecutor> =
        Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
            settings: VcsSettings::default(),
        })));
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo))
        .with_executor(recorder.clone());
    let root_id = state.multi.roots[0].id.clone();
    state.fetch_log(root_id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);
    settle_until(&mut harness, &short(&seed.newest));

    // Enter the scope. The cold read is the ONE batch `ensure_log` fills, and
    // the scope is 100 commits long, so it genuinely has a second batch to give.
    let scope = LogScope::Path(PathBuf::from("f.txt"));
    harness.state_mut().ui.log_path_scope = Some(PathBuf::from("f.txt"));
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &scope) == LOG_BATCH_SIZE
    });
    assert!(
        harness.state().caches.scoped_log_has_more(&root_id, &scope),
        "the scope holds one of its two batches — the premise of this test"
    );
    let after_cold_read = recorder.log_call_count();

    // Settle at the bottom of the SCOPED list: one wheel, no click. The next
    // batch is the scope's, not the unscoped union's.
    wheel_log_to_bottom(&mut harness, &seed.repo);
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &scope) == 2 * LOG_BATCH_SIZE
    });
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_eq!(
        recorder.log_call_count(),
        after_cold_read + 1,
        "one arrival at the bottom is one batch"
    );
    assert_eq!(
        harness.state().caches.log(&root_id).map_or(0, <[_]>::len),
        LOG_BATCH_SIZE,
        "and the unscoped window behind the scope did not grow"
    );

    // The scope's whole history is 100 commits, so the second arrival asks for a
    // batch past the end, gets the anchor row alone, and the scope stops: the
    // window holds still and no further read is issued.
    wheel_log_to_bottom(&mut harness, &seed.repo);
    settle_where(&mut harness, |s| {
        !s.caches.scoped_log_has_more(&root_id, &scope)
    });
    let settled = recorder.log_call_count();
    for _ in 0..20 {
        harness.step();
    }
    assert_eq!(
        recorder.log_call_count(),
        settled,
        "a settled bottom at the end of the scope's history must not re-issue fetches"
    );
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_not_painted(&harness, "Load more");
}

/// A scoped listing batches like the unscoped one (log-view-scaling 03), so Load
/// more is painted INSIDE a scope while that scope's history continues past the
/// window it holds, and goes away on its own once the scope is exhausted.
///
/// This is the inversion of the rule that stood here before: a scope was
/// fetched whole, so it had nothing more to offer and the affordance was
/// suppressed. Scopes batch now, so the suppression's premise is gone.
///
/// The scope genuinely continues past one batch, and each leg says so before it
/// asserts anything about the affordance: `seeded_project()` has
/// `LOG_BATCH_SIZE + 1` commits, every one of them touches `f.txt`, and `main`
/// is the only branch — so the path scope and the ref scope each hold a FULL
/// first batch and still have one commit past it. A fixture that ever stopped
/// being long enough would come back with a short window, and the premise
/// assertions would fail instead of quietly making the affirmation vacuous.
#[test]
fn load_more_is_painted_inside_a_scope_while_the_scope_continues() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let root_id = harness.state().multi.roots[0].id.clone();
    // The unscoped window agrees and the affordance is up, so the legs below
    // are looking at the same list before and after it is scoped — and what
    // changes is the scope, not the button.
    assert!(
        harness.state().caches.log_has_more(&root_id),
        "the unscoped window does have more — the premise of this test"
    );
    assert_painted(&harness, "Load more");

    // --- path scope. `f.txt` is touched by every seeded commit, so the scoped
    // listing is the whole 51-commit history: one batch held, one commit to ask
    // for. The affordance is offered.
    let path = LogScope::Path(PathBuf::from("f.txt"));
    harness.state_mut().ui.log_path_scope = Some(PathBuf::from("f.txt"));
    settle(&mut harness);
    assert_eq!(
        scoped_len(harness.state(), &root_id, &path),
        LOG_BATCH_SIZE,
        "the scope's first batch is the full batch, not a short one"
    );
    assert!(
        harness.state().caches.scoped_log_has_more(&root_id, &path),
        "and the scope's history continues past it — the premise of this leg"
    );
    assert!(
        !scoped_ids(harness.state(), &root_id, &path).contains(&seed.oldest),
        "the commit past the batch is genuinely not in the window yet"
    );
    assert_painted(&harness, "Load more");
    assert_painted(&harness, &format!("{LOG_BATCH_SIZE} shown"));

    // Pressing it batches the SCOPE, not the unscoped union: the commit the
    // first batch could not hold arrives, and the scope — which now holds the
    // whole of its history — says so, which is what takes the affordance away.
    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &path) == LOG_BATCH_SIZE + 1
    });
    assert!(
        scoped_ids(harness.state(), &root_id, &path).contains(&seed.oldest),
        "the batch brought the oldest commit into the scope's window"
    );
    assert!(
        !harness.state().caches.scoped_log_has_more(&root_id, &path),
        "a short batch ends the scope, so no further batch is claimed"
    );
    assert_painted(&harness, &format!("{} shown", LOG_BATCH_SIZE + 1));
    assert_not_painted(&harness, "Load more");
    // The unscoped window was NOT batched by any of that: one scope, one read.
    assert!(
        harness
            .state()
            .caches
            .scoped_log(&root_id, &path)
            .is_some_and(|w| w.len() == LOG_BATCH_SIZE + 1),
        "the scope's window is the one that grew"
    );
    assert_eq!(
        harness.state().caches.log(&root_id).map_or(0, <[_]>::len),
        LOG_BATCH_SIZE,
        "the unscoped window behind it is untouched"
    );

    // --- ref scope. The same listing, reached as `main`'s history rather than
    // `f.txt`'s, with the same premise and the same offer.
    harness.state_mut().ui.log_path_scope = None;
    harness.state_mut().ui.log_ref_scope = Some((root_id.clone(), "main".to_owned()));
    settle(&mut harness);
    let by_ref = LogScope::Ref("main".to_owned());
    assert_eq!(
        scoped_len(harness.state(), &root_id, &by_ref),
        LOG_BATCH_SIZE,
        "the ref scope's first batch is the full batch too"
    );
    assert!(
        harness
            .state()
            .caches
            .scoped_log_has_more(&root_id, &by_ref),
        "and `main` continues past it — the premise of this leg"
    );
    assert_painted(&harness, "Load more");

    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        scoped_len(s, &root_id, &by_ref) == LOG_BATCH_SIZE + 1
    });
    assert_painted(&harness, &format!("{} shown", LOG_BATCH_SIZE + 1));
    assert_not_painted(&harness, "Load more");
}

// --- Issue 17: removable path-filter chip ---------------------------------------

/// A quick two-commit single-root project: `c1` touches `f.txt`, `c2`
/// touches `g.txt` — scoping to `f.txt` must hide `c2`.
struct SmallSeed {
    _tmp: TempDir,
    project: PathBuf,
    repo: PathBuf,
    /// The commit touching `f.txt` — the only one visible once scoped.
    c1: String,
    /// The commit touching `g.txt`, named by the row label those tests drive with.
    c2: String,
}

fn small_project() -> SmallSeed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    let c1 = commit_file(&repo, "f.txt", "one\n", "alpha: first");
    let c2 = commit_file(&repo, "g.txt", "two\n", "alpha: second");
    SmallSeed {
        _tmp: tmp,
        project,
        repo,
        c1,
        c2,
    }
}

/// Drive the full user path: select `row_label`, right-click its changed-file
/// entry, and activate "Show history for file…" on the shared menu host.
fn scope_log_to_file(harness: &mut Harness<'_, AppState>, row_label: &str, file: &str) {
    harness.get_by_label(row_label).click();
    settle(harness);
    right_click_row(harness, file);
    click_menu_item(harness, "Show blame", "Show history for file…");
    settle(harness);
}

#[test]
fn path_scope_renders_as_a_removable_chip() {
    let seed = small_project();
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    // No chip while unscoped.
    assert_not_painted(&harness, "Path filter:");

    // Scope via the changed-files context menu…
    scope_log_to_file(
        &mut harness,
        &format!("{} alpha: first", short(&seed.c1)),
        "f.txt",
    );
    assert!(harness.state().ui.log_path_scope.is_some());

    // …the scope renders as a chip in the log header.
    assert_painted(&harness, "Path filter:");
    // Only the scoped commit's rows remain.
    assert_painted(&harness, "alpha: first");
    assert_not_painted(&harness, "alpha: second");

    // The chip's × removes the scope; the full log returns.
    harness.get_by_label("Remove path filter").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_path_scope, None);
    assert_not_painted(&harness, "Path filter:");
}

// --- Issue 17: search covers code change -----------------------------------------

#[test]
fn commit_search_finds_code_changes_not_just_metadata() {
    let seed = small_project();
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    // Searching a string that only appears in a commit's *content* ("one"
    // is f.txt's body — no message, hash, or author contains it) must
    // surface that commit via the pickaxe, and hide the one that never
    // touched the string.
    harness.get_by_label("Search commits").click();
    harness.get_by_label("Search commits").type_text("one");
    settle(&mut harness);

    assert_painted(&harness, "alpha: first");
    assert_not_painted(&harness, "alpha: second");
}

// --- Issue 17: commit details — committer, signature, parents, copy hash ----

#[test]
fn details_show_committer_copy_hash_and_clickable_parents() {
    let seed = small_project();
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    harness
        .get_by_label(&format!("{} alpha: second", short(&seed.c2)))
        .click();
    settle(&mut harness);

    // Committer row (screen 09) — and an unsigned commit must not claim a
    // signature. The redesign's meta grid labels without the trailing colon.
    assert_painted(&harness, "Committer");
    assert_not_painted(&harness, "signed ✓");

    // Copy hash is the commit menu's, not the pane's (ADR-0024): the hash is
    // still shown, and clicking it is what moved.
    assert_not_painted(&harness, "Copy hash");
    right_click_row(&mut harness, &format!("{} alpha: second", short(&seed.c2)));
    click_menu_item(&mut harness, "Copy hash", "Copy hash");
    settle(&mut harness);
    assert_painted(&harness, "Copied");

    // Parent hashes are links: clicking one selects that commit.
    harness.get_by_label(&short(&seed.c1)).click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c1.as_str()),
        "clicking a parent hash must select the parent commit"
    );
}

// --- Issue 17: remote gone + tag pushed states in the branches pane -------------

/// A repo with a bare `origin`: `main` was pushed (and then deleted on the
/// remote without a local prune), tag `v1.0` was pushed, `v2.0` never left
/// the machine.
fn ref_project() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    let origin = tmp.path().join("origin.git");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("f.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    git(&repo, &["tag", "v1.0"]);
    git(
        &repo,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    git(&repo, &["push", "-q", "origin", "v1.0"]);
    git(&repo, &["tag", "v2.0"]);
    // Delete the branch inside the bare remote: a `git push :main` would
    // prune the local remote-tracking ref, leaving nothing to mark gone.
    git(&origin, &["update-ref", "-d", "refs/heads/main"]);
    (tmp, project, repo)
}

#[test]
fn branches_pane_marks_remote_gone_and_tag_push_states() {
    let (_tmp, project, repo) = ref_project();
    let mut state = AppState::for_roots(&project, &[repo]);
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    // The pane is now the shared repo-grouped tree (branch-tree extraction,
    // plan step 4): remote branches group under their `origin` header with
    // the prefix stripped, still carrying the gone marker; the TAGS rows
    // carry their push state (screen 09) via the view model's TagLeaf.
    // Tags start collapsed like in the Branches window — open the group.
    // Remote groups start collapsed too (the Log pane's default) — open `origin`.
    harness.get_by_label("Tags").click();
    click_button(&mut harness, "origin");
    settle(&mut harness);
    assert_painted(&harness, "origin");
    assert_painted(&harness, "gone");
    assert_painted(&harness, "v1.0");
    assert_painted(&harness, "pushed");
    assert_painted(&harness, "v2.0");
    assert_painted(&harness, "local only");
}

// --- Branch-tree extraction (plan step 4): ref-scoped graph filtering --------

/// Click the `Role::Button` labelled exactly `label`. Branch rows carry their
/// name twice in the a11y tree (the row's label and the inner text's value),
/// so a bare `get_by_label` would be ambiguous.
#[track_caller]
fn click_button(harness: &mut Harness<'_, AppState>, label: &str) {
    let node = harness
        .query_all_by_label_contains(label)
        .find(|n| {
            n.accesskit_node().role() == egui::accesskit::Role::Button
                && n.accesskit_node().label().is_some_and(|l| l == label)
        })
        .unwrap_or_else(|| panic!("no button labelled {label:?}"));
    node.click();
}

/// Extend [`ref_project`] with a second branch: `topic` carries one commit
/// `main` does not, so scoping the graph to `main` must hide it.
fn ref_project_with_topic() -> (TempDir, PathBuf, PathBuf, String) {
    let (tmp, project, repo) = ref_project();
    git(&repo, &["checkout", "-q", "-b", "topic"]);
    let c2 = commit_file(&repo, "g.txt", "two\n", "topic: side commit");
    git(&repo, &["checkout", "-q", "main"]);
    (tmp, project, repo, c2)
}

#[test]
fn activating_a_ref_scopes_the_graph_to_its_history() {
    let (_tmp, project, repo, topic_commit) = ref_project_with_topic();
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root_id = state.multi.roots[0].id.clone();
    state.fetch_log(root_id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    // Unscoped: main's fetched batch — the side commit lives only on topic.
    settle_until(&mut harness, "c1");
    assert_not_painted(&harness, "topic: side commit");

    // Activating the `topic` row in the branches pane scopes the graph.
    click_button(&mut harness, "topic");
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some((root_id.clone(), "topic".to_string())),
        "row activation sets the ref scope"
    );
    // The scope renders as a removable chip, same gesture as the path scope.
    assert_painted(&harness, "Ref filter:");
    // The graph narrows to the ref's history: the scoped listing fetches
    // through the engine seam and surfaces the side commit.
    settle_until(&mut harness, "topic: side commit");
    let _ = topic_commit;

    // The chip's × clears the scope; the main batch returns.
    harness.get_by_label("Remove ref filter").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_ref_scope, None);
    assert_not_painted(&harness, "Ref filter:");
    assert_painted(&harness, "c1");
}

#[test]
fn switching_repository_drops_the_ref_scope() {
    // Two repos: activating beta's `topic` scopes the graph, then switching
    // the selected repository to alpha must clear it (plan D9).
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    let beta = project.join("beta");
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    for dir in [&alpha, &beta] {
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.email", "t@t"]);
        git(dir, &["config", "user.name", "t"]);
    }
    commit_file(&alpha, "a.txt", "a\n", "alpha commit");
    commit_file(&beta, "b0.txt", "x\n", "beta root commit");
    git(&beta, &["checkout", "-q", "-b", "topic"]);
    commit_file(&beta, "b.txt", "b\n", "topic commit");
    let _beta_id = git(&beta, &["rev-parse", "HEAD"]).trim().to_string();

    let mut state = AppState::for_roots(&project, &[alpha, beta]);
    let alpha_root = state.multi.roots[0].id.clone();
    let beta_root = state.multi.roots[1].id.clone();
    for r in &state.multi.roots.clone() {
        state.fetch_log(r.id.clone());
    }
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);
    settle_until(&mut harness, "beta root commit");

    // Activate beta's `topic` row: the scope sets and follows the repo.
    click_button(&mut harness, "topic");
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some((beta_root.clone(), "topic".to_string()))
    );
    assert_eq!(
        harness.state().selected_root.as_ref(),
        Some(&beta_root),
        "the selection follows the scoped ref's repository"
    );

    // Switching the selected repository drops the scope.
    harness.state_mut().selected_root = Some(alpha_root);
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.log_ref_scope,
        None,
        "the scope must never outlive its repository"
    );
}
