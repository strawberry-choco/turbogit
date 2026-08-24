//! Issue #14 — Branches popup: groups, row actions, keyboard flow.
//!
//! Headless egui_kittest harness (same pattern as `redesign_harness.rs`)
//! driving [`turbogit::ui::render`] over a **real temp git repository**.
//! Asserts only on public surfaces: painted text/geometry and public
//! `AppState` transitions.
//!
//! Covered (spec §8.5, ADR-0012):
//! - RECENT / LOCAL / REMOTE / TAGS groups render with correct members
//! - current branch pinned top of LOCAL, check-marked and emphasized
//! - checkout via row click AND via Enter on the highlighted row
//! - New Branch… flow creates + checks out a branch
//! - Rename / Delete / Compare… / New Worktree… render inert (no-op)
//! - starred rows sort first; star uses the STATE_WARNING token
//! - multi-root sync notice states the N repositories count
//! - Esc closes; typing filters live

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::{assert_not_painted, assert_painted, galley_origin, painted_text};
use egui::Key;
use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use turbogit::state::{AppState, Dialog};
use turbogit::theme::Palette;
use turbogit::ui::branch_widget;

// --- git fixture -------------------------------------------------------------

/// Run `git <args>` in `dir`, panicking on failure. Identity comes from env so
/// no per-repo config calls are needed.
fn git(dir: &Path, args: &[&str]) {
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
}

fn commit_readme(repo: &Path) {
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "init"]);
}

/// A project dir holding one real repo `alpha` on `main` with local branches
/// `feature-a`, `feature-b`, `zebra`, tag `v1.0`, and a bare remote `origin`
/// that carries a remote-only branch (`remote-only`) fetched locally.
fn single_repo_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    commit_readme(&alpha);
    for b in ["feature-a", "feature-b", "zebra"] {
        git(&alpha, &["branch", b]);
    }
    git(&alpha, &["tag", "v1.0"]);

    // Bare remote with a branch that exists only remotely.
    git(&project, &["clone", "--bare", "alpha", "origin.git"]);
    git(&alpha, &["remote", "add", "origin", "../origin.git"]);
    git(&alpha, &["push", "origin", "main:remote-only"]);
    git(&alpha, &["fetch", "origin"]);
    (tmp, project)
}

/// Like [`single_repo_project`] plus a second root `beta` (multi-root).
fn two_repo_project() -> (TempDir, PathBuf) {
    let (tmp, project) = single_repo_project();
    let beta = project.join("beta");
    git(&project, &["-c", "init.defaultBranch=main", "init", "beta"]);
    commit_readme(&beta);
    (tmp, project)
}

// --- harness -----------------------------------------------------------------

/// Harness over a real repo-backed project. Setup mirrors production app.rs:
/// event pump first, then dark-only tokens + embedded fonts once, then render.
fn branches_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    let state = AppState::new(project_dir);
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            state.drain_events();
            turbogit::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 768.0));
    harness
}

/// Step frames until painted output is stable for 3 consecutive frames
/// (tolerates slow background git subprocesses, unlike a fixed frame count).
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

/// Step frames until `pred` holds on public state (async op completion).
fn pump_until(harness: &mut Harness<'_, AppState>, what: &str, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..600 {
        if pred(harness.state()) {
            return;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    let s = harness.state();
    panic!(
        "timed out waiting for: {what}\n  last_error={:?}\n  busy={} toast={:?} popup={}\n  painted={:?}",
        s.last_error,
        s.ui.busy,
        s.ui.toast,
        s.ui.branches_popup,
        common::painted_text(harness),
    );
}

fn open_popup(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.branches_popup = true;
    settle_quiet(harness);
}

fn set_favorite(harness: &mut Harness<'_, AppState>, name: &str) {
    let st = harness.state_mut();
    let id = st.selected_root.clone().expect("selected root");
    for r in st.multi.roots.iter_mut() {
        if r.id == id {
            for b in r.branches.iter_mut() {
                if b.name == name {
                    b.favorite = true;
                }
            }
        }
    }
}

// --- Cycle 1: groups ----------------------------------------------------------

#[test]
fn popup_groups_render_with_members() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);

    // Seed one recent branch so all four groups have members.
    harness.state_mut().ui.recent_branches = vec!["feature-b".into()];
    open_popup(&mut harness);

    for group in ["RECENT", "LOCAL", "REMOTE", "TAGS"] {
        assert_painted(&harness, group);
    }
    for member in [
        "feature-b",
        "feature-a",
        "zebra",
        "origin/remote-only",
        "v1.0",
    ] {
        assert_painted(&harness, member);
    }
}

#[test]
fn current_branch_pinned_top_and_checkmarked() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_popup(&mut harness);

    // Check-marked pinned row…
    assert_painted(&harness, "✓ main");
    let pinned = galley_origin(&harness, "✓ main").expect("pinned current row painted");
    // …sits above the LOCAL heading and every other listed row.
    let local_title = galley_origin(&harness, "LOCAL").expect("LOCAL group title painted");
    let feature_a = galley_origin(&harness, "feature-a").expect("local row painted");
    assert!(
        pinned.y < local_title.y && pinned.y < feature_a.y,
        "pinned row ({:?}) must render above LOCAL ({:?}) and members ({:?})",
        pinned,
        local_title,
        feature_a
    );
}

// --- Cycle 2: wired actions ----------------------------------------------------

#[test]
fn checkout_via_click_switches_branch() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_popup(&mut harness);

    harness.get_by_label("feature-a").click();
    pump_until(&mut harness, "checkout via click", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.current_branch.clone())
            .as_deref()
            == Some("feature-a")
    });
    assert!(
        !harness.state().ui.branches_popup,
        "popup must close after checkout"
    );
}

#[test]
fn checkout_via_enter_checks_out_highlighted_row() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_popup(&mut harness);

    // Cursor defaults to the first selectable row (feature-a: alphabetically
    // first local below the pinned current branch, no favorites seeded).
    harness.key_press(Key::Enter);
    pump_until(&mut harness, "checkout via Enter", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.current_branch.clone())
            .as_deref()
            == Some("feature-a")
    });
    assert!(
        !harness.state().ui.branches_popup,
        "popup must close after Enter-checkout"
    );
}

#[test]
fn new_branch_flow_creates_and_checks_out() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_popup(&mut harness);

    harness.get_by_label("New Branch…").click();
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::NewBranch),
        "New Branch… must open the New Branch dialog"
    );

    // Type into the dialog's first input (Name:).
    {
        let name_field = harness
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .next()
            .expect("dialog Name input queryable");
        name_field.focus();
        name_field.type_text("issue14-x");
    }
    settle_quiet(&mut harness);
    harness.get_by_label("Create").click();
    pump_until(&mut harness, "branch created and checked out", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .map(|r| {
                r.current_branch.as_deref() == Some("issue14-x")
                    && r.branches.iter().any(|b| b.name == "issue14-x")
            })
            .unwrap_or(false)
    });
}

// --- Cycle 3: inert actions (ADR-0012) -----------------------------------------

#[test]
fn inert_actions_render_but_do_nothing() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_popup(&mut harness);

    // Filter to exactly one row so action labels are unambiguous: the
    // remote-only branch exists on no local ref, so nothing else matches.
    {
        let search = harness.get_by_label("Search branches");
        search.focus();
        search.type_text("remote-only");
    }
    settle_quiet(&mut harness);
    for label in ["Checkout", "Rename", "Delete", "Compare…", "New Worktree…"] {
        assert_painted(&harness, label);
    }

    let snapshot = |h: &Harness<'_, AppState>| {
        let s = h.state();
        (
            s.ui.dialog,
            s.ui.confirm.is_some(),
            s.ui.diff.is_some(),
            s.ui.busy,
            s.multi.roots[0]
                .branches
                .iter()
                .map(|b| b.name.clone())
                .collect::<Vec<_>>(),
        )
    };
    let before = snapshot(&harness);

    for label in ["Rename", "Delete", "Compare…", "New Worktree…"] {
        harness.get_by_label(label).click();
        settle_quiet(&mut harness);
        assert_eq!(
            snapshot(&harness),
            before,
            "clicking inert `{label}` must change nothing"
        );
    }
}

// --- Cycle 4: stars -------------------------------------------------------------

#[test]
fn starred_rows_sort_first_and_star_uses_warning_token() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    set_favorite(&mut harness, "zebra");
    open_popup(&mut harness);

    assert_painted(&harness, "★");
    let zebra = galley_origin(&harness, "zebra").expect("favorite row painted");
    let feature_a = galley_origin(&harness, "feature-a").expect("plain row painted");
    assert!(
        zebra.y < feature_a.y,
        "starred rows must sort above plain rows within LOCAL"
    );
    // Token contract: the star color is the central warning token.
    assert_eq!(branch_widget::STAR_COLOR, Palette::STATE_WARNING);
}

// --- Cycle 5: keyboard ------------------------------------------------------------

#[test]
fn esc_closes_popup() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_popup(&mut harness);

    harness.key_press(Key::Escape);
    settle_quiet(&mut harness);

    assert!(
        !harness.state().ui.branches_popup,
        "Esc must close the popup"
    );
    assert_not_painted(&harness, "Search branches");
}

#[test]
fn typing_filters_live() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    open_popup(&mut harness);

    // No Enter pressed — the list narrows as characters land.
    {
        let search = harness.get_by_label("Search branches");
        search.focus();
        search.type_text("feat");
    }
    settle_quiet(&mut harness);

    assert_painted(&harness, "feature-a");
    assert_not_painted(&harness, "zebra");
    assert_not_painted(&harness, "v1.0");
}

// --- Cycle 6: multi-root sync notice ----------------------------------------------

#[test]
fn multi_root_sync_notice_states_n_repositories() {
    let (_project, dir) = two_repo_project();
    let mut harness = branches_harness(dir);
    assert_eq!(
        harness.state().multi.roots.len(),
        2,
        "fixture needs two roots"
    );

    harness.state_mut().settings.synchronous_branches = true;
    open_popup(&mut harness);

    assert_painted(
        &harness,
        "Synchronous branch operations across 2 repositories",
    );
}

// --- Pure logic (no harness): ordering, filtering, recents --------------------------

mod pure {
    use turbogit::model::{Branch, BranchKind};
    use turbogit::ui::branch_widget::{popup_entries, push_recent};

    fn b(name: &str, kind: BranchKind, favorite: bool) -> Branch {
        Branch {
            name: name.into(),
            kind,
            tracking: None,
            favorite,
            protected: false,
            exists: true,
        }
    }

    #[test]
    fn entries_order_groups_and_favorite_first() {
        let locals = [
            b("zeta", BranchKind::Local, true),
            b("alpha", BranchKind::Local, false),
            b("main", BranchKind::Local, false),
        ];
        let remotes = [b("only", BranchKind::Remote, false)];
        let tags = vec!["v2".to_string(), "v1".to_string()];
        let rows = popup_entries(&locals, &remotes, &tags, &[], None, "");
        let names: Vec<String> = rows.iter().map(|r| r.label()).collect();

        // Locals favorites-first then alphabetical, then remotes, then tags.
        assert_eq!(
            names,
            vec!["zeta", "alpha", "main", "origin/only", "v1", "v2"]
        );
    }

    #[test]
    fn recent_rows_render_first_and_starred_first() {
        let locals = [
            b("a", BranchKind::Local, true),
            b("b", BranchKind::Local, false),
        ];
        let rows = popup_entries(
            &locals,
            &[],
            &[],
            &["b".to_string(), "a".to_string()],
            None,
            "",
        );
        let names: Vec<String> = rows.iter().map(|r| r.label()).collect();
        // RECENT group leads, starred ("a") before unstarred ("b"); the same
        // branches still list under LOCAL afterwards.
        assert_eq!(
            &names[..4],
            &[
                "a".to_string(),
                "b".to_string(),
                "a".to_string(),
                "b".to_string()
            ]
        );
    }

    #[test]
    fn filter_matches_display_names_and_current_is_excluded_from_locals() {
        let locals = [
            b("main", BranchKind::Local, false),
            b("feature", BranchKind::Local, false),
        ];
        // Current branch is pinned separately, so it never repeats under LOCAL.
        let rows = popup_entries(&locals, &[], &[], &[], Some("main"), "");
        let names: Vec<String> = rows.iter().map(|r| r.label()).collect();
        assert_eq!(names, vec!["feature"]);

        // Live filter matches display names.
        let rows = popup_entries(&locals, &[], &[], &[], Some("main"), "feat");
        let names: Vec<String> = rows.iter().map(|r| r.label()).collect();
        assert_eq!(names, vec!["feature"]);
    }

    #[test]
    fn push_recent_is_deduped_and_capped_at_five() {
        let mut recents = vec![];
        for n in ["a", "b", "c", "d", "e", "f"] {
            push_recent(&mut recents, n);
        }
        push_recent(&mut recents, "c");
        assert_eq!(recents, vec!["c", "f", "e", "d", "b"]);
    }
}
