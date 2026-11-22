//! Rewrite gating, preflight, reword, and undo, ported from the egui
//! `rewrite_gating` suite: the one reason a rewrite is blocked, the
//! preflight's drop and reword verbs, and the undo the success offers.

use std::path::{Path, PathBuf};

use support::{Project, for_roots, for_roots_seeded};
use test_support::git_seed::{commit, git};
use turbogit_app::OwnedOperation;
use turbogit_app::state::HistoryVerb;
use turbogit_domain::model::{Commit, RootId, Signature, SignatureState};
use turbogit_ui_nsh::screens::rewrite::{Facts, RewriteVerb, Scope, blocked, scope_of};

mod support;

fn repo(project: &Project, name: &str) -> PathBuf {
    let repo = support::seeded_repo(project.path(), name);
    commit(&repo, "a.txt", "one\n", "the first commit");
    commit(&repo, "b.txt", "two\n", "the second commit");
    repo
}

fn commit_at(id: &str, parents: usize) -> Commit {
    let who = Signature {
        name: "Author".into(),
        email: "author@example.com".into(),
        time: 1,
    };
    Commit {
        id: id.to_string(),
        parents: (0..parents).map(|n| format!("{n}123")).collect(),
        author: who.clone(),
        committer: who,
        message: "subject".into(),
        time: 1,
        root: RootId(Path::new("/ws/alpha").into()),
        signature: SignatureState::Unsigned,
    }
}

fn facts() -> Facts {
    Facts {
        owner: None,
        on_current_branch: true,
        protected_branch: false,
        scope: Scope::Own,
    }
}

#[test]
fn an_owned_operation_wins_over_every_other_reason() {
    let mut blocking = facts();
    blocking.owner = Some(OwnedOperation::Revert);
    blocking.protected_branch = true;
    assert_eq!(
        blocked(&commit_at("abc", 2), &blocking).map(|b| b.stated(RewriteVerb::Drop)),
        Some("Cannot rewrite history during a revert")
    );
    for (owner, text) in [
        (
            OwnedOperation::CherryPick,
            "Cannot rewrite history during a cherry-pick",
        ),
        (
            OwnedOperation::Rebase,
            "Cannot rewrite history during a rebase",
        ),
        (
            OwnedOperation::Merge,
            "Cannot rewrite history during a merge",
        ),
    ] {
        let mut blocking = facts();
        blocking.owner = Some(owner);
        assert_eq!(
            blocked(&commit_at("abc", 1), &blocking).map(|b| b.stated(RewriteVerb::Drop)),
            Some(text)
        );
    }
}

#[test]
fn a_merge_commit_states_its_verb_specific_reason() {
    assert_eq!(
        blocked(&commit_at("abc", 2), &facts()).map(|b| b.stated(RewriteVerb::Drop)),
        Some("a merge commit cannot be dropped")
    );
    assert_eq!(
        blocked(&commit_at("abc", 2), &facts()).map(|b| b.stated(RewriteVerb::Reword)),
        Some("a merge commit cannot be reworded")
    );
    assert_eq!(
        blocked(&commit_at("abc", 2), &facts()).map(|b| b.stated(RewriteVerb::Interactive)),
        Some("a merge commit cannot be rebased")
    );
}

#[test]
fn a_root_commit_states_its_verb_specific_reason() {
    assert_eq!(
        blocked(&commit_at("abc", 0), &facts()).map(|b| b.stated(RewriteVerb::Drop)),
        Some("a root commit cannot be dropped")
    );
}

#[test]
fn one_reason_wins_when_several_are_true() {
    let mut everything = facts();
    everything.owner = Some(OwnedOperation::Revert);
    everything.on_current_branch = false;
    everything.protected_branch = true;
    everything.scope = Scope::External;
    assert_eq!(
        blocked(&commit_at("abc", 2), &everything).map(|b| b.stated(RewriteVerb::Drop)),
        Some("Cannot rewrite history during a revert")
    );
    everything.owner = None;
    assert_eq!(
        blocked(&commit_at("abc", 2), &everything).map(|b| b.stated(RewriteVerb::Drop)),
        Some("a merge commit cannot be dropped")
    );
    assert_eq!(
        blocked(&commit_at("abc", 1), &everything).map(|b| b.stated(RewriteVerb::Drop)),
        Some("not on the current branch")
    );
}

#[test]
fn the_scope_and_the_branch_policy_are_the_last_two_reasons() {
    let mut several = facts();
    several.scope = Scope::Several;
    assert_eq!(
        blocked(&commit_at("abc", 1), &several).map(|b| b.stated(RewriteVerb::Drop)),
        Some("the rewrite runs in one repository; several are in scope")
    );
    let mut external = facts();
    external.scope = Scope::External;
    assert_eq!(
        blocked(&commit_at("abc", 1), &external).map(|b| b.stated(RewriteVerb::Drop)),
        Some("the commit is in a repository outside the ones in scope")
    );
    let mut protected = facts();
    protected.protected_branch = true;
    assert_eq!(
        blocked(&commit_at("abc", 1), &protected).map(|b| b.stated(RewriteVerb::Drop)),
        Some("the current branch is protected")
    );
    assert_eq!(blocked(&commit_at("abc", 1), &facts()), None);
}

#[test]
fn the_scope_of_a_selection_is_its_own_or_several_or_external() {
    let alpha = RootId(Path::new("/ws/alpha").into());
    let beta = RootId(Path::new("/ws/beta").into());
    let mut selection = std::collections::HashSet::new();
    assert_eq!(scope_of(&selection, &alpha), Scope::Own);
    selection.insert(alpha.clone());
    assert_eq!(scope_of(&selection, &alpha), Scope::Own);
    selection.insert(beta.clone());
    assert_eq!(scope_of(&selection, &alpha), Scope::Several);
    let mut external = std::collections::HashSet::new();
    external.insert(beta);
    assert_eq!(scope_of(&external, &alpha), Scope::External);
}

#[test]
fn a_drop_preflight_paints_the_commit_and_its_plan() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let mut app = for_roots_seeded(project.path(), std::slice::from_ref(&repo), move |s| {
        s.settings.protected_branch_patterns.clear();
        let root = s.multi.roots[0].id.clone();
        s.open_rewrite_preflight(&root, &head, HistoryVerb::Drop);
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:rewrite-preflight")
        .expect("the preflight paints");
    assert!(support::shows_part(&app, "DROPPED COMMIT"));
    assert!(support::shows_part(&app, "the second commit"));
    assert!(support::shows_part(&app, "Drop this commit"));
    assert!(support::shows_part(&app, "Recovery:"));
}

#[test]
fn an_empty_reword_refuses_before_the_preflight() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let mut app = for_roots_seeded(project.path(), std::slice::from_ref(&repo), move |s| {
        s.settings.protected_branch_patterns.clear();
        let root = s.multi.roots[0].id.clone();
        s.open_rewrite_preflight(
            &root,
            &head,
            HistoryVerb::Reword {
                message: "   ".to_string(),
            },
        );
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("text:/cannot have an empty message/")
        .expect("the empty-message refusal paints");
    assert!(
        !app.screen()
            .matches("id:dialog:rewrite-preflight")
            .unwrap_or(false)
    );
}

#[test]
fn the_reword_editor_refuses_blank_and_opens_the_preflight_when_filled() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let mut app = for_roots_seeded(project.path(), std::slice::from_ref(&repo), move |s| {
        s.settings.protected_branch_patterns.clear();
        let root = s.multi.roots[0].id.clone();
        let commits = s.recent_commits(&root, 10).unwrap_or_default();
        s.caches.store_log(root.clone(), commits);
        s.open_reword_editor(&root, &head);
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:reword")
        .expect("the reword editor paints");
    assert!(support::shows_part(&app, "The first line is the subject"));
    app.clear("id:rewrite:reword-message")
        .expect("clear the draft");
    app.wait_for("text:/A commit cannot have an empty message/")
        .expect("the blank draft warns");
    app.paste(
        "id:rewrite:reword-message",
        &nsh_core::PasteEvent::new("a better subject"),
    )
    .expect("type the new subject");
    app.press("id:dialog:reword:action")
        .expect("reword this commit");
    app.wait_for("id:dialog:rewrite-preflight")
        .expect("the preflight follows the reword draft");
    assert!(support::shows_part(&app, "a better subject"));
}

#[test]
fn a_drop_dispatch_offers_undo_and_the_undo_restores_the_head() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    let head_before = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let head = head_before.clone();
    let mut app = for_roots_seeded(project.path(), std::slice::from_ref(&repo), move |s| {
        s.settings.protected_branch_patterns.clear();
        let root = s.multi.roots[0].id.clone();
        s.open_rewrite_preflight(&root, &head, HistoryVerb::Drop);
    });
    app.first_render().expect("the shell mounts");
    app.wait_for("id:dialog:rewrite-preflight").unwrap();
    app.press("id:dialog:rewrite-preflight:action")
        .expect("drop this commit");
    app.wait_for("id:rewrite:undo")
        .expect("the undo offer paints after the rewrite");
    assert!(
        test_support::wait_until(10_000, || {
            git(&repo, &["rev-parse", "HEAD"]).trim() != head_before
        }),
        "the rewrite moved the head"
    );
    app.press("id:rewrite:undo-action").expect("undo");
    assert!(
        test_support::wait_until(10_000, || {
            git(&repo, &["rev-parse", "HEAD"]).trim() == head_before
        }),
        "the undo restored the pre-rewrite head"
    );
}

#[test]
fn the_rewrite_route_states_a_held_rebase() {
    let project = Project::new();
    let repo = repo(&project, "alpha");
    std::fs::create_dir_all(repo.join(".git/rebase-merge")).unwrap();
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo));
    app.first_render().expect("the shell mounts");
    app.press("id:toolbar:n5-rewrite")
        .expect("the rewrite band");
    app.wait_for("id:rewrite:gate")
        .expect("the gate states its reason");
    assert!(support::shows_part(
        &app,
        "Cannot rewrite history during a rebase"
    ));
}
