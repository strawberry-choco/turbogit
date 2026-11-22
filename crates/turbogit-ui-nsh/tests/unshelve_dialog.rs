//! Unshelve, ported from the egui `unshelve_dialog` suite: every root's
//! shelves in one picker, the remove option's captured default, and the
//! apply that keeps or destroys the shelf and git's copy.

use std::path::{Path, PathBuf};

use support::{Mounted, Project, for_roots_seeded};
use test_support::git_seed::git;
use turbogit_app::state::AppState;
use turbogit_domain::model::{RootId, Shelf, ShelfSource};

mod support;

fn repo(project: &Project, name: &str) -> PathBuf {
    support::seeded_repo(project.path(), name)
}

fn shelf(name: &str, root: &Path) -> Shelf {
    Shelf {
        name: name.to_string(),
        changes: Vec::new(),
        created_at: chrono::Utc::now(),
        root: RootId(root.into()),
        source: ShelfSource::Stash,
    }
}

fn stash(repo: &Path, message: &str) {
    std::fs::write(repo.join("README.md"), "shelved work\n").expect("dirty file");
    git(repo, &["stash", "push", "-q", "-m", message]);
}

fn unshelve_over(
    project: &Project,
    roots: &[PathBuf],
    seed: impl FnOnce(&mut AppState) + Send + 'static,
) -> Mounted {
    let mut app = for_roots_seeded(project.path(), roots, |s| {
        seed(s);
        s.open_unshelve_dialog();
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:unshelve")
        .expect("the unshelve dialog paints");
    app
}

fn exact(app: &Mounted, text: &str) -> bool {
    app.screen().texts().contains(&text)
}

#[test]
fn the_name_field_lists_every_roots_shelves() {
    let project = Project::new();
    let alpha = repo(&project, "alpha");
    let beta = repo(&project, "beta");
    let a = alpha.clone();
    let b = beta.clone();
    let app = unshelve_over(&project, &[alpha, beta], move |s| {
        s.ui.shelves.push(shelf("weather", &a));
        s.ui.shelves.push(shelf("tides", &b));
    });
    for text in ["weather", "tides", "ALPHA", "BETA"] {
        assert!(support::shows_part(&app, text), "{text} paints");
    }
}

#[test]
fn the_remove_option_is_off_every_time_the_dialog_opens() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let r = repo.clone();
    let app = unshelve_over(&project, std::slice::from_ref(&repo), move |s| {
        s.ui.shelves.push(shelf("weather", &r));
        s.ui.dlg.unshelve_remove = true;
    });
    assert!(exact(
        &app,
        "Remove successfully applied files from the shelf"
    ));
    assert!(exact(&app, "Unshelve"), "the keeping verb paints");
    assert!(
        !exact(&app, "Unshelve and Remove"),
        "the destructive name stays out while the option is off"
    );
}

#[test]
fn ticking_the_remove_option_renames_the_affirmative() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let r = repo.clone();
    let mut app = unshelve_over(&project, std::slice::from_ref(&repo), move |s| {
        s.ui.shelves.push(shelf("weather", &r));
    });
    app.press("id:unshelve:remove-option").expect("the option");
    app.wait_for("text:Unshelve and Remove")
        .expect("the affirmative renames");
    assert!(!exact(&app, "Unshelve"), "one affirmative, not two");
}

#[test]
fn unshelving_through_the_dialog_keeps_the_shelf_and_gits_copy() {
    let project = Project::new();
    let repo = repo(&project, "weather");
    stash(&repo, "weather");
    let r = repo.clone();
    let app = unshelve_over(&project, std::slice::from_ref(&repo), move |s| {
        s.ui.shelves.push(shelf("weather", &r));
    });
    let mut app = app;
    app.press("id:dialog:unshelve:action").expect("unshelve");
    assert!(
        test_support::wait_until(10_000, || {
            git(&repo, &["status", "--porcelain"]).contains("README.md")
        }),
        "the work lands back in the tree"
    );
    assert!(
        !git(&repo, &["stash", "list"]).trim().is_empty(),
        "the keeping option leaves git's copy"
    );
}

#[test]
fn ticking_the_option_and_confirming_destroys_the_shelf_and_gits_copy() {
    let project = Project::new();
    let repo = repo(&project, "weather");
    stash(&repo, "weather");
    let r = repo.clone();
    let mut app = unshelve_over(&project, std::slice::from_ref(&repo), move |s| {
        s.ui.shelves.push(shelf("weather", &r));
    });
    app.press("id:unshelve:remove-option").expect("the option");
    app.press("id:dialog:unshelve:action")
        .expect("unshelve and remove");
    assert!(
        test_support::wait_until(10_000, || {
            git(&repo, &["status", "--porcelain"]).contains("README.md")
                && git(&repo, &["stash", "list"]).trim().is_empty()
        }),
        "the work lands and git's copy goes: {}",
        git(&repo, &["stash", "list"])
    );
    let stored = turbogit_app::shelve_stash::load_shelves(project.path()).unwrap_or_default();
    assert!(stored.is_empty(), "the store drops the destroyed shelf");
}

#[test]
fn a_root_with_nothing_shelved_says_so_and_offers_no_verb() {
    let project = Project::new();
    let repo = repo(&project, "empty");
    let app = unshelve_over(&project, std::slice::from_ref(&repo), |_| {});
    assert!(support::shows_part(
        &app,
        "Nothing is shelved in the selected repositories"
    ));
    assert!(!exact(&app, "Unshelve"), "no affirmative is offered");
    assert!(exact(&app, "Cancel"), "the way out is always there");
}

#[test]
fn the_changelist_fields_are_absent_rather_than_inert() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let r = repo.clone();
    let app = unshelve_over(&project, std::slice::from_ref(&repo), move |s| {
        s.ui.shelves.push(shelf("weather", &r));
    });
    let painted = app.screen().texts().join(" ").to_lowercase();
    for absent in ["comment", "set active", "track context", "changelist"] {
        assert!(!painted.contains(absent), "`{absent}` is not painted");
    }
}
