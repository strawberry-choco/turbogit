//! The interactive rebase editor, ported from the egui
//! `interactive_rebase_editor` suite: the plan rows and verb chips, mouse and
//! keyboard reordering, the drag, the preview, the raw todo, the cautions
//! rail, and the backup-backed start.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nsh_testing::Headless;
use test_support::RecordingExecutor;
use test_support::git_seed::git;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, RebaseEditorTab};
use turbogit_domain::model::{RebaseAction, RebasePlanEntry, VcsSettings};
use turbogit_engine::cli::CliExecutor;

mod support;

fn plan_repo(project: &support::Project, name: &str) -> PathBuf {
    let repo = support::seeded_repo(project.path(), name);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    support::commit_into(&repo, "a.txt", "feature-1");
    support::commit_into(&repo, "b.txt", "feature-2");
    support::commit_into(&repo, "c.txt", "feature-3");
    repo
}

fn recorder() -> Arc<RecordingExecutor> {
    Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })))
}

fn log_pairs(repo: &Path) -> Vec<(String, String)> {
    git(repo, &["log", "main..HEAD", "--format=%H%x00%s"])
        .lines()
        .filter_map(|line| line.split_once('\u{0}'))
        .map(|(id, subject)| (id.to_string(), subject.to_string()))
        .collect()
}

fn mount(
    project: &support::Project,
    roots: &[PathBuf],
    exec: Arc<RecordingExecutor>,
    seed: impl FnOnce(&mut AppState) + Send + 'static,
) -> Headless {
    let project = project.path().to_path_buf();
    let roots = roots.to_vec();
    let mut app = Headless::start(support::app_config(move || {
        let mut state = AppState::for_roots(&project, &roots)
            .with_executor(exec)
            .with_settings(VcsSettings::default());
        seed(&mut state);
        state.refresh(Affected::All);
        state.drain_events();
        state
    }))
    .expect("the shell mounts");
    app.first_render().expect("the first frame paints");
    app
}

fn open_editor(project: &support::Project, repo: &Path, exec: Arc<RecordingExecutor>) -> Headless {
    let oldest = log_pairs(repo)
        .last()
        .map(|(id, _)| id.clone())
        .expect("a commit past main");
    let roots = vec![repo.to_path_buf()];
    let mut app = mount(project, &roots, exec, move |s| {
        let root = s.multi.roots[0].id.clone();
        s.open_interactive_rebase(&root, &oldest);
    });
    app.wait_for("id:dialog:interactive-rebase")
        .expect("the editor paints");
    app.wait_for("text:feature-1").expect("the plan rows paint");
    app
}

fn recorded_plan(exec: &RecordingExecutor) -> Vec<RebasePlanEntry> {
    exec.recorded()
        .into_iter()
        .find_map(|call| match call {
            test_support::RecordedCall::RebaseInteractive { plan, .. } => Some(plan),
            _ => None,
        })
        .expect("an interactive plan was dispatched")
}

#[test]
fn the_editor_paints_tabs_plan_rows_chips_and_moves() {
    let project = support::Project::new();
    let repo = plan_repo(&project, "alpha");
    let app = open_editor(&project, &repo, recorder());
    for tab in ["Plan", "Preview", "Log"] {
        assert!(support::shows_part(&app, tab), "the {tab} tab paints");
    }
    for subject in ["feature-1", "feature-2", "feature-3"] {
        assert!(support::shows_part(&app, subject));
    }
    for chip in [
        "pick",
        "edit",
        "reword",
        "squash",
        "fixup",
        "drop",
        "update-ref",
    ] {
        assert!(support::shows_part(&app, chip), "the {chip} chip paints");
    }
    assert!(support::shows_part(&app, "↑"));
    assert!(support::shows_part(&app, "↓"));
}

#[test]
fn a_chip_retargets_its_row() {
    let project = support::Project::new();
    let repo = plan_repo(&project, "alpha");
    let exec = recorder();
    let mut app = open_editor(&project, &repo, exec.clone());
    app.press("id:rebase-editor:chip:squash >> nth=0")
        .expect("the squash chip");
    app.press("id:dialog:interactive-rebase:action")
        .expect("the start action");
    assert!(test_support::wait_until(10_000, || {
        exec.recorded()
            .iter()
            .any(|call| matches!(call, test_support::RecordedCall::RebaseInteractive { .. }))
    }));
    let plan = recorded_plan(&exec);
    assert_eq!(plan[0].action, RebaseAction::Squash, "plan: {plan:?}");
}

#[test]
fn a_chip_retargets_its_row_and_the_move_buttons_reorder() {
    let project = support::Project::new();
    let repo = plan_repo(&project, "alpha");
    let exec = recorder();
    let mut app = open_editor(&project, &repo, exec.clone());
    app.press("id:rebase-editor:chip:squash >> nth=0")
        .expect("the squash chip");
    app.press("id:rebase-editor:down >> nth=0")
        .expect("the down move");
    app.wait_until("the plan reorders", Duration::from_secs(5), |screen| {
        let texts = screen.texts();
        let f2 = texts.iter().position(|t| *t == "feature-2");
        let f1 = texts.iter().position(|t| *t == "feature-1");
        matches!((f2, f1), (Some(a), Some(b)) if a < b)
    })
    .expect("feature-1 moved below feature-2");
    app.press("id:dialog:interactive-rebase:action")
        .expect("the start action");
    assert!(test_support::wait_until(10_000, || {
        exec.recorded()
            .iter()
            .any(|call| matches!(call, test_support::RecordedCall::RebaseInteractive { .. }))
    }));
    let plan = recorded_plan(&exec);
    let order: Vec<&str> = plan.iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(order, ["feature-2", "feature-1", "feature-3"]);
    assert_eq!(
        plan[1].action,
        RebaseAction::Squash,
        "the action stayed with its commit"
    );
}

#[test]
#[ignore = "nsh headless drag starts no pan on a virtual-list row; the macOS lane drives the mouse drag"]
fn dragging_a_row_onto_another_reorders_the_plan() {
    let project = support::Project::new();
    let repo = plan_repo(&project, "alpha");
    let mut app = open_editor(&project, &repo, recorder());
    app.drag(
        "id:rebase-editor:row >> nth=1",
        nsh_core::geometry::point(nsh_core::Points(0.0), nsh_core::Points(26.0)),
    )
    .expect("the row hears the drag");
    app.wait_until("the dragged row moves", Duration::from_secs(5), |screen| {
        let texts = screen.texts();
        let f3 = texts.iter().position(|t| *t == "feature-3");
        let f2 = texts.iter().position(|t| *t == "feature-2");
        matches!((f3, f2), (Some(a), Some(b)) if a < b)
    })
    .expect("the centre row swapped places with the row below it");
}

#[test]
fn keyboard_shortcuts_move_and_retarget_the_selected_row() {
    let project = support::Project::new();
    let repo = plan_repo(&project, "alpha");
    let exec = recorder();
    let mut app = open_editor(&project, &repo, exec.clone());
    app.press("id:rebase-editor:row >> nth=0")
        .expect("the first row takes the selection");
    app.press_key("shift+down").expect("the move key");
    app.press_key("s").expect("the squash key");
    app.wait_until(
        "the plan follows the keys",
        Duration::from_secs(5),
        |screen| {
            let texts = screen.texts();
            let f2 = texts.iter().position(|t| *t == "feature-2");
            let f1 = texts.iter().position(|t| *t == "feature-1");
            matches!((f2, f1), (Some(a), Some(b)) if a < b)
        },
    )
    .expect("shift+down moved the selection");
    app.press("id:dialog:interactive-rebase:action")
        .expect("the start action");
    assert!(test_support::wait_until(10_000, || {
        exec.recorded()
            .iter()
            .any(|call| matches!(call, test_support::RecordedCall::RebaseInteractive { .. }))
    }));
    let plan = recorded_plan(&exec);
    let order: Vec<&str> = plan.iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(order, ["feature-2", "feature-1", "feature-3"]);
    assert_eq!(
        plan[1].action,
        RebaseAction::Squash,
        "the key retargeted the selection that moved with it"
    );
}

fn plan_entry(action: RebaseAction, commit: &str, subject: &str) -> RebasePlanEntry {
    RebasePlanEntry {
        action,
        commit: commit.to_string(),
        subject: subject.to_string(),
        message: None,
    }
}

#[test]
fn the_preview_tab_shows_the_result_sequence_counts_and_folds() {
    let project = support::Project::new();
    let repo = plan_repo(&project, "alpha");
    let mut app = mount(&project, std::slice::from_ref(&repo), recorder(), |s| {
        s.ui.dlg.rebase_plan = Some(vec![
            plan_entry(RebaseAction::Pick, "a91c3f7aaa", "feat: dry-run mode"),
            plan_entry(RebaseAction::Fixup, "f7c1d08bbb", "wip: trim checks"),
            plan_entry(RebaseAction::Squash, "3a90be2ccc", "wip: rename hooks"),
            plan_entry(RebaseAction::Drop, "7d10b4eddd", "test: redundant case"),
            plan_entry(RebaseAction::Pick, "f4e2a91eee", "refactor: split"),
        ]);
        s.ui.dlg.rebase_tab = RebaseEditorTab::Plan;
        s.ui.dialog = Some(Dialog::InteractiveRebase);
    });
    app.wait_for("id:dialog:interactive-rebase").unwrap();
    app.wait_for("text:feat: dry-run mode")
        .expect("the seeded plan rows paint");
    app.press("id:rebase-editor:tab-Preview")
        .expect("the preview tab");
    app.wait_for("text:5 → 2 COMMITS")
        .expect("the result count paints");
    assert!(support::shows_part(
        &app,
        "f7c1d08 + 3a90be2 folded into a91c3f7"
    ));
    assert!(support::shows_part(&app, "1 commit dropped"));
    assert!(support::shows_part(&app, "feat: dry-run mode"));
    assert!(support::shows_part(&app, "refactor: split"));
    assert!(!support::shows_part(&app, "test: redundant case"));
}

#[test]
fn editing_the_raw_todo_reparses_into_the_plan() {
    let project = support::Project::new();
    let repo = plan_repo(&project, "alpha");
    let mut app = open_editor(&project, &repo, recorder());
    app.press("id:rebase-editor:tab-Log").expect("the log tab");
    app.wait_for("id:rebase-editor:todo-input").unwrap();
    app.paste(
        "id:rebase-editor:todo-input",
        &nsh_core::PasteEvent::new("\ndrop abc1234 extra commit"),
    )
    .expect("the todo takes the pasted line");
    app.press("id:rebase-editor:tab-Plan")
        .expect("the plan tab");
    app.wait_for("text:extra commit")
        .expect("the appended line joins the plan");
    let texts = app.screen().texts();
    let extra = texts.iter().position(|t| *t == "extra commit").unwrap();
    let last = texts.iter().position(|t| *t == "feature-3").unwrap();
    assert!(last < extra, "the parsed line follows the original plan");
}

#[test]
fn the_rail_names_computed_cautions_and_the_recovery_ref() {
    let project = support::Project::new();
    let repo = plan_repo(&project, "alpha");
    git(&repo, &["config", "user.name", "Other"]);
    support::commit_into(&repo, "d.txt", "feature-4");
    let app = open_editor(&project, &repo, recorder());
    assert!(support::shows_part(&app, "CAUTIONS"));
    assert!(
        support::shows_part(&app, "Mixed committer identities (2 authors)"),
        "the rail names the mixed identities"
    );
    assert!(support::shows_part(&app, "RECOVERY"));
    assert!(support::shows_part(&app, "refs/turbogit/preflight-backup"));
}

#[test]
fn the_rail_lists_the_affected_repos() {
    let project = support::Project::new();
    let focused = plan_repo(&project, "alpha");
    let sibling = plan_repo(&project, "beta");
    let oldest = log_pairs(&focused).last().unwrap().0.clone();
    let mut app = mount(&project, &[focused, sibling], recorder(), move |s| {
        let root = s.multi.roots[0].id.clone();
        s.open_interactive_rebase(&root, &oldest);
    });
    app.wait_for("id:rebase-editor:affected").unwrap();
    assert!(support::shows_part(&app, "AFFECTED REPOS"));
    assert!(support::shows_part(&app, "beta"));
    assert!(support::shows_part(&app, "SHORTCUTS"));
}

#[test]
fn the_footer_estimates_and_start_dispatches_the_backup_backed_plan() {
    let project = support::Project::new();
    let repo = plan_repo(&project, "alpha");
    let exec = recorder();
    let pairs = log_pairs(&repo);
    let oldest = pairs.last().unwrap().0.clone();
    let mut app = mount(
        &project,
        std::slice::from_ref(&repo),
        exec.clone(),
        move |s| {
            let root = s.multi.roots[0].id.clone();
            s.open_interactive_rebase(&root, &oldest);
        },
    );
    app.wait_for("id:rebase-editor:footer").unwrap();
    assert!(support::shows_part(&app, "Ready to rebase 3 commits"));
    let pre_head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    app.press("id:dialog:interactive-rebase:action")
        .expect("the start action");
    assert!(test_support::wait_until(10_000, || {
        exec.recorded()
            .iter()
            .any(|call| matches!(call, test_support::RecordedCall::RebaseInteractive { .. }))
    }));
    assert_eq!(
        git(&repo, &["rev-parse", "refs/turbogit/preflight-backup"])
            .trim()
            .to_string(),
        pre_head,
        "the backup ref names the pre-rebase state"
    );
    app.wait_until("the editor closes", Duration::from_secs(5), |screen| {
        !screen
            .matches("id:dialog:interactive-rebase")
            .unwrap_or(false)
    })
    .expect("the editor closes after the start");
}
