//! The Branches tab's `Recent` group, capped at five.
//!
//! Recent is a view-level group above `Local` whose members are the five most
//! recently checked-out branches, derived from the reflog (the reference's own
//! mechanism) — not from anything the app bookkept — so
//! recents survive restarts and reflect checkouts made outside the app. The
//! cap is the documented one ("Recent branches node shows up to five recently
//! checked-out branches", manage-branches.md:61); no overflow affordance is
//! built beyond it (the escape is `UNVERIFIED`, so cap and stop), and the
//! under-cap case paints no empty-state or overflow text.
//!
//! The three structural assertions the ticket names are the ones here:
//! position-counted rows around the `Local` header, `galley_origin` ordering,
//! and the under-cap shape.

use std::path::PathBuf;

use egui_kittest::{Harness, kittest::NodeT as _, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{assert_not_painted, galley_origin, painted_galleys, settle_quiet};
use turbogit_app::state::{AppState, Tab};

// --- git fixture -------------------------------------------------------------

// No repository `user.*` here, so every call is pinned per call through the
// fixture crate's runner: a headless machine has no global identity to borrow.

/// A repo `alpha` on `main` with `count` branches `r1..rN` that were checked
/// out **in order** (r1 first), so the reflog answers newest-first: rN is the
/// most recent checkout. Branch names keep the reflog order unambiguous.
fn checked_out_project(count: usize) -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git_with_env(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
        &GitEnv::fixture(),
    );
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git_with_env(&alpha, &["add", "."], &GitEnv::fixture());
    git_with_env(&alpha, &["commit", "-m", "init"], &GitEnv::fixture());
    for i in 1..=count {
        git_with_env(&alpha, &["branch", &format!("r{i}")], &GitEnv::fixture());
    }
    for i in 1..=count {
        git_with_env(
            &alpha,
            &["checkout", "-q", &format!("r{i}")],
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

/// The fixture plus an opened Branches tab; the `TempDir` rides along because
/// dropping it deletes the repository every later read answers from.
fn open_checked_out(count: usize) -> (TempDir, Harness<'static, AppState>) {
    let (tmp, dir) = checked_out_project(count);
    let mut harness = branches_harness(dir);
    open_branches_tab(&mut harness);
    (tmp, harness)
}

/// The Branches tab's own header nodes, plus every row node painted for the
/// named branch. A branch checked out recently paints **twice** — once in the
/// Recent group and once in Local — so callers assert on the set of rects, not
/// on the first match.
fn row_rects(harness: &Harness<'_, AppState>, name: &str) -> Vec<egui::Rect> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| n.accesskit_node().label() == Some(name.to_string()))
        .map(|n| n.rect())
        .collect()
}

fn header_rect(harness: &Harness<'_, AppState>, label: &str) -> egui::Rect {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(label.to_string()))
        .unwrap_or_else(|| panic!("no {label} header node"))
        .rect()
}

// --- ticket verification -------------------------------------------------------

/// Verification 1 — with seven branches checked out across time, the Branches
/// tab paints exactly one `Recent` group header and exactly **five** branch
/// rows above the `Local` group, counted by position: the fifth row's rect is
/// above the `Local` header's and the sixth branch appears only below it.
#[test]
fn recent_group_caps_at_five_rows_above_local() {
    let (_tmp, harness) = open_checked_out(7);

    // Exactly one Recent group header (the uppercase section label).
    let recent_headers = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.text == "RECENT")
        .count();
    assert_eq!(recent_headers, 1, "exactly one Recent group header");

    // The cap: r7..r3 (newest first) each have a row ABOVE the Local header —
    // their Recent copy — and r2, r1 have none.
    let local = header_rect(&harness, "Local");
    let mut prev_top: Option<f32> = None;
    let mut prev_name = "";
    for (i, name) in ["r7", "r6", "r5", "r4", "r3"].iter().enumerate() {
        let rects = row_rects(&harness, name);
        assert_eq!(
            rects.len(),
            2,
            "{name} paints in Recent and in Local: {rects:?}"
        );
        let above = rects.iter().filter(|r| r.max.y <= local.min.y).count();
        assert_eq!(above, 1, "{name} (recent slot {i}) has one row above Local");
        // Recency order within the group: newer rows sit higher. Compared on
        // this name's own row rects, not a galley search: branch names also
        // paint outside this tab (the sidebar's current-branch label), and a
        // whole-window galley match would order against that.
        let row = rects.iter().find(|r| r.max.y <= local.min.y).unwrap();
        if let Some(prev_top) = prev_top {
            assert!(
                prev_top < row.top(),
                "{prev_name} (checked out more recently) must sit above {name}"
            );
        }
        prev_top = Some(row.top());
        prev_name = name;
    }
    for name in ["r2", "r1"] {
        let rects = row_rects(&harness, name);
        assert!(
            rects.iter().all(|r| r.min.y >= local.max.y),
            "{name} was never among the five most recent; every copy must be \
             below the Local header: {rects:?}"
        );
    }
}

/// Verification 2 — the `Recent` group's header galley origin sits above the
/// `Local` group's, separated by at least one row height. `painted_text`
/// proves presence; this proves position.
#[test]
fn recent_header_sits_above_local_by_at_least_one_row() {
    let (_tmp, harness) = open_checked_out(7);
    let recent = galley_origin(&harness, "RECENT").expect("Recent header painted");
    let local = galley_origin(&harness, "LOCAL").expect("Local header painted");
    assert!(recent.y < local.y, "Recent must sit above Local");
    // One 30px branch row separates them at minimum (five rows actually sit
    // between, but the ticket's bound is the weaker one).
    assert!(
        local.y - recent.y >= 30.0,
        "at least one row height between the two headers: {recent:?} vs {local:?}"
    );
}

/// Verification 3 — with exactly three branches ever checked out, the Recent
/// group paints three rows and **no** empty-state or overflow text. The
/// under-cap case is what regresses when the cap is implemented as "always
/// reserve five slots".
#[test]
fn under_cap_recent_paints_its_rows_and_no_overflow() {
    let (_tmp, harness) = open_checked_out(3);
    let local = header_rect(&harness, "Local");
    for name in ["r3", "r2", "r1"] {
        let rects = row_rects(&harness, name);
        assert_eq!(rects.len(), 2, "{name} paints in Recent and in Local");
        assert!(
            rects.iter().any(|r| r.max.y <= local.min.y),
            "{name} has its Recent row above the Local header: {rects:?}"
        );
    }
    // Cap-and-stop: no overflow affordance is painted speculatively, and an
    // under-cap group reserves no empty slots.
    for overflow in ["Show all", "show all", "older", "more…"] {
        assert_not_painted(&harness, overflow);
    }
}
