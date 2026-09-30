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
//! Since the shell's repo header was deleted, the rail's repo row is where
//! the focused root's branch is painted, and the rail is the only surface
//! carrying the project → root path (as the tree itself). Refresh no longer
//! has a button of its own here: the tests drive `Ctrl+T`, the frozen
//! shortcut the shell's own `handle_shortcuts` still binds to the same
//! `state.refresh(Affected::All)`.
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories (CONTEXT.md "Headless harness") and
//! assert only on public surfaces: painted labels, public `AppState`
//! transitions, and the exact galley texts painted into the frame.
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

/// Create an initialized temp repository with one base commit on `main`
/// plus an `origin` remote so upstream reads can be exercised.
/// Local repo builder, deliberately NOT `test_support::git_seed::repo_with_origin`.
///
/// The recipe commits `README.md`; this suite's change lists and diff panes name the
/// file they changed, so the base commit has to be `base.txt`. A `base.txt` recipe
/// would be `test-support` work, not this lane's.
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
///
/// Deliberately unstyled: this suite measures the rail's painted band, the focus
/// band's geometry, counter chips and row fills, and the styled preamble would
/// re-lay them out in the embedded font stack. `max_steps` is 1024 against
/// kittest's default of 4, which the styled constructor cannot express.
fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024)
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

/// Drive a manual refresh through `Ctrl+T`.
///
/// The repo header's right-aligned Refresh button is gone, and the rail has
/// no refresh control of its own. `Ctrl+T` is the frozen shortcut the shell
/// still binds to `state.refresh(Affected::All)` (`shell::handle_shortcuts`)
/// — the same dispatch the header button made, reachable from anywhere the
/// shell is up, so the headless harness gets its status/ahead-behind caches
/// filled synchronously exactly as before.
#[track_caller]
fn manual_refresh(h: &mut Harness<'_, AppState>) {
    h.key_press_modifiers(Modifiers::CTRL, Key::T);
    settle(h);
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
    // Ahead/behind fills through the same synchronous `Ctrl+T` refresh the
    // shell dispatches for a manual one (headless harness path).
    manual_refresh(&mut h);

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
/// Run `git` in `repo` WITHOUT asserting success, and deliberately NOT
/// `test_support::git_seed::git` — which asserts it.
///
/// `smart_group_project` puts `extra` mid-merge with a `git merge` that is expected to
/// exit non-zero; that refusal is what leaves the conflict the group counts.
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
    manual_refresh(&mut h);

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
    manual_refresh(&mut h);

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
    manual_refresh(&mut h);
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
    // the Commit window's one tree also paints repo names, so assert on the
    // sidebar's own group header).
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

// -- Cycle B — clicking a repo focuses it in the rail's own row -------------

#[test]
fn clicking_repo_row_moves_the_focus_band_to_it() {
    let (project, alpha, lib) = two_group_project("focus");
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    // `for_roots` focuses the first registered root (alpha), so the focus band
    // starts on alpha's row.
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

    // Public state: selection follows the click.
    assert_eq!(
        h.state().selected_root,
        Some(turbogit_domain::model::RootId(lib.clone().into()))
    );
    // And the rail follows: the focus band moved with the selection. This is
    // where the shell now shows the focused root — the project → root
    // breadcrumb that used to live in the deleted repo header is gone, and
    // the removed metadata rail took the path with it; the tree's own
    // structure and this selection band are what carry it.
    let lib_row = h.get_by_label("lib").rect();
    let band = focused_band(&h).expect("the clicked repo row must paint the focus-selected band");
    assert!(
        lib_row.intersect(band) == band,
        "the focus band must sit inside the clicked row; row {lib_row:?}, band {band:?}"
    );
}

/// The focus-selected fill painted for a row, if any. `RowState::FocusSelected`
/// is the tree/list vocabulary's focus band, and the fill is read off the token
/// rather than restated, so it keeps tracking the theme.
fn focused_band(h: &Harness<'_, AppState>) -> Option<egui::Rect> {
    let fill = row_fill(RowState::FocusSelected);
    filled_rects(h)
        .into_iter()
        .find(|(r, c)| *c == fill && r.height() > 0.0)
        .map(|(r, _)| r)
}

/// A repo row paints its own branch, and the focused one is where the shell
/// shows the current branch now that the repo header's branch pill is gone.
#[test]
fn focused_repo_row_paints_its_branch_where_the_header_pill_did() {
    let (project, alpha, lib) = two_group_project("branch");
    // A distinctive branch so the assertion cannot pass on some other "main".
    git(&lib, &["checkout", "-q", "-b", "release/7"]);
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("lib").click();
    settle(&mut h);

    // The branch is painted *inside lib's own row*, not merely somewhere in
    // the frame.
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
    // The unfocused sibling keeps its own branch, so the row's branch label is
    // a per-row fact rather than a single frame-wide readout.
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
    // Palette-wise the branch is the row's quiet ink, one step down from the
    // repo name beside it.
    let branch_color = galleys
        .iter()
        .find(|g| g.text == "release/7" && row.intersect(g.rect) == g.rect)
        .expect("the branch galley inside the focused row")
        .color;
    assert_eq!(branch_color, Palette::INK_3, "the branch label's ink");
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
    manual_refresh(&mut h);
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
    // ahead/behind caches; the next frame badges the row with no manual
    // refresh.
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

// -- 19 — the rail's vocabulary: one surface, one state map, one chip, one
//    pane header --------------------------------------------------
//
// The rail is the surface with the most small marks in the app — a surface, a
// selection, a state, four kinds of number and two section headers — and every
// one of them used to be spelled locally. These four tests are the rail's share
// of the design-system contract, and each is scoped to the *sidebar's own*
// files so a change in another screen cannot make them pass vacuously.

/// Every fill painted inside `row`, as `(rect, colour)`.
fn fills_in(h: &Harness<'_, AppState>, row: egui::Rect) -> Vec<(egui::Rect, egui::Color32)> {
    filled_rects(h)
        .into_iter()
        .filter(|(rect, _)| row.intersect(*rect) == *rect)
        .collect()
}

/// Every filled circle painted inside `row`, as `(centre, radius, colour)`.
fn circles_in(h: &Harness<'_, AppState>, row: egui::Rect) -> Vec<(egui::Pos2, f32, egui::Color32)> {
    filled_circles(h)
        .into_iter()
        .filter(|(centre, _, _)| row.contains(*centre))
        .collect()
}

/// The rail paints **its own** surface token, and it is the sidebar's fill.
///
/// Ticket 08 resolved the token's fate in favour of the sidebar adopting it,
/// which left "the surface token has a real consumer" satisfied by a comment
/// rather than by a render. This is the assertion that turns it back into a
/// fact: the rail's root fill is `Palette::SIDEBAR`, and the value is the
/// designed one — darker than the app background it sits beside, so the left
/// rail reads as a surface the content is *beside* rather than as an unlabelled
/// gap in it.
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
    // The value is the designed one, and the two relationships the token layer
    // documents about it are what make the rail's ladder hold: darker than the
    // app background beside it, darker than every raised surface inside it.
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

/// A repository's state is a **dot** in the rail's status gutter, and its
/// colour is the one repository-state map's answer.
///
/// R6 in one test: a state is a mark, so it is a circle and not a filled
/// rectangle; and it is coloured from `RepoState::color`, so a clean repository
/// and a diverged one can never wear the same colour. The dot is found by
/// geometry — a circle inside the repository row — rather than by a colour, so
/// a dot that came from anywhere but the map fails here rather than passing
/// because the right token happened to be named.
#[test]
fn a_repository_rows_state_is_a_dot_in_the_one_state_colour_map() {
    use turbogit_ui::theme::RepoState;
    let (project, alpha, lib) = two_group_project("dot");
    // `alpha` diverges: one local commit and one fetched upstream commit, so it
    // is neither clean nor merely unpushed. `lib` stays clean.
    // The bare remote `temp_repo` seeds sits beside the repo, so the "upstream"
    // commit is pushed from a second clone of it — the same shape the poll test
    // above uses.
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
    // The colours are the map's, read through the sidebar's own accessor, so
    // the assertion keeps tracking the theme rather than restating a token.
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
    // …and the two are different, which is the claim the map exists for.
    assert_ne!(
        alpha_dots[0].2, lib_dots[0].2,
        "a clean repository and a diverged one must never look alike"
    );
    // One dot size across the rail: the smart-group rows' dots and the
    // repository rows' dots are the same mark at the same scale.
    assert_eq!(
        alpha_dots[0].1, lib_dots[0].1,
        "the rail carries one state-dot radius"
    );

    // The negative half, and it is the one R6 is really about: **no
    // repository state reaches the rail as a filled shape.** A dot is a mark;
    // behind a fill it stops reading as state and starts reading as a
    // category, which is the mistake the branches screen made twice.
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

/// Every number in the rail is the **count chip** — the shared geometry and the
/// shared colour pair — so a number is not a fourth kind of badge.
///
/// The four counters that are numbers: the workspace header's repository total,
/// the folder rows' repository totals, and the smart-group and user-rule rows'
/// member counts. Each is asserted from painted output against
/// `widgets::COMPACT_CHIP_GEOMETRY` and `widgets::COUNT_CHIP_COLORS`, read off
/// the shared vocabulary rather than restated — so a rail that spelled its own
/// chip geometry, or reached for a different fill, fails here.
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
        // The ink is the chip's own secondary ink, resolved at paint time —
        // a galley laid out in white and never overridden would read as white
        // here and would be unreadable on the raised fill.
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
    // And the negative: a counter never wears a repository-state colour. A
    // number is a mark, not a category.
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

/// The reserved counter orange appears in the rail **only** where it is a dirt
/// or unpushed count.
///
/// This is the ratchet for the reservation itself, and it is stated as a
/// positive list rather than a scan for the colour, because a scan cannot say
/// *where* a colour is allowed. Three sites earn it, and each is named with
/// the fact that earns it:
///
/// | Site | Why orange is legal there |
/// |---|---|
/// | a folder row's **dirty** subtree count | it counts dirty repositories — dirt |
/// | a repository row's **↓N** incoming badge | it counts unpulled commits — unpushed |
/// | the **unpulled** and **dirty** smart-group dots | those two groups *are* dirt and unpushed |
///
/// Everything else that is a number is a count chip, which is neutral by
/// construction. The test walks the painted output and reports any orange that
/// is not one of the three.
#[test]
fn the_reserved_counter_orange_on_the_sidebar_is_only_dirt_or_unpulled() {
    use turbogit_ui::theme::RepoState;
    let (project, alpha, ui, cli, lib) = two_by_two_project("orange");
    // `alpha` goes behind its upstream (the unpulled case, the `↓N` badge) and
    // `ui` gets an untracked file (the dirty case, the folder's dirty count and
    // the dirty smart-group dot).
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

    // The map the reservation is stated in terms of: the orange is exactly the
    // dirty and unpulled answers, and nothing else in the map is orange.
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

    // Walk the rail: every orange pixel is a circle (a state dot) or a
    // text galley (a coloured count), and every one of them belongs to a
    // dirt-or-unpulled fact. Fills are excluded by construction — the previous
    // test already says no state fills a rect — so this is about the two
    // remaining shapes.
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
    // …and at least one of them fired, so the loop above is not passing
    // because nothing was orange.
    assert!(
        !orange_text.is_empty(),
        "the fixture must produce at least one dirt or unpushed count, or the \
         reservation is being asserted against nothing"
    );

    // **The counter that is a number is a chip, and the chip is not orange.**
    // This is the half that catches "let me just make this counter orange
    // too", and it is stated per row rather than as a colour scan, because the
    // mistake is not "somewhere in the rail" — it is "in the row where a
    // number already says something else".
    //
    // A folder row carries up to two numbers: the subtree's **dirty** count,
    // which is state and is coloured text, and its **repository total**, which
    // is a number and is the count chip. The rule is positional, and it is the
    // rule the sidebar's own layout already states: the dirty count is laid
    // out first (rightmost), the chip after it. So a row that shows an orange
    // state count must still show its total as a chip, and the chip must be
    // *inside* of the orange text. Paint the total orange instead and the
    // chip is gone.
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
    // The negative, over the whole rail: a count chip is a mark, not a
    // category, so it never takes a state colour — the orange included.
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
    // Every orange circle is a **state dot**, and the rail has exactly two dot
    // columns. Which column it is decides what the dot is allowed to say, and
    // that is the whole content of the reservation here:
    //
    // - the **smart-group** dot column, 24 points in: the group rows, whose two
    //   orange groups are *unpulled* and *dirty* by name;
    // - the **repository** status gutter, 37.5 points in: a repository whose
    //   state is dirty or unpulled — the one map's own two orange answers.
    //
    // So an orange dot in the repository gutter is a repository *state* saying
    // dirt or unpushed, which is exactly what the orange means, and one in the
    // group column is a group saying the same. Neither is a counter borrowing a
    // colour, which is the failure the reservation exists to prevent.
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

/// The rail's two group sections wear the **shared pane header**.
///
/// R7 for the sidebar: a section title in the rail is the same mark as a section
/// title in the log, the worktrees pane or the submodules pane — the shared
/// type, the shared muted ink, one structural hairline, the shared band height.
///
/// Asserted as a **relationship** between the title and the rule under it,
/// because that is what the shared header actually is: a band of
/// `PANE_HEADER_HEIGHT` whose title sits in it and whose one hairline closes
/// its bottom edge. Asserting either half alone would pass for a title with no
/// rule or a rule with no title; asserting the two together is the contract,
/// and it is what a hand-rolled section label at the wrong size cannot satisfy.
///
/// Before this the rail spelled both titles itself at `TYPE_CONTROL` — a
/// body-size label rather than a section label — and gave them no rule at all.
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
    // Every structural hairline in the rail: one pixel, the shared hairline
    // tone, spanning the rail's width. Reading them as a set first is what lets
    // the per-title assertion be about "the one below *this* title" rather than
    // about a global count.
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
        // The shared pane-title ink: the muted step, which is legal on the
        // sidebar surface — the one audited surface it reaches by arithmetic
        // rather than by a fourth decision.
        assert_eq!(
            galley.color,
            Palette::INK_3,
            "`{title}` wears the shared pane-title ink"
        );
        // Exactly one rule closes the band this title sits in.
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
        // And the title is centred in the band above it — which is the *shared*
        // band's height, so a title set at a different size, or a rule at the
        // wrong distance, fails here.
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
    // Two sections, two rules — and no more. A third rule anywhere in the rail
    // under these two titles is the nested-boxes failure the shared header
    // exists to prevent, and the count is what says so.
    assert_eq!(
        rail_rules.len(),
        2,
        "the rail's two group sections wear one rule each and nothing else \
         paints a structural hairline in the rail: {rail_rules:?}"
    );
}
