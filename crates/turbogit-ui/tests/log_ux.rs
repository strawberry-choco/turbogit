use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable},
};
use tempfile::TempDir;
use test_support::RecordingExecutor;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{
    assert_not_painted, assert_painted, click_menu_item, painted_text, right_click_row,
    shell_harness_over,
};
use turbogit_app::root_caches::LogScope;
use turbogit_app::state::{AppState, LOG_BATCH_SIZE};
use turbogit_domain::model::RootId;
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;

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

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).unwrap();
    git_with_env(dir, &["add", "."], &GitEnv::fixture());
    git_with_env(dir, &["commit", "-q", "-m", msg], &GitEnv::fixture());
    git_with_env(dir, &["rev-parse", "HEAD"], &GitEnv::fixture())
        .trim()
        .to_string()
}

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    repo: PathBuf,
    newest: String,
    oldest: String,
}
fn seeded_project() -> Seed {
    seeded_project_with(LOG_BATCH_SIZE + 1)
}

fn seeded_project_with(n: usize) -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
    let mut stream = String::new();
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
    git_with_env(
        &repo,
        &["reset", "-q", "--hard", "main"],
        &GitEnv::fixture(),
    );
    let mut oldest = String::new();
    let mut newest = String::new();
    for i in 0..n {
        let id = git_with_env(
            &repo,
            &["rev-parse", &format!("HEAD~{}", n - 1 - i)],
            &GitEnv::fixture(),
        )
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

fn harness_over(state: AppState) -> Harness<'static, AppState> {
    let mut harness = shell_harness_over(
        state,
        egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    );
    settle(&mut harness);
    harness
}

fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    assert_eq!(state.multi.roots.len(), 1, "one root registered");
    let root_id = state.multi.roots[0].id.clone();
    state.fetch_log(root_id);
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);
    settle_until(&mut harness, &short(&seed.newest));
    harness
}

fn row_label(repo: &Path, rev: &str) -> String {
    let hash = git_with_env(repo, &["rev-parse", rev], &GitEnv::fixture());
    let subject = git_with_env(
        repo,
        &["log", "-1", "--format=%s", hash.trim()],
        &GitEnv::fixture(),
    );
    format!("{} {}", short(hash.trim()), subject.trim())
}

/// Scroll the commit list to its end with a wheel gesture: the pointer lands
/// inside the list, over a built row where there is one, or just under the
/// search box, which is above every row position the list can be scrolled to,
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

fn cached_ids(state: &AppState, root: &turbogit_domain::model::RootId) -> Vec<String> {
    state
        .caches
        .log(root)
        .unwrap_or_default()
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

fn scoped_ids(state: &AppState, root: &RootId, scope: &LogScope) -> Vec<String> {
    state
        .caches
        .scoped_log(root, scope)
        .unwrap_or_default()
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

fn scoped_len(state: &AppState, root: &RootId, scope: &LogScope) -> usize {
    state.caches.scoped_log(root, scope).map_or(0, <[_]>::len)
}

#[test]
fn commit_list_loads_batches_via_load_more() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let root_id = harness.state().multi.roots[0].id.clone();

    assert_painted(&harness, &short(&seed.newest));
    assert_not_painted(&harness, &short(&seed.oldest));
    assert_painted(&harness, &format!("{} shown", LOG_BATCH_SIZE));
    assert_painted(&harness, "Load more");
    assert!(
        !cached_ids(harness.state(), &root_id).contains(&seed.oldest),
        "the first batch must not hold the oldest commit yet"
    );

    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        s.caches.log(&root_id).map_or(0, |c| c.len()) == LOG_BATCH_SIZE + 1
    });

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

#[test]
fn a_history_of_exact_batches_stops_offering_more_without_showing_a_phantom_batch() {
    let seed = seeded_project_with(2 * LOG_BATCH_SIZE);
    let mut harness = log_harness(&seed);
    let root_id = harness.state().multi.roots[0].id.clone();

    assert_painted(&harness, &format!("{} shown", LOG_BATCH_SIZE));

    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| {
        s.caches.log(&root_id).map_or(0, |c| c.len()) == 2 * LOG_BATCH_SIZE
    });
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_painted(&harness, "Load more");

    harness.get_by_label("Load more").click();
    settle_where(&mut harness, |s| !s.caches.log_has_more(&root_id));
    assert_painted(&harness, &format!("{} shown", 2 * LOG_BATCH_SIZE));
    assert_not_painted(&harness, "Load more");

    let settled = harness.state().caches.log(&root_id).map_or(0, |c| c.len());
    settle(&mut harness);
    assert_eq!(
        harness.state().caches.log(&root_id).map_or(0, |c| c.len()),
        settled,
        "the window is stable once history has ended"
    );
}

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

#[test]
fn load_more_is_painted_inside_a_scope_while_the_scope_continues() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let root_id = harness.state().multi.roots[0].id.clone();
    assert!(
        harness.state().caches.log_has_more(&root_id),
        "the unscoped window does have more — the premise of this test"
    );
    assert_painted(&harness, "Load more");

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

struct SmallSeed {
    _tmp: TempDir,
    project: PathBuf,
    repo: PathBuf,
    c1: String,
    c2: String,
}

fn small_project() -> SmallSeed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
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

    assert_not_painted(&harness, "Path filter:");

    scope_log_to_file(
        &mut harness,
        &format!("{} alpha: first", short(&seed.c1)),
        "f.txt",
    );
    assert!(harness.state().ui.log_path_scope.is_some());

    assert_painted(&harness, "Path filter:");
    assert_painted(&harness, "alpha: first");
    assert_not_painted(&harness, "alpha: second");

    harness.get_by_label("Remove path filter").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_path_scope, None);
    assert_not_painted(&harness, "Path filter:");
}

#[test]
fn commit_search_finds_code_changes_not_just_metadata() {
    let seed = small_project();
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    harness.get_by_label("Search commits").click();
    harness.get_by_label("Search commits").type_text("one");
    settle(&mut harness);

    assert_painted(&harness, "alpha: first");
    assert_not_painted(&harness, "alpha: second");
}

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

    assert_painted(&harness, "Committer");
    assert_not_painted(&harness, "signed ✓");

    assert_not_painted(&harness, "Copy hash");
    right_click_row(&mut harness, &format!("{} alpha: second", short(&seed.c2)));
    click_menu_item(&mut harness, "Copy hash", "Copy hash");
    settle(&mut harness);
    assert_painted(&harness, "Copied");

    harness.get_by_label(&short(&seed.c1)).click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c1.as_str()),
        "clicking a parent hash must select the parent commit"
    );
}

fn ref_project() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    let origin = tmp.path().join("origin.git");
    std::fs::create_dir_all(&repo).unwrap();
    git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
    git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
    std::fs::write(repo.join("f.txt"), "one\n").unwrap();
    git_with_env(&repo, &["add", "."], &GitEnv::fixture());
    git_with_env(&repo, &["commit", "-q", "-m", "c1"], &GitEnv::fixture());
    git_with_env(&repo, &["tag", "v1.0"], &GitEnv::fixture());
    git_with_env(
        &repo,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
        &GitEnv::fixture(),
    );
    git_with_env(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    git_with_env(
        &repo,
        &["push", "-q", "-u", "origin", "main"],
        &GitEnv::fixture(),
    );
    git_with_env(&repo, &["push", "-q", "origin", "v1.0"], &GitEnv::fixture());
    git_with_env(&repo, &["tag", "v2.0"], &GitEnv::fixture());
    // Delete the branch inside the bare remote: a `git push :main` would
    // prune the local remote-tracking ref, leaving nothing to mark gone.
    git_with_env(
        &origin,
        &["update-ref", "-d", "refs/heads/main"],
        &GitEnv::fixture(),
    );
    (tmp, project, repo)
}

#[test]
fn branches_pane_marks_remote_gone_and_tag_push_states() {
    let (_tmp, project, repo) = ref_project();
    let mut state = AppState::for_roots(&project, &[repo]);
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

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

fn ref_project_with_topic() -> (TempDir, PathBuf, PathBuf, String) {
    let (tmp, project, repo) = ref_project();
    git_with_env(
        &repo,
        &["checkout", "-q", "-b", "topic"],
        &GitEnv::fixture(),
    );
    let c2 = commit_file(&repo, "g.txt", "two\n", "topic: side commit");
    git_with_env(&repo, &["checkout", "-q", "main"], &GitEnv::fixture());
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

    settle_until(&mut harness, "c1");
    assert_not_painted(&harness, "topic: side commit");

    click_button(&mut harness, "topic");
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some((root_id.clone(), "topic".to_string())),
        "row activation sets the ref scope"
    );
    assert_painted(&harness, "Ref filter:");
    settle_until(&mut harness, "topic: side commit");
    let _ = topic_commit;

    harness.get_by_label("Remove ref filter").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_ref_scope, None);
    assert_not_painted(&harness, "Ref filter:");
    assert_painted(&harness, "c1");
}

#[test]
fn switching_repository_drops_the_ref_scope() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    let beta = project.join("beta");
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    for dir in [&alpha, &beta] {
        git_with_env(dir, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(dir, &["config", "user.email", "t@t"], &GitEnv::fixture());
        git_with_env(dir, &["config", "user.name", "t"], &GitEnv::fixture());
    }
    commit_file(&alpha, "a.txt", "a\n", "alpha commit");
    commit_file(&beta, "b0.txt", "x\n", "beta root commit");
    git_with_env(
        &beta,
        &["checkout", "-q", "-b", "topic"],
        &GitEnv::fixture(),
    );
    commit_file(&beta, "b.txt", "b\n", "topic commit");
    let _beta_id = git_with_env(&beta, &["rev-parse", "HEAD"], &GitEnv::fixture())
        .trim()
        .to_string();

    let mut state = AppState::for_roots(&project, &[alpha, beta]);
    let alpha_root = state.multi.roots[0].id.clone();
    let beta_root = state.multi.roots[1].id.clone();
    for r in &state.multi.roots.clone() {
        state.fetch_log(r.id.clone());
    }
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);
    settle_until(&mut harness, "beta root commit");

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

    harness.state_mut().selected_root = Some(alpha_root);
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.log_ref_scope,
        None,
        "the scope must never outlive its repository"
    );
}
