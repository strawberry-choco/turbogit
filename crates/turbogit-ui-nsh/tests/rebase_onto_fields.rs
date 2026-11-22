//! The rebase `--onto` fields, ported from the egui `rebase_onto_fields`
//! suite: the pair appears together, the new base paints above the old, each
//! field owns its error, the confirm waits for two usable bases, and the
//! pair reaches git in git's own argument order.

use std::path::{Path, PathBuf};
use std::sync::Arc;

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

fn mount(
    project: &support::Project,
    repo: &Path,
    exec: Arc<RecordingExecutor>,
    seed: impl FnOnce(&mut AppState) + Send + 'static,
) -> Headless {
    let project = project.path().to_path_buf();
    let roots = vec![repo.to_path_buf()];
    let mut app = Headless::start(support::app_config(move || {
        let mut state = AppState::for_roots(&project, &roots)
            .with_executor(exec)
            .with_settings(VcsSettings::default());
        state.ui.dlg.rebase_mode = RebaseMode::Standard;
        state.ui.dlg.rebase_onto_enabled = true;
        seed(&mut state);
        let root = state.multi.roots[0].id.clone();
        state.open_dialog(Dialog::Rebase, Some(std::slice::from_ref(&root)));
        state.refresh(Affected::All);
        state.drain_events();
        state
    }))
    .expect("the shell mounts");
    app.first_render().expect("the first frame paints");
    app.wait_for("id:dialog:rebase")
        .expect("the rebase dialog paints");
    app
}

fn recorder() -> Arc<RecordingExecutor> {
    Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })))
}

#[test]
fn the_two_bases_hide_until_onto_is_enabled() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let mut app = mount(&project, &repo, recorder(), |s| {
        s.ui.dlg.rebase_onto_enabled = false;
    });
    assert!(!support::shows_part(&app, "NEW BASE"));
    assert!(!support::shows_part(&app, "OLD BASE"));
    assert!(support::shows_part(&app, "Custom base (--onto)"));
    app.press("id:rebase:option-Custom base (--onto)")
        .expect("the onto toggle");
    app.wait_for("text:NEW BASE").expect("the new base appears");
    assert!(support::shows_part(&app, "OLD BASE"));
    assert!(support::shows_part(
        &app,
        "superseded by the two base fields"
    ));
}

#[test]
fn the_new_base_paints_strictly_above_the_old_base() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let app = mount(&project, &repo, recorder(), |s| {
        s.ui.dlg.rebase_new_base = "main".into();
        s.ui.dlg.rebase_old_base = "HEAD~1".into();
    });
    let texts = app.screen().texts();
    let new_at = texts.iter().position(|t| *t == "NEW BASE").expect("new");
    let old_at = texts.iter().position(|t| *t == "OLD BASE").expect("old");
    assert!(new_at < old_at, "the new base paints above the old one");
}

#[test]
fn an_empty_new_base_errors_in_its_own_field_and_nowhere_else() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let app = mount(&project, &repo, recorder(), |s| {
        s.ui.dlg.rebase_old_base = "main".into();
    });
    assert!(support::shows_part(&app, "Select a new base"));
    assert!(
        !support::shows_part(&app, "Select an old base"),
        "the old base's own error stays out of the new base's field"
    );
}

#[test]
fn an_empty_old_base_errors_in_its_own_field_and_nowhere_else() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let app = mount(&project, &repo, recorder(), |s| {
        s.ui.dlg.rebase_new_base = "main".into();
    });
    assert!(support::shows_part(&app, "Select an old base"));
    assert!(!support::shows_part(&app, "Select a new base"));
}

#[test]
fn a_base_that_names_nothing_errors_with_the_missing_ref_message() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let app = mount(&project, &repo, recorder(), |s| {
        s.ui.dlg.rebase_new_base = "no-such-branch-or-hash".into();
        s.ui.dlg.rebase_old_base = "main".into();
    });
    assert!(support::shows_part(&app, "There is no such branch or hash"));
    assert!(!support::shows_part(&app, "Select an old base"));
}

#[test]
fn the_confirm_is_withheld_until_both_bases_are_usable() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let mut app = mount(&project, &repo, recorder(), |s| {
        s.ui.dlg.rebase_new_base = "main".into();
    });
    assert!(
        !app.screen()
            .enabled("id:dialog:rebase:action")
            .unwrap_or(true),
        "an empty old base withholds the confirm"
    );
    app.type_text("id:rebase:old-base", "main")
        .expect("the old base takes text");
    app.wait_until(
        "the confirm arms",
        std::time::Duration::from_secs(5),
        |screen| screen.enabled("id:dialog:rebase:action").unwrap_or(false),
    )
    .expect("two usable bases arm the confirm");
}

#[test]
fn the_dispatch_passes_both_bases_in_git_own_order() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    let exec = recorder();
    let mut app = mount(&project, &repo, exec.clone(), |s| {
        s.ui.dlg.rebase_new_base = "main".into();
        s.ui.dlg.rebase_old_base = "HEAD~1".into();
    });
    app.press("id:dialog:rebase:action")
        .expect("the start action");
    assert!(test_support::wait_until(10_000, || {
        exec.recorded()
            .iter()
            .any(|call| matches!(call, test_support::RecordedCall::Rebase { .. }))
    }));
    let calls: Vec<(String, Option<String>)> = exec
        .recorded()
        .into_iter()
        .filter_map(|call| match call {
            test_support::RecordedCall::Rebase { onto, opts, .. } => Some((onto, opts.onto)),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        vec![("HEAD~1".to_string(), Some("main".to_string()))]
    );
}

#[test]
fn onto_moves_only_the_commits_after_the_old_base() {
    let project = support::Project::new();
    let repo = rebase_repo(&project, "alpha");
    git(&repo, &["checkout", "-q", "main"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    support::commit_into(&repo, "s.txt", "side-1");
    git(&repo, &["checkout", "-q", "feature"]);
    let cutoff = git(&repo, &["rev-parse", "HEAD~1"]).trim().to_string();

    let mut app = mount(&project, &repo, recorder(), move |s| {
        s.ui.dlg.rebase_new_base = "side".into();
        s.ui.dlg.rebase_old_base = cutoff.clone();
    });
    app.press("id:dialog:rebase:action")
        .expect("the start action");
    assert!(
        test_support::wait_until(15_000, || {
            git(&repo, &["log", "--format=%s", "feature"])
                .lines()
                .count()
                == 3
        }),
        "the onto rebase lands: {:?}",
        git(&repo, &["log", "--format=%s", "feature"])
    );
    let subjects: Vec<String> = git(&repo, &["log", "--format=%s", "feature"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(subjects, ["feature-2", "side-1", "init"]);
}
