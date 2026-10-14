//! The Branches surface's four grouping and visibility toggles.
//!
//! Four persisted toggles behind a settings gear: `Group by Directory`,
//! `Group by Repository`, `Show Recent Branches`, `Show Tags`
//! (`manage-branches.md:70`, all four in the bundle at GitBundle:1783-1786).
//! They change **grouping and visibility, never data**: no toggle changes what
//! git is asked or what a row's action does, and un-grouping must never shorten
//! the list — that is why the assertions below count rows across a toggle as
//! well as reading the header.
//!
//! The three structural assertions the ticket names are here: the
//! prefix-stripping pair (grouped shows `feature/` and strips the leaves; flat
//! shows the full names and paints no directory header), the frame-invariant
//! `galley_origin` pair (grouping changes the interior, not the header or the
//! first row), and the tags-off pair scoped by the list's own rectangles.

use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{assert_not_painted, galley_origin, painted_galleys, settle_quiet};
use turbogit_app::state::{AppState, Tab};

// --- git fixture -------------------------------------------------------------

// No repository `user.*` here, so every call is pinned per call through the
// fixture crate's runner: a headless machine has no global identity to borrow.

/// A repo on `main` with `feature/a`, `feature/b`, `plain` and two tags, and
/// **no checkout history**: every branch is made with `git branch`, so HEAD's
/// reflog holds no move and the Recent group stays out of the way of the
/// grouping assertions.
fn grouped_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["init", "-q", "-b", "main", alpha.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git_with_env(&alpha, &["add", "."], &GitEnv::fixture());
    git_with_env(&alpha, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
    for b in ["feature/a", "feature/b", "plain"] {
        git_with_env(&alpha, &["branch", b], &GitEnv::fixture());
    }
    git_with_env(&alpha, &["tag", "v1.0"], &GitEnv::fixture());
    git_with_env(&alpha, &["tag", "v2.0"], &GitEnv::fixture());
    (tmp, project)
}

/// Two repositories, each on `main` with one branch of its own and one that
/// **both** repositories have, so a merged list cannot be read without the
/// repository it came from.
fn two_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    for name in ["alpha", "beta"] {
        let dir = project.join(name);
        git_with_env(
            &project,
            &["init", "-q", "-b", "main", dir.to_str().unwrap()],
            &GitEnv::fixture(),
        );
        std::fs::write(dir.join("README.md"), "x\n").unwrap();
        git_with_env(&dir, &["add", "."], &GitEnv::fixture());
        git_with_env(&dir, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
        // `main` already exists — `git init -b main` made it and the commit above
        // landed on it. It is the name both repositories share, which is what
        // this fixture is for; `only-*` is the name neither shares.
        git_with_env(
            &dir,
            &["branch", &format!("only-{name}")] as &[_],
            &GitEnv::fixture(),
        );
    }
    (tmp, project)
}

// --- harness -----------------------------------------------------------------

fn branches_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    let state = AppState::new(project_dir);
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

fn open_branches_tab(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(harness);
}

/// Open the gear's toggle list. The gear is an icon-only button, so its
/// accessible name is the icon's own — the same name `widgets::button_response`
/// gives every icon button, and the name kittest presses it by.
fn open_gear(harness: &mut Harness<'_, AppState>) {
    harness.get_by_label("settings").click();
    settle_quiet(harness);
}

/// Flip one named toggle in the open gear and let the tree re-render.
fn toggle(harness: &mut Harness<'_, AppState>, label: &str) {
    open_gear(harness);
    harness.get_by_label(label).click();
    settle_quiet(harness);
}

/// How many times a string is painted at an exact text (not a substring).
fn painted_count(harness: &Harness<'_, AppState>, text: &str) -> usize {
    painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == text)
        .count()
}

fn has_painted(harness: &Harness<'_, AppState>, text: &str) -> bool {
    painted_count(harness, text) > 0
}

// --- verification 1: directory grouping, both directions ---------------------

#[test]
fn grouped_directory_paints_the_header_and_strips_the_leaf_prefix() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    // The default is grouped, as it was before any of this was togglable.
    assert!(
        has_painted(&harness, "feature/"),
        "the directory header paints its segment with its separator"
    );
    assert!(has_painted(&harness, "a"), "the leaf is stripped");
    assert!(has_painted(&harness, "b"), "the leaf is stripped");
    // The part that regresses: a leaf that re-prints its own prefix.
    for full in ["feature/a", "feature/b"] {
        assert_not_painted(&harness, full);
    }
}

#[test]
fn ungrouped_directory_paints_full_names_and_no_directory_header() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);
    toggle(&mut harness, "Group by Directory");

    assert!(has_painted(&harness, "feature/a"));
    assert!(has_painted(&harness, "feature/b"));
    // Flattened, not truncated: the prefix is the name now, so the header that
    // used to carry it is gone.
    assert_eq!(painted_count(&harness, "feature/"), 0);
    // Never a shorter list: the same three branches, plus `main`.
    for name in ["plain", "main"] {
        assert!(has_painted(&harness, name), "{name} is still listed");
    }
}

// --- verification 2: the frame does not move, only the interior --------------

#[test]
fn directory_toggle_changes_the_rows_between_the_header_and_the_last_row() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    // `main` is checked out, so it leads the Local group either way; the
    // grouping level appears *after* it.
    let header_before = galley_origin(&harness, "LOCAL").expect("LOCAL header");
    let first_before = galley_origin(&harness, "main").expect("first row");
    let last_before = galley_origin(&harness, "b").expect("last row when grouped");

    toggle(&mut harness, "Group by Directory");

    let header_after = galley_origin(&harness, "LOCAL").expect("LOCAL header");
    let first_after = galley_origin(&harness, "main").expect("first row");
    let last_after = galley_origin(&harness, "feature/b").expect("last row when flat");

    assert_eq!(
        header_before, header_after,
        "the group header is not part of what grouping moves"
    );
    assert_eq!(
        first_before, first_after,
        "the first row keeps its origin, whatever the level below it does"
    );
    assert_ne!(
        last_before.y, last_after.y,
        "a directory header fewer lifts the last row — the interior did change"
    );
    assert!(
        last_after.y < last_before.y,
        "flattening shortens the list: {last_after:?} vs {last_before:?}"
    );
}

// --- verification 3: Show Tags, scoped to the list ---------------------------

#[test]
fn show_tags_off_paints_no_tag_row_and_no_tags_header() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    // Both tags exist and the group is open before the toggle; the tags group
    // starts collapsed, so open it the way a user would.
    harness.get_by_label("Tags").click();
    settle_quiet(&mut harness);
    assert!(has_painted(&harness, "TAGS"), "the group is showing");
    assert!(has_painted(&harness, "v1.0"));
    let local_before = galley_origin(&harness, "LOCAL").expect("LOCAL header");

    toggle(&mut harness, "Show Tags");

    // Scoped by the list's own geometry: `Tags` is a word that appears on this
    // surface as a menu item and nowhere else as a header, and a tag name can
    // appear in a commit row — so assert the painted header and the painted row
    // are gone from where the list paints, not merely absent from a substring.
    let galleys = painted_galleys(&harness);
    let in_list = |t: &str| {
        galleys
            .iter()
            .any(|g| g.text == t && g.rect.min.x > 220.0 && g.rect.min.x < 900.0)
    };
    assert!(!in_list("TAGS"), "no Tags header in the list");
    assert!(!in_list("v1.0"), "no tag row in the list");
    assert!(!in_list("v2.0"), "no tag row in the list");
    assert_eq!(
        galley_origin(&harness, "LOCAL"),
        Some(local_before),
        "the Local group is untouched by hiding Tags"
    );
}

// --- the gear itself, and what it says ---------------------------------------

#[test]
fn the_gear_exposes_the_four_documented_toggles() {
    let (_tmp, project) = grouped_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);
    open_gear(&mut harness);

    // The reference's own wording, from the bundle keys.
    for label in [
        "Group by Directory",
        "Group by Repository",
        "Show Recent Branches",
        "Show Tags",
    ] {
        assert!(
            has_painted(&harness, label),
            "{label} is one of the four: painted {:?}",
            painted_galleys(&harness)
                .into_iter()
                .map(|g| g.text)
                .collect::<Vec<_>>()
        );
    }
}

// --- Show Recent Branches ----------------------------------------------------

#[test]
fn show_recent_off_hides_the_group_the_other_ticket_built() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["init", "-q", "-b", "main", alpha.to_str().unwrap()],
        &GitEnv::fixture(),
    );
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git_with_env(&alpha, &["add", "."], &GitEnv::fixture());
    git_with_env(&alpha, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
    git_with_env(&alpha, &["branch", "work"], &GitEnv::fixture());
    git_with_env(&alpha, &["checkout", "-q", "work"], &GitEnv::fixture());

    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);
    assert!(has_painted(&harness, "RECENT"), "the group is on");

    toggle(&mut harness, "Show Recent Branches");
    assert_eq!(painted_count(&harness, "RECENT"), 0);
    // Visibility, not data: the branch is still in Local.
    assert!(has_painted(&harness, "work"));
}

// --- Group by Repository -----------------------------------------------------

#[test]
fn repository_grouping_off_removes_the_level_without_losing_a_branch() {
    let (_tmp, project) = two_repo_project();
    let mut harness = branches_harness(project);
    open_branches_tab(&mut harness);

    // Two repositories in scope: one Local group per section while grouped.
    assert_eq!(
        painted_count(&harness, "LOCAL"),
        2,
        "each repository is its own grouping level"
    );
    for name in ["only-alpha", "only-beta"] {
        assert!(has_painted(&harness, name), "{name} is listed");
    }

    toggle(&mut harness, "Group by Repository");

    assert_eq!(
        painted_count(&harness, "LOCAL"),
        1,
        "one Local group over both repositories"
    );
    // The differentiator survives the toggle: both repositories' branches are
    // still here, so the toggle never shortened the list.
    for name in ["only-alpha", "only-beta"] {
        assert!(has_painted(&harness, name), "{name} is still listed");
    }
    // And a row that could belong to either repository says which.
    assert!(
        has_painted(&harness, "main · alpha") || has_painted(&harness, "main · beta"),
        "an un-grouped list still names the repository a row came from"
    );
}

// --- persistence -------------------------------------------------------------

#[test]
fn a_toggled_grouping_comes_back_from_the_project_on_the_next_launch() {
    let (tmp, project) = grouped_project();
    {
        let mut harness = branches_harness(project.clone());
        open_branches_tab(&mut harness);
        toggle(&mut harness, "Group by Directory");
        assert!(has_painted(&harness, "feature/a"));
        // The harness's own state is the only writer; the file is what outlives it.
        harness.state().save_durable_ui();
    }
    let persisted = std::fs::read_to_string(project.join(".turbogit").join("ui.ron"))
        .expect("ui.ron holds the toggles");
    assert!(
        persisted.contains("group_by_directory: false"),
        "the flip is written down, not just painted: {persisted}"
    );

    let mut again = branches_harness(project);
    open_branches_tab(&mut again);
    assert!(
        has_painted(&again, "feature/a"),
        "a fresh launch reads the preference back"
    );
    assert_eq!(painted_count(&again, "feature/"), 0);
    drop(tmp);
}
