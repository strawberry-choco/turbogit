//! The Rebase dialog, ported from the egui `rebase_dialog` suite: the onto
//! picker, the modes, the option rows, the dispatch each mode makes, the
//! cross-repo banner, and the protected-branch gate.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nsh_testing::Headless;
use test_support::RecordingExecutor;
use test_support::git_seed::git;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RebaseMode, VcsSettings};
use turbogit_engine::cli::CliExecutor;

mod support;

fn rebase_repo(project: &support::Project, name: &str) -> PathBuf {
    let repo = support::seeded_repo(project.path(), name);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    support::commit_into(&repo, "a.txt", "feature-1");
    support::commit_into(&repo, "b.txt", "feature-2");
    repo
}

fn recorder() -> Arc<RecordingExecutor> {
    Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })))
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
        state.drain_events();
        seed(&mut state);
        state.refresh(Affected::All);
        state.drain_events();
        state
    }))
    .expect("the shell mounts");
    app.first_render().expect("the first frame paints");
    app
}

fn open_rebase(app: &mut Headless, mode: RebaseMode) {
    let _ = mode;
    app.wait_for("id:dialog:rebase")
        .expect("the rebase dialog paints");
}

fn choose_onto(app: &mut Headless, branch: &str) {
    app.press("id:rebase:onto").expect("the onto select opens");
    app.wait_for(&format!("text:{branch}"))
        .expect("the branch item paints");
    app.press(&format!("text:{branch}"))
        .expect("the branch is chosen");
}

fn mounted_rebase(
    project: &support::Project,
    roots: &[PathBuf],
    exec: Arc<RecordingExecutor>,
    mode: RebaseMode,
    seed: impl FnOnce(&mut AppState) + Send + 'static,
) -> Headless {
    let mode_for_seed = mode;
    let mut app = mount(project, roots, exec, move |s| {
        s.ui.dlg.rebase_mode = mode_for_seed;
        let root = s.multi.roots[0].id.clone();
        s.open_dialog(Dialog::Rebase, Some(std::slice::from_ref(&root)));
        seed(s);
    });
    open_rebase(&mut app, mode);
    app
}

#[test]
fn the_dialog_paints_the_modes_options_and_start_verb() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let exec = recorder();
    let app = mounted_rebase(
        &project,
        std::slice::from_ref(&repo),
        exec,
        RebaseMode::Interactive,
        |_| {},
    );
    for label in [
        "ONTO BRANCH",
        "MODE",
        "Interactive",
        "Standard",
        "Autosquash",
        "OPTIONS",
        "Autosquash fixup commits",
        "Update branches (--update-refs)",
        "Keep empty commits",
    ] {
        assert!(
            support::shows_part(&app, label),
            "the dialog paints {label}"
        );
    }
    assert!(
        app.screen()
            .matches("id:dialog:rebase:action")
            .unwrap_or(false)
    );
    assert!(
        support::shows_part(&app, "Start interactive rebase"),
        "the interactive mode names its verb"
    );
}

#[test]
fn choosing_an_onto_branch_lists_the_commits_to_rebase() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let exec = recorder();
    let mut app = mounted_rebase(
        &project,
        std::slice::from_ref(&repo),
        exec,
        RebaseMode::Interactive,
        |_| {},
    );
    assert!(!support::shows_part(&app, "COMMITS TO REBASE"));
    choose_onto(&mut app, "main");
    app.wait_for("id:rebase:commits").expect("the plan lands");
    assert!(support::shows_part(&app, "2 COMMITS TO REBASE"));
    assert!(support::shows_part(&app, "feature-1"));
    assert!(support::shows_part(&app, "feature-2"));
}

#[test]
fn the_mode_rows_change_the_start_verb() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let exec = recorder();
    let mut app = mounted_rebase(
        &project,
        std::slice::from_ref(&repo),
        exec,
        RebaseMode::Interactive,
        |_| {},
    );
    app.press("id:rebase:mode-Standard").expect("the mode row");
    app.wait_for("text:Start rebase")
        .expect("the standard verb paints");
    app.press("id:rebase:mode-Autosquash")
        .expect("the mode row");
    app.wait_for("text:Start autosquash rebase")
        .expect("the autosquash verb paints");
    app.press("id:rebase:mode-Interactive")
        .expect("the mode row");
    app.wait_for("text:Start interactive rebase")
        .expect("the interactive verb returns");
}

#[test]
fn interactive_mode_dispatches_the_oldest_first_plan() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let exec = recorder();
    let mut app = mounted_rebase(
        &project,
        std::slice::from_ref(&repo),
        exec.clone(),
        RebaseMode::Interactive,
        |_| {},
    );
    choose_onto(&mut app, "main");
    app.wait_for("id:rebase:commits").unwrap();
    app.press("id:dialog:rebase:action")
        .expect("the start action");
    assert!(
        test_support::wait_until(10_000, || {
            exec.recorded()
                .iter()
                .any(|call| matches!(call, test_support::RecordedCall::RebaseInteractive { .. }))
        }),
        "an interactive plan was dispatched"
    );
    let plans: Vec<_> = exec
        .recorded()
        .into_iter()
        .filter_map(|call| match call {
            test_support::RecordedCall::RebaseInteractive { plan, .. } => Some(plan),
            _ => None,
        })
        .collect();
    assert_eq!(plans.len(), 1);
    let subjects: Vec<&str> = plans[0].iter().map(|e| e.subject.as_str()).collect();
    assert_eq!(subjects, ["feature-1", "feature-2"]);
    assert!(
        !exec
            .recorded()
            .iter()
            .any(|call| matches!(call, test_support::RecordedCall::Rebase { .. })),
        "no plain rebase escaped"
    );
}

#[test]
fn standard_and_autosquash_modes_dispatch_their_flags() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let exec = recorder();
    let mut app = mounted_rebase(
        &project,
        std::slice::from_ref(&repo),
        exec.clone(),
        RebaseMode::Standard,
        |s| s.ui.dlg.rebase_update_refs = true,
    );
    choose_onto(&mut app, "main");
    app.press("id:dialog:rebase:action").unwrap();
    assert!(test_support::wait_until(10_000, || {
        exec.recorded()
            .iter()
            .any(|call| matches!(call, test_support::RecordedCall::Rebase { onto, .. } if onto == "main"))
    }));
    let rebases: Vec<_> = exec
        .recorded()
        .into_iter()
        .filter_map(|call| match call {
            test_support::RecordedCall::Rebase { onto, opts, .. } => Some((onto, opts)),
            _ => None,
        })
        .collect();
    assert_eq!(rebases.len(), 1);
    assert!(rebases[0].1.update_refs, "the option reached the dispatch");

    let project = support::Project::new();
    let repo = rebase_repo(&project, "beta");
    let exec = recorder();
    let mut app = mounted_rebase(
        &project,
        std::slice::from_ref(&repo),
        exec.clone(),
        RebaseMode::Autosquash,
        |_| {},
    );
    choose_onto(&mut app, "main");
    app.press("id:dialog:rebase:action").unwrap();
    assert!(test_support::wait_until(10_000, || {
        exec.recorded()
            .iter()
            .any(|call| matches!(call, test_support::RecordedCall::Rebase { opts, .. } if opts.autosquash))
    }));
}

#[test]
fn the_cross_repo_banner_names_the_rewrite() {
    let project = support::Project::new();
    let focused = rebase_repo(&project, "alpha");
    let sibling = rebase_repo(&project, "beta");
    let exec = recorder();
    let mut app = mounted_rebase(
        &project,
        &[focused, sibling],
        exec,
        RebaseMode::Interactive,
        |_| {},
    );
    choose_onto(&mut app, "main");
    app.wait_for("id:rebase:commits").unwrap();
    app.wait_for("id:rebase:banner")
        .expect("the sibling banner paints");
    assert!(support::shows_part(
        &app,
        "Rewrites 2 commits across 2 repos"
    ));
}

#[test]
fn a_protected_branch_blocks_the_start() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let exec = recorder();
    let mut app = mounted_rebase(
        &project,
        std::slice::from_ref(&repo),
        exec.clone(),
        RebaseMode::Interactive,
        |s| s.settings.protected_branch_patterns = vec!["feature".to_string()],
    );
    choose_onto(&mut app, "main");
    app.wait_for("id:rebase:commits").unwrap();
    app.wait_for("text:/protected branch/")
        .expect("the blocker is stated");
    assert!(
        !app.screen()
            .enabled("id:dialog:rebase:action")
            .unwrap_or(true),
        "the start action is withheld"
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        exec.recorded().is_empty(),
        "a protected branch dispatched nothing: {:?}",
        exec.recorded()
    );
}
