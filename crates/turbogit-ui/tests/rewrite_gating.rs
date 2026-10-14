//! Rewrite gating is a state machine over the repository's state,
//! and the reason it gives is a status-bar affordance.
//!
//! TurboGit gated the two history verbs on the protected-branch rule alone. The
//! reference gates them on **which operation owns the repository**, and its
//! vocabulary is fully enumerable: four operation states (cherry-pick, rebase,
//! merge, revert), one structural one (the selection has parents), and two
//! multi-root ones (several repositories are in scope while the verb runs in
//! one; the commit's repository is outside the ones in scope). Seven reasons,
//! and a rewrite is blocked by **exactly one** of them — a UI that concatenates
//! every applicable reason reads as noise, so mutual exclusion is the property.
//!
//! The three structural assertions the ticket names:
//!
//! 1. **per-root discrimination** — a rebase in flight in root A makes root A's
//!    rewrite item say `during a rebase` while root B's item says nothing. A
//!    global in-flight flag passes nothing here and is the bug most likely to
//!    ship, so both roots are measured in one shell.
//! 2. **mutual exclusion** — for a merge-commit selection, a cross-repository
//!    selection and an external-repository selection, the expected reason paints
//!    and the other six do not.
//! 3. **location** — the reason paints in the **status bar**: its galley origin
//!    is below every menu item's, which is the part a naive implementation gets
//!    wrong by putting it in an inline tooltip only.
//!
//! Two fixture choices are load-bearing and stated where they are made: the
//! repositories are checked out on `feature`, because TurboGit's default
//! settings protect `main` and `master` and the protected guard is an up-front
//! refusal that stays — a fixture on `main` would have every
//! verb blocked by a reason this suite is not measuring; and the commit subjects
//! are root-qualified, because the log's listing is the union across visible
//! roots, so two repositories with the same subjects would let a right-click
//! open whichever repository the row order happened to put first.
//!
//! In-flight states are manufactured by writing git's own marker files rather
//! than by driving a conflicting operation: the marker names are measured
//! against real git in `turbogit-services/tests/repo_operation_state.rs`, and
//! here the question is what the app *does* with the answer while its log,
//! branch and selection state are all still the clean ones the test sets up.

use std::path::{Path, PathBuf};

use egui::{Pos2, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
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

// ------------------------------------------------------------------ reason vocabulary --
//
// One sentence per reason, spelled once here and once in `rewrite_gating.rs`.
// The four operation states share their shape and differ only by owner, which is
// what makes "exactly one is painted" a real assertion rather than a tautology.
const DURING_CHERRY_PICK: &str = "Cannot rewrite history during a cherry-pick";
const DURING_REBASE: &str = "Cannot rewrite history during a rebase";
const DURING_MERGE: &str = "Cannot rewrite history during a merge";
const DURING_REVERT: &str = "Cannot rewrite history during a revert";
const HAS_PARENTS: &str = "a merge commit cannot be dropped";
const OUTSIDE_SCOPE: &str = "the commit is in a repository outside the ones in scope";
const SEVERAL_IN_SCOPE: &str = "the rewrite runs in one repository; several are in scope";

/// The whole vocabulary, so an assertion can say "this one, and none of the
/// other six" without restating six names at every call site.
const REASONS: [&str; 7] = [
    DURING_CHERRY_PICK,
    DURING_REBASE,
    DURING_MERGE,
    DURING_REVERT,
    HAS_PARENTS,
    OUTSIDE_SCOPE,
    SEVERAL_IN_SCOPE,
];

// --- the gate, off the shell ---------------------------------------------------
//
// The two cross-repository reasons are computed by the same gate as the four
// operation states, and they are measured here rather than on the shell for a
// reason that is a fact of the product, not of the test: a checked repository set
// replaces the Log pane with the multi-repo selection surface
// (`shell.rs::show_tool_window`), so no commit row can be right-clicked while two
// repositories are in scope. The vocabulary is the contract the ticket names —
// TurboGit computes both reasons from the same scope every fleet verb and the
// status bar's own `N repos in scope` counter read, and a fleet rewrite entry is
// the surface that would paint them. What the shell DOES measure is the property
// a global in-flight flag would fake: the reason is computed for the root the row
// belongs to and for no other (assertion 1).

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

/// A commit with one parent: the row a rewrite verb is ever offered on.
fn plain_commit() -> Commit {
    commit("feature: one", 1)
}

/// A commit with two.
fn merge_commit() -> Commit {
    commit("Merge branch 'side'", 2)
}

/// Nothing holds the repository and the commit's own repository is the one in
/// scope — the posture every reason below is measured as a departure from.
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

/// Exactly one reason speaks: `expected` paints and every other member of the
/// vocabulary is silent.
#[track_caller]
fn assert_only_reason(harness: &Harness<'_, AppState>, expected: &str) {
    assert_painted(harness, expected);
    for reason in REASONS {
        if reason != expected {
            assert_not_painted(harness, reason);
        }
    }
}

// -------------------------------------------------------------------- fixtures --

struct Project {
    _tmp: TempDir,
    project: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
}

// Every call is pinned per call through the fixture crate's runner: a headless
// machine has no global identity to borrow. The dates ride on the same pin
// because the gating rows are ordered by time, and an unpinned wall clock makes
// a "was this commit old enough" row an accident of when the suite ran.

/// The shared pin, with both roles committing at a fixed epoch: one value for
/// every call here, which is what the old runner's fixed `GIT_*_DATE` did.
fn dated() -> GitEnv {
    let mut env = GitEnv::fixture();
    env.author_date = Some("@100 +0000".to_string());
    env.committer_date = Some("@100 +0000".to_string());
    env
}

/// One repository: `base` ← `<name> 1` ← `<name> 2` on `main`, a `side` branch
/// off `base`, and the checkout left on `feature` — see the module doc for why
/// the verbs are not measured on the protected default branch.
fn repo(parent: &Path, name: &str) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git_with_env(&dir, &["init", "-q", "-b", "main"], &dated());
    git_with_env(&dir, &["config", "user.email", "t@t"], &dated());
    git_with_env(&dir, &["config", "user.name", "t"], &dated());
    git_with_env(&dir, &["config", "core.autocrlf", "false"], &dated());
    write_commit(&dir, "base.txt", "base\n", "the base commit");
    write_commit(&dir, "a.txt", "one\n", &format!("{name} 1"));
    write_commit(&dir, "b.txt", "two\n", &format!("{name} 2"));
    git_with_env(&dir, &["branch", "side", "HEAD~2"], &dated());
    git_with_env(&dir, &["checkout", "-q", "-b", "feature"], &dated());
    dir
}

/// Commit with the identity and the timestamp taken from the caller's `git`
/// wrapper: both are pinned, so the commit ids — and therefore the union
/// listing's row order, which falls back to the id on a time tie — are the same
/// in every run, and the row a test right-clicks is the row the menu opens on.
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

/// Put `repo` into git's own `rebase` state without moving its HEAD: the marker
/// directory is what the gate reads, and the app's branch, log and selection
/// state stay the ones the test set up.
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

/// `alpha` with a merge commit on top of its current branch, so the
/// parent-count reason has a real row to state itself on.
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

// ------------------------------------------------------------------- harness --

/// The window box: 1280 px of tool window beside the sidebar, 800 tall. The
/// status-bar band is measured off the same height, so resizing here cannot
/// leave an assertion measuring the wrong strip.
fn window_size() -> Vec2 {
    egui::vec2(1280.0 + SIDEBAR_WIDTH, 800.0)
}

/// The shell on the Git Log tab over both roots, with both logs warm through the
/// production event path.
fn log_harness(p: &Project) -> Harness<'static, AppState> {
    let mut state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());
    // The reason is a STATUS-BAR affordance, and `for_roots` builds the headless
    // shell with the status bar hidden (`UiState::default()`, unlike
    // `launch_in`). Hidden, the reference's own docs say the user cannot see the
    // reason at all — so every measurement here starts with the region on.
    state.ui.show_status_bar = true;
    let mut harness = harness_over_state(state);
    warm_logs(&mut harness);
    harness.state_mut().ui.tab = Tab::Log;
    harness.step();
    harness
}

/// `step_dt` 1/60 because the menu floats over a row whose highlight animates;
/// the shared quarter-second default would move its rect against the row the
/// test is measured on.
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

/// Right-click the commit row whose subject contains `subject`, opening the
/// commit menu on that row's commit in that row's repository.
fn open_menu_on(harness: &mut Harness<'_, AppState>, subject: &str) {
    test_support::harness::right_click_row(harness, subject);
    harness.step();
    assert!(
        harness.state().ui.log_commit_menu.is_some(),
        "the commit menu did not open on {subject}"
    );
}

/// The open menu's left edge, read off its first row: only this menu's rows
/// share one column, so the anchor keeps a shell-wide label from matching a
/// toolbar or a popup button.
fn menu_column(harness: &Harness<'_, AppState>) -> f32 {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some("Copy hash".to_string()))
        .expect("the commit menu is open (no Copy hash row)")
        .rect()
        .min
        .x
}

/// The menu's own rows, for the location measurement: everything in the open
/// menu's column.
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

/// Rest the pointer on a menu item and keep framing until `reason` paints —
/// the hover affordance is a two-frame gesture (the hint is written by the menu
/// pass and read by the status bar the following frame, because the shell lays
/// the bottom panel out before the tool window), so polling is the honest way to
/// observe it.
fn hover_until_reason(harness: &mut Harness<'_, AppState>, label: &str, reason: &str) {
    // The menu is an `egui::Area` with a 0.12 s fade (`theme.rs`), and a
    // right-click leaves the pointer on the ROW the menu floats away from, not on
    // the row's items. Settling first is what a user does — they hover a menu
    // that has finished opening — and it is what makes the rect measured below
    // the rect the item is painted at.
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

/// The painted galleys carrying `text`, with their origins — the position-scoped
/// half of every assertion here.
fn occurrences(harness: &Harness<'_, AppState>, text: &str) -> Vec<Pos2> {
    painted_galleys(harness)
        .iter()
        .filter(|g| g.text.contains(text))
        .map(|g| g.pos)
        .collect()
}

/// The bottom band of the window, which is where the shell's status bar paints.
///
/// `8.0` px of slack above the panel's own 24 px, not 1: a status-line label is
/// vertically centred, and its galley ORIGIN sits a few points above the band it
/// is painted into (the version line's origin is 774.0 in a panel spanning
/// 776.0..800.0). Measuring the tight rect would put the shell's own version line
/// outside its own status bar.
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

// -------------------------------------------------------------------- tests --

/// Assertion 1 — the per-root discrimination, which is the whole point. A rebase
/// in flight in `alpha` makes `alpha`'s rewrite item name the rebase; `beta`'s
/// item, in the same shell, says nothing at all.
#[test]
fn a_rebase_in_one_root_gates_that_root_and_not_the_other() {
    let p = project();
    rebase_in_flight(&p.alpha);
    let mut h = log_harness(&p);

    open_menu_on(&mut h, "alpha 1");
    assert_menu_item_gated(&mut h, "Copy hash", "Drop commit");
    hover_until_reason(&mut h, "Drop commit", DURING_REBASE);
    assert_only_reason(&h, DURING_REBASE);

    // The same verb, one root over: live, and silent about any reason. A global
    // in-flight flag fails here and passes everything above.
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

/// One of git's four held states, measured as a triple: the word the suite names
/// it by, the marker it writes into `.git`, and the reason that must state it.
struct HeldState {
    name: &'static str,
    mark: for<'a> fn(&'a Path),
    reason: &'static str,
}

/// Each of the four operation states names ITS owner and no other's, on both
/// rewrite verbs — the two verbs share one gate, so a reason that reached only
/// one of them would leave the other clickable mid-operation.
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

/// Assertion 2 — a merge commit is blocked by its parent count and by nothing
/// else: the operation states and the two scope reasons stay unspoken.
#[test]
fn a_merge_commit_selection_states_only_its_parent_count() {
    let p = project_with_merge();
    let mut h = log_harness(&p);
    // The merge is the current branch's tip, which the union listing walks first.
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

/// The commit's repository is not one of the ones in scope. Mutually exclusive
/// with the reason above by construction: `Scope` is one enum, so a repository
/// cannot be both outside the scope and one of several in it.
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

/// Exactly one reason speaks when several are true at once, and it is the one the
/// developer can act on next. The order the gate documents
/// (`rewrite_gating::blocked`): the operation that owns the repository outranks
/// the commit's shape, which outranks where the commit sits, which outranks the
/// workspace's posture, which outranks the branch policy. This compiles either
/// way, which is why it is pinned here rather than left to reading the arms.
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
    // A repository held by a revert: the revert is the reason, and the merge's
    // parent count, the off-branch bound, the outside-scope bound and the
    // protected policy all stay unspoken.
    let items = gate(&merge_commit(), everything);
    assert_eq!(
        blocked(&items, CommitMenuAction::DropCommit),
        MenuItemState::disabled("Cannot rewrite history during a revert")
    );

    // The same commit with the repository free: the next reason down the list
    // takes over, and it alone.
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

    // A plain commit in the same posture: shape and reachability give way to the
    // scope, and the scope gives way to nothing but itself.
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
    // The gate bounds the two rewrites and nothing else: reading, copying,
    // branching and cherry-picking a blocked commit stay live, so one guard
    // never silences the nine items that do not need the repository free.
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

/// Assertion 3 — the reason belongs to the STATUS BAR, not only the menu: its
/// galley origin is below every menu item's, which is the part an inline
/// tooltip gets wrong. The reference documents exactly this affordance ("hover
/// over the action in the context menu and look for the message in the status
/// bar"), including that the user has to have the status text region on.
#[test]
fn the_reason_paints_below_every_menu_item_in_the_status_bar() {
    let p = project();
    rebase_in_flight(&p.alpha);
    let mut h = log_harness(&p);
    open_menu_on(&mut h, "alpha 1");
    hover_until_reason(&mut h, "Drop commit", DURING_REBASE);

    // Every row of the open menu, measured at its lowest point.
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

/// A rewrite with nothing wrong about it stays live — the gate is enablement, and
/// no reason paints until a blocked item is hovered. The ticket's posture is
/// enablement over confirmation: nothing here adds a confirmation step, and the
/// preflight that a live verb opens is the one already shipped.
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

/// Leaving the blocked item takes the reason with it: the status bar reports
/// what the pointer is on now, not the last thing it was on.
#[test]
fn the_reason_goes_silent_when_the_pointer_leaves_the_item() {
    let p = project();
    rebase_in_flight(&p.alpha);
    let mut h = log_harness(&p);
    open_menu_on(&mut h, "alpha 1");
    hover_until_reason(&mut h, "Drop commit", DURING_REBASE);

    // Move onto an item nothing can block.
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
