//! Issue #14 — Branches popup: groups, row actions, keyboard flow.
//!
//! Headless egui_kittest harness (same pattern as `shell_frame.rs`)
//! driving [`turbogit_ui::ui::render`] over a **real temp git repository**.
//! Asserts only on public surfaces: painted text/geometry and public
//! `AppState` transitions.
//! Covered (spec §8.5, ADR-0012):
//! - RECENT / LOCAL / REMOTE / TAGS groups render with correct members
//! - current branch pinned top of LOCAL, check-marked and emphasized
//! - checkout via row click AND via Enter on the highlighted row
//! - New Branch… flow creates + checks out a branch
//! - Rename / Delete / Compare… / New Worktree… render inert (no-op)
//! - starred rows sort first; star uses the STATE_WARNING token
//! - multi-root sync notice states the N repositories count
//! - Esc closes; typing filters live

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use egui::Key;
use egui_kittest::{Harness, kittest::NodeT as _, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::harness::{assert_not_painted, assert_painted, galley_origin, painted_text};
use turbogit_app::state::{AppState, Dialog};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::branch_widget;

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
        painted_text(harness),
    );
}

fn open_popup(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.branches_popup = true;
    settle_quiet(harness);
}

/// The recents store behind the RECENT group is a global config file
/// (ADR-0005), so earlier runs leak into a fresh harness. Tests that need a
/// stable row set clear it before opening the popup.
fn clear_recents(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.recent_branches.clear();
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

    // Type into the dialog's first input (Name:). The Commit window's file
    // filter (spec R7) also matches Role::TextInput but carries an
    // accessible label ("Filter files"); the dialog's raw fields are
    // unlabeled.
    {
        let name_field = harness
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .find(|n| n.accesskit_node().label().is_none())
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

// --- Cycle 3: row action gating (issue 32 supersedes ADR-0012 inertness) ---

#[test]
fn row_actions_render_per_gating_and_delete_dispatches() {
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

    // Remote rows: check out, delete (remote ref), compare — Rename is a
    // local-branch verb, so it stays rendered but disabled.
    for label in ["Checkout", "Rename", "Delete", "Compare…", "New Worktree…"] {
        assert_painted(&harness, label);
    }
    assert!(
        harness
            .get_by_label("Rename")
            .accesskit_node()
            .is_disabled(),
        "Rename on a remote row must be disabled"
    );
    assert!(
        !harness
            .get_by_label("Delete")
            .accesskit_node()
            .is_disabled(),
        "Delete on a remote row must stay enabled"
    );

    // Delete dispatches the remote-branch confirmation (issue 32): the
    // rich confirmation from issue 02, not a no-op.
    harness.get_by_label("Delete").click();
    settle_quiet(&mut harness);
    assert!(
        harness.state().ui.confirm.is_some(),
        "Delete must open the confirmation flow"
    );
    assert_painted(&harness, "Delete remote branch");
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

// --- Cycle 7: issue 32 row actions end to end ---------------------------------

/// Issue 32 sync fixture: `alpha` on `main` (c1, 21 days old) with `feat`
/// tracking `origin/main` (ahead 2, behind 1), `ghost` tracking a pruned
/// `origin/ghost` (gone), and untracked `zebra` sitting on the old commit
/// (stale badge "3w").
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

    // ghost: tracked upstream that will be deleted and pruned.
    git(&alpha, &["branch", "ghost"]);
    git(&alpha, &["push", "-q", "-u", "origin", "ghost"]);
    git(&alpha, &["fetch", "-q", "origin"]);
    // zebra: stale untracked branch on the old commit.
    git(&alpha, &["branch", "zebra"]);

    // feat: two fresh commits past the fork; main advances one fresh commit.
    git(&alpha, &["branch", "feat"]);
    git(&alpha, &["branch", "--set-upstream-to=origin/main", "feat"]);
    git(&alpha, &["checkout", "-q", "feat"]);
    commit_pinned(&alpha, "c2", now);
    commit_pinned(&alpha, "c3", now);
    git(&alpha, &["checkout", "-q", "main"]);
    commit_pinned(&alpha, "c4", now);
    git(&alpha, &["push", "-q", "origin", "main"]);

    // Delete the remote ghost branch and prune its tracking ref: the local
    // ghost branch still tracks it, so the popup shows `origin/ghost  gone`.
    git(
        &project.join("origin.git"),
        &["update-ref", "-d", "refs/heads/ghost"],
    );
    git(&alpha, &["fetch", "-q", "--prune", "origin"]);
    (tmp, project)
}

/// Commit with both author and committer dates pinned (git internal format);
/// each commit touches a file so no commit is empty.
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
fn stale_badge_remote_arrows_and_gone_render() {
    let (_project, dir) = sync_repo_project();
    let mut harness = branches_harness(dir);
    clear_recents(&mut harness);
    open_popup(&mut harness);

    // Stale badge: zebra (and ghost) sit on the 3-week-old commit.
    assert_painted(&harness, "3w");
    // Remote rows: origin/main diverged from feat (↑2 ↓1), origin/ghost
    // synthesized from the local tracking with its upstream pruned (gone).
    assert_painted(&harness, "↑2 ↓1");
    assert_painted(&harness, "gone");
    assert_painted(&harness, "origin/ghost");
    // Fresh branches carry the "now" badge, not a stale one.
    assert_painted(&harness, "now");
}

#[test]
fn protected_rows_render_lock_gate_delete_and_offer_hover_actions() {
    let (_project, dir) = sync_repo_project();
    let mut harness = branches_harness(dir);
    harness.state_mut().settings.protected_branch_patterns = vec!["zebra".into()];
    clear_recents(&mut harness);
    open_popup(&mut harness);

    // Narrow to the protected zebra row: its destructive action is gated
    // (no Delete offered), but checkout stays.
    {
        let search = harness.get_by_label("Search branches");
        search.focus();
        search.type_text("zebra");
    }
    settle_quiet(&mut harness);
    assert_painted(&harness, "zebra");
    assert_painted(&harness, "Checkout");
    // The destructive action stays rendered but gated (issue 32).
    assert_painted(&harness, "Delete");
    assert!(
        harness
            .get_by_label("Delete")
            .accesskit_node()
            .is_disabled(),
        "Delete on a protected branch must be gated"
    );
    assert!(
        !harness
            .get_by_label("Checkout")
            .accesskit_node()
            .is_disabled(),
        "checkout stays available on a protected branch"
    );

    // Hover reveals the pull / merge quick actions.
    harness.get_by_label("zebra").hover();
    settle_quiet(&mut harness);
    assert_painted(&harness, "Pull");
    assert_painted(&harness, "Merge");

    // Merge is wired: it presets the existing merge dialog with the protected
    // branch as the source and closes the popup.
    harness.get_by_label("Merge").click();
    settle_quiet(&mut harness);
    assert_eq!(harness.state().ui.dialog, Some(Dialog::Merge));
    assert_eq!(harness.state().ui.dlg.merge_target, "zebra");
    assert!(!harness.state().ui.branches_popup);
}

#[test]
fn delete_flow_confirms_and_refreshes_the_list() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    clear_recents(&mut harness);
    open_popup(&mut harness);

    {
        let search = harness.get_by_label("Search branches");
        search.focus();
        search.type_text("feature-a");
    }
    settle_quiet(&mut harness);
    // The filter also matches the remote-tracking `origin/feature-a` row the
    // fixture's bare clone created; LOCAL rows precede REMOTE ones, so the
    // first Delete is the local branch's.
    let deletes: Vec<_> = harness.get_all_by_label("Delete").collect();
    deletes[0].click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "Delete local branch 'feature-a'");

    harness.get_by_label("OK").click();
    // The remote-tracking `origin/feature-a` row the fixture's bare clone
    // created survives; only the local branch must be gone.
    pump_until(&mut harness, "branch deleted", |s| {
        !s.multi.roots[0]
            .branches
            .iter()
            .any(|b| b.kind == turbogit_domain::model::BranchKind::Local && b.name == "feature-a")
    });
}

#[test]
fn rename_flow_renames_through_the_dialog() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    clear_recents(&mut harness);
    open_popup(&mut harness);

    {
        let search = harness.get_by_label("Search branches");
        search.focus();
        search.type_text("feature-b");
    }
    settle_quiet(&mut harness);
    // The filter also matches the remote-tracking `origin/feature-b` row the
    // fixture's bare clone created (its Rename renders disabled); LOCAL rows
    // precede REMOTE ones, so the first Rename is the local branch's.
    let renames: Vec<_> = harness.get_all_by_label("Rename").collect();
    renames[0].click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "Rename 'feature-b' to:");

    {
        let field = harness
            .get_all_by_role(egui::accesskit::Role::TextInput)
            .find(|n| n.accesskit_node().label().is_none())
            .expect("rename input queryable");
        field.focus();
        field.type_text("renamed-x");
    }
    settle_quiet(&mut harness);
    harness.get_by_label("Rename").click();
    pump_until(&mut harness, "branch renamed", |s| {
        s.multi.roots[0]
            .branches
            .iter()
            .any(|b| b.name == "renamed-x")
    });
    let names: Vec<String> = harness.state().multi.roots[0]
        .branches
        .iter()
        .filter(|b| b.kind == turbogit_domain::model::BranchKind::Local)
        .map(|b| b.name.clone())
        .collect();
    assert!(
        !names.iter().any(|n| n == "feature-b"),
        "the old local name must be gone after rename; locals = {names:?}"
    );
}

#[test]
fn compare_flow_lists_commits_and_swaps() {
    let (_project, dir) = sync_repo_project();
    let mut harness = branches_harness(dir);
    clear_recents(&mut harness);
    open_popup(&mut harness);

    {
        let search = harness.get_by_label("Search branches");
        search.focus();
        search.type_text("feat");
    }
    settle_quiet(&mut harness);
    // Row action first ("Compare…" also lives in the popup footer).
    let rows: Vec<_> = harness.get_all_by_label("Compare…").collect();
    rows[0].click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "Compare feat with main");
    assert_painted(&harness, "2 commit(s) in 'feat' missing from 'main'");

    harness.get_by_label("Swap Branches").click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "1 commit(s) in 'main' missing from 'feat'");
}

#[test]
fn footer_entries_render_and_new_branch_opens_the_dialog() {
    let (_project, dir) = single_repo_project();
    let mut harness = branches_harness(dir);
    clear_recents(&mut harness);
    open_popup(&mut harness);

    for label in ["+ New branch", "Manage remotes…", "Compare…"] {
        assert_painted(&harness, label);
    }
    harness.get_by_label("+ New branch").click();
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::NewBranch),
        "+ New branch must open the New Branch dialog"
    );
}

// --- Pure logic (no harness): ordering, filtering, recents --------------------------

mod pure {
    use turbogit_domain::model::{Branch, BranchKind};
    use turbogit_ui::ui::branch_widget::{popup_entries, push_recent};

    fn b(name: &str, kind: BranchKind, favorite: bool) -> Branch {
        Branch {
            name: name.into(),
            kind,
            tracking: None,
            favorite,
            protected: false,
            exists: true,
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: None,
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

    // --- Issue 32: stale badges, remote sync markers, action gating ---------

    #[test]
    fn stale_badge_compacts_age_to_unit_labels() {
        use std::time::Duration;
        use turbogit_ui::ui::branch_widget::stale_badge;
        assert_eq!(stale_badge(Duration::from_secs(30)), "now");
        assert_eq!(stale_badge(Duration::from_secs(5 * 60)), "5m");
        assert_eq!(stale_badge(Duration::from_secs(3 * 60 * 60)), "3h");
        assert_eq!(stale_badge(Duration::from_secs(2 * 24 * 60 * 60)), "2d");
        assert_eq!(stale_badge(Duration::from_secs(3 * 7 * 24 * 60 * 60)), "3w");
        assert_eq!(stale_badge(Duration::from_secs(60 * 24 * 60 * 60)), "2mo");
        assert_eq!(stale_badge(Duration::from_secs(400 * 24 * 60 * 60)), "1y");
    }

    #[test]
    fn remote_row_marker_prefers_gone_then_ahead_behind() {
        use turbogit_ui::ui::branch_widget::remote_marker;
        // Diverged local counterpart: arrows, ahead first.
        assert_eq!(remote_marker(2, 1, false), Some("↑2 ↓1".into()));
        // Deleted upstream wins over any count.
        assert_eq!(remote_marker(3, 0, true), Some("gone".into()));
        // In sync: no marker.
        assert_eq!(remote_marker(0, 0, false), None);
    }

    /// Gated actions stay RENDERED with a hover explanation (the repo's
    /// disabled-with-reason convention), so each gate has pinned wording.
    #[test]
    fn gated_actions_explain_themselves() {
        use turbogit_ui::ui::branch_widget::{PopupEntry, delete_gate, rename_gate};
        let local = PopupEntry::Local {
            name: "feature".into(),
            favorite: false,
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: None,
        };
        // Enabled actions have no reason.
        assert_eq!(delete_gate(&local, Some("main"), false, true), None);
        assert_eq!(rename_gate(&local, Some("main"), false, true), None);
        // Protected: Delete explains the gate, Rename stays open.
        assert_eq!(
            delete_gate(&local, Some("main"), true, true),
            Some("protected branches cannot be deleted")
        );
        assert_eq!(rename_gate(&local, Some("main"), true, true), None);
        // The current branch, a vanished recent, tags and remote rows.
        assert_eq!(
            delete_gate(&local, Some("feature"), false, true),
            Some("check out another branch first")
        );
        assert_eq!(
            delete_gate(&local, Some("main"), false, false),
            Some("this branch no longer exists")
        );
        let tag = PopupEntry::Tag { name: "v1".into() };
        assert_eq!(
            delete_gate(&tag, None, false, true),
            Some("tags are not deleted from here")
        );
        let remote = PopupEntry::Remote {
            name: "main".into(),
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: None,
        };
        assert_eq!(
            rename_gate(&remote, Some("feature"), false, true),
            Some("remote branches cannot be renamed")
        );
        assert_eq!(
            rename_gate(&tag, None, false, true),
            Some("tags cannot be renamed")
        );
    }

    #[test]
    fn row_actions_gate_delete_on_current_protected_and_tags() {
        use turbogit_ui::ui::branch_widget::{PopupEntry, row_actions};
        let local = PopupEntry::Local {
            name: "feature".into(),
            favorite: false,
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: None,
        };
        // Ordinary local: every action offered.
        let a = row_actions(&local, Some("main"), false, true);
        assert!(a.delete && a.rename && !a.protected);
        // Protected: delete gated (destructive), lock set, rename+compare stay.
        let a = row_actions(&local, Some("main"), true, true);
        assert!(!a.delete && a.rename && a.protected);
        // The current branch cannot be deleted (git refuses) nor renamed by
        // itself from the list (the pinned row carries no actions).
        let cur = PopupEntry::Local {
            name: "main".into(),
            favorite: false,
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: None,
        };
        let a = row_actions(&cur, Some("main"), false, true);
        assert!(!a.delete && !a.rename);
        // Tags: neither delete nor rename.
        let tag = PopupEntry::Tag { name: "v1".into() };
        let a = row_actions(&tag, None, false, true);
        assert!(!a.delete && !a.rename && !a.protected);
        // A recent whose branch was deleted offers nothing destructive.
        let recent = PopupEntry::Recent {
            name: "gone-branch".into(),
            favorite: false,
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: None,
        };
        let a = row_actions(&recent, None, false, false);
        assert!(!a.delete && !a.rename);
    }
}
