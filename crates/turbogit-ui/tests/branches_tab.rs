use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::Datelike;
use egui_kittest::{Harness, Node, kittest::NodeT as _, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{
    PaintedGalley, assert_not_painted, assert_painted, filled_circles, filled_rects, galley_origin,
    painted_galleys, painted_text, settle_quiet,
};
use test_support::srcscan;
use turbogit_app::state::{AppState, Tab};

fn commit_readme(repo: &Path) {
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    git_with_env(repo, &["add", "."], &GitEnv::fixture());
    git_with_env(repo, &["commit", "-m", "init"], &GitEnv::fixture());
}

fn single_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    commit_readme(&alpha);
    git_with_env(
        &project,
        &["clone", "--bare", "alpha", "origin.git"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["remote", "add", "origin", "../origin.git"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["push", "-q", "origin", "main"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["push", "-q", "origin", "main:remote-only"],
        &GitEnv::fixture(),
    );
    for b in ["feature-a", "feature-b", "zebra"] {
        git_with_env(&alpha, &["branch", b], &GitEnv::fixture());
    }
    git_with_env(&alpha, &["tag", "v1.0"], &GitEnv::fixture());
    git_with_env(&alpha, &["fetch", "-q", "origin"], &GitEnv::fixture());
    (tmp, project)
}

fn fresh_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    (tmp, project)
}

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
    harness.set_size(egui::vec2(1008.0, 768.0));
    harness
}

fn open_branches_tab(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(harness);
}

fn open_branches_over(
    project: (TempDir, PathBuf),
    harness_of: fn(PathBuf) -> Harness<'static, AppState>,
) -> (TempDir, PathBuf, Harness<'static, AppState>) {
    open_branches_over_with(project, harness_of, |_| {})
}

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

fn show_remotes(harness: &mut Harness<'_, AppState>) {
    row_node(harness, "Remote").click();
    settle_quiet(harness);
}

#[test]
fn tab_opens_with_groups_counts_and_nothing_selected() {
    let (_project, _dir, harness) = open_branches_over(single_repo_project(), branches_harness);

    assert_section(&harness, "LOCAL", 4);
    assert_painted(&harness, "1 remote · 2 branches");
    assert_section(&harness, "TAGS", 1);

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

    assert_eq!(
        harness.state().ui.branches_tree.selected,
        None,
        "the tab opens with nothing selected"
    );
}

#[test]
fn current_branch_is_first_row_and_visible_without_scrolling() {
    let (_project, _dir, harness) = open_branches_over(single_repo_project(), branches_harness);

    let current = galley_origin(&harness, "main").expect("current row painted");
    for other in ["feature-a", "feature-b", "zebra"] {
        let o = galley_origin(&harness, other).expect("branch row painted");
        assert!(
            current.y < o.y,
            "current branch must be the topmost branch row ({current:?} vs {o:?})"
        );
    }
    assert!(
        current.y < 400.0,
        "current branch is visible without scrolling, was at y={}",
        current.y
    );
}

#[test]
fn tags_group_starts_collapsed_and_headers_keep_counts() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    assert_painted(&harness, "zebra");
    assert_painted(&harness, "1 remote · 2 branches");
    assert_not_painted(&harness, "remote-only");
    assert_section(&harness, "TAGS", 1);
    assert_not_painted(&harness, "v1.0");

    harness.get_by_label("Tags").click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "v1.0");
}

#[test]
fn fresh_repo_shows_one_sentence_and_create_action_not_headers() {
    let (_project, _dir, harness) = open_branches_over(fresh_repo_project(), branches_harness);

    assert_painted(&harness, "has no branches yet");
    assert_painted(&harness, "Create the first branch");
    for header in ["LOCAL", "REMOTE", "TAGS"] {
        assert_not_painted(&harness, header);
    }
}

#[test]
fn the_branch_list_fills_the_width_the_detail_panel_used_to_take() {
    let (_project, _dir, harness) = open_branches_over(single_repo_project(), branches_harness);

    let row = row_node(&harness, "feature-a");
    assert!(
        row.rect().right() > 1024.0 - 280.0 + 40.0,
        "a row spans the content width, was {:?}",
        row.rect()
    );
    assert_painted(&harness, "feature-a");
    assert_not_painted(&harness, "Select a branch");
}

#[test]
fn long_branch_names_truncate_in_the_middle_on_the_row() {
    let (_project, _dir, harness) =
        open_branches_over_with(single_repo_project(), branches_harness, |harness| {
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
        });

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

fn sync_repo_project() -> (TempDir, PathBuf) {
    let now = chrono::Utc::now().timestamp();
    let old = now - 21 * 24 * 60 * 60;

    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    git_with_env(&alpha, &["config", "user.email", "t@t"], &GitEnv::fixture());
    git_with_env(&alpha, &["config", "user.name", "t"], &GitEnv::fixture());
    commit_pinned(&alpha, "c1", old);

    git_with_env(
        &project,
        &["clone", "--bare", "alpha", "origin.git"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["remote", "add", "origin", "../origin.git"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["push", "-q", "origin", "main"],
        &GitEnv::fixture(),
    );

    git_with_env(&alpha, &["branch", "ghost"], &GitEnv::fixture());
    git_with_env(
        &alpha,
        &["push", "-q", "-u", "origin", "ghost"],
        &GitEnv::fixture(),
    );
    git_with_env(&alpha, &["fetch", "-q", "origin"], &GitEnv::fixture());
    git_with_env(&alpha, &["branch", "zebra"], &GitEnv::fixture());

    git_with_env(&alpha, &["branch", "feat"], &GitEnv::fixture());
    git_with_env(
        &alpha,
        &["branch", "--set-upstream-to=origin/main", "feat"],
        &GitEnv::fixture(),
    );
    git_with_env(&alpha, &["checkout", "-q", "feat"], &GitEnv::fixture());
    commit_pinned(&alpha, "c2", now);
    commit_pinned(&alpha, "c3", now);
    git_with_env(&alpha, &["checkout", "-q", "main"], &GitEnv::fixture());
    commit_pinned(&alpha, "c4", now);
    git_with_env(
        &alpha,
        &["push", "-q", "origin", "main"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["branch", "--set-upstream-to=origin/main", "main"],
        &GitEnv::fixture(),
    );

    git_with_env(
        &project.join("origin.git"),
        &["update-ref", "-d", "refs/heads/ghost"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["fetch", "-q", "--prune", "origin"],
        &GitEnv::fixture(),
    );
    (tmp, project)
}

fn commit_pinned(repo: &Path, msg: &str, epoch: i64) {
    std::fs::write(repo.join("f.txt"), format!("{msg}\n")).unwrap();
    git_with_env(repo, &["add", "."], &GitEnv::fixture());
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

    assert_painted(&harness, "↑2");
    assert_painted(&harness, "↓1");
    assert_painted(&harness, "origin/main");
    assert_not_painted(&harness, "in sync");
    assert_painted(&harness, "gone");
    assert_painted(&harness, "origin/ghost");
    assert_painted(&harness, "zebra");
}

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

#[test]
fn click_selects_the_row_visibly_and_never_checks_out() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    assert!(
        !filled_rects(&harness)
            .iter()
            .any(|(_, fill)| *fill == turbogit_ui::theme::Palette::ROW_SELECTED),
        "nothing is selected at rest"
    );

    harness
        .get_all_by_label("feature-a")
        .next()
        .expect("local feature-a row")
        .click();
    settle_quiet(&mut harness);

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

    let cur = harness
        .state()
        .selected_root
        .as_ref()
        .and_then(|id| harness.state().multi.by_id(id))
        .and_then(|r| r.current_branch.clone());
    assert_eq!(cur.as_deref(), Some("main"));
}

const LIST_RIGHT: f32 = 1024.0;
const LIST_LEFT: f32 = 0.0;

#[track_caller]
fn tool_window_top(harness: &Harness<'_, AppState>) -> f32 {
    harness.get_by_label("Changes").rect().max.y
}

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

fn open_row_menu(harness: &mut Harness<'_, AppState>, branch: &str) {
    row_node(harness, branch).click_secondary();
    harness.step();
    harness.step();
    harness.remove_cursor();
    harness.step();
}

fn menu_action(harness: &mut Harness<'_, AppState>, branch: &str, label: &str) {
    open_row_menu(harness, branch);
    harness
        .get_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("menu item {label}"))
        .click();
    settle_quiet(harness);
}

fn row_node<'t>(harness: &'t Harness<'_, AppState>, name: &str) -> egui_kittest::Node<'t> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(name.to_string()))
        .unwrap_or_else(|| panic!("row button for {name}"))
}

fn click_then_enter(harness: &mut Harness<'static, AppState>, branch: &str) {
    harness
        .get_all_by_label(branch)
        .next()
        .unwrap_or_else(|| panic!("local {branch} row"))
        .click();
    settle_quiet(harness);
    harness.key_press(egui::Key::Enter);
}

fn double_click(harness: &mut Harness<'static, AppState>, branch: &str) {
    let node = row_node(harness, branch);
    node.click();
    let _ = node;
    harness.step();
    let node = row_node(harness, branch);
    node.click();
    let _ = node;
}

type CheckoutGesture = fn(&mut Harness<'static, AppState>, &str);

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

    type_search(&mut harness, "mre");
    assert_painted(&harness, "feature/");
    assert_painted(&harness, "multi-root-executor");
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

    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    type_search(&mut harness, "zebra");
    assert_painted(&harness, "zebra");
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("feature-a"),
        "filtering never drops the selection"
    );

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

    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    harness.key_press(egui::Key::Enter);
    settle_quiet(&mut harness);

    assert_painted(&harness, "You have uncommitted changes");
    assert_painted(&harness, "Bring along");
    assert_painted(&harness, "Set aside");
    assert_painted(&harness, "Cancel");
    assert_painted(&harness, "if any conflict with the other branch");

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

    let stash = git_with_env(&alpha, &["stash", "list"], &GitEnv::fixture());
    assert!(
        stash.contains("set aside before checkout"),
        "stash must hold the changes; got {stash:?}"
    );
    let clean_tree = std::fs::read_to_string(alpha.join("README.md")).unwrap();
    assert!(
        !clean_tree.contains("dirty"),
        "feature-b's tree is the committed version, got {clean_tree:?}"
    );

    git_with_env(&alpha, &["checkout", "-q", "main"], &GitEnv::fixture());
    git_with_env(&alpha, &["stash", "pop"], &GitEnv::fixture());
    let restored = std::fs::read_to_string(alpha.join("README.md")).unwrap();
    assert!(
        restored.contains("dirty edit"),
        "the set-aside changes return intact, got {restored:?}"
    );
}

#[test]
fn clean_checkout_switches_and_notes_the_switch_quietly() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

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

    assert_painted(&harness, "New Branch");
    harness.get_by_label("New Branch").click();
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(turbogit_app::state::Dialog::NewBranch)
    );

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

struct CreateCase {
    what: &'static str,
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
            assert!(harness.state().ui.branches_tree.scroll_to.is_none());
        }
        assert_painted(&harness, case.name);
    }
}

#[test]
fn merge_from_detail_opens_the_preflighted_dialog() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    menu_action(&mut harness, "feature-a", "Merge into main");

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

fn conflict_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    git_with_env(&alpha, &["config", "user.email", "t@t"], &GitEnv::fixture());
    git_with_env(&alpha, &["config", "user.name", "t"], &GitEnv::fixture());
    std::fs::write(alpha.join("f.txt"), "base\n").unwrap();
    git_with_env(&alpha, &["add", "."], &GitEnv::fixture());
    git_with_env(&alpha, &["commit", "-m", "base"], &GitEnv::fixture());

    git_with_env(
        &alpha,
        &["checkout", "-q", "-b", "conflict-a"],
        &GitEnv::fixture(),
    );
    std::fs::write(alpha.join("f.txt"), "a\n").unwrap();
    git_with_env(
        &alpha,
        &["commit", "-am", "conflict side"],
        &GitEnv::fixture(),
    );
    git_with_env(&alpha, &["checkout", "-q", "main"], &GitEnv::fixture());
    std::fs::write(alpha.join("f.txt"), "b\n").unwrap();
    git_with_env(&alpha, &["commit", "-am", "main side"], &GitEnv::fixture());
    (tmp, project)
}

#[test]
fn conflicted_merge_hands_off_to_the_conflict_experience() {
    let (_project, _dir, mut harness) =
        open_branches_over(conflict_repo_project(), branches_harness);

    menu_action(&mut harness, "conflict-a", "Merge into main");
    harness.get_by_label("Merge").click();

    pump_until(&mut harness, "conflicted merge hands off", |s| {
        s.ui.conflict_resolver_open && s.ui.merge_in_progress
    });
    let last = harness.state().ui.activity.entries.last().cloned().unwrap();
    assert!(
        last.message.contains("unresolved conflicts"),
        "the report says what happened, got {:?}",
        last.message
    );

    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(&mut harness);
    assert_painted(&harness, "merging…");
}

#[test]
fn compare_from_detail_opens_read_only_and_closes_back_to_the_same_place() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    row_node(&harness, "feature-a").click();
    settle_quiet(&mut harness);
    let scroll_before = harness.state().ui.branches_tree.scroll;
    menu_action(&mut harness, "feature-a", "Compare with main");

    assert_painted(&harness, "Compare feature-a with main");
    assert_painted(&harness, "No commits — branches are in sync.");
    assert_painted(&harness, "Swap Branches");
    assert_painted(&harness, "Close");
    assert!(
        !harness
            .get_by_label("Swap Branches")
            .accesskit_node()
            .is_disabled(),
        "swapping sides stays available (a view toggle, not a write)"
    );

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

#[test]
fn rename_is_inline_on_the_row_and_resorts_correctly() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    menu_action(&mut harness, "feature-a", "Rename branch");

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
            let st = harness.state_mut();
            let id = st.selected_root.clone().unwrap();
            if let Some(r) = st.multi.roots.iter_mut().find(|r| r.id == id)
                && let Some(b) = r.branches.iter_mut().find(|b| b.name == "feature-a")
            {
                b.tracking = turbogit_domain::model::Upstream::from_git_ref("origin/feature-a");
            }
        });

    menu_action(&mut harness, "feature-a", "Rename branch");

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

#[test]
fn delete_names_the_consequence_and_undo_restores() {
    let (_project, _dir, mut harness) =
        open_branches_over(conflict_repo_project(), branches_harness);

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

#[test]
fn remote_rows_group_under_their_remote_and_are_quiet() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

    assert_not_painted(&harness, "remote-only");
    show_remotes(&mut harness);

    assert!(
        galley_origin(&harness, "origin").is_some(),
        "the remote group header names the remote"
    );
    assert!(
        galley_origin(&harness, "remote-only").is_some(),
        "the leaf shows the prefix-stripped name"
    );
    assert_not_painted(&harness, "origin/remote-only");
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

    click_fetch(&mut harness);
    pump_until(&mut harness, "fetch reports nothing changed", |s| {
        s.ui.activity
            .entries
            .last()
            .is_some_and(|e| e.message == "Fetch · nothing changed")
    });

    git_with_env(
        &dir.join("origin.git"),
        &["branch", "fresh-remote"],
        &GitEnv::fixture(),
    );
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
    show_remotes(&mut harness);

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

mod pure {
    use chrono::Utc;
    use turbogit_domain::model::{Branch, BranchKind, BranchTip};
    use turbogit_ui::ui::branches::{branch_matches, fuzzy_word, is_stale, row_meta};
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
    fn stale_threshold_is_roughly_four_weeks() {
        let now = Utc::now();
        let fresh = b("f", Some(now.timestamp() - 6 * 24 * 60 * 60));
        assert!(!is_stale(&fresh, now), "under a week is not stale");
        let edge = b("e", Some(now.timestamp() - 27 * 24 * 60 * 60));
        assert!(!is_stale(&edge, now), "27 days stays inside the window");
        let stale = b("s", Some(now.timestamp() - 30 * 24 * 60 * 60));
        assert!(is_stale(&stale, now), "30 days dims the row");
        let unknown = b("u", None);
        assert!(!is_stale(&unknown, now));
    }

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
        RowMetaCase {
            what: "diverged",
            name: "feat",
            tracking: Some("origin/main"),
            ahead: 2,
            behind: 1,
            gone: false,
            expect_upstream: Some("origin/main"),
            expect_badge: &[(SyncKind::Ahead, "↑2"), (SyncKind::Behind, "↓1")],
        },
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

    #[test]
    fn fuzzy_word_is_subsequence_and_case_insensitive() {
        assert!(fuzzy_word("feature/multi-root-executor", "mre"));
        assert!(fuzzy_word("feature/multi-root-executor", "MRE"));
        assert!(fuzzy_word("feature/multi-root-executor", "mro"));
        assert!(!fuzzy_word("feature/multi-root-executor", "zzz"));
        assert!(fuzzy_word("anything", ""));
    }

    #[test]
    fn multiword_query_needs_every_word() {
        let br = b("feature/multi-root-executor", Some(0));
        assert!(branch_matches(&br, "multi root"));
        assert!(branch_matches(&br, "root executor"));
        assert!(!branch_matches(&br, "multi zebra"));
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

fn two_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    for name in ["alpha", "beta"] {
        git_with_env(
            &project,
            &["-c", "init.defaultBranch=main", "init", name],
            &GitEnv::fixture(),
        );
        commit_readme(&project.join(name));
    }
    git_with_env(
        &project,
        &[
            "clone",
            "--bare",
            project.join("alpha").to_str().unwrap(),
            "origin.git",
        ],
        &GitEnv::fixture(),
    );
    let alpha = project.join("alpha");
    git_with_env(
        &alpha,
        &["remote", "add", "origin", "../origin.git"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["push", "-q", "origin", "main"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &alpha,
        &["push", "-q", "origin", "main:remote-only"],
        &GitEnv::fixture(),
    );
    git_with_env(&alpha, &["branch", "feature-a"], &GitEnv::fixture());
    git_with_env(
        &project.join("beta"),
        &["branch", "clever"],
        &GitEnv::fixture(),
    );
    git_with_env(&alpha, &["fetch", "-q", "origin"], &GitEnv::fixture());
    (tmp, project)
}

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
    harness.set_size(egui::vec2(1008.0, 768.0));
    harness
}

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

    assert_section(&harness, "LOCAL", 2);

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

    assert_painted(&harness, "all 2 repos");

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

    git_with_env(
        &dir.join("origin.git"),
        &["branch", "fresh-remote"],
        &GitEnv::fixture(),
    );
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

#[test]
fn keyboard_path_filter_arrow_enter_checks_out() {
    let (_project, _dir, mut harness) = open_branches_over(single_repo_project(), branches_harness);

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

type StartOperation = fn(&mut AppState);

const MID_OPERATION: &[(&str, StartOperation, Tab)] = &[
    ("merging…", |s| s.ui.merge_in_progress = true, Tab::Commit),
    ("working…", |s| s.ui.busy = true, Tab::Log),
];

#[test]
fn a_mid_operation_marks_the_list_in_place_and_survives_tab_switches() {
    for &(marker, start, away) in MID_OPERATION {
        let (_project, _dir, mut harness) =
            open_branches_over(single_repo_project(), branches_harness);

        start(harness.state_mut());
        harness.step();
        settle_quiet(&mut harness);
        assert_painted(&harness, marker);

        harness.state_mut().ui.tab = away;
        settle_quiet(&mut harness);
        harness.state_mut().ui.tab = Tab::Branches;
        settle_quiet(&mut harness);
        assert_painted(&harness, marker);

        harness.state_mut().ui.busy = false;
        harness.state_mut().ui.merge_in_progress = false;
        harness.step();
        settle_quiet(&mut harness);
        assert_not_painted(&harness, marker);
    }
}

fn galleys_in(harness: &Harness<'_, AppState>, rect: egui::Rect) -> Vec<PaintedGalley> {
    painted_galleys(harness)
        .into_iter()
        .filter(|g| rect.contains(g.pos))
        .collect()
}

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

    show_remotes(&mut harness);
    assert_not_painted(&harness, "fetched ");

    click_fetch(&mut harness);
    pump_until(&mut harness, "fetch stamped the last-fetch time", |s| {
        s.ui.branches_last_fetch.is_some()
    });
    assert_eq!(
        harness.state().ui.branches_tree.remotes_revealed.len(),
        1,
        "fetching never hides the repository's remotes again"
    );

    harness.state_mut().ui.branches_last_fetch =
        Some(chrono::Utc::now() - chrono::Duration::minutes(5));
    settle_quiet(&mut harness);
    assert_painted(&harness, "fetched 5m");

    let texts = painted_text(&harness);
    assert!(
        texts.iter().any(|t| t == "2"),
        "the expanded remote group counts its branches: {texts:?}"
    );
}

#[test]
fn fetch_sits_at_the_repo_level_while_remotes_are_hidden() {
    let (_project, _dir, harness) = open_branches_over(two_repo_project(), two_repo_harness);

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

#[test]
fn repo_section_headers_name_their_repo_inside_the_list() {
    use turbogit_ui::theme::Palette;

    let (_project, _dir, harness) = open_branches_over(two_repo_project(), two_repo_harness);

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

    let list_x = LIST_RIGHT;
    let dots: Vec<_> = filled_circles(&harness)
        .into_iter()
        .filter(|(c, r, _)| {
            c.y > list_top
                && c.x < list_x
                && (*r - turbogit_ui::ui::components::STATE_DOT_R).abs() < f32::EPSILON
        })
        .collect();
    assert!(
        dots.is_empty(),
        "no state dot paints in the branch list: {dots:?}"
    );

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
    assert_eq!(family_of("LOCAL"), FontFamily::Proportional);
    assert_eq!(family_of("TAGS"), FontFamily::Proportional);
    assert_eq!(
        family_of("1 remote · 2 branches"),
        FontFamily::Proportional,
        "the rollup is a label+count phrase, not a name"
    );

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

    assert_painted(&harness, "all 2 repos");

    let brand = test_support::harness::filled_rects(&harness)
        .into_iter()
        .any(|(r, f)| f == turbogit_ui::theme::Palette::ACCENT && r.intersects(new_branch));
    assert!(brand, "New Branch paints on an accent-filled surface");
}

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
    assert_eq!(
        harness.state().ui.tab,
        Tab::Branches,
        "opening the palette must not switch the active tool window"
    );
}

#[test]
fn a_branch_name_is_a_ref_chip_and_its_state_is_coloured_text_beside_it() {
    use turbogit_ui::theme::Palette;

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

    let (_project, _dir, harness) = open_branches_over(sync_repo_project(), branches_harness);
    assert_eq!(
        row_name_color(&harness, "feat"),
        Some(Palette::T_PRIMARY),
        "a diverged branch's *name* is not the state: the state is beside it"
    );
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
    assert!(
        state_inks_on_row(&harness, "main").is_empty(),
        "a branch in sync paints no state words beside its name"
    );
}
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
        .filter(|g| g.text != name && g.text != "origin/main" && g.pos.x > name_galley.rect.right())
        .collect();
    words.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
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
/// has to compile, no reflection, no stable `variant_count`, so the check is the
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
fn repo_bands_state_their_status_in_words_without_a_dot() {
    use turbogit_ui::theme::Palette;

    let (_project, _dir, harness) =
        open_branches_over_with(two_repo_project(), two_repo_harness, |harness| {
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
                .find(|b| b.kind == turbogit_domain::model::BranchKind::Local && b.name == "main")
                .expect("beta main");
            main.tracking = turbogit_domain::model::Upstream::from_git_ref("origin/main");
            main.ahead = 0;
            main.behind = 3;
        });

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
    let band_dots: Vec<_> = filled_circles(&harness)
        .into_iter()
        .filter(|(c, r, _)| {
            c.y > list_top
                && c.x < list_x
                && (*r - turbogit_ui::ui::components::STATE_DOT_R).abs() < f32::EPSILON
                && band_names.iter().any(|y| (c.y - y).abs() < 20.0)
        })
        .collect();
    assert_eq!(
        band_dots.len(),
        0,
        "no state dot on a repo band: the words state the state, found {band_dots:?}"
    );

    let beta_band = band_names[1];
    let hits: Vec<_> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.text.contains("unpulled commits") && (g.pos.y - beta_band).abs() < 20.0)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "the band states the state in words exactly once, on its own line: {hits:#?}"
    );
    assert_eq!(
        hits[0].color,
        Palette::COUNTER,
        "the state words wear the one repository-state colour map's ink"
    );
}

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

    assert_eq!(
        scope_chip(&harness).accesskit_node().label(),
        Some("all 2 repos".to_string()),
        "the chip announces the state, not a bare word"
    );
    assert_painted(&harness, "feature-a");
    assert_painted(&harness, "clever");

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

#[test]
fn no_chip_in_the_branches_pane_fills_the_brand_token() {
    let (_project, _dir, harness) = open_branches_over(two_repo_project(), two_repo_harness);
    assert_painted(&harness, "all 2 repos");

    let brand = turbogit_ui::theme::Palette::BRAND;
    let brand_rects: Vec<egui::Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == brand)
        .map(|(rect, _)| rect)
        .collect();

    let brand_chips: Vec<egui::Rect> = brand_rects
        .iter()
        .copied()
        .filter(|rect| (rect.height() - turbogit_ui::ui::widgets::CHIP_HEIGHT).abs() < 0.01)
        .collect();
    assert!(
        brand_chips.is_empty(),
        "no chip in the pane fills the brand token: {brand_chips:?}"
    );

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

#[test]
fn a_repository_bands_fetch_fetches_that_repository_only() {
    let (_project, _dir, mut harness) = open_branches_over(two_repo_project(), two_repo_harness);
    harness.state_mut().ui.branches_fetch_before.clear();

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

    for s in [
        "LOCAL",
        "New Branch",
        "Search branches",
        "feature-a",
        "zebra",
    ] {
        assert_painted(&harness, s);
    }
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
