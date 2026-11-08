//! Manual perf harness for the log view's branches pane (the branch filter).
//!
//! Run it with:
//! `cargo test --release -p turbogit-ui --test perf_log_branches -- --ignored --nocapture --test-threads=1`
//!
//! It seeds one repository with 10k local branches, 15k remote-tracking
//! branches and 8k tags, then times the pane's frames: opening, idle scrolling,
//! expanding a collapsed remote, expanding the tags group, and filtering.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use tempfile::TempDir;
use turbogit_app::state::{AppState, Tab};

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
    let status = child.wait().expect("update-ref");
    assert!(status.success(), "update-ref --stdin failed");
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

fn report(label: &str, mut v: Vec<Duration>) {
    v.sort();
    let mid = v[v.len() / 2];
    println!(
        "{label}: median {:.2}ms range {:.2}..{:.2}ms (n={})",
        mid.as_secs_f64() * 1000.0,
        v[0].as_secs_f64() * 1000.0,
        v[v.len() - 1].as_secs_f64() * 1000.0,
        v.len()
    );
}

fn leaked_config_dir() -> PathBuf {
    let dir = tempfile::tempdir().expect("config dir");
    let path = dir.path().to_path_buf();
    std::mem::forget(dir);
    path
}

/// The production pump. `for_roots` runs reads inline, which would hide the
/// tag-read stall this harness exists to measure.
fn spawned_state(seed: &Seed) -> AppState {
    let state = AppState::launch_in(Some(seed.project.clone()), Some(leaked_config_dir()));
    assert!(!state.multi.roots.is_empty(), "the repo is discovered");
    state
}

fn wait_for_tags(state: &mut AppState) -> Duration {
    let root = state.multi.roots[0].id.clone();
    state.request_branch_tags();
    let start = Instant::now();
    loop {
        state.drain_events();
        if state.caches.tags_loaded(&root) {
            return start.elapsed();
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "tags never arrived"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn pane_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = spawned_state(seed);
    state.ui.tab = Tab::Log;
    let root = state.multi.roots[0].id.clone();
    state.request_branch_tags();
    state.fetch_refs(root.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        state.drain_events();
        if state.caches.tags_loaded(&root) && state.caches.refs_loaded(&root) {
            break;
        }
        assert!(Instant::now() < deadline, "reads never settled");
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events();
            egui::CentralPanel::default().show(ui, |ui| {
                turbogit_ui::ui::log_window::branches_pane(ui, state)
            });
        },
        state,
    );
    harness.set_size(egui::vec2(240.0, 760.0));
    harness
}

fn frame_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(240.0, 760.0),
        )),
        ..Default::default()
    }
}

/// The first frame is spent before `Harness` returns (it runs one to
/// initialise accesskit), so this runs the pane on a bare `egui::Context`.
/// Frame one opens without tags. The tag read then finishes on its worker,
/// and the frame that folds the tags in pays the one rebuild.
#[test]
#[ignore = "manual perf harness; run with --ignored --nocapture --test-threads=1"]
fn perf_log_branches_first_frame() {
    let seed = seeded();
    let mut first = Vec::new();
    let mut tag_ready = Vec::new();
    let mut with_tags = Vec::new();
    for _ in 0..5 {
        let mut state = spawned_state(&seed);
        state.ui.tab = Tab::Log;
        let ctx = egui::Context::default();
        turbogit_ui::theme::configure_style(&ctx);
        turbogit_ui::theme::install_fonts(&ctx);
        let _ = ctx.run_ui(frame_input(), |_ui| {});

        let t = Instant::now();
        let _ = ctx.run_ui(frame_input(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                turbogit_ui::ui::log_window::branches_pane(ui, &mut state)
            });
        });
        first.push(t.elapsed());

        tag_ready.push(wait_for_tags(&mut state));

        let t = Instant::now();
        let _ = ctx.run_ui(frame_input(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                turbogit_ui::ui::log_window::branches_pane(ui, &mut state)
            });
        });
        with_tags.push(t.elapsed());
    }
    report("first-frame", first);
    report("tags-ready-wait", tag_ready);
    report("first-frame-with-tags", with_tags);
}

#[test]
#[ignore = "manual perf harness; run with --ignored --nocapture --test-threads=1"]
fn perf_log_branches_pane() {
    let seed = seeded();
    let mut harness = pane_harness(&seed);

    let mut open = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        harness.step();
        open.push(t.elapsed());
    }
    report("open-first-10", open);

    let mut steady = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        harness.step();
        steady.push(t.elapsed());
    }
    report("steady", steady);

    let search = harness.get_by_label("Search branches").rect();
    harness.hover_at(egui::pos2(search.center().x, search.bottom() + 60.0));
    harness.step();

    let mut scroll = Vec::new();
    for _ in 0..20 {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -80.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
        let t = Instant::now();
        harness.step();
        scroll.push(t.elapsed());
    }
    report("scroll", scroll);

    let root = harness.state().multi.roots[0].id.clone();
    harness
        .state_mut()
        .ui
        .log_tree
        .collapsed_remotes
        .insert((root, "origin".to_string()));
    let mut expand_remote = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        harness.step();
        expand_remote.push(t.elapsed());
    }
    report("expand-remote", expand_remote);

    harness.state_mut().ui.log_tree.groups.tags = true;
    let mut expand_tags = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        harness.step();
        expand_tags.push(t.elapsed());
    }
    report("expand-tags", expand_tags);

    harness.state_mut().ui.log_branch_filter = "branch-12".to_string();
    let mut filtered = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        harness.step();
        filtered.push(t.elapsed());
    }
    report("filtered-steady", filtered);
}
