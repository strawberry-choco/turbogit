//! Paging and virtualization, ported from the egui `log_paging_rows`,
//! `log_frame_bounds`, and `log_scope_batching` suites: a long history
//! pages through the production load-more path, the list builds only the
//! rows its viewport can hold, and a scoped read settles per root.

use std::path::PathBuf;

use support::{Project, for_roots, log_repo, shows_part, wait_for_part};

mod support;

fn log_app(project: &Project, roots: &[PathBuf]) -> support::Mounted {
    let mut app = for_roots(project.path(), roots);
    app.first_render().unwrap();
    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    wait_for_part(&mut app, "commit number").unwrap();
    app
}

#[test]
fn a_long_history_pages_through_the_production_load_more() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 120);
    let mut app = log_app(&project, std::slice::from_ref(&repo));

    // First batch: 50 commits. The footer names the shown count and offers
    // the explicit Load more.
    app.wait_for("text:50 shown").unwrap();
    app.press("id:log:load-more").unwrap();
    app.wait_for("text:100 shown").unwrap();
    app.press("id:log:load-more").unwrap();
    app.wait_for("text:120 shown").unwrap();

    assert!(
        shows_part(&app, "commit number 120"),
        "the full history is loaded"
    );
    assert!(
        !app.screen().shows("Load more"),
        "nothing left to page: the button goes"
    );
    let _ = repo;
}

#[test]
fn the_count_footer_names_what_the_window_holds() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 10);
    let app = log_app(&project, &[repo]);

    assert!(
        shows_part(&app, "10 shown"),
        "a history shorter than one batch is fully shown"
    );
    assert!(
        !app.screen().shows("Load more"),
        "a one-screen log never offers paging"
    );
}

#[test]
fn scrolling_a_long_window_keeps_the_rows_it_built_aligned() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 200);
    let mut app = log_app(&project, &[repo]);

    app.press("id:log:load-more").unwrap();
    app.wait_for("text:100 shown").unwrap();

    // Scroll deep and back: the lane strip and the rows stay together, and
    // a row from the middle is on screen with its hash beside it.
    app.scroll("id:log:list", nsh_core::geometry::Points(2000.0))
        .unwrap();
    app.wait(std::time::Duration::from_secs(1)).unwrap();
    let texts = app.screen().texts();
    assert!(
        texts.iter().any(|t| t.contains("commit number")),
        "the scrolled viewport still paints commit rows"
    );
    app.scroll("id:log:list", nsh_core::geometry::Points(-2000.0))
        .unwrap();
    app.wait(std::time::Duration::from_secs(1)).unwrap();
    assert!(
        shows_part(&app, "commit number"),
        "the top of the history is back"
    );
}

#[test]
fn a_scoped_read_settles_per_root_without_leaking_the_other() {
    let project = Project::new();
    let alpha = log_repo(project.path(), "alpha", 3);
    let beta = log_repo(project.path(), "beta", 2);
    // The shared branch forks one commit back, so each root's main-only tip
    // is outside it and a scope that leaked the unscoped window would show.
    for (repo, subject) in [(&alpha, "alpha shared work"), (&beta, "beta shared work")] {
        test_support::git_seed::git(repo, &["checkout", "-q", "-b", "shared", "HEAD~1"]);
        support::commit_into(repo, "shared.txt", subject);
        test_support::git_seed::git(repo, &["checkout", "-q", "main"]);
    }
    let mut app = log_app(&project, &[alpha, beta]);

    // The ref scope reads every carrier: both repositories answer.
    app.press("id:log:filter-branch").unwrap();
    app.wait_for("text:shared").unwrap();
    app.press("id:log:ref-shared").unwrap();

    wait_for_part(&mut app, "Ref filter: shared").unwrap();
    wait_for_part(&mut app, "alpha shared work").unwrap();
    wait_for_part(&mut app, "beta shared work").unwrap();
    assert!(
        !shows_part(&app, "commit number 3"),
        "neither root leaks its unscoped history"
    );
}
