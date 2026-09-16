//! Issue 15 — Log commit actions: cherry-pick to…, revert commit, create
//! branch here, offered from an Actions section in the commit-details pane
//! (screen 09).
//!
//! Headless kittest harness driving [`turbogit_ui::ui::render`] end-to-end
//! over a seeded repo (real git, tempdir):
//!
//! - `main` with `c1` ← `c2` (c2 adds `b.txt`)
//! - `feature` forked at `c1` with one extra commit
//!
//! Assertions use only public surfaces: painted output, public `AppState`
//! transitions, and the real git state of the seeded repo.

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui_kittest::kittest::NodeT as _;
use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::harness::{assert_painted, painted_text};
use turbogit_app::events::AppEvent;
use turbogit_app::state::{AppState, Dialog, Tab};
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;

// --- git fixture ---------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
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

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    alpha: PathBuf,
    #[expect(dead_code)]
    c1: String,
    c2: String,
}

fn seeded_repo() -> Seed {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    git(&alpha, &["init", "-q", "-b", "main"]);
    git(&alpha, &["config", "user.email", "t@t"]);
    git(&alpha, &["config", "user.name", "t"]);
    git(&alpha, &["config", "core.autocrlf", "false"]);
    let c1 = commit_file(&alpha, "a.txt", "one\n", "alpha: first commit");
    let c2 = commit_file(&alpha, "b.txt", "two\n", "alpha: second commit");
    git(&alpha, &["checkout", "-q", "-b", "feature", &c1]);
    commit_file(&alpha, "f.txt", "feature\n", "alpha: feature work");
    git(&alpha, &["checkout", "-q", "main"]);
    Seed {
        _tmp: tmp,
        project,
        alpha,
        c1,
        c2,
    }
}

// --- harness -------------------------------------------------------------------

fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    for root in state.multi.roots.clone() {
        let commits = engine.log(&root.path, &LogOpts::default()).expect("log");
        state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.id.clone(),
                commits: Ok(commits),
            })
            .expect("send LogLoaded");
    }
    state.drain_events();
    state.ui.tab = Tab::Log;

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
    harness.set_size(egui::vec2(
        1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
        800.0,
    ));
    settle(&mut harness);
    harness
}

fn settle(harness: &mut Harness<'_, AppState>) {
    let mut stable = 0;
    let mut prev = String::new();
    for _ in 0..300 {
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
        let cur = format!("{:?}", painted_text(harness));
        if cur == prev {
            stable += 1;
            if stable >= 3 {
                return;
            }
        } else {
            stable = 0;
            prev = cur;
        }
    }
    panic!("log layout did not settle within 300 frames");
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// Select a commit row in the graph and wait for the details pane.
fn select_commit(harness: &mut Harness<'_, AppState>, id: &str, subject: &str) {
    let label = format!("{} {subject}", short(id));
    harness.get_by_label(&label).click();
    settle(harness);
}

/// Wait until `pred` holds on the harness state.
fn wait_for(harness: &mut Harness<'_, AppState>, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..300 {
        harness.step();
        if pred(harness.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "condition not met; toast={:?} last_error={:?}",
        harness.state().ui.toast,
        harness.state().last_error
    );
}

// --- Cycle 1: the details pane offers the three actions ------------------------

#[test]
fn details_pane_offers_cherry_pick_revert_and_create_branch_actions() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    select_commit(&mut harness, &seed.c2, "alpha: second commit");

    assert_painted(&harness, "ACTIONS");
    assert_painted(&harness, "Cherry-pick to…");
    assert_painted(&harness, "Revert commit");
    assert_painted(&harness, "Create branch here");
}

// --- Cycle 2: revert with confirmation, refreshes the graph --------------------

#[test]
fn revert_from_the_log_creates_a_revert_commit_visible_in_the_graph() {
    let seed = seeded_repo();
    // The default settings protect main; this run exercises the happy path.
    let mut harness = log_harness(&seed);
    harness
        .state_mut()
        .settings
        .protected_branch_patterns
        .clear();

    select_commit(&mut harness, &seed.c2, "alpha: second commit");
    harness.get_by_label("Revert commit").click();
    settle(&mut harness);

    // Confirmation gate first: the revert must not run before OK.
    assert!(
        harness.state().ui.confirm.is_some(),
        "revert must ask for confirmation before dispatching"
    );
    harness.get_by_label("OK").click();
    settle(&mut harness);

    // The revert commit appears in the refreshed graph (production event
    // path: OpCompleted → refresh → LogLoaded → cache → repaint).
    wait_for(&mut harness, |s| {
        s.caches
            .log(&s.selected_root.clone().unwrap())
            .is_some_and(|cs| {
                cs.iter()
                    .any(|c| c.message.to_lowercase().starts_with("revert"))
            })
    });
    assert_painted(&harness, "Revert");
    assert!(
        harness
            .state()
            .ui
            .toast
            .as_ref()
            .is_some_and(|t| t.message.contains("Revert")),
        "a completed revert surfaces feedback"
    );
    // The revert really committed: HEAD's subject is the inverse commit.
    let subject = git(&seed.alpha, &["log", "-1", "--format=%s"]);
    assert!(
        subject.to_lowercase().starts_with("revert"),
        "HEAD must be the revert commit; got {subject:?}"
    );
}

// --- Cycle 3: cherry-pick to a chosen branch -----------------------------------

#[test]
fn cherry_pick_opens_the_branch_picker_and_applies_onto_the_chosen_branch() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    // Pick a main commit to apply onto feature.
    select_commit(&mut harness, &seed.c2, "alpha: second commit");
    harness.get_by_label("Cherry-pick to…").click();
    settle(&mut harness);

    // The picker lists the repo's local branches; the protected one is
    // disabled and explained, the free one is selectable.
    assert!(
        harness.state().ui.dialog == Some(Dialog::CherryPickTarget),
        "the action must open the target-branch picker"
    );
    assert_painted(&harness, "main");
    assert_painted(&harness, "(protected)");
    // Since the branch-tree extraction the branches pane paints its rows as
    // Buttons too, so "feature" matches twice; the picker's row is the one
    // right of the leftmost branches pane.
    let feature: Vec<_> = harness
        .query_all_by_label_contains("feature")
        .filter(|n| {
            n.accesskit_node().role() == egui::accesskit::Role::Button
                && n.accesskit_node().label().is_some_and(|l| l == "feature")
        })
        .collect();
    assert!(
        feature.len() >= 2,
        "the picker row and the pane row must both exist: {}",
        feature.len()
    );
    let mut sorted = feature;
    sorted.sort_by(|a, b| a.rect().left().total_cmp(&b.rect().left()));
    // The dialog's picker renders in a default-positioned Area at the far
    // left; the pane's row sits right of the sidebar.
    sorted.first().expect("picker row").click();
    settle(&mut harness);

    wait_for(&mut harness, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });
    // c2's change landed on feature exactly once…
    let count = git(&seed.alpha, &["rev-list", "--count", "feature"])
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(
        count, 3,
        "feature must gain exactly one cherry-picked commit"
    );
    assert_eq!(
        std::fs::read_to_string(seed.alpha.join("b.txt")).unwrap(),
        "two\n",
        "the cherry-picked commit's change must be on the target branch"
    );
    // …and the original checkout was restored.
    assert_eq!(
        git(&seed.alpha, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "main"
    );
}

// --- Cycle 4: create branch here prefills the new-branch dialog ----------------

#[test]
fn create_branch_here_opens_the_new_branch_dialog_prefilled() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    select_commit(&mut harness, &seed.c2, "alpha: second commit");
    harness.get_by_label("Create branch here").click();
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::NewBranch),
        "the action must open the new-branch dialog"
    );
    assert_eq!(
        harness.state().ui.dlg.new_branch_start,
        seed.c2,
        "the start point must be prefilled with the selected commit"
    );
}

// --- Cycle 5: guardrails — protected branch / dirty worktree -------------------

#[test]
fn actions_are_blocked_with_an_explanation_on_protected_branches() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    // main is protected by the default settings and currently checked out:
    // revert must be disabled with a visible explanation.
    select_commit(&mut harness, &seed.c2, "alpha: second commit");
    assert_painted(&harness, "protected");
    harness.get_by_label("Revert commit").click();
    settle(&mut harness);
    assert!(
        harness.state().ui.confirm.is_none(),
        "a blocked revert must never reach the confirmation gate"
    );

    // A dirty worktree additionally blocks cherry-pick, with its own reason.
    std::fs::write(seed.alpha.join("a.txt"), "uncommitted\n").unwrap();
    harness
        .state_mut()
        .refresh(turbogit_app::root_caches::Affected::All);
    settle(&mut harness);
    assert_painted(&harness, "dirty");
    harness.get_by_label("Cherry-pick to…").click();
    settle(&mut harness);
    assert!(
        harness.state().ui.dialog.is_none(),
        "a blocked cherry-pick must not open the branch picker"
    );
}
