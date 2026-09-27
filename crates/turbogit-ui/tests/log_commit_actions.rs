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
use test_support::harness::{
    assert_menu_item_gated, assert_not_painted, assert_painted, click_menu_item, painted_galleys,
    painted_text, right_click_row,
};
use turbogit_app::events::{AppEvent, LogPageMode};
use turbogit_app::state::{AppState, Dialog, NewBranchBase, Tab};
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::Palette;

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

/// `git` WITHOUT asserting success, for reading a ref that is expected to be
/// absent — `git rev-parse --verify --quiet` exits non-zero when the ref does
/// not exist, and that non-zero exit is the answer.
fn git_absent(dir: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git must be on PATH");
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
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

/// Answer every registered root's log through the production event path. A
/// `refresh` drops the cache, so a test that watches a rewrite warm again.
fn warm_logs(harness: &mut Harness<'_, AppState>) {
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    let roots = harness.state().multi.roots.clone();
    for root in roots {
        let commits = engine.log(&root.path, &LogOpts::default()).expect("log");
        harness
            .state_mut()
            .tx
            .send(AppEvent::LogLoaded {
                root: root.id.clone(),
                commits: Ok(commits),
                mode: LogPageMode::Replace,
            })
            .expect("send LogLoaded");
    }
    harness.step();
}

fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    log_harness_over(seed.project.clone())
}

/// The log shell over any project directory, with its logs warm.
fn log_harness_over(project: PathBuf) -> Harness<'static, AppState> {
    let mut state = AppState::new(project);
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
                mode: LogPageMode::Replace,
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

/// The new-branch dialog's own row for `branch`. The branches pane paints rows
/// with the same labels, and the dialog's rows sit leftmost.
fn picker_row<'h>(harness: &'h Harness<'_, AppState>, branch: &'h str) -> egui_kittest::Node<'h> {
    harness
        .query_all_by_label(branch)
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .min_by_key(|n| n.rect().left() as i32)
        .unwrap_or_else(|| panic!("the picker's {branch:?} row"))
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// A commit row's accessibility label: the short reference, then the subject.
fn row_label(id: &str, subject: &str) -> String {
    format!("{} {subject}", short(id))
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

// --- Cycle 1: the commit's menu offers the actions -----------------------------

/// The details pane used to carry an ACTIONS group with these verbs. A right-click
/// on the commit row is now the one place they live, so the test keeps its shape
/// and changes only its driver: the row is right-clicked rather than selected.
#[test]
fn the_commit_menu_offers_the_actions_the_details_pane_used_to() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));

    assert_painted(&harness, "Cherry-pick to…");
    assert_painted(&harness, "Revert commit");
    assert_painted(&harness, "New branch");
    // The group title and its alert box went with the group.
    assert_not_painted(&harness, "ACTIONS");
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

    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    click_menu_item(&mut harness, "Copy hash", "Revert commit");
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
    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    click_menu_item(&mut harness, "Copy hash", "Cherry-pick to…");
    settle(&mut harness);

    // The picker lists the repo's local branches. `main` is the branch the work
    // is standing on, so it is refused as the current branch — the picker states
    // one reason per row, and this one wins over the protection `main` also
    // carries, because no setting change turns the branch you are on into a
    // destination. (The hover text that goes with it is pinned by
    // `commit_context_menu.rs`, which can drive a pointer into the row.)
    assert!(
        harness.state().ui.dialog == Some(Dialog::CherryPickTarget),
        "the action must open the target-branch picker"
    );
    assert_painted(&harness, "main (current)");
    assert_not_painted(&harness, "main (protected)");
    assert!(
        harness
            .get_by_label("main (current)")
            .accesskit_node()
            .is_disabled(),
        "the current branch is refused, and stays rendered rather than vanishing"
    );
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

/// New branch creates the branch WHERE THE ROW WAS RIGHT-CLICKED, not at the
/// tip, and leaves the current branch checked out. The repository is the
/// assertion, not a dialog field: every field could be right while the ref git
/// wrote points at the tip instead.
#[test]
fn new_branch_from_the_menu_creates_the_branch_at_the_right_clicked_commit() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);
    // `c1` is the middle commit, so a branch created at the tip and one created
    // at the clicked commit cannot be mistaken for each other.
    right_click_row(&mut harness, &row_label(&seed.c1, "alpha: first commit"));
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::NewBranch),
        "the action must open the new-branch dialog"
    );

    harness.state_mut().ui.dlg.new_branch_name = "from-the-log".into();
    settle(&mut harness);
    harness.get_by_label("Create").click();
    wait_for(&mut harness, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });

    assert_eq!(
        git(&seed.alpha, &["rev-parse", "from-the-log"]).trim(),
        seed.c1,
        "the new branch points at the commit that was right-clicked"
    );
    assert_eq!(
        git(&seed.alpha, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "main",
        "and creating it leaves the current branch checked out"
    );
}

/// Cancelling the new-branch dialog is a decision not to create a branch, so
/// the repository has to say so: a name good enough to create from, the real
/// Cancel affordance, and then no ref and no commit anywhere.
#[test]
fn cancelling_the_new_branch_dialog_from_the_menu_creates_nothing() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);
    let branches_before = git(&seed.alpha, &["for-each-ref", "--format=%(refname:short)"]);

    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle(&mut harness);
    harness.state_mut().ui.dlg.new_branch_name = "should-never-exist".into();
    settle(&mut harness);

    harness.get_by_label("Cancel").click();
    settle(&mut harness);

    assert!(harness.state().ui.dialog.is_none(), "the dialog went");
    assert_eq!(
        git(&seed.alpha, &["for-each-ref", "--format=%(refname:short)"]),
        branches_before,
        "and no branch was created"
    );
    assert_eq!(
        git(&seed.alpha, &["rev-parse", "HEAD"]).trim(),
        seed.c2,
        "no commit was made either"
    );
    assert_eq!(
        git(&seed.alpha, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "main",
        "and the branch the work was standing on is still checked out"
    );
}

/// "Start from:" has to read as whatever it names. A branch reads as a branch
/// name, unchanged; a commit reads as the short reference the log shows
/// everywhere, with its subject as a quiet annotation beside it — never the
/// 40-character hash, which is neither legible nor a value anyone recognises,
/// and never at the cost of the one control that changes the base.
#[test]
fn the_start_from_row_reads_a_commit_as_a_short_reference_and_its_subject() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    right_click_row(&mut harness, &row_label(&seed.c1, "alpha: first commit"));
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle(&mut harness);

    assert_painted(&harness, &short(&seed.c1));
    // A 40-character hash is not a base anyone can read.
    assert_not_painted(&harness, &seed.c1);
    let subject = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "alpha: first commit" && g.color == Palette::INK_3)
        .expect("the commit's subject rides beside the reference, in the muted ink");
    let change = harness.get_by_label("Change…").rect();
    assert!(
        subject.rect.max.x <= change.min.x,
        "and the subject must not run under the button that changes the base: \
         subject ends at {:?}, Change… starts at {:?}",
        subject.rect.max,
        change.min
    );
}

/// The branch picker lists branches, and a commit is not one of them. So the
/// base the log named survives opening the picker — the row above still states
/// it, legibly — and picking a branch is how you change one's mind: the base
/// becomes that branch, the picker closes, and the row reads as a branch name.
#[test]
fn the_branch_picker_offers_branches_while_the_base_is_a_commit() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);
    right_click_row(&mut harness, &row_label(&seed.c1, "alpha: first commit"));
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle(&mut harness);

    harness.get_by_label("Change…").click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.dlg.new_branch_base,
        NewBranchBase::Commit(seed.c1.clone()),
        "opening the picker does not drop the commit the log named"
    );
    // The row above still names it, so a list with nothing marked is not a
    // mystery.
    assert_painted(&harness, &short(&seed.c1));
    assert!(
        painted_galleys(&harness)
            .iter()
            .any(|g| g.text == "alpha: first commit" && g.color == Palette::INK_3),
        "subject included: the base stays readable while the picker is open"
    );

    // Picking a branch is the way out: it replaces the commit, closes the list,
    // and the row reads as a branch name again. The branches pane paints rows
    // with the same labels, so the picker's row is the leftmost — the dialog is a
    // default-positioned area at the far left (the same disambiguation the
    // cherry-pick test above makes).
    picker_row(&harness, "feature").click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.dlg.new_branch_base,
        NewBranchBase::Branch("feature".into()),
        "a picked branch becomes the base"
    );
    assert!(
        !harness.state().ui.dlg.new_branch_base_picker_open,
        "and the picker closes, as picking from a list does"
    );
    assert!(
        !painted_galleys(&harness)
            .iter()
            .any(|g| g.text == "alpha: first commit" && g.color == Palette::INK_3),
        "the commit is no longer the base, so its subject leaves the row"
    );
    assert_painted(&harness, "feature");
}

// --- Cycle 4b: drop commit, end to end ----------------------------------------

/// The commit count of `main`, read the way the tests reason about it.
fn count_of(dir: &Path, rev: &str) -> usize {
    git(dir, &["rev-list", "--count", rev])
        .trim()
        .parse()
        .expect("a commit count")
}

/// A project whose `feature` branch has a commit with a DESCENDANT on it — the
/// only shape a "drop one, keep the rest" assertion can be made on. The seed is
/// `test_support::git_seed`'s, the same history the app suite's settlement
/// tests rewrite, so both measure the same operation. Returns the project, the
/// repository, the commit to drop and its descendant.
fn history_to_rewrite() -> (TempDir, PathBuf, PathBuf, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("ws");
    std::fs::create_dir_all(&project).unwrap();
    let repo = test_support::git_seed::repo_with_history(&project, "alpha");
    let target = git(&repo, &["rev-parse", "HEAD~1"]).trim().to_string();
    let descendant = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    (tmp, project, repo, target, descendant)
}

/// Right-click `subject`'s row, take Drop commit, and stop at the preflight.
fn open_rewrite_preflight(harness: &mut Harness<'_, AppState>, c1: &str, subject: &str) {
    right_click_row(harness, &row_label(c1, subject));
    click_menu_item(harness, "Copy hash", "Drop commit");
    settle(harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::RewritePreflight),
        "the item opens the preflight, not the rewrite"
    );
}

/// Confirming removes exactly that one commit, keeps the descendant that was
/// built on it, and leaves the backup ref the recovery path restores from.
///
/// Every assertion is read back from the repository: the operation's own report
/// is not the evidence that history moved, and a log that says one thing while
/// git says another is the failure this ticket exists to prevent.
#[test]
fn confirming_the_drop_preflight_removes_one_commit_and_keeps_its_descendants() {
    let (_tmp, project, repo, target, _descendant) = history_to_rewrite();
    let mut harness = log_harness_over(project);
    let head_before = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let count_before = count_of(&repo, "feature");

    open_rewrite_preflight(&mut harness, &target, "feature-2");
    harness.get_by_label("Drop this commit").click();
    wait_for(&mut harness, |s| s.ui.toast.is_some());

    // Exactly one commit fewer, and it is the one that was named.
    assert_eq!(
        count_of(&repo, "feature"),
        count_before - 1,
        "one commit fewer, not the whole branch"
    );
    let subjects = git(&repo, &["log", "--format=%s", "feature"]);
    assert!(
        !subjects.contains("feature-2"),
        "the commit that was dropped is gone: {subjects}"
    );
    assert!(
        subjects.contains("feature-3"),
        "and the commit built on it is not: {subjects}"
    );
    // Its change came along with it, and the dropped commit's own change did
    // not: the replay is the history minus one commit, not a squash of two.
    assert_eq!(
        std::fs::read_to_string(repo.join("feature-3.txt")).unwrap(),
        "3\n",
        "the kept commit's change is still on the branch"
    );
    assert!(
        !repo.join("feature-2.txt").exists(),
        "and the dropped commit's change is not"
    );
    // The safety net: a backup ref written at the pre-rewrite HEAD, which is
    // what the recovery path restores from.
    assert_eq!(
        git(&repo, &["rev-parse", "refs/turbogit/preflight-backup"]).trim(),
        head_before,
        "the backup ref points at the history as it was before the rewrite"
    );
    assert_eq!(
        git(&repo, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "feature",
        "and the branch that was checked out is still checked out"
    );

    // The log refreshed through the ordinary completion path — the surface makes
    // no refresh call of its own, so a stale listing here would mean the ordinary
    // path did not run. (The graph's own count line is the assertion: the
    // dropped SUBJECT still shows in the changed-files pane until the selection
    // follows the history, which is the next test's subject.)
    warm_logs(&mut harness);
    settle(&mut harness);
    assert_painted(&harness, "3 shown");
    assert_painted(&harness, "feature-3");
}

/// Right-click `subject`'s row, take Reword commit, write `message`, and stop at
/// the preflight — the two steps of the verb, with the developer's typing
/// standing in for the keystrokes the editor test drives for real.
fn open_reword_preflight(
    harness: &mut Harness<'_, AppState>,
    c1: &str,
    subject: &str,
    message: &str,
) {
    right_click_row(harness, &row_label(c1, subject));
    click_menu_item(harness, "Copy hash", "Reword commit");
    settle(harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::Reword),
        "the item opens the editor, not the rewrite"
    );
    harness.state_mut().ui.dlg.reword_message = message.to_owned();
    settle(harness);
    harness.get_by_label("Reword this commit").click();
    settle(harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::RewritePreflight),
        "and the editor's confirm opens the preflight, not the rewrite"
    );
}

/// Confirming a reword rewrites the commit with the new message and IDENTICAL
/// content — the whole point of the verb, and the one thing it must never be
/// mistaken for. Correcting a typo does not change what the commit did, so the
/// tree is compared before and after, and every other assertion is read back from
/// the repository rather than taken from the operation's own report.
#[test]
fn confirming_the_reword_preflight_rewrites_the_message_and_nothing_else() {
    let (_tmp, project, repo, target, tip_before) = history_to_rewrite();
    let mut harness = log_harness_over(project);
    let count_before = count_of(&repo, "feature");
    let tree_before = git(&repo, &["rev-parse", "HEAD^{tree}"]).trim().to_string();

    open_reword_preflight(
        &mut harness,
        &target,
        "feature-2",
        "feature: the second one",
    );
    harness.get_by_label("Reword this commit").click();
    wait_for(&mut harness, |s| s.ui.toast.is_some());

    // The message is the new one, and it is on the commit that holds this
    // commit's place: a reword keeps the row, so the row is read by position.
    let message = git(&repo, &["log", "--format=%B", "-1", "feature~1"]);
    assert!(
        message.trim() == "feature: the second one",
        "the reworded commit carries the message that was typed: {message:?}"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD^{tree}"]).trim(),
        tree_before,
        "and the content is untouched: correcting a typo does not change what \
         the commit did"
    );
    assert_eq!(
        count_of(&repo, "feature"),
        count_before,
        "a reword changes no commit's place in the history, only the message"
    );
    // Its change came along with it, which is what "identical content" means on
    // disk and not only in the tree object.
    assert_eq!(
        std::fs::read_to_string(repo.join("feature-2.txt")).unwrap(),
        "2\n",
        "the reworded commit's own change is still in the working tree"
    );
    // The old reference is dead as a commit IN the history: the branch no longer
    // contains it. It survives in the object database, which is what the backup
    // ref below is for.
    let on_branch = git(&repo, &["rev-list", "feature"]);
    assert!(
        !on_branch.contains(&target),
        "the rewritten commit is a new object, so the old reference names \
         nothing in the branch any more"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "refs/turbogit/preflight-backup"]).trim(),
        tip_before,
        "and the backup ref still points at the pre-rewrite history, so the \
         recovery path can restore what the old reference meant"
    );
    assert_eq!(
        git(&repo, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "feature",
        "and the branch that was checked out is still checked out"
    );

    // The log refreshed through the ordinary completion path, and it now shows
    // the new message.
    warm_logs(&mut harness);
    settle(&mut harness);
    assert_painted(&harness, "feature: the second one");
    // The old subject is gone as a SUBJECT. Checked on a whole galley rather than
    // as a substring, because the reworded commit's file is `feature-2.txt` and
    // the changed-files pane still paints it — the name it is, not a message.
    assert!(
        !painted_galleys(&harness)
            .iter()
            .any(|g| g.text == "feature-2"),
        "the log no longer shows the old subject"
    );
}

/// A reword keeps the row, so the selection has a commit to STAY on — but under
/// a new hash, because every commit above a rewritten one is re-created. It
/// follows the history to the commit that was reworded, through the shared
/// reselection rule, and the pane never points at an id the repository no longer
/// has as a commit in the log.
#[test]
fn the_selection_stays_on_the_commit_that_was_reworded_at_its_new_hash() {
    let (_tmp, project, repo, target, _descendant) = history_to_rewrite();
    let mut harness = log_harness_over(project);
    let root = harness
        .state()
        .selected_root
        .clone()
        .expect("the log selected a repository");

    open_reword_preflight(
        &mut harness,
        &target,
        "feature-2",
        "feature: the second one",
    );
    harness.get_by_label("Reword this commit").click();
    // The selection moves when the POST-rewrite log arrives, not when the
    // operation returns — the cache is dropped by the refresh and refilled. The
    // log's LENGTH cannot be the signal here: a reword does not change how many
    // commits there are, so a pre-rewrite log already has the count this waits
    // for. The new message is the arrival.
    wait_for(&mut harness, |s| {
        s.caches.log(&root).is_some_and(|cs| {
            cs.iter()
                .any(|c| c.message.lines().next() == Some("feature: the second one"))
        })
    });
    settle(&mut harness);

    let reworded = git(&repo, &["rev-parse", "feature~1"]).trim().to_string();
    assert_eq!(
        harness
            .state()
            .caches
            .log(&root)
            .map(<[turbogit_domain::model::Commit]>::len),
        Some(4),
        "and a reword changed no commit's place, so the count is what it was"
    );
    assert_ne!(reworded, target, "the reworded commit is a new object");
    let selected = harness
        .state()
        .ui
        .selected_commit
        .clone()
        .expect("a selection after the rewrite");
    assert_eq!(
        selected, reworded,
        "the selection followed the row to its new hash, through the shared rule"
    );
    let held = harness
        .state()
        .caches
        .log(&root)
        .is_some_and(|cs| cs.iter().any(|c| c.id == selected));
    assert!(
        held,
        "and it names a commit the refreshed log actually holds"
    );
    // The pane followed too: it describes the reworded commit's own change.
    assert_painted(&harness, "feature-2.txt");
}

/// A rewrite gives every descendant a new hash, so the selection cannot stay on
/// the id it had — it follows the history to the commit that took the dropped
/// commit's place. The pane keeps describing where the developer is, and never
/// points at a commit the repository no longer has.
#[test]
fn the_selection_follows_the_history_to_the_commit_that_took_the_dropped_places() {
    let (_tmp, project, repo, target, descendant_before) = history_to_rewrite();
    let mut harness = log_harness_over(project);
    let root = harness
        .state()
        .selected_root
        .clone()
        .expect("the log selected a repository");

    open_rewrite_preflight(&mut harness, &target, "feature-2");
    harness.get_by_label("Drop this commit").click();
    // The selection moves when the POST-rewrite log arrives, not when the
    // operation returns — the cache is dropped by the refresh and refilled.
    wait_for(&mut harness, |s| {
        s.caches.log(&root).is_some_and(|cs| cs.len() == 3)
    });
    settle(&mut harness);

    let tip_after = git(&repo, &["rev-parse", "feature"]).trim().to_string();
    assert_ne!(
        tip_after, descendant_before,
        "the rewrite gave the descendant a new hash, so the old id names nothing"
    );
    let selected = harness
        .state()
        .ui
        .selected_commit
        .clone()
        .expect("a selection after the rewrite");
    assert_ne!(selected, target, "the dropped commit is not the selection");
    assert_eq!(
        selected, tip_after,
        "the selection is the commit that took the dropped commit's place"
    );
    assert!(
        harness
            .state()
            .caches
            .log(&root)
            .is_some_and(|cs| cs.iter().any(|c| c.id == selected)),
        "and it names a commit the refreshed log actually holds"
    );
    // The pane followed: it describes the successor's change, and no longer the
    // dropped commit's file. Only the changed-files pane paints paths, so this
    // is the pane's own text.
    assert_painted(&harness, "feature-3.txt");
    assert_not_painted(&harness, "feature-2.txt");
}

/// A rewrite that git refuses is reported as a FAILURE, and the selection stays
/// exactly where it was — a rewrite that moved nothing must not leave the pane
/// pointing at a commit the app believes was re-created.
///
/// The trigger is a worktree that turns dirty after the preflight was built, so
/// the refusal comes from git through the production dispatch path rather than
/// from a menu gate: `git rebase -i` refuses to start, and nothing is replayed.
/// (A content conflict is not reachable for a reword at all — no content
/// changes, so every patch re-applies to the parent it was written against. The
/// reachable failure is git declining the replay, and that is what this drives.)
#[test]
fn a_reword_that_git_refuses_is_reported_as_a_failure_and_moves_nothing() {
    let (_tmp, project, repo, target, _descendant) = history_to_rewrite();
    let mut harness = log_harness_over(project);
    let root = harness
        .state()
        .selected_root
        .clone()
        .expect("the log selected a repository");
    let head_before = git(&repo, &["rev-parse", "feature"]).trim().to_string();

    // The developer is looking at the commit they are about to reword.
    harness
        .get_by_label(&row_label(&target, "feature-2"))
        .click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(target.as_str()),
        "the row under the edit is the commit being reworded"
    );

    open_reword_preflight(
        &mut harness,
        &target,
        "feature-2",
        "feature: the second one",
    );
    // The worktree goes dirty between the briefing and the confirm — the menu
    // read it clean, and this is the gap a gate can never close on its own. A
    // TRACKED file: an untracked one does not stop a rebase, so it would not be
    // a refusal at all.
    let tracked = repo.join("feature-3.txt");
    let mut content = std::fs::read_to_string(&tracked).unwrap();
    content.push_str("an uncommitted edit\n");
    std::fs::write(&tracked, content).unwrap();
    harness.get_by_label("Reword this commit").click();
    wait_for(&mut harness, |s| {
        s.ui.activity
            .entries
            .iter()
            .any(|e| e.kind == turbogit_app::activity::ActivityKind::Error)
    });
    settle(&mut harness);

    // Reported as a failure, by name, with git's own reason.
    let failure = harness
        .state()
        .ui
        .activity
        .entries
        .last()
        .expect("an activity entry");
    assert_eq!(failure.kind, turbogit_app::activity::ActivityKind::Error);
    assert!(
        failure
            .message
            .starts_with(&format!("Reword commit {}:", short(&target))),
        "the failure names the verb and the commit it belongs to: {:?}",
        failure.message
    );
    // What the failure carries is git's VERDICT, not git's words: the engine runs
    // `rebase -i` through `Command::status()`, so the report is the non-zero exit
    // and the app's own summary. The verb and the commit are named either way,
    // which is what makes this a failure of a known operation rather than a bare
    // error in a feed.
    assert!(
        failure.message.contains("git exited with code"),
        "and attributes the failure to git rather than leaving it bare: {:?}",
        failure.message
    );
    assert!(
        harness
            .state()
            .ui
            .activity
            .entries
            .iter()
            .all(|e| { e.kind != turbogit_app::activity::ActivityKind::Success }),
        "a refused rewrite never reads as a success"
    );

    // Nothing moved, and the selection did not follow anything.
    assert_eq!(
        git(&repo, &["rev-parse", "feature"]).trim(),
        head_before,
        "the branch is where it was"
    );
    assert!(
        git(&repo, &["log", "--format=%s", "feature"]).contains("feature-2"),
        "and the commit still says what it said"
    );
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(target.as_str()),
        "the selection stays on the commit the developer was looking at"
    );
    assert!(
        harness.state().ui.pending_reselect.is_none(),
        "and nothing is left waiting to move it after a rewrite that did not run"
    );
    // The log the developer is reading still describes the repository.
    wait_for(&mut harness, |s| {
        s.caches.log(&root).is_some_and(|cs| cs.len() == 4)
    });
    warm_logs(&mut harness);
    settle(&mut harness);
    assert_painted(&harness, "feature-2");
}

/// Cancelling is a decision not to drop anything, and the repository has to say
/// so: no ref written, no commit moved, and the pane still describing the commit
/// it was showing.
#[test]
fn cancelling_the_drop_preflight_changes_nothing() {
    let (_tmp, project, repo, target, _descendant) = history_to_rewrite();
    let mut harness = log_harness_over(project);
    let head_before = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let count_before = count_of(&repo, "feature");

    open_rewrite_preflight(&mut harness, &target, "feature-2");
    harness.get_by_label("Cancel").click();
    settle(&mut harness);

    assert!(harness.state().ui.dialog.is_none(), "the dialog went");
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]).trim(),
        head_before,
        "no commit moved"
    );
    assert_eq!(count_of(&repo, "feature"), count_before);
    assert_eq!(
        git_absent(
            &repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "refs/turbogit/preflight-backup"
            ]
        ),
        None,
        "and no backup ref was written for a rewrite that never ran"
    );
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(target.as_str()),
        "the selection is still the commit the developer was looking at"
    );
    assert_painted(&harness, "feature-2");
}

// --- Cycle 5: guardrails — protected branch / dirty worktree -------------------
#[test]
fn actions_are_blocked_with_an_explanation_on_protected_branches() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    // main is protected by the default settings and currently checked out:
    // revert must be gated on its own item, not explained in a box elsewhere.
    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    assert_menu_item_gated(&mut harness, "Copy hash", "Revert commit");
    click_menu_item(&mut harness, "Copy hash", "Revert commit");
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
    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    assert_menu_item_gated(&mut harness, "Copy hash", "Cherry-pick to…");
    click_menu_item(&mut harness, "Copy hash", "Cherry-pick to…");
    settle(&mut harness);
    assert!(
        harness.state().ui.dialog.is_none(),
        "a blocked cherry-pick must not open the branch picker"
    );
}
