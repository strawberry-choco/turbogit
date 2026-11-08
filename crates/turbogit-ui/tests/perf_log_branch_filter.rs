//! Manual perf harness for the Log filter bar's Branch dropdown.
//!
//! Run it with:
//! `cargo test --release -p turbogit-ui --test perf_log_branch_filter -- --ignored --nocapture --test-threads=1`
//!
//! Same ref fixture as `perf_log_branches.rs`, which measures the left pane.
//! This one opens the `Branch` dropdown and times its frames, then attributes
//! the cost to the three per-frame passes: the band model rebuild, the popup
//! width fit, and the row build.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use tempfile::TempDir;
use test_support::harness::painted_galleys;
use turbogit_app::log_filters::{Fold, RefGroup, ref_bands};
use turbogit_app::state::{AppState, Tab};

const LOCAL: usize = 10_000;
const REMOTE: usize = 10_000;
const TAGS: usize = 8_000;

const BAR_SIZE: egui::Vec2 = egui::vec2(1200.0, 800.0);

/// Everything the bar and the popup paint that is not a ref row. The fixture's
/// local names are half `branch-` and half `feature/deep/branch-`, so a single
/// prefix undercounts by 20%.
const CHROME: &[&str] = &[
    "All",
    "Select\u{2026}",
    "Apply",
    "Branch",
    "User",
    "Date",
    "Paths",
    "Clear",
    "Local branches",
    "Remote branches",
    "Tag refs",
    "Branch name",
];

fn is_ref(label: &str) -> bool {
    !label.is_empty()
        && !CHROME.contains(&label)
        && (label.starts_with("branch-")
            || label.starts_with("feature/deep/branch-")
            || label.starts_with("origin/")
            || label.starts_with("upstream/")
            || label
                .strip_prefix('v')
                .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit())))
}

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

fn spawned_state(seed: &Seed) -> AppState {
    let state = AppState::launch_in(Some(seed.project.clone()), Some(leaked_config_dir()));
    assert!(!state.multi.roots.is_empty(), "the repo is discovered");
    state
}

fn wait_for_tags(state: &mut AppState) {
    let root = state.multi.roots[0].id.clone();
    state.request_branch_tags();
    let start = Instant::now();
    loop {
        state.drain_events();
        if state.caches.tags_loaded(&root) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "tags never arrived"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn bar_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = spawned_state(seed);
    state.ui.tab = Tab::Log;
    wait_for_tags(&mut state);
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
                turbogit_ui::ui::filter_bar(ui, state);
            });
        },
        state,
    );
    harness.set_size(BAR_SIZE);
    harness
}

fn rows_built(harness: &Harness<'_, AppState>) -> usize {
    harness
        .query_all_by_role(Role::Button)
        .filter(|node| node.accesskit_node().label().as_deref().is_some_and(is_ref))
        .count()
}

fn rows_shown(harness: &Harness<'_, AppState>) -> usize {
    painted_galleys(harness)
        .iter()
        .filter(|g| is_ref(&g.text))
        .count()
}

fn timed_frames(harness: &mut Harness<'static, AppState>, n: usize) -> Vec<Duration> {
    (0..n)
        .map(|_| {
            let t = Instant::now();
            harness.step();
            t.elapsed()
        })
        .collect()
}

fn set_folds(harness: &mut Harness<'static, AppState>, fold: Fold) {
    for group in [RefGroup::Local, RefGroup::Remote, RefGroup::Tag] {
        harness.state_mut().ui.log_ref_folds.insert(group, fold);
    }
}

#[test]
#[ignore = "manual perf harness; run with --ignored --nocapture --test-threads=1"]
fn perf_log_branch_filter() {
    let seed = seeded();
    let mut harness = bar_harness(&seed);

    report("closed-bar", timed_frames(&mut harness, 10));

    harness.get_by_label("Branch").click();
    report("open-first-10", timed_frames(&mut harness, 10));
    println!(
        "default-folds: built {} ref rows, painted {}",
        rows_built(&harness),
        rows_shown(&harness)
    );
    report("steady-local-open", timed_frames(&mut harness, 20));

    let list = painted_galleys(&harness)
        .into_iter()
        .find(|g| is_ref(&g.text))
        .expect("the open dropdown paints a ref row");
    harness.hover_at(list.pos);
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

    set_folds(&mut harness, Fold::Open);
    report("all-bands-open", timed_frames(&mut harness, 10));
    println!(
        "all-open: built {} ref rows, painted {}",
        rows_built(&harness),
        rows_shown(&harness)
    );

    set_folds(&mut harness, Fold::Closed);
    report("all-bands-folded", timed_frames(&mut harness, 20));
    println!("all-folded: built {} ref rows", rows_built(&harness));

    // The model pass on its own, with no egui in the timed region.
    let mut model = Vec::new();
    for _ in 0..20 {
        let state = harness.state();
        let selected = state.ui.log_roots.selected(&state.multi.roots);
        let tags = state.caches.tags_by_root();
        let folds = &state.ui.log_ref_folds;
        let t = Instant::now();
        let bands = ref_bands(&selected, tags, folds);
        model.push(t.elapsed());
        std::hint::black_box(&bands);
    }
    report("ref_bands-alone", model);

    // The width pass on its own: one layout_no_wrap per open-band rev, which is
    // what branch_menu_width does every frame.
    let texts: Vec<String> = {
        let state = harness.state();
        let selected = state.ui.log_roots.selected(&state.multi.roots);
        let bands = ref_bands(
            &selected,
            state.caches.tags_by_root(),
            &state.ui.log_ref_folds,
        );
        bands
            .iter()
            .filter(|b| b.group == RefGroup::Local)
            .flat_map(|b| b.revs.iter().cloned())
            .collect()
    };
    let ctx = egui::Context::default();
    turbogit_ui::theme::configure_style(&ctx);
    turbogit_ui::theme::install_fonts(&ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, BAR_SIZE)),
        ..Default::default()
    };
    let mut width = Vec::new();
    for _ in 0..10 {
        let _ = ctx.run_ui(input.clone(), |ui| {
            let painter = ui.painter().clone();
            let font = turbogit_ui::theme::chrome_font(turbogit_ui::theme::TYPE_CONTROL);
            let ink = turbogit_ui::theme::Palette::INK;
            let t = Instant::now();
            let mut widest = 0.0_f32;
            for text in &texts {
                widest = widest.max(
                    painter
                        .layout_no_wrap(text.clone(), font.clone(), ink)
                        .size()
                        .x,
                );
            }
            width.push(t.elapsed());
            std::hint::black_box(widest);
        });
    }
    report("width-fit-alone-10k", width);
    println!("attributed over {} local revs", texts.len());
}
