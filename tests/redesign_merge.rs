//! Issue #15 — 3-way merge editor redesign tests.
//!
//! Headless egui_kittest harness driving [`turbogit::ui::render`] against a
//! temporary repository seeded with a REAL two-hunk merge conflict, asserting
//! only on public surfaces:
//!
//! - **Painted output** — pane headers, block text, yours/theirs tints,
//!   marker strips, the Result focus outline, and the "N conflicts
//!   remaining" counter.
//! - **State transitions** — public `AppState` fields (`conflict_res`,
//!   `conflict_text`, `conflict_open`) and the on-disk file after Apply.
//!
//! Covered behavior:
//! - three EQUAL panes Local | Result | Incoming with discrete conflict
//!   blocks (marker strips + tinted yours/theirs sections) and the Result
//!   pane visually outlined as focused
//! - per-block Accept Yours / Accept Theirs / Ignore buttons drive a
//!   READ-ONLY composed Result and decrement the remaining counter
//! - Apply is disabled until every block resolves, then writes the composed
//!   result through the engine's existing resolution flow and clears the
//!   conflict state

mod common;

use common::{assert_not_painted, assert_painted, filled_rects, galley_origin};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, Key, Modifiers, Pos2, Rect, Shape};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use turbogit::engine::{AppEvent, GitExecutor};
use turbogit::model::{Root, RootId, VcsSettings};
use turbogit::state::AppState;
use turbogit::theme::Palette;

// ---------------------------------------------------------------- helpers --

/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Run `git` without asserting success (for commands that may legitimately
/// fail, e.g. a merge that conflicts).
fn git_unchecked(repo: &Path, args: &[&str]) {
    let _ = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output();
}

struct Repo {
    path: PathBuf,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly. The caller
/// keeps `parent` (a `TempDir`) alive for the duration of the test.
fn temp_repo(parent: &Path, name: &str) -> Repo {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path }
}

impl Repo {
    fn branch(&self) -> String {
        git(&self.path, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim()
            .to_string()
    }
}

/// Seed `conf.txt` with TWO well-separated conflict hunks on `repo`'s
/// default branch: ours rewrites lines 1 and 9 to `MAIN-*`, theirs to
/// `SIDE-*`. The seven unchanged lines between the edits keep git from
/// coalescing both hunks into a single conflict region.
fn seed_two_conflicts(repo: &Path, branch: &str) {
    std::fs::write(
        repo.join("conf.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "base conf"]);
    git(repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "side commit"]);
    git(repo, &["checkout", "-q", branch]);
    std::fs::write(
        repo.join("conf.txt"),
        "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "main commit"]);
    git_unchecked(repo, &["merge", "--no-edit", "side"]); // expected to conflict
}

/// Expected composed result for resolutions `[Accept Yours 1, Accept Theirs 2]`.
const COMPOSED: &str = "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n";

/// Build an `AppState` over the given roots with synchronous status scans
/// (no background threads), so tests are deterministic.
fn app_state(roots: &[PathBuf]) -> AppState {
    let (tx, rx) = crossbeam_channel::unbounded();
    let settings = VcsSettings::default();
    let executor: Arc<dyn GitExecutor> = Arc::new(turbogit::engine::cli::CliExecutor {
        settings: settings.clone(),
    });
    let mut st = AppState {
        project_dir: roots
            .first()
            .and_then(|r| r.parent())
            .map(Path::to_path_buf)
            .unwrap_or_default(),
        executor,
        settings,
        multi: Default::default(),
        tx,
        rx,
        selected_root: None,
        clone_url: String::new(),
        last_error: None,
        ui: Default::default(),
        log_cache: Default::default(),
        ahead_behind: Default::default(),
        recents_config_dir: None,
        dir_picker: None,
        ref_cache: Default::default(),
        files_cache: Default::default(),
        log_path_cache: Default::default(),
    };
    for r in roots {
        let id = RootId(r.clone());
        let status = st.executor.status(r).expect("status scan");
        let current_branch = st.executor.current_branch(r).ok().flatten();
        st.multi.register_root(Root {
            id: id.clone(),
            path: r.clone(),
            remotes: vec![],
            branches: vec![],
            current_branch,
            head: None,
            status,
        });
        if st.selected_root.is_none() {
            st.selected_root = Some(id);
        }
    }
    st
}

/// Drain worker-thread events exactly like `app.rs`, but re-status
/// synchronously after completed ops so tests stay deterministic.
fn drain_events(state: &mut AppState) {
    while let Ok(ev) = state.rx.try_recv() {
        match ev {
            AppEvent::StatusScanned {
                root,
                status: Ok(s),
            } => {
                if let Some(r) = state.multi.roots.iter_mut().find(|r| r.id == root) {
                    r.status = s;
                }
            }
            AppEvent::StatusScanned { .. } => {}
            AppEvent::OpCompleted { label, result } => {
                state.ui.busy = false;
                match result {
                    Ok(()) => {
                        state.ui.toast = Some(turbogit::state::Toast::success(label.to_string()));
                        for root in &mut state.multi.roots {
                            if let Ok(s) = state.executor.status(&root.path) {
                                root.status = s;
                            }
                        }
                    }
                    Err(e) => {
                        state.ui.toast =
                            Some(turbogit::state::Toast::error(format!("{label}: {e}")));
                        state.last_error = Some(e.to_string());
                    }
                }
            }
            AppEvent::DiffReady { key, result } => {
                state.ui.diff_loading = false;
                match result {
                    Ok(text) => {
                        state.ui.diff_error = None;
                        state.ui.diff_cache = Some((key, text));
                    }
                    Err(e) => state.ui.diff_error = Some(e.to_string()),
                }
            }
            AppEvent::AheadBehind {
                root,
                ahead,
                behind,
            } => {
                state.ahead_behind.insert(root, (ahead, behind));
            }
            _ => {}
        }
    }
}

/// Headless harness driving the full app UI with event draining per frame.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            drain_events(state);
            turbogit::ui::render(ui, state);
        },
        state,
    )
}

/// Poll `f` until it returns true or the deadline elapses (worker threads run
/// asynchronously, so completion is observed by polling).
fn wait_until<F: Fn() -> bool>(ms: u64, f: F) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Open the redesigned merge editor for the seeded conflicted file.
fn open_merge_editor(h: &mut Harness<'_, AppState>) {
    h.get_by_label("Merge…").click();
    h.run();
}

/// The Apply action button of the merge editor window.
fn apply_button<'h>(h: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    h.get_by_label("Apply")
}

// --------------------------------------------- tint + geometry mirror fns --

/// Mirror of the editor's `tint_over_bg`: per-channel linear blend of
/// `accent` over [`Palette::BG`] at opacity `t`. Kept local so the tests
/// pin the exact painted colors without exposing crate internals.
fn tint_over_bg(accent: Color32, t: f32) -> Color32 {
    let bg = Palette::BG;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        mix(bg.r(), accent.r()),
        mix(bg.g(), accent.g()),
        mix(bg.b(), accent.b()),
    )
}

/// Conflict "yours" section background (STATE_INFO @ 12% over BG).
fn yours_bg() -> Color32 {
    tint_over_bg(Palette::STATE_INFO, 0.12)
}

/// Conflict "theirs" section background (STATE_ERROR @ 12% over BG).
fn theirs_bg() -> Color32 {
    tint_over_bg(Palette::STATE_ERROR, 0.12)
}

/// Conflict marker-strip background (STATE_WARNING @ 15% over BG).
fn marker_bg() -> Color32 {
    tint_over_bg(Palette::STATE_WARNING, 0.15)
}

/// All stroked rectangles painted by the last frame as `(rect, color)`.
fn stroked_rects(harness: &Harness<'_, AppState>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rs) if rs.stroke != egui::Stroke::NONE => Some((rs.rect, rs.stroke.color)),
            _ => None,
        })
        .collect()
}

/// Paint-time origin of the first text galley whose content equals `text`
/// modulo trailing newlines (block labels carry a trailing '\n').
fn origin_of(harness: &Harness<'_, AppState>, text: &str) -> Option<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text().trim_end() == text => Some(shape.pos),
            _ => None,
        })
}

/// The smallest SURFACE-filled rect containing `label`'s galley origin
/// (i.e. the pane header band behind the label, not the window backdrop).
fn header_band(harness: &Harness<'_, AppState>, label: &str) -> Rect {
    let pos = galley_origin(harness, label)
        .unwrap_or_else(|| panic!("{label} header label was not painted"));
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::SURFACE && r.contains(pos))
        .min_by_key(|(r, _)| r.area() as i64)
        .map(|(r, _)| r)
        .unwrap_or_else(|| panic!("{label} header band was not painted"))
}

// ------------------------------------------------------------------ tests --

#[test]
fn merge_editor_renders_three_equal_panes_with_tinted_blocks() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "panes-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    // Three pane headers paint.
    assert_painted(&h, "Local (Yours)");
    assert_painted(&h, "Result");
    assert_painted(&h, "Incoming (Theirs)");

    // Both sides of the first conflict block paint…
    assert_painted(&h, "MAIN-one");
    assert_painted(&h, "SIDE-one");
    // …and unresolved blocks show the read-only placeholder in Result.
    assert_painted(&h, "<< unresolved >>");

    // The counter starts at the full count.
    assert_painted(&h, "2 conflicts remaining");

    // The three panes are EQUAL width (header bands within 6px).
    let local = header_band(&h, "Local (Yours)");
    let result = header_band(&h, "Result");
    let incoming = header_band(&h, "Incoming (Theirs)");
    assert!(
        (local.width() - result.width()).abs() <= 6.0
            && (result.width() - incoming.width()).abs() <= 6.0,
        "panes must be equal width, got widths {}/{}/{}",
        local.width(),
        result.width(),
        incoming.width()
    );

    // Yours/theirs sections are tinted behind their block text.
    let ours_pos = origin_of(&h, "MAIN-one").expect("ours text painted");
    let theirs_pos = origin_of(&h, "SIDE-one").expect("theirs text painted");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == yours_bg() && r.contains(ours_pos)),
        "yours section must be tinted behind its text"
    );
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == theirs_bg() && r.contains(theirs_pos)),
        "theirs section must be tinted behind its text"
    );

    // Marker strips paint as warning-tinted bands.
    assert!(
        filled_rects(&h).iter().any(|(_, c)| *c == marker_bg()),
        "conflict marker strips must be painted"
    );

    // The Result pane is visually outlined as focused (BRAND stroke around
    // the Result header region).
    let result_pos = galley_origin(&h, "Result").expect("Result header painted");
    assert!(
        stroked_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::BRAND && r.contains(result_pos)),
        "Result pane must carry the BRAND focus outline"
    );
}

#[test]
fn accept_buttons_resolve_blocks_update_read_only_result_and_counter() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "resolve-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    // Resolving block 1 decrements the counter and records the choice.
    h.get_by_label("Accept Yours 1").click();
    h.run();
    assert_painted(&h, "1 conflict remaining");
    assert_eq!(h.state().ui.conflict_res[0], Some(0));

    // Resolving block 2 reaches zero and composes the final text.
    h.get_by_label("Accept Theirs 2").click();
    h.run();
    assert_painted(&h, "0 conflicts remaining");
    assert_eq!(
        h.state().ui.conflict_res,
        vec![Some(0), Some(1)],
        "per-block choices must land in conflict_res"
    );
    assert_eq!(
        h.state().ui.conflict_text,
        COMPOSED,
        "resolutions must compose the read-only Result"
    );
    assert_not_painted(&h, "<< unresolved >>");

    // READ-ONLY: the composed buffer is not exposed as one editable widget
    // (the old editor offered a multiline TextEdit over the whole text), and
    // stray keyboard input cannot mutate it.
    assert!(
        h.query_by_label(COMPOSED).is_none(),
        "the composed result must not be a single (editable) widget value"
    );
    let before = h.state().ui.conflict_text.clone();
    h.key_press_modifiers(Modifiers::NONE, Key::A);
    h.run();
    assert_eq!(
        h.state().ui.conflict_text,
        before,
        "free-text editing is deferred: input must not change the buffer"
    );
}

#[test]
fn apply_is_gated_until_all_blocks_resolved() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "gate-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    assert!(
        apply_button(&h).accesskit_node().is_disabled(),
        "Apply must start disabled while conflicts remain"
    );

    h.get_by_label("Accept Yours 1").click();
    h.run();
    assert!(
        apply_button(&h).accesskit_node().is_disabled(),
        "Apply must stay disabled while any block is unresolved"
    );

    h.get_by_label("Accept Theirs 2").click();
    h.run();
    assert!(
        !apply_button(&h).accesskit_node().is_disabled(),
        "Apply must enable at zero conflicts remaining"
    );
}

#[test]
fn apply_writes_composed_result_and_clears_conflict_state() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "apply-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    h.get_by_label("Accept Yours 1").click();
    h.get_by_label("Accept Theirs 2").click();
    h.run();
    apply_button(&h).click();
    h.run();

    // End-to-end: the composed result lands on disk through the engine's
    // resolution flow (write + stage), clearing the unmerged index state.
    assert!(
        wait_until(15_000, || {
            std::fs::read_to_string(repo.path.join("conf.txt"))
                .map(|s| s == COMPOSED)
                .unwrap_or(false)
                && !git(&repo.path, &["status", "--porcelain"])
                    .lines()
                    .any(|l| l.starts_with("UU"))
        }),
        "Apply must write the composed result and clear the unmerged state, got:\n{}",
        git(&repo.path, &["status", "--porcelain"])
    );

    // The editor closes and the root rescans to zero conflicts.
    let root_id = RootId(repo.path.clone());
    let start = Instant::now();
    loop {
        h.run();
        let cleared = h.state().ui.conflict_open.is_none()
            && h.state()
                .multi
                .by_id(&root_id)
                .map(|r| r.status.conflicted.is_empty())
                .unwrap_or(false);
        if cleared {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "conflict state must clear after Apply (open={:?})",
            h.state().ui.conflict_open
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn ignore_keeps_both_sides_and_counts_as_resolved() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "ignore-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    h.get_by_label("Ignore 1").click();
    h.run();

    assert_painted(&h, "1 conflict remaining");
    assert_eq!(h.state().ui.conflict_res[0], Some(2));
    let text = &h.state().ui.conflict_text;
    assert!(
        text.contains("MAIN-one") && text.contains("SIDE-one"),
        "Ignore must keep both sides of the block, got {text:?}"
    );
}
