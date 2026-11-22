//! The fleet history, ported from the egui `bulk_history` suite: the
//! recorded runs, the per-repository drill-down, and the rollback a
//! reversible run offers.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Mounted, Project, for_roots_seeded};
use test_support::git_seed::git;
use turbogit_app::bulk_history::{BulkRunRecord, RepoOutcome, RepoRecord};
use turbogit_app::bulk_ops::BulkOp;
use turbogit_app::state::AppState;

mod support;

fn repo(project: &Project, name: &str) -> PathBuf {
    support::seeded_repo(project.path(), name)
}

fn repo_record(root: &Path, name: &str, outcome: RepoOutcome) -> RepoRecord {
    RepoRecord {
        root: root.to_path_buf(),
        name: name.to_string(),
        prior_branch: None,
        created: false,
        outcome,
    }
}

fn record(id: u64, op: BulkOp, branch: &str, repos: Vec<RepoRecord>) -> BulkRunRecord {
    BulkRunRecord {
        id,
        at: 1_791_234_000_000,
        op,
        branch: branch.to_string(),
        repos,
        rolled_back: false,
    }
}

fn bulk_over(
    project: &Project,
    roots: &[PathBuf],
    seed: impl FnOnce(&mut AppState) + Send + 'static,
) -> Mounted {
    let mut app = for_roots_seeded(project.path(), roots, seed);
    app.first_render().expect("the shell mounts");
    app.press("id:toolbar:n5-bulk").expect("the bulk band");
    app.wait_for("id:route:bulk").expect("the bulk route");
    app
}

#[test]
fn the_history_list_renders_rows_and_details_drills_down_per_repo() {
    let project = Project::new();
    let alpha = repo(&project, "alpha");
    let ui = repo(&project, "ui");
    let alpha_for_seed = alpha.clone();
    let ui_for_seed = ui.clone();
    let mut app = bulk_over(&project, &[alpha.clone(), ui.clone()], move |s| {
        s.ui.bulk_history = vec![record(
            1,
            BulkOp::FetchAll,
            "",
            vec![
                repo_record(&alpha_for_seed, "alpha", RepoOutcome::Done),
                repo_record(
                    &ui_for_seed,
                    "ui",
                    RepoOutcome::Skipped {
                        reason: "offline remote".to_string(),
                    },
                ),
            ],
        )];
    });
    app.press("id:bulk:history-open").expect("open history");
    app.wait_for("id:bulk:history").expect("the history panel");
    assert!(support::shows_part(&app, "RECENT FLEET OPERATIONS"));
    assert!(support::shows_part(&app, "Fetch all · 2 repos"));
    assert!(support::shows_part(
        &app,
        "1 ok · 1 skipped (offline remote)"
    ));
    app.press("id:bulk:details:1").expect("details");
    app.wait_for("text:alpha · done").expect("the repo outcome");
    assert!(support::shows_part(&app, "ui · skipped (offline remote)"));
    assert!(
        !app.screen().matches("id:bulk:rollback:1").unwrap_or(true),
        "a fetch is not reversible"
    );
}

#[test]
fn rollback_is_offered_for_a_created_branch_and_undoes_it() {
    let project = Project::new();
    let alpha = repo(&project, "alpha");
    let ui = repo(&project, "ui");
    git(&alpha, &["checkout", "-q", "-b", "feature/x"]);
    let alpha_for_seed = alpha.clone();
    let ui_for_seed = ui.clone();
    let mut app = bulk_over(&project, &[alpha.clone(), ui.clone()], move |s| {
        let mut created = repo_record(&alpha_for_seed, "alpha", RepoOutcome::Done);
        created.prior_branch = Some("main".to_string());
        created.created = true;
        s.ui.bulk_history = vec![record(
            1,
            BulkOp::CreateBranch,
            "feature/x",
            vec![
                created,
                repo_record(
                    &ui_for_seed,
                    "ui",
                    RepoOutcome::Skipped {
                        reason: "dirty worktree".to_string(),
                    },
                ),
            ],
        )];
    });
    app.press("id:bulk:history-open").expect("open history");
    app.wait_for("text:Create branch feature/x · 2 repos")
        .expect("the record paints");
    app.press("id:bulk:rollback:1").expect("rollback");
    assert!(
        test_support::wait_until(10_000, || {
            git(&alpha, &["branch", "--show-current"]).trim() == "main"
                && !git(&alpha, &["branch", "--list", "feature/x"]).contains("feature/x")
        }),
        "the rollback restored main and deleted the created branch: {}",
        git(&alpha, &["branch", "--list"])
    );
}

#[test]
fn a_record_details_and_summary_are_the_same_run() {
    let project = Project::new();
    let alpha = repo(&project, "alpha");
    let ui = repo(&project, "ui");
    let alpha_for_seed = alpha.clone();
    let ui_for_seed = ui.clone();
    let mut app = bulk_over(&project, &[alpha, ui], move |s| {
        s.ui.bulk_history = vec![record(
            7,
            BulkOp::StashAll,
            "",
            vec![
                repo_record(&alpha_for_seed, "alpha", RepoOutcome::Done),
                repo_record(
                    &ui_for_seed,
                    "ui",
                    RepoOutcome::Skipped {
                        reason: "clean tree".to_string(),
                    },
                ),
            ],
        )];
    });
    app.press("id:bulk:history-open").expect("open history");
    app.wait_for("text:Stash all · 2 repos").unwrap();
    app.press("id:bulk:details:7").expect("details");
    app.wait_for("text:alpha · done").unwrap();
    app.press("id:bulk:details:7")
        .expect("the same details control folds");
    app.wait_until(
        "the details list closes",
        Duration::from_secs(5),
        |screen| !screen.texts().contains(&"alpha · done"),
    )
    .expect("a second details press folds the list");
}
