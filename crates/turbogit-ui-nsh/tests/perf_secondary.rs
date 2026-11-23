//! The N6 perf fixture: cold start, idle CPU, and log scroll with the
//! secondary surfaces mounted, against the egui trunk at the same window,
//! display, and fixture.
//!
//! Run with:
//! `cargo test --release -p turbogit-ui-nsh --test perf_secondary -- --ignored --nocapture --test-threads=1`
//! `N6_IDLE_SECS` tunes the idle sample (default 300, the gate's five
//! minutes).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use tempfile::TempDir;
use turbogit_app::state::{AppState, Tab};

mod support;

const COMMITS: usize = 500;
const VIEWPORT: egui::Vec2 = egui::vec2(1280.0, 820.0);

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A project with one repository, `COMMITS` commits, and a 2k line file the
/// blame surface reads.
fn fixture(tmp: &TempDir) -> (PathBuf, PathBuf, Vec<String>) {
    let project = tmp.path().join("secondary");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);

    let mut stream = String::new();
    stream.push_str("commit refs/heads/main\nmark :1\nauthor T <t@t> 1 +0000\ncommitter T <t@t> 1 +0000\ndata 5\nbase\nM 644 inline base.txt\ndata 5\nbase\n");
    let mut big = String::new();
    for line in 1..=2000 {
        big.push_str(&format!("line {line}\n"));
    }
    stream.push_str(&format!(
        "commit refs/heads/main\nmark :2\nauthor T <t@t> 2 +0000\ncommitter T <t@t> 2 +0000\ndata 4\nbig\nM 644 inline big.txt\ndata {len}\n{big}\n",
        len = big.len()
    ));
    for n in 0..COMMITS {
        let mark = n + 3;
        let parent = mark - 1;
        let subject = format!("commit {n:04}");
        let body = format!("body {n}");
        stream.push_str(&format!(
            "commit refs/heads/main\nmark :{mark}\nauthor T <t@t> {time} +0000\ncommitter T <t@t> {time} +0000\ndata {len}\n{subject}\nfrom :{parent}\nM 644 inline f.txt\ndata {body_len}\n{body}\n",
            time = n + 3,
            len = subject.len(),
            body_len = body.len(),
        ));
    }
    let mut child = Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(&repo)
        .stdin(Stdio::piped())
        .spawn()
        .expect("fast-import");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stream.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    git(&repo, &["checkout", "-q", "main"]);
    let commits: Vec<String> = git(&repo, &["rev-list", "main"])
        .lines()
        .map(|line| line.trim().to_string())
        .collect();
    (project, repo, commits)
}

fn state(project: &Path, repo: &Path, commits: &[String], surfaces: bool) -> AppState {
    let repo_path = repo.to_path_buf();
    let mut state = AppState::for_roots(project, std::slice::from_ref(&repo_path));
    state.ui.tab = Tab::Log;
    state.refresh(turbogit_app::root_caches::Affected::All);
    state.drain_events();
    let root = state.multi.roots[0].id.clone();
    if let Some(head) = commits.first() {
        state.ui.selected_commit = Some((root.clone(), head.clone()));
        state.ui.log_selected_file = Some((root.clone(), PathBuf::from("big.txt")));
        if surfaces {
            state.ui.blame = Some(turbogit_app::state::BlameTarget {
                root: root.clone(),
                path: repo.join("big.txt"),
                rev: head.clone(),
            });
            state.ui.settings_open = true;
            state.ui.settings_draft = Some(state.settings.clone());
            state.ui.shortcuts_open = true;
            state.ui.command_palette = true;
            state.ui.command_query.clear();
            state.open_bulk_preflight(turbogit_app::bulk_ops::BulkOp::Custom, &[]);
            state.ui.bulk_command = "git status --short".to_string();
        }
    }
    state
}

fn head(project: &Path, repo: &Path, commits: &[String], surfaces: bool) -> nsh_testing::Headless {
    let project = project.to_path_buf();
    let repo = repo.to_path_buf();
    let commits = commits.to_vec();
    nsh_testing::Headless::start(support::app_config(move || {
        state(&project, &repo, &commits, surfaces)
    }))
    .expect("the nsh app mounts")
}

fn trunk(
    project: &Path,
    repo: &Path,
    commits: &[String],
    surfaces: bool,
) -> Harness<'static, AppState> {
    let state = state(project, repo, commits, surfaces);
    let mut fonts = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts = true;
            }
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(VIEWPORT);
    harness
}

fn median(values: &[Duration]) -> Duration {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

fn report(label: &str, values: &[Duration]) {
    println!(
        "{label}: p50 {:.2}ms max {:.2}ms (n={})",
        median(values).as_secs_f64() * 1000.0,
        values
            .iter()
            .max()
            .copied()
            .unwrap_or_default()
            .as_secs_f64()
            * 1000.0,
        values.len(),
    );
}

#[test]
#[ignore = "manual perf harness; run with --ignored --nocapture --test-threads=1"]
fn perf_secondary_cold_start() {
    let tmp = tempfile::tempdir().unwrap();
    let (project, repo, commits) = fixture(&tmp);

    let mut head_times = Vec::new();
    let mut trunk_times = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let mut app = head(&project, &repo, &commits, false);
        app.first_render().unwrap();
        head_times.push(start.elapsed());

        let start = Instant::now();
        let mut app = trunk(&project, &repo, &commits, false);
        app.step();
        trunk_times.push(start.elapsed());
    }
    report("cold start head", &head_times);
    report("cold start trunk", &trunk_times);
    let head = median(&head_times);
    let base = median(&trunk_times);
    println!(
        "cold start rule: head {:.2}ms <= 1.5x trunk {:.2}ms = {:.2}ms",
        head.as_secs_f64() * 1000.0,
        base.as_secs_f64() * 1000.0,
        base.as_secs_f64() * 1500.0
    );
    assert!(head.as_secs_f64() <= base.as_secs_f64() * 1.5);
}

#[test]
#[ignore = "manual perf harness; run with --ignored --nocapture --test-threads=1"]
fn perf_secondary_log_scroll() {
    let tmp = tempfile::tempdir().unwrap();
    let (project, repo, commits) = fixture(&tmp);
    let mut head = head(&project, &repo, &commits, false);
    head.first_render().unwrap();
    let mut trunk = trunk(&project, &repo, &commits, false);

    for _ in 0..20 {
        head.next_frame(Duration::from_millis(16)).unwrap();
        trunk.step();
    }

    let mut head_frames = Vec::new();
    let mut trunk_frames = Vec::new();
    for n in 0..40 {
        let cid = commits[(n * 7) % commits.len()].clone();

        let _ = head.scroll("id:log:list", nsh_core::Points(180.0));
        let start = Instant::now();
        head.next_frame(Duration::from_millis(16)).unwrap();
        head_frames.push(start.elapsed());

        trunk.state_mut().ui.log_scroll_to = Some(cid);
        let start = Instant::now();
        trunk.step();
        trunk_frames.push(start.elapsed());
    }
    report("log scroll head", &head_frames);
    report("log scroll trunk", &trunk_frames);
    let head_p95 = p95(&head_frames);
    let trunk_p95 = p95(&trunk_frames);
    println!(
        "log scroll rule: head p95 {:.2}ms <= trunk p95 {:.2}ms + 2ms = {:.2}ms",
        head_p95.as_secs_f64() * 1000.0,
        trunk_p95.as_secs_f64() * 1000.0,
        (trunk_p95 + Duration::from_millis(2)).as_secs_f64() * 1000.0
    );
    assert!(head_p95 <= trunk_p95 + Duration::from_millis(2));
    assert!(head_p95 < Duration::from_micros(16_700));
}

#[test]
#[ignore = "manual perf harness; run with --ignored --nocapture --test-threads=1"]
fn perf_secondary_surfaces_open() {
    let tmp = tempfile::tempdir().unwrap();
    let (project, repo, commits) = fixture(&tmp);
    let mut head = head(&project, &repo, &commits, true);
    head.first_render().unwrap();
    let mut trunk = trunk(&project, &repo, &commits, true);

    for _ in 0..20 {
        head.next_frame(Duration::from_millis(16)).unwrap();
        trunk.step();
    }
    let mut head_frames = Vec::new();
    let mut trunk_frames = Vec::new();
    for _ in 0..40 {
        let start = Instant::now();
        head.next_frame(Duration::from_millis(16)).unwrap();
        head_frames.push(start.elapsed());
        let start = Instant::now();
        trunk.step();
        trunk_frames.push(start.elapsed());
    }
    report("surfaces open head", &head_frames);
    report("surfaces open trunk", &trunk_frames);
    let head_p95 = p95(&head_frames);
    let trunk_p95 = p95(&trunk_frames);
    println!(
        "surfaces open rule: head p95 {:.2}ms <= trunk p95 {:.2}ms + 2ms = {:.2}ms",
        head_p95.as_secs_f64() * 1000.0,
        trunk_p95.as_secs_f64() * 1000.0,
        (trunk_p95 + Duration::from_millis(2)).as_secs_f64() * 1000.0
    );
    assert!(head_p95 <= trunk_p95 + Duration::from_millis(2));
}

#[test]
#[ignore = "manual perf harness; run with --ignored --nocapture --test-threads=1"]
fn perf_secondary_idle_cpu() {
    let secs: u64 = std::env::var("N6_IDLE_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(300);
    let tmp = tempfile::tempdir().unwrap();
    let (project, repo, commits) = fixture(&tmp);
    let mut head = head(&project, &repo, &commits, false);
    head.first_render().unwrap();
    let mut trunk = trunk(&project, &repo, &commits, false);

    let head_cpu = idle_cpu(secs, || {
        head.next_frame(Duration::from_millis(16)).unwrap();
    });
    let trunk_cpu = idle_cpu(secs, || {
        trunk.step();
    });
    println!(
        "idle CPU over {secs}s: head {:.3}% trunk {:.3}%",
        head_cpu * 100.0,
        trunk_cpu * 100.0
    );
    assert!(head_cpu <= 0.01, "head idle CPU over 1 percent: {head_cpu}");
}

fn idle_cpu(secs: u64, mut frame: impl FnMut()) -> f64 {
    let before = cpu_time();
    let start = Instant::now();
    let deadline = start + Duration::from_secs(secs);
    while Instant::now() < deadline {
        frame();
        std::thread::sleep(Duration::from_millis(16));
    }
    let cpu = cpu_time() - before;
    cpu.as_secs_f64() / start.elapsed().as_secs_f64()
}

fn cpu_time() -> Duration {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `getrusage` fills the struct it is given.
    let code = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    assert_eq!(code, 0, "getrusage");
    // SAFETY: the call filled it.
    let usage = unsafe { usage.assume_init() };
    let user = Duration::new(
        usage.ru_utime.tv_sec as u64,
        usage.ru_utime.tv_usec as u32 * 1000,
    );
    let sys = Duration::new(
        usage.ru_stime.tv_sec as u64,
        usage.ru_stime.tv_usec as u32 * 1000,
    );
    user + sys
}

fn p95(values: &[Duration]) -> Duration {
    let mut sorted = values.to_vec();
    sorted.sort();
    let index = ((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    sorted[index.min(sorted.len() - 1)]
}
