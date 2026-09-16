//! Issue #11 — Commit tool window redesign tests.
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` against
//! temporary git repositories seeded with modified / added / unversioned /
//! conflicted files, asserting painted labels (canonical groups, count
//! badges, M/A/C file-row badges) and public `AppState` transitions only.
//!
//! Issue #18 extends this suite to the Commit window's sub-tab strip:
//! Local Changes / Unversioned Files carry active data, Shelf / Stash are
//! clickable placeholder panes (ADR-0008), and the "Advanced options..."
//! control is visible but inert (ADR-0010).
//!
//! Painted-text assertions come from `tests/common`; harness helpers beyond
//! those remain local to this file (per issue spec: do not edit
//! `tests/shell_frame.rs`).
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, painted_text,
};
use turbogit_app::state::{AppState, CommitSubTab, Dialog};
use turbogit_ui::theme::Palette;
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
    fn subjects(&self) -> Vec<String> {
        git(&self.path, &["log", "--format=%s"])
            .lines()
            .map(str::to_string)
            .collect()
    }
    fn commit_count(&self) -> usize {
        git(&self.path, &["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .unwrap()
    }
}

/// Seed one change of each tracked kind: modified (`M`), staged added (`A`)
/// and unversioned (`?`).
fn seed_changes(repo: &Path) {
    std::fs::write(repo.join("base.txt"), "modified content\n").unwrap();
    std::fs::write(repo.join("added.txt"), "new tracked file\n").unwrap();
    git(repo, &["add", "added.txt"]);
    std::fs::write(repo.join("untracked.txt"), "untracked\n").unwrap();
}

/// Create a real merge conflict in `conf.txt` on `repo`'s default branch.
fn seed_conflict(repo: &Path, branch: &str) {
    git(repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("conf.txt"), "side\n").unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "side commit"]);
    git(repo, &["checkout", "-q", branch]);
    std::fs::write(repo.join("conf.txt"), "main line\n").unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "main commit"]);
    git_unchecked(repo, &["merge", "--no-edit", "side"]); // expected to conflict
}

/// Headless harness over the given repository roots (see CONTEXT.md).
fn app_state(roots: &[PathBuf]) -> AppState {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    AppState::for_roots(&dir, roots)
}

/// Headless harness driving the full app UI with event draining per frame.
/// `max_steps` is raised well above the default because the async diff
/// preview legitimately spins (repainting) while `git diff` runs on a
/// worker thread.
fn harness(state: AppState) -> Harness<'static, AppState> {
    // Generous width so the Commit window's two zones (fixed-width commit
    // panel + diff preview) fit without clipping the multi-root select-all
    // rows; the shell's metadata rail was removed (redesign 03).
    let mut harness = Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1280.0, 800.0));
    // The first frames after startup relayout (embedded fonts take effect
    // at pass 2), so clicks must only happen on a settled frame
    // (test-support `settle`'s rationale).
    test_support::harness::settle(&mut harness);
    harness
}

/// Poll `f` until it returns true or the deadline elapses (worker threads run
/// asynchronously, so completion is observed by polling). `FnMut` so pollers
/// can repaint the harness while waiting (issue 06 stats load).
fn wait_until<F: FnMut() -> bool>(ms: u64, mut f: F) -> bool {
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

/// The primary Commit action's main part (issue 07: the `Commit ▾` split
/// button). Labeled via `WidgetInfo` — the panel heading also says "Commit",
/// so tests target the unambiguous accessible label "Commit changes".
fn commit_action_button<'h>(h: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    h.get_by_label("Commit changes")
}

/// Open the split button's dropdown (the ▾ chevron) and return the harness,
/// so the caller can click a menu item (e.g. "Commit and Push...") by label.
fn open_commit_split_dropdown(h: &Harness<'_, AppState>) {
    h.get_by_label("Commit split options").click();
}

fn commit_button_is_disabled(h: &Harness<'_, AppState>) -> bool {
    commit_action_button(h).accesskit_node().is_disabled()
}

// ------------------------------------------------------------------ tests --

#[test]
fn canonical_groups_count_badges_and_status_rows_paint() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "paint-repo");
    let branch = repo.branch();
    seed_changes(&repo.path);
    seed_conflict(&repo.path, &branch);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Issue 07: the staging section headers are gone — file rows paint
    // directly under the repo group; only the `Merge conflicts` group keeps
    // its count-badged header. Row badges still match the file states.
    h.get_by_label("base.txt");
    h.get_by_label("added.txt");
    h.get_by_label("Merge conflicts (1)");
    h.get_by_label("C conf.txt");

    // Untracked rows live in the bottom group of the same tree.
    h.get_by_label("Unversioned Files (1)");
    h.get_by_label("untracked.txt");

    // Single-root project: no root sub-groups / select-all checkboxes.
    assert!(
        h.query_by_label("Select all paint-repo").is_none(),
        "select-all must only appear for multi-root projects"
    );
}

#[test]
fn file_row_checkbox_toggles_commit_inclusion() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "toggle-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    let p = repo.path.join("base.txt");

    assert!(!h.state().ui.selected.contains(&p));

    h.get_by_label("Select base.txt").click();
    h.run();
    assert!(
        h.state().ui.selected.contains(&p),
        "checking a row includes the change"
    );

    h.get_by_label("Select base.txt").click();
    h.run();
    assert!(
        !h.state().ui.selected.contains(&p),
        "unchecking excludes the change again"
    );
}

#[test]
fn commit_stays_disabled_without_message_or_selection_and_enables_with_both() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "gate-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Empty message AND empty selection → disabled.
    assert!(commit_button_is_disabled(&h));

    // Message set but selection still empty → still disabled (asserted).
    h.state_mut().ui.commit_message = "has message".into();
    h.run();
    assert!(
        commit_button_is_disabled(&h),
        "Commit must stay disabled while no change is included"
    );

    // Selection made but message cleared → still disabled (asserted).
    h.state_mut().ui.commit_message.clear();
    h.get_by_label("base.txt").click();
    h.run();
    assert!(
        commit_button_is_disabled(&h),
        "Commit must stay disabled while the message is empty"
    );

    // Both present → enabled.
    h.state_mut().ui.commit_message = "ready".into();
    h.run();
    assert!(
        !commit_button_is_disabled(&h),
        "Commit must enable with a non-empty message and at least one included change"
    );
}

#[test]
fn commit_executes_for_valid_input() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "commit-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "issue11: real commit".into();
    h.run();
    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"issue11: real commit".to_string())),
        "commit should land in the temp repository, got {:?}",
        repo.subjects()
    );
}

#[test]
fn amend_commits_with_amend_flag() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "amend-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    let before = repo.commit_count();
    assert_eq!(before, 1);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    // Settle the async status snapshot first: clicks issued against early
    // frames are hit-tested on geometry that still shifts as roots load.
    for _ in 0..5 {
        h.run();
    }
    // One click per frame: kittest delivers queued events only at the next
    // run, so two clicks without an intervening run would land together and
    // only the release target would register.
    h.get_by_label("base.txt").click();
    h.run();
    h.get_by_label("Amend").click();
    h.run();
    assert!(h.state().ui.amend, "the Amend checkbox must toggle");
    h.state_mut().ui.commit_message = "amended subject".into();
    h.run();
    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"amended subject".to_string())
            && repo.commit_count() == before),
        "amend must rewrite HEAD without adding a commit (count={}, subjects={:?})",
        repo.commit_count(),
        repo.subjects()
    );
}

#[test]
fn commit_and_push_chains_commit_then_opens_push_dialog() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "push-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "push chain subject".into();
    h.run();
    // Issue 07: "Commit and Push..." lives inside the split button's
    // dropdown instead of as a sibling action.
    open_commit_split_dropdown(&h);
    h.run();
    h.get_by_label("Commit and Push...").click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"push chain subject".to_string())),
        "commit-and-push should first perform the commit"
    );
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Push),
        "commit-and-push must open the push dialog afterwards"
    );
}

/// Track a new file in `repo` so it counts as a modified (M) change when
/// its worktree content diverges afterwards — untracked files render as
/// `? name` rows and would not exercise the modified-row assertions.
fn seed_tracked(repo: &Path, name: &str) {
    std::fs::write(repo.join(name), "tracked\n").unwrap();
    git(repo, &["add", name]);
    git(repo, &["commit", "-q", "-m", &format!("track {name}")]);
    std::fs::write(repo.join(name), "modified\n").unwrap();
}

#[test]
fn repo_groups_render_collapsible_one_tree_with_focus() {
    // Issue 04: per-repo sections collapse into ONE tree — each repo renders
    // as a collapsible group header. Non-focused repos are collapsed by
    // default; the selected repo's files are the visual focus.
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let h = harness(app_state(&[a.path.clone(), b.path.clone()]));

    // Every repo paints as a group header inside the tree.
    h.get_by_label("Toggle repo-a");
    h.get_by_label("Toggle repo-b");

    // Focus = expand: the selected (first) root's file row is visible…
    h.get_by_label("a.txt");
    // …the non-focused repo is collapsed by default so its rows stay hidden.
    assert!(
        h.query_by_label("b.txt").is_none(),
        "non-focused repo-b must be collapsed by default"
    );
}

#[test]
fn manual_toggle_expands_collapsed_repo_group_and_collapses_again() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");
    let b_id = turbogit_domain::model::RootId(b.path.clone().into());

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));
    assert!(
        h.query_by_label("b.txt").is_none(),
        "repo-b starts collapsed"
    );
    assert!(!h.state().ui.changes_expanded.contains(&b_id));

    // Expand the collapsed group: its rows appear and the key is recorded.
    h.get_by_label("Toggle repo-b").click();
    h.run();
    h.get_by_label("b.txt");
    assert!(
        h.state().ui.changes_expanded.contains(&b_id),
        "expanded group must be recorded in changes_expanded"
    );

    // Toggle again: collapsed, key cleared.
    h.get_by_label("Toggle repo-b").click();
    h.run();
    assert!(
        h.query_by_label("b.txt").is_none(),
        "toggling again must collapse the group"
    );
    assert!(!h.state().ui.changes_expanded.contains(&b_id));
}

#[test]
fn sidebar_selection_refocuses_tree_collapsing_other_groups() {
    let parent = tempfile::tempdir().unwrap();
    // Both repos share one subfolder of the project dir, so the sidebar
    // renders a single group ("shared") whose repo rows keep unambiguous
    // labels — with repos as direct children, the sidebar's group header and
    // its repo row would share the same name and collide on label queries.
    let shared = parent.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let a = temp_repo(&shared, "repo-a");
    let b = temp_repo(&shared, "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let mut h = harness(AppState::for_roots(
        parent.path(),
        &[a.path.clone(), b.path.clone()],
    ));
    assert_eq!(
        h.state().selected_root,
        Some(turbogit_domain::model::RootId(a.path.clone().into())),
        "first registered root starts focused"
    );

    // Manually expand repo-b, then select it via its sidebar repo row.
    h.get_by_label("Toggle repo-b").click();
    h.run();
    h.get_by_label("b.txt");
    h.get_by_label("repo-b").click();
    h.run();

    // Focus = expand: the newly selected repo's group is the only expanded
    // one — the previously focused repo-a collapses.
    assert_eq!(
        h.state().selected_root,
        Some(turbogit_domain::model::RootId(b.path.clone().into()))
    );
    h.get_by_label("b.txt");
    assert!(
        h.query_by_label("a.txt").is_none(),
        "selecting repo-b must collapse repo-a's group"
    );
    assert!(
        h.state().ui.changes_expanded.is_empty(),
        "manual expansions reset when the focus moves"
    );
}

#[test]
fn commit_window_layout_is_fixed_panel_plus_flexible_preview() {
    // Redesign 03: the Commit window is exactly two zones — a fixed-width
    // Commit panel on the left and a flexible diff preview on the right.
    // The diff zone is a permanent pane: it paints its heading by default
    // (no Commit/Preview toggle to reveal it) and starts at the fixed
    // panel's edge. With the 280px workspace sidebar visible in the 1280px
    // harness, that edge is SIDEBAR_WIDTH + 340 (COMMIT_PANEL_WIDTH).
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "two-zone-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt");

    let origin = galley_origin(&h, "Preview")
        .expect("the diff preview zone must paint its heading without any toggle");
    let expected_x = turbogit_ui::ui::sidebar::SIDEBAR_WIDTH + 340.0;
    assert!(
        (origin.x - expected_x).abs() < 40.0,
        "the diff preview must start at the fixed commit panel's edge \
         (x≈{expected_x}), got x={}",
        origin.x
    );
}

#[test]
fn commit_message_box_and_amend_sit_at_the_top_of_the_commit_panel() {
    // Issue 07: one set of commit controls for the whole view. The single
    // commit message box + Amend option sit at the TOP of the fixed commit
    // panel — above the tree toolbar, inside the panel's width — instead of
    // at the bottom of the preview pane.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "msg-top-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let panel_right = turbogit_ui::ui::sidebar::SIDEBAR_WIDTH
        + turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH;
    let changes_origin =
        galley_origin(&h, "Changes (1)").expect("the tree toolbar label must paint");

    let msg_origin =
        galley_origin(&h, "Commit message:").expect("the commit message label must paint");
    assert!(
        msg_origin.x < panel_right && msg_origin.y < changes_origin.y,
        "the message box must sit at the top of the fixed commit panel, \
         inside the panel width (x<{panel_right}) and above the tree toolbar \
         (y<{}), got x={} y={}",
        changes_origin.y,
        msg_origin.x,
        msg_origin.y
    );

    // Exactly one subscriber: no per-repo commit controls anywhere.
    let labels = painted_text(&h);
    assert_eq!(
        labels
            .iter()
            .filter(|t| t.as_str() == "Commit message:")
            .count(),
        1,
        "exactly one commit message box must paint"
    );

    // The Amend option travels with the message box, not at the bottom.
    let amend_origin =
        galley_origin(&h, "Amend").expect("the Amend option must paint with the message box");
    assert!(
        amend_origin.y < changes_origin.y,
        "Amend must sit above the tree toolbar, got y={}",
        amend_origin.y
    );
}

#[test]
fn staging_section_headers_and_stage_all_actions_are_removed() {
    // Issue 07: staging lives in the per-file checkboxes (issue 05), so the
    // per-repo `UNSTAGED (n)` / `STAGED (n)` header rows and their `Stage all`
    // / `Unstage all` actions are gone. File rows still paint.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "sectionless-row");
    seed_changes(&repo.path);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    // File rows (and the unversioned group) still paint…
    h.get_by_label("base.txt");
    h.get_by_label("added.txt");
    h.get_by_label("untracked.txt");

    // …but no per-repo staging section headers or stage-all actions anywhere.
    let text = painted_text(&h).join("\n");
    assert!(
        !text.contains("UNSTAGED") && !text.contains("STAGED"),
        "staging section headers must be removed, got:\n{text}"
    );
    assert!(
        !text.contains("Stage all") && !text.contains("Unstage all"),
        "stage-all / unstage-all actions must be removed, got:\n{text}"
    );
}

#[test]
fn merge_conflicts_group_survives_the_section_removal() {
    // Issue 07 (seam decision): only the UNSTAGED/STAGED section headers and
    // their Stage all actions are removed; the distinct `Merge conflicts`
    // group with its C review row stays.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "conflict-kept");
    let branch = repo.branch();
    seed_changes(&repo.path);
    seed_conflict(&repo.path, &branch);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("Merge conflicts (1)");
    h.get_by_label("C conf.txt");
}

#[test]
fn single_primary_commit_split_button_with_grey_shelve_stash() {
    // Issue 07: one primary action only. The panel bottom carries a single
    // `Commit ▾` split button with small grey `Shelve…` / `Stash…` beside it —
    // no more sibling `Commit and Push...` button or cascade queue in the row.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "split-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "split subject".into();
    h.run();

    // The one primary Commit action (main part of the split button).
    let commit = commit_action_button(&h);
    assert!(
        !commit.accesskit_node().is_disabled(),
        "the primary Commit action must be enabled with a message and selection"
    );

    // `Commit and Push...` is NOT a sibling button anymore — the alternatives
    // live inside the split button's dropdown only.
    assert_not_painted(&h, "Commit and Push");

    // Grey Shelve…/Stash… small buttons sit beside the primary action on the
    // same row (one primary action, secondary buttons grey).
    let row_y = commit.rect().center().y;
    for label in ["Shelve…", "Stash…"] {
        let n = h.get_by_label(label);
        assert!(
            (n.rect().center().y - row_y).abs() < 14.0,
            "{label} must sit beside the primary Commit action (y≈{row_y}), got {}",
            n.rect().center().y
        );
        assert!(
            n.rect().left() > commit.rect().right(),
            "{label} to the right"
        );
    }
}

#[test]
fn commit_split_dropdown_offers_commit_and_push_only_in_single_repo_scope() {
    // Issue 07: the split button's dropdown holds the alternative commit
    // surfaces — `Commit and Push...` always. The multi-repo cascade entry
    // stays gated on an active multi-repo selection (the shell shows the
    // selection summary instead of the commit panel while that is live), so
    // in the normal single-scope view it must not appear.
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "dd-a");
    let b = temp_repo(parent.path(), "dd-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));
    // Select the focused repo's file via its checkbox, then arm the message
    // (one click per frame — see the Amend test note).
    h.get_by_label("Select a.txt").click();
    h.run();
    assert!(
        !h.state().ui.selected.is_empty(),
        "the clicked row must be in the commit selection"
    );
    h.state_mut().ui.commit_message = "dd subject".into();
    h.run();

    open_commit_split_dropdown(&h);
    h.run();

    // The dropdown offers commit-and-push…
    h.get_by_label("Commit and Push...");
    // …but no cascade entry without an active multi-repo selection.
    assert_not_painted(&h, "Also commit on");
}

#[test]
fn diff_preview_reflects_selected_file() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "preview-repo");
    seed_changes(&repo.path);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Clicking a file row selects it for the preview pane; the header
    // paints the previewed path (issue 06).
    h.get_by_label("base.txt").click();
    h.run();
    assert_eq!(h.state().ui.preview_change, Some(PathBuf::from("base.txt")));
    h.get_by_label("Previewing base.txt");

    // Selecting another row swaps the preview to that file.
    h.get_by_label("added.txt").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(PathBuf::from("added.txt"))
    );
    h.get_by_label("Previewing added.txt");
}

#[test]
fn diff_header_shows_path_status_chip_stats_and_nav_on_selection() {
    // Issue 06 (design doc §5): selecting a changed file paints the preview
    // header — the path, a `Modified` status chip, `+N −M` change-size
    // stats, and prev/next change navigation over the changed files.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "header-repo");
    // 1 removed / 2 added: a middle line becomes two (verified against git:
    // `-b` then `+x`, `+y`).
    std::fs::write(repo.path.join("base.txt"), "a\nb\nc\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.path.join("base.txt"), "a\nx\ny\nc\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // No selection: no header chrome paints, the hint text shows instead.
    assert_not_painted(&h, "Modified");
    assert!(h.query_by_label("Previewing base.txt").is_none());

    h.get_by_label("base.txt").click();
    h.run();

    // Path, status chip, and nav paint immediately on selection.
    h.get_by_label("Previewing base.txt");
    assert_painted(&h, "Modified");
    h.get_by_label("Previous change");
    h.get_by_label("Next change");

    // The `+2 −1` stats land once the async diff is computed.
    assert!(
        wait_until(15_000, || {
            h.run();
            let text = painted_text(&h).join("\n");
            text.contains("+2") && text.contains("\u{2212}1")
        }),
        "the header must show the +2 −1 change-size stats once the diff is computed"
    );
}

#[test]
fn header_nav_buttons_walk_previous_and_next_changed_files() {
    // Issue 06 (design doc §5): the header's prev/next navigation walks the
    // active sub-tab's changed files — the same path a file-row click uses,
    // so the new diff loads and the header follows. The buttons disable at
    // the ends of the list.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "nav-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("other.txt"), "edited\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();
    assert_eq!(h.state().ui.preview_change, Some(PathBuf::from("base.txt")));

    // First file: Previous is disabled, Next advances.
    let prev = h.get_by_label("Previous change");
    assert!(
        prev.accesskit_node().is_disabled(),
        "Previous must be disabled on the first changed file"
    );
    h.get_by_label("Next change").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(PathBuf::from("other.txt")),
        "Next must walk to the second changed file"
    );
    h.get_by_label("Previewing other.txt");

    // Last file: Next is disabled, Previous returns.
    let next = h.get_by_label("Next change");
    assert!(
        next.accesskit_node().is_disabled(),
        "Next must be disabled on the last changed file"
    );
    h.get_by_label("Previous change").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(PathBuf::from("base.txt")),
        "Previous must walk back to the first changed file"
    );
}

// ------------------------------------------------- issue #18 sub-tabs --

#[test]
fn sub_tab_strip_switches_active_sub_tab_and_restores_local_changes() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "subtab-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // The strip is Local Changes / Shelf / Stash: the Unversioned Files
    // sub-tab is gone (issue 04 merged it into the one tree as a bottom
    // group — unversioned rows live in Local Changes now).
    h.get_by_label("Local Changes");
    h.get_by_label("Shelf");
    h.get_by_label("Stash");
    assert!(
        h.query_by_label("Unversioned Files").is_none(),
        "the Unversioned Files sub-tab must be removed (issue 04)"
    );
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::LocalChanges);
    h.get_by_label("base.txt");

    // Clicking a sub-tab switches the active sub-tab…
    h.get_by_label("Shelf").click();
    h.run();
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::Shelf);

    // …and switching back restores the active changelist data.
    h.get_by_label("Local Changes").click();
    h.run();
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::LocalChanges);
    h.get_by_label("base.txt");
}

#[test]
fn shelf_and_stash_show_labeled_placeholder_panes() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "placeholder-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // No placeholder copy leaks onto the active data views.
    assert_not_painted(&h, "arrives in a later phase");

    h.get_by_label("Shelf").click();
    h.run();
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::Shelf);
    assert_painted(&h, "Shelf arrives in a later phase.");
    // The placeholder replaces the changelist data instead of stacking on it.
    assert_not_painted(&h, "UNSTAGED");
    assert_not_painted(&h, "base.txt");

    h.get_by_label("Stash").click();
    h.run();
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::Stash);
    assert_painted(&h, "Stash arrives in a later phase.");
    assert_not_painted(&h, "base.txt");
}

#[test]
fn unversioned_group_lists_untracked_in_one_tree_includable_in_commit() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "untracked-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("untracked.txt"), "untracked\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::LocalChanges);

    // Issue 04: untracked files stop appearing under the repo's sections and
    // render only in the `Unversioned Files` group at the bottom of the same
    // tree (issue 07 removed the section headers entirely). The modified
    // tracked file and the untracked one paint on the same surface now.
    h.get_by_label("base.txt");
    h.get_by_label("Unversioned Files (1)");
    h.get_by_label("untracked.txt");

    // …and checking one includes it in the next commit.
    h.get_by_label("untracked.txt").click();
    h.state_mut().ui.commit_message = "issue18: include untracked".into();
    h.run();
    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"issue18: include untracked".to_string())),
        "commit should land in the temp repository, got {:?}",
        repo.subjects()
    );
    let last_files = git(&repo.path, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(
        last_files.lines().any(|l| l == "untracked.txt"),
        "the previously-untracked file must be committed, got {last_files:?}"
    );
    assert!(
        !last_files.lines().any(|l| l == "base.txt"),
        "unchecked modifications must stay out of the commit, got {last_files:?}"
    );
}

#[test]
fn unversioned_group_collapses_and_expands_via_header() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "uv-toggle-repo");
    std::fs::write(repo.path.join("untracked.txt"), "untracked\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("untracked.txt");

    // Collapse the bottom group: its rows hide…
    h.get_by_label("Unversioned Files (1)").click();
    h.run();
    assert!(
        h.query_by_label("untracked.txt").is_none(),
        "collapsed Unversioned Files hides its rows"
    );

    // …and expanding brings them back.
    h.get_by_label("Unversioned Files (1)").click();
    h.run();
    h.get_by_label("untracked.txt");
}

// ------------------------------------------- issue 04 tree toolbar row --

// ------------------------------------------- issue 05 file-row redesign --

#[test]
fn file_rows_paint_checkbox_letter_coloured_name_and_dim_location() {
    // Issue 05 (design doc §4): every row reads
    // `[checkbox] [status letter] [coloured filename] … [dim location]`.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "row-layout");
    // A root-level modification, a modified file inside `src/`, and a new
    // untracked file.
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    std::fs::write(repo.path.join("src/app.rs"), "fn main() {}\n").unwrap();
    git(&repo.path, &["add", "src/app.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "track app"]);
    std::fs::write(repo.path.join("src/app.rs"), "fn main() { println!(); }\n").unwrap();
    std::fs::write(repo.path.join("untracked.txt"), "new\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Row bodies are addressable by filename (no more "M base.txt" label).
    h.get_by_label("base.txt");
    h.get_by_label("app.rs");
    h.get_by_label("untracked.txt");
    // Each row carries a checkbox for per-file staging.
    h.get_by_label("Select base.txt");
    h.get_by_label("Select app.rs");
    h.get_by_label("Select untracked.txt");

    // Status letters paint as their own colour-coded elements and the dim
    // location renders after the filename.
    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("src/"),
        "rows must paint the dim location, got:\n{text}"
    );
    // Hunk counts moved out of rows into the diff header (issue 05/06).
    assert!(
        !text.contains("hunk"),
        "rows must not paint hunk counts, got:\n{text}"
    );
}

#[test]
fn row_selection_unifies_commit_inclusion_preview_and_selection_fill() {
    // Issue 05 (design doc §4): selecting a row checks its checkbox AND
    // previews the diff — "what I'm committing and what I'm looking at are
    // the same thing" — and the selected row paints the solid #2E436E
    // selection background.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "select-row");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("other.txt"), "edit\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    let p = repo.path.join("base.txt");
    assert!(!h.state().ui.selected.contains(&p));

    // Clicking the row selects it for the commit AND previews its diff.
    h.get_by_label("base.txt").click();
    h.run();
    assert!(
        h.state().ui.selected.contains(&p),
        "a clicked row must be in the commit selection"
    );
    assert_eq!(
        h.state().ui.preview_change,
        Some(PathBuf::from("base.txt")),
        "a clicked row must drive the diff preview"
    );

    // The selected row paints the solid design-blue selection background.
    let row_origin = galley_origin(&h, "base.txt").expect("row paints its filename");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::SELECTION_BG && r.contains(row_origin)),
        "the selected row must paint the solid #2E436E selection background"
    );

    // Unchecking via the checkbox removes it from the commit…
    h.get_by_label("Select base.txt").click();
    h.run();
    assert!(
        !h.state().ui.selected.contains(&p),
        "the checkbox must toggle commit inclusion"
    );
    assert!(
        !filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::SELECTION_BG && r.contains(row_origin)),
        "an unchecked row must lose the selection background"
    );
    // …without moving the preview.
    assert_eq!(
        h.state().ui.preview_change,
        Some(PathBuf::from("base.txt")),
        "the checkbox must not change what is previewed"
    );
}

#[test]
fn tree_toolbar_shows_changes_label_total_and_icon_controls() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let h = harness(app_state(&[a.path.clone(), b.path.clone()]));

    // Label + total count (one change per repo = 2 files).
    assert_painted(&h, "Changes (2)");
    // Icon-only controls, all reachable by label (issue 04 checklist).
    h.get_by_label("Expand all groups");
    h.get_by_label("Group by");
    h.get_by_label("Rollback");
    h.get_by_label("Refresh changes");
    // The old text buttons are gone (issue 05 removes them; issue 04 ships
    // the icon-only row).
    assert_not_painted(&h, "Stage selected");
    assert_not_painted(&h, "Unstage selected");
}

#[test]
fn expand_collapse_all_toggles_every_repo_group() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");
    let b_id = turbogit_domain::model::RootId(b.path.clone().into());

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));
    assert!(
        h.query_by_label("b.txt").is_none(),
        "repo-b starts collapsed"
    );

    // Expand all: every non-focused group opens.
    h.get_by_label("Expand all groups").click();
    h.run();
    h.get_by_label("b.txt");
    assert!(
        h.state().ui.changes_expanded.contains(&b_id),
        "expand-all must record every non-focused group"
    );
    // The control flips to its collapse action while groups are open.
    h.get_by_label("Collapse all groups").click();
    h.run();
    assert!(
        h.query_by_label("b.txt").is_none(),
        "collapse-all closes every non-focused group"
    );
    assert!(h.state().ui.changes_expanded.is_empty());
}

#[test]
fn rollback_icon_confirms_discard_and_toasts_when_empty() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "rollback-repo");
    seed_tracked(&repo.path, "a.txt");

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Nothing selected: rollback warns without opening the confirm.
    h.get_by_label("Rollback").click();
    h.run();
    assert!(
        h.state().ui.confirm.is_none(),
        "rollback with an empty selection must not open the discard confirm"
    );

    // A selected file: rollback opens the destructive confirm.
    h.get_by_label("a.txt").click();
    h.run();
    h.get_by_label("Rollback").click();
    h.run();
    match &h.state().ui.confirm {
        Some(turbogit_app::state::PendingConfirm::Discard { changes }) => {
            assert_eq!(changes.len(), 1, "one selected file enters the confirm");
        }
        None => panic!("expected a Discard confirm, got none"),
        Some(_) => panic!("expected a Discard confirm, got a different confirm"),
    }
}

#[test]
fn group_by_icon_is_inert() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "groupby-repo");
    seed_tracked(&repo.path, "a.txt");

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("Group by").click();
    h.run();

    // Clicking the group-by icon must change no observable state (ADR-0010
    // inert-control pattern, same as "Advanced options...").
    let s = h.state();
    assert_eq!(s.ui.commit_subtab, CommitSubTab::LocalChanges);
    assert!(s.ui.dialog.is_none());
    assert!(s.ui.confirm.is_none());
    assert!(s.ui.selected.is_empty());
}

#[test]
fn refresh_icon_refreshes_changes() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "refresh-repo");
    seed_tracked(&repo.path, "a.txt");

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    let gen_before = h.state().ui.pane_generation;

    h.get_by_label("Refresh changes").click();
    h.run();

    assert!(
        h.state().ui.pane_generation > gen_before,
        "refresh must dispatch the full scoped refresh (pane generation bump)"
    );
}

#[test]
fn advanced_options_link_is_removed_into_the_tree_toolbar_gear() {
    // Issue 07: the `advanced options…` link is gone; the panel's gear icon
    // in the tree toolbar carries the options. Following the ADR-0010 inert
    // pattern, clicking the gear must change no observable state (the options
    // surface has no backing feature yet — same as Group by).
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "gear-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // The link is removed from the commit controls…
    assert_not_painted(&h, "Advanced options");

    // …and the gear icon lives in the tree toolbar row.
    let gear = h.get_by_label("Commit options");

    #[derive(Debug)]
    struct CommitUiSnap {
        subtab: CommitSubTab,
        dialog: Option<Dialog>,
        message: String,
        amend: bool,
        selected_len: usize,
        toast: Option<turbogit_app::state::Toast>,
        busy: bool,
    }
    fn snap(h: &Harness<'_, AppState>) -> CommitUiSnap {
        let s = h.state();
        CommitUiSnap {
            subtab: s.ui.commit_subtab,
            dialog: s.ui.dialog,
            message: s.ui.commit_message.clone(),
            amend: s.ui.amend,
            selected_len: s.ui.selected.len(),
            toast: s.ui.toast.clone(),
            busy: s.ui.busy,
        }
    }

    let before = snap(&h);
    gear.click();
    h.run();
    let after = snap(&h);
    assert_eq!(after.subtab, before.subtab);
    assert_eq!(after.dialog, before.dialog);
    assert_eq!(after.message, before.message);
    assert_eq!(after.amend, before.amend);
    assert_eq!(after.selected_len, before.selected_len);
    assert_eq!(after.toast.is_some(), before.toast.is_some());
    assert_eq!(after.busy, before.busy);
}

// ------------------------------------ long file names wrap onto a new line --

/// `(origin, size, line count)` of the painted galley carrying exactly
/// `text`.
///
/// The shared painted-text helpers expose a galley's text and origin only,
/// and egui keeps the *input* string on a wrapped galley
/// (`Galley::text` is documented as "the full, non-elided text of the input
/// job"), so a wrap is only observable from the galley's geometry: several
/// laid-out rows, none wider than the sheet's name column.
fn galley_metrics(h: &Harness<'_, AppState>, text: &str) -> (egui::Pos2, egui::Vec2, usize) {
    h.output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.text() == text => {
                Some((shape.pos, shape.galley.size(), shape.galley.rows.len()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no painted galley carries {text:?}"))
}

/// Height of the selection fill painted under `origin` — the selected row's
/// own rect, so a row that wrapped a name is measurably taller than the
/// single-line file row.
fn selected_row_height(h: &Harness<'_, AppState>, origin: egui::Pos2) -> f32 {
    filled_rects(h)
        .into_iter()
        .find(|(r, c)| *c == Palette::SELECTION_BG && r.contains(origin))
        .expect("the selected row paints the solid #2E436E selection fill")
        .0
        .height()
}

#[test]
fn long_file_names_wrap_onto_a_continuation_line_and_grow_the_row() {
    // A filename wider than the commit panel's name column wraps onto the
    // next line — the row grows with it — instead of running past the pane
    // edge or painting over the dim location column.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "wrap-long-name");
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    let long = "an_extremely_long_file_name_that_cannot_fit_the_commit_panel_column.rs";
    let path = repo.path.join("src").join(long);
    std::fs::write(&path, "one\n").unwrap();
    git(&repo.path, &["add", "-A"]);
    git(&repo.path, &["commit", "-q", "-m", "track long name"]);
    std::fs::write(&path, "two\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // The row is still addressable by its file name…
    h.get_by_label(long);

    // …and the name is laid out across several lines inside its column.
    let (name_pos, name_size, name_lines) = galley_metrics(&h, long);
    assert!(
        name_lines >= 2,
        "a name too wide for its column must wrap onto the next line, \
         got {name_lines} line(s)"
    );

    // The dim location keeps its right-aligned first-line seat: it paints to
    // the right of the name, on the name's first line.
    let (loc_pos, ..) = galley_metrics(&h, "src/");
    assert!(
        loc_pos.x > name_pos.x,
        "the location column must stay right of the name (name x={}, location x={})",
        name_pos.x,
        loc_pos.x
    );
    assert!(
        (loc_pos.y - name_pos.y).abs() < 1.0,
        "the location must ride the name's first line (name y={}, location y={})",
        name_pos.y,
        loc_pos.y
    );
    assert!(
        name_size.x <= loc_pos.x - name_pos.x,
        "every wrapped line must stay inside the name column, left of the \
         location (name width={}, column={})",
        name_size.x,
        loc_pos.x - name_pos.x
    );

    // Selecting the row shows the grown row: the selection fill is taller
    // than the single-line file-row height.
    h.get_by_label(&format!("Select {long}")).click();
    h.run();
    let height = selected_row_height(&h, name_pos);
    assert!(
        height > turbogit_ui::theme::FILE_ROW_HEIGHT,
        "a wrapped row must grow past {} px, got {height}",
        turbogit_ui::theme::FILE_ROW_HEIGHT
    );
}

#[test]
fn renamed_rows_move_the_old_path_below_a_wrapped_new_name() {
    // Spec R8 keeps the muted arrow + old path on the row. When the new name
    // wraps, that marker moves onto its own line under the name instead of
    // colliding with the continuations (or with the location column).
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "wrap-rename");
    let long = "a_renamed_file_whose_new_name_cannot_fit_the_commit_panel_column.rs";
    std::fs::write(repo.path.join("old.txt"), "content\n").unwrap();
    git(&repo.path, &["add", "-A"]);
    git(&repo.path, &["commit", "-q", "-m", "track old name"]);
    git(&repo.path, &["mv", "old.txt", long]);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let (name_pos, name_size, name_lines) = galley_metrics(&h, long);
    assert!(
        name_lines >= 2,
        "the renamed-to name must wrap, got {name_lines} line(s)"
    );
    // The old path paints on the line after the name's last line.
    let (orig_pos, ..) = galley_metrics(&h, "old.txt");
    assert!(
        orig_pos.y >= name_pos.y + name_size.y - 1.0,
        "the renamed-from path must follow the wrapped name on its own line \
         (name {}..{}, old path y={})",
        name_pos.y,
        name_pos.y + name_size.y,
        orig_pos.y
    );
}

#[test]
fn short_file_names_keep_the_single_line_row_height() {
    // The wrap must not disturb the common case: a name that fits stays on
    // one line, on exactly the design's 24 px row.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "wrap-short-name");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    let (origin, _, lines) = galley_metrics(&h, "base.txt");
    assert_eq!(lines, 1, "a short name must stay on one line");

    h.get_by_label("Select base.txt").click();
    h.run();
    assert_eq!(
        selected_row_height(&h, origin),
        turbogit_ui::theme::FILE_ROW_HEIGHT,
        "a single-line row keeps the 24 px file-row height"
    );
}
