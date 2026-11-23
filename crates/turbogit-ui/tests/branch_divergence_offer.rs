//! The divergence offer, ported from the egui `branch_divergence_offer`
//! suite: both sets named with their root identifiers, the two controls,
//! and the acceptance that clears the offer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use support::{Project, for_roots_seeded, shows_part};
use test_support::git_seed::git;
use turbogit_app::state::{BranchDivergence, DivergentRoot};
use turbogit_domain::model::RootId;

mod support;

fn alpha_and_beta(project: &Project) -> (PathBuf, PathBuf) {
    let alpha = support::seeded_repo(project.path(), "alpha");
    let beta = support::seeded_repo(project.path(), "beta");
    (alpha, beta)
}

fn offer(alpha: &Path, beta: &Path) -> BranchDivergence {
    let root = |path: &Path| RootId(Arc::from(path));
    BranchDivergence {
        record_id: 7,
        operation: "Cascade merge".to_string(),
        branch: "main".to_string(),
        changed: vec![DivergentRoot {
            root: root(alpha),
            name: "alpha".to_string(),
            branch: "main".to_string(),
            reason: None,
        }],
        failed: vec![DivergentRoot {
            root: root(beta),
            name: "beta".to_string(),
            branch: "main".to_string(),
            reason: Some("conflict".to_string()),
        }],
    }
}

fn offered(project: &Project) -> support::Mounted {
    let (alpha, beta) = alpha_and_beta(project);
    let held = offer(&alpha, &beta);
    let mut app = for_roots_seeded(project.path(), &[alpha, beta], move |s| {
        s.ui.branch_divergence = Some(held.clone());
    });
    app.first_render().unwrap();
    app.wait_for("id:divergence:offer").unwrap();
    app
}

#[test]
fn the_offer_names_both_sets_and_the_rollback_verb() {
    let project = Project::new();
    let app = offered(&project);
    assert!(
        shows_part(&app, "Cascade merge did not land in 1 of 2."),
        "the summary counts the sets"
    );
    assert!(
        shows_part(&app, "beta — still on main · conflict"),
        "the failure names its root and reason"
    );
    assert!(
        shows_part(&app, "alpha — now on main"),
        "the success names its root"
    );
    assert!(
        app.screen().shows("Roll back 1 repository"),
        "the rollback verb counts the succeeded repositories"
    );
    assert!(app.screen().shows("Keep divergent"), "the decline paints");
}

#[test]
fn accepting_clears_the_offer() {
    let project = Project::new();
    let mut app = offered(&project);
    app.press("text:Roll back 1 repository").unwrap();
    app.wait_until(
        "the offer clears",
        std::time::Duration::from_secs(10),
        |screen| !screen.matches("id:divergence:offer").unwrap_or(false),
    )
    .unwrap();
}

#[test]
fn declining_leaves_the_offer_cleared_but_keeps_the_branches() {
    let project = Project::new();
    let mut app = offered(&project);
    app.press("text:Keep divergent").unwrap();
    app.wait_until(
        "the offer clears",
        std::time::Duration::from_secs(10),
        |screen| !screen.matches("id:divergence:offer").unwrap_or(false),
    )
    .unwrap();
}

#[test]
fn an_offer_whose_roots_left_the_project_stops_painting() {
    let project = Project::new();
    let (alpha, _beta) = alpha_and_beta(&project);
    let vanished = project.path().join("vanished");
    std::fs::create_dir_all(&vanished).unwrap();
    let held = BranchDivergence {
        record_id: 7,
        operation: "Cascade merge".to_string(),
        branch: "main".to_string(),
        changed: vec![DivergentRoot {
            root: RootId(Arc::from(alpha.as_path())),
            name: "alpha".to_string(),
            branch: "main".to_string(),
            reason: None,
        }],
        failed: vec![DivergentRoot {
            root: RootId(Arc::from(vanished.as_path())),
            name: "vanished".to_string(),
            branch: "main".to_string(),
            reason: Some("conflict".to_string()),
        }],
    };
    let mut app = for_roots_seeded(project.path(), std::slice::from_ref(&alpha), move |s| {
        s.ui.branch_divergence = Some(held.clone());
    });
    app.first_render().unwrap();
    app.wait_until(
        "a stale offer paints nothing",
        std::time::Duration::from_secs(10),
        |screen| !screen.matches("id:divergence:offer").unwrap_or(false),
    )
    .unwrap();
    let _ = git(&alpha, &["status"]);
}
