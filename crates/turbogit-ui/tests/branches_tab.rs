//! Issue 03 — Branches tab frame + warm grouped list (spine, design doc §12–§16).
//!
//! The empty Branches tab becomes the screen: a one-row toolbar, the grouped
//! branch list (Local expanded, Remote expanded, Tags collapsed, each header
//! carrying its live count), and the 280px right-hand detail panel showing a
//! quiet selection prompt. Branch data is warm from repo open; slow reads show
//! a muted "reading branches…" line; a repo with no branches gets one sentence
//! plus a single create action — never an empty tree with headers. The current
//! branch is visible without scrolling; branch names truncate in the middle.
//! Asserts only on public surfaces: painted text/geometry + `AppState`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::Datelike;
use egui_kittest::{Harness, Node, kittest::NodeT as _, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::harness::{
    PaintedGalley, assert_not_painted, assert_painted, filled_circles, filled_rects, galley_origin,
    painted_galleys, painted_text, settle_quiet,
};
use test_support::srcscan;
use turbogit_app::state::{AppState, Tab};

// --- git fixture -------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit_readme(repo: &Path) {
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "init"]);
}

/// A project dir holding one real repo `alpha` on `main` with local branches
/// `feature-a`, `feature-b`, `zebra` (local-only), tag `v1.0`, and a bare
/// remote `origin` carrying `main` and a remote-only branch (`remote-only`).
fn single_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    commit_readme(&alpha);
    // The bare clone is made from the base commit so the local branches
    // created below never exist on the remote (labels stay unambiguous).
    git(&project, &["clone", "--bare", "alpha", "origin.git"]);
    git(&alpha, &["remote", "add", "origin", "../origin.git"]);
    git(&alpha, &["push", "-q", "origin", "main"]);
    git(&alpha, &["push", "-q", "origin", "main:remote-only"]);
    for b in ["feature-a", "feature-b", "zebra"] {
        git(&alpha, &["branch", b]);
    }
    git(&alpha, &["tag", "v1.0"]);
    git(&alpha, &["fetch", "-q", "origin"]);
    (tmp, project)
}

/// A freshly initialized repo with no commits at all.
fn fresh_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    (tmp, project)
}

// --- harness -----------------------------------------------------------------

fn branches_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    let state = AppState::new(project_dir);
    let mut fonts_installed = false;
    // A small frame step keeps egui's double-click window (0.3s) reachable:
    // kittest's default 0.25s-per-frame would stretch a click's press+release
    // beyond it, making `double_clicked` untestable.
    let mut harness = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, state| {
            state.drain_events();
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    harness
}

fn open_branches_tab(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(harness);
}

/// **The invariant prefix, with the fixture as a parameter.** These render tests are not
/// all about the same repository — badges, conflicts, no branches, two sections — so
/// hard-coding `single_repo_project` would either not fit or fit by changing what they
/// exercise. The `TempDir` is returned beside the harness and must outlive it (drop it
/// and every later read fails).
fn open_branches_over(
    project: (TempDir, PathBuf),
    harness_of: fn(PathBuf) -> Harness<'static, AppState>,
) -> (TempDir, PathBuf, Harness<'static, AppState>) {
    open_branches_over_with(project, harness_of, |_| {})
}

/// The same prefix, with `edit` run in the slot between the harness loading the
/// repository and the tab's first paint.
fn open_branches_over_with(
    project: (TempDir, PathBuf),
    harness_of: fn(PathBuf) -> Harness<'static, AppState>,
    edit: impl FnOnce(&mut Harness<'static, AppState>),
) -> (TempDir, PathBuf, Harness<'static, AppState>) {
    let (project, dir) = project;
    let mut harness = harness_of(dir.clone());
    edit(&mut harness);
    open_branches_tab(&mut harness);
    (project, dir, harness)
}

/// Reveal remotes through the tree's collapsed Remote rollup.
fn show_remotes(harness: &mut Harness<'_, AppState>) {
    row_node(harness, "Remote").click();
    settle_quiet(harness);
}

// --- Cycle 1: groups with live counts ------------------------------------------

#[test]
fn tab_opens_with_groups_counts_and_nothing_selected() {
    let (_project, _dir, harness) = open_branches_over(single_repo_project(), branches_harness);

    // Group headers carry live counts: 4 locals, the Remote area collapsed
    // into a per-repo rollup (remotes are hidden by default, issue 03), 1 tag.
    assert_section(&harness, "LOCAL", 4);
    assert_painted(&harness, "1 remote · 2 branches");
    assert_section(&harness, "TAGS", 1);

    // Local members paint. Remote members stay hidden while remotes are off;
    // their counts remain in the tree rollup, not in the toolbar.
    for member in ["feature-a", "feature-b", "zebra"] {
        assert_painted(&harness, member);
    }
    assert_not_painted(&harness, "remote-only");
    assert_not_painted(&harness, "Show remotes");
    assert_not_painted(&harness, "Hide remotes");
    assert!(
        galley_origin(&harness, "2").is_none(),
        "the hidden remote-branch count no longer paints as its own label"
    );

    // Tags are collapsed by default, so v1.0 stays hidden (covered by the
    // expand/collapse test).

    assert_eq!(
        harness.state().ui.branches_tree.selected,
        None,
        "the tab opens with nothing selected"
    );
}

// --- Cycle 2: current branch visible without scrolling ---------------------------

#[test]
fn current_branch_is_first_row_and_visible_without_scrolling() {
    let (_project, _dir, harness) = open_branches_over(single_repo_project(), branches_harness);

    let current = galley_origin(&harness, "main").expect("current row painted");
    // Only local rows are visible while remotes are off (issue 03), so the
    // comparison is against the locals the user actually sees.
    for other in ["feature-a", "feature-b", "zebra"] {
        let o = galley_origin(&harness, other).expect("branch row painted");
        assert!(
            current.y < o.y,
            "current branch must be the topmost branch row ({current:?} vs {o:?})"
        );
    }
    // The current row sits inside the first group, well above the list bottom
    // (a 768px-tall harness shows far more than one 30px row).
    assert!(
        current.y < 400.0,
        "current branch is visible without scrolling, was at y={}",
        current.y
    );
}

// --- Cycle 3: group expand/collapse --------------------------------------------

#[test]
fn tags_group_starts_collapsed_and_headers_keep_counts() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // Local paints; the Remote area paints its collapsed rollup (remotes are
    // hidden by default), so the remote member does not.
    assert_painted(&harness, "zebra");
    assert_painted(&harness, "1 remote · 2 branches");
    assert_not_painted(&harness, "remote-only");
    // Tags is collapsed: the header with its count paints, the member does not.
    assert_section(&harness, "TAGS", 1);
    assert_not_painted(&harness, "v1.0");

    // Clicking the Tags header expands it.
    harness.get_by_label("Tags").click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "v1.0");
}

// --- Cycle 4: empty repo state ---------------------------------------------------

#[test]
fn fresh_repo_shows_one_sentence_and_create_action_not_headers() {
    let (_project, _dir, harness) = open_branches_over(fresh_repo_project(), branches_harness);

    // One explanatory sentence + a single create action; no empty group
    // headers and no blank panel.
    assert_painted(&harness, "has no branches yet");
    assert_painted(&harness, "Create the first branch");
    for header in ["LOCAL", "REMOTE", "TAGS"] {
        assert_not_painted(&harness, header);
    }
}

// --- Cycle 5: the list owns the whole width --------------------------------------

#[test]
fn the_branch_list_fills_the_width_the_detail_panel_used_to_take() {
    let (_project, _dir, harness) = open_branches_over(single_repo_project(), branches_harness);

    // The 280px panel is gone: a row reaches into the strip it reserved.
    let row = row_node(&harness, "feature-a");
    assert!(
        row.rect().right() > 1024.0 - 280.0 + 40.0,
        "a row spans the content width, was {:?}",
        row.rect()
    );
    assert_painted(&harness, "feature-a");
    assert_not_painted(&harness, "Select a branch");
}

// --- Cycle 6: reading state is first-class (no blank panel) -----------------------

#[test]
fn reading_state_replaces_a_blank_panel() {
    use turbogit_domain::model::{Root, RootId, RootStatus};
    use turbogit_ui::ui::branches::{ListState, list_state};

    let root = Root {
        id: RootId(PathBuf::from("/r").into()),
        path: PathBuf::from("/r"),
        remotes: vec![],
        branches: vec![],
        current_branch: None,
        head: None,
        status: RootStatus::default(),
    };
    // A scan still in flight: muted reading state, never a blank panel.
    assert_eq!(list_state(&root, true), ListState::Reading);
    // The same repo after the scan finished: it genuinely has no branches.
    assert_eq!(list_state(&root, false), ListState::Empty);
}

// --- Cycle 7: middle truncation never end-clips -------------------------------------

#[test]
fn long_branch_names_truncate_in_the_middle_on_the_row() {
    let (_project, _dir, harness) =
        open_branches_over_with(single_repo_project(), branches_harness, |harness| {
            // A deliberately over-long name, so the row has something to elide.
            {
                let st = harness.state_mut();
                let id = st.selected_root.clone().expect("selected root");
                if let Some(r) = st.multi.roots.iter_mut().find(|r| r.id == id) {
                    r.branches.push(turbogit_domain::model::Branch {
                        name: "feature/multi-root-executor-rewrite".into(),
                        kind: turbogit_domain::model::BranchKind::Local,
                        tracking: None,
                        favorite: false,
                        protected: false,
                        exists: true,
                        ahead: 0,
                        behind: 0,
                        gone: false,
                        last_touched: None,
                        tip: None,
                        remote: None,
                    });
                }
            }
        });

    // The shared "feature/" prefix groups into its own directory header; the
    // leaf keeps its own name, middle-truncated so both identifying ends
    // survive (issue 02: directory subgroups + stripped labels).
    assert_painted(&harness, "feature/");
    let texts: Vec<String> = painted_text(&harness);
    let row = texts
        .iter()
        .find(|t| t.contains("multi") && t.contains("rewrite"))
        .expect("long name truncated in the middle");
    assert!(
        row.starts_with("multi") && row.ends_with("rewrite"),
        "middle truncation keeps both ends, got {row:?}"
    );
}

// --- Cycle 8: row state at a glance (issue 04) -------------------------------------

/// Sync fixture: `alpha` on `main` with `feat` tracking `origin/main`
/// (ahead 2, behind 1), `ghost` tracking a pruned `origin/ghost` (gone), and
/// untracked `zebra` on the 3-week-old commit (stale, dimmed but shown).
fn sync_repo_project() -> (TempDir, PathBuf) {
    let now = chrono::Utc::now().timestamp();
    let old = now - 21 * 24 * 60 * 60;

    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    git(&alpha, &["config", "user.email", "t@t"]);
    git(&alpha, &["config", "user.name", "t"]);
    commit_pinned(&alpha, "c1", old);

    git(&project, &["clone", "--bare", "alpha", "origin.git"]);
    git(&alpha, &["remote", "add", "origin", "../origin.git"]);
    git(&alpha, &["push", "-q", "origin", "main"]);

    git(&alpha, &["branch", "ghost"]);
    git(&alpha, &["push", "-q", "-u", "origin", "ghost"]);
    git(&alpha, &["fetch", "-q", "origin"]);
    git(&alpha, &["branch", "zebra"]);

    git(&alpha, &["branch", "feat"]);
    git(&alpha, &["branch", "--set-upstream-to=origin/main", "feat"]);
    git(&alpha, &["checkout", "-q", "feat"]);
    commit_pinned(&alpha, "c2", now);
    commit_pinned(&alpha, "c3", now);
    git(&alpha, &["checkout", "-q", "main"]);
    commit_pinned(&alpha, "c4", now);
    git(&alpha, &["push", "-q", "origin", "main"]);
    git(&alpha, &["branch", "--set-upstream-to=origin/main", "main"]);

    git(
        &project.join("origin.git"),
        &["update-ref", "-d", "refs/heads/ghost"],
    );
    git(&alpha, &["fetch", "-q", "--prune", "origin"]);
    (tmp, project)
}

fn commit_pinned(repo: &Path, msg: &str, epoch: i64) {
    std::fs::write(repo.join("f.txt"), format!("{msg}\n")).unwrap();
    git(repo, &["add", "."]);
    let out = Command::new("git")
        .args(["commit", "-q", "-m", msg])
        .env("GIT_AUTHOR_DATE", format!("@{epoch} +0000"))
        .env("GIT_COMMITTER_DATE", format!("@{epoch} +0000"))
        .current_dir(repo)
        .output()
        .expect("git commit");
    assert!(
        out.status.success(),
        "commit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A group header paints its uppercase label and its live count as a separate
/// badge beside it (ticket 05) — not as one string.
#[track_caller]
fn assert_section(harness: &Harness<'_, AppState>, label: &str, count: usize) {
    let galleys = painted_galleys(harness);
    let head = galleys.iter().find(|g| g.text == label).unwrap_or_else(|| {
        panic!(
            "`{label}` paints no label; painted {:?}",
            painted_text(harness)
        )
    });
    assert!(
        galleys.iter().any(|g| {
            g.text == count.to_string()
                && (g.pos.y - head.pos.y).abs() < 12.0
                && g.pos.x > head.rect.right()
        }),
        "`{label}`'s count {count} is not a badge beside it; painted {:?}",
        painted_text(harness)
    );
}

#[test]
fn rows_render_icon_count_pairs_in_sync_gone_and_upstream() {
    let (_project, _dir, harness) = open_branches_over(sync_repo_project(), branches_harness);

    // feat: diverged from origin/main → one badge per direction, each with its
    // own arrow icon and its words.
    assert_painted(&harness, "2 ahead");
    assert_painted(&harness, "1 behind");
    assert_painted(&harness, "origin/main");
    // main: tracks origin/main and is in sync → unmarked (no "in sync" label).
    assert_not_painted(&harness, "in sync");
    // ghost: upstream deleted → gone marker.
    assert_painted(&harness, "gone");
    assert_painted(&harness, "origin/ghost");
    // Stale zebra is dimmed but never hidden (still painted).
    assert_painted(&harness, "zebra");
}

/// The detail panel's relationship line and the row's badges must say the same
/// thing the same way — `sync_badge` is the only place the words are built.
#[test]
fn rows_never_paint_absolute_dates() {
    let (_project, _dir, harness) = open_branches_over(sync_repo_project(), branches_harness);

    let year = format!("{}", chrono::Utc::now().year());
    let texts: Vec<String> = painted_text(&harness);
    assert!(
        !texts.iter().any(|t| t.contains(&year)),
        "absolute dates must never render on rows; got {texts:?}"
    );
}

// --- Cycle 9: selection (issue 05) ------------------------------------------------

#[test]
fn click_selects_the_row_visibly_and_never_checks_out() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // An unselected row carries no selection fill at all.
    assert!(
        !filled_rects(&harness)
            .iter()
            .any(|(_, fill)| *fill == turbogit_ui::theme::Palette::ROW_SELECTED),
        "nothing is selected at rest"
    );

    // The local row (first match; the remote group repeats short names).
    harness
        .get_all_by_label("feature-a")
        .next()
        .expect("local feature-a row")
        .click();
    settle_quiet(&mut harness);

    // Selection still has a job after the detail panel went: it marks the row.
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("feature-a")
    );
    let row = row_node(&harness, "feature-a").rect();
    assert!(
        filled_rects(&harness).iter().any(|(r, fill)| *fill
            == turbogit_ui::theme::Palette::ROW_SELECTED
            && (r.center() - row.center()).length() < 2.0),
        "the selected row is filled with the selected-row fill, rows: {row:?}"
    );

    // Clicking never checks out.
    let cur = harness
        .state()
        .selected_root
        .as_ref()
        .and_then(|id| harness.state().multi.by_id(id))
        .and_then(|r| r.current_branch.clone());
    assert_eq!(cur.as_deref(), Some("main"));
}

/// The list spans the whole content width, so the only horizontal bound
/// separating a list control from a shell-chrome one is the window's own
/// right edge. Vertically the list is bounded by the tab strip instead —
/// see [`tool_window_top`], which derives that edge from painted geometry.
const LIST_RIGHT: f32 = 1024.0;
/// The window's own left edge: the bound that keeps a band name read here from
/// picking up the sidebar's project row, which prints the same repository name.
const LIST_LEFT: f32 = 0.0;

/// The tool window's top edge, derived from the shell's own tab strip rather
/// than restated as a literal.
///
/// The active tab item's rect ends exactly where the Branches list begins, so
/// it is the one painted rect that separates a list control (its repo-header
/// `Fetch`) from the chrome above it. Deriving it here keeps this bound honest
/// when the header / tab-strip metrics move: nothing in this file hardcodes a
/// chrome offset, so the topbar's deletion (and any future band change) cannot
/// silently leave the filter one pixel too tight.
#[track_caller]
fn tool_window_top(harness: &Harness<'_, AppState>) -> f32 {
    harness.get_by_label("Changes").rect().max.y
}

/// Step frames until `pred` holds on public state (async op completion).
fn pump_until(harness: &mut Harness<'_, AppState>, what: &str, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..600 {
        if pred(harness.state()) {
            return;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for: {what}\n  last_error={:?}\n  toast={:?}\n  merge={} resolver={}\n  painted={:?}",
        harness.state().last_error,
        harness.state().ui.toast,
        harness.state().ui.merge_in_progress,
        harness.state().ui.conflict_resolver_open,
        painted_text(harness)
    );
}

/// Open a row's context menu — now the only place a branch action lives, so
/// every former detail-panel click-through starts here.
fn open_row_menu(harness: &mut Harness<'_, AppState>, branch: &str) {
    row_node(harness, branch).click_secondary();
    harness.step();
    harness.step();
    harness.remove_cursor();
    harness.step();
}

/// Run one menu item against `branch`.
fn menu_action(harness: &mut Harness<'_, AppState>, branch: &str, label: &str) {
    open_row_menu(harness, branch);
    harness
        .get_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("menu item {label}"))
        .click();
    settle_quiet(harness);
}

/// The row's clickable node: the row is a Button whose accessible label is
/// the branch name (the inner text Label also matches by name, so scope by
/// role to keep queries unambiguous).
fn row_node<'t>(harness: &'t Harness<'_, AppState>, name: &str) -> egui_kittest::Node<'t> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(name.to_string()))
        .unwrap_or_else(|| panic!("row button for {name}"))
}

/// The keyboard path: click the row, then Enter.
fn click_then_enter(harness: &mut Harness<'static, AppState>, branch: &str) {
    harness
        .get_all_by_label(branch)
        .next()
        .unwrap_or_else(|| panic!("local {branch} row"))
        .click();
    settle_quiet(harness);
    harness.key_press(egui::Key::Enter);
}

/// The mouse path: two clicks inside egui's double-click window, which
/// `branches_harness`'s 1/60s frame step keeps reachable.
fn double_click(harness: &mut Harness<'static, AppState>, branch: &str) {
    let node = row_node(harness, branch);
    node.click();
    let _ = node;
    harness.step();
    let node = row_node(harness, branch);
    node.click();
    let _ = node;
}

/// One gesture for checking a list row out, aimed at `branch`.
type CheckoutGesture = fn(&mut Harness<'static, AppState>, &str);

/// Every way the list says "check this one out": the gesture, its branch, and the
/// name the failure quotes. Each row keeps its own gesture so a red row names the
/// input that broke.
const CHECKOUT_GESTURES: &[(&str, CheckoutGesture, &str)] = &[
    (
        "Enter checks out the selected row",
        click_then_enter,
        "feature-b",
    ),
    ("double-click checks out the row", double_click, "zebra"),
];

#[test]
fn every_checkout_gesture_switches_branches() {
    for &(what, gesture, branch) in CHECKOUT_GESTURES {
        let (_project, _dir, mut harness) =
            open_branches_over(single_repo_project(), branches_harness);
        gesture(&mut harness, branch);
        pump_until(&mut harness, what, |s| {
            s.selected_root
                .as_ref()
                .and_then(|id| s.multi.by_id(id))
                .and_then(|r| r.current_branch.clone())
                .as_deref()
                == Some(branch)
        });
    }
}

// --- Cycle 10: search & jump (issue 06) -------------------------------------------

fn type_search(harness: &mut Harness<'_, AppState>, text: &str) {
    let search = harness.get_by_label("Search branches");
    search.focus();
    search.type_text(text);
    settle_quiet(harness);
}

#[test]
fn typing_filters_live_and_counts_update() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    type_search(&mut harness, "feat");
    assert_section(&harness, "LOCAL", 2);
    assert_painted(&harness, "feature-a");
    assert_painted(&harness, "feature-b");
    // zebra and the tags stay filtered out (the shell chrome always paints
    // `main` elsewhere, so the current branch is not asserted absent).
    assert_not_painted(&harness, "zebra");
    assert_not_painted(&harness, "remote-only");
}

#[test]
fn fuzzy_query_matches_prefix_dots_and_multiword() {
    let (_project, _dir, mut harness) =
        open_branches_over_with(single_repo_project(), branches_harness, |harness| {
            let st = harness.state_mut();
            let id = st.selected_root.clone().expect("selected root");
            if let Some(r) = st.multi.roots.iter_mut().find(|r| r.id == id) {
                r.branches.push(turbogit_domain::model::Branch {
                    name: "feature/multi-root-executor".into(),
                    kind: turbogit_domain::model::BranchKind::Local,
                    tracking: None,
                    favorite: false,
                    protected: false,
                    exists: true,
                    ahead: 0,
                    behind: 0,
                    gone: false,
                    last_touched: None,
                    tip: None,
                    remote: None,
                });
            }
        });

    // `mre` surfaces feature/multi-root-executor (subsequence match): the
    // shared "feature/" prefix groups into its directory header and the leaf
    // shows the stripped name (issue 02)…
    type_search(&mut harness, "mre");
    assert_painted(&harness, "feature/");
    assert_painted(&harness, "multi-root-executor");
    // …and a multi-word query like `multi root` matches too.
    harness.state_mut().ui.branches_filter.clear();
    type_search(&mut harness, "multi root");
    assert_painted(&harness, "multi-root-executor");
}

#[test]
fn query_matches_the_tip_commit_message() {
    let (_project, _dir, mut harness) =
        open_branches_over_with(single_repo_project(), branches_harness, |harness| {
            let st = harness.state_mut();
            let id = st.selected_root.clone().expect("selected root");
            if let Some(r) = st.multi.roots.iter_mut().find(|r| r.id == id) {
                r.branches.push(turbogit_domain::model::Branch {
                    name: "fix/dirty-worktree".into(),
                    kind: turbogit_domain::model::BranchKind::Local,
                    tracking: None,
                    favorite: false,
                    protected: false,
                    exists: true,
                    ahead: 0,
                    behind: 0,
                    gone: false,
                    last_touched: None,
                    tip: Some(turbogit_domain::model::BranchTip {
                        short_hash: "abcd123".into(),
                        message: "fix the dirty worktree detection".into(),
                        author: "t".into(),
                        time: chrono::Utc::now(),
                    }),
                    remote: None,
                });
            }
        });

    // Remembering what a branch contains, not its name. The shared "fix/"
    // prefix groups into its directory header; the leaf keeps the stripped
    // name (issue 02).
    type_search(&mut harness, "worktree");
    assert_painted(&harness, "fix/");
    assert_painted(&harness, "dirty-worktree");
    assert_not_painted(&harness, "feature-a");
}

#[test]
fn no_match_says_create_it_inside_the_list() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    type_search(&mut harness, "zzz-no-branch");
    assert_painted(&harness, "no branch called zzz-no-branch. Create it?");
}

#[test]
fn clearing_restores_selection_and_the_full_list() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // Select a branch first, then filter the list to zebra. The selection
    // persists (the detail panel keeps the selected branch — search is for
    // jumping, and clearing restores the prior selection).
    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    type_search(&mut harness, "zebra");
    assert_painted(&harness, "zebra");
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("feature-a"),
        "filtering never drops the selection"
    );

    // Esc clears the filter: the full list (and its counts) return, and the
    // prior selection is intact.
    harness.key_press(egui::Key::Escape);
    settle_quiet(&mut harness);
    assert_section(&harness, "LOCAL", 4);
    assert_painted(&harness, "feature-a");
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("feature-a"),
        "clearing search restores the prior selection"
    );
}

#[test]
fn opening_the_tab_focuses_search_and_consumes_the_flag() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);

    // Click the real tab strip entry: focus lands in the search box.
    harness.get_by_label("Branches").click();
    settle_quiet(&mut harness);
    assert!(
        harness.get_by_label("Search branches").is_focused(),
        "focus lands in the search box when the Branches tab opens"
    );
    assert!(
        !harness.state().ui.branches_focus_search,
        "the focus request is consumed on render"
    );
}

// --- Cycle 11: checkout with dirty-worktree care (issue 07) -------------------------

fn dirty_root(harness: &mut Harness<'_, AppState>) {
    let st = harness.state_mut();
    let id = st.selected_root.clone().unwrap();
    if let Some(r) = st.multi.roots.iter_mut().find(|r| r.id == id) {
        r.status.changes.push(turbogit_domain::model::Change {
            path: PathBuf::from("README.md"),
            status: turbogit_domain::model::ChangeStatus::Modified,
            chunks: vec![],
            staged: false,
            unstaged: true,
            orig_path: None,
        });
    }
}

fn current_branch(harness: &Harness<'_, AppState>) -> Option<String> {
    harness
        .state()
        .selected_root
        .as_ref()
        .and_then(|id| harness.state().multi.by_id(id))
        .and_then(|r| r.current_branch.clone())
}

#[test]
fn dirty_tree_checkout_offers_plain_choices_and_cancel_leaves_everything() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);
    dirty_root(&mut harness);

    // Checkout (Enter on the selected row) hits the care dialog, never a
    // bare refusal.
    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Enter);
    settle_quiet(&mut harness);

    // Plain-language care dialog — never a bare "cannot checkout".
    assert_painted(&harness, "You have uncommitted changes");
    assert_painted(&harness, "Bring along");
    assert_painted(&harness, "Set aside");
    assert_painted(&harness, "Cancel");
    assert_painted(&harness, "if any conflict with the other branch");

    // Cancel leaves the working tree and current branch untouched.
    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);
    assert_eq!(current_branch(&harness).as_deref(), Some("main"));
    assert!(
        harness.state().multi.roots[0].status.modified() > 0,
        "the dirty tree is untouched"
    );
    assert!(harness.state().ui.confirm.is_none());
}

#[test]
fn set_aside_stashes_changes_and_returns_them_intact() {
    let (_project, dir) = single_repo_project();
    let alpha = dir.join("alpha");
    std::fs::write(alpha.join("README.md"), "dirty edit\n").unwrap();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);
    // Refresh so the root snapshot carries the real edit.
    {
        let st = harness.state_mut();
        let id = st.selected_root.clone().unwrap();
        st.refresh(turbogit_app::root_caches::Affected::Root(id));
    }
    settle_quiet(&mut harness);

    row_node(&harness, "feature-b").click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Enter);
    settle_quiet(&mut harness);
    harness.get_by_label("Set aside").click();
    pump_until(&mut harness, "set-aside switches branch", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.current_branch.clone())
            .as_deref()
            == Some("feature-b")
    });

    // The changes are stashed (intact) and the worktree is clean.
    let stash = git(&alpha, &["stash", "list"]);
    assert!(
        stash.contains("set aside before checkout"),
        "stash must hold the changes; got {stash:?}"
    );
    let clean_tree = std::fs::read_to_string(alpha.join("README.md")).unwrap();
    assert!(
        !clean_tree.contains("dirty"),
        "feature-b's tree is the committed version, got {clean_tree:?}"
    );

    // Coming back restores them intact.
    git(&alpha, &["checkout", "-q", "main"]);
    git(&alpha, &["stash", "pop"]);
    let restored = std::fs::read_to_string(alpha.join("README.md")).unwrap();
    assert!(
        restored.contains("dirty edit"),
        "the set-aside changes return intact, got {restored:?}"
    );
}

#[test]
fn clean_checkout_switches_and_notes_the_switch_quietly() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // Clean tree: Enter on the selected row switches directly, with no dialog.
    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Enter);
    pump_until(&mut harness, "clean checkout switches", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.current_branch.clone())
            .as_deref()
            == Some("feature-a")
    });

    // The activity area notes it quietly (Success entry, no modal).
    let last = harness.state().ui.activity.entries.last().cloned();
    let (msg, kind) = last
        .map(|e| (e.message, e.kind))
        .unwrap_or_else(|| panic!("checkout must leave an activity entry"));
    assert_eq!(msg, "Checkout feature-a");
    assert_eq!(kind, turbogit_app::activity::ActivityKind::Success);
    assert!(harness.state().ui.confirm.is_none());
}

#[test]
fn branch_checked_out_in_another_worktree_is_flagged_up_front() {
    let (_project, _dir, mut harness) =
        open_branches_over_with(single_repo_project(), branches_harness, |harness| {
            let st = harness.state_mut();
            let id = st.selected_root.clone().unwrap();
            st.caches.store_worktrees(
                id.clone(),
                vec![turbogit_domain::model::Worktree {
                    path: PathBuf::from("C:\\wt\\feature-a"),
                    branch: "feature-a".into(),
                    dirty: None,
                    root: id,
                }],
            );
        });

    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Enter);
    settle_quiet(&mut harness);
    assert_painted(&harness, "already checked out in another worktree");
    assert_painted(&harness, "C:\\wt\\feature-a");
    assert_eq!(
        current_branch(&harness).as_deref(),
        Some("main"),
        "no confusing checkout failure"
    );

    harness.get_by_label("OK").click();
    settle_quiet(&mut harness);
    assert!(harness.state().ui.confirm.is_none());
}

// --- Cycle 12: create branch (issue 08) -------------------------------------------

#[test]
fn new_branch_dialog_footer_preserves_order_minimum_targets_and_divider() {
    let (_project, _dir, mut h) = open_branches_over(single_repo_project(), branches_harness);
    h.get_by_label("New Branch").click();
    settle_quiet(&mut h);

    let create = h.get_by_label("Create");
    let cancel = h.get_by_label("Cancel");
    assert!(create.rect().left() < cancel.rect().left());
    assert!(create.rect().height() >= 28.0);
    assert!(cancel.rect().height() >= 28.0);
    assert!(
        filled_rects(&h)
            .into_iter()
            .any(|(rect, _)| (rect.height() - 1.0).abs() < 0.01 && rect.top() < create.rect().top()),
        "the shared footer divider must be present above the actions"
    );
}

#[test]
fn new_branch_is_prominent_and_defaults_base_to_current_with_switch_on() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // The primary toolbar button opens the flow.
    assert_painted(&harness, "New Branch");
    harness.get_by_label("New Branch").click();
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(turbogit_app::state::Dialog::NewBranch)
    );

    // Base defaults to the current branch; "Switch now" defaults on.
    assert_eq!(
        harness.state().ui.dlg.new_branch_base,
        turbogit_app::state::NewBranchBase::Branch("main".into()),
        "base defaults to the current branch"
    );
    assert!(
        harness.state().ui.dlg.new_branch_checkout,
        "switch-now defaults to yes"
    );
    assert_painted(&harness, "Switch to the new branch now");
}

/// One "Create" run through the New Branch dialog; `what` is the string `pump_until`
/// quotes when the row times out.
struct CreateCase {
    what: &'static str,
    /// Whether the row leaves the switch decision alone (`true`) or turns it off first.
    switch_now: bool,
    name: &'static str,
    expect_current: &'static str,
}

const CREATE_CASES: &[CreateCase] = &[
    CreateCase {
        what: "branch created and checked out",
        switch_now: true,
        name: "issue08-x",
        expect_current: "issue08-x",
    },
    CreateCase {
        what: "branch created without switching",
        switch_now: false,
        name: "issue08-no-switch",
        expect_current: "main",
    },
];

#[test]
fn creating_a_branch_honours_the_switch_decision_it_was_given() {
    for case in CREATE_CASES {
        let (_project, _dir, mut harness) =
            open_branches_over(single_repo_project(), branches_harness);
        harness.get_by_label("New Branch").click();
        settle_quiet(&mut harness);
        if !case.switch_now {
            harness.get_by_label("Switch to the new branch now").click();
            settle_quiet(&mut harness);
        }
        {
            let field = harness
                .get_all_by_role(egui::accesskit::Role::TextInput)
                .find(|n| n.accesskit_node().label().is_none())
                .expect("dialog Name input queryable");
            field.focus();
            field.type_text(case.name);
        }
        settle_quiet(&mut harness);
        harness.get_by_label("Create").click();

        // The branch lands either way; the decision decides whether it is current.
        pump_until(&mut harness, case.what, |s| {
            s.selected_root
                .as_ref()
                .and_then(|id| s.multi.by_id(id))
                .map(|r| {
                    r.current_branch.as_deref() == Some(case.expect_current)
                        && r.branches.iter().any(|b| b.name == case.name)
                })
                .unwrap_or(false)
        });
        if case.switch_now {
            // The scroll-to intent was consumed (the new current branch orders first).
            assert!(harness.state().ui.branches_tree.scroll_to.is_none());
        }
        assert_painted(&harness, case.name);
    }
}

// --- Cycle 13: merge / rebase from the detail (issue 09) ----------------------------

#[test]
fn merge_from_detail_opens_the_preflighted_dialog() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    menu_action(&mut harness, "feature-a", "Merge into main");

    // The direction is preset and the preflight (preview) is computed before
    // anything runs.
    assert_eq!(
        harness.state().ui.dialog,
        Some(turbogit_app::state::Dialog::Merge)
    );
    assert_eq!(harness.state().ui.dlg.merge_target, "feature-a");
    assert!(
        harness.state().ui.dlg.merge_preview.is_some(),
        "pre-flight preview is computed before starting"
    );
}

#[test]
fn rebase_from_detail_states_direction_and_runs() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    open_row_menu(&mut harness, "feature-b");
    harness.get_by_label("Rebase onto main").click();

    // The direction is stated in the label; the branch is checked out then
    // replayed onto main (both sit on the same commit, so the rebase is a
    // clean no-op that lands on feature-b).
    pump_until(&mut harness, "rebase onto current completes", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.current_branch.clone())
            .as_deref()
            == Some("feature-b")
    });
    let last = harness.state().ui.activity.entries.last().cloned().unwrap();
    assert!(
        last.message.starts_with("Rebase feature-b onto main"),
        "direction is stated in the report, got {:?}",
        last.message
    );
}

/// A repo where merging `conflict-a` into `main` must conflict (both changed
/// the same file).
fn conflict_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    git(&alpha, &["config", "user.email", "t@t"]);
    git(&alpha, &["config", "user.name", "t"]);
    std::fs::write(alpha.join("f.txt"), "base\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-m", "base"]);

    git(&alpha, &["checkout", "-q", "-b", "conflict-a"]);
    std::fs::write(alpha.join("f.txt"), "a\n").unwrap();
    git(&alpha, &["commit", "-am", "conflict side"]);
    git(&alpha, &["checkout", "-q", "main"]);
    std::fs::write(alpha.join("f.txt"), "b\n").unwrap();
    git(&alpha, &["commit", "-am", "main side"]);
    (tmp, project)
}

#[test]
fn conflicted_merge_hands_off_to_the_conflict_experience() {
    let (_project, _dir, mut harness) =
        open_branches_over(conflict_repo_project(), branches_harness);

    menu_action(&mut harness, "conflict-a", "Merge into main");
    harness.get_by_label("Merge").click();

    // The merge conflicts mid-way: the screen hands off to the conflict
    // experience and the mid-operation state is explicit.
    pump_until(&mut harness, "conflicted merge hands off", |s| {
        s.ui.conflict_resolver_open && s.ui.merge_in_progress
    });
    let last = harness.state().ui.activity.entries.last().cloned().unwrap();
    assert!(
        last.message.contains("unresolved conflicts"),
        "the report says what happened, got {:?}",
        last.message
    );

    // Returning to the Branches tab still shows the mid-operation row.
    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(&mut harness);
    assert_painted(&harness, "merging…");
}

// --- Cycle 14: compare from the detail (issue 10) ------------------------------------

#[test]
fn compare_from_detail_opens_read_only_and_closes_back_to_the_same_place() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // Select a branch and note the scroll position.
    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    let scroll_before = harness.state().ui.branches_tree.scroll;
    menu_action(&mut harness, "feature-a", "Compare with main");

    // The compare surface names both sides and is read-only: a commit list
    // with only view/close controls (feature-a shares main's commit, so it
    // reports the branches in sync).
    assert_painted(&harness, "Compare feature-a with main");
    assert_painted(&harness, "No commits — branches are in sync.");
    assert_painted(&harness, "Swap Branches");
    assert_painted(&harness, "Close");
    // No write-path control belongs to the compare surface.
    assert!(
        !harness
            .get_by_label("Swap Branches")
            .accesskit_node()
            .is_disabled(),
        "swapping sides stays available (a view toggle, not a write)"
    );

    // Closing returns to the same scroll position and selection.
    harness.get_by_label("Close").click();
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("feature-a"),
        "selection survives compare"
    );
    assert_eq!(
        harness.state().ui.branches_tree.scroll,
        scroll_before,
        "scroll position survives compare"
    );
    assert!(harness.state().ui.dialog.is_none());
}

// --- Cycle 15: rename inline (issue 11) ---------------------------------------------

#[test]
fn rename_is_inline_on_the_row_and_resorts_correctly() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    menu_action(&mut harness, "feature-a", "Rename branch");

    // Inline on the row — not a separate form screen.
    assert!(harness.state().ui.dialog.is_none());
    assert_eq!(
        harness.state().ui.branches_tree.renaming.as_deref(),
        Some("feature-a")
    );

    harness.state_mut().ui.branches_tree.rename_draft = "aaa-renamed".into();
    harness.get_by_label("Apply rename").click();
    pump_until(&mut harness, "inline rename applied", |s| {
        s.multi.roots[0]
            .branches
            .iter()
            .any(|b| b.name == "aaa-renamed")
    });

    // The row and detail text update; the old name is gone; the renamed
    // branch re-sorts under the current ordering (same recency, so
    // alphabetically first).
    assert_painted(&harness, "aaa-renamed");
    assert!(
        !harness.state().multi.roots[0]
            .branches
            .iter()
            .any(|b| b.name == "feature-a")
    );
}

#[test]
fn rename_discloses_that_tracking_does_not_follow() {
    let (_project, _dir, mut harness) =
        open_branches_over_with(single_repo_project(), branches_harness, |harness| {
            // feature-a needs a tracked upstream for the disclosure to have something to say.
            {
                let st = harness.state_mut();
                let id = st.selected_root.clone().unwrap();
                if let Some(r) = st.multi.roots.iter_mut().find(|r| r.id == id)
                    && let Some(b) = r.branches.iter_mut().find(|b| b.name == "feature-a")
                {
                    b.tracking = turbogit_domain::model::Upstream::from_git_ref("origin/feature-a");
                }
            }
        });

    menu_action(&mut harness, "feature-a", "Rename branch");

    // The one thing the person must know before confirming.
    assert_painted(
        &harness,
        "tracking origin/feature-a does not follow the new name — set it again after",
    );
}

#[test]
fn rename_current_branch_keeps_the_marker_and_leaves_the_tree_untouched() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    let tree_changes_before = harness.state().multi.roots[0].status.changes.len();
    menu_action(&mut harness, "main", "Rename branch");
    harness.state_mut().ui.branches_tree.rename_draft = "main2".into();
    harness.get_by_label("Apply rename").click();

    // Renaming the current branch updates the marker everywhere and never
    // disturbs the working tree.
    pump_until(&mut harness, "current branch renamed", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.current_branch.clone())
            .as_deref()
            == Some("main2")
    });
    assert_eq!(
        harness.state().multi.roots[0].status.changes.len(),
        tree_changes_before,
        "the working tree is untouched"
    );
}

// --- Cycle 16: delete with care + undo (issue 12) -------------------------------------

#[test]
fn delete_names_the_consequence_and_undo_restores() {
    let (_project, _dir, mut harness) =
        open_branches_over(conflict_repo_project(), branches_harness);

    // conflict-a carries a commit not on main: the confirmation says what
    // would become unreachable, in human terms.
    menu_action(&mut harness, "conflict-a", "Delete branch");
    assert_painted(&harness, "Delete local branch 'conflict-a'?");
    assert_painted(&harness, "has 1 commit(s) not on main");
    assert_painted(&harness, "unreachable after deleting");

    harness.get_by_label("OK").click();
    pump_until(&mut harness, "branch deleted", |s| {
        !s.multi.roots[0]
            .branches
            .iter()
            .any(|b| b.name == "conflict-a")
    });

    // The undo affordance appears for a short window and restores it.
    assert_painted(&harness, "Undo delete");
    assert!(harness.state().ui.branches_undo.is_some());
    harness.get_by_label("Undo delete").click();
    pump_until(&mut harness, "branch restored", |s| {
        s.multi.roots[0]
            .branches
            .iter()
            .any(|b| b.name == "conflict-a")
    });
    assert!(harness.state().ui.branches_undo.is_none());
}

#[test]
fn safe_to_delete_is_said_in_human_terms() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // feature-a shares main's commit: safe to delete, and the confirmation
    // says so.
    menu_action(&mut harness, "feature-a", "Delete branch");
    assert_painted(
        &harness,
        "everything on this branch already exists on main — safe to delete",
    );
}

#[test]
fn delete_refuses_a_branch_checked_out_elsewhere() {
    let (_project, _dir, mut harness) =
        open_branches_over_with(single_repo_project(), branches_harness, |harness| {
            let st = harness.state_mut();
            let id = st.selected_root.clone().unwrap();
            st.caches.store_worktrees(
                id.clone(),
                vec![turbogit_domain::model::Worktree {
                    path: PathBuf::from("C:\\wt\\feature-a"),
                    branch: "feature-a".into(),
                    dirty: None,
                    root: id,
                }],
            );
        });

    menu_action(&mut harness, "feature-a", "Delete branch");
    assert_painted(&harness, "already checked out in another worktree");
    assert_painted(&harness, "C:\\wt\\feature-a");
    assert!(
        harness.state().multi.roots[0]
            .branches
            .iter()
            .any(|b| b.name == "feature-a"),
        "the branch is not deleted"
    );
}

// --- Cycle 17: remote branches & fetch (issue 13) -------------------------------------

#[test]
fn remote_rows_group_under_their_remote_and_are_quiet() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // Remotes are hidden by default; the tree rollup reveals them for the
    // whole view (issue 03). Before expanding it no remote branches paint.
    assert_not_painted(&harness, "remote-only");
    show_remotes(&mut harness);

    // The rows nest under the remote group header, whose name is not printed
    // again on the leaves (issue 03): "remote-only", never "origin/remote-only".
    assert!(
        galley_origin(&harness, "origin").is_some(),
        "the remote group header names the remote"
    );
    assert!(
        galley_origin(&harness, "remote-only").is_some(),
        "the leaf shows the prefix-stripped name"
    );
    assert_not_painted(&harness, "origin/remote-only");
    // A quiet-ink decision, unit-tested against the palette.
    use turbogit_domain::model::BranchKind;
    let remote = harness.state().multi.roots[0]
        .branches
        .iter()
        .find(|b| b.kind == BranchKind::Remote && b.name == "remote-only")
        .expect("remote-only row");
    assert_eq!(
        remote.remote.as_deref(),
        Some("origin"),
        "the remote name rides the row"
    );
}

/// Click the repo-level Fetch inside the Branches list (issue 03). The shell
/// chrome has no `Fetch` button of its own any more (the topbar's went with
/// the topbar), so the search is bounded to the list area: below the tab strip
/// and left of the §12 detail panel.
fn click_fetch(harness: &mut Harness<'_, AppState>) {
    let list_x = LIST_RIGHT;
    let list_top = tool_window_top(harness);
    let nodes: Vec<_> = harness.get_all_by_label("Fetch").collect();
    let node = nodes
        .iter()
        .find(|n| n.rect().top() >= list_top && n.rect().min.x < list_x)
        .unwrap_or_else(|| panic!("repo-level Fetch not found below y={list_top}"));
    node.click();
}

/// Click every repository header's own Fetch — one operation per repo, which is
/// all a multi-repo refresh can be asked for from a header. One per frame: two
/// synthetic clicks in the same frame collapse into one.
fn click_repo_fetches(harness: &mut Harness<'_, AppState>) {
    let list_x = LIST_RIGHT;
    let in_list =
        |n: &Node<'_>, list_top: f32| n.rect().top() >= list_top && n.rect().min.x < list_x;
    let nth_fetch = |harness: &Harness<'_, AppState>, n: usize, list_top: f32| {
        let mut rects: Vec<egui::Rect> = harness
            .get_all_by_label("Fetch")
            .filter(|node| in_list(node, list_top))
            .map(|node| node.rect())
            .collect();
        rects.sort_by(|a, b| a.top().total_cmp(&b.top()));
        rects[n]
    };
    let list_top = tool_window_top(harness);
    assert_eq!(
        harness
            .get_all_by_label("Fetch")
            .filter(|n| in_list(n, list_top))
            .count(),
        2,
        "one Fetch per repo header"
    );
    for n in 0..2 {
        let rect = nth_fetch(harness, n, list_top);
        harness
            .get_all_by_label("Fetch")
            .find(|node| node.rect() == rect)
            .expect("that header's Fetch is still painted")
            .click();
        settle_quiet(harness);
    }
}

#[test]
fn fetch_reports_nothing_changed_or_new_remote_branches() {
    let (_project, dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // Fetch with nothing new: plainly "nothing changed".
    click_fetch(&mut harness);
    pump_until(&mut harness, "fetch reports nothing changed", |s| {
        s.ui.activity
            .entries
            .last()
            .is_some_and(|e| e.message == "Fetch · nothing changed")
    });

    // A new branch appears on the remote; fetching says so.
    git(&dir.join("origin.git"), &["branch", "fresh-remote"]);
    click_fetch(&mut harness);
    pump_until(&mut harness, "fetch reports new remote branch", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .is_some_and(|r| {
                r.branches.iter().any(|b| {
                    b.kind == turbogit_domain::model::BranchKind::Remote && b.name == "fresh-remote"
                })
            })
    });
    let last = harness.state().ui.activity.entries.last().cloned().unwrap();
    assert!(
        last.message.contains("1 new remote branches"),
        "the fetch report names what changed, got {:?}",
        last.message
    );
}

#[test]
fn switching_to_a_remote_branch_creates_a_tracking_local_in_one_intent() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);
    // Remote rows are hidden by default (issue 03): reveal them first.
    show_remotes(&mut harness);

    // Select the remote row and press Enter: "I want to work on this" becomes
    // a matching local branch that tracks it, checked out.
    row_node(&harness, "remote-only").click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Enter);
    pump_until(
        &mut harness,
        "remote checkout creates tracking local",
        |s| {
            s.selected_root
                .as_ref()
                .and_then(|id| s.multi.by_id(id))
                .map(|r| {
                    r.current_branch.as_deref() == Some("remote-only")
                        && r.branches.iter().any(|b| {
                            b.kind == turbogit_domain::model::BranchKind::Local
                                && b.name == "remote-only"
                                && b.tracking
                                    == turbogit_domain::model::Upstream::from_git_ref(
                                        "origin/remote-only",
                                    )
                        })
                })
                .unwrap_or(false)
        },
    );
}

// --- Pure logic -------------------------------------------------------------------

mod pure {
    use chrono::Utc;
    use turbogit_domain::model::{Branch, BranchKind, BranchTip};
    use turbogit_ui::ui::branches::{
        branch_matches, fuzzy_word, is_stale, ordered_locals, row_meta,
    };
    use turbogit_ui::ui::components::SyncKind;

    fn b(name: &str, touched: Option<i64>) -> Branch {
        Branch {
            name: name.into(),
            kind: BranchKind::Local,
            tracking: None,
            favorite: false,
            protected: false,
            exists: true,
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: touched
                .map(|s| chrono::DateTime::from_timestamp(s, 0).expect("post-epoch")),
            tip: None,
            remote: None,
        }
    }

    #[test]
    fn current_branch_orders_first_then_most_recent() {
        let locals = vec![
            b("feature-a", Some(100)),
            b("main", Some(300)),
            b("feature-b", Some(200)),
        ];
        let ordered: Vec<String> = ordered_locals(&locals, Some("main"))
            .into_iter()
            .map(|x| x.name)
            .collect();
        assert_eq!(ordered, vec!["main", "feature-b", "feature-a"]);
    }

    // --- Issue 04: stale threshold ------------------------------------------

    #[test]
    fn stale_threshold_is_roughly_four_weeks() {
        let now = Utc::now();
        let fresh = b("f", Some(now.timestamp() - 6 * 24 * 60 * 60));
        assert!(!is_stale(&fresh, now), "under a week is not stale");
        let edge = b("e", Some(now.timestamp() - 27 * 24 * 60 * 60));
        assert!(!is_stale(&edge, now), "27 days stays inside the window");
        let stale = b("s", Some(now.timestamp() - 30 * 24 * 60 * 60));
        assert!(is_stale(&stale, now), "30 days dims the row");
        // Unknown last-touched is never dimmed.
        let unknown = b("u", None);
        assert!(!is_stale(&unknown, now));
    }

    // --- Issue 04: row metadata ----------------------------------------------

    /// One branch shape asked of `row_meta`, and what the row must then carry. Every
    /// row is a literal, not a computation over the row above it; an empty
    /// `expect_badge` is how a row says "nothing to report".
    struct RowMetaCase {
        what: &'static str,
        name: &'static str,
        tracking: Option<&'static str>,
        ahead: usize,
        behind: usize,
        gone: bool,
        expect_upstream: Option<&'static str>,
        expect_badge: &'static [(SyncKind, &'static str)],
    }

    const ROW_META_CASES: &[RowMetaCase] = &[
        // Diverged: one badge per *direction*.
        RowMetaCase {
            what: "diverged",
            name: "feat",
            tracking: Some("origin/main"),
            ahead: 2,
            behind: 1,
            gone: false,
            expect_upstream: Some("origin/main"),
            expect_badge: &[(SyncKind::Ahead, "2 ahead"), (SyncKind::Behind, "1 behind")],
        },
        // Untracked: no upstream, no badge.
        RowMetaCase {
            what: "untracked",
            name: "zebra",
            tracking: None,
            ahead: 0,
            behind: 0,
            gone: false,
            expect_upstream: None,
            expect_badge: &[],
        },
        // In sync: silence is the answer.
        RowMetaCase {
            what: "in sync",
            name: "main",
            tracking: Some("origin/main"),
            ahead: 0,
            behind: 0,
            gone: false,
            expect_upstream: Some("origin/main"),
            expect_badge: &[],
        },
        // Upstream deleted on the remote.
        RowMetaCase {
            what: "gone upstream",
            name: "ghost",
            tracking: Some("origin/ghost"),
            ahead: 0,
            behind: 0,
            gone: true,
            expect_upstream: Some("origin/ghost"),
            expect_badge: &[(SyncKind::Gone, "gone")],
        },
    ];

    #[test]
    fn row_meta_carries_the_upstream_and_exactly_the_badges_the_state_earns() {
        let now = Utc::now();
        for case in ROW_META_CASES {
            let mut br = b(case.name, Some(now.timestamp()));
            br.tracking =
                turbogit_domain::model::Upstream::from_git_ref(case.tracking.unwrap_or(""));
            br.ahead = case.ahead;
            br.behind = case.behind;
            br.gone = case.gone;
            let meta = row_meta(&br, now);
            assert_eq!(
                meta.upstream,
                turbogit_domain::model::Upstream::from_git_ref(case.expect_upstream.unwrap_or("")),
                "the {} row carries its upstream verbatim",
                case.what
            );
            let expect: Vec<(SyncKind, String)> = case
                .expect_badge
                .iter()
                .map(|(kind, words)| (*kind, words.to_string()))
                .collect();
            assert_eq!(
                meta.badge, expect,
                "the {} row badges exactly what its state earns",
                case.what
            );
        }
    }

    // --- Issue 06: fuzzy search ---------------------------------------------

    #[test]
    fn fuzzy_word_is_subsequence_and_case_insensitive() {
        assert!(fuzzy_word("feature/multi-root-executor", "mre"));
        assert!(fuzzy_word("feature/multi-root-executor", "MRE"));
        assert!(fuzzy_word("feature/multi-root-executor", "mro"));
        // A character that does not occur at all can never match.
        assert!(!fuzzy_word("feature/multi-root-executor", "zzz"));
        assert!(fuzzy_word("anything", ""));
    }

    #[test]
    fn multiword_query_needs_every_word() {
        let br = b("feature/multi-root-executor", Some(0));
        assert!(branch_matches(&br, "multi root"));
        assert!(branch_matches(&br, "root executor"));
        assert!(!branch_matches(&br, "multi zebra"));
        // Untouched empty query matches everything.
        assert!(branch_matches(&br, "  "));
    }

    #[test]
    fn query_matches_the_tip_commit_message() {
        let mut br = b("fix/dirty-worktree", Some(0));
        br.tip = Some(BranchTip {
            short_hash: "abc1234".into(),
            message: "fix the dirty worktree detection".into(),
            author: "t".into(),
            time: Utc::now(),
        });
        assert!(branch_matches(&br, "worktree"));
        assert!(branch_matches(&br, "dirty detection"));
        assert!(!branch_matches(&br, "network"));
    }
}

// --- Cycle 18: multi-repo scope & per-repo reporting (issue 14) ---------------------

/// Two real repos `alpha` (main, feature-a, origin with a remote branch) and
/// `beta` (main, clever): the same-named current branches prove every row
/// identifies its owning repo, and each repo carries a name unique to it for
/// filtered / per-repo assertions.
fn two_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    for name in ["alpha", "beta"] {
        git(&project, &["-c", "init.defaultBranch=main", "init", name]);
        commit_readme(&project.join(name));
    }
    // The bare origin is cloned from alpha's base commit — before any local
    // branches below exist — so the remote carries only main (+ remote-only).
    git(
        &project,
        &[
            "clone",
            "--bare",
            project.join("alpha").to_str().unwrap(),
            "origin.git",
        ],
    );
    let alpha = project.join("alpha");
    git(&alpha, &["remote", "add", "origin", "../origin.git"]);
    git(&alpha, &["push", "-q", "origin", "main"]);
    git(&alpha, &["push", "-q", "origin", "main:remote-only"]);
    git(&alpha, &["branch", "feature-a"]);
    git(&project.join("beta"), &["branch", "clever"]);
    git(&alpha, &["fetch", "-q", "origin"]);
    (tmp, project)
}

/// A multi-repo harness over both roots registered deterministically
/// ([`AppState::for_roots`] keeps git ops synchronous).
fn two_repo_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    let roots = [project_dir.join("alpha"), project_dir.join("beta")];
    let state = AppState::for_roots(&project_dir, &roots);
    let mut fonts_installed = false;
    let mut harness = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, state| {
            state.drain_events();
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    harness
}

/// All row buttons carrying this branch name inside the list area (below the
/// tab strip, left of the detail panel — a same-named repo-header breadcrumb
/// or metadata row never leaks in), ordered top-to-bottom.
fn row_nodes<'h>(harness: &'h Harness<'_, AppState>, name: &str) -> Vec<egui_kittest::Node<'h>> {
    let list_x = LIST_RIGHT;
    let list_top = tool_window_top(harness);
    let mut nodes: Vec<_> = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| {
            n.accesskit_node().label() == Some(name.to_string())
                && n.rect().top() >= list_top
                && n.rect().min.x < list_x
        })
        .collect();
    nodes.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    nodes
}

#[test]
fn two_current_branches_are_distinct_rows_owned_by_their_repo() {
    let (_project, _dir, mut harness) = open_branches_over(two_repo_project(), two_repo_harness);

    // Both repos contribute their own grouped section; the two `main`s are
    // separate rows, never one anonymous row. Each section carries its own
    // Local count.
    assert_section(&harness, "LOCAL", 2);

    // What tells the two `main`s apart is the section each one sits in, not a
    // name repeated on the row. The section headers anchor that: the first main
    // falls between alpha's band and beta's, the second below beta's.
    //
    // **What moved, and why.** These were read off the band's 4px heading status
    // dot. Ticket 16 removed that dot: the band's state reaches the screen as
    // the mark pair's leading dot beside the state summary, so there is one dot
    // per band instead of two, and it is at the mark radius. The bands are now
    // located by the repository names they print, which is what a reader reads
    // them by.
    let mut header_y: Vec<f32> = ["alpha", "beta"]
        .iter()
        .map(|repo| {
            painted_galleys(&harness)
                .into_iter()
                .find(|g| g.text == *repo && g.pos.x > LIST_LEFT && g.pos.y > 0.0)
                .unwrap_or_else(|| panic!("the `{repo}` band names its repository"))
                .pos
                .y
        })
        .collect();
    header_y.sort_by(f32::total_cmp);
    assert_eq!(header_y.len(), 2, "one repo header per section");
    {
        // Remotes are hidden by default, so exactly the two local mains paint
        // as buttons; `row_nodes` sorts top-to-bottom.
        let mains = row_nodes(&harness, "main");
        assert_eq!(mains.len(), 2, "one local main per repo section");
        let ys: Vec<f32> = mains.iter().map(|n| n.rect().center().y).collect();
        assert!(
            ys[0] > header_y[0] && ys[0] < header_y[1],
            "the first main sits inside alpha's section: {ys:?} vs headers {header_y:?}"
        );
        assert!(
            ys[1] > header_y[1],
            "the second main sits inside beta's section: {ys:?} vs headers {header_y:?}"
        );
        mains[0].click();
    }
    settle_quiet(&mut harness);
    let first = harness.state().ui.branches_tree.selected_root.clone();
    {
        let mains = row_nodes(&harness, "main");
        mains[1].click();
    }
    settle_quiet(&mut harness);
    let second = harness.state().ui.branches_tree.selected_root.clone();
    assert!(
        first.is_some() && second.is_some() && first != second,
        "same-named rows resolve to different owners: {first:?} vs {second:?}"
    );
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("main")
    );
}

#[test]
fn rows_never_repeat_the_repo_name_their_section_already_states() {
    let (_project, _dir, harness) = open_branches_over(two_repo_project(), two_repo_harness);

    // The section header names the repo, so every row beneath it would be
    // repeating it. Scoped to each row's own surface: the header (and the app
    // chrome) legitimately paint the repo's name — the *row* must not.
    for (branch, repo) in [("clever", "beta"), ("feature-a", "alpha")] {
        let row = row_node(&harness, branch).rect();
        let painted: Vec<String> = galleys_in(&harness, row)
            .into_iter()
            .map(|g| g.text)
            .collect();
        assert!(
            !painted.iter().any(|t| t.contains(repo)),
            "the `{branch}` row must not repeat its repo name; painted {painted:?}"
        );
    }
}

#[test]
fn repo_filter_narrows_the_list_and_the_header_says_so() {
    let (_project, _dir, mut harness) = open_branches_over(two_repo_project(), two_repo_harness);

    // Unfiltered: the header names the whole scope.
    assert_painted(&harness, "all 2 repos");

    // The scope chip is a **control**, so it is the mouse path to the filter:
    // press it, and the picker it owns chooses the repository. The chip's own
    // accessible label is its state ("all 2 repos"), never a bare "Scope" —
    // ADR-0027's answer to the accessibility objection the reversal overrode.
    let chip = harness.get_by_role_and_label(egui::accesskit::Role::Button, "all 2 repos");
    assert!(
        chip.rect().height() >= 18.0,
        "the scope indicator is a chip, not a bare label: {chip:?}"
    );
    assert!(
        test_support::harness::filled_rects(&harness)
            .into_iter()
            .any(|(r, f)| f == turbogit_ui::theme::Palette::RAISED && r.intersects(chip.rect())),
        "and it paints the count chip's raised fill, so the one blue object in \
         the toolbar is still New Branch"
    );
    chip.click();
    settle_quiet(&mut harness);
    harness.get_by_label("Show beta").click();
    settle_quiet(&mut harness);

    // The chip announces the narrowing, alpha's rows vanish, beta's stay.
    assert_eq!(
        harness
            .state()
            .ui
            .branches_repo_filter
            .as_ref()
            .map(|r| r.name()),
        Some("beta".to_string()),
        "the chip's click is what changed the list's repository filter"
    );
    assert_painted(&harness, "filtered to beta");
    assert_not_painted(&harness, "feature-a");
    assert_painted(&harness, "clever");
    assert_section(&harness, "LOCAL", 2);
}

#[test]
fn fetch_across_all_repos_reports_each_repo_separately() {
    let (_project, dir, mut harness) = open_branches_over(two_repo_project(), two_repo_harness);

    // A new branch appears only on alpha's remote.
    git(&dir.join("origin.git"), &["branch", "fresh-remote"]);
    click_repo_fetches(&mut harness);
    pump_until(&mut harness, "both repos reported their fetch", |s| {
        s.multi
            .roots
            .iter()
            .find(|r| r.id.name() == "alpha")
            .is_some_and(|r| {
                r.branches.iter().any(|b| {
                    b.kind == turbogit_domain::model::BranchKind::Remote && b.name == "fresh-remote"
                })
            })
            && s.ui
                .activity
                .entries
                .iter()
                .filter(|e| e.message.starts_with("Fetch"))
                .count()
                >= 2
    });

    // Each repo reported its own outcome — never one flat "Fetch · done".
    let entries: Vec<_> = harness
        .state()
        .ui
        .activity
        .entries
        .iter()
        .filter(|e| e.message.starts_with("Fetch"))
        .map(|e| (e.repo.clone(), e.message.clone()))
        .collect();
    assert!(
        entries
            .iter()
            .any(|(repo, msg)| repo.as_deref() == Some("alpha")
                && msg.contains("1 new remote branches")),
        "alpha reports its own new branch, got {entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|(repo, msg)| repo.as_deref() == Some("beta") && msg == "Fetch · nothing changed"),
        "beta reports nothing changed in its own entry, got {entries:?}"
    );
}

/// A header's Fetch is that repository's control. Clicking it must not fetch
/// every repo in the project.
#[test]
fn a_headers_fetch_fetches_only_its_own_repository() {
    let (_project, _dir, mut harness) = open_branches_over(two_repo_project(), two_repo_harness);

    let list_x = LIST_RIGHT;
    let mut fetches: Vec<_> = harness
        .get_all_by_label("Fetch")
        .filter(|n| n.rect().top() > 80.0 && n.rect().min.x < list_x)
        .collect();
    fetches.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    assert_eq!(fetches.len(), 2, "one Fetch per repo header");
    // The lower header is `beta`.
    fetches[1].click();
    harness.step();

    let recorded: Vec<String> = harness
        .state()
        .ui
        .branches_fetch_before
        .iter()
        .map(|(root, _)| root.name())
        .collect();
    assert_eq!(
        recorded,
        vec!["beta".to_string()],
        "one header Fetch touches one repository"
    );
}

/// The detail panel's actions are one ladder, not a stack of differently-wide
/// buttons — and the spec's order is part of that shape.
/// A quiet read reports on the list; it never moves it.
#[test]
fn a_quiet_read_never_shifts_the_list() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);
    let before = row_node(&harness, "feature-a").rect().top();

    harness.state_mut().ui.busy = true;
    settle_quiet(&mut harness);
    assert_painted(&harness, "working…");
    let during = row_node(&harness, "feature-a").rect().top();

    harness.state_mut().ui.busy = false;
    settle_quiet(&mut harness);
    let after = row_node(&harness, "feature-a").rect().top();

    assert_eq!(
        (before, during, after),
        (before, before, before),
        "the lane above the list is reserved, so the busy line shifts nothing"
    );
}

// --- Cycle 19: keyboard, feedback & DoD final pass (issue 15) --------------------

#[test]
fn keyboard_path_filter_arrow_enter_checks_out() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // The keyboard path (§15): focus lands in the filter box on open, typing
    // narrows, an arrow selects the surviving row, and Enter checks it out —
    // with the cursor still in the filter box.
    type_search(&mut harness, "zeb");
    harness.key_press(egui::Key::ArrowDown);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("zebra"),
        "the arrow selects the surviving row"
    );

    harness.key_press(egui::Key::Enter);
    pump_until(&mut harness, "keyboard Enter checks out zebra", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .is_some_and(|r| r.current_branch.as_deref() == Some("zebra"))
    });
}

#[test]
fn arrow_keys_move_the_selection_through_branches() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // Nothing selected: the first Down lands on the current branch (it sits
    // first in the list), then each Down moves one row further.
    harness.key_press(egui::Key::ArrowDown);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("main")
    );
    harness.key_press(egui::Key::ArrowDown);
    settle_quiet(&mut harness);
    let first = harness.state().ui.branches_tree.selected.clone().unwrap();
    harness.key_press(egui::Key::ArrowDown);
    settle_quiet(&mut harness);
    let second = harness.state().ui.branches_tree.selected.clone().unwrap();
    harness.key_press(egui::Key::ArrowUp);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some(first.as_str()),
        "Up returns to the previous row"
    );
    assert_ne!(first, second, "each Down moves to a different branch");
}

#[test]
fn delete_key_asks_for_the_selected_branch() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // Select a branch and press Delete: the ask appears with the human-terms
    // consequence — never a silent or instant deletion.
    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Delete);
    settle_quiet(&mut harness);
    assert!(harness.state().ui.confirm.is_some(), "Delete asks first");
    assert_painted(
        &harness,
        "everything on this branch already exists on main — safe to delete",
    );
}

#[test]
fn delete_key_never_targets_the_current_branch() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    row_node(&harness, "main").click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Delete);
    settle_quiet(&mut harness);
    assert!(
        harness.state().ui.confirm.is_none(),
        "the current branch is never deleted"
    );
}

/// Puts the app into one in-flight state.
type StartOperation = fn(&mut AppState);

/// Each in-flight operation: the flag that starts it, the muted marker it paints,
/// and the tool window the view hides behind it. The marker is the row's identity.
const MID_OPERATION: &[(&str, StartOperation, Tab)] = &[
    ("merging…", |s| s.ui.merge_in_progress = true, Tab::Commit),
    ("working…", |s| s.ui.busy = true, Tab::Log),
];

#[test]
fn a_mid_operation_marks_the_list_in_place_and_survives_tab_switches() {
    for &(marker, start, away) in MID_OPERATION {
        let (_project, _dir, mut harness) =
            open_branches_over(single_repo_project(), branches_harness);

        // A marker over the list, never silence.
        start(harness.state_mut());
        harness.step();
        settle_quiet(&mut harness);
        assert_painted(&harness, marker);

        // First-class: it survives switching away and back.
        harness.state_mut().ui.tab = away;
        settle_quiet(&mut harness);
        harness.state_mut().ui.tab = Tab::Branches;
        settle_quiet(&mut harness);
        assert_painted(&harness, marker);

        // And it goes when the operation ends, so it cannot go stale.
        harness.state_mut().ui.busy = false;
        harness.state_mut().ui.merge_in_progress = false;
        harness.step();
        settle_quiet(&mut harness);
        assert_not_painted(&harness, marker);
    }
}

// --- Cycle 20: remotes toggle, expanded groups & freshness (issue 03) --------------

/// Every galley painted inside `rect`.
///
/// The same string usually paints more than once — the current branch names
/// both the repo header's branch pill and the list row — so matching on text
/// alone would happily return the wrong occurrence. Scoping to a widget's own
/// rectangle ties the answer to the widget under test.
fn galleys_in(harness: &Harness<'_, AppState>, rect: egui::Rect) -> Vec<PaintedGalley> {
    painted_galleys(harness)
        .into_iter()
        .filter(|g| rect.contains(g.pos))
        .collect()
}

/// The ink a *row's own* name paints in.
fn row_name_color(harness: &Harness<'_, AppState>, name: &str) -> Option<egui::Color32> {
    let row = row_nodes(harness, name).into_iter().next()?.rect();
    galleys_in(harness, row)
        .into_iter()
        .find(|g| g.text == name)
        .map(|g| g.color)
}

#[test]
fn tree_remote_rollup_reveals_remote_groups_without_a_toolbar_toggle() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    assert!(!harness.state().ui.branches_tree.show_remotes);
    assert_painted(&harness, "1 remote · 2 branches");
    assert_not_painted(&harness, "Show remotes");
    assert_not_painted(&harness, "Hide remotes");

    // On: each repo expands to per-remote groups. The remote name becomes the
    // group header, and the leaf never re-prints the "origin/" the header
    // already states.
    show_remotes(&mut harness);
    let texts = painted_text(&harness);
    assert!(
        texts.iter().any(|t| t == "origin"),
        "the remote group header names its remote: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "remote-only"),
        "the remote leaf paints under its group: {texts:?}"
    );
    assert_not_painted(&harness, "origin/remote-only");
    // Revealing remotes records the reveal against that one repository — the
    // view-wide switch is untouched, and no toolbar toggle appears.
    assert_eq!(
        harness.state().ui.branches_tree.remotes_revealed.len(),
        1,
        "the reveal belongs to the repository whose rollup was clicked"
    );
    assert!(
        !harness.state().ui.branches_tree.show_remotes,
        "a repo reveal never turns the view-wide switch on"
    );
    assert_not_painted(&harness, "Show remotes");
    assert_not_painted(&harness, "Hide remotes");
}

#[test]
fn expanded_remote_group_header_shows_its_count_and_freshness_hint() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // Nothing has been fetched yet, so there is no freshness to report.
    show_remotes(&mut harness);
    assert_not_painted(&harness, "fetched ");

    // Fetch is one click away even with remotes hidden (issue 03): it moved up
    // to the repo level, and running it stamps the timestamp the hint reads.
    click_fetch(&mut harness);
    pump_until(&mut harness, "fetch stamped the last-fetch time", |s| {
        s.ui.branches_last_fetch.is_some()
    });
    assert_eq!(
        harness.state().ui.branches_tree.remotes_revealed.len(),
        1,
        "fetching never hides the repository's remotes again"
    );

    // Pin a known age so the rendered hint is deterministic.
    harness.state_mut().ui.branches_last_fetch =
        Some(chrono::Utc::now() - chrono::Duration::minutes(5));
    settle_quiet(&mut harness);
    assert_painted(&harness, "fetched 5m");

    // The same header carries this remote's branch count.
    let texts = painted_text(&harness);
    assert!(
        texts.iter().any(|t| t == "2"),
        "the expanded remote group counts its branches: {texts:?}"
    );
}

#[test]
fn fetch_sits_at_the_repo_level_while_remotes_are_hidden() {
    let (_project, _dir, harness) = open_branches_over(two_repo_project(), two_repo_harness);

    // Remotes are off, so no Remote group header exists to hang Fetch on — yet
    // every repo's section still offers it, one control per repo.
    assert!(!harness.state().ui.branches_tree.show_remotes);
    assert_not_painted(&harness, "remote-only");
    let list_x = LIST_RIGHT;
    let fetches: Vec<_> = harness
        .get_all_by_label("Fetch")
        .filter(|n| n.rect().top() > 80.0 && n.rect().min.x < list_x)
        .collect();
    assert_eq!(
        fetches.len(),
        2,
        "one repo-level Fetch paints per repo section"
    );
}

// --- Cycle 21: toolbar composition & visual pass (issue 04) -------------------------

#[test]
fn repo_section_headers_name_their_repo_inside_the_list() {
    use turbogit_ui::theme::Palette;

    let (_project, _dir, harness) = open_branches_over(two_repo_project(), two_repo_harness);

    // **What moved, and why.** This suite used to anchor the bands on a 4px
    // heading status dot on their left. Ticket 16 removed that dot: the band's
    // state reaches the screen as the *mark pair* — one leading dot beside the
    // state summary — so a band has one dot, at the mark radius, and it leads
    // the words rather than the name. The bands are now located by the
    // repository names they print, below the tab strip.
    let list_top = tool_window_top(&harness);
    let galleys = painted_galleys(&harness);
    let mut bands: Vec<f32> = ["alpha", "beta"]
        .iter()
        .map(|repo| {
            galleys
                .iter()
                .find(|g| g.text == *repo && g.pos.y > list_top)
                .unwrap_or_else(|| panic!("`{repo}` names the band inside the list"))
                .pos
                .y
        })
        .collect();
    bands.sort_by(f32::total_cmp);
    assert_eq!(bands.len(), 2, "one band per repo section");

    // One mark per band, in that repository's state colour, on the band's line.
    let list_x = LIST_RIGHT;
    let marks: Vec<_> = filled_circles(&harness)
        .into_iter()
        .filter(|(c, r, _)| c.y > list_top && c.x < list_x && (*r - 3.0).abs() < f32::EPSILON)
        .collect();
    assert!(
        marks
            .iter()
            .all(|(c, _, _)| bands.iter().any(|y| (c.y - y).abs() < 20.0)),
        "each mark leads a band's state summary, on that band's own line: \
         marks {marks:?} vs bands {bands:?}"
    );

    // The head ref rides the same band line, and it is a **ref chip**: a ref
    // name on the raised-on-card surface in the data face. It used to be a pill
    // filling the accent token with brand ink.
    let chip = galleys
        .iter()
        .find(|g| g.text == "main" && bands.iter().any(|y| (g.pos.y - y).abs() < 20.0))
        .expect("a head-ref chip paints on a band line");
    assert_eq!(
        chip.color,
        Palette::INK_2,
        "the band's head ref is a neutral ref chip, inked in the chip's own step"
    );
    assert_eq!(
        chip.family,
        egui::FontFamily::Monospace,
        "the chip shows a branch name, so it is set in the data face"
    );
}

#[test]
fn names_are_monospace_and_labels_and_counts_are_sans() {
    use egui::FontFamily;

    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);
    let galleys = painted_galleys(&harness);
    let family_of = |text: &str| {
        galleys
            .iter()
            .find(|g| g.text == text)
            .unwrap_or_else(|| panic!("`{text}` was never painted"))
            .family
            .clone()
    };

    // Data — branch names — is monospace, so `feature/` paths line up (spec §19).
    // Read each name out of its own row: the same string also labels the repo
    // header's branch pill, and that is chrome.
    for name in ["main", "feature-a", "zebra"] {
        let row = row_nodes(&harness, name)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no `{name}` row"))
            .rect();
        let galley = galleys_in(&harness, row)
            .into_iter()
            .find(|g| g.text == name)
            .unwrap_or_else(|| panic!("`{name}` never painted on its own row"));
        assert_eq!(
            galley.family,
            FontFamily::Monospace,
            "the `{name}` row name is data"
        );
    }
    // Chrome — group labels and their counts — is the UI sans.
    assert_eq!(family_of("LOCAL"), FontFamily::Proportional);
    assert_eq!(family_of("TAGS"), FontFamily::Proportional);
    assert_eq!(
        family_of("1 remote · 2 branches"),
        FontFamily::Proportional,
        "the rollup is a label+count phrase, not a name"
    );

    // With remotes on, a remote's *name* joins the data face (mono) while the
    // per-remote count it carries stays chrome (sans).
    show_remotes(&mut harness);
    let galleys = painted_galleys(&harness);
    let family_of = |text: &str| {
        galleys
            .iter()
            .find(|g| g.text == text)
            .unwrap_or_else(|| panic!("`{text}` was never painted with remotes on"))
            .family
            .clone()
    };
    assert_eq!(
        family_of("origin"),
        FontFamily::Monospace,
        "a remote's name is data"
    );
    // The remote *group*'s own count is a label beside the remote's name, so it
    // is chrome. It is located by that name rather than by its text: with
    // remotes open, a section header's count **chip** also prints "2", and that
    // one is a chip (the data face, by the shared vocabulary), so a first-match
    // read of the string answers about the wrong object.
    let origin = galleys
        .iter()
        .find(|g| g.text == "origin")
        .expect("the remote's name");
    let group_count = galleys
        .iter()
        .find(|g| {
            g.text == "2" && (g.pos.y - origin.pos.y).abs() < 12.0 && g.pos.x > origin.rect.right()
        })
        .unwrap_or_else(|| panic!("the remote group's own count paints beside its name"));
    assert_eq!(
        group_count.family,
        FontFamily::Proportional,
        "the expanded remote group's count is chrome"
    );
    // …and the section bands' counts are the shared count chip, which is
    // monospaced so a column of counts aligns digit-for-digit.
    let local = galleys.iter().find(|g| g.text == "LOCAL").expect("LOCAL");
    let section_count = galleys
        .iter()
        .find(|g| {
            g.text == "4" && (g.pos.y - local.pos.y).abs() < 12.0 && g.pos.x > local.rect.right()
        })
        .expect("the LOCAL band's count chip");
    assert_eq!(
        section_count.family,
        FontFamily::Monospace,
        "a count chip is monospaced, like every chip"
    );
}

#[test]
fn toolbar_composes_scope_and_new_branch_without_remotes_toggle() {
    let (_project, _dir, harness) = open_branches_over(two_repo_project(), two_repo_harness);

    // The toolbar keeps the scope chip before New Branch, with no remotes toggle.
    // The chip's label is its state, so the query names the state.
    let scope = harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "all 2 repos")
        .rect();
    let new_branch = harness.get_by_label("New Branch").rect();
    assert!(
        scope.max.x <= new_branch.min.x,
        "the scope chip precedes New Branch: {scope:?} vs {new_branch:?}"
    );
    assert_not_painted(&harness, "Show remotes");
    assert_not_painted(&harness, "Hide remotes");

    // Before any narrowing the chip names the whole scope.
    assert_painted(&harness, "all 2 repos");

    // New Branch carries the primary-action blue: its own fill is the accent
    // token, so the one action that creates something reads as the primary one.
    let brand = test_support::harness::filled_rects(&harness)
        .into_iter()
        .any(|(r, f)| f == turbogit_ui::theme::Palette::ACCENT && r.intersects(new_branch));
    assert!(brand, "New Branch paints on an accent-filled surface");
}

/// The search input shares the toolbar row with the action cluster and stays
/// inside the window. It used to be added *after* a `with_layout` cluster,
/// which leaves the parent's cursor at the band's right edge: the box painted
/// off-screen — invisible and unreachable, with the toolbar's whole left half
/// empty above the tree.
#[test]
fn toolbar_search_shares_the_row_and_stays_in_the_window() {
    let (_project, _dir, harness) = open_branches_over(two_repo_project(), two_repo_harness);

    let win = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1024.0, 768.0));
    let search = harness.get_by_label("Search branches").rect();
    let scope = harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "all 2 repos")
        .rect();
    assert!(
        win.contains_rect(search),
        "the search box is inside the window: {search:?}"
    );
    assert!(
        search.width() > 100.0,
        "the box takes the left space: {search:?}"
    );
    assert!(
        search.max.x <= scope.min.x,
        "the search box precedes the scope chip: {search:?} vs {scope:?}"
    );
    assert!(
        search.y_range().intersects(scope.y_range()),
        "both halves share one toolbar row: {search:?} vs {scope:?}"
    );
}

#[test]
fn pull_and_push_stay_one_click_away_on_the_branches_tab() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    // The redesign is a view-level change: the app-level sync actions are
    // unchanged and still one click away while Branches is the active tab.
    // The topbar's action cluster is gone, so that surface is the command
    // palette (`Ctrl+Shift+A`) — it must offer the same verbs whatever the
    // active tool window is. (The Branches list has its own repo-level
    // `Fetch`, so the rows are matched inside the palette window.)
    harness.state_mut().ui.command_palette = true;
    harness.state_mut().ui.command_query.clear();
    settle_quiet(&mut harness);

    let palette = harness
        .get_by_role_and_label(egui::accesskit::Role::Window, "Find Action")
        .rect();
    for action in ["Fetch", "Pull", "Push…"] {
        assert!(
            harness
                .get_all_by_label(action)
                .map(|n| n.rect())
                .any(|r| palette.contains(r.center())),
            "{action} must be listed inside the command palette; palette={palette:?}"
        );
    }
    // …and it really is the palette, not a band of buttons left behind.
    assert_eq!(
        harness.state().ui.tab,
        Tab::Branches,
        "opening the palette must not switch the active tool window"
    );
}

/// **The ladder is gone.** A branch name is a **ref chip** in the row's own ink,
/// whatever the branch's sync state, and the state is **coloured text beside
/// it**. This replaces a suite that asserted the opposite — diverged read red
/// *in the name*, unpulled read amber *in the name* — so identity and state were
/// the same kind of thing and the name changed colour with the state.
#[test]
fn a_branch_name_is_a_ref_chip_and_its_state_is_coloured_text_beside_it() {
    use turbogit_ui::theme::Palette;

    // The current branch and a plain local branch read at the same ink: the band,
    // the rail and the marker carry "where am I", not the name.
    let (_project, _dir, harness) = open_branches_over(single_repo_project(), branches_harness);
    assert_eq!(
        row_name_color(&harness, "main"),
        Some(Palette::T_PRIMARY),
        "the active branch keeps the row's own ink; the band, the rail and the \
         marker carry the fact"
    );
    assert_eq!(
        row_name_color(&harness, "zebra"),
        Some(Palette::T_PRIMARY),
        "a plain local branch reads at the same primary ink"
    );

    // Diverged (ahead *and* behind its upstream): the **name** is unmoved, and
    // the words beside it wear the diverged red.
    let (_project, _dir, harness) = open_branches_over(sync_repo_project(), branches_harness);
    assert_eq!(
        row_name_color(&harness, "feat"),
        Some(Palette::T_PRIMARY),
        "a diverged branch's *name* is not the state: the state is beside it"
    );
    // A diverged row carries one mark per *direction*, and each direction wears
    // its own state from the one map — ahead is unpushed, behind is unpulled.
    // That is `sync_badge`'s contract (one badge per direction) restated in ink.
    // Compared as a set: which direction is drawn first is a layout decision, and
    // the claim is that both inks are the map's and that neither is borrowed.
    let mut diverged = state_inks_on_row(&harness, "feat");
    diverged.sort_by_key(|c| (c.r(), c.g(), c.b()));
    let mut expected = vec![
        turbogit_ui::theme::RepoState::Unpulled.color(),
        turbogit_ui::theme::RepoState::Unpushed.color(),
    ];
    expected.sort_by_key(|c| (c.r(), c.g(), c.b()));
    assert_eq!(
        diverged, expected,
        "…and the words beside it wear the one repository-state map, one ink \
         per direction"
    );
    // A branch with nothing to report says nothing: silence is the in-sync
    // answer, and it is the band's summary that speaks for the repository.
    assert!(
        state_inks_on_row(&harness, "main").is_empty(),
        "a branch in sync paints no state words beside its name"
    );
}
/// The inks of the state words a row paints beside its name, left to right.
///
/// The mark pair paints its dot and its words in one colour, so every painted
/// word on the row that is *not* the name, the tracking ref or the current
/// marker is state. Scoped by the row's own rect, because "3 behind" paints on
/// every row that is three behind — and every ink is checked against the one
/// map, so a state that grew a colour of its own would fail here rather than in
/// the token suite.
fn state_inks_on_row(harness: &Harness<'_, AppState>, name: &str) -> Vec<egui::Color32> {
    let Some(row) = row_nodes(harness, name)
        .into_iter()
        .next()
        .map(|n| n.rect())
    else {
        return Vec::new();
    };
    let Some(name_galley) = galleys_in(harness, row)
        .into_iter()
        .find(|g| g.text == name)
    else {
        return Vec::new();
    };
    let mut words: Vec<PaintedGalley> = galleys_in(harness, row)
        .into_iter()
        .filter(|g| {
            g.text != name
                && g.text != "origin/main"
                && g.text != "Current"
                && g.pos.x > name_galley.rect.right()
        })
        .collect();
    words.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
    // Every colour the one state map can produce, asked of the map rather than
    // transcribed. A hand-written list of variants is **not** exhaustiveness
    // checked — an inferred-length array just grows, silently — which is how the
    // seventh `RepoState` (`Uninitialized`, work the user is owed, wearing the
    // muted ink) came to be missing from this list while the suite stayed green
    // and the check quietly stopped covering the new state. So the set is derived
    // from the enum's own declaration and the enumeration is asserted closed
    // against it; `every_repo_state_variant_is_enumerated_here` in
    // `tests/branch_component_kit.rs` is the same mechanism over the same enum,
    // and the two lists have to agree because both are read from `theme.rs`.
    let map: Vec<egui::Color32> = all_repo_states()
        .iter()
        .map(|state| state.color())
        .collect();
    for g in &words {
        assert!(
            map.contains(&g.color),
            "`{}` on the `{name}` row is painted in {:?}, which is not a colour \
             any repository state produces: state text comes from \
             `RepoState::color()` and nowhere else",
            g.text,
            g.color
        );
    }
    words.iter().map(|g| g.color).collect()
}

/// Every `RepoState` variant, read from `theme.rs`'s own declaration and compared
/// against the written-out list.
///
/// Rust has no way to say "this enum gained a variant" from inside a test that
/// has to compile — no reflection, no stable `variant_count` — so the check is the
/// only honest form available: a *removed* variant is a compile error (the list
/// names a path that no longer exists) and an *added* one is this assertion,
/// naming the variant that arrived. The alternative, a bare array literal, is
/// what silently skipped `Uninitialized`.
fn all_repo_states() -> Vec<turbogit_ui::theme::RepoState> {
    const LISTED: [turbogit_ui::theme::RepoState; 7] = [
        turbogit_ui::theme::RepoState::Clean,
        turbogit_ui::theme::RepoState::Dirty,
        turbogit_ui::theme::RepoState::Conflict,
        turbogit_ui::theme::RepoState::Diverged,
        turbogit_ui::theme::RepoState::Unpushed,
        turbogit_ui::theme::RepoState::Unpulled,
        turbogit_ui::theme::RepoState::Uninitialized,
    ];
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/theme.rs"),
    )
    .expect("read theme.rs");
    // Comments blanked, so a doc comment on a variant is not read as one.
    let mut declared: Vec<String> = Vec::new();
    let mut inside = false;
    for line in srcscan::code_with_literals(&src).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub enum RepoState") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if trimmed == "}" {
            break;
        }
        if !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && trimmed.ends_with(',')
            && trimmed
                .trim_end_matches(',')
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            declared.push(trimmed.trim_end_matches(',').to_owned());
        }
    }
    let listed: Vec<String> = LISTED.iter().map(|s| format!("{s:?}")).collect();
    assert_eq!(
        listed, declared,
        "the repository states this suite knows about are not the states the map \
         declares. A new `RepoState` has to be added here — or the check that \
         every painted state word wears a colour from the map quietly stops \
         covering it, which is exactly what happened when `Uninitialized` landed."
    );
    LISTED.to_vec()
}

#[test]
fn an_unpulled_branch_states_its_state_beside_the_name_not_on_it() {
    use turbogit_ui::theme::Palette;

    let (_project, _dir, harness) =
        open_branches_over_with(single_repo_project(), branches_harness, |harness| {
            let st = harness.state_mut();
            let id = st.selected_root.clone().expect("selected root");
            if let Some(r) = st.multi.roots.iter_mut().find(|r| r.id == id) {
                r.branches.push(turbogit_domain::model::Branch {
                    name: "unpulled".into(),
                    kind: turbogit_domain::model::BranchKind::Local,
                    tracking: turbogit_domain::model::Upstream::from_git_ref("origin/main"),
                    favorite: false,
                    protected: false,
                    exists: true,
                    ahead: 0,
                    behind: 3,
                    gone: false,
                    last_touched: None,
                    tip: None,
                    remote: None,
                });
            }
        });

    // Behind-only is "unpulled": the counter orange, in the words beside the
    // name. Red is reserved for a branch that has genuinely diverged (ahead
    // *and* behind), so the two are never conflated.
    assert_eq!(
        state_inks_on_row(&harness, "unpulled"),
        vec![Palette::COUNTER],
        "an unpulled branch states it in words, in the one map's colour — which \
         for `Unpulled` is the reserved counter orange"
    );
    assert_eq!(
        row_name_color(&harness, "unpulled"),
        Some(Palette::T_PRIMARY),
        "and the name itself is unmoved: identity and state are two objects"
    );
}

#[test]
fn repo_status_marks_paint_their_status_color() {
    use turbogit_ui::theme::Palette;

    let (_project, _dir, harness) =
        open_branches_over_with(two_repo_project(), two_repo_harness, |harness| {
            // beta's current branch is 3 commits behind its upstream → the repo reads
            // "unpulled", while alpha stays clean.
            {
                let st = harness.state_mut();
                let beta = st
                    .multi
                    .roots
                    .iter_mut()
                    .find(|r| r.id.name() == "beta")
                    .expect("beta root");
                let main = beta
                    .branches
                    .iter_mut()
                    .find(|b| {
                        b.kind == turbogit_domain::model::BranchKind::Local && b.name == "main"
                    })
                    .expect("beta main");
                main.tracking = turbogit_domain::model::Upstream::from_git_ref("origin/main");
                main.ahead = 0;
                main.behind = 3;
            }
        });

    // **What moved, and why.** The bands' 4px heading status dot is gone; the
    // band's state is the mark pair's leading dot beside its state summary, at
    // the mark radius. The mark is what carries the colour, so this is the same
    // claim at the mark's own geometry.
    let list_top = tool_window_top(&harness);
    let list_x = LIST_RIGHT;
    let band_names: Vec<f32> = ["alpha", "beta"]
        .iter()
        .map(|repo| {
            painted_galleys(&harness)
                .into_iter()
                .find(|g| g.text == *repo && g.pos.y > list_top)
                .unwrap_or_else(|| panic!("the `{repo}` band names its repository"))
                .pos
                .y
        })
        .collect();
    let marks: Vec<_> = filled_circles(&harness)
        .into_iter()
        .filter(|(c, r, _)| {
            c.y > list_top
                && c.x < list_x
                && (*r - turbogit_ui::ui::components::STATE_DOT_R).abs() < f32::EPSILON
                && band_names.iter().any(|y| (c.y - y).abs() < 20.0)
        })
        .collect();
    assert_eq!(
        marks.len(),
        2,
        "one state mark per repo band, on that band's own line: {marks:?}"
    );
    assert!(
        marks.iter().any(|(_, _, f)| *f == Palette::COUNTER),
        "the unpulled repo's mark is the counter orange: {marks:?}"
    );
    assert!(
        marks.iter().any(|(_, _, f)| *f == Palette::AHEAD),
        "the clean repo's mark is the ahead green: {marks:?}"
    );
}

// --- 16: the pane's own vocabulary at the screen seam ----------------------------

/// The scope chip's click is the thing that changes the list's repository
/// filter. Not a label that happens to sit beside a button, and not a chip that
/// is only a label: press the chip, choose a repository, and the list below
/// narrows to it — and comes back.
///
/// **The reversal this pins.** `branches::scope_label` used to paint bare
/// coloured text and sit beside a separate quiet "Scope…" button, with a comment
/// saying a chip would "newly register an accessibility node for a piece of
/// status text". ADR-0027 reversed that: the node is the node for a control with
/// a state and an action, so a screen-reader user gains the filter. The comment
/// is gone from the code and replaced by the argument, and this is the behaviour
/// half of the answer.
/// The pane's scope control: a `Button` whose accessible label is its **state**
/// ("all 2 repos" / "filtered to beta"), never a bare word.
fn scope_chip<'h>(harness: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("all ") || l.starts_with("filtered to "))
        })
        .unwrap_or_else(|| panic!("the scope chip is a Button whose label is its state"))
}

#[test]
fn the_scope_chip_is_the_control_that_narrows_the_list() {
    let (_project, _dir, mut harness) = open_branches_over(two_repo_project(), two_repo_harness);

    // Unnarrowed: the chip states the scope, and both repositories' rows paint.
    assert_eq!(
        scope_chip(&harness).accesskit_node().label(),
        Some("all 2 repos".to_string()),
        "the chip announces the state, not a bare word"
    );
    assert_painted(&harness, "feature-a");
    assert_painted(&harness, "clever");

    // Press the chip → the picker it owns → one repository. The filter is the
    // observable, not the geometry.
    scope_chip(&harness).click();
    settle_quiet(&mut harness);
    harness.get_by_label("Show beta").click();
    settle_quiet(&mut harness);
    assert_eq!(
        harness
            .state()
            .ui
            .branches_repo_filter
            .as_ref()
            .map(|r| r.name()),
        Some("beta".to_string()),
        "the chip's click is what changed the list's repository filter"
    );
    assert_not_painted(&harness, "feature-a");
    assert_painted(&harness, "clever");
    assert_eq!(
        scope_chip(&harness).accesskit_node().label(),
        Some("filtered to beta".to_string()),
        "and the chip says so: a node that announced a bare noun would tell a \
         screen-reader user a control exists, not what it is set to"
    );

    // And back again, from the same chip.
    scope_chip(&harness).click();
    settle_quiet(&mut harness);
    harness.get_by_label("All repos").click();
    settle_quiet(&mut harness);
    assert!(
        harness.state().ui.branches_repo_filter.is_none(),
        "the picker clears the filter"
    );
    assert_painted(&harness, "feature-a");
}

/// **No chip anywhere in the pane fills the brand token**, and no row band is a
/// solid brand fill either. The one blue object left in the branches screen is
/// the New Branch button, which is a *control* — a chip or a row band in the
/// brand token is the blue soup this ticket exists to remove.
///
/// Scoped by **shape**, not by a region: a chip is the shared chip height, a row
/// band is the shared row height, and the shell's own brand chrome (the active
/// tab's underline) and the toolbar's primary button are neither. The claim is
/// about chips and row bands, and the constants that define those two shapes are
/// the filter — so a chip that grew to some third height would still be caught by
/// the chip-height assertion's sibling below, and a reader can see which object
/// each half is about.
#[test]
fn no_chip_in_the_branches_pane_fills_the_brand_token() {
    let (_project, _dir, harness) = open_branches_over(two_repo_project(), two_repo_harness);
    // Multi-repo, so the scope chip is painted, and a current row is painted, so
    // every chip the pane can produce is on screen at once.
    assert_painted(&harness, "all 2 repos");

    let brand = turbogit_ui::theme::Palette::BRAND;
    let brand_rects: Vec<egui::Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == brand)
        .map(|(rect, _)| rect)
        .collect();

    // (1) No chip. The current chip is the vocabulary's one chip allowed near
    // the accent, and it takes the *selected-row* fill, not a solid brand — so
    // even it is absent here, and the assertion is about the whole chip height.
    let brand_chips: Vec<egui::Rect> = brand_rects
        .iter()
        .copied()
        .filter(|rect| (rect.height() - turbogit_ui::ui::widgets::CHIP_HEIGHT).abs() < 0.01)
        .collect();
    assert!(
        brand_chips.is_empty(),
        "no chip in the pane fills the brand token: {brand_chips:?}"
    );

    // (2) No solid brand row band. Inside the list the brand is only ever the 2px
    // rail at a marked row's leading edge — the current row, and any selected
    // row — so a row band of brand width would be the old selection fill back.
    let row_bands: Vec<egui::Rect> = brand_rects
        .iter()
        .copied()
        .filter(|rect| (rect.height() - turbogit_ui::ui::components::BRANCH_ROW_H).abs() < 0.5)
        .collect();
    let rails: Vec<&egui::Rect> = row_bands
        .iter()
        .filter(|rect| (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01)
        .collect();
    assert_eq!(
        rails.len(),
        row_bands.len(),
        "every brand-filled row band is a 2px rail at the row's leading edge, \
         never a solid brand band: {row_bands:?}"
    );
    assert!(
        !rails.is_empty(),
        "…and there is at least one: the pane marks its current row with a rail, \
         so this half reads a live value rather than an absent one"
    );
}

/// **A repository band's Fetch fetches that repository only.**
///
/// `dispatch` records a pre-fetch remote snapshot for exactly the roots the
/// operation carries, so `ui.branches_fetch_before` is the operation's own
/// root list, read back. Clicking the *second* band's Fetch must leave the first
/// repository out of it — which is the whole reason the control hangs on the
/// band and not on the collapsed remote rollup, which has no group header of its
/// own to carry a repository-scoped action.
#[test]
fn a_repository_bands_fetch_fetches_that_repository_only() {
    let (_project, _dir, mut harness) = open_branches_over(two_repo_project(), two_repo_harness);
    harness.state_mut().ui.branches_fetch_before.clear();

    // Both bands paint a Fetch; `row_nodes`-style ordering puts beta's second.
    let fetches: Vec<egui::Rect> = harness
        .get_all_by_label("Fetch")
        .map(|n| n.rect())
        .collect();
    assert_eq!(fetches.len(), 2, "one Fetch per repository band");
    let beta = fetches
        .iter()
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .copied()
        .expect("beta's band");
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.rect().top() == beta.top())
        .expect("beta's Fetch button")
        .click();
    pump_until(&mut harness, "beta's band reported its fetch", |s| {
        !s.ui.branches_fetch_before.is_empty()
    });

    let roots: Vec<String> = harness
        .state()
        .ui
        .branches_fetch_before
        .iter()
        .map(|(id, _)| id.name())
        .collect();
    assert_eq!(
        roots,
        vec!["beta".to_string()],
        "one root, and it is the band that was pressed: the action is \
         per-repository, so fetching one slow remote does not fetch all of them"
    );
}

#[test]
fn screen_legible_at_150_percent_scaling() {
    let (_project, dir) = single_repo_project();
    let state = AppState::new(dir);
    let mut fonts_installed = false;
    let mut harness = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, state| {
            state.drain_events();
            ui.ctx().set_zoom_factor(1.5);
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1600.0, 1000.0));
    open_branches_tab(&mut harness);

    // Essentials stay painted at 150% — no clipped-away chrome.
    for s in [
        "LOCAL",
        "New Branch",
        "Search branches",
        "feature-a",
        "zebra",
    ] {
        assert_painted(&harness, s);
    }
    // Every branch row fully inside the window: nothing clips off-screen.
    let win = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1600.0, 1000.0));
    let rows = row_nodes(&harness, "feature-a");
    assert_eq!(rows.len(), 1);
    for node in row_nodes(&harness, "main") {
        assert!(
            win.contains_rect(node.rect()),
            "main row {node:?} clips at 150%"
        );
    }
    assert!(win.contains_rect(rows[0].rect()));
}
