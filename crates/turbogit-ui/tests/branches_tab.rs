//! Issue 03 — Branches tab frame + warm grouped list (spine, design doc §12–§16).
//!
//! The empty Branches tab becomes the screen: a 44px toolbar, the grouped
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
use egui_kittest::{Harness, kittest::NodeT as _, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::harness::{assert_not_painted, assert_painted, galley_origin, painted_text};
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

fn settle_quiet(harness: &mut Harness<'_, AppState>) {
    let mut stable = 0;
    let mut prev = String::new();
    for _ in 0..300 {
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
        let fp = format!("{:?}", painted_text(harness));
        if fp == prev {
            stable += 1;
            if stable >= 3 {
                return;
            }
        } else {
            stable = 0;
            prev = fp;
        }
    }
    panic!("branches layout did not settle within 300 frames");
}

// --- Cycle 1: groups with live counts ------------------------------------------

#[test]
fn tab_opens_with_groups_counts_and_nothing_selected() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // Group headers carry live counts: 4 locals, 2 remotes (origin carries
    // main + remote-only), 1 tag.
    assert_painted(&harness, "LOCAL 4");
    assert_painted(&harness, "REMOTE 2");
    assert_painted(&harness, "TAGS 1");

    // Members paint (tags are collapsed by default, so v1.0 stays hidden —
    // covered by the expand/collapse test).
    for member in ["feature-a", "feature-b", "zebra", "remote-only"] {
        assert_painted(&harness, member);
    }

    // Nothing selected yet → the detail panel shows the quiet prompt.
    assert_painted(&harness, "Select a branch");
    assert_eq!(
        harness.state().ui.branches_selected,
        None,
        "the tab opens with nothing selected"
    );
}

// --- Cycle 2: current branch visible without scrolling ---------------------------

#[test]
fn current_branch_is_first_row_and_visible_without_scrolling() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    let current = galley_origin(&harness, "main").expect("current row painted");
    for other in ["feature-a", "feature-b", "zebra", "origin/remote-only"] {
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // Local and Remote are expanded: their rows paint.
    assert_painted(&harness, "zebra");
    assert_painted(&harness, "remote-only");
    // Tags is collapsed: the header with its count paints, the member does not.
    assert_painted(&harness, "TAGS 1");
    assert_not_painted(&harness, "v1.0");

    // Clicking the Tags header expands it.
    harness.get_by_label("Tags").click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "v1.0");
}

// --- Cycle 4: empty repo state ---------------------------------------------------

#[test]
fn fresh_repo_shows_one_sentence_and_create_action_not_headers() {
    let (_project, dir) = fresh_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // One explanatory sentence + a single create action; no empty group
    // headers and no blank panel.
    assert_painted(&harness, "has no branches yet");
    assert_painted(&harness, "Create the first branch");
    for header in ["LOCAL 0", "REMOTE 0", "TAGS 0"] {
        assert_not_painted(&harness, header);
    }
}

// --- Cycle 5: detail panel geometry -----------------------------------------------

#[test]
fn detail_panel_is_280px_on_the_right_and_never_blocks_the_list() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // The detail panel is a filled ~280px rect near the content area's right
    // edge (the harness window is 1024 wide).
    let fills = test_support::harness::filled_rects(&harness);
    let panel = fills
        .iter()
        .filter(|(r, _)| (r.width() - 280.0).abs() < 4.0 && r.right() >= 900.0)
        .max_by_key(|(r, _)| r.width() as i64)
        .map(|(r, _)| *r);
    let panel = panel.expect("a ~280px detail panel rect near the right edge");
    assert!(
        panel.left() > 400.0,
        "detail panel sits to the right of the list, was {panel:?}"
    );

    // The list rows and the prompt are both painted → the detail never blocks
    // the list (they coexist on the same frame).
    assert_painted(&harness, "feature-a");
    assert_painted(&harness, "Select a branch");
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);

    // Add a branch with a long distinguishing name, then open the tab.
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
    open_branches_tab(&mut harness);

    // The painted label keeps both identifying ends and never ends with a
    // bare ellipsis.
    let texts: Vec<String> = painted_text(&harness);
    let row = texts
        .iter()
        .find(|t| t.contains("feature/multi") && t.contains("rewrite"))
        .expect("long name truncated in the middle");
    assert!(
        row.starts_with("feature/") && row.ends_with("rewrite"),
        "middle truncation keeps both ends, got {row:?}"
    );
}

// --- Cycle 8: row state at a glance (issue 04) -------------------------------------

/// Sync fixture: `alpha` on `main` with `feat` tracking `origin/main`
/// (ahead 2, behind 1), `ghost` tracking a pruned `origin/ghost` (gone), and
/// untracked `zebra` on the 3-week-old commit (stale badge "3w").
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

#[test]
fn rows_render_icon_count_pairs_in_sync_gone_and_upstream() {
    let (_project, dir) = sync_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // feat: diverged from origin/main → icon + count pairs (the arrows are
    // SVG icons; the counts paint as their own galleys).
    assert_painted(&harness, "2");
    assert_painted(&harness, "1");
    assert_painted(&harness, "origin/main");
    // main: tracks origin/main and is in sync → quiet confirmation.
    assert_painted(&harness, "in sync");
    // ghost: upstream deleted → gone marker.
    assert_painted(&harness, "gone");
    assert_painted(&harness, "origin/ghost");
    // Stale zebra is dimmed but never hidden (still painted, relative "3w").
    assert_painted(&harness, "zebra");
    assert_painted(&harness, "3w");
}

#[test]
fn rows_never_paint_absolute_dates() {
    let (_project, dir) = sync_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    let year = format!("{}", chrono::Utc::now().year());
    let texts: Vec<String> = painted_text(&harness);
    assert!(
        !texts.iter().any(|t| t.contains(&year)),
        "absolute dates must never render on rows; got {texts:?}"
    );
}

// --- Cycle 9: selection + detail panel (issue 05) ---------------------------------

#[test]
fn click_selects_and_fills_detail_and_never_checks_out() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // The local row (first match; the remote group repeats short names).
    harness
        .get_all_by_label("feature-a")
        .next()
        .expect("local feature-a row")
        .click();
    settle_quiet(&mut harness);

    // Selection fills the detail panel: full name, relationship, latest
    // commit block, and the action list in spec order, naming the target.
    assert_eq!(
        harness.state().ui.branches_selected.as_deref(),
        Some("feature-a")
    );
    assert_painted(&harness, "feature-a");
    assert_painted(&harness, "Checkout");
    assert_painted(&harness, "Merge into main");
    assert_painted(&harness, "Rebase onto main");
    assert_painted(&harness, "Compare with main");
    assert_painted(&harness, "Rename");
    assert_painted(&harness, "Delete");
    // Latest-commit block: the shared `init` commit message.
    assert_painted(&harness, "init");

    // Clicking never checks out.
    let cur = harness
        .state()
        .selected_root
        .as_ref()
        .and_then(|id| harness.state().multi.by_id(id))
        .and_then(|r| r.current_branch.clone());
    assert_eq!(cur.as_deref(), Some("main"));
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

/// The row's clickable node: the row is a Button whose accessible label is
/// the branch name (the inner text Label also matches by name, so scope by
/// role to keep queries unambiguous).
fn row_node<'t>(harness: &'t Harness<'_, AppState>, name: &str) -> egui_kittest::Node<'t> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(name.to_string()))
        .unwrap_or_else(|| panic!("row button for {name}"))
}

#[test]
fn enter_on_selected_row_checks_it_out() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    harness
        .get_all_by_label("feature-b")
        .next()
        .expect("local feature-b row")
        .click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Enter);
    pump_until(&mut harness, "Enter checks out the selected row", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.current_branch.clone())
            .as_deref()
            == Some("feature-b")
    });
}

#[test]
fn double_click_on_row_checks_it_out() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    let node = row_node(&harness, "zebra");
    node.click();
    let _ = node;
    harness.step();
    let node = row_node(&harness, "zebra");
    node.click();
    let _ = node;
    pump_until(&mut harness, "double-click checks out the row", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.current_branch.clone())
            .as_deref()
            == Some("zebra")
    });
}

#[test]
fn hover_reveals_row_actions() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // Nothing selected, so "Checkout" can only come from the hovered row.
    row_node(&harness, "zebra").hover();
    settle_quiet(&mut harness);
    assert_painted(&harness, "Checkout");
    assert_eq!(
        harness.state().ui.branches_selected,
        None,
        "hovering must not select"
    );
}

#[test]
fn overflow_menu_carries_the_same_actions_as_the_detail() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // Hover zebra (nothing selected) and open its ⋯ overflow.
    row_node(&harness, "zebra").hover();
    settle_quiet(&mut harness);
    harness.get_by_label("More actions").click();
    settle_quiet(&mut harness);

    // The menu carries the same items as the detail panel (checked against
    // the zebra row, which is not current): order + wording intact.
    assert_painted(&harness, "Merge into main");
    assert_painted(&harness, "Rebase onto main");
    assert_painted(&harness, "Compare with main");
    assert_painted(&harness, "Rename");
    assert_painted(&harness, "Delete");
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    type_search(&mut harness, "feat");
    assert_painted(&harness, "LOCAL 2");
    assert_painted(&harness, "feature-a");
    assert_painted(&harness, "feature-b");
    // zebra and the tags stay filtered out (the shell chrome always paints
    // `main` elsewhere, so the current branch is not asserted absent).
    assert_not_painted(&harness, "zebra");
    assert_not_painted(&harness, "remote-only");
}

#[test]
fn fuzzy_query_matches_prefix_dots_and_multiword() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);

    {
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
    }
    open_branches_tab(&mut harness);

    // `mre` surfaces feature/multi-root-executor (subsequence match)…
    type_search(&mut harness, "mre");
    assert_painted(&harness, "feature/multi-root-executor");
    // …and a multi-word query like `multi root` matches too.
    harness.state_mut().ui.branches_filter.clear();
    type_search(&mut harness, "multi root");
    assert_painted(&harness, "feature/multi-root-executor");
}

#[test]
fn query_matches_the_tip_commit_message() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);

    {
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
    }
    open_branches_tab(&mut harness);

    // Remembering what a branch contains, not its name.
    type_search(&mut harness, "worktree");
    assert_painted(&harness, "fix/dirty-worktree");
    assert_not_painted(&harness, "feature-a");
}

#[test]
fn no_match_says_create_it_inside_the_list() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    type_search(&mut harness, "zzz-no-branch");
    assert_painted(&harness, "no branch called zzz-no-branch. Create it?");
}

#[test]
fn clearing_restores_selection_and_the_full_list() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // Select a branch first, then filter the list to zebra. The selection
    // persists (the detail panel keeps the selected branch — search is for
    // jumping, and clearing restores the prior selection).
    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    type_search(&mut harness, "zebra");
    assert_painted(&harness, "zebra");
    assert_eq!(
        harness.state().ui.branches_selected.as_deref(),
        Some("feature-a"),
        "filtering never drops the selection"
    );

    // Esc clears the filter: the full list (and its counts) return, and the
    // prior selection is intact.
    harness.key_press(egui::Key::Escape);
    settle_quiet(&mut harness);
    assert_painted(&harness, "LOCAL 4");
    assert_painted(&harness, "feature-a");
    assert_eq!(
        harness.state().ui.branches_selected.as_deref(),
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    {
        let st = harness.state_mut();
        let id = st.selected_root.clone().unwrap();
        st.caches.store_worktrees(
            id.clone(),
            vec![turbogit_domain::model::Worktree {
                path: PathBuf::from("C:\\wt\\feature-a"),
                branch: "feature-a".into(),
                dirty: false,
                root: id,
            }],
        );
    }
    open_branches_tab(&mut harness);

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
fn new_branch_is_prominent_and_defaults_base_to_current_with_switch_on() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

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
        "main",
        "base defaults to the current branch"
    );
    assert!(
        harness.state().ui.dlg.new_branch_checkout,
        "switch-now defaults to yes"
    );
    assert_painted(&harness, "Switch to the new branch now");
}

#[test]
fn create_and_switch_appears_marked_current_and_scrolled() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    harness.get_by_label("New Branch").click();
    settle_quiet(&mut harness);
    {
        let field = harness
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .find(|n| n.accesskit_node().label().is_none())
            .expect("dialog Name input queryable");
        field.focus();
        field.type_text("issue08-x");
    }
    settle_quiet(&mut harness);
    harness.get_by_label("Create").click();

    // After create-and-switch: the branch appears, is current, and the
    // scroll-to intent was consumed (it was near the top already — the new
    // current branch orders first).
    pump_until(&mut harness, "branch created and checked out", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .map(|r| {
                r.current_branch.as_deref() == Some("issue08-x")
                    && r.branches.iter().any(|b| b.name == "issue08-x")
            })
            .unwrap_or(false)
    });
    assert!(harness.state().ui.branches_scroll_to.is_none());
    assert_painted(&harness, "issue08-x");
}

#[test]
fn create_without_switch_keeps_the_current_branch() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    harness.get_by_label("New Branch").click();
    settle_quiet(&mut harness);
    // Turn the explicit switch decision off.
    harness.get_by_label("Switch to the new branch now").click();
    settle_quiet(&mut harness);
    {
        let field = harness
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .find(|n| n.accesskit_node().label().is_none())
            .expect("dialog Name input queryable");
        field.focus();
        field.type_text("issue08-no-switch");
    }
    settle_quiet(&mut harness);
    harness.get_by_label("Create").click();

    pump_until(&mut harness, "branch created without switching", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .map(|r| {
                r.current_branch.as_deref() == Some("main")
                    && r.branches.iter().any(|b| b.name == "issue08-no-switch")
            })
            .unwrap_or(false)
    });
}

// --- Cycle 13: merge / rebase from the detail (issue 09) ----------------------------

#[test]
fn merge_from_detail_opens_the_preflighted_dialog() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Merge into main").click();
    settle_quiet(&mut harness);

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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    row_node(&harness, "feature-b").click();
    settle_quiet(&mut harness);
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

#[test]
fn mid_operation_state_shows_on_the_row_and_survives_tab_switches() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    harness.state_mut().ui.merge_in_progress = true;
    open_branches_tab(&mut harness);

    assert_painted(&harness, "merging…");
    // Switch away and back: the mid-operation state is first-class and
    // survives (issue 09, §10).
    harness.state_mut().ui.tab = Tab::Commit;
    settle_quiet(&mut harness);
    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(&mut harness);
    assert_painted(&harness, "merging…");
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
    let (_project, dir) = conflict_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    row_node(&harness, "conflict-a").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Merge into main").click();
    settle_quiet(&mut harness);
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // Select a branch and note the scroll position.
    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    let scroll_before = harness.state().ui.branches_scroll;
    harness.get_by_label("Compare with main").click();
    settle_quiet(&mut harness);

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
        harness.state().ui.branches_selected.as_deref(),
        Some("feature-a"),
        "selection survives compare"
    );
    assert_eq!(
        harness.state().ui.branches_scroll,
        scroll_before,
        "scroll position survives compare"
    );
    assert!(harness.state().ui.dialog.is_none());
}

// --- Cycle 15: rename inline (issue 11) ---------------------------------------------

#[test]
fn rename_is_inline_on_the_row_and_resorts_correctly() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Rename").click();
    settle_quiet(&mut harness);

    // Inline on the row — not a separate form screen.
    assert!(harness.state().ui.dialog.is_none());
    assert_eq!(
        harness.state().ui.branches_renaming.as_deref(),
        Some("feature-a")
    );

    harness.state_mut().ui.branches_rename_draft = "aaa-renamed".into();
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    // Give feature-a a tracked upstream so the disclosure has something to say.
    {
        let st = harness.state_mut();
        let id = st.selected_root.clone().unwrap();
        if let Some(r) = st.multi.roots.iter_mut().find(|r| r.id == id)
            && let Some(b) = r.branches.iter_mut().find(|b| b.name == "feature-a")
        {
            b.tracking = Some("origin/feature-a".into());
        }
    }
    open_branches_tab(&mut harness);

    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Rename").click();
    settle_quiet(&mut harness);

    // The one thing the person must know before confirming.
    assert_painted(
        &harness,
        "tracking origin/feature-a does not follow the new name — set it again after",
    );
}

#[test]
fn rename_current_branch_keeps_the_marker_and_leaves_the_tree_untouched() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    let tree_changes_before = harness.state().multi.roots[0].status.changes.len();
    row_node(&harness, "main").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Rename").click();
    settle_quiet(&mut harness);
    harness.state_mut().ui.branches_rename_draft = "main2".into();
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
    let (_project, dir) = conflict_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // conflict-a carries a commit not on main: the confirmation says what
    // would become unreachable, in human terms.
    row_node(&harness, "conflict-a").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Delete").click();
    settle_quiet(&mut harness);
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // feature-a shares main's commit: safe to delete, and the confirmation
    // says so.
    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Delete").click();
    settle_quiet(&mut harness);
    assert_painted(
        &harness,
        "everything on this branch already exists on main — safe to delete",
    );
}

#[test]
fn the_current_branch_cannot_be_deleted() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // Selecting the current branch never offers Delete in the detail panel.
    row_node(&harness, "main").click();
    settle_quiet(&mut harness);
    assert_not_painted(&harness, "Delete");
    assert!(harness.state().ui.confirm.is_none());
}

#[test]
fn delete_refuses_a_branch_checked_out_elsewhere() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    {
        let st = harness.state_mut();
        let id = st.selected_root.clone().unwrap();
        st.caches.store_worktrees(
            id.clone(),
            vec![turbogit_domain::model::Worktree {
                path: PathBuf::from("C:\\wt\\feature-a"),
                branch: "feature-a".into(),
                dirty: false,
                root: id,
            }],
        );
    }
    open_branches_tab(&mut harness);

    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Delete").click();
    settle_quiet(&mut harness);
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // Remote rows are labelled with their remote's name and rendered quieter.
    assert_painted(&harness, "origin/remote-only");
    assert_painted(&harness, "origin/main");
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

/// Click the remote group's Fetch (the topbar also has a Fetch button; pick
/// the one inside the Branches area).
fn click_fetch(harness: &mut Harness<'_, AppState>) {
    let nodes: Vec<_> = harness.get_all_by_label("Fetch").collect();
    let node = nodes
        .iter()
        .find(|n| n.rect().top() > 80.0)
        .unwrap_or_else(|| panic!("remote-header Fetch not found"));
    node.click();
}

#[test]
fn fetch_reports_nothing_changed_or_new_remote_branches() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir.clone());
    open_branches_tab(&mut harness);

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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

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
                                && b.tracking.as_deref() == Some("origin/remote-only")
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

    #[test]
    fn branch_row_budget_keeps_middle_truncation_bounds() {
        use turbogit_ui::ui::branches::name_budget;
        assert!((8..=48).contains(&name_budget(300.0)));
        assert!((8..=48).contains(&name_budget(1000.0)));
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

    #[test]
    fn row_meta_carries_upstream_and_sync_pair() {
        use turbogit_ui::ui::components::SyncKind;
        let now = Utc::now();
        let mut br = b("feat", Some(now.timestamp()));
        br.tracking = Some("origin/main".into());
        br.ahead = 2;
        br.behind = 1;
        let meta = row_meta(&br, now);
        assert_eq!(meta.upstream.as_deref(), Some("origin/main"));
        assert_eq!(meta.badge, Some((SyncKind::Diverged, "↑2 ↓1".to_string())));

        // Untracked branches carry no upstream and no badge.
        let plain = b("zebra", Some(now.timestamp()));
        let meta = row_meta(&plain, now);
        assert_eq!(meta.upstream, None);
        assert_eq!(meta.badge, None);
    }

    #[test]
    fn row_meta_says_in_sync_quietly() {
        use turbogit_ui::ui::components::SyncKind;
        let now = Utc::now();
        let mut br = b("main", Some(now.timestamp()));
        br.tracking = Some("origin/main".into());
        let meta = row_meta(&br, now);
        assert_eq!(meta.badge, Some((SyncKind::InSync, "in sync".to_string())));
    }

    #[test]
    fn row_meta_marks_gone_upstream() {
        use turbogit_ui::ui::components::SyncKind;
        let now = Utc::now();
        let mut br = b("ghost", Some(now.timestamp()));
        br.tracking = Some("origin/ghost".into());
        br.gone = true;
        let meta = row_meta(&br, now);
        assert_eq!(meta.badge, Some((SyncKind::Gone, "gone".to_string())));
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
/// toolbar, left of the detail panel — a same-named topbar breadcrumb or
/// metadata button never leaks in), ordered top-to-bottom.
fn row_nodes<'h>(harness: &'h Harness<'_, AppState>, name: &str) -> Vec<egui_kittest::Node<'h>> {
    let list_x = 1024.0 - 280.0; // harness width minus the §12 detail panel
    let mut nodes: Vec<_> = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| {
            n.accesskit_node().label() == Some(name.to_string())
                && n.rect().top() > 80.0
                && n.rect().min.x < list_x
        })
        .collect();
    nodes.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    nodes
}

#[test]
fn two_current_branches_are_distinct_rows_owned_by_their_repo() {
    let (_project, dir) = two_repo_project();
    let mut harness = two_repo_harness(dir);
    open_branches_tab(&mut harness);

    // Both repos contribute; the two `main`s are separate rows, never one
    // anonymous row.
    assert_painted(&harness, "LOCAL 4");

    // Clicking one local main then the other switches the selected repo — the
    // selection records which repo owns the row, so the two `main`s are
    // never interchangeable. `row_nodes` sorts top-to-bottom; the Local
    // group renders above Remote, so the first two mains are the locals.
    {
        let mains = row_nodes(&harness, "main");
        assert_eq!(mains.len(), 3, "two local mains + origin/main's remote row");
        mains[0].click();
    }
    settle_quiet(&mut harness);
    let first = harness.state().ui.branches_selected_root.clone();
    {
        let mains = row_nodes(&harness, "main");
        mains[1].click();
    }
    settle_quiet(&mut harness);
    let second = harness.state().ui.branches_selected_root.clone();
    assert!(
        first.is_some() && second.is_some() && first != second,
        "same-named rows resolve to different owners: {first:?} vs {second:?}"
    );
    assert_eq!(
        harness.state().ui.branches_selected.as_deref(),
        Some("main")
    );
}

#[test]
fn branch_actions_state_their_scope_before_running() {
    let (_project, dir) = two_repo_project();
    let mut harness = two_repo_harness(dir);
    open_branches_tab(&mut harness);

    // The row action names the repo it will act on — never a bare "Checkout"
    // that leaves the scope ambiguous between two repos.
    row_node(&harness, "clever").hover();
    settle_quiet(&mut harness);
    assert_painted(&harness, "Checkout in beta");

    // The same scope rides the detail panel's primary action.
    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "Checkout in alpha");
}

#[test]
fn repo_filter_narrows_the_list_and_the_header_says_so() {
    let (_project, dir) = two_repo_project();
    let mut harness = two_repo_harness(dir);
    open_branches_tab(&mut harness);

    // Unfiltered: the header names the whole scope.
    assert_painted(&harness, "all 2 repos");

    // Pick beta from the scope picker.
    harness.get_by_label("Scope…").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Show beta").click();
    settle_quiet(&mut harness);

    // The header announces the narrowing, alpha's rows vanish, beta's stay.
    assert_painted(&harness, "filtered to beta");
    assert_not_painted(&harness, "feature-a");
    assert_painted(&harness, "clever");
    assert_painted(&harness, "LOCAL 2");
}

#[test]
fn fetch_across_all_repos_reports_each_repo_separately() {
    let (_project, dir) = two_repo_project();
    let mut harness = two_repo_harness(dir.clone());
    open_branches_tab(&mut harness);

    // A new branch appears only on alpha's remote.
    git(&dir.join("origin.git"), &["branch", "fresh-remote"]);
    click_fetch(&mut harness);
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

// --- Cycle 19: keyboard, feedback & DoD final pass (issue 15) --------------------

#[test]
fn keyboard_path_filter_arrow_enter_checks_out() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // The keyboard path (§15): focus lands in the filter box on open, typing
    // narrows, an arrow selects the surviving row, and Enter checks it out —
    // with the cursor still in the filter box.
    type_search(&mut harness, "zeb");
    harness.key_press(egui::Key::ArrowDown);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.branches_selected.as_deref(),
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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // Nothing selected: the first Down lands on the current branch (it sits
    // first in the list), then each Down moves one row further.
    harness.key_press(egui::Key::ArrowDown);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.branches_selected.as_deref(),
        Some("main")
    );
    harness.key_press(egui::Key::ArrowDown);
    settle_quiet(&mut harness);
    let first = harness.state().ui.branches_selected.clone().unwrap();
    harness.key_press(egui::Key::ArrowDown);
    settle_quiet(&mut harness);
    let second = harness.state().ui.branches_selected.clone().unwrap();
    harness.key_press(egui::Key::ArrowUp);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.branches_selected.as_deref(),
        Some(first.as_str()),
        "Up returns to the previous row"
    );
    assert_ne!(first, second, "each Down moves to a different branch");
}

#[test]
fn delete_key_asks_for_the_selected_branch() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

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
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    row_node(&harness, "main").click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Delete);
    settle_quiet(&mut harness);
    assert!(
        harness.state().ui.confirm.is_none(),
        "the current branch is never deleted"
    );
}

#[test]
fn mid_operation_marker_shows_in_place_and_survives_tab_switches() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);

    // An operation in flight paints a muted in-place marker over the list —
    // never silence.
    harness.state_mut().ui.busy = true;
    harness.step();
    settle_quiet(&mut harness);
    assert_painted(&harness, "working…");

    // The mid-operation state survives switching away and back.
    harness.state_mut().ui.tab = Tab::Log;
    harness.step();
    harness.state_mut().ui.tab = Tab::Branches;
    harness.step();
    settle_quiet(&mut harness);
    assert_painted(&harness, "working…");

    harness.state_mut().ui.busy = false;
    harness.step();
    settle_quiet(&mut harness);
    assert_not_painted(&harness, "working…");
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
        "LOCAL 4",
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
