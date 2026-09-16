//! Issue #05 — Workspace tree sidebar.
//!
//! The left rail lists every discovered repository root as a recursive
//! project tree (sidebar-project-tree issue 01): a workspace header with
//! the total repo count, a filter field, folder nodes with subtree counts
//! and tri-state checkboxes, and one row per repo with a status dot,
//! branch label, and ahead/behind badges. Single-repo folders collapse and
//! promote their repo with a path label; the rules are uniform under the
//! live and smart-group filters (issue 03).
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories (CONTEXT.md "Headless harness") and
//! assert only on public surfaces: painted labels, public `AppState`
//! transitions, and the exact galley texts painted into the frame.
use egui::accesskit::Role;
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::harness::{assert_not_painted, assert_painted, painted_text, settle};
use turbogit_app::state::AppState;

/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

/// Create an initialized temp repository with one base commit on `main`
/// plus an `origin` remote so upstream reads can be exercised.
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

/// A deterministic two-group project:
/// `<root>/wsb/<group>/<repo>` with groups `frontend` (alpha) and
/// `oss` (lib). Returns the project dir and the two repo paths.
fn two_group_project(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/wsb-{tag}"));
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

/// A deterministic two-repos-per-folder project:
/// `<root>/ws2/open/{alpha, ui}` and `<root>/ws2/oss/{cli, lib}`, so every
/// first-level folder displays. Returns the project dir and the four repo
/// paths.
fn two_by_two_project(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/ws2-{tag}"));
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

/// A nested project exercising every recursive shape:
/// `wsn/app/.git` + `wsn/app/core/.git` (a repo inside a repo),
/// `wsn/tools/{cli, gui}` (a folder over two repos), and
/// `wsn/foo/bar/.git` (a single-repo chain → path label `f/bar`).
/// Returns the project dir and every repo path.
fn nested_project(tag: &str) -> (PathBuf, Vec<PathBuf>) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/wsn-{tag}"));
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

/// A project with same-named folders at different depths:
/// `wss/a/src/{r1, r2}` and `wss/b/src/{r3, r4}` — collapse state must
/// key by relative path so one collapses independently of the other.
fn same_named_folders_project(tag: &str) -> (PathBuf, Vec<PathBuf>) {
    let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/wss-{tag}"));
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

/// Headless harness driving the full app UI (mirrors `workspace_shell_frame`).
/// Sized wide of the sidebar's small-window threshold so the rail renders.
fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    h.set_size(egui::vec2(1280.0, 800.0));
    h
}

fn app_state(project_dir: &Path, roots: &[PathBuf]) -> AppState {
    AppState::for_roots(project_dir, roots)
}

/// Assert some painted galley is exactly `text` (counts paint as their own
/// galleys, so exact matching keeps "2" distinct from "2 total").
#[track_caller]
fn assert_galley(harness: &Harness<'_, AppState>, text: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t == text),
        "`{text}` was not painted as an exact galley; painted text:\n{texts:#?}"
    );
}

// -- Cycle A — the tree paints: workspace header, folders, repo rows --

#[test]
fn sidebar_paints_workspace_header_folders_and_repo_rows() {
    let (project, alpha, ui, cli, lib) = two_by_two_project("paint");
    let state = app_state(&project, &[alpha, ui, cli, lib]);
    let mut h = harness(state);
    settle(&mut h);

    // Workspace header: project basename + the total repo count badge.
    assert_painted(&h, "ws2");
    assert_galley(&h, "4");
    // Filter field with its placeholder hint.
    assert_painted(&h, "Filter repos / branches…");
    // Section title.
    assert_painted(&h, "PROJECTS");
    // Folder rows: the project dir folder plus one per first-level group,
    // each holding two repos (the "2" folder badge).
    assert_galley(&h, "ws2");
    assert_galley(&h, "frontend");
    assert_galley(&h, "oss");
    assert_galley(&h, "2");
    // Repo rows with branch labels.
    assert_painted(&h, "alpha");
    assert_painted(&h, "ui");
    assert_painted(&h, "cli");
    assert_painted(&h, "lib");
    assert_painted(&h, "main");
}

#[test]
fn sidebar_repo_row_paints_ahead_badge_after_refresh() {
    let (project, alpha, lib) = two_group_project("badge");
    // Put alpha one commit ahead of its upstream.
    std::fs::write(alpha.join("a.txt"), "a\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-q", "-m", "ahead 1"]);

    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);
    // Ahead/behind fills through the same synchronous refresh the header
    // badge uses (headless harness path).
    h.get_by_label("Refresh").click();
    settle(&mut h);

    assert_galley(&h, "↑1");
}

#[test]
fn sidebar_hides_below_the_small_window_threshold() {
    let (project, alpha, lib) = two_group_project("small");
    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    // Below MIN_SIDEBAR_WINDOW_WIDTH the log's minimum pane sizes cannot
    // hold next to the rail (issue #23), so the sidebar hides entirely.
    h.set_size(egui::vec2(600.0, 400.0));
    settle(&mut h);

    assert_not_painted(&h, "PROJECTS");
    assert_not_painted(&h, "Filter repos / branches…");
}

// -- Cycle E — built-in smart groups (issue #06) --

/// Run `git` in `repo` without asserting success (for expected failures
/// such as a conflicting merge).
fn git_raw(repo: &Path, args: &[&str]) {
    let _ = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output();
}

/// A three-repo project exercising every built-in smart-group predicate:
/// `alpha` is diverged (one local commit, one fetched upstream commit and
/// therefore also unpushed), `lib` is dirty (an untracked file), `extra`
/// sits mid-merge with unresolved conflicts. Returns the project dir and
/// the three repo paths.
fn smart_group_project(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let (project, alpha, lib) = two_group_project(tag);
    let frontend = project.join("frontend");
    let extra = temp_repo(&frontend, "extra");

    // lib: dirty worktree (untracked file).
    std::fs::write(lib.join("wip.txt"), "wip\n").unwrap();

    // alpha: diverged — commit locally, then land a different commit on
    // origin from a scratch clone and fetch it.
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

    // extra: leave a merge mid-conflict on main. The main-side commit is
    // pushed first so the repo is conflicted but *not* unpushed.
    git(&extra, &["checkout", "-q", "-b", "other"]);
    std::fs::write(extra.join("f.txt"), "other\n").unwrap();
    git(&extra, &["add", "."]);
    git(&extra, &["commit", "-q", "-m", "other side"]);
    git(&extra, &["checkout", "-q", "main"]);
    std::fs::write(extra.join("f.txt"), "main\n").unwrap();
    git(&extra, &["add", "."]);
    git(&extra, &["commit", "-q", "-m", "main side"]);
    git(&extra, &["push", "-q", "origin", "main"]);
    git_raw(&extra, &["merge", "other"]); // exits non-zero on purpose

    (project, alpha, lib, extra)
}

#[test]
fn smart_groups_paint_with_member_counts() {
    let (project, alpha, lib, extra) = smart_group_project("sg-paint");
    let state = app_state(&project, &[alpha, lib, extra]);
    let mut h = harness(state);
    settle(&mut h);
    h.get_by_label("Refresh").click();
    settle(&mut h);

    // The section header and every non-empty built-in group (the diverged
    // alpha is also behind its upstream, so "unpulled commits" joins the
    // row with its own member — issue 02)…
    assert_painted(&h, "SMART GROUPS");
    assert_painted(&h, "diverged");
    assert_painted(&h, "has conflicts");
    assert_painted(&h, "unpushed commits");
    assert_painted(&h, "unpulled commits");
    assert_painted(&h, "dirty worktree");
    // …each currently holding exactly one member (five count badges of 1).
    assert_galley(&h, "1");
}

/// Assert the sidebar no longer shows a repo row for `name`. Scoped through
/// the sidebar's own "Select repo {name}" checkbox label, because the Commit
/// window's one-tree (issue 04) also paints repo names — whole-window text
/// absence would be a false positive there.
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
    h.get_by_label("Refresh").click();
    settle(&mut h);

    // Only alpha has unpushed commits: the tree narrows to it, dropping
    // lib and extra plus the now-empty oss group.
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

    // Switching to another group re-filters: dirty worktree keeps only lib
    // (the status bar still paints the focused repo alpha, so the tree-level
    // signal is the frontend folder dropping out and the single surviving
    // chain collapsing to its path label).
    h.get_by_label("dirty worktree").click();
    settle(&mut h);
    assert_eq!(
        h.state().ui.sidebar_smart_group,
        Some("dirty worktree".into())
    );
    assert_galley(&h, "w/o/lib");
    h.get_by_label("Select repo lib");
    assert!(
        h.query_by_label("Select group frontend").is_none(),
        "the memberless frontend folder header must drop out"
    );

    // Clicking the active group again clears the filter: the whole tree
    // comes back.
    h.get_by_label("dirty worktree").click();
    settle(&mut h);
    assert_eq!(h.state().ui.sidebar_smart_group, None);
    assert_painted(&h, "alpha");
    assert_painted(&h, "extra");
    assert_painted(&h, "o/lib");
}

#[test]
fn clicking_unpulled_group_filters_to_repos_with_incoming_commits() {
    // Issue 02: "unpulled commits" behaves like the other built-ins — a
    // click narrows the tree to repos with behind > 0 (only the diverged
    // alpha here; lib's untracked file and extra's unresolved conflict do
    // not count as unpulled).
    let (project, alpha, lib, extra) = smart_group_project("sg-unpulled");
    let state = app_state(&project, &[alpha, lib, extra]);
    let mut h = harness(state);
    settle(&mut h);
    h.get_by_label("Refresh").click();
    settle(&mut h);

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
    h.get_by_label("Refresh").click();
    settle(&mut h);

    // All-clean workspace: zero-member groups hide entirely (the section
    // header itself still paints, per the design).
    assert_painted(&h, "SMART GROUPS");
    assert_not_painted(&h, "diverged");
    assert_not_painted(&h, "has conflicts");
    assert_not_painted(&h, "unpushed commits");
    assert_not_painted(&h, "dirty worktree");

    // lib gets uncommitted work on disk; the next refresh collects it into
    // the dirty worktree group with no manual action beyond the refresh.
    std::fs::write(lib.join("wip.txt"), "wip\n").unwrap();
    h.get_by_label("Refresh").click();
    settle(&mut h);
    assert_painted(&h, "dirty worktree");
    assert_galley(&h, "1");
    // Membership is computed, not manual: the still-clean alpha joins
    // nothing else.
    assert_not_painted(&h, "unpushed commits");
}

// -- Cycle F — user-defined smart group rules (issue #07) --

#[test]
fn new_rule_editor_creates_a_rule_that_collects_and_filters_repos() {
    let (project, alpha, lib) = two_group_project("rule-create");
    // lib sits on a release branch; alpha stays on main.
    git(&lib, &["checkout", "-q", "-b", "release/1"]);
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    // The "+" in the SMART GROUPS header opens the rule editor.
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
    // The rule renders alongside the built-ins with its live count…
    assert_painted(&h, "release branches");
    assert_galley(&h, "1");

    // …and evaluates like a built-in: clicking it narrows the tree to the
    // matching repos (lib only; the memberless frontend group drops out —
    // the breadcrumb still names the focused repo, so assert on the group).
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
    // Seed a saved rule directly (creation is covered above); with no
    // predicate it matches both repos, so the edit below can narrow it.
    h.state_mut().ui.smart_group_rules = vec![turbogit_app::smart_rules::SmartGroupRule {
        label: "release branches".into(),
        ..Default::default()
    }];
    settle(&mut h);

    // Edit: the pencil opens the editor prefilled with the rule.
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
    // Tighten the predicate to release/* and save.
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
    // The count narrowed to lib only.
    assert_galley(&h, "1");

    // Delete: the trash removes the rule from state and the workspace.
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

    // Create the demo rule through the editor.
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

    // Restart: relaunch the same project through the production launch
    // path and re-render. The rule comes back and still evaluates.
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

// -- Cycle B — clicking a repo focuses it everywhere --

#[test]
fn clicking_repo_row_focuses_it_in_header_and_breadcrumb() {
    let (project, alpha, lib) = two_group_project("focus");
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    // `for_roots` focuses the first registered root (alpha); the repo
    // header/breadcrumb show alpha's name.
    assert_eq!(
        h.state().selected_root,
        Some(turbogit_domain::model::RootId(alpha.clone().into()))
    );

    h.get_by_label("lib").click();
    settle(&mut h);

    // Public state: selection follows the click.
    assert_eq!(
        h.state().selected_root,
        Some(turbogit_domain::model::RootId(lib.clone().into()))
    );
    // The shell follows everywhere: breadcrumb/header repaint the focused
    // repo's path/name (the metadata rail that once carried the path was
    // removed in the redesign; the topbar breadcrumb keeps it reachable).
    assert_painted(&h, "oss/lib");
}

// -- Cycle C — folder collapses key by relative path --

#[test]
fn folder_header_toggles_collapse_and_expand() {
    let (project, alpha, ui, cli, lib) = two_by_two_project("collapse");
    let state = app_state(&project, &[alpha, ui, cli, lib]);
    let mut h = harness(state);
    settle(&mut h);

    // Collapse the `oss` folder: its rows disappear (a first-level folder's
    // relative-path key equals its name) and the key is recorded.
    h.get_by_label("oss").click();
    settle(&mut h);
    assert_not_painted(&h, "lib");
    assert_not_painted(&h, "cli");
    assert!(h.state().ui.sidebar_collapsed.contains("oss"));

    // Expand again: the rows return and the key is cleared.
    h.get_by_label("oss").click();
    settle(&mut h);
    assert_painted(&h, "lib");
    assert!(!h.state().ui.sidebar_collapsed.contains("oss"));
}

#[test]
fn same_named_folders_at_different_depths_collapse_independently() {
    let (project, repos) = same_named_folders_project("paths");
    let mut state = app_state(&project, &repos);
    // Seed a collapse for the `a/src` folder only; matching must key by
    // relative path, never by the name `src` alone.
    state.ui.sidebar_collapsed.insert("a/src".to_string());
    let mut h = harness(state);
    settle(&mut h);

    // Both `src` folders paint (they each hold two repos)…
    assert_painted(&h, "src");
    assert_painted(&h, "r3");
    assert_painted(&h, "r4");
    // …but only the a-side rows are hidden (the b-side folders stay
    // visible under their own `src`; absence is a sidebar-scoped checkbox
    // query so the Commit window's repo lists never interfere).
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

    // Restart the same project through the production launch path: the
    // relative-path key comes back and the folder stays collapsed.
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

// -- Cycle D — the filter narrows the tree live and re-collapses it --

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

    // Public state carries the query; one survivor re-collapses every
    // folder above it, so no view shows a folder with a single child.
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

    // A folder-name match keeps the whole subtree.
    h.state_mut().ui.sidebar_filter.clear();
    settle(&mut h);
    let search = h.get_by_label("Filter repos / branches…");
    search.focus();
    search.type_text("frontend");
    settle(&mut h);
    assert_painted(&h, "alpha");
    assert_painted(&h, "ui");
    assert_not_painted(&h, "lib");

    // Clearing the query restores the whole tree.
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

    // All-clean: folder badges show repo counts (a "2" per folder) and no
    // dirty badge ("1" appears nowhere — counts are 2 and 4).
    assert_galley(&h, "2");
    let texts = painted_text(&h);
    assert!(
        !texts.iter().any(|t| t == "1"),
        "an all-clean worktree paints no dirty badge: {texts:#?}"
    );

    // lib gets uncommitted work; the refresh lands it in the `oss` folder's
    // dirty badge (a "1" for one dirty repo in the subtree) and in the
    // "dirty worktree" smart-group row.
    std::fs::write(lib.join("wip.txt"), "wip\n").unwrap();
    h.get_by_label("Refresh").click();
    settle(&mut h);
    assert_galley(&h, "1");

    // A conflicted repo counts as dirty too (the smart-group predicate).
    assert_painted(&h, "dirty worktree");
}

// -- Background incoming check (issue #27) -------------------------------------

#[test]
fn background_poll_badges_incoming_commits_without_manual_refresh() {
    let (project, alpha, lib) = two_group_project("poll");
    // An incoming commit sits on alpha's remote; alpha itself has not
    // fetched, so nothing in the app knows about it yet.
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
    // Before the poll no badge exists: registration itself never fetches.
    assert!(
        !painted_text(&h).iter().any(|t| t == "↓1"),
        "no incoming badge may exist before the poll runs"
    );

    // One scheduler tick polls the remotes and lands the finding in the
    // ahead/behind caches; the next frame badges the row with no Refresh.
    h.state_mut().tick_incoming_poll(std::time::Instant::now());
    h.run();

    assert_galley(&h, "↓1");
}

// -- Sidebar-project-tree: nested shapes, path labels, expandable repos --

#[test]
fn nested_tree_paints_repo_inside_repo_and_promoted_path_labels() {
    let (project, repos) = nested_project("nested");
    let state = app_state(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    // The `tools` folder displays with its two repos beneath it.
    assert_galley(&h, "tools");
    assert_painted(&h, "cli");
    assert_painted(&h, "gui");
    // The single-repo chain `foo/bar` collapsed to its path label — the
    // folder wraps nothing.
    assert_galley(&h, "f/bar");
    // A repo inside a repo: `app` renders as a repo row (its own label)
    // with `core` beneath it, visible without any click.
    assert_galley(&h, "app");
    assert_painted(&h, "core");
    // Folder subtree totals count repos nested beneath repo rows: the
    // `wsn` folder badge and the header total both show all five repos.
    assert_galley(&h, "5");
}

#[test]
fn a_repo_with_nested_repos_collapses_and_expands_by_its_relative_path() {
    let (project, repos) = nested_project("expand");
    let state = app_state(&project, &repos);
    let mut h = harness(state);
    settle(&mut h);

    // Expanded by default: the nested repo is visible without a click.
    assert_painted(&h, "core");
    // The expander chevron collapses the nested repo, keyed by its
    // relative path.
    h.get_by_label("Contract repo app").click();
    settle(&mut h);
    assert_sidebar_repo_dropped(&h, "core");
    assert!(h.state().ui.sidebar_collapsed.contains("app"));
    assert_painted(&h, "app");
    // Expanding again brings the nested repo back and clears the key.
    h.get_by_label("Contract repo app").click();
    settle(&mut h);
    assert!(h.query_by_label("Select repo core").is_some());
    assert!(!h.state().ui.sidebar_collapsed.contains("app"));
}
