use std::path::{Path, PathBuf};

use egui::{Pos2, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env, pin_eol_lf};
use test_support::harness::{
    assert_menu_item_gated, assert_not_painted, assert_painted, painted_galleys,
    shell_harness_over_animated,
};
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{Commit, LogOpts, RootId, Signature, SignatureState, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::integrate_service::OwnedOperation;
use turbogit_ui::ui::commit_menu::{CommitFacts, CommitMenuAction, commit_menu_items};
use turbogit_ui::ui::rewrite_gating::Scope;
use turbogit_ui::ui::shell::STATUS_BAR_HEIGHT;
use turbogit_ui::ui::sidebar::SIDEBAR_WIDTH;
use turbogit_ui::ui::widgets::MenuItemState;

const DURING_CHERRY_PICK: &str = "Cannot rewrite history during a cherry-pick";
const DURING_REBASE: &str = "Cannot rewrite history during a rebase";
const DURING_MERGE: &str = "Cannot rewrite history during a merge";
const DURING_REVERT: &str = "Cannot rewrite history during a revert";
const HAS_PARENTS: &str = "a merge commit cannot be dropped";
const OUTSIDE_SCOPE: &str = "the commit is in a repository outside the ones in scope";
const SEVERAL_IN_SCOPE: &str = "the rewrite runs in one repository; several are in scope";

const REASONS: [&str; 7] = [
    DURING_CHERRY_PICK,
    DURING_REBASE,
    DURING_MERGE,
    DURING_REVERT,
    HAS_PARENTS,
    OUTSIDE_SCOPE,
    SEVERAL_IN_SCOPE,
];

fn commit(subject: &str, parents: usize) -> Commit {
    let who = Signature {
        name: "Author".into(),
        email: "author@example.com".into(),
        time: 1_700_000_000,
    };
    Commit {
        id: "0123456789abcdef0123456789abcdef01234567".into(),
        parents: (0..parents)
            .map(|n| format!("{n}123456789abcdef0123456789abcdef012345678"))
            .collect(),
        author: who.clone(),
        committer: who,
        message: subject.into(),
        time: 1_700_000_000,
        root: RootId(std::path::Path::new("/ws/alpha").into()),
        signature: SignatureState::Unsigned,
    }
}

fn plain_commit() -> Commit {
    commit("feature: one", 1)
}

fn merge_commit() -> Commit {
    commit("Merge branch 'side'", 2)
}

const NO_OPERATION: CommitFacts = CommitFacts {
    dirty: false,
    protected_branch: false,
    on_current_branch: true,
    multi_root: true,
    owner: None,
    scope: Scope::Own,
    repo_name: "alpha",
};

#[track_caller]
fn gate(commit: &Commit, facts: CommitFacts<'_>) -> [MenuItemState; 11] {
    commit_menu_items(commit, &facts)
}

#[track_caller]
fn blocked(items: &[MenuItemState; 11], verb: CommitMenuAction) -> MenuItemState {
    items[verb.index()]
}

#[track_caller]
fn assert_only_reason(harness: &Harness<'_, AppState>, expected: &str) {
    assert_painted(harness, expected);
    for reason in REASONS {
        if reason != expected {
            assert_not_painted(harness, reason);
        }
    }
}

struct Project {
    _tmp: TempDir,
    project: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
}

fn dated() -> GitEnv {
    let mut env = GitEnv::fixture();
    env.author_date = Some("@100 +0000".to_string());
    env.committer_date = Some("@100 +0000".to_string());
    env
}

fn repo(parent: &Path, name: &str) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git_with_env(&dir, &["init", "-q", "-b", "main"], &dated());
    git_with_env(&dir, &["config", "user.email", "t@t"], &dated());
    git_with_env(&dir, &["config", "user.name", "t"], &dated());
    pin_eol_lf(&dir);
    write_commit(&dir, "base.txt", "base\n", "the base commit");
    write_commit(&dir, "a.txt", "one\n", &format!("{name} 1"));
    write_commit(&dir, "b.txt", "two\n", &format!("{name} 2"));
    git_with_env(&dir, &["branch", "side", "HEAD~2"], &dated());
    git_with_env(&dir, &["checkout", "-q", "-b", "feature"], &dated());
    dir
}

fn write_commit(dir: &Path, file: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(file), body).unwrap();
    git_with_env(dir, &["add", "."], &dated());
    git_with_env(dir, &["commit", "-q", "-m", msg], &dated());
    git_with_env(dir, &["rev-parse", "HEAD"], &dated())
        .trim()
        .to_string()
}

fn project() -> Project {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    std::fs::create_dir_all(&root).unwrap();
    let alpha = repo(&root, "alpha");
    let beta = repo(&root, "beta");
    Project {
        _tmp: tmp,
        project: root,
        alpha,
        beta,
    }
}

fn rebase_in_flight(repo: &Path) {
    std::fs::create_dir_all(repo.join(".git/rebase-merge")).unwrap();
}

fn cherry_pick_in_flight(repo: &Path) {
    std::fs::write(repo.join(".git/CHERRY_PICK_HEAD"), "deadbeef\n").unwrap();
}

fn merge_in_flight(repo: &Path) {
    std::fs::write(repo.join(".git/MERGE_HEAD"), "deadbeef\n").unwrap();
}

fn revert_in_flight(repo: &Path) {
    std::fs::write(repo.join(".git/REVERT_HEAD"), "deadbeef\n").unwrap();
}

fn project_with_merge() -> Project {
    let p = project();
    git_with_env(&p.alpha, &["checkout", "-q", "side"], &dated());
    write_commit(&p.alpha, "s.txt", "side\n", "side commit");
    git_with_env(&p.alpha, &["checkout", "-q", "feature"], &dated());
    git_with_env(&p.alpha, &["merge", "-q", "--no-edit", "side"], &dated());
    let merge = git_with_env(&p.alpha, &["rev-parse", "HEAD"], &dated());
    assert_eq!(
        git_with_env(&p.alpha, &["cat-file", "-p", merge.trim()], &dated())
            .lines()
            .filter(|line| line.starts_with("parent "))
            .count(),
        2,
        "the seeded tip is a merge commit"
    );
    p
}

fn window_size() -> Vec2 {
    egui::vec2(1280.0 + SIDEBAR_WIDTH, 800.0)
}

fn log_harness(p: &Project) -> Harness<'static, AppState> {
    let mut state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());
    state.ui.show_status_bar = true;
    let mut harness = harness_over_state(state);
    warm_logs(&mut harness);
    harness.state_mut().ui.tab = Tab::Log;
    harness.step();
    harness
}

fn harness_over_state(state: AppState) -> Harness<'static, AppState> {
    let mut harness = shell_harness_over_animated(state, window_size());
    test_support::harness::settle_quiet(&mut harness);
    harness
}

fn warm_logs(harness: &mut Harness<'_, AppState>) {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let roots = harness.state().multi.roots.clone();
    for root in roots {
        let commits = engine
            .log(&root.path, &LogOpts::default())
            .expect("log on a warm root");
        harness
            .state_mut()
            .tx
            .send(AppEvent::LogLoaded {
                root: root.id.clone(),
                commits: Ok(commits),
                mode: LogBatchMode::Replace,
            })
            .unwrap();
    }
    harness.step();
}

fn open_menu_on(harness: &mut Harness<'_, AppState>, subject: &str) {
    test_support::harness::right_click_row(harness, subject);
    harness.step();
    assert!(
        harness.state().ui.log_commit_menu.is_some(),
        "the commit menu did not open on {subject}"
    );
}

fn menu_column(harness: &Harness<'_, AppState>) -> f32 {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some("Copy hash".to_string()))
        .expect("the commit menu is open (no Copy hash row)")
        .rect()
        .min
        .x
}

fn menu_rows(harness: &Harness<'_, AppState>, column: f32) -> Vec<Rect> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| (n.rect().min.x - column).abs() < 2.0)
        .map(|n| n.rect())
        .collect()
}

fn menu_item_node<'t>(harness: &'t Harness<'_, AppState>, label: &str) -> egui_kittest::Node<'t> {
    let column = menu_column(harness);
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node().label() == Some(label.to_string())
                && (n.rect().min.x - column).abs() < 2.0
        })
        .unwrap_or_else(|| panic!("menu item {label} inside the open menu"))
}

fn hover_until_reason(harness: &mut Harness<'_, AppState>, label: &str, reason: &str) {
    harness.remove_cursor();
    for _ in 0..20 {
        harness.step();
    }
    let rect = menu_item_node(harness, label).rect();
    harness.remove_cursor();
    harness.step();
    harness.hover_at(rect.center());
    for _ in 0..120 {
        harness.step();
        if painted_galleys(harness)
            .iter()
            .any(|g| g.text.contains(reason))
        {
            return;
        }
    }
    panic!(
        "{reason:?} never painted while hovering {label:?}; painted: {:?}",
        painted_galleys(harness)
            .iter()
            .map(|g| &g.text)
            .collect::<Vec<_>>()
    );
}

fn occurrences(harness: &Harness<'_, AppState>, text: &str) -> Vec<Pos2> {
    painted_galleys(harness)
        .iter()
        .filter(|g| g.text.contains(text))
        .map(|g| g.pos)
        .collect()
}

fn status_bar_band() -> Rect {
    let height = window_size().y;
    Rect::from_min_max(
        egui::pos2(0.0, height - STATUS_BAR_HEIGHT - 8.0),
        egui::pos2(f32::INFINITY, height),
    )
}

fn in_band(positions: &[Pos2], band: Rect) -> usize {
    positions.iter().filter(|p| band.contains(**p)).count()
}

#[test]
fn a_rebase_in_one_root_gates_that_root_and_not_the_other() {
    let p = project();
    rebase_in_flight(&p.alpha);
    let mut h = log_harness(&p);

    open_menu_on(&mut h, "alpha 1");
    assert_menu_item_gated(&mut h, "Copy hash", "Drop commit");
    hover_until_reason(&mut h, "Drop commit", DURING_REBASE);
    assert_only_reason(&h, DURING_REBASE);

    open_menu_on(&mut h, "beta 1");
    let item = menu_item_node(&h, "Drop commit");
    assert!(
        !item.accesskit_node().is_disabled(),
        "a rebase in another repository must not gate this root's rewrite"
    );
    h.remove_cursor();
    h.step();
    for reason in REASONS {
        assert_not_painted(&h, reason);
    }
}

struct HeldState {
    name: &'static str,
    mark: for<'a> fn(&'a Path),
    reason: &'static str,
}

#[test]
fn each_operation_state_names_its_own_owner_on_both_verbs() {
    let states = [
        HeldState {
            name: "cherry-pick",
            mark: cherry_pick_in_flight,
            reason: DURING_CHERRY_PICK,
        },
        HeldState {
            name: "rebase",
            mark: rebase_in_flight,
            reason: DURING_REBASE,
        },
        HeldState {
            name: "merge",
            mark: merge_in_flight,
            reason: DURING_MERGE,
        },
        HeldState {
            name: "revert",
            mark: revert_in_flight,
            reason: DURING_REVERT,
        },
    ];
    for state in states {
        for verb in ["Drop commit", "Reword commit"] {
            let p = project();
            (state.mark)(&p.alpha);
            let mut h = log_harness(&p);
            open_menu_on(&mut h, "alpha 1");
            assert_menu_item_gated(&mut h, "Copy hash", verb);
            hover_until_reason(&mut h, verb, state.reason);
            assert!(
                !occurrences(&h, state.reason).is_empty(),
                "the {} state must name itself on {verb}",
                state.name
            );
            assert_only_reason(&h, state.reason);
        }
    }
}

#[test]
fn a_merge_commit_selection_states_only_its_parent_count() {
    let p = project_with_merge();
    let mut h = log_harness(&p);
    open_menu_on(&mut h, "Merge branch 'side'");
    assert_menu_item_gated(&mut h, "Copy hash", "Drop commit");
    hover_until_reason(&mut h, "Drop commit", HAS_PARENTS);
    assert_only_reason(&h, HAS_PARENTS);
}

#[test]
fn a_fleet_scope_of_several_repositories_states_the_scope_reason() {
    for verb in [CommitMenuAction::DropCommit, CommitMenuAction::RewordCommit] {
        let items = gate(
            &plain_commit(),
            CommitFacts {
                scope: Scope::Several,
                ..NO_OPERATION
            },
        );
        assert_eq!(
            blocked(&items, verb),
            MenuItemState::disabled("the rewrite runs in one repository; several are in scope"),
            "{verb:?}"
        );
    }
}

#[test]
fn a_commit_outside_the_fleet_scope_states_the_external_reason() {
    let items = gate(
        &plain_commit(),
        CommitFacts {
            scope: Scope::External,
            ..NO_OPERATION
        },
    );
    assert_eq!(
        blocked(&items, CommitMenuAction::DropCommit),
        MenuItemState::disabled("the commit is in a repository outside the ones in scope")
    );
}

#[test]
fn one_reason_wins_when_several_are_true() {
    let everything = CommitFacts {
        dirty: true,
        protected_branch: true,
        on_current_branch: false,
        multi_root: true,
        owner: Some(OwnedOperation::Revert),
        scope: Scope::External,
        repo_name: "alpha",
    };
    let items = gate(&merge_commit(), everything);
    assert_eq!(
        blocked(&items, CommitMenuAction::DropCommit),
        MenuItemState::disabled("Cannot rewrite history during a revert")
    );

    let items = gate(
        &merge_commit(),
        CommitFacts {
            owner: None,
            ..everything
        },
    );
    assert_eq!(
        blocked(&items, CommitMenuAction::DropCommit),
        MenuItemState::disabled("a merge commit cannot be dropped")
    );

    let items = gate(
        &plain_commit(),
        CommitFacts {
            owner: None,
            ..everything
        },
    );
    assert_eq!(
        blocked(&items, CommitMenuAction::DropCommit),
        MenuItemState::disabled("not on the current branch")
    );
    for other in [
        CommitMenuAction::CopyHash,
        CommitMenuAction::CopyMessage,
        CommitMenuAction::CreatePatch,
        CommitMenuAction::NewBranch,
        CommitMenuAction::NewTag,
    ] {
        assert_eq!(
            blocked(&items, other),
            MenuItemState::enabled(),
            "{other:?}"
        );
    }
    let items = gate(
        &plain_commit(),
        CommitFacts {
            owner: None,
            on_current_branch: true,
            ..everything
        },
    );
    assert_eq!(
        blocked(&items, CommitMenuAction::DropCommit),
        MenuItemState::disabled("the commit is in a repository outside the ones in scope")
    );
    let items = gate(
        &plain_commit(),
        CommitFacts {
            owner: None,
            on_current_branch: true,
            scope: Scope::Own,
            ..everything
        },
    );
    assert_eq!(
        blocked(&items, CommitMenuAction::DropCommit),
        MenuItemState::disabled("the current branch is protected")
    );
}

#[test]
fn the_reason_paints_below_every_menu_item_in_the_status_bar() {
    let p = project();
    rebase_in_flight(&p.alpha);
    let mut h = log_harness(&p);
    open_menu_on(&mut h, "alpha 1");
    hover_until_reason(&mut h, "Drop commit", DURING_REBASE);

    let menu_bottom = menu_rows(&h, menu_column(&h))
        .into_iter()
        .map(|rect| rect.max.y)
        .fold(0.0_f32, f32::max);

    let painted = occurrences(&h, DURING_REBASE);
    assert!(!painted.is_empty(), "the reason never painted at all");
    assert!(
        in_band(&painted, status_bar_band()) >= 1,
        "the reason must paint in the status bar band: {painted:?}"
    );
    let highest_bar_occurrence = painted
        .iter()
        .filter(|p| status_bar_band().contains(**p))
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min);
    assert!(
        highest_bar_occurrence > menu_bottom,
        "the status bar's reason ({highest_bar_occurrence}) must sit below every \
         menu item ({menu_bottom}); a reason inside the menu is the naive shape",
    );
}

#[test]
fn an_ordinary_commit_stays_live_and_says_nothing() {
    let p = project();
    let mut h = log_harness(&p);
    open_menu_on(&mut h, "alpha 1");
    for verb in ["Drop commit", "Reword commit"] {
        let item = menu_item_node(&h, verb);
        assert!(
            !item.accesskit_node().is_disabled(),
            "a clean, in-scope, single-parent commit on the current branch is rewritable: {verb}"
        );
    }
    h.remove_cursor();
    h.step();
    for reason in REASONS {
        assert_not_painted(&h, reason);
    }
}

#[test]
fn the_reason_goes_silent_when_the_pointer_leaves_the_item() {
    let p = project();
    rebase_in_flight(&p.alpha);
    let mut h = log_harness(&p);
    open_menu_on(&mut h, "alpha 1");
    hover_until_reason(&mut h, "Drop commit", DURING_REBASE);

    let copy = menu_item_node(&h, "Copy hash").rect();
    h.hover_at(copy.center());
    for _ in 0..30 {
        h.step();
        if occurrences(&h, DURING_REBASE).is_empty() {
            return;
        }
    }
    panic!(
        "the reason is still painted after the pointer left the blocked item: {:?}",
        occurrences(&h, DURING_REBASE)
    );
}
