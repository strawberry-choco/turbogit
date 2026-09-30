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
use test_support::git_seed::git;
use test_support::harness::{
    assert_not_painted, assert_painted, filled_rects, galley_origin, painted_galleys, painted_ink,
    painted_paths, painted_text, shell_harness_over_unstyled, stroked_rects,
};
use turbogit_app::state::{AppState, CommitSubTab, Dialog};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::widgets::{CHIP_HEIGHT, PANE_HEADER_HEIGHT};

/// Run `git` without asserting success (for commands that may legitimately
/// fail, e.g. a merge that conflicts).
///
/// Deliberately NOT `git_seed::git`, which asserts: this exists for the command
/// whose failure IS the fixture (`seed_conflict`'s `git merge --no-edit side`).
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
///
/// Kept local, NOT `git_seed::repo_with_one_commit`: the seeded file's NAME is
/// load-bearing (`seed_changes` rewrites `base.txt` as a modification, and half
/// the suite asserts on the `M` badge), and the unqualified `git init -q` leaves
/// the default branch as the machine's, which `Repo::branch` reads back.
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
///
/// ## This harness does **not** configure the app's style — read this before
/// trusting it for anything egui-native
///
/// [`turbogit_ui::theme::configure_style`] is called in `src/app.rs` and **nowhere
/// else**, so this harness — like most of the suites that drive
/// `turbogit_ui::ui::render` directly — renders egui's *own* widgets with egui's
/// stock dark visuals rather than with the app's tokens and spacing. The custom
/// vocabulary is unaffected, because every shared widget paints from
/// `Palette::*` itself; what differs is everything that leans on `egui::style()`:
/// a bare `ui.button`, a `TextEdit`, a combo box, a scrollbar, a menu frame, and
/// the **metrics** — `item_spacing`, `button_padding`, `window_margin`,
/// `text_styles`. Ticket 11 observed this while reconciling the header geometry
/// and reported it rather than fixing it.
///
/// The harness is deliberately the **unstyled** one: no `configure_style`, no
/// `install_fonts`. Calling `configure_style` here would shift `item_spacing` and
/// `button_padding` for the ~40 tests in this file, and every assertion below
/// about a `ui.button`, `TextEdit`, combo box, scrollbar or menu frame is
/// measured against egui's defaults. This file is not a proxy for any
/// egui-native widget.
fn harness(state: AppState) -> Harness<'static, AppState> {
    // Generous width so the Commit window's two zones (fixed-width commit
    // panel + diff preview) fit without clipping the multi-root select-all
    // rows. `1024` is the stated `max_steps` rather than kittest's default of 4,
    // because `Harness::run` PANICS once a run exceeds it.
    let mut harness = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
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

// ------------------------- the changes header (design system v2, R7) ---------

/// The changes card's header title, as the shared pane header printed it.
///
/// Located by its own string rather than by being "the first galley": a header
/// band holds the title, a count chip and an action slot, and the chip's
/// 10px mono number is painted *above* the 9px title because both are centred
/// in the same band. "First painted" would find the number.
///
/// The panic spells out the mistake it is looking for, because a heading of
/// `CHANGES (3)` is the exact regression this lookup exists to catch and the
/// bare "not painted" would send a reader looking for a font problem.
fn changes_title_galley(h: &Harness<'_, AppState>) -> test_support::harness::PaintedGalley {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text == "CHANGES")
        .unwrap_or_else(|| {
            panic!(
                "the changes card must be headed by the shared pane header's own \
                 title, `CHANGES`. No such text painted, so the heading is \
                 something else — a `CHANGES (n)` title is the count interpolated \
                 into the heading, and the count belongs in a count chip BESIDE it."
            )
        })
}

/// The hairline the shared pane header paints at its band's last pixel: a 1px
/// `RULE_STRUCTURAL` fill, found by sitting immediately under the header's title
/// rather than by being the first one painted.
///
/// Scoped by position because the frame is full of rules — a `ui.separator()`
/// is a `Line`, not a fill, but the *only* `RULE_STRUCTURAL` fills in this
/// window are pane-header hairlines, and a new one must not be mistaken for
/// this card's.
fn changes_header_rule(h: &Harness<'_, AppState>) -> egui::Rect {
    let title = changes_title_galley(h);
    filled_rects(h)
        .into_iter()
        .filter(|(rect, fill)| {
            *fill == Palette::RULE_STRUCTURAL && (rect.height() - 1.0).abs() < 0.01
        })
        .map(|(rect, _)| rect)
        .filter(|rect| rect.top() >= title.pos.y + turbogit_ui::theme::TYPE_SECTION)
        .min_by(|a, b| a.top().partial_cmp(&b.top()).expect("finite rule top"))
        .unwrap_or_else(|| {
            panic!(
                "the changes header paints no structural hairline under its title at {:?}",
                title.pos
            )
        })
}

/// The header's own band: the 28pt strip above its hairline, spanning the
/// hairline's width. Derived from paint, so "a control is *in the header*" is a
/// measured claim rather than a claim about which function was called.
fn changes_header_band(h: &Harness<'_, AppState>) -> egui::Rect {
    let rule = changes_header_rule(h);
    egui::Rect::from_min_max(
        egui::pos2(rule.left(), rule.top() - PANE_HEADER_HEIGHT),
        rule.max,
    )
}

/// The accessible labels of every Button whose rect lies inside `band`,
/// sorted, so the set is comparable without depending on the tree's walk order.
fn button_labels_in(band: egui::Rect, h: &Harness<'_, AppState>) -> Vec<String> {
    let mut labels: Vec<String> = h
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|node| band.contains_rect(node.rect()))
        .filter_map(|node| node.accesskit_node().label())
        .collect();
    labels.sort();
    labels
}

/// The count chip the shared pane header paints: the one chip-height
/// `RAISED`-filled rect in the header band, at the chip radius — the shared
/// count chip, not a fourth badge and not a parenthesised number inside the
/// heading.
///
/// Scoped to `band` because the sidebar wears the same shared count chip: a
/// frame-wide count here would describe how many chips the window happens to
/// show rather than what this header is wearing.
fn changes_count_chip(h: &Harness<'_, AppState>) -> (egui::Rect, egui::CornerRadius) {
    let band = changes_header_band(h);
    let hits: Vec<_> = painted_rects(h)
        .into_iter()
        .filter(|(rect, fill, _, radius)| {
            *fill == Palette::RAISED
                && (rect.height() - CHIP_HEIGHT).abs() < 0.01
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
                && band.contains_rect(*rect)
        })
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one count chip in the changes header, found {hits:#?}"
    );
    let (rect, _, _, radius) = hits.into_iter().next().expect("one count chip");
    (rect, radius)
}

/// Every stroked rectangle the frame painted, including the members of a
/// `Shape::Vec`.
///
/// `test_support::harness::stroked_rects` sees `Shape::Rect` only, and a menu's
/// frame is a `Shape::Vec` — its shadow plus its rounded fill-and-stroke rect —
/// so the one stroke R2 allows on a floating surface is invisible to the shared
/// helper. This is its sibling, for the frames that float.
fn stroked_rects_including_vec(h: &Harness<'_, AppState>) -> Vec<(egui::Rect, egui::Color32, f32)> {
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        let members: Vec<&egui::Shape> = match &clipped.shape {
            egui::Shape::Rect(rs) => {
                out.push((rs.rect, rs.stroke.color, rs.stroke.width));
                continue;
            }
            egui::Shape::Vec(shapes) => shapes.iter().collect(),
            _ => continue,
        };
        for shape in members {
            if let egui::Shape::Rect(rs) = shape {
                out.push((rs.rect, rs.stroke.color, rs.stroke.width));
            }
        }
    }
    out
}

/// Everything an inert header control could possibly have touched, as a type
/// with `PartialEq` — so "pressing it changed nothing" is one assertion over the
/// whole surface rather than a list a future field can be added past.
///
/// `busy` and `pane_generation` are the two that make the claim strong: every
/// `AppState::dispatch` sets `busy` and every refresh bumps the generation, so a
/// control that dispatched *anything* moves at least one of them.
#[derive(Debug, PartialEq, Eq, Clone)]
struct InertSnapshot {
    subtab: CommitSubTab,
    dialog: Option<Dialog>,
    confirm_open: bool,
    message: String,
    amend: bool,
    selected: Vec<PathBuf>,
    expanded: Vec<String>,
    filter: String,
    preview: Option<PathBuf>,
    toast_kind: Option<turbogit_app::state::ToastKind>,
    toast_message: Option<String>,
    busy: bool,
    pane_generation: u64,
    last_error: Option<String>,
}

fn inert_snapshot(h: &Harness<'_, AppState>) -> InertSnapshot {
    let s = h.state();
    let mut expanded: Vec<String> = s.ui.changes_expanded.iter().map(|id| id.name()).collect();
    expanded.sort();
    let mut selected: Vec<PathBuf> = s.ui.selected.iter().cloned().collect();
    selected.sort();
    InertSnapshot {
        subtab: s.ui.commit_subtab,
        dialog: s.ui.dialog,
        confirm_open: s.ui.confirm.is_some(),
        message: s.ui.commit_message.clone(),
        amend: s.ui.amend,
        selected,
        expanded,
        filter: s.ui.file_filter.clone(),
        preview: s.ui.preview_change.clone(),
        toast_kind: s.ui.toast.as_ref().map(|t| t.kind),
        toast_message: s.ui.toast.as_ref().map(|t| t.message.clone()),
        busy: s.ui.busy,
        pane_generation: s.ui.pane_generation,
        last_error: s.last_error.clone(),
    }
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

/// **The commit column width is unchanged, and no other pane's width moved.**
///
/// The header shed two icons rather than borrowing width, so this is a
/// regression gate with a number in it: `COMMIT_PANEL_WIDTH` is 340 and lives in
/// the commit-window module (it is deliberately *not* a theme token), and the
/// split between the two zones is measured at that width.
///
/// The split is asserted *relatively* — preview edge minus changes edge equals
/// `COMMIT_PANEL_WIDTH` — rather than against `SIDEBAR_WIDTH`. The sidebar's
/// width belongs to another surface; what this test owns is that the column
/// between the two cards did not move, which is exactly the claim "no other
/// pane's width moves" makes from this side.
#[test]
fn commit_window_layout_is_fixed_panel_plus_flexible_preview() {
    // Redesign 03: the Commit window is exactly two zones — a fixed-width
    // Commit panel on the left and a flexible diff preview on the right.
    // The diff zone is a permanent pane: it paints its heading by default
    // (no Commit/Preview toggle to reveal it) and starts at the fixed
    // panel's edge.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "two-zone-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt");

    // The width itself: unchanged, and still owned by this module.
    assert_eq!(
        turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH,
        340.0,
        "the commit column keeps its width. The header shed two icons and gave \
         the filter its own row; it did not borrow width from anywhere."
    );

    // The changes card *is* the column: it fills the fixed width exactly.
    let changes_card = card_rect(&h, changes_title_galley(&h).pos);
    assert_eq!(
        changes_card.width(),
        turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH,
        "the changes card must fill the {}-wide commit column, got {changes_card:?}",
        turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH
    );

    // …and the preview zone starts exactly one column to its right.
    let preview = galley_origin(&h, "Preview")
        .expect("the diff preview zone must paint its heading without any toggle");
    let expected_x = changes_card.left()
        + turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH
        + turbogit_ui::theme::PANEL_PADDING;
    assert!(
        (preview.x - expected_x).abs() < 0.5,
        "the diff preview must start at the fixed commit panel's edge \
         (x={expected_x}), got x={}",
        preview.x
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
    let changes_origin = changes_title_galley(&h).pos;

    let msg_origin =
        galley_origin(&h, "Commit message:").expect("the commit message label must paint");
    assert!(
        msg_origin.x < panel_right && msg_origin.y < changes_origin.y,
        "the message box must sit at the top of the fixed commit panel, \
         inside the panel width (x<{panel_right}) and above the changes header \
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
        "Amend must sit above the changes header, got y={}",
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
            .any(|(r, c)| *c == Palette::ROW_SELECTED && r.contains(row_origin)),
        "the selected row must paint the list-row selection fill (ticket 03)"
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
            .any(|(r, c)| *c == Palette::ROW_SELECTED && r.contains(row_origin)),
        "an unchecked row must lose the selection background"
    );
    // …without moving the preview.
    assert_eq!(
        h.state().ui.preview_change,
        Some(PathBuf::from("base.txt")),
        "the checkbox must not change what is previewed"
    );
}

/// **The changes card is headed by the shared pane header, and its action slot
/// holds exactly three controls.** R7 measured on a real screen: a 9px tracked
/// `INK_3` title, the file count as a `count_chip` *beside* it, one
/// `RULE_STRUCTURAL` hairline, and a right-aligned slot holding the three
/// controls that actually do something.
///
/// The fourth name in the expected set is the `⋯` that opens the overflow menu
/// the two inert controls moved into. It is an affordance, not a fourth thing
/// this pane can do — and it is named here rather than left implicit, so the
/// list below is the whole band and a new icon cannot slip into it unnoticed.
#[test]
fn the_changes_header_is_the_shared_pane_header_with_three_controls() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let h = harness(app_state(&[a.path.clone(), b.path.clone()]));

    // The count is a chip BESIDE the heading, never characters inside it. Checked
    // as painted strings FIRST, so a title of `CHANGES (2)` fails on the claim
    // it is rather than on "the title is missing".
    let painted = painted_text(&h);
    assert!(
        !painted.iter().any(|t| t.contains("Changes (")),
        "the file count must not be characters inside the heading text; painted: {painted:?}"
    );
    assert!(
        painted.contains(&"CHANGES".to_owned()),
        "the heading must paint as the bare pane title `CHANGES`, with the count \
         beside it; painted: {painted:?}"
    );

    // The heading: the pane's own word, in the app's one case, at the shared
    // 9px title size in the muted ink — the same treatment the worktrees,
    // submodules and log panes wear.
    let title = changes_title_galley(&h);
    assert_eq!(painted_ink(&h, "CHANGES"), Some(Palette::INK_3));
    assert_eq!(
        painted_font(&h, "CHANGES").size,
        turbogit_ui::theme::TYPE_SECTION,
        "the heading is the shared 9px pane title, not a body-size label"
    );

    // The count, as the shared count chip beside the heading: one change per
    // repo, so `2`.
    let (chip, radius) = changes_count_chip(&h);
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert!(
        title.rect.right() <= chip.left(),
        "the count chip sits BESIDE the heading, not inside it: title {:?}, chip {chip:?}",
        title.rect
    );
    // Scoped by position: "2" is a string several parts of a frame can paint.
    let chip_ink = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "2" && chip.contains_rect(g.rect))
        .expect("the count paints inside its chip")
        .color;
    assert_eq!(
        chip_ink,
        Palette::INK_2,
        "the count chip carries the shared secondary ink, not the muted one"
    );

    // **Exactly three controls**, in a right-aligned slot hard against the
    // band's trailing edge.
    let band = changes_header_band(&h);
    assert_eq!(
        button_labels_in(band, &h),
        vec![
            "Changes header overflow",
            "Expand all groups",
            "Refresh changes",
            "Rollback",
        ],
        "the changes header's band holds the three live controls (refresh, \
         rollback, expand/collapse-all) plus the `⋯` that opens the overflow \
         menu the two inert ones moved into — and nothing else. A fifth name \
         here means the five-icon row is coming back."
    );
    // The two that moved paint no button anywhere in the frame while their
    // menu is closed.
    assert!(
        h.query_by_label("Commit options").is_none(),
        "commit options paints no button in the header — it is a menu row now"
    );
    assert!(
        h.query_by_label("Group by").is_none(),
        "group by paints no button in the header — it is a menu row now"
    );

    // One hairline, at the band's foot, spanning the header's own width.
    let rule = changes_header_rule(&h);
    assert!(
        (rule.top() - band.top() - PANE_HEADER_HEIGHT).abs() < 0.01
            && (rule.bottom() - band.bottom()).abs() < 0.01,
        "the hairline is the band's last pixel: band {band:?}, rule {rule:?}"
    );
    assert!(
        (rule.left() - title.rect.left()).abs() < 0.01,
        "the hairline starts where the title starts: rule {rule:?}, title {:?}",
        title.rect
    );

    // **No stroke.** The card is a surface, so nothing in it draws a hairline
    // around its own band; the only rule the header has is its own fill.
    let strokes: Vec<_> = stroked_rects(&h)
        .into_iter()
        .filter(|(rect, _, _)| band.intersects(*rect))
        .collect();
    assert!(
        strokes.is_empty(),
        "the pane header paints no stroke — a 1px outline here would be a second \
         way of saying the same thing as the one hairline, and the only surfaces \
         allowed a stroke are the ones that float. Found {strokes:?}"
    );

    // The old text buttons are gone (issue 05 removes them; issue 04 ships
    // the icon-only row).
    assert_not_painted(&h, "Stage selected");
    assert_not_painted(&h, "Unstage selected");
}

/// **The two that cannot act moved into the header's overflow menu, where they
/// stay reachable and stay honestly inert.**
///
/// "Visibly inert" is asserted three ways, all from what the frame produced: the
/// accessibility node is `is_disabled()`, the label's paint-time ink is 45 % of
/// `INK` (the shared disabled treatment), and the reason is reachable on hover.
/// "Inert" is then asserted the strong way — pressing either changes *nothing*,
/// across every field a control could have reached, and leaves the repository
/// alone.
///
/// The menu keeps its 1px stroke because it is a floating layer (R2): that is the
/// rule being satisfied, not a missed one.
#[test]
fn the_inert_header_controls_are_inert_overflow_items_that_dispatch_nothing() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "overflow-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // The `advanced options…` link is still gone (issue 07), and the gear lives
    // in the menu rather than in the header.
    assert_not_painted(&h, "Advanced options");
    assert!(h.query_by_label("Commit options").is_none());
    assert!(h.query_by_label("Group by").is_none());

    let commits_before = repo.commit_count();
    let subjects_before = repo.subjects();

    h.get_by_label("Changes header overflow").click();
    h.run();

    let items: [(&str, &str); 2] = [
        ("Commit options", "No commit-options surface yet (ADR-0010)"),
        ("Group by", "No grouping surface yet (ADR-0010)"),
    ];
    let mut item_rects = Vec::new();
    for (label, _) in items {
        let node = h.get_by_label(label);
        assert!(
            node.accesskit_node().is_disabled(),
            "`{label}` is a not-yet-shipped flow (ADR-0010), so its row must say \
             so in the accessibility tree rather than pretending to be live"
        );
        let rect = node.rect();
        // The row keeps its own glyph: a gated action is still a row, not a bare
        // string, so the icon is painted inside it.
        assert!(
            painted_paths(&h)
                .iter()
                .any(|(glyph, _)| rect.intersects(*glyph)),
            "`{label}` must keep its icon glyph inside its row {rect:?}"
        );
        item_rects.push(rect);
    }

    // The label is painted at 45 % of `INK`. Color32 stores premultiplied, so
    // the spec facts are the un-multiplied channels: INK's rgb, and an alpha of
    // 115 = round(255 × 0.45).
    for (label, _) in items {
        let galley = painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == label)
            .unwrap_or_else(|| panic!("`{label}` is never hidden — a gated action stays rendered"));
        let [r, g, b, a] = galley.color.to_srgba_unmultiplied();
        assert_eq!(a, 115, "`{label}` paints at the shared 45 % disabled ink");
        let near = |x: u8, y: u8| (x as i32 - y as i32).abs() <= 2;
        assert!(
            near(r, 0xDF) && near(g, 0xE1) && near(b, 0xE5),
            "`{label}` keeps INK's hue at 45 %, got {r:#04x}/{g:#04x}/{b:#04x}",
        );
    }

    // The menu is a floating layer, so it keeps its 1px stroke — R2's "a
    // stroke means this floats" holding for exactly the surface that floats.
    // A borderless menu would be a missed rule, not a clean one.
    //
    // The *token* is not asserted here, and deliberately: this suite drives
    // `ui::render` directly and never installs the app's `Visuals`
    // (`theme::configure_style` is wired in `src/app.rs`), so the popup frame
    // wears egui's stock chrome colour here. The token mapping
    // `SURFACE` fill + `LINE` stroke is `theme::dark_visuals`'s and is pinned
    // there; what belongs to this test is the shape claim — a 1px outline
    // around the rows, not a bare surface.
    let border = stroked_rects_including_vec(&h)
        .into_iter()
        .find(|(rect, _, width)| {
            *width == 1.0 && item_rects.iter().all(|item| rect.contains_rect(*item))
        });
    assert!(
        border.is_some(),
        "an open menu floats above the card, so it wears the 1px hairline R2 \
         reserves for floating surfaces; a borderless menu would be a missed \
         rule, not a clean one. Item rects: {item_rects:?}"
    );

    // The reason is reachable on hover — a blocked row explains itself rather
    // than sitting there mute.
    h.hover_at(item_rects[0].center());
    let mut reason_painted = false;
    for _ in 0..20 {
        h.step();
        if painted_text(&h).iter().any(|t| t.contains(items[0].1)) {
            reason_painted = true;
            break;
        }
    }
    assert!(
        reason_painted,
        "hovering a gated item must state why it is gated (`{}`)",
        items[0].1
    );

    // **The proof: pressing either dispatches nothing.** One snapshot over every
    // field a control could reach, compared before and after each press — and
    // the repository itself, so a git call that changed nothing would still be
    // visible as a changed commit list.
    for (label, _) in items {
        if h.query_by_label(label).is_none() {
            h.get_by_label("Changes header overflow").click();
            h.run();
        }
        let before = inert_snapshot(&h);
        h.get_by_label(label).click();
        h.run();
        let after = inert_snapshot(&h);
        assert_eq!(
            before, after,
            "pressing `{label}` must dispatch nothing. `busy` and `pane_generation` \
             are in the snapshot because every `AppState::dispatch` sets `busy` \
             and every refresh bumps the generation — so a wired control cannot \
             pass this by being quiet."
        );
        assert_eq!(
            repo.commit_count(),
            commits_before,
            "pressing `{label}` must not call git"
        );
        assert_eq!(repo.subjects(), subjects_before);
    }
    // Nothing above armed a confirm, so the one toast a wired rollback would
    // have raised is provably absent rather than merely unobserved.
    assert!(
        h.state().ui.toast.is_none(),
        "a wired `Rollback` would have toasted \"Select files to discard.\""
    );
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
        .find(|(r, c)| *c == Palette::ROW_SELECTED && r.contains(origin))
        .expect("the selected row paints the list-row selection fill (ticket 03)")
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

// -------------------------------- local changes redesign: card containment --

/// The mockup's card surface, as hexes from the design export — deliberately
/// literals rather than `Palette` references, so the assertion cannot be
/// satisfied by re-aliasing a token onto something else.
const CARD_FILL: egui::Color32 = egui::Color32::from_rgb(0x23, 0x25, 0x29);

/// Every rectangle the last frame painted, as
/// `(rect, fill, stroke, corner radius)`.
fn painted_rects(
    h: &Harness<'_, AppState>,
) -> Vec<(egui::Rect, egui::Color32, egui::Stroke, egui::CornerRadius)> {
    h.output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(rs) => Some((rs.rect, rs.fill, rs.stroke, rs.corner_radius)),
            _ => None,
        })
        .collect()
}

/// The card containing `point`, identified by the signature the mockup still
/// gives every card: the same rect painted once as a `#232529` content fill at
/// the 8pt card radius.
///
/// The 1px `#4E5157` hairline that used to complete that signature is gone by
/// design — a card is a surface and a stroke means the surface floats, and a
/// content region does not float (see `turbogit_ui::ui::widgets::CardFrame`).
/// So the card is found by its fill and rounding, and the hairline's ABSENCE
/// at the card's own edge is asserted below instead of required as a
/// precondition. Both halves are read off painted output; neither reads a
/// field of the frame that produced them.
#[track_caller]
fn card_rect(h: &Harness<'_, AppState>, point: egui::Pos2) -> egui::Rect {
    let rects = painted_rects(h);
    let card = rects
        .iter()
        .filter(|(rect, fill, _, radius)| {
            *fill == CARD_FILL
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
                && rect.contains(point)
        })
        .map(|(rect, _, _, _)| *rect)
        .next()
        .unwrap_or_else(|| {
            panic!("no card (a {CARD_FILL:?} fill at CARD_RADIUS) contains {point:?}")
        });

    // The rule the migration installs, asserted on a real screen rather than on
    // the shared frame: nothing strokes a content region. Scoped to the card's
    // OWN edge — a checkbox or a focus ring inside the card is a control, not
    // the region claiming to float.
    for (rect, _, stroke, _) in &rects {
        if stroke.width > 0.0
            && stroke.color != egui::Color32::TRANSPARENT
            && (rect.min - card.min).length() < 0.5
            && (rect.max - card.max).length() < 0.5
        {
            panic!(
                "card {card:?} must paint no stroke, but {rect:?} strokes {stroke:?} on its edge"
            );
        }
    }
    card
}

#[test]
fn commit_message_controls_sit_inside_one_bordered_card() {
    // Redesign Phase 1/2: the flat `ui.heading` + `ui.separator` regions get
    // real containment, so "Commit commits the message above it" is readable
    // from the frame rather than inferred. The message label and the Amend
    // option are the region's top and bottom; one card must hold both.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-msg-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let message = galley_origin(&h, "Commit message:").expect("the message label paints");
    let amend = galley_origin(&h, "Amend").expect("the Amend option paints");
    let card = card_rect(&h, message);
    assert!(
        card.contains(amend),
        "the message label and Amend must sit in the same card, card is {card:?}"
    );
}

#[test]
fn commit_action_row_is_the_commit_card_s_pinned_footer() {
    // Redesign Phase 2 (mockup): the message editor, its meta row and the
    // action bar share ONE card, so "Commit commits the message above it" is
    // readable from containment rather than from adjacency. The action row is
    // the card's footer — inside the border, below the message controls.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-footer-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "footer subject".into();
    h.run();

    let message = galley_origin(&h, "Commit message:").expect("the message label paints");
    let card = card_rect(&h, message);
    let commit = commit_action_button(&h);
    assert!(
        card.contains_rect(commit.rect()),
        "the primary Commit action must sit inside the commit card {card:?}, \
         got {:?}",
        commit.rect()
    );
    let stash = h.get_by_label("Stash…");
    assert!(
        card.contains_rect(stash.rect()),
        "the whole action row is the footer, so `Stash…` is inside the card too, \
         got {:?}",
        stash.rect()
    );
    assert!(
        commit.rect().top() > message.y,
        "the action row sits below the message controls (message y={}, row top={})",
        message.y,
        commit.rect().top()
    );
}

#[test]
fn changes_tree_sits_in_a_card_with_its_toolbar_as_the_header() {
    // Redesign Phase 3 (mockup): the file list gets its own card, and the
    // toolbar stops being a loose row over the panel — the shared pane header
    // becomes the card's header strip, so the controls read as "act on this
    // list" rather than as commit-box chrome. (The card itself paints no
    // stroke: `card_rect` asserts that at its own edge.)
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "card-changes-a");
    let b = temp_repo(parent.path(), "card-changes-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let h = harness(app_state(&[a.path.clone(), b.path.clone()]));

    let title = changes_title_galley(&h).pos;
    let card = card_rect(&h, title);

    // The tree's rows are inside the same card as its header. Only the
    // focused repo's rows show ("focus = expand").
    let row = galley_origin(&h, "a.txt").expect("the focused repo's file row paints");
    assert!(
        card.contains(row),
        "the changes tree must sit inside the card {card:?} headed at {title:?}"
    );

    // Every header control is in the card, in the header band above the rows.
    for label in [
        "Expand all groups",
        "Rollback",
        "Refresh changes",
        "Changes header overflow",
    ] {
        let rect = h.get_by_label(label).rect();
        assert!(
            card.contains_rect(rect),
            "`{label}` must sit inside the changes card {card:?}, got {rect:?}"
        );
        assert!(
            rect.bottom() <= row.y,
            "`{label}` belongs to the header band, above the first row \
             (row y={}, control bottom={})",
            row.y,
            rect.bottom()
        );
    }
}

/// The file filter's own painted input frame: the `SURFACE_3` fill that wraps
/// the search icon and the edit inside it.
///
/// Not the label-addressable node, deliberately. `widgets::search_input` returns
/// the inner `TextEdit`'s `Response`, so `get_by_label("Filter files").rect()` is
/// the *edit field* — a rectangle inset from the control's own edges by the
/// frame margin, the stroke and the icon gutter. Asserting "a full row" against
/// that rect would be measuring the wrong rectangle; the frame is what the user
/// sees as the control.
fn filter_frame(h: &Harness<'_, AppState>) -> egui::Rect {
    let edit = h.get_by_label("Filter files").rect();
    let mut frames: Vec<egui::Rect> = filled_rects(h)
        .into_iter()
        .filter(|(rect, fill)| *fill == Palette::SURFACE_3 && rect.contains_rect(edit))
        .map(|(rect, _)| rect)
        .collect();
    frames.sort_by(|a, b| b.width().partial_cmp(&a.width()).expect("finite width"));
    frames
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("the filter paints no SURFACE_3 input frame around {edit:?}"))
}

/// **The file filter has a row of its own, at full height, and is not clipped.**
///
/// ADR-0016 keeps `Filter files` inside the changes card (mockup wins over the
/// execution plan), but the old header could not hold it: the title plus five
/// icon buttons left it about 24px on the title's row, so it took a second row
/// *inside the heading*. The squeeze is gone now — the header is one 28pt band,
/// the count moved into a chip beside the title, and the filter is the card's
/// first **content** row under the hairline.
///
/// Three geometry facts, all measured off paint and the widget tree:
///
/// 1. it starts below the header's hairline, so it shares no pixel with the
///    header band — a row of its own;
/// 2. it spans the card's whole inner width, so it is a *full* row; and
/// 3. it is drawn at its own full height — at least the app's 24px click-target
///    floor, with the painted frame and the laid-out control the same rectangle,
///    so nothing about it is clipped.
#[test]
fn the_file_filter_gets_its_own_full_width_row_under_the_header() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-filter-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let title = changes_title_galley(&h).pos;
    let card = card_rect(&h, title);
    let rule = changes_header_rule(&h);
    let row = h.get_by_label("base.txt").rect();
    let frame = filter_frame(&h);

    // Inside the card it filters…
    assert!(
        card.contains_rect(frame),
        "the file filter must sit inside the changes card {card:?}, got {frame:?}"
    );

    // (1) A row of its own: no pixel of the filter is inside the header band.
    // Asserted first, because it is the claim the whole ticket turns on — a
    // filter that shares the heading's row is exactly the squeeze that shipped.
    assert!(
        frame.top() >= rule.bottom(),
        "the filter shares no pixel with the header band: it starts below the \
         hairline (band bottom {}, filter top {})",
        rule.bottom(),
        frame.top()
    );

    // …and still above the list it filters.
    assert!(
        frame.bottom() <= row.top() + 1.0,
        "the filter must stay above the list it filters (filter {frame:?}, row {row:?})"
    );

    // (2) A full row: the card's whole inner width, not the heading's leftover.
    let pad = turbogit_ui::theme::PANEL_PADDING;
    let inner_width = card.width() - 2.0 * pad;
    assert!(
        (frame.width() - inner_width).abs() < 1.0,
        "the filter must span the card's whole inner row ({inner_width:.1}px), \
         got {:.1}px — a narrower one means it is still sharing a row with \
         something",
        frame.width()
    );
    // The historical squeeze, kept as a floor: the 340px column must never cost
    // the filter its own hint text again.
    assert!(
        frame.width() >= 96.0,
        "`Filter files` must stay wide enough to be usable, got {:.1}px ({frame:?})",
        frame.width()
    );

    // (3) Its one row is drawn at full height — at least the app's 24px
    // click-target floor, and the painted frame *is* the control: a shorter
    // frame than the widget's own rect would be a clipped row.
    assert!(
        frame.height() >= turbogit_ui::ui::components::CLICK_TARGET_MIN,
        "the filter's row must render at full height (>= {}px), got {:.1}px",
        turbogit_ui::ui::components::CLICK_TARGET_MIN,
        frame.height()
    );
    assert!(
        frame.height() > rule.bottom() - rule.top(),
        "the filter's row is taller than the 1px hairline it sits under, so the \
         row is a control rather than a rule"
    );
}

#[test]
fn preview_is_a_card_headered_by_the_path_status_and_diff_mode() {
    // Redesign Phase 4 (mockup): the flat "Preview" heading becomes a
    // full-height card whose header states what is being previewed and in
    // which mode — the selection→preview link the empty state used to hide.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-preview-repo");
    std::fs::write(repo.path.join("base.txt"), "a\nb\nc\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.path.join("base.txt"), "a\nx\ny\nc\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();

    let heading = galley_origin(&h, "Preview").expect("the preview zone keeps its heading");
    let card = card_rect(&h, heading);
    let heading_band = heading.y + 8.0;

    let path = h.get_by_label("Previewing base.txt").rect();
    assert!(
        card.contains_rect(path),
        "the previewed path must sit inside the preview card {card:?}, got {path:?}"
    );

    for label in ["Modified", "Unified diff", "Previous change", "Next change"] {
        let rect = h.get_by_label(label).rect();
        assert!(
            card.contains_rect(rect),
            "`{label}` must sit inside the preview card {card:?}, got {rect:?}"
        );
        assert!(
            (rect.center().y - heading_band).abs() < 12.0,
            "`{label}` belongs to the header strip beside the heading \
             (band y={heading_band}, control centre {})",
            rect.center().y
        );
    }
}

#[test]
fn expanded_repo_group_paints_one_indent_guide_over_its_file_block() {
    // Redesign Phase 5: file rows hang off their repo group but nothing
    // connected them. One 1px `LINE_SUBTLE` guide now spans exactly the
    // expanded block — and per risk R3 it sits in the gutter between the
    // row checkbox and the status letter, never over either.
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "guide-repo");
    // Both tracked then edited, so both land in the focused repo group's one
    // row block — an untracked file would sit in the Unversioned group instead.
    seed_tracked(&repo.path, "base.txt");
    seed_tracked(&repo.path, "other.txt");

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let first = h.get_by_label("base.txt").rect();
    let last = h.get_by_label("other.txt").rect();
    let block_top = first.top().min(last.top());
    let block_bottom = first.bottom().max(last.bottom());

    let guides: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        // The guide is a 1px-wide LINE_SUBTLE sliver; no other hairline fill in
        // the tree carries that token.
        .filter(|(_, c)| *c == Palette::LINE_SUBTLE)
        .map(|(r, _)| r)
        .filter(|r| r.width() <= 2.0 && r.height() >= turbogit_ui::theme::FILE_ROW_HEIGHT)
        .collect();
    let guide = guides
        .iter()
        .find(|r| r.top() <= block_top + 1.0 && r.bottom() >= block_bottom - 1.0)
        .unwrap_or_else(|| {
            panic!(
                "no vertical LINE_SUBTLE guide spans the file block \
                 y={block_top}..{block_bottom}; guides: {guides:?}"
            )
        });

    // Clear of the checkbox column, and not so far right it cuts the name.
    for name in ["base.txt", "other.txt"] {
        let checkbox = h.get_by_label(&format!("Select {name}")).rect();
        assert!(
            guide.center().x >= checkbox.right() && guide.center().x <= checkbox.right() + 8.0,
            "the guide must sit in the gutter after `{name}`'s checkbox \
             (checkbox right {}, guide x {})",
            checkbox.right(),
            guide.center().x
        );
    }
}

// ------------------------------ issue: severity vocabulary (C2) -------------

/// C2: commit-window messages use the shared semantic severity inks too —
/// the surfaced error in STATE_ERROR, the subject-length guidance in
/// STATE_WARNING — via the actual commit consumers, not raw severity colors.
#[test]
fn commit_severity_messages_use_the_shared_semantic_inks() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "sev");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // The last_error banner is a blocking error.
    h.state_mut().last_error = Some("push failed".into());
    h.run();
    assert_eq!(
        painted_ink_of(&h, "push failed"),
        Some(Palette::STATE_ERROR),
        "the commit error banner must render in the shared error ink"
    );

    // The subject-length guidance is a warning with its message preserved.
    h.state_mut().ui.commit_message = "a".repeat(51);
    h.run();
    assert_eq!(
        painted_ink_of(&h, "keep \u{2264} 50"),
        Some(Palette::STATE_WARNING),
        "subject-length guidance must render in the shared warning ink"
    );
}

/// The ink of the painted galley whose text contains `needle`.
fn painted_ink_of(h: &Harness<'_, AppState>, needle: &str) -> Option<egui::Color32> {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text.contains(needle))
        .map(|g| g.color)
}

// ------------------------------ issue: shared typography roles (T2) ---------

/// T2: the commit file row's name (and badge letter) render at the shared
/// body size — the local 12.5px copy is superseded by the shared TYPE_BODY
/// role so a central type-scale change reaches every filename consumer.
#[test]
fn file_row_name_renders_at_the_shared_body_size() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "type-role");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.run();

    // The file row is painted; its name galley carries the shared body size.
    let name_size = {
        let mut sizes = Vec::new();
        for clipped in &h.output().shapes {
            if let egui::Shape::Text(shape) = &clipped.shape {
                let text = shape.galley.text();
                if text.contains("base.txt") {
                    sizes.push(
                        shape
                            .galley
                            .job
                            .sections
                            .first()
                            .map_or(0.0, |s| s.format.font_id.size),
                    );
                }
            }
        }
        sizes
    };
    assert!(
        !name_size.is_empty(),
        "the commit file row must paint its name"
    );
    assert!(
        name_size
            .iter()
            .any(|s| (*s - turbogit_ui::theme::TYPE_BODY).abs() < 0.01),
        "the file-row name must render at the shared body size (TYPE_BODY={}); got {name_size:?}",
        turbogit_ui::theme::TYPE_BODY
    );
}

// ---------------- design system v2, conformance 10: the changes list ---------

/// The changes card's own rect, found through the card's own signature.
///
/// [`card_rect`] also asserts the card paints no stroke at its own edge, so
/// every test below inherits that half of the conformance claim from the
/// containment lookup rather than restating it.
fn changes_card(h: &Harness<'_, AppState>) -> egui::Rect {
    card_rect(h, changes_title_galley(h).pos)
}

/// Every row-shaped filled rect inside `card` — a band is at least as wide as
/// the list and as tall as a file row, which is what separates it from the
/// narrow things a row also paints (the 2px rail, the 16px checkbox, the
/// count chip).
///
/// The card's own rect is excluded, so a *nested* card is reported rather
/// than mistaken for the surface everything else sits on.
fn row_bands(h: &Harness<'_, AppState>, card: egui::Rect) -> Vec<(egui::Rect, egui::Color32)> {
    filled_rects(h)
        .into_iter()
        .filter(|(rect, _)| {
            *rect != card
                && card.intersects(*rect)
                && rect.width() > 100.0
                && rect.height() >= turbogit_ui::theme::FILE_ROW_HEIGHT - 0.5
        })
        .collect()
}

/// The one row band carrying `fill` that contains `point`.
///
/// Position-scoped, because a string — and therefore a band, once two rows
/// share a state — paints more than once, and "the first band painted" is not
/// a statement about which row is selected.
#[track_caller]
fn band_containing(
    h: &Harness<'_, AppState>,
    card: egui::Rect,
    fill: egui::Color32,
    point: egui::Pos2,
) -> egui::Rect {
    let hits: Vec<egui::Rect> = row_bands(h, card)
        .into_iter()
        .filter(|(_, painted)| *painted == fill)
        .map(|(rect, _)| rect)
        .filter(|rect| rect.contains(point))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one {fill:?} row band containing {point:?}, found {hits:?}"
    );
    hits.into_iter().next().expect("one band")
}

/// **A chosen file row is the row-selected fill plus a 2px accent rail — and
/// the loudest blue in the app is nowhere else in the list.**
///
/// Three claims, in the order they fail. The band's fill is the shared
/// list-row selection token, read from the row-fill decision rather than
/// restated as a literal. The rail is [`RAIL_WIDTH`] of [`Palette::BRAND`] at
/// the band's **own** leading edge, full height — the one rail painter, not a
/// second bar this view drew for itself. And no band in the list is the
/// current-ref selection token or the brand fill: a solid brand band behind
/// running text is the thing this row no longer is.
#[test]
fn a_selected_file_row_paints_the_row_selected_fill_and_a_leading_accent_rail() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "row-selected-fill");
    seed_tracked(&repo.path, "base.txt");
    seed_tracked(&repo.path, "other.txt");

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();

    let card = changes_card(&h);
    let origin = galley_origin(&h, "base.txt").expect("the selected row paints its name");
    let band = band_containing(&h, card, Palette::ROW_SELECTED, origin);

    // The value, from the construction site: `RowState::RowSelected` is the
    // one list-row selection fill, and it is neither of the two tokens a
    // louder row used to take.
    assert_eq!(
        turbogit_ui::ui::components::row_fill(turbogit_ui::ui::components::RowState::RowSelected),
        Palette::ROW_SELECTED,
        "a chosen list row's fill is the row-selected token, read from the \
         row-fill decision rather than restated here"
    );
    assert_ne!(
        Palette::ROW_SELECTED,
        Palette::SELECTION,
        "the current-ref band is not the list-row selection fill"
    );
    assert!(
        (band.height() - turbogit_ui::theme::FILE_ROW_HEIGHT).abs() < 0.01,
        "the selected band is exactly one file row tall, got {band:?}"
    );

    // The rail: the shared width, at the band's leading edge, its full height.
    let rails: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, fill)| {
            *fill == Palette::BRAND
                && card.intersects(*rect)
                && (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
        })
        .map(|(rect, _)| rect)
        .collect();
    assert_eq!(
        rails.len(),
        1,
        "exactly one rail paints in the changes list, and it belongs to the \
         chosen row: {rails:?}"
    );
    let rail = rails[0];
    assert!(
        (rail.left() - band.left()).abs() < 0.01,
        "the rail is paint at the row's own leading edge, not a reserved \
         gutter beside it: rail {rail:?} vs band {band:?}"
    );
    assert!(
        (rail.height() - band.height()).abs() < 0.01,
        "the rail spans the chosen row's full height: rail {rail:?} vs band {band:?}"
    );

    // The negative rule, at every band the list paints. The rail is excluded
    // by the band width, so a brand *band* cannot hide behind it.
    for (rect, fill) in row_bands(&h, card) {
        assert_ne!(
            fill,
            Palette::SELECTION,
            "no list row fills with the current-ref band: {rect:?} is {fill:?}"
        );
        assert_ne!(
            fill,
            Palette::BRAND,
            "the accent is the rail, the primary action and the active tab \
             underline — never a row's own fill: {rect:?} is {fill:?}"
        );
    }
}

/// **A chosen row's text origin is an unchosen row's text origin.** The rail is
/// paint; reserving its width as padding would shift every row's content the
/// moment it is selected, and the names would stop lining up down the column.
///
/// Both rows are the same status at the repo root, so the only variable is
/// the selection. The comparison row is hovered so that *its* band is painted
/// too — an untouched row paints nothing and cannot be located from paint.
#[test]
fn a_selected_file_rows_text_origin_equals_an_unselected_rows() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "row-origin");
    seed_tracked(&repo.path, "base.txt");
    seed_tracked(&repo.path, "other.txt");

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();
    h.get_by_label("other.txt").hover();
    h.run();

    let card = changes_card(&h);
    let (selected, comparison) = (
        band_containing(
            &h,
            card,
            Palette::ROW_SELECTED,
            galley_origin(&h, "base.txt").expect("the chosen row paints its name"),
        ),
        band_containing(
            &h,
            card,
            Palette::RAISED_ON_CARD,
            galley_origin(&h, "other.txt").expect("the comparison row paints its name"),
        ),
    );
    let name_in = |name: &str| {
        painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == name)
            .unwrap_or_else(|| panic!("no painted galley carries {name:?}"))
    };
    let (chosen, plain) = (name_in("base.txt"), name_in("other.txt"));

    assert_eq!(
        chosen.pos.x - selected.left(),
        plain.pos.x - comparison.left(),
        "a chosen row's name starts the same distance from its own leading edge \
         as an unchosen row's: the rail is paint, never reserved padding \
         (chosen {:?} in {selected:?}, plain {:?} in {comparison:?})",
        chosen.pos,
        plain.pos
    );
    assert_eq!(
        chosen.pos.x, plain.pos.x,
        "both rows share one name column, selected or not"
    );
    assert_eq!(
        selected.size(),
        comparison.size(),
        "choosing a row does not resize it either"
    );
}

/// Every `(text, ink)` pair painted inside one file row, read off that row's
/// own checkbox node.
///
/// The checkbox is the row's fixed 16px leading column, vertically centred in
/// the 24px row, so the row is the checkbox grown to the row's height and run
/// out to the list's trailing edge. That rect is the same before and after a
/// selection — which is exactly what makes the two ink sets comparable.
fn file_row_inks(
    h: &Harness<'_, AppState>,
    name: &str,
    card: egui::Rect,
) -> Vec<(String, [u8; 4])> {
    let checkbox = h.get_by_label(&format!("Select {name}")).rect();
    let grow = (turbogit_ui::theme::FILE_ROW_HEIGHT - checkbox.height()) / 2.0;
    let row = egui::Rect::from_min_max(
        egui::pos2(checkbox.left() - 24.0, checkbox.top() - grow),
        egui::pos2(card.right() - 24.0, checkbox.bottom() + grow),
    );
    let mut inks: Vec<(String, [u8; 4])> = painted_galleys(h)
        .into_iter()
        .filter(|g| row.contains(g.pos))
        .map(|g| (g.text, g.color.to_array()))
        .collect();
    inks.sort();
    inks
}

/// **Choosing a file row changes no ink in it, and the inks it does use are
/// the ramp's.**
///
/// The first claim is the one a selection band used to break: an opaque fill
/// behind running text tempts a caller to re-ink the text for the new
/// surface, which is how a selected row ends up reading as a different kind
/// of thing from an unselected one. The row's ink set is captured before and
/// after, position-scoped, and compared.
///
/// The second is where each ink lands: the **name** — the thing the user came
/// to read — in `INK`, and the **location suffix** — the furniture around it —
/// in `INK_3`.
#[test]
fn selecting_a_file_row_changes_no_ink_in_it() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "row-ink");
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    std::fs::write(repo.path.join("src/app.rs"), "fn main() {}\n").unwrap();
    git(&repo.path, &["add", "src/app.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "track app"]);
    std::fs::write(repo.path.join("src/app.rs"), "fn main() { println!(); }\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = changes_card(&h);
    let before = file_row_inks(&h, "app.rs", card);
    assert!(
        before.iter().any(|(text, _)| text == "app.rs"),
        "the row paints its name before selection, got {before:?}"
    );

    h.get_by_label("app.rs").click();
    h.run();

    let after = file_row_inks(&h, "app.rs", card);
    assert_eq!(
        before, after,
        "selecting a row must not re-ink any of its text: the row is the \
         same kind of object chosen or not"
    );

    let ink_of = |text: &str| {
        after
            .iter()
            .find(|(painted, _)| painted == text)
            .map(|(_, colour)| {
                egui::Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3])
            })
            .unwrap_or_else(|| panic!("{text:?} is not painted in the selected row, got {after:?}"))
    };
    assert_eq!(
        ink_of("app.rs"),
        Palette::INK,
        "a file name is the row's answer, and answers at the primary ink"
    );
    assert_eq!(
        ink_of("src/"),
        Palette::INK_3,
        "the location suffix is the furniture around the name, and dims to \
         the muted step"
    );
    assert_eq!(
        ink_of("M"),
        turbogit_ui::theme::Palette::STATUS_MODIFIED,
        "the status letter keeps its semantic hue — it is state, not ramp"
    );
}

/// **A long name wraps: the row grows, and the painted galley grows with it.**
///
/// Asserted from geometry only — laid-out rows, galley extent, band height.
/// `Galley::text()` returns the *unwrapped* input, so a wrap test written
/// against it reads as though nothing wrapped even when the row grew
/// correctly; the galley is located by that accessor and never judged by it.
#[test]
fn a_long_file_name_grows_its_galley_and_its_row() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "wrap-galley");
    let long = "an_extremely_long_file_name_that_cannot_fit_the_commit_panel_column.rs";
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    std::fs::write(repo.path.join("src").join(long), "one\n").unwrap();
    std::fs::write(repo.path.join("src/short.rs"), "two\n").unwrap();
    git(&repo.path, &["add", "-A"]);
    git(&repo.path, &["commit", "-q", "-m", "track both"]);
    std::fs::write(repo.path.join("src").join(long), "one edited\n").unwrap();
    std::fs::write(repo.path.join("src/short.rs"), "two edited\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.run();

    let (_, long_size, long_rows) = galley_metrics(&h, long);
    let (_, short_size, short_rows) = galley_metrics(&h, "short.rs");
    assert!(
        long_rows >= 2,
        "a name too wide for its column wraps onto continuation lines, got \
         {long_rows} laid-out row(s)"
    );
    assert_eq!(
        short_rows, 1,
        "the short name in the same column stays on one line, or the two are \
         not being compared at the same width"
    );
    assert!(
        long_size.y > short_size.y,
        "the painted galley's own extent grows with the wrap: long {long_size:?} \
         vs short {short_size:?}"
    );

    // Both rows selected, so both bands are painted and each is located by the
    // name it carries rather than by paint order.
    h.get_by_label(&format!("Select {long}")).click();
    h.get_by_label("Select short.rs").click();
    h.run();

    let card = changes_card(&h);
    let height_of = |name: &str| {
        band_containing(
            &h,
            card,
            Palette::ROW_SELECTED,
            galley_origin(&h, name).unwrap_or_else(|| panic!("{name:?} paints its name")),
        )
        .height()
    };
    let (long_h, short_h) = (height_of(long), height_of("short.rs"));
    assert!(
        long_h > turbogit_ui::theme::FILE_ROW_HEIGHT,
        "a wrapped row grows past the single-line height, got {long_h}"
    );
    assert!(
        (short_h - turbogit_ui::theme::FILE_ROW_HEIGHT).abs() < 0.01,
        "and the common case keeps exactly the file-row height, got {short_h}"
    );
}

/// **Group headers sit on the section band, and no group is a nested card.**
///
/// Three headers at once — the repo group, the `Merge conflicts` group and the
/// `Unversioned Files` group — because the claim is about the *shared* header
/// row, and a view that conforms in one place and not the other has three
/// headers rather than one.
///
/// Each band is the section band at the shared group-row height, at
/// [`CornerRadius::ZERO`] — square, so a group reads as scaffolding on the
/// card rather than as a smaller card inside it — and spans its own header row.
/// The separation from the list below is spacing or at most one hairline, and
/// nothing strokes a group.
#[test]
fn group_headers_sit_on_the_section_band_with_no_nested_card() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "group-band");
    let branch = repo.branch();
    let name = repo
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    seed_tracked(&repo.path, "base.txt");
    std::fs::write(repo.path.join("untracked.txt"), "untracked\n").unwrap();
    seed_conflict(&repo.path, &branch);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = changes_card(&h);

    for label in [
        format!("Toggle {name}"),
        "Merge conflicts (1)".to_owned(),
        "Unversioned Files (1)".to_owned(),
    ] {
        let header = h.get_by_label(&label).rect();
        let bands: Vec<egui::Rect> = filled_rects(&h)
            .into_iter()
            .filter(|(rect, fill)| *fill == Palette::SECTION_BG && rect.contains(header.center()))
            .map(|(rect, _)| rect)
            .collect();
        assert_eq!(
            bands.len(),
            1,
            "`{label}` paints exactly one section band under itself, found {bands:?}"
        );
        let band = bands[0];
        // The band itself is square. The focused repo's group additionally
        // carries the shared row state's translucent focus fill *over* it at
        // the control radius, which is the row vocabulary answering on top of
        // the band — so the assertion is on the band shape, and the only other
        // shape allowed at this geometry is that row state.
        let radii: Vec<(egui::Color32, egui::CornerRadius)> = painted_rects(&h)
            .into_iter()
            .filter(|(rect, fill, _, _)| *rect == band && *fill != Palette::SECTION_BG)
            .map(|(_, fill, _, radius)| (fill, radius))
            .collect();
        assert!(
            radii.iter().all(|(fill, _)| *fill
                == turbogit_ui::ui::components::row_fill(
                    turbogit_ui::ui::components::RowState::FocusSelected
                )),
            "the only fill over a group band is the shared focus row state, so \
             a group cannot grow a second surface of its own: {radii:?} at {band:?}"
        );
        assert_eq!(
            painted_rects(&h)
                .into_iter()
                .filter(|(rect, fill, _, _)| *rect == band && *fill == Palette::SECTION_BG)
                .map(|(_, _, _, radius)| radius)
                .collect::<Vec<_>>(),
            vec![egui::CornerRadius::ZERO],
            "`{label}`'s band is square: a group is scaffolding on the card, not \
             a smaller card inside it"
        );
        assert!(
            (band.height() - turbogit_ui::theme::GROUP_ROW_HEIGHT).abs() < 0.01,
            "`{label}`'s band is the shared group-row height, got {band:?}"
        );
        assert!(
            band.width() >= header.width() - 0.5,
            "`{label}`'s band spans its whole header row (band {band:?}, header \
             {header:?}) — a band narrower than the row it heads is a chip"
        );
    }

    // No group is a card inside the card: the only card-shaped fill inside
    // the changes card is the changes card.
    let nested: Vec<egui::Rect> = painted_rects(&h)
        .into_iter()
        .filter(|(rect, fill, _, radius)| {
            *rect != card
                && *fill == CARD_FILL
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
                && card.contains_rect(*rect)
        })
        .map(|(rect, _, _, _)| rect)
        .collect();
    assert!(
        nested.is_empty(),
        "a group is a band, not a nested card: {nested:?} inside {card:?}"
    );

    // The separation from the list is spacing or at most one hairline, and
    // nothing strokes it.
    let header = h.get_by_label("Merge conflicts (1)").rect();
    let first_row = h.get_by_label("C conf.txt").rect();
    let gap = first_row.top() - header.bottom();
    assert!(
        (-0.5..=turbogit_ui::theme::ITEM_SPACING.y).contains(&gap),
        "the list is separated from the group band by spacing or a hairline, \
         not by a border: gap {gap} between {header:?} and {first_row:?}"
    );
    let between: Vec<(egui::Rect, egui::Color32, f32)> = stroked_rects(&h)
        .into_iter()
        .filter(|(rect, _, _)| {
            rect.width() > 100.0
                && rect.top() >= header.bottom() - 0.5
                && rect.bottom() <= first_row.top() + 0.5
        })
        .collect();
    assert!(
        between.is_empty(),
        "nothing strokes the seam between a group header and its rows: {between:?}"
    );
}

/// **The list reads on the four-step ramp, and never on the dim step.**
///
/// `INK_4` is sub-AA on every surface in the palette, so it has no legal
/// surface at all; a path a user might need to read is `INK_3` at worst. This
/// asserts the whole card rather than three sampled strings, because the rule
/// is "no `INK_4` in this list", not "`INK_4` is absent from these three
/// words" — and it names where each ink that *is* used lands.
#[test]
fn the_changes_list_reads_on_the_ink_ramp_and_never_the_dim_step() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "list-ink-ramp");
    let branch = repo.branch();
    let name = repo
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    seed_tracked(&repo.path, "base.txt");
    // A nested path as well, so the sweep covers the dim location suffix and
    // not just the names — the suffix is the other text in this list, and it
    // is the one a caller is most tempted to sink a step further.
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    std::fs::write(repo.path.join("src/app.rs"), "fn main() {}\n").unwrap();
    git(&repo.path, &["add", "src/app.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "track app"]);
    std::fs::write(repo.path.join("src/app.rs"), "fn main() { println!(); }\n").unwrap();
    std::fs::write(repo.path.join("untracked.txt"), "untracked\n").unwrap();
    seed_conflict(&repo.path, &branch);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = changes_card(&h);

    let ink_of = |text: &str| painted_ink(&h, text);
    assert_eq!(
        ink_of(&name),
        Some(Palette::INK),
        "a group header's name is the group's answer, and answers at the \
         primary ink"
    );
    assert_eq!(
        ink_of("base.txt"),
        Some(Palette::INK),
        "a file name reads at the primary ink — the status letter beside it \
         carries the state, so the name does not have to"
    );
    assert_eq!(
        ink_of("Unversioned Files"),
        Some(Palette::INK),
        "the bottom group's name answers at the same step as every other group"
    );
    assert_eq!(
        ink_of("src/"),
        Some(Palette::INK_3),
        "the dim location suffix is the furniture around the name, and takes \
         the muted step — `INK_3`, never `INK_4`"
    );

    let dim: Vec<String> = painted_galleys(&h)
        .into_iter()
        .filter(|g| card.contains(g.pos) && g.color == Palette::INK_4)
        .map(|g| g.text)
        .collect();
    assert!(
        dim.is_empty(),
        "`INK_4` is sub-AA on every surface in the palette and has no legal \
         surface at all; nothing in the changes list may paint it: {dim:?}"
    );
}

/// The body of `fn <name>` in the commit window's source, comments stripped.
///
/// Brace-counted rather than regex-matched, so a wrapped signature or a nested
/// block is read whole; comment-stripped, so a doc comment describing the
/// opposite of what the body does cannot satisfy a claim about the body.
fn commit_window_fn_body(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/commit_window.rs");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));

    // Comment stripping, tracking string literals so a `//` inside one is not
    // mistaken for a comment.
    let mut stripped = String::with_capacity(src.len());
    let chars: Vec<char> = src.chars().collect();
    let (mut i, mut in_line, mut in_block, mut in_string) = (0usize, false, false, false);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if in_line {
            if c == '\n' {
                in_line = false;
                stripped.push('\n');
            } else {
                stripped.push(' ');
            }
        } else if in_block {
            if c == '*' && next == '/' {
                in_block = false;
                stripped.push_str("  ");
                i += 2;
                continue;
            }
            stripped.push(if c == '\n' { '\n' } else { ' ' });
        } else if in_string {
            if c == '\\' {
                stripped.push(c);
                if let Some(escaped) = chars.get(i + 1) {
                    stripped.push(*escaped);
                }
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            stripped.push(c);
        } else if c == '/' && next == '/' {
            in_line = true;
            stripped.push_str("  ");
            i += 2;
            continue;
        } else if c == '/' && next == '*' {
            in_block = true;
            stripped.push_str("  ");
            i += 2;
            continue;
        } else {
            if c == '"' {
                in_string = true;
            }
            stripped.push(c);
        }
        i += 1;
    }

    let declaration = format!("fn {name}(");
    let start = stripped
        .find(&declaration)
        .unwrap_or_else(|| panic!("commit_window.rs declares no `{declaration}`"));
    let rest = &stripped[start..];
    let mut depth = 0i32;
    let mut end = rest.len();
    for (offset, c) in rest.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = offset + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    rest[..end].to_owned()
}

/// **The changes list reaches its fill and its rail through the shared row
/// shell, and names neither of the two loud tokens at all.**
///
/// The rendered ratchets above can only see the sites a frame happens to
/// paint, so the negative half of "no list row fills with the selection or
/// brand token" is pinned here too, at the construction site: the two
/// functions that paint a file row must not mention [`Palette::SELECTION`] or
/// [`Palette::BRAND`] at all, must reach the fill through
/// [`components::row_shell`], and must not call the rail painter directly —
/// a view that drew its own 2px bar would be a second definition of the rail
/// the token layer says is written down once.
#[test]
fn the_changes_list_reaches_its_fill_and_rail_through_the_shared_row_shell() {
    for name in ["change_row", "conflict_row", "paint_checkbox"] {
        let body = commit_window_fn_body(name);
        assert!(
            !body.contains("Palette::SELECTION"),
            "`{name}` must never name the current-ref band; a chosen list row \
             takes the row-selected fill, and the negative rule is written at \
             the site that chooses the fill:\n{body}"
        );
        assert!(
            !body.contains("Palette::BRAND"),
            "`{name}` must never name the brand fill; the accent belongs to the \
             primary action, the active tab underline and the row's rail:\n{body}"
        );
        assert!(
            !body.contains("paint_rail("),
            "`{name}` must reach the rail through `components::row_shell`, so \
             there is one rail painter and one rail width:\n{body}"
        );
    }

    for name in ["change_row", "conflict_row"] {
        let body = commit_window_fn_body(name);
        assert!(
            body.contains("components::row_shell("),
            "`{name}` paints its row's shell through `components::row_shell`, \
             which is where the fill and the rail are decided:\n{body}"
        );
        assert!(
            body.contains("components::RowShell::Railed"),
            "`{name}` asks for the railed variant, so a chosen row is marked by \
             the shared accent rather than by a local bar:\n{body}"
        );
    }
}

/// Local mirror of the widget-library suite's font lookup, so the heading's
/// *size* is read from the painted galley rather than trusted.
fn painted_font<S>(h: &Harness<'_, S>, needle: &str) -> egui::FontId {
    h.output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == needle => text
                .galley
                .job
                .sections
                .first()
                .map(|s| s.format.font_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no painted galley carries {needle:?}"))
}

// ------------------- design system v2 ticket 11: the commit card -------------

/// The commit card's own rect — the card the message well's heading sits in.
#[track_caller]
fn commit_card(h: &Harness<'_, AppState>) -> egui::Rect {
    let heading = galley_origin(h, "Commit message:")
        .unwrap_or_else(|| panic!("the commit card's heading `Commit message:` must paint"));
    card_rect(h, heading)
}

/// The message well: the one rect on the card that spans the card's whole inner
/// row.
///
/// Found by its **width**, not by its fill: the well's fill is the thing the
/// raised-ladder ratchet is about, so a helper that could only find the well by
/// its correct colour would report "not found" instead of "wrong colour" — a
/// failure that does not say what is wrong. Template and Clear are 28 pt and
/// Amend's box is 16 pt, so only the well is the full inner width.
#[track_caller]
fn message_well(h: &Harness<'_, AppState>, card: egui::Rect) -> egui::Rect {
    let inner = card.width() - 2.0 * turbogit_ui::theme::PANEL_PADDING;
    let hits: Vec<egui::Rect> = filled_rects(h)
        .into_iter()
        .map(|(rect, _)| rect)
        .filter(|rect| (rect.width() - inner).abs() < 1.0 && card.contains_rect(*rect))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one full-inner-width rect on the commit card {card:?} \
         (the message well), found {hits:?}"
    );
    hits.into_iter().next().expect("one well")
}

/// The card's one meta row, as the rect its controls share.
///
/// Measured, not computed: "all four controls are on one row" is the claim, and
/// the claim is that their painted/allocated rectangles agree, so the row is
/// their union.
#[track_caller]
fn meta_row(h: &Harness<'_, AppState>, card: egui::Rect) -> egui::Rect {
    let rects: Vec<egui::Rect> = ["Amend", "Template", "Clear"]
        .iter()
        .map(|label| h.get_by_label(label).rect())
        .chain(std::iter::once(counter_rect(h)))
        .collect();
    for r in &rects {
        assert!(
            card.contains_rect(*r),
            "every meta-row control must sit inside the commit card {card:?}, \
             got {r:?} (all: {rects:?})"
        );
    }
    let centres: Vec<f32> = rects.iter().map(egui::Rect::center).map(|c| c.y).collect();
    let lo = centres.iter().cloned().fold(f32::INFINITY, f32::min);
    let hi = centres.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        hi - lo <= 1.0,
        "the counter, Amend, Template and Clear must share ONE row, not two: \
         their centre lines span {:.1}pt (centres {centres:?}). The card is three \
         things — a well, a meta row and a Commit control — and a second row for \
         any one of the four puts it back to six.",
        hi - lo
    );

    // …and the row *fits* the column, which is the whole premise of keeping the
    // width fixed. The counter is the item that grows (its over-length guidance
    // appears beside it), so the claim is specifically that the counter's text
    // stops short of the leftmost control.
    let counter = counter_rect(h);
    let controls_left = ["Amend", "Template", "Clear"]
        .iter()
        .map(|label| h.get_by_label(label).rect().left())
        .fold(f32::INFINITY, f32::min);
    assert!(
        counter.right() + f32::from(turbogit_ui::theme::CONTROL_RADIUS) <= controls_left,
        "the meta row fits the {COMMIT_PANEL_WIDTH}-wide column: the counter ends at \
         {:.1} and the first control starts at {:.1}. The column width is not going \
         anywhere, so a counter that runs into Amend has to be measured, not \
         tolerated.",
        counter.right(),
        controls_left
    );
    let right = ["Amend", "Template", "Clear"]
        .iter()
        .map(|label| h.get_by_label(label).rect().right())
        .fold(f32::NEG_INFINITY, f32::max);
    let inner = card.width() - 2.0 * turbogit_ui::theme::PANEL_PADDING;
    assert!(
        (right - (card.left() + turbogit_ui::theme::PANEL_PADDING) - inner).abs() < 1.0,
        "the meta row's controls are right-aligned to the card's inner edge \
         (x={:.1}), got {right:.1}",
        card.left() + turbogit_ui::theme::PANEL_PADDING
    );

    egui::Rect::from_min_max(
        egui::pos2(
            rects
                .iter()
                .map(egui::Rect::left)
                .fold(f32::INFINITY, f32::min),
            rects
                .iter()
                .map(egui::Rect::top)
                .fold(f32::INFINITY, f32::min),
        ),
        egui::pos2(
            rects
                .iter()
                .map(egui::Rect::right)
                .fold(f32::NEG_INFINITY, f32::max),
            rects
                .iter()
                .map(egui::Rect::bottom)
                .fold(f32::NEG_INFINITY, f32::max),
        ),
    )
}

/// The painted `Subject: n/50` counter's rect.
#[track_caller]
fn counter_rect(h: &Harness<'_, AppState>) -> egui::Rect {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text.starts_with("Subject: ") && g.text.ends_with("/50"))
        .unwrap_or_else(|| {
            panic!(
                "the card's one meta row must carry the `Subject: n/50` counter; \
                 painted text: {:?}",
                painted_text(h)
            )
        })
        .rect
}

/// The Commit control's two painted halves, as
/// `(label half, chevron half, the 1 px rule between them)`.
///
/// Read by geometry inside the control's own bounds, which is the only place a
/// "one control" claim can be made honestly: a separate chevron *button* beside
/// a brand button looks the same in an accesskit dump and is not the same
/// control.
#[track_caller]
fn commit_control_parts(h: &Harness<'_, AppState>) -> (egui::Rect, egui::Rect, egui::Rect) {
    let label = h.get_by_label("Commit changes").rect();
    let chevron = h.get_by_label("Commit split options").rect();
    let button = egui::Rect::from_min_max(label.min, chevron.max);

    let brand: Vec<egui::Rect> = filled_rects(h)
        .into_iter()
        .filter(|(rect, fill)| *fill == Palette::ACCENT && button.contains_rect(*rect))
        .map(|(rect, _)| rect)
        .collect();
    assert_eq!(
        brand.len(),
        2,
        "the Commit control paints exactly two filled halves (the label half and \
         the chevron half), found {brand:?} inside {button:?}. A third brand rect \
         here is a second control, not a split."
    );
    let rule: Vec<egui::Rect> = stroked_rects(h)
        .into_iter()
        .filter(|(rect, _, width)| (*width - 1.0).abs() < 0.01 && button.contains_rect(*rect))
        .map(|(rect, _, _)| rect)
        .collect();
    match rule.len() {
        1 => {}
        0 => panic!(
            "the Commit control paints no 1px rule between its halves ({button:?}). \
             A gap with nothing in it is the old separate-chevron-button reading: two \
             carets glued together. One hairline, on the button, is what makes the \
             two halves one control."
        ),
        n => panic!(
            "the Commit control carries {n} 1px strokes ({rule:?}) inside {button:?}. \
             One rule, between the halves. A stroke around the button's own edge is a \
             bordered button, which the R2 rules forbid."
        ),
    }
    let mut brand = brand;
    brand.sort_by(|a, b| a.left().partial_cmp(&b.left()).expect("finite left"));
    (brand[0], brand[1], rule[0])
}

/// The file list's own card, found as the card *below* `card` in the same
/// column.
///
/// Deliberately not [`changes_card`], which finds the list by its `CHANGES`
/// heading: a well that has eaten the list pushes that heading off the screen,
/// and the failure worth reporting is "the list has no room left", not "the
/// heading is something else".
#[track_caller]
fn changes_card_below(h: &Harness<'_, AppState>, card: egui::Rect) -> egui::Rect {
    let mut below: Vec<egui::Rect> = painted_rects(h)
        .into_iter()
        .filter(|(rect, fill, _, radius)| {
            *fill == CARD_FILL
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
                && (rect.width() - COMMIT_PANEL_WIDTH).abs() < 0.5
                && rect.top() >= card.bottom() - 1.0
        })
        .map(|(rect, _, _, _)| rect)
        .collect();
    assert_eq!(
        below.len(),
        1,
        "the file list's own card must still be painted in the column below the \
         commit card, which now ends at y={:.1}. Found {below:?}. A message well \
         that grows until the file list is off the screen has solved the message at \
         the list's expense, and the list is what the well shares the fixed column \
         with.",
        card.bottom()
    );
    below.remove(0)
}

/// The commit window's left column: the fixed [`COMMIT_PANEL_WIDTH`]-wide pane
/// both cards live in.
///
/// Its *bottom* is the one edge the list runs out of, and it is read from the
/// shell's own left rail rather than from a hard-coded window height: the
/// sidebar is the sibling region of the same work area, so its bottom edge is
/// the work area's bottom edge, which is exactly the bottom of the rect
/// `two_zone` hands the commit column. A number typed in here would be a number
/// about this harness, not about the layout.
#[track_caller]
fn commit_column_bottom(h: &Harness<'_, AppState>) -> f32 {
    let bottoms: Vec<f32> = filled_rects(h)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::SIDEBAR)
        .map(|(rect, _)| rect.bottom())
        .collect();
    assert_eq!(
        bottoms.len(),
        1,
        "the shell's own sidebar surface is how this suite reads the work area's \
         bottom edge (the commit column is clipped to it), expected exactly one, \
         found {bottoms:?}"
    );
    bottoms[0]
}

const COMMIT_PANEL_WIDTH: f32 = turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH;

/// **The card is a well, ONE meta row, and ONE Commit control — and the well
/// and the Commit control are on different rows from the meta row and from each
/// other.**
///
/// The four controls the ticket named (counter, Template, Clear, Amend) have to
/// be *present* and *together*; the Commit control has to be *below* them. The
/// old layout put the counter on one row and Amend/Template/Clear on the next,
/// and that second row is the width the header had been borrowing — so
/// "together" is the ratchet, and it is checked by centre line rather than by
/// reading which function drew what.
#[test]
fn the_commit_card_is_a_well_one_meta_row_and_one_commit_control() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-three-things");
    seed_changes(&repo.path);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "three things".into();
    h.run();

    let card = commit_card(&h);
    let well = message_well(&h, card);
    let row = meta_row(&h, card);
    let commit = h.get_by_label("Commit changes").rect();
    let chevron = h.get_by_label("Commit split options").rect();

    // The three parts stack in the card's own order: well, meta row, action.
    assert!(
        card.contains(egui::pos2(well.left(), well.top())),
        "the well must be inside the commit card {card:?}, got {well:?}"
    );
    assert!(
        row.top() >= well.bottom() - 0.5,
        "the meta row sits under the well (well {well:?}, row {row:?})"
    );
    assert!(
        commit.top() > row.bottom(),
        "the Commit control is the card's action row, below the meta row: \
         meta row {row:?}, Commit {commit:?}. A second primary beside the \
         message controls is the reading the ticket rules out."
    );
    assert!(
        card.contains_rect(chevron),
        "the chevron half is part of the Commit control and lives in the card: \
         {chevron:?} in {card:?}"
    );

    // The card's width is the column's width, and the column's width is the
    // number the ticket refuses to move.
    assert!(
        (card.width() - COMMIT_PANEL_WIDTH).abs() < 0.5,
        "the commit card fills the fixed {COMMIT_PANEL_WIDTH}-wide column, got {:.1}",
        card.width()
    );
}

/// **The chevron paints INSIDE the Commit button, across one 1 px rule, and the
/// button is content-width.**
///
/// Three claims, each with its own failure:
///
/// - **Inside.** The two brand fills are the button's two halves and the single
///   1 px stroke sits exactly between them — a 1 pt gap, one rule wide. A
///   separate chevron button is a gap with no rule in it, and that is the
///   "two carets" reading this replaced.
/// - **Not around.** The rule is the *only* stroke on the control, so the button
///   is not bordered.
/// - **Content-width.** The control is sized by its own label and the chevron's
///   own square, and is a fraction of the row it sits in. A full-width brand bar
///   would be a different control wearing Commit's label.
#[test]
fn the_commit_control_is_one_content_width_split_button_with_a_single_rule() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-split");
    seed_changes(&repo.path);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "split subject".into();
    h.run();

    let card = commit_card(&h);
    let (left, right, rule) = commit_control_parts(&h);

    // The two halves meet the rule: no gap on either side of it.
    assert_eq!(
        left.right(),
        rule.left(),
        "the label half must run right up to the rule — a gap here is the seam \
         that made the old control read as two buttons: {left:?} {rule:?}"
    );
    assert_eq!(
        right.left(),
        rule.right(),
        "the chevron half must start at the rule's other edge: {rule:?} {right:?}"
    );
    assert_eq!(
        rule.width(),
        1.0,
        "the rule between the halves is one hairline, got {:.1}pt",
        rule.width()
    );
    assert_eq!(
        rule.height(),
        left.height(),
        "the rule spans the control's full height, so the halves read as one \
         shape split rather than as two of different sizes: {rule:?} vs {left:?}"
    );

    // Both halves carry the brand fill: one brand run, no second fill elsewhere.
    assert_eq!(left.height(), right.height());
    assert!(
        (left.height() - 32.0).abs() < 0.5,
        "the Commit control takes the shared primary's 32pt, got {:.1}pt",
        left.height()
    );

    // Content-width: measured against the row it sits in, not against a guess.
    let inner = card.width() - 2.0 * turbogit_ui::theme::PANEL_PADDING;
    let total = right.right() - left.left();
    assert!(
        total < inner * 0.6,
        "the Commit control is content-width: it measures {:.1}pt of the {inner:.1}pt \
         row it sits in. A stretched primary is a different control, and a wider \
         one is how this card solves its layout by giving back the width the \
         ticket said it would keep.",
        total
    );
    // And the two secondary actions still sit to its right on the same row.
    let commit = h.get_by_label("Commit changes").rect();
    for label in ["Shelve\u{2026}", "Stash\u{2026}"] {
        let r = h.get_by_label(label).rect();
        assert!(
            r.left() > right.right(),
            "`{label}` sits to the right of the Commit control, got {r:?} vs {right:?}"
        );
        assert!(
            (r.center().y - commit.center().y).abs() < 1.0,
            "`{label}` is on the Commit control's own row: {r:?} vs {commit:?}"
        );
    }
}

/// **The menu still opens — from the chevron half.**
///
/// The pitfall this guards is reading "one Commit button with a chevron inside"
/// as "delete the split menu". The chevron half is inside the control's bounds
/// and carries its own accessible name, and pressing it opens the alternatives
/// menu, and picking from that menu still commits.
#[test]
fn the_alternatives_menu_still_opens_from_the_chevron_half() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-menu");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    let before = repo.commit_count();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "menu subject".into();
    h.run();

    // The chevron half is the control's right half — not a button beside it.
    let label = h.get_by_label("Commit changes").rect();
    let chevron = h.get_by_label("Commit split options").rect();
    assert!(
        chevron.left() >= label.right() - 1.0,
        "the chevron half is the second half of the control, to the right of the \
         label half: {chevron:?} vs {label:?}"
    );
    assert!(
        (chevron.height() - label.height()).abs() < 0.5,
        "both halves are the same height, or they read as two controls of \
         different sizes: {chevron:?} vs {label:?}"
    );

    // …and pressing it opens the menu, whose first item commits and pushes.
    open_commit_split_dropdown(&h);
    h.run();
    h.get_by_label("Commit and Push...").click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"menu subject".to_string())
            && repo.commit_count() == before + 1),
        "the chevron half still opens a working alternatives menu: subjects={:?} \
         count={} (was {before})",
        repo.subjects(),
        repo.commit_count()
    );
    assert_eq!(h.state().ui.dialog, Some(Dialog::Push));
}

/// **The well renders at about 76 pt AND the file list beside it still shows
/// multiple rows — measured together.**
///
/// Neither number is a useful ratchet alone: a 76 pt well next to a one-row list
/// is a failure, and a 600 pt well next to a generous list passes the first
/// assertion. So this asserts the well's height *and* the height the list keeps
/// after it, from the column's own clip rect rather than from a hard-coded
/// window height.
#[test]
fn the_well_is_about_76px_and_the_file_list_still_gets_rows() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-heights");
    seed_changes(&repo.path);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = commit_card(&h);
    let well = message_well(&h, card);
    let changes = changes_card_below(&h, card);
    let column_bottom = commit_column_bottom(&h);

    assert!(
        (well.height() - turbogit_ui::ui::commit_window::MESSAGE_WELL_HEIGHT).abs() <= 4.0,
        "the message well renders at about {:.0}pt ({} rows of body text in its \
         own padding), got {:.1}pt",
        turbogit_ui::ui::commit_window::MESSAGE_WELL_HEIGHT,
        turbogit_ui::ui::commit_window::MESSAGE_WELL_HEIGHT,
        well.height()
    );

    // What is left of the column for the list, from the top of the list card to
    // the bottom of the column the list is clipped to.
    let remaining = column_bottom - changes.top();
    assert!(
        remaining >= 3.0 * turbogit_ui::theme::FILE_ROW_HEIGHT,
        "the file list keeps at least three rows of the {:.0}pt the column has \
         left after the well: {remaining:.1}pt of column, list card {changes:?}, \
         column bottom {column_bottom:.1}. A well that grows until the list is \
         one row solves the message at the list's expense.",
        remaining
    );

    // …and the rows are actually painted and actually *inside* the clip rect, so
    // "shows multiple rows" is about what is on screen rather than about what
    // was laid out past the bottom.
    let names = ["base.txt", "added.txt", "untracked.txt"];
    for name in names {
        let g = painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == name)
            .unwrap_or_else(|| panic!("the file list must still paint `{name}`"));
        let clipped = h
            .output()
            .shapes
            .iter()
            .find(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == name))
            .map(|c| c.clip_rect)
            .expect("the row's painted shape");
        assert!(
            changes.contains(g.pos) && clipped.contains_rect(g.rect),
            "`{name}` must be inside the changes card {changes:?} and inside the \
             clip rect {clipped:?} that bounds it: painted {}",
            g.rect
        );
    }
}

/// **The well, Template and Clear all paint the raised-on-CARD fill, and none of
/// them paints the raised-on-background fill.**
///
/// The commit card is a content-surface card, so every control raised on it has
/// to take the *second* rung of the ladder. `RAISED` is 1.1:1 against this card
/// and is the rung for controls on the app or a panel background; a Template
/// button painted in it is a grey shape the eye cannot find. Asserting the exact
/// token (and naming the wrong one) is what makes the mutation — one word,
/// `RAISED_ON_CARD` to `RAISED` — a failure rather than a comment.
#[test]
fn the_well_template_and_clear_paint_the_raised_on_card_fill() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-raised");
    seed_changes(&repo.path);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = commit_card(&h);

    for (who, rect) in [
        ("the message well", message_well(&h, card)),
        ("Template", h.get_by_label("Template").rect()),
        ("Clear", h.get_by_label("Clear").rect()),
        ("Amend's checkbox", {
            let node = h.get_by_label("Amend").rect();
            egui::Rect::from_center_size(
                egui::pos2(node.left() + 8.0, node.center().y),
                egui::vec2(16.0, 16.0),
            )
        }),
    ] {
        let on_it: Vec<(egui::Rect, egui::Color32)> = filled_rects(&h)
            .into_iter()
            .filter(|(painted, _)| card.contains_rect(*painted) && rect.intersects(*painted))
            .collect();
        assert!(
            on_it.iter().any(|(painted, fill)| {
                *fill == Palette::RAISED_ON_CARD && rect.contains_rect(*painted)
            }),
            "{who} must paint the RAISED_ON_CARD fill over {rect:?} — the card is a \
             content-surface card, so a raised control on it takes the SECOND rung \
             of the ladder. Painted there: {on_it:?}"
        );
        assert!(
            !on_it.iter().any(|(_, fill)| *fill == Palette::RAISED),
            "{who} must not paint the raised-on-BACKGROUND fill (Palette::RAISED, \
             #2B2D30) on a content-surface card: it measures 1.1:1 against \
             CONTENT_BG and is invisible there. Painted there: {on_it:?}"
        );
    }

    // The two rungs are different values, so the assertion above is not
    // satisfied by naming the wrong one.
    assert_ne!(Palette::RAISED, Palette::RAISED_ON_CARD);
}

/// **The Commit control is the only brand-filled control on the card.**
///
/// Scanned as *fills*, not as galley inks: a galley is laid out in white and
/// inked at paint time, so an ink-based scan would either read white for
/// everything or need the paint-time override rule. The scan walks every filled
/// rect on the card and requires every brand one to belong to the Commit
/// control's bounds — which is the form the claim takes: not "one brand rect"
/// (the control legitimately paints two halves) but "no brand fill outside the
/// one control".
#[test]
fn the_commit_control_is_the_only_brand_filled_control_on_the_card() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-brand");
    seed_changes(&repo.path);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "brand subject".into();
    h.run();

    let card = commit_card(&h);
    let label = h.get_by_label("Commit changes").rect();
    let chevron = h.get_by_label("Commit split options").rect();
    let control = egui::Rect::from_min_max(label.min, chevron.max);

    let mut brand: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, fill)| *fill == Palette::ACCENT && card.contains_rect(*rect))
        .map(|(rect, _)| rect)
        .collect();
    brand.sort_by(|a, b| a.left().partial_cmp(&b.left()).expect("finite left"));
    assert_eq!(
        brand.len(),
        2,
        "the card's brand fill is the Commit control and nothing else: expected its \
         two halves, found {brand:?} on the card {card:?}"
    );
    for rect in &brand {
        assert!(
            control.contains_rect(*rect),
            "a brand-filled control is the card's only primary; {rect:?} is outside \
             the Commit control {control:?}"
        );
    }
    assert_eq!(
        Palette::ACCENT,
        Palette::BRAND,
        "one accent token, one name"
    );
}

/// **A typed message is still editable, the counter still tracks the subject, and
/// committing still dispatches exactly as before.**
///
/// The card was rebuilt around the well and its meta row, so the three things
/// that make it a commit form rather than a picture have to be re-asserted: the
/// well takes keystrokes, the counter reads the subject it just received, and
/// the Commit control still runs the same commit.
#[test]
fn a_typed_message_is_editable_counted_and_commits() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-typed");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    let before = repo.commit_count();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();

    // The well is a text field, and it takes the typing.
    h.get_by_label("Commit message").focus();
    h.get_by_label("Commit message")
        .type_text("typed from the harness");
    h.run();
    assert_eq!(
        h.state().ui.commit_message,
        "typed from the harness",
        "the message well must still be editable"
    );

    // The counter tracks the subject it just received.
    let counter = counter_rect(&h);
    let subject_len = "typed from the harness".chars().count();
    assert!(
        painted_text(&h)
            .iter()
            .any(|t| t == &format!("Subject: {subject_len}/50")),
        "the counter must track the subject length: painted {:?}",
        painted_text(&h)
    );
    let card = commit_card(&h);
    assert!(
        card.contains(egui::pos2(counter.left(), counter.top())),
        "the counter stays on the card {card:?}, got {counter:?}"
    );

    // And Commit still dispatches.
    commit_action_button(&h).click();
    h.run();
    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"typed from the harness".to_string())
            && repo.commit_count() == before + 1),
        "the Commit control must still dispatch the same commit: subjects={:?} \
         count={} (was {before})",
        repo.subjects(),
        repo.commit_count()
    );
}

/// **The `+N −M` pair prints twice in the preview card and both copies are the
/// same pair.**
///
/// Ticket 12 moved the counts onto the diff pane's file row, so this card paints
/// them in its summary strip *and* on the file row. The duplication is
/// deliberate (see `diff_stats` in `commit_window.rs`): the header is the card's
/// persistent identity row, and it already restates the file's name and status,
/// which the file row prints too. What is forbidden is the two copies
/// **disagreeing** — a header that says `+2 −1` over a file row that says
/// `+1 −0` is a real defect, and nothing in either view would catch it.
#[test]
fn the_preview_cards_two_line_count_copies_agree() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-counts");
    std::fs::write(repo.path.join("base.txt"), "a\nb\nc\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.path.join("base.txt"), "a\nx\ny\nc\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();

    assert!(
        wait_until(15_000, || {
            h.run();
            let text = painted_text(&h);
            text.iter().any(|t| t.contains("+2")) && text.iter().any(|t| t.contains("\u{2212}1"))
        }),
        "both copies of the change-size stats must paint once the diff lands: {:?}",
        painted_text(&h)
    );

    // Every painted `+N` / `−M` pair in the preview card must be the same pair.
    let card = card_rect(
        &h,
        galley_origin(&h, "Preview").expect("the preview heading"),
    );
    let pairs: Vec<(String, egui::Rect)> = painted_galleys(&h)
        .into_iter()
        .filter(|g| card.contains(g.pos))
        .filter(|g| {
            g.text
                .strip_prefix('+')
                .or_else(|| g.text.strip_prefix('\u{2212}'))
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|g| (g.text, g.rect))
        .collect();
    let added: Vec<&str> = pairs
        .iter()
        .filter(|(t, _)| t.starts_with('+'))
        .map(|(t, _)| t.as_str())
        .collect();
    let removed: Vec<&str> = pairs
        .iter()
        .filter(|(t, _)| t.starts_with('\u{2212}'))
        .map(|(t, _)| t.as_str())
        .collect();
    assert!(
        added.len() >= 2 && removed.len() >= 2,
        "the pair is painted in the card's summary strip AND on the file row, so \
         both copies must be there: added={added:?} removed={removed:?} pairs={pairs:?}"
    );
    for t in &added {
        assert_eq!(
            *t, "+2",
            "every `+N` in the preview card is the same file's count: {added:?}"
        );
    }
    for t in &removed {
        assert_eq!(
            *t, "\u{2212}1",
            "every `−M` in the preview card is the same file's count: {removed:?}"
        );
    }
    // And the two copies are on different rows, which is what makes them a
    // summary strip and a file row rather than one pair drawn twice by accident.
    let header_copy = pairs
        .iter()
        .filter(|(t, _)| t.starts_with('+'))
        .map(|(_, r)| r)
        .min_by(|a, b| a.top().partial_cmp(&b.top()).expect("finite top"))
        .expect("the header's copy");
    let row_copy = pairs
        .iter()
        .filter(|(t, _)| t.starts_with('+'))
        .map(|(_, r)| r)
        .max_by(|a, b| a.top().partial_cmp(&b.top()).expect("finite top"))
        .expect("the file row's copy");
    assert!(
        row_copy.top() > header_copy.bottom() + 8.0,
        "the two copies sit on different rows: header {header_copy:?} vs file row \
         {row_copy:?}"
    );
}
