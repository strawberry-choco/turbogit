//! The N5 perf fixture: a 1k-commit interactive rebase plan and a 1k-hunk
//! conflict file, p95 frame build against the egui trunk's screens at the
//! same window and display, interleaved.
//!
//! Run with:
//! `cargo test --release -p turbogit-ui-nsh --test perf_rewrite -- --ignored --nocapture --test-threads=1`

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use tempfile::TempDir;
use turbogit_app::state::AppState;

mod support;

const COMMITS: usize = 1_000;
const HUNKS: usize = 1_000;
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

fn plan_repo(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let project = tmp.path().join("plan");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);

    let mut stream = String::new();
    stream.push_str("commit refs/heads/main\nmark :1\nauthor T <t@t> 1 +0000\ncommitter T <t@t> 1 +0000\ndata 5\nbase\nM 644 inline base.txt\ndata 5\nbase\n");
    for n in 0..COMMITS {
        let mark = n + 2;
        let parent = mark - 1;
        let subject = format!("commit {n:04}");
        let body = format!("body {n}");
        stream.push_str(&format!(
            "commit refs/heads/feature\nmark :{mark}\nauthor T <t@t> {time} +0000\ncommitter T <t@t> {time} +0000\ndata {len}\n{subject}\nfrom :{parent}\nM 644 inline f.txt\ndata {body_len}\n{body}\n",
            time = n + 2,
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
    git(&repo, &["checkout", "-q", "feature"]);
    (project, repo)
}

fn conflict_repo(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let project = tmp.path().join("conflict");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);

    let build = |ours: bool| {
        let mut text = String::new();
        for hunk in 0..HUNKS {
            let first = if ours {
                format!("ours-{hunk}\n")
            } else {
                format!("base-{hunk}\n")
            };
            text.push_str(&first);
            for line in 0..9 {
                text.push_str(&format!("common-{hunk}-{line}\n"));
            }
        }
        text
    };
    std::fs::write(repo.join("conf.txt"), build(false)).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    let mut side = String::new();
    for hunk in 0..HUNKS {
        let line = format!("theirs-{hunk}\n");
        side.push_str(&line);
        for line in 0..9 {
            side.push_str(&format!("common-{hunk}-{line}\n"));
        }
    }
    std::fs::write(repo.join("conf.txt"), side).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "side"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("conf.txt"), build(true)).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "ours"]);
    let _ = Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo)
        .output();
    (project, repo)
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

fn interleaved(
    mut head: nsh_testing::Headless,
    mut trunk: Harness<'static, AppState>,
) -> (Duration, Duration) {
    for _ in 0..20 {
        head.next_frame(Duration::from_millis(16)).unwrap();
        trunk.step();
    }
    let frame = Duration::from_millis(16);
    let mut head_frames = Vec::new();
    let mut trunk_frames = Vec::new();
    for _ in 0..40 {
        let start = Instant::now();
        head.next_frame(frame).unwrap();
        head_frames.push(start.elapsed());
        let start = Instant::now();
        trunk.step();
        trunk_frames.push(start.elapsed());
    }
    report("head", &head_frames);
    report("trunk", &trunk_frames);
    (p95(&head_frames), p95(&trunk_frames))
}

#[test]
#[ignore = "manual perf harness; run with --ignored --nocapture --test-threads=1"]
fn perf_rebase_plan_head_against_trunk() {
    let tmp = tempfile::tempdir().unwrap();
    let (project, repo) = plan_repo(&tmp);
    let oldest = git(&repo, &["rev-list", "--reverse", "main..feature"])
        .lines()
        .next()
        .expect("a commit past main")
        .trim()
        .to_string();

    let head_project = project.clone();
    let head_repo = repo.clone();
    let head_oldest = oldest.clone();
    let mut head = nsh_testing::Headless::start(support::app_config(move || {
        let mut state = AppState::for_roots(&head_project, std::slice::from_ref(&head_repo));
        let root = state.multi.roots[0].id.clone();
        state.open_interactive_rebase(&root, &head_oldest);
        state.refresh(turbogit_app::root_caches::Affected::All);
        state.drain_events();
        state
    }))
    .expect("the nsh app mounts");
    head.wait_until("the plan paints", Duration::from_secs(120), |screen| {
        screen
            .texts()
            .iter()
            .any(|text| text.contains("commit 0000"))
    })
    .expect("the 1k plan paints");

    let mut trunk_state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = trunk_state.multi.roots[0].id.clone();
    trunk_state.open_interactive_rebase(&root, &oldest);
    let mut fonts = false;
    let mut trunk = Harness::new_ui_state(
        move |ui, state| {
            turbogit_ui_egui::theme::configure_style(ui.ctx());
            if !fonts {
                turbogit_ui_egui::theme::install_fonts(ui.ctx());
                fonts = true;
            }
            state.drain_events();
            egui::CentralPanel::default().show(ui, |ui| {
                turbogit_ui_egui::ui::interactive_rebase::show(ui, state)
            });
        },
        trunk_state,
    );
    trunk.set_size(VIEWPORT);

    let (head_p95, trunk_p95) = interleaved(head, trunk);
    let ceiling = trunk_p95 + Duration::from_millis(2);
    println!(
        "plan rule: head p95 {:.2}ms <= trunk p95 {:.2}ms + 2ms = {:.2}ms; under 16.7ms: {}",
        head_p95.as_secs_f64() * 1000.0,
        trunk_p95.as_secs_f64() * 1000.0,
        ceiling.as_secs_f64() * 1000.0,
        head_p95 < Duration::from_micros(16_700),
    );
    assert!(head_p95 <= ceiling);
    assert!(head_p95 < Duration::from_micros(16_700));
}

#[test]
#[ignore = "manual perf harness; run with --ignored --nocapture --test-threads=1"]
fn perf_conflict_file_head_against_trunk() {
    let tmp = tempfile::tempdir().unwrap();
    let (project, repo) = conflict_repo(&tmp);

    let head_project = project.clone();
    let head_repo = repo.clone();
    let mut head = nsh_testing::Headless::start(support::app_config(move || {
        let mut state = AppState::for_roots(&head_project, std::slice::from_ref(&head_repo));
        state.refresh(turbogit_app::root_caches::Affected::All);
        state.drain_events();
        let root = state.multi.roots[0].id.clone();
        turbogit_ui::screens::conflicts::open_resolver_in(
            &mut state,
            &root,
            Path::new("conf.txt"),
        );
        state
    }))
    .expect("the nsh app mounts");
    head.wait_until("the resolver paints", Duration::from_secs(120), |screen| {
        screen
            .matches("id:dialog:resolve-conflicts")
            .unwrap_or(false)
    })
    .expect("the resolver paints");

    let mut trunk_state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root = trunk_state.multi.roots[0].id.clone();
    let content = std::fs::read_to_string(repo.join("conf.txt")).unwrap();
    let (segs, _) = turbogit_app::conflict::parse_conflict_markers(&content);
    trunk_state.ui.conflict_segs = segs;
    trunk_state.ui.conflict_res = vec![None; HUNKS];
    trunk_state.ui.conflict_resolver_undo = vec![None; HUNKS];
    trunk_state.ui.conflict_text = content;
    trunk_state.ui.conflict_resolver_root = Some(root.clone());
    trunk_state.ui.conflict_resolver_selected = Some(PathBuf::from("conf.txt"));
    trunk_state.ui.conflict_resolver_open = true;
    trunk_state.ui.merge_in_progress = true;
    trunk_state.refresh(turbogit_app::root_caches::Affected::All);
    trunk_state.drain_events();

    let mut fonts = false;
    let mut trunk = Harness::new_ui_state(
        move |ui, state| {
            turbogit_ui_egui::theme::configure_style(ui.ctx());
            if !fonts {
                turbogit_ui_egui::theme::install_fonts(ui.ctx());
                fonts = true;
            }
            state.drain_events();
            egui::CentralPanel::default().show(ui, |ui| {
                turbogit_ui_egui::ui::conflict_resolver::render(ui, state)
            });
        },
        trunk_state,
    );
    trunk.set_size(VIEWPORT);

    let (head_p95, trunk_p95) = interleaved(head, trunk);
    let ceiling = trunk_p95 + Duration::from_millis(2);
    println!(
        "conflict rule: head p95 {:.2}ms <= trunk p95 {:.2}ms + 2ms = {:.2}ms; under 16.7ms: {}",
        head_p95.as_secs_f64() * 1000.0,
        trunk_p95.as_secs_f64() * 1000.0,
        ceiling.as_secs_f64() * 1000.0,
        head_p95 < Duration::from_micros(16_700),
    );
    assert!(head_p95 <= ceiling);
    assert!(head_p95 < Duration::from_micros(16_700));
}
