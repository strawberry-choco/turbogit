//! The N4 branch-pane perf fixture, ported from the egui
//! `perf_log_branches` and `perf_log_branch_filter` suites: one repository
//! with 10k local branches, 15k remote-tracking branches, and 8k tags, then
//! p95 frame time for the nsh Branches route at 1280x820.
//!
//! The egui comparison arm retired with the crate in N7; the relative
//! baseline lives on the `egui-final` tag. Run with:
//! `cargo test --release -p turbogit-ui --test perf_branches -- --ignored --nocapture --test-threads=1`

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use turbogit_app::state::{AppState, Tab};

mod support;

const LOCAL: usize = 10_000;
const REMOTE: usize = 10_000;
const TAGS: usize = 8_000;

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
}

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

fn batch_refs(repo: &Path, lines: &str) {
    let mut child = Command::new("git")
        .args(["update-ref", "--stdin"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn update-ref");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(lines.as_bytes())
        .expect("write refs");
    assert!(child.wait().expect("update-ref").success());
}

fn seeded() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("README.md"), "x\n").expect("readme");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let sha = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();

    let mut lines = String::new();
    for n in 0..LOCAL {
        let dir = if n % 5 == 0 { "feature/deep/" } else { "" };
        lines.push_str(&format!("create refs/heads/{dir}branch-{n:04} {sha}\n"));
    }
    for n in 0..REMOTE {
        lines.push_str(&format!("create refs/remotes/origin/remote-{n:04} {sha}\n"));
        if n % 2 == 0 {
            lines.push_str(&format!("create refs/remotes/upstream/up-{n:04} {sha}\n"));
        }
    }
    for n in 0..TAGS {
        lines.push_str(&format!("create refs/tags/v{n:04} {sha}\n"));
    }
    batch_refs(&repo, &lines);

    for remote in ["origin", "upstream"] {
        let bare = tmp.path().join(format!("{remote}.git"));
        git(
            tmp.path(),
            &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
        );
        git(&repo, &["remote", "add", remote, bare.to_str().unwrap()]);
    }

    Seed { _tmp: tmp, project }
}

fn head_app(seed: &Seed) -> nsh_testing::Headless {
    let project = seed.project.clone();
    let repo = seed.project.join("alpha");
    let mut app = nsh_testing::Headless::start(support::app_config(move || {
        let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
        state.ui.tab = Tab::Branches;
        state.refresh(turbogit_app::root_caches::Affected::All);
        state.drain_events();
        state
    }))
    .expect("the nsh shell mounts");
    app.wait_until("the shell paints", Duration::from_secs(120), |screen| {
        screen.matches("id:tab:Branches").unwrap_or(false)
    })
    .expect("the first frame paints");
    app.wait_until(
        "the head branch data settles",
        Duration::from_secs(120),
        |screen| screen.texts().iter().any(|text| text.contains("LOCAL (")),
    )
    .expect("the head tree arrives");
    // The remote and tag bands sit below the 10k rows, so their labels never
    // paint in the visible window; the warm frames let their reads land and
    // the band model settle before the sample.
    for _ in 0..60 {
        app.next_frame(Duration::from_millis(16)).unwrap();
    }
    app
}

fn p95(values: &[Duration]) -> Duration {
    let mut sorted = values.to_vec();
    sorted.sort();
    let index = ((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    sorted[index.min(sorted.len() - 1)]
}

fn median(values: &[Duration]) -> Duration {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

fn report(label: &str, values: &[Duration]) {
    println!(
        "{label}: p50 {:.2}ms p95 {:.2}ms max {:.2}ms (n={})",
        median(values).as_secs_f64() * 1000.0,
        p95(values).as_secs_f64() * 1000.0,
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
fn perf_branches_frame_budget() {
    let seed = seeded();
    let mut head = head_app(&seed);

    for _ in 0..20 {
        head.next_frame(Duration::from_millis(16)).unwrap();
    }

    let frame = Duration::from_millis(16);
    let mut head_frames = Vec::new();
    for _ in 0..40 {
        let start = Instant::now();
        head.next_frame(frame).unwrap();
        head_frames.push(start.elapsed());
    }

    report("head-branches", &head_frames);

    let head_p95 = p95(&head_frames);
    println!(
        "frame budget: head p95 {:.2}ms; under 16.7ms: {}",
        head_p95.as_secs_f64() * 1000.0,
        head_p95 < Duration::from_micros(16_700),
    );
    assert!(
        head_p95 < Duration::from_micros(16_700),
        "head p95 {:?} misses the 16.7ms frame budget",
        head_p95
    );
}
