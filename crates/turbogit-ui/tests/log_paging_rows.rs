use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use egui_kittest::{Harness, kittest::Queryable};
use test_support::RecordingExecutor;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{assert_painted, painted_text, shell_harness_over};
use turbogit_app::state::{AppState, LOG_BATCH_SIZE};
use turbogit_domain::model::VcsSettings;
use turbogit_engine::cli::CliExecutor;

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

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

fn row_label(repo: &Path, nth_from_head: usize) -> String {
    let id = git_with_env(
        repo,
        &["rev-parse", &format!("HEAD~{}", nth_from_head)],
        &GitEnv::fixture(),
    );
    let head = git_with_env(repo, &["rev-list", "--count", "HEAD"], &GitEnv::fixture());
    let total: usize = head.trim().parse().expect("commit count");
    format!("{} c{}", short(id.trim()), total - nth_from_head - 1)
}

fn seeded_project(n: usize) -> (PathBuf, PathBuf) {
    {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path().join("project");
        let repo = project.join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
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
        git_with_env(
            &repo,
            &["reset", "-q", "--hard", "main"],
            &GitEnv::fixture(),
        );
        std::mem::forget(dir);
        (project, repo)
    }
}

fn long_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| seeded_project(2 * LOG_BATCH_SIZE))
}

fn one_batch_project() -> &'static (PathBuf, PathBuf) {
    static PROJECT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    PROJECT.get_or_init(|| seeded_project(LOG_BATCH_SIZE))
}

fn log_harness_over(project: &Path, repo: &Path) -> Harness<'static, AppState> {
    let recorder: Arc<RecordingExecutor> =
        Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
            settings: VcsSettings::default(),
        })));
    let mut state = AppState::for_roots(project, std::slice::from_ref(&repo.to_path_buf()))
        .with_executor(recorder);
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = shell_harness_over(
        state,
        egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    );
    settle(&mut harness);
    let head = git_with_env(repo, &["rev-parse", "HEAD"], &GitEnv::fixture());
    settle_until(&mut harness, &short(head.trim()));
    harness
}

fn log_harness() -> Harness<'static, AppState> {
    let (project, repo) = long_project();
    log_harness_over(project, repo)
}

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

#[test]
fn only_the_rows_in_the_viewport_are_laid_out() {
    let (_project, repo) = long_project();
    let harness = log_harness();

    let top = row_label(repo.as_path(), 0);
    assert!(
        harness.query_by_label(&top).is_some(),
        "the first row of the window is in view and must exist"
    );
    let below_the_fold = row_label(repo.as_path(), 40);
    assert!(
        harness.query_by_label(&below_the_fold).is_none(),
        "a row 40 rows down was laid out — the list is not paging over its rows"
    );
    assert_painted(&harness, &format!("{LOG_BATCH_SIZE} shown"));
}

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
    assert_painted(&harness, "HASH");
    assert_painted(&harness, "MESSAGE");
}

#[test]
fn an_empty_window_says_so_outside_the_paged_rows() {
    let (project, repo) = one_batch_project();
    let mut harness = log_harness_over(project, repo);
    assert!(
        harness
            .query_by_label(&row_label(repo.as_path(), 0))
            .is_some(),
        "the list starts populated"
    );

    harness.state_mut().ui.log_filter = "no-such-commit-anywhere".to_owned();
    settle(&mut harness);
    assert_painted(&harness, "No commits matching filters");
    assert_painted(&harness, "0 shown");
}
