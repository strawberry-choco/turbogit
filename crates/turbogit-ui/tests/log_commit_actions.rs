use std::path::{Path, PathBuf};
use std::time::Duration;

use egui_kittest::kittest::NodeT as _;
use egui_kittest::{Harness, kittest::Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::{commit, git, pin_eol_lf};
use test_support::harness::{
    assert_menu_item_gated, assert_not_painted, assert_painted, click_menu_item, filled_rects,
    painted_galleys, right_click_row, settle_quiet, shell_harness_over,
};
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::state::{AppState, Dialog, NewBranchBase, Tab};
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::Palette;

/// `git` WITHOUT asserting success, for reading a ref that is expected to be
/// absent, `git rev-parse --verify --quiet` exits non-zero when the ref does
/// not exist, and that non-zero exit is the answer.
///
/// The OPPOSITE contract from `git_seed::git`, which asserts success: the non-zero
/// exit IS the value being read.
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
    commit(dir, name, body, msg);
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
    pin_eol_lf(&alpha);
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
                mode: LogBatchMode::Replace,
            })
            .expect("send LogLoaded");
    }
    harness.step();
}

fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    log_harness_over(seed.project.clone())
}

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
                mode: LogBatchMode::Replace,
            })
            .expect("send LogLoaded");
    }
    state.drain_events();
    state.ui.tab = Tab::Log;

    let mut harness = shell_harness_over(
        state,
        egui::vec2(1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH, 800.0),
    );
    settle_quiet(&mut harness);
    harness
}

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

fn row_label(id: &str, subject: &str) -> String {
    format!("{} {subject}", short(id))
}

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

#[test]
fn the_commit_menu_offers_the_actions_the_details_pane_used_to() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));

    assert_painted(&harness, "Cherry-pick to…");
    assert_painted(&harness, "Revert commit");
    assert_painted(&harness, "New branch");
    assert_not_painted(&harness, "ACTIONS");
}

#[test]
fn revert_from_the_log_creates_a_revert_commit_visible_in_the_graph() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);
    harness
        .state_mut()
        .settings
        .protected_branch_patterns
        .clear();

    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    click_menu_item(&mut harness, "Copy hash", "Revert commit");
    settle_quiet(&mut harness);

    assert!(
        harness.state().ui.confirm.is_some(),
        "revert must ask for confirmation before dispatching"
    );
    harness.get_by_label("OK").click();
    settle_quiet(&mut harness);

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
    let subject = git(&seed.alpha, &["log", "-1", "--format=%s"]);
    assert!(
        subject.to_lowercase().starts_with("revert"),
        "HEAD must be the revert commit; got {subject:?}"
    );
}

#[test]
fn cherry_pick_opens_the_branch_picker_and_applies_onto_the_chosen_branch() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    click_menu_item(&mut harness, "Copy hash", "Cherry-pick to…");
    settle_quiet(&mut harness);

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
    sorted.first().expect("picker row").click();
    settle_quiet(&mut harness);

    wait_for(&mut harness, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.kind == turbogit_app::state::ToastKind::Success)
    });
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
    assert_eq!(
        git(&seed.alpha, &["symbolic-ref", "--short", "HEAD"]).trim(),
        "main"
    );
}

#[test]
fn new_branch_from_the_menu_creates_the_branch_at_the_right_clicked_commit() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);
    right_click_row(&mut harness, &row_label(&seed.c1, "alpha: first commit"));
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::NewBranch),
        "the action must open the new-branch dialog"
    );

    harness.state_mut().ui.dlg.new_branch_name = "from-the-log".into();
    settle_quiet(&mut harness);
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

#[test]
fn cancelling_the_new_branch_dialog_from_the_menu_creates_nothing() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);
    let branches_before = git(&seed.alpha, &["for-each-ref", "--format=%(refname:short)"]);

    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle_quiet(&mut harness);
    harness.state_mut().ui.dlg.new_branch_name = "should-never-exist".into();
    settle_quiet(&mut harness);

    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);

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

#[test]
fn the_start_from_row_reads_a_commit_as_a_short_reference_and_its_subject() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    right_click_row(&mut harness, &row_label(&seed.c1, "alpha: first commit"));
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle_quiet(&mut harness);

    assert_painted(&harness, &short(&seed.c1));
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

#[test]
fn the_branch_picker_offers_branches_while_the_base_is_a_commit() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);
    right_click_row(&mut harness, &row_label(&seed.c1, "alpha: first commit"));
    click_menu_item(&mut harness, "Copy hash", "New branch");
    settle_quiet(&mut harness);

    harness.get_by_label("Change…").click();
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dlg.new_branch_base,
        NewBranchBase::Commit(seed.c1.clone()),
        "opening the picker does not drop the commit the log named"
    );
    assert_painted(&harness, &short(&seed.c1));
    assert!(
        painted_galleys(&harness)
            .iter()
            .any(|g| g.text == "alpha: first commit" && g.color == Palette::INK_3),
        "subject included: the base stays readable while the picker is open"
    );

    picker_row(&harness, "feature").click();
    settle_quiet(&mut harness);
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

fn count_of(dir: &Path, rev: &str) -> usize {
    git(dir, &["rev-list", "--count", rev])
        .trim()
        .parse()
        .expect("a commit count")
}

fn history_to_rewrite() -> (TempDir, PathBuf, PathBuf, String, String) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("ws");
    std::fs::create_dir_all(&project).unwrap();
    let repo = test_support::git_seed::repo_with_history(&project, "alpha");
    let target = git(&repo, &["rev-parse", "HEAD~1"]).trim().to_string();
    let descendant = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    (tmp, project, repo, target, descendant)
}

fn open_rewrite_preflight(harness: &mut Harness<'_, AppState>, c1: &str, subject: &str) {
    right_click_row(harness, &row_label(c1, subject));
    click_menu_item(harness, "Copy hash", "Drop commit");
    settle_quiet(harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::RewritePreflight),
        "the item opens the preflight, not the rewrite"
    );
}

#[test]
fn confirming_the_drop_preflight_removes_one_commit_and_keeps_its_descendants() {
    let (_tmp, project, repo, target, _descendant) = history_to_rewrite();
    let mut harness = log_harness_over(project);
    let head_before = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let count_before = count_of(&repo, "feature");

    open_rewrite_preflight(&mut harness, &target, "feature-2");
    harness.get_by_label("Drop this commit").click();
    wait_for(&mut harness, |s| s.ui.toast.is_some());

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
    assert_eq!(
        std::fs::read_to_string(repo.join("feature-3.txt")).unwrap(),
        "3\n",
        "the kept commit's change is still on the branch"
    );
    assert!(
        !repo.join("feature-2.txt").exists(),
        "and the dropped commit's change is not"
    );
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

    warm_logs(&mut harness);
    settle_quiet(&mut harness);
    assert_painted(&harness, "3 shown");
    assert_painted(&harness, "feature-3");
}

fn open_reword_preflight(
    harness: &mut Harness<'_, AppState>,
    c1: &str,
    subject: &str,
    message: &str,
) {
    right_click_row(harness, &row_label(c1, subject));
    click_menu_item(harness, "Copy hash", "Reword commit");
    settle_quiet(harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::Reword),
        "the item opens the editor, not the rewrite"
    );
    harness.state_mut().ui.dlg.reword_message = message.to_owned();
    settle_quiet(harness);
    harness.get_by_label("Reword this commit").click();
    settle_quiet(harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::RewritePreflight),
        "and the editor's confirm opens the preflight, not the rewrite"
    );
}

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
    assert_eq!(
        std::fs::read_to_string(repo.join("feature-2.txt")).unwrap(),
        "2\n",
        "the reworded commit's own change is still in the working tree"
    );
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

    warm_logs(&mut harness);
    settle_quiet(&mut harness);
    assert_painted(&harness, "feature: the second one");
    assert!(
        !painted_galleys(&harness)
            .iter()
            .any(|g| g.text == "feature-2"),
        "the log no longer shows the old subject"
    );
}

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
    wait_for(&mut harness, |s| {
        s.caches.log(&root).is_some_and(|cs| {
            cs.iter()
                .any(|c| c.message.lines().next() == Some("feature: the second one"))
        })
    });
    settle_quiet(&mut harness);

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
    assert_painted(&harness, "feature-2.txt");
}

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
    wait_for(&mut harness, |s| {
        s.caches.log(&root).is_some_and(|cs| cs.len() == 3)
    });
    settle_quiet(&mut harness);

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
    assert_painted(&harness, "feature-3.txt");
    assert_not_painted(&harness, "feature-2.txt");
}

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

    harness
        .get_by_label(&row_label(&target, "feature-2"))
        .click();
    settle_quiet(&mut harness);
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
    settle_quiet(&mut harness);

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
    wait_for(&mut harness, |s| {
        s.caches.log(&root).is_some_and(|cs| cs.len() == 4)
    });
    warm_logs(&mut harness);
    settle_quiet(&mut harness);
    assert_painted(&harness, "feature-2");
}

#[test]
fn cancelling_the_drop_preflight_changes_nothing() {
    let (_tmp, project, repo, target, _descendant) = history_to_rewrite();
    let mut harness = log_harness_over(project);
    let head_before = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let count_before = count_of(&repo, "feature");

    open_rewrite_preflight(&mut harness, &target, "feature-2");
    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);

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

#[test]
fn actions_are_blocked_with_an_explanation_on_protected_branches() {
    let seed = seeded_repo();
    let mut harness = log_harness(&seed);

    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    assert_menu_item_gated(&mut harness, "Copy hash", "Revert commit");
    click_menu_item(&mut harness, "Copy hash", "Revert commit");
    settle_quiet(&mut harness);
    assert!(
        harness.state().ui.confirm.is_none(),
        "a blocked revert must never reach the confirmation gate"
    );

    std::fs::write(seed.alpha.join("a.txt"), "uncommitted\n").unwrap();
    harness
        .state_mut()
        .refresh(turbogit_app::root_caches::Affected::All);
    settle_quiet(&mut harness);
    right_click_row(&mut harness, &row_label(&seed.c2, "alpha: second commit"));
    assert_menu_item_gated(&mut harness, "Copy hash", "Cherry-pick to…");
    click_menu_item(&mut harness, "Copy hash", "Cherry-pick to…");
    settle_quiet(&mut harness);
    assert!(
        harness.state().ui.dialog.is_none(),
        "a blocked cherry-pick must not open the branch picker"
    );
}

fn reword_and_arm_undo(harness: &mut Harness<'static, AppState>, target: &str) {
    open_reword_preflight(harness, target, "feature-2", "feature: the second one");
    harness.get_by_label("Reword this commit").click();
    wait_for(harness, |s| s.ui.rewrite_undo.is_some());
}

#[test]
fn the_reword_success_notification_carries_an_undo_control() {
    let (_tmp, project, _repo, target, _tip) = history_to_rewrite();
    let mut harness = log_harness_over(project);

    reword_and_arm_undo(&mut harness, &target);

    assert_painted(&harness, "Commit message changed");
    assert_painted(&harness, "Undo");
    let undo_rect = harness.get_by_label("Undo").rect();
    assert!(
        filled_rects(&harness)
            .iter()
            .any(|(r, fill)| *fill == Palette::ACCENT && r.intersects(undo_rect)),
        "the Undo control is filled with the accent, not painted inert"
    );
}

#[test]
fn a_second_operation_withholds_the_reword_undo_with_the_reason() {
    let (_tmp, project, repo, target, _tip) = history_to_rewrite();
    let mut harness = log_harness_over(project);

    reword_and_arm_undo(&mut harness, &target);

    commit(&repo, "e.txt", "4\n", "a second operation");
    let root = harness.state().selected_root.clone().unwrap();
    harness
        .state_mut()
        .dispatch(turbogit_app::operation::Operation::Fetch { roots: vec![root] });
    wait_for(&mut harness, |s| {
        s.ui.rewrite_undo.as_ref().is_some_and(|o| o.withheld)
    });

    assert_painted(&harness, "Cannot undo commit message edit");
    assert_not_painted(&harness, "Commit message changed");
    assert_not_painted(&harness, "Undo");
    let reason_rect = painted_galleys(&harness)
        .iter()
        .find(|g| g.text == "Cannot undo commit message edit — the repository has changed since")
        .map(|g| g.rect)
        .expect("the withheld reason is painted");
    let row = egui::Rect::from_min_max(
        egui::Pos2::new(0.0, reason_rect.top()),
        egui::Pos2::new(reason_rect.right() + 600.0, reason_rect.bottom()),
    );
    assert!(
        !filled_rects(&harness)
            .iter()
            .any(|(r, fill)| *fill == Palette::ACCENT && r.intersects(row)),
        "a withheld undo paints no control at all, not an inert one"
    );
}

#[test]
fn clicking_undo_after_a_drop_restores_the_dropped_commit() {
    let (_tmp, project, repo, target, _tip) = history_to_rewrite();
    let mut harness = log_harness_over(project);

    open_rewrite_preflight(&mut harness, &target, "feature-2");
    harness.get_by_label("Drop this commit").click();
    wait_for(&mut harness, |s| s.ui.rewrite_undo.is_some());

    assert_painted(&harness, "Dropped commit");
    assert_painted(&harness, "Undo");
    harness.get_by_label("Undo").click();
    wait_for(&mut harness, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.message == "Undo drop")
    });

    let subjects = git(&repo, &["log", "--format=%s", "feature"]);
    assert!(
        subjects.contains("feature-2"),
        "the dropped commit is back: {subjects}"
    );
    assert!(
        git_absent(
            &repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "refs/turbogit/preflight-backup"
            ]
        )
        .is_none(),
        "and the spent backup is discarded"
    );
}

#[test]
fn an_undo_without_its_backup_reports_a_distinct_failure() {
    let (_tmp, project, repo, target, _tip) = history_to_rewrite();
    let mut harness = log_harness_over(project);

    open_rewrite_preflight(&mut harness, &target, "feature-2");
    harness.get_by_label("Drop this commit").click();
    wait_for(&mut harness, |s| s.ui.rewrite_undo.is_some());

    git(
        &repo,
        &["update-ref", "-d", "refs/turbogit/preflight-backup"],
    );

    harness.get_by_label("Undo").click();
    wait_for(&mut harness, |s| {
        s.ui.toast
            .as_ref()
            .is_some_and(|t| t.message.contains("Undo drop:"))
    });

    assert_painted(&harness, "no rewrite backup to restore");
    assert_not_painted(&harness, "Dropped commit");
    let subjects = git(&repo, &["log", "--format=%s", "feature"]);
    assert!(
        !subjects.contains("feature-2"),
        "and nothing was restored: {subjects}"
    );
}

#[test]
fn the_three_undo_outcomes_never_share_wording() {
    use turbogit_app::state::RewriteUndoVerb;
    use turbogit_ui::ui::rewrite_undo::{cannot_undo, success_title, undo_label};

    for verb in [RewriteUndoVerb::Drop, RewriteUndoVerb::Reword] {
        let success = success_title(verb, "abc1234");
        let impossible = cannot_undo(verb);
        let failed = undo_label(verb);
        assert_ne!(success, impossible, "{verb:?}");
        assert_ne!(success, failed, "{verb:?}");
        assert_ne!(impossible, failed, "{verb:?}");
    }
    for (a, b) in [
        (RewriteUndoVerb::Drop, RewriteUndoVerb::Reword),
        (RewriteUndoVerb::Reword, RewriteUndoVerb::Drop),
    ] {
        assert_ne!(success_title(a, "abc1234"), success_title(b, "def5678"));
        assert_ne!(cannot_undo(a), cannot_undo(b));
        assert_ne!(undo_label(a), undo_label(b));
    }
}
