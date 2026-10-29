use egui::accesskit::Role;
use egui::{Color32, Key, Modifiers, Rect};
use egui_kittest::{Harness, kittest::Queryable as _};
use std::path::{Path, PathBuf};
use test_support::git_seed::git;
use test_support::harness::{
    assert_not_painted, assert_painted, filled_circles, filled_rects, painted_galleys,
    painted_text, settle, shell_harness_over_unstyled,
};
use turbogit_app::state::AppState;
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::components::{RowState, row_fill};

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    let bare = parent.join(format!("{name}.origin"));
    let _ = std::fs::remove_dir_all(&bare);
    git(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&path, &["push", "-q", "origin", "main"]);
    git(&path, &["branch", "--set-upstream-to=origin/main", "main"]);
    path
}

fn two_group_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = test_support::scratch_root(&format!("wsb-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsb");
    let frontend = project.join("frontend");
    let oss = project.join("oss");
    std::fs::create_dir_all(&frontend).unwrap();
    std::fs::create_dir_all(&oss).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let lib = temp_repo(&oss, "lib");
    (project, alpha, lib)
}

fn two_by_two_project(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    let base = test_support::scratch_root(&format!("ws2-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("ws2");
    let frontend = project.join("frontend");
    let oss = project.join("oss");
    std::fs::create_dir_all(&frontend).unwrap();
    std::fs::create_dir_all(&oss).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    let cli = temp_repo(&oss, "cli");
    let lib = temp_repo(&oss, "lib");
    (project, alpha, ui, cli, lib)
}

fn nested_project(tag: &str) -> (PathBuf, Vec<PathBuf>) {
    let base = test_support::scratch_root(&format!("wsn-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wsn");
    let tools = project.join("tools");
    let foo_dir = project.join("foo");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::create_dir_all(&foo_dir).unwrap();
    let app = temp_repo(&project, "app");
    let core = temp_repo(&app, "core");
    let cli = temp_repo(&tools, "cli");
    let gui = temp_repo(&tools, "gui");
    let bar = temp_repo(&foo_dir, "bar");
    (project, vec![app, core, cli, gui, bar])
}

fn same_named_folders_project(tag: &str) -> (PathBuf, Vec<PathBuf>) {
    let base = test_support::scratch_root(&format!("wss-{tag}"));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("wss");
    let a_src = project.join("a").join("src");
    let b_src = project.join("b").join("src");
    std::fs::create_dir_all(&a_src).unwrap();
    std::fs::create_dir_all(&b_src).unwrap();
    let r1 = temp_repo(&a_src, "r1");
    let r2 = temp_repo(&a_src, "r2");
    let r3 = temp_repo(&b_src, "r3");
    let r4 = temp_repo(&b_src, "r4");
    (project, vec![r1, r2, r3, r4])
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024)
}

fn app_state(project_dir: &Path, roots: &[PathBuf]) -> AppState {
    AppState::for_roots(project_dir, roots)
}

#[track_caller]
fn assert_galley(harness: &Harness<'_, AppState>, text: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t == text),
        "`{text}` was not painted as an exact galley; painted text:\n{texts:#?}"
    );
}

#[track_caller]
fn manual_refresh(h: &mut Harness<'_, AppState>) {
    h.key_press_modifiers(Modifiers::CTRL, Key::T);
    settle(h);
}

#[test]
fn sidebar_paints_workspace_header_folders_and_repo_rows() {
    let (project, alpha, ui, cli, lib) = two_by_two_project("paint");
    let state = app_state(&project, &[alpha, ui, cli, lib]);
    let mut h = harness(state);
    settle(&mut h);

    assert_painted(&h, "ws2");
    assert_galley(&h, "4");
    assert_painted(&h, "Filter repos / branches…");
    assert_painted(&h, "PROJECTS");
    assert_galley(&h, "ws2");
    assert_galley(&h, "frontend");
    assert_galley(&h, "oss");
    assert_galley(&h, "2");
    assert_painted(&h, "alpha");
    assert_painted(&h, "ui");
    assert_painted(&h, "cli");
    assert_painted(&h, "lib");
    assert_painted(&h, "main");
}

#[test]
fn sidebar_repo_row_paints_ahead_badge_after_refresh() {
    let (project, alpha, lib) = two_group_project("badge");
    std::fs::write(alpha.join("a.txt"), "a\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-q", "-m", "ahead 1"]);

    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);

    assert_galley(&h, "↑1");
}

#[test]
fn sidebar_hides_below_the_small_window_threshold() {
    let (project, alpha, lib) = two_group_project("small");
    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    h.set_size(egui::vec2(600.0, 400.0));
    settle(&mut h);

    assert_not_painted(&h, "PROJECTS");
    assert_not_painted(&h, "Filter repos / branches…");
}

/// Run `git` in `repo` without asserting success (for expected failures
/// such as a conflicting merge).
/// Run `git` in `repo` WITHOUT asserting success, and deliberately NOT
/// `test_support::git_seed::git`, which asserts it.
///
/// `smart_group_project` puts `extra` mid-merge with a `git merge` that is expected to
/// exit non-zero; that refusal is what leaves the conflict the group counts.
fn git_raw(repo: &Path, args: &[&str]) {
    let _ = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output();
}

fn smart_group_project(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let (project, alpha, lib) = two_group_project(tag);
    let frontend = project.join("frontend");
    let extra = temp_repo(&frontend, "extra");

    std::fs::write(lib.join("wip.txt"), "wip\n").unwrap();

    std::fs::write(alpha.join("local.txt"), "local\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-q", "-m", "local commit"]);
    let base = project.parent().unwrap();
    let seed = base.join(format!("seed-{tag}"));
    let _ = std::fs::remove_dir_all(&seed);
    git(
        base,
        &[
            "clone",
            "-q",
            frontend.join("alpha.origin").to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    );
    std::fs::write(seed.join("remote.txt"), "remote\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "remote commit"]);
    git(&seed, &["push", "-q", "origin", "main"]);
    git(&alpha, &["fetch", "-q"]);

    git(&extra, &["checkout", "-q", "-b", "other"]);
    std::fs::write(extra.join("f.txt"), "other\n").unwrap();
    git(&extra, &["add", "."]);
    git(&extra, &["commit", "-q", "-m", "other side"]);
    git(&extra, &["checkout", "-q", "main"]);
    std::fs::write(extra.join("f.txt"), "main\n").unwrap();
    git(&extra, &["add", "."]);
    git(&extra, &["commit", "-q", "-m", "main side"]);
    git(&extra, &["push", "-q", "origin", "main"]);
    git_raw(&extra, &["merge", "other"]);
    (project, alpha, lib, extra)
}

#[test]
fn smart_groups_paint_with_member_counts() {
    let (project, alpha, lib, extra) = smart_group_project("sg-paint");
    let state = app_state(&project, &[alpha, lib, extra]);
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);

    assert_painted(&h, "SMART GROUPS");
    assert_painted(&h, "diverged");
    assert_painted(&h, "has conflicts");
    assert_painted(&h, "unpushed commits");
    assert_painted(&h, "unpulled commits");
    assert_painted(&h, "dirty");
    assert_galley(&h, "1");
}

#[track_caller]
fn assert_sidebar_repo_dropped(h: &Harness<'_, AppState>, name: &str) {
    assert!(
        h.query_by_label(&format!("Select repo {name}")).is_none(),
        "the sidebar repo row for `{name}` must drop out while filtered"
    );
}

#[test]
fn clicking_a_smart_group_filters_the_tree_to_its_members() {
    let (project, alpha, lib, extra) = smart_group_project("sg-click");
    let state = app_state(&project, &[alpha, lib, extra]);
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);

    h.get_by_label("unpushed commits").click();
    settle(&mut h);
    assert_eq!(
        h.state().ui.sidebar_smart_group,
        Some("unpushed commits".into())
    );
    assert_painted(&h, "alpha");
    assert_sidebar_repo_dropped(&h, "lib");
    assert_sidebar_repo_dropped(&h, "extra");
    assert!(
        h.query_by_label("Select group oss").is_none(),
        "the memberless oss group header must drop out"
    );

    h.get_by_label("dirty").click();
    settle(&mut h);
    assert_eq!(h.state().ui.sidebar_smart_group, Some("dirty".into()));
    assert_galley(&h, "w/o/lib");
    h.get_by_label("Select repo lib");
    assert!(
        h.query_by_label("Select group frontend").is_none(),
        "the memberless frontend folder header must drop out"
    );

    h.get_by_label("dirty").click();
    settle(&mut h);
    assert_eq!(h.state().ui.sidebar_smart_group, None);
    assert_painted(&h, "alpha");
    assert_painted(&h, "extra");
    assert_painted(&h, "o/lib");
}

#[test]
fn clicking_unpulled_group_filters_to_repos_with_incoming_commits() {
    let (project, alpha, lib, extra) = smart_group_project("sg-unpulled");
    let state = app_state(&project, &[alpha, lib, extra]);
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);

    h.get_by_label("unpulled commits").click();
    settle(&mut h);
    assert_eq!(
        h.state().ui.sidebar_smart_group.as_deref(),
        Some("unpulled commits")
    );
    assert_painted(&h, "alpha");
    assert_sidebar_repo_dropped(&h, "lib");
    assert!(
        h.query_by_label("Select group oss").is_none(),
        "the memberless oss group header must drop out"
    );
}

#[test]
fn smart_group_membership_follows_a_refresh() {
    let (project, alpha, lib) = two_group_project("sg-refresh");
    let state = app_state(&project, &[alpha, lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);

    assert_painted(&h, "SMART GROUPS");
    assert_not_painted(&h, "diverged");
    assert_not_painted(&h, "has conflicts");
    assert_not_painted(&h, "unpushed commits");
    assert_not_painted(&h, "dirty");

    std::fs::write(lib.join("wip.txt"), "wip\n").unwrap();
    manual_refresh(&mut h);
    assert_painted(&h, "dirty");
    assert_galley(&h, "1");
    assert_not_painted(&h, "unpushed commits");
}

#[test]
fn new_rule_editor_creates_a_rule_that_collects_and_filters_repos() {
    let (project, alpha, lib) = two_group_project("rule-create");
    git(&lib, &["checkout", "-q", "-b", "release/1"]);
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("New rule").click();
    settle(&mut h);
    assert!(
        h.state().ui.smart_rule_editor_open,
        "the editor modal is open"
    );
    assert_painted(&h, "Smart Group Rule");

    // Name + branch pattern is enough for the demo rule. Inputs are
    // queried by role so the row captions (plain labels) never shadow
    // them; the focus handoff needs a settle per field: the queued
    // AccessKit focus action only lands on the next step.
    let name = h.get_by_role_and_label(Role::TextInput, "Rule name");
    name.focus();
    name.type_text("release branches");
    settle(&mut h);
    let pattern = h.get_by_role_and_label(Role::TextInput, "Rule branch pattern");
    pattern.focus();
    let _ = pattern;
    settle(&mut h);
    assert!(
        h.get_by_role_and_label(Role::TextInput, "Rule branch pattern")
            .is_focused(),
        "the branch pattern input took focus"
    );
    h.get_by_role_and_label(Role::TextInput, "Rule branch pattern")
        .type_text("release/*");
    settle(&mut h);
    h.get_by_label("Save").click();
    settle(&mut h);

    assert!(
        !h.state().ui.smart_rule_editor_open,
        "saving closes the editor"
    );
    assert_eq!(h.state().ui.smart_group_rules.len(), 1);
    assert_painted(&h, "release branches");
    assert_galley(&h, "1");

    h.get_by_label("release branches").click();
    settle(&mut h);
    assert_eq!(
        h.state().ui.sidebar_smart_group.as_deref(),
        Some("release branches")
    );
    assert_painted(&h, "lib");
    let texts = painted_text(&h);
    assert!(
        !texts.iter().any(|t| t == "frontend"),
        "the memberless frontend group header must drop out: {texts:?}"
    );
}

#[test]
fn rule_rows_offer_edit_and_delete() {
    let (project, alpha, lib) = two_group_project("rule-edit");
    git(&lib, &["checkout", "-q", "-b", "release/1"]);
    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    h.state_mut().ui.smart_group_rules = vec![turbogit_app::smart_rules::SmartGroupRule {
        label: "release branches".into(),
        ..Default::default()
    }];
    settle(&mut h);

    h.get_by_label("Edit rule release branches").click();
    settle(&mut h);
    assert!(
        h.state().ui.smart_rule_editor_open,
        "the editor modal is open"
    );
    assert_eq!(h.state().ui.smart_rule_editing, Some(0));
    assert_eq!(
        h.state()
            .ui
            .smart_rule_draft
            .as_ref()
            .map(|r| r.label.as_str()),
        Some("release branches")
    );
    let pattern = h.get_by_role_and_label(Role::TextInput, "Rule branch pattern");
    pattern.focus();
    let _ = pattern;
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Rule branch pattern")
        .type_text("release/*");
    settle(&mut h);
    h.get_by_label("Save").click();
    settle(&mut h);

    assert_eq!(
        h.state().ui.smart_group_rules.len(),
        1,
        "editing replaces the rule, it does not append"
    );
    assert_eq!(
        h.state().ui.smart_group_rules[0].branch_pattern.as_deref(),
        Some("release/*")
    );
    assert_galley(&h, "1");

    h.get_by_label("Delete rule release branches").click();
    settle(&mut h);
    assert!(h.state().ui.smart_group_rules.is_empty());
    assert_not_painted(&h, "release branches");
}

#[test]
fn rules_survive_an_app_restart() {
    let (project, alpha, lib) = two_group_project("rule-restart");
    git(&lib, &["checkout", "-q", "-b", "release/1"]);
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("New rule").click();
    settle(&mut h);
    let name = h.get_by_role_and_label(Role::TextInput, "Rule name");
    name.focus();
    name.type_text("release branches");
    settle(&mut h);
    let pattern = h.get_by_role_and_label(Role::TextInput, "Rule branch pattern");
    pattern.focus();
    let _ = pattern;
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Rule branch pattern")
        .type_text("release/*");
    settle(&mut h);
    h.get_by_label("Save").click();
    settle(&mut h);
    assert_eq!(h.state().ui.smart_group_rules.len(), 1);
    drop(h);

    let recents_cfg = tempfile::tempdir().unwrap();
    let relaunched = turbogit_app::state::AppState::launch_in(
        Some(project.clone()),
        Some(recents_cfg.path().to_path_buf()),
    );
    let mut h2 = harness(relaunched);
    settle(&mut h2);

    assert_eq!(h2.state().ui.smart_group_rules.len(), 1);
    assert_painted(&h2, "release branches");
    assert_galley(&h2, "1");
}

#[test]
fn clicking_repo_row_moves_the_focus_band_to_it() {
    let (project, alpha, lib) = two_group_project("focus");
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    assert_eq!(
        h.state().selected_root,
        Some(turbogit_domain::model::RootId(alpha.clone().into()))
    );
    let alpha_row = h.get_by_label("alpha").rect();
    assert!(
        focused_band(&h).is_some_and(|b| alpha_row.intersect(b) == b),
        "the first registered root's row must start out carrying the focus band"
    );

    h.get_by_label("lib").click();
    settle(&mut h);

    assert_eq!(
        h.state().selected_root,
        Some(turbogit_domain::model::RootId(lib.clone().into()))
    );
    let lib_row = h.get_by_label("lib").rect();
    let band = focused_band(&h).expect("the clicked repo row must paint the focus-selected band");
    assert!(
        lib_row.intersect(band) == band,
        "the focus band must sit inside the clicked row; row {lib_row:?}, band {band:?}"
    );
}

fn focused_band(h: &Harness<'_, AppState>) -> Option<egui::Rect> {
    let fill = row_fill(RowState::FocusSelected);
    filled_rects(h)
        .into_iter()
        .find(|(r, c)| *c == fill && r.height() > 0.0)
        .map(|(r, _)| r)
}

#[test]
fn focused_repo_row_paints_its_branch_where_the_header_pill_did() {
    let (project, alpha, lib) = two_group_project("branch");
    git(&lib, &["checkout", "-q", "-b", "release/7"]);
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("lib").click();
    settle(&mut h);

    let row = h.get_by_label("lib").rect();
    let galleys = painted_galleys(&h);
    let inside: Vec<&str> = galleys
        .iter()
        .filter(|g| row.intersect(g.rect) == g.rect)
        .map(|g| g.text.as_str())
        .collect();
    assert!(
        inside.contains(&"release/7"),
        "the focused repo row must paint its branch; the row holds {inside:?}"
    );
    let alpha_row = h.get_by_label("alpha").rect();
    let alpha_inside: Vec<&str> = galleys
        .iter()
        .filter(|g| alpha_row.intersect(g.rect) == g.rect)
        .map(|g| g.text.as_str())
        .collect();
    assert!(
        alpha_inside.contains(&"main"),
        "an unfocused repo row must still paint its own branch; the row holds {alpha_inside:?}"
    );
    let branch_color = galleys
        .iter()
        .find(|g| g.text == "release/7" && row.intersect(g.rect) == g.rect)
        .expect("the branch galley inside the focused row")
        .color;
    assert_eq!(branch_color, Palette::INK_3, "the branch label's ink");
}

#[test]
fn folder_header_toggles_collapse_and_expand() {
    let (project, alpha, ui, cli, lib) = two_by_two_project("collapse");
    let state = app_state(&project, &[alpha, ui, cli, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("oss").click();
    settle(&mut h);
    assert_not_painted(&h, "lib");
    assert_not_painted(&h, "cli");
    assert!(h.state().ui.sidebar_collapsed.contains("oss"));

    h.get_by_label("oss").click();
    settle(&mut h);
    assert_painted(&h, "lib");
    assert!(!h.state().ui.sidebar_collapsed.contains("oss"));
}

#[test]
fn same_named_folders_at_different_depths_collapse_independently() {
    let (project, repos) = same_named_folders_project("paths");
    let mut state = app_state(&project, &repos);
    state.ui.sidebar_collapsed.insert("a/src".to_string());
    let mut h = harness(state);
    settle(&mut h);

    assert_painted(&h, "src");
    assert_painted(&h, "r3");
    assert_painted(&h, "r4");
    assert_sidebar_repo_dropped(&h, "r1");
    assert_sidebar_repo_dropped(&h, "r2");
    assert!(h.query_by_label("Select repo r3").is_some());
    assert!(h.query_by_label("Select repo r4").is_some());
    assert_eq!(h.state().ui.sidebar_collapsed.len(), 1);
    assert!(h.state().ui.sidebar_collapsed.contains("a/src"));
}

#[test]
fn collapse_state_survives_an_app_restart() {
    let (project, alpha, ui, cli, lib) = two_by_two_project("restart");
    let state = app_state(&project, &[alpha, ui, cli, lib]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("oss").click();
    settle(&mut h);
    assert!(h.state().ui.sidebar_collapsed.contains("oss"));
    drop(h);

    let recents_cfg = tempfile::tempdir().unwrap();
    let relaunched = turbogit_app::state::AppState::launch_in(
        Some(project.clone()),
        Some(recents_cfg.path().to_path_buf()),
    );
    let mut h2 = harness(relaunched);
    settle(&mut h2);
    assert!(h2.state().ui.sidebar_collapsed.contains("oss"));
    assert_not_painted(&h2, "lib");
    assert_not_painted(&h2, "cli");
}

#[test]
fn filter_input_narrows_the_tree_live_and_recollapses() {
    let (project, alpha, ui, cli, lib) = two_by_two_project("filter");
    let state = app_state(&project, &[alpha, ui, cli, lib]);
    let mut h = harness(state);
    settle(&mut h);

    let search = h.get_by_label("Filter repos / branches…");
    search.focus();
    search.type_text("lib");
    settle(&mut h);

    assert_eq!(h.state().ui.sidebar_filter, "lib");
    assert_galley(&h, "w/o/lib");
    let texts = painted_text(&h);
    assert!(
        !texts.iter().any(|t| t == "oss"),
        "a folder with one surviving repo must collapse: {texts:#?}"
    );
    assert!(
        !texts.iter().any(|t| t == "frontend"),
        "the memberless frontend folder must drop out: {texts:#?}"
    );

    h.state_mut().ui.sidebar_filter.clear();
    settle(&mut h);
    let search = h.get_by_label("Filter repos / branches…");
    search.focus();
    search.type_text("frontend");
    settle(&mut h);
    assert_painted(&h, "alpha");
    assert_painted(&h, "ui");
    assert_not_painted(&h, "lib");

    h.state_mut().ui.sidebar_filter.clear();
    settle(&mut h);
    assert_galley(&h, "frontend");
    assert_painted(&h, "lib");
}

#[test]
fn folder_rows_paint_subtree_dirty_badges_reflecting_the_worktree() {
    let (project, alpha, ui, cli, lib) = two_by_two_project("dirty-badge");
    let state = app_state(&project, &[alpha, ui, cli, lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    assert_galley(&h, "2");
    let texts = painted_text(&h);
    assert!(
        !texts.iter().any(|t| t == "1"),
        "an all-clean worktree paints no dirty badge: {texts:#?}"
    );

    std::fs::write(lib.join("wip.txt"), "wip\n").unwrap();
    manual_refresh(&mut h);
    assert_galley(&h, "1");

    assert_painted(&h, "dirty");
}

#[test]
fn background_poll_badges_incoming_commits_without_manual_refresh() {
    let (project, alpha, lib) = two_group_project("poll");
    let parent = alpha.parent().unwrap();
    let bare = parent.join("alpha.origin");
    let other = parent.join("alpha-other");
    git(
        parent,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.email", "test@example.com"]);
    git(&other, &["config", "user.name", "Test"]);
    git(&other, &["checkout", "-q", "main"]);
    std::fs::write(other.join("incoming.txt"), "incoming\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-q", "-m", "incoming"]);
    git(&other, &["push", "-q", "origin", "main"]);

    let state = AppState::for_roots(&project, &[alpha, lib]).with_settings(
        turbogit_domain::model::VcsSettings {
            incoming_poll: true,
            ..turbogit_domain::model::VcsSettings::default()
        },
    );
    let mut h = harness(state);
    settle(&mut h);
    assert!(
        !painted_text(&h).iter().any(|t| t == "↓1"),
        "no incoming badge may exist before the poll runs"
    );

    h.state_mut().tick_incoming_poll(std::time::Instant::now());
    h.run();

    assert_galley(&h, "↓1");
}

#[test]
fn nested_tree_paints_repo_inside_repo_and_promoted_path_labels() {
    let (project, repos) = nested_project("nested");
    let state = app_state(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    assert_galley(&h, "tools");
    assert_painted(&h, "cli");
    assert_painted(&h, "gui");
    assert_galley(&h, "f/bar");
    assert_galley(&h, "app");
    assert_painted(&h, "core");
    assert_galley(&h, "5");
}

#[test]
fn a_repo_with_nested_repos_collapses_and_expands_by_its_relative_path() {
    let (project, repos) = nested_project("expand");
    let state = app_state(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    assert_painted(&h, "core");
    h.get_by_label("Contract repo app").click();
    settle(&mut h);
    assert_sidebar_repo_dropped(&h, "core");
    assert!(h.state().ui.sidebar_collapsed.contains("app"));
    assert_painted(&h, "app");
    h.get_by_label("Contract repo app").click();
    settle(&mut h);
    assert!(h.query_by_label("Select repo core").is_some());
    assert!(!h.state().ui.sidebar_collapsed.contains("app"));
}

fn fills_in(h: &Harness<'_, AppState>, row: egui::Rect) -> Vec<(egui::Rect, egui::Color32)> {
    filled_rects(h)
        .into_iter()
        .filter(|(rect, _)| row.intersect(*rect) == *rect)
        .collect()
}

fn circles_in(h: &Harness<'_, AppState>, row: egui::Rect) -> Vec<(egui::Pos2, f32, egui::Color32)> {
    filled_circles(h)
        .into_iter()
        .filter(|(centre, _, _)| row.contains(*centre))
        .collect()
}

#[test]
fn the_rail_paints_its_own_surface_token() {
    let (project, alpha, lib) = two_group_project("surface");
    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    let rail = filled_rects(&h)
        .into_iter()
        .find(|(rect, color)| {
            *color == Palette::SIDEBAR
                && (rect.width() - turbogit_ui::ui::sidebar::SIDEBAR_WIDTH).abs() < 0.5
        })
        .map(|(rect, _)| rect)
        .expect("the rail paints `Palette::SIDEBAR` at the rail's own width");
    assert!(
        rail.height() > 100.0,
        "the rail's fill is the whole rail, not a strip: {rail:?}"
    );
    assert!(
        Palette::SIDEBAR != Palette::BG,
        "the sidebar surface is distinct from the app background — that is the \\
         whole reason the token exists"
    );
    for raised in [Palette::SURFACE, Palette::SURFACE_2, Palette::SURFACE_3] {
        assert_ne!(
            Palette::SIDEBAR,
            raised,
            "the rail must be darker than every raised surface, so a hover fill \\
             or a selection band inside it steps *up* from it"
        );
    }
}

#[test]
fn a_repository_rows_state_is_a_dot_in_the_one_state_colour_map() {
    use turbogit_ui::theme::RepoState;
    let (project, alpha, lib) = two_group_project("dot");
    let parent = alpha.parent().unwrap().to_path_buf();
    let bare = parent.join("alpha.origin");
    let other = parent.join("alpha-other");
    git(
        &parent,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.email", "test@example.com"]);
    git(&other, &["config", "user.name", "Test"]);
    git(&other, &["checkout", "-q", "main"]);
    std::fs::write(other.join("remote.txt"), "remote\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-q", "-m", "remote only"]);
    git(&other, &["push", "-q", "origin", "main"]);
    git(&alpha, &["fetch", "-q"]);
    std::fs::write(alpha.join("local.txt"), "local\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-q", "-m", "local only"]);

    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);

    let dot_in = |row_label: &str| -> Vec<(egui::Pos2, f32, egui::Color32)> {
        let row = h.get_by_label(row_label).rect();
        circles_in(&h, row)
    };
    let alpha_dots = dot_in("alpha");
    let lib_dots = dot_in("lib");
    assert_eq!(
        alpha_dots.len(),
        1,
        "a diverged repository paints exactly one state dot in its row, and it \\
         is a circle: {alpha_dots:?}"
    );
    assert_eq!(
        lib_dots.len(),
        1,
        "a clean repository paints exactly one state dot in its row too — the \\
         dot is the repository's state, not a warning: {lib_dots:?}"
    );
    assert_eq!(
        alpha_dots[0].2,
        turbogit_ui::ui::sidebar::dot_color(RepoState::Diverged),
        "the diverged repository's dot wears the one state map's diverged colour"
    );
    assert_eq!(
        lib_dots[0].2,
        turbogit_ui::ui::sidebar::dot_color(RepoState::Clean),
        "the clean repository's dot wears the one state map's clean colour"
    );
    assert_ne!(
        alpha_dots[0].2, lib_dots[0].2,
        "a clean repository and a diverged one must never look alike"
    );
    assert_eq!(
        alpha_dots[0].1, lib_dots[0].1,
        "the rail carries one state-dot radius"
    );

    let state_colors = [Palette::COUNTER, Palette::AHEAD, Palette::STATUS_DIVERGED];
    for row_label in ["alpha", "lib"] {
        let row = h.get_by_label(row_label).rect();
        for (rect, color) in fills_in(&h, row) {
            assert!(
                !state_colors.contains(&color),
                "a repository state must not fill a rect in the rail: {row_label} \\
                 painted {color:?} in {rect:?}"
            );
        }
    }
}

#[test]
fn the_sidebar_counters_are_count_chips() {
    let (project, alpha, ui, cli, lib) = two_by_two_project("chips");
    let state = app_state(&project, &[alpha, ui, cli, lib]);
    let mut h = harness(state);
    settle(&mut h);

    let geometry = turbogit_ui::ui::widgets::COMPACT_CHIP_GEOMETRY;
    let colors = turbogit_ui::ui::widgets::COUNT_CHIP_COLORS;
    let chips: Vec<Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, color)| {
            *color == colors.bg
                && (rect.height() - geometry.height).abs() < 0.01
                && (rect.width() - geometry.pad_x * 2.0) > 0.0
        })
        .map(|(rect, _)| rect)
        .collect();
    assert!(
        chips.len() >= 4,
        "the fixture must exercise every kind of counter — the workspace total, \\
         a folder total and the two smart-group counts. Found {chips:?}"
    );
    for chip in &chips {
        assert_eq!(
            chip.height(),
            geometry.height,
            "a counter is the count chip's height, not a literal: {chip:?}"
        );
        let inks: Vec<_> = painted_galleys(&h)
            .into_iter()
            .filter(|g| chip.contains_rect(g.rect))
            .map(|g| (g.text.clone(), g.color))
            .collect();
        assert!(
            !inks.is_empty(),
            "a count chip carries a number: {chip:?} painted no text"
        );
        for (text, ink) in inks {
            assert_eq!(
                ink, colors.fg,
                "a count chip's number wears the chip's own ink; `{text}` in \\
                 {chip:?} painted {ink:?}"
            );
        }
    }
    for (rect, color) in filled_rects(&h) {
        if (rect.height() - geometry.height).abs() < 0.01 {
            assert_ne!(
                color,
                Palette::COUNTER,
                "a count chip never fills the reserved counter orange: {rect:?}"
            );
            assert_ne!(
                color,
                Palette::AHEAD,
                "a count chip never fills a repository-state colour: {rect:?}"
            );
        }
    }
}

#[test]
fn the_reserved_counter_orange_on_the_sidebar_is_only_dirt_or_unpulled() {
    use turbogit_ui::theme::RepoState;
    let (project, alpha, ui, cli, lib) = two_by_two_project("orange");
    let parent = alpha.parent().unwrap().to_path_buf();
    let bare = parent.join("alpha.origin");
    let other = parent.join("alpha-other");
    git(
        &parent,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.email", "test@example.com"]);
    git(&other, &["config", "user.name", "Test"]);
    git(&other, &["checkout", "-q", "main"]);
    std::fs::write(other.join("remote.txt"), "remote\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-q", "-m", "remote only"]);
    git(&other, &["push", "-q", "origin", "main"]);
    git(&alpha, &["fetch", "-q"]);
    std::fs::write(ui.join("wip.txt"), "wip\n").unwrap();

    let state = app_state(&project, &[alpha, ui, cli, lib]);
    let mut h = harness(state);
    settle(&mut h);
    manual_refresh(&mut h);

    assert_eq!(RepoState::Dirty.color(), Palette::COUNTER);
    assert_eq!(RepoState::Unpulled.color(), Palette::COUNTER);
    for state_color in [RepoState::Clean, RepoState::Unpushed] {
        assert_ne!(
            state_color.color(),
            Palette::COUNTER,
            "{state_color:?} is not a dirt or unpulled state and may not wear \\
             the reserved counter orange"
        );
    }

    let rail = filled_rects(&h)
        .into_iter()
        .find(|(rect, color)| {
            *color == Palette::SIDEBAR
                && (rect.width() - turbogit_ui::ui::sidebar::SIDEBAR_WIDTH).abs() < 0.5
        })
        .map(|(rect, _)| rect)
        .expect("the rail's own surface");
    let in_rail = |rect: egui::Rect| rail.intersect(rect) == rect;

    let orange_text: Vec<(String, egui::Rect)> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.color == Palette::COUNTER && in_rail(g.rect))
        .map(|g| (g.text.clone(), g.rect))
        .collect();
    for (text, _) in &orange_text {
        assert!(
            !text.is_empty()
                && text
                    .chars()
                    .all(|c| c.is_ascii_digit() || "\u{2193}\u{2191} ".contains(c)),
            "the reserved counter orange is for dirt and unpushed COUNTS only, \
             and `{text}` is not one. Orange text painted in the rail: \
             {orange_text:?}"
        );
    }
    assert!(
        !orange_text.is_empty(),
        "the fixture must produce at least one dirt or unpushed count, or the \
         reservation is being asserted against nothing"
    );

    let chip_geometry = turbogit_ui::ui::widgets::COMPACT_CHIP_GEOMETRY;
    let chip_color = turbogit_ui::ui::widgets::COUNT_CHIP_COLORS;
    let chips_in_rail: Vec<(Rect, Color32)> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, color)| {
            *color == chip_color.bg
                && (rect.height() - chip_geometry.height).abs() < 0.01
                && in_rail(*rect)
        })
        .collect();
    for folder in ["frontend", "oss"] {
        let row = h.get_by_label(folder).rect();
        let orange_here: Vec<&egui::Rect> = orange_text
            .iter()
            .filter(|(_, r)| row.intersect(*r) == *r)
            .map(|(_, r)| r)
            .collect();
        if orange_here.is_empty() {
            continue;
        }
        let chip_here: Vec<&Rect> = chips_in_rail
            .iter()
            .filter(|(r, _)| row.intersect(*r) == *r)
            .map(|(r, _)| r)
            .collect();
        assert_eq!(
            chip_here.len(),
            1,
            "the `{folder}` row shows a dirty count, so it also shows its \
             repository total — and that total is the count chip. Orange in the \
             row: {orange_here:?}; chips in the row: {chip_here:?}; every chip in \
             the rail: {chips_in_rail:?}"
        );
        for orange in &orange_here {
            assert!(
                chip_here[0].left() < orange.left(),
                "in the `{folder}` row the dirty count is laid out first and the \
                 repository total's chip inside it; the chip is at {:?} and the \
                 orange count at {orange:?}",
                chip_here[0]
            );
        }
    }
    for (rect, color) in &chips_in_rail {
        assert_ne!(
            *color,
            Palette::COUNTER,
            "a count chip never fills the reserved counter orange: {rect:?}"
        );
    }

    let orange_dots: Vec<(egui::Pos2, f32)> = filled_circles(&h)
        .into_iter()
        .filter(|(centre, _, color)| *color == Palette::COUNTER && rail.contains(*centre))
        .map(|(centre, radius, _)| (centre, radius))
        .collect();
    for (centre, _) in &orange_dots {
        let on_a_smart_group_row = (centre.x - (rail.left() + 24.0)).abs() < 1.0;
        let in_the_repository_gutter = (centre.x - (rail.left() + 37.5)).abs() < 1.0;
        assert!(
            on_a_smart_group_row || in_the_repository_gutter,
            "an orange dot in the rail is a state dot in one of the rail's two \
             dot columns — a smart-group row's, or a repository row's status \
             gutter; found one at {centre:?} in a rail at {rail:?}"
        );
    }
}

#[test]
fn the_sidebar_group_sections_use_the_shared_pane_header() {
    use turbogit_ui::ui::widgets::PANE_HEADER_HEIGHT;
    let rule = Palette::RULE_STRUCTURAL;
    let (project, alpha, lib) = two_group_project("pane");
    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    let rail = filled_rects(&h)
        .into_iter()
        .find(|(rect, color)| {
            *color == Palette::SIDEBAR
                && (rect.width() - turbogit_ui::ui::sidebar::SIDEBAR_WIDTH).abs() < 0.5
        })
        .map(|(rect, _)| rect)
        .expect("the rail's own surface");
    let rail_rules: Vec<Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, color)| {
            *color == rule
                && (rect.height() - 1.0).abs() < 0.01
                && (rect.left() - rail.left()).abs() < 1.0
                && (rect.right() - rail.right()).abs() < 1.0
        })
        .map(|(rect, _)| rect)
        .collect();

    for title in ["SMART GROUPS", "PROJECTS"] {
        let galley = painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == title)
            .unwrap_or_else(|| panic!("`{title}` paints in the rail"));
        assert_eq!(
            galley.color,
            Palette::INK_3,
            "`{title}` wears the shared pane-title ink"
        );
        let under: Vec<Rect> = rail_rules
            .iter()
            .copied()
            .filter(|r| {
                r.top() >= galley.rect.bottom() - 1.0
                    && r.top() <= galley.rect.bottom() + PANE_HEADER_HEIGHT
            })
            .collect();
        assert_eq!(
            under.len(),
            1,
            "`{title}` is closed by exactly one structural hairline, the shared \
             pane header's own rule; found {under:?} among the rail's rules \
             {rail_rules:?}"
        );
        let rule_rect = under[0];
        let band_top = rule_rect.top() - PANE_HEADER_HEIGHT;
        let band = Rect::from_min_max(
            egui::pos2(rail.left(), band_top),
            egui::pos2(rail.right(), rule_rect.top()),
        );
        assert!(
            band.contains_rect(galley.rect),
            "`{title}` sits inside the shared band its rule closes: title {:?}, \
             band {band:?}",
            galley.rect
        );
        assert!(
            (galley.rect.center().y - band.center().y).abs() <= 1.0,
            "`{title}` is centred in the shared band: title centre {}, band \
             centre {}",
            galley.rect.center().y,
            band.center().y
        );
    }
    assert_eq!(
        rail_rules.len(),
        2,
        "the rail's two group sections wear one rule each and nothing else \
         paints a structural hairline in the rail: {rail_rules:?}"
    );
}

#[test]
fn the_rails_non_row_content_clears_the_rails_edges() {
    use turbogit_ui::ui::sidebar::RAIL_INSET;
    let (project, alpha, lib) = two_group_project("inset");
    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    let rail = filled_rects(&h)
        .into_iter()
        .find(|(rect, color)| {
            *color == Palette::SIDEBAR
                && (rect.width() - turbogit_ui::ui::sidebar::SIDEBAR_WIDTH).abs() < 0.5
        })
        .map(|(rect, _)| rect)
        .expect("the rail's own surface");

    for title in ["SMART GROUPS", "PROJECTS"] {
        let galley = painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == title)
            .unwrap_or_else(|| panic!("`{title}` paints in the rail"));
        assert!(
            galley.rect.left() >= rail.left() + RAIL_INSET - 0.5,
            "`{title}` clears the rail's left edge by the rail inset: galley \
             {:?}, rail {rail:?}",
            galley.rect
        );
    }

    let search = h.get_by_label("Filter repos / branches…");
    assert!(
        search.rect().left() >= rail.left() + RAIL_INSET - 0.5
            && search.rect().right() <= rail.right() - RAIL_INSET + 0.5,
        "the filter field clears both rail edges by the rail inset: {:?}, \
         rail {rail:?}",
        search.rect()
    );

    let new_rule = h.get_by_label("New rule");
    assert!(
        new_rule.rect().right() <= rail.right() - RAIL_INSET + 0.5,
        "the section action clears the rail's right edge by the rail inset: \
         {:?}, rail {rail:?}",
        new_rule.rect()
    );
}
