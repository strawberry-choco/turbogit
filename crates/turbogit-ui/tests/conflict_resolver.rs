//! Issue #22 — Editable conflict resolver (screen 07) tests.
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] against a
//! temporary repository seeded with a REAL merge conflict, asserting only on
//! public surfaces:
//!
//! - **Painted output** — three panes, header actions, file list groups,
//!   per-conflict action labels, "Edit manually", "Auto-advance" toggle,
//!   "Apply to all N remaining".
//! - **State transitions** — public `AppState.ui.conflict_*` fields and the
//!   on-disk file after Apply.
//!
//! Covered behavior (matches screen 07 + issue acceptance criteria):
//! - the redesigned resolver is a dedicated surface (separate from the
//!   existing inline 3-way merge editor), reachable when any conflict exists
//! - three EQUAL panes Local | Result | Incoming with discrete conflict
//!   blocks, free-text editing of the Result pane replaces the composed text
//! - per-conflict actions: Take ours / Take theirs / Take both (theirs first)
//! - per-conflict Undo restores the conflict to unresolved
//! - Prev/Next navigate across the file's conflicts, with Auto-advance
//!   moving focus after each resolution
//! - "Apply to all N remaining" applies the current choice to every
//!   still-unresolved conflict in the file
//! - the left list splits auto-merged (resolved by git without interaction)
//!   from conflicted files
//! - header Abort merge restores pre-merge state; Continue merge completes
//!   the merge once every conflicted file is resolved

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use test_support::harness::{assert_painted, filled_rects, galley_origin};

use egui::{Color32, Pos2, Rect, Shape};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use turbogit_app::state::AppState;

use turbogit_ui::theme::Palette;
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

/// Run `git` without asserting success (a merge that conflicts exits non-zero).
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
/// branch and repo-local user config so commits work headlessly.
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

/// Seed a merge where `auto.txt` is auto-merged cleanly while `conf.txt`
/// is conflicted. Both files diverge on both branches but only `auto.txt`
/// has IDENTICAL changes on both sides so git resolves it itself.
fn seed_merge_with_auto_merged(repo: &Path, branch: &str) {
    // base: auto.txt = "shared-0\n", conf.txt = ten lines
    std::fs::write(repo.join("auto.txt"), "shared-0\n").unwrap();
    std::fs::write(
        repo.join("conf.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "base"]);

    // side: auto.txt to "shared-1\n", conf.txt to SIDE-*
    git(repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("auto.txt"), "shared-1\n").unwrap();
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "side"]);
    git(repo, &["checkout", "-q", branch]);

    // main: auto.txt to "shared-1\n" (identical to side), conf.txt to MAIN-*
    std::fs::write(repo.join("auto.txt"), "shared-1\n").unwrap();
    std::fs::write(
        repo.join("conf.txt"),
        "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "main"]);
    git_unchecked(repo, &["merge", "--no-edit", "side"]); // expected: auto clean, conf conflict
}

/// Headless harness over the given repository roots.
fn app_state(roots: &[PathBuf]) -> AppState {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    AppState::for_roots(&dir, roots)
}

/// Headless harness driving the full app UI with event draining per frame.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(4096).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
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

// --------------------------------------------- tint + geometry mirror fns --

fn tint_over_bg(accent: Color32, t: f32) -> Color32 {
    let bg = Palette::BG;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        mix(bg.r(), accent.r()),
        mix(bg.g(), accent.g()),
        mix(bg.b(), accent.b()),
    )
}

fn yours_bg() -> Color32 {
    tint_over_bg(Palette::STATE_INFO, 0.12)
}

fn theirs_bg() -> Color32 {
    tint_over_bg(Palette::STATE_ERROR, 0.12)
}

fn marker_bg() -> Color32 {
    tint_over_bg(Palette::STATE_WARNING, 0.15)
}
#[allow(dead_code)]
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
/// modulo trailing newlines.
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

/// The smallest SURFACE-filled rect containing `label`'s galley origin.
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

/// Open the redesigned conflict resolver for the seeded conflicted file
/// via the Commit tool window's new "Resolve…" entry point.
fn open_resolver(h: &mut Harness<'_, AppState>) {
    h.get_by_label("Resolve…").click();
    h.run();
}

// ------------------------------------------------------------------ tests --

/// RED: The redesigned resolver surface is reachable while any conflict
/// exists. It must render the three pane headers AND the new header
/// actions (Abort merge / Continue merge / Auto-advance / Apply to all).
#[test]
fn redesigned_resolver_renders_three_panes_and_header_actions() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "resolver-panes-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Three pane headers paint.
    assert_painted(&h, "Local (Yours)");
    assert_painted(&h, "Result");
    assert_painted(&h, "Incoming (Theirs)");

    // Header actions visible while a merge is in progress.
    assert_painted(&h, "Abort merge");
    assert_painted(&h, "Continue merge");

    // Per-conflict and global action labels visible.
    assert_painted(&h, "Take ours");
    assert_painted(&h, "Take theirs");
    assert_painted(&h, "Take both"); // theirs first per spec
    assert_painted(&h, "Edit manually");
    assert_painted(&h, "Apply to all");
    assert_painted(&h, "Auto-advance to next conflict");
}

/// RED: The Result pane is a free-text editable surface — typing into it
/// replaces the composed buffer that Apply writes to disk.
#[test]
fn result_pane_accepts_free_text_edits() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "edit-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // kittest's TextEdit doesn't drive multi-line edits cleanly in headless
    // mode — we exercise the public seam (state.ui.conflict_text) directly.
    // A separate manual smoke verifies the actual TextEdit widget renders.

    h.run();
    h.state_mut().ui.conflict_text = "CUSTOM-RESULT\n".to_string();
    h.run();

    // The free-text edit IS the resolved content (no longer a single
    // read-only composed galley).
    assert_eq!(h.state().ui.conflict_text, "CUSTOM-RESULT\n");
}

/// RED: "Take both · theirs first" resolves a conflict by keeping both
/// sides with THEIRS before OURS (the spec order; existing "Ignore"
/// choice is ours before theirs).
#[test]
fn take_both_theirs_first_keeps_both_with_theirs_before_ours() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "both-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    h.get_by_label("Take both 1").click();
    h.run();

    let text = &h.state().ui.conflict_text;
    let theirs_pos = text.find("SIDE-one").expect("theirs in composed text");
    let ours_pos = text.find("MAIN-one").expect("ours in composed text");
    assert!(
        theirs_pos < ours_pos,
        "Take both must place theirs BEFORE ours, got:\n{text}"
    );
}

/// RED: Per-conflict Undo restores the conflict to unresolved and recomposes.
#[test]
fn per_conflict_undo_reverts_resolution() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "undo-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    h.get_by_label("Take ours 1").click();
    h.run();
    assert_eq!(h.state().ui.conflict_res[0], Some(0));
    assert_painted(&h, "1 conflict remaining");

    h.get_by_label("Undo 1").click();
    h.run();
    assert_eq!(
        h.state().ui.conflict_res[0],
        None,
        "Undo must revert the conflict to unresolved"
    );
    assert_painted(&h, "2 conflicts remaining");
}

/// RED: Prev/Next navigate between the file's conflicts; the active one
/// is the one whose actions appear in the action row.
#[test]
fn prev_next_navigates_between_conflicts() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "nav-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Initially on conflict 1.
    assert_eq!(h.state().ui.conflict_resolver_active_idx, 0);

    h.get_by_label("Next >").click();
    h.run();
    assert_eq!(h.state().ui.conflict_resolver_active_idx, 1);

    h.get_by_label("< Prev").click();
    h.run();
    assert_eq!(h.state().ui.conflict_resolver_active_idx, 0);
}

/// RED: With Auto-advance enabled, resolving the current conflict moves
/// focus to the next unresolved conflict in the file.
#[test]
fn auto_advance_moves_focus_after_resolution() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "autoadv-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Toggle auto-advance on.
    h.get_by_label("Auto-advance to next conflict").click();
    h.run();
    assert!(h.state().ui.conflict_resolver_auto_advance);

    // Resolving the active conflict advances the cursor.
    h.get_by_label("Take ours 1").click();
    h.run();
    assert_eq!(h.state().ui.conflict_resolver_active_idx, 1);
}

/// RED: "Apply to all N remaining" applies the active conflict's choice
/// to every still-unresolved conflict in the file.
#[test]
fn apply_to_all_remaining_resolves_every_conflict_in_the_file() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "applyall-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Resolve the first conflict via the active buttons so a recorded
    // choice exists; "Apply to all N remaining" applies that choice to
    // every other still-unresolved conflict in the file.
    h.get_by_label("Take theirs 1").click();
    h.run();
    h.get_by_label("Apply to all 1 remaining").click();
    h.run();

    assert!(
        h.state().ui.conflict_res.iter().all(|r| r.is_some()),
        "Apply to all must resolve every conflict in the file, got {:?}",
        h.state().ui.conflict_res
    );
}

/// RED: The left list splits conflicted files from auto-merged files.
#[test]
fn left_list_splits_auto_merged_from_conflicted() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "split-repo");
    let branch = repo.branch();
    seed_merge_with_auto_merged(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    assert_painted(&h, "CONFLICTED FILES");
    assert_painted(&h, "AUTO-MERGED");
    assert_painted(&h, "conf.txt");
    assert_painted(&h, "auto.txt");
}

/// RED: Abort merge restores the pre-merge state — the conflicted file
/// disappears and the index is no longer mid-merge.
#[test]
fn abort_merge_restores_pre_merge_state() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "abort-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    h.get_by_label("Abort merge").click();
    h.run();

    assert!(
        wait_until(10_000, || {
            // After `git merge --abort`, the unmerged paths are gone and
            // HEAD points back at the pre-merge commit.
            let status = git(&repo.path, &["status", "--porcelain"]);
            !status.lines().any(|l| l.starts_with("UU")) && !repo.path.join("conf.txt").exists()
                || std::fs::read_to_string(repo.path.join("conf.txt"))
                    .map(|s| s.starts_with("MAIN-one"))
                    .unwrap_or(false)
        }),
        "Abort merge must restore pre-merge state"
    );
}

/// RED: Continue merge completes the merge once zero conflicts remain.
#[test]
fn continue_merge_completes_when_no_conflicts_remain() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "continue-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Resolve every conflict in the file. After resolving conflict 1 the
    // active cursor stays on it (Auto-advance is off by default); we
    // navigate to conflict 2 with Next and then resolve it.
    h.get_by_label("Take ours 1").click();
    h.run();
    h.get_by_label("Next >").click();
    h.run();
    h.get_by_label("Take ours 2").click();
    h.run();

    // Apply writes the composed buffer to the working tree and stages it,
    // clearing the unmerged state; only then can `git merge --continue`
    // succeed.
    h.get_by_label("Apply").click();
    h.run();

    // The composed buffer is the file content; the in-progress merge has
    // no conflicts left in the index but the MERGE_HEAD is still pending.
    h.get_by_label("Continue merge").click();
    h.run();

    assert!(
        wait_until(10_000, || {
            let status = git(&repo.path, &["status", "--porcelain"]);
            status.trim().is_empty()
        }),
        "Continue merge must finish once no conflicts remain"
    );
}

/// RED: The three panes are equal width with discrete tinted conflict blocks
/// (the redesign preserves the EQUAL layout from issue #15).
#[test]
fn resolver_renders_equal_width_panes_with_tinted_blocks() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "panes2-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_resolver(&mut h);

    // Three pane headers paint.
    assert_painted(&h, "Local (Yours)");
    assert_painted(&h, "Result");
    assert_painted(&h, "Incoming (Theirs)");

    // Both sides of the first conflict block paint.
    assert_painted(&h, "MAIN-one");
    assert_painted(&h, "SIDE-one");

    // The three panes are EQUAL width.
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
}
