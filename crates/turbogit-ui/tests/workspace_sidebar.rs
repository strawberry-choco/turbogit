//! Issue #05 — Workspace tree sidebar.
//!
//! The left rail lists every discovered repository root as a tree of
//! projects → repos: a workspace header with the total repo count, a
//! filter field, collapsible project groups with aggregate counts, and one
//! row per repo with a status dot, branch label, and ahead/behind badges.
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
        .nth(3)
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

// -- Cycle A — the tree paints: workspace header, groups, repo rows --

#[test]
fn sidebar_paints_workspace_header_groups_and_repo_rows() {
    let (project, alpha, lib) = two_group_project("paint");
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    // Workspace header: project basename + the total repo count badge.
    assert_painted(&h, "wsb");
    assert_galley(&h, "2");
    // Filter field with its placeholder hint.
    assert_painted(&h, "Filter repos / branches…");
    // Section title.
    assert_painted(&h, "PROJECTS");
    // Group headers.
    assert_painted(&h, "frontend");
    assert_painted(&h, "oss");
    // Repo rows with branch labels.
    assert_painted(&h, "alpha");
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

    // The section header and every non-empty built-in group…
    assert_painted(&h, "SMART GROUPS");
    assert_painted(&h, "diverged");
    assert_painted(&h, "has conflicts");
    assert_painted(&h, "unpushed commits");
    assert_painted(&h, "dirty worktree");
    // …each currently holding exactly one member (four count badges of 1).
    assert_galley(&h, "1");
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
    let texts = painted_text(&h);
    assert!(
        !texts.iter().any(|t| t == "lib"),
        "non-member repo rows must drop out while filtered: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "extra"),
        "non-member repo rows must drop out while filtered: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "oss"),
        "the memberless oss group header must drop out: {texts:?}"
    );

    // Switching to another group re-filters: dirty worktree keeps only lib
    // (the status bar still paints the focused repo alpha, so the tree-level
    // signal is the frontend group header dropping out).
    h.get_by_label("dirty worktree").click();
    settle(&mut h);
    assert_eq!(
        h.state().ui.sidebar_smart_group,
        Some("dirty worktree".into())
    );
    assert_painted(&h, "lib");
    let texts = painted_text(&h);
    assert!(
        !texts.iter().any(|t| t == "frontend"),
        "the memberless frontend group header must drop out: {texts:?}"
    );

    // Clicking the active group again clears the filter: the whole tree
    // comes back.
    h.get_by_label("dirty worktree").click();
    settle(&mut h);
    assert_eq!(h.state().ui.sidebar_smart_group, None);
    assert_painted(&h, "alpha");
    assert_painted(&h, "lib");
    assert_galley(&h, "oss");
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
fn clicking_repo_row_focuses_it_in_header_and_metadata_rail() {
    let (project, alpha, lib) = two_group_project("focus");
    let state = app_state(&project, &[alpha.clone(), lib.clone()]);
    let mut h = harness(state);
    settle(&mut h);

    // `for_roots` focuses the first registered root (alpha); its metadata
    // rail paints alpha's path.
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
    // repo name, and the metadata rail repaints its path.
    assert_painted(&h, "oss/lib");
}

// -- Cycle C — group collapse / expand --

#[test]
fn group_header_toggles_collapse_and_expand() {
    let (project, alpha, lib) = two_group_project("collapse");
    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    // Collapse the `oss` group: its rows disappear (lib is not the focused
    // root, so nothing else repaints its name) and the key is recorded.
    h.get_by_label("oss").click();
    settle(&mut h);
    assert_not_painted(&h, "lib");
    assert!(h.state().ui.sidebar_collapsed.contains("oss"));

    // Expand again: the row returns and the key is cleared.
    h.get_by_label("oss").click();
    settle(&mut h);
    assert_painted(&h, "lib");
    assert!(!h.state().ui.sidebar_collapsed.contains("oss"));
}

// -- Cycle D — the filter narrows the tree live --

#[test]
fn filter_input_narrows_the_tree_live() {
    let (project, alpha, lib) = two_group_project("filter");
    let state = app_state(&project, &[alpha, lib]);
    let mut h = harness(state);
    settle(&mut h);

    let search = h.get_by_label("Filter repos / branches…");
    search.focus();
    search.type_text("lib");
    settle(&mut h);

    // Public state carries the query; the tree narrowed live: the oss
    // group survives with lib, the frontend group drops out entirely.
    // (The breadcrumb still paints "frontend/alpha" for the focused root,
    // so group assertions use exact galleys.)
    assert_eq!(h.state().ui.sidebar_filter, "lib");
    assert_galley(&h, "oss");
    assert_painted(&h, "lib");
    let texts = painted_text(&h);
    assert!(
        !texts.iter().any(|t| t == "frontend"),
        "frontend group header must drop out while filtered: {texts:?}"
    );

    // Clearing the query restores the whole tree.
    h.state_mut().ui.sidebar_filter.clear();
    settle(&mut h);
    assert_galley(&h, "frontend");
    assert_painted(&h, "alpha");
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
