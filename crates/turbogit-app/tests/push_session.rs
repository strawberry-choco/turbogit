//! The push session's invariants, pinned without a harness: the rows are the
//! dialog's only statement of what it will do, so each test names the row state
//! and the payload those rows add up to. Where the promise is about git, the
//! repository is real.

use std::path::{Path, PathBuf};

use test_support::git_seed::{commit, git, repo_with_origin};
use turbogit_app::push_session::{
    Focus, Obstacle, PushRow, PushSession, TargetField, build_row, target_line,
};
use turbogit_app::state::AppState;
use turbogit_domain::model::{
    Branch, BranchKind, Commit, Remote, Root, RootId, RootStatus, Signature, SignatureState,
    Upstream,
};
use turbogit_engine_api::PushTags;
use turbogit_services::sync_service::{PushTarget, seed_push_target};

fn root_id(name: &str) -> RootId {
    RootId(PathBuf::from(format!("/repo/{name}")).into())
}

fn local(name: &str, tracked: bool) -> Branch {
    Branch {
        name: name.to_string(),
        kind: BranchKind::Local,
        tracking: tracked.then(|| Upstream {
            remote: "origin".to_string(),
            branch: name.to_string(),
        }),
        favorite: false,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: None,
        tip: None,
        remote: None,
    }
}

/// A repository with one local `main`, the named remotes, HEAD on `main` unless
/// it is the `detached` case, and `main` tracking `origin/main` when `tracked`.
fn fake_root(name: &str, remotes: &[&str], tracked: bool) -> Root {
    let id = root_id(name);
    Root {
        branches: vec![local("main", tracked)],
        current_branch: if name == "detached" {
            None
        } else {
            Some("main".to_string())
        },
        head: Some("abc".to_string()),
        id: id.clone(),
        path: id.as_path().to_path_buf(),
        remotes: remotes
            .iter()
            .map(|remote| Remote {
                name: remote.to_string(),
                fetch_url: Some(String::new()),
                push_url: None,
            })
            .collect(),
        status: RootStatus::default(),
    }
}

fn listed(id: &str, subject: &str) -> Commit {
    let someone = Signature {
        name: "t".to_string(),
        email: "t@t".to_string(),
        time: 0,
    };
    Commit {
        id: id.to_string(),
        parents: Vec::new(),
        committer: someone.clone(),
        author: someone,
        message: subject.to_string(),
        time: 1,
        root: root_id("alpha"),
        signature: SignatureState::Unsigned,
    }
}

fn row(root: &Root, consented: bool) -> PushRow {
    build_row(root, consented, None, Ok(vec![listed("a1", "one")]))
}

fn session(rows: Vec<PushRow>) -> PushSession {
    PushSession::open(1, rows, Some(Focus::AllRoots))
}

fn names(targets: &[PushTarget]) -> Vec<String> {
    targets.iter().map(|t| t.root.name()).collect()
}

/// A repository on `main`, published to a bare `origin`, one commit ahead.
fn ahead_repo(project: &Path, name: &str) -> PathBuf {
    let repo = repo_with_origin(project, name);
    commit(&repo, "ahead.txt", "ahead\n", "ahead work");
    repo
}

/// `main` published and level, `feature` published with an upstream and one
/// commit ahead of it, HEAD left on `main`.
fn feature_ahead_repo(project: &Path, name: &str) -> PathBuf {
    let repo = repo_with_origin(project, name);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "feature.txt", "feature\n", "seed");
    git(&repo, &["push", "-q", "-u", "origin", "feature"]);
    commit(&repo, "feature.txt", "feature more\n", "feature work");
    git(&repo, &["checkout", "-q", "main"]);
    repo
}

#[test]
fn an_open_without_a_caller_root_consents_no_row() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let first = ahead_repo(tmp.path(), "first");
    let second = ahead_repo(tmp.path(), "second");
    let mut state = AppState::for_roots(tmp.path(), &[first, second]);

    state.open_push();
    let rows = &state.ui.dlg.push.rows;
    assert_eq!(
        rows.iter().map(|r| r.id.name()).collect::<Vec<String>>(),
        vec!["first".to_string(), "second".to_string()],
        "one row per registered root, in registration order"
    );
    assert!(
        rows.iter().all(|row| !row.consented),
        "no caller root means no consent: the batch-everything default is gone"
    );
    assert!(rows.iter().all(|row| !row.expanded));
    assert_eq!(
        rows[0]
            .commits
            .iter()
            .map(|c| c.subject.as_str())
            .collect::<Vec<&str>>(),
        vec!["ahead work"],
        "the listed commits are HEAD's outgoing range joined against the log"
    );
}

#[test]
fn the_selector_scopes_a_consented_row_out_of_the_payload() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let first = ahead_repo(tmp.path(), "first");
    let second = ahead_repo(tmp.path(), "second");
    let mut state = AppState::for_roots(tmp.path(), &[first.clone(), second.clone()]);
    let first_id = RootId(first.into());
    let second_id = RootId(second.into());
    state.open_push_for_roots(&[first_id.clone(), second_id.clone()]);
    assert_eq!(state.ui.dlg.push.payload().targets.len(), 2);

    state.ui.dlg.push.scope = Some(vec![first_id.clone()]);
    let scoped = state.ui.dlg.push.payload();
    assert_eq!(
        scoped
            .targets
            .iter()
            .map(|target| target.root.clone())
            .collect::<Vec<_>>(),
        vec![first_id],
        "a consented row outside the selector's scope is not pushed"
    );

    state.ui.dlg.push.scope = Some(Vec::new());
    assert!(
        state.ui.dlg.push.payload().is_empty(),
        "an empty selection is nothing in play, not everything"
    );
}

#[test]
fn every_row_carries_its_resolved_target_whether_or_not_it_is_consented() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let first = ahead_repo(tmp.path(), "first");
    let second = ahead_repo(tmp.path(), "second");
    let mut state = AppState::for_roots(tmp.path(), &[first, second]);
    state.open_push();

    for index in 0..state.multi.roots.len() {
        let row = &state.ui.dlg.push.rows[index];
        let snapshot = &state.multi.roots[index];
        assert_eq!(
            row.target(),
            seed_push_target(snapshot),
            "row {index} prints the target its own push would use"
        );
        assert_eq!(row.remote, TargetField::Resolved("origin".to_string()));
        assert_eq!(row.branch, TargetField::Resolved("main".to_string()));
        assert_eq!(row.local.as_deref(), Some("main"));
        assert_eq!(row.obstacle, None);
        assert!(
            !row.set_upstream,
            "main has an upstream to record nothing against"
        );
    }
    assert!(
        !state.ui.dlg.push.rows[1].consented,
        "the unticked row still states its target"
    );
}

#[test]
fn the_branch_menu_seed_pushes_the_clicked_branch_not_the_current_one() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = feature_ahead_repo(tmp.path(), "repo");
    let id = RootId(repo.to_path_buf().into());
    assert_eq!(
        git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main",
        "HEAD stays on main while the verb names feature"
    );

    let mut state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    state.open_push_for_branch(&id, "feature");
    let row = state
        .ui
        .dlg
        .push
        .row(&id)
        .expect("the clicked root has a row");
    assert!(
        row.consented,
        "the verb that was clicked is the row that goes"
    );
    assert_eq!(row.local.as_deref(), Some("main"), "HEAD is stated as HEAD");
    assert_eq!(row.branch, TargetField::Resolved("feature".to_string()));
    assert_eq!(row.remote, TargetField::Resolved("origin".to_string()));

    let plan = state.ui.dlg.push.payload();
    assert_eq!(
        plan.targets,
        vec![PushTarget {
            root: id.clone(),
            remote: "origin".into(),
            branch: "feature".into(),
            set_upstream: false,
        }],
        "the batch carries the branch that was clicked, not `current_branch`"
    );
    assert_eq!(
        row.commits
            .iter()
            .map(|c| c.subject.as_str())
            .collect::<Vec<&str>>(),
        vec!["feature work"],
        "the listing is the clicked branch's outgoing range, not HEAD's"
    );
}

#[test]
fn the_payload_is_exactly_the_rows_that_are_consented() {
    let alpha = fake_root("alpha", &["origin"], true);
    let beta = fake_root("beta", &["origin"], true);
    let gamma = fake_root("gamma", &["origin"], true);
    let mut rows = vec![row(&alpha, true), row(&beta, false), row(&gamma, true)];
    rows[2].branch = TargetField::Editing {
        value: "topic".to_string(),
        seed: "main".to_string(),
    };
    let mut held = session(rows);
    held.force = true;
    held.no_verify = true;

    let plan = held.payload();
    assert_eq!(
        names(&plan.targets),
        vec!["alpha".to_string(), "gamma".to_string()],
        "registration order, consented rows only"
    );
    assert!(plan.blocked.is_empty());
    assert!(plan.force);
    assert!(plan.no_verify);
    assert_eq!(
        plan.options().tags,
        PushTags::None,
        "the untouched mode sends no tags flag"
    );
    assert_eq!(
        plan.targets[1].branch, "topic",
        "an edited field is the row's answer, not the seed it came from"
    );

    held.rows[1].toggle_consent();
    assert_eq!(names(&held.payload().targets).len(), 3);
    assert!(held.rows[1].expanded, "ticking reveals the row's commits");
}

#[test]
fn each_tags_mode_reaches_the_service_as_itself_not_as_a_yes_or_no() {
    let alpha = fake_root("alpha", &["origin"], true);
    for mode in [PushTags::None, PushTags::All, PushTags::Follow] {
        let mut held = session(vec![row(&alpha, true)]);
        held.tags = mode;
        let plan = held.payload();
        assert_eq!(plan.tags, mode, "the plan carries the session's mode");
        assert_eq!(
            plan.options().tags,
            mode,
            "and so does the batch it feeds: {mode:?} is its own command, not `true`"
        );
    }
    assert_eq!(
        [
            PushTags::None.label(),
            PushTags::All.label(),
            PushTags::Follow.label()
        ],
        ["No tags", "All", "Current Branch"],
        "the combo's words are the reference's, and the third one is a real option"
    );
}

#[test]
fn a_consented_row_may_still_carry_an_obstacle_and_the_payload_names_it() {
    let detached = fake_root("detached", &["origin"], true);
    let first_push = fake_root("alpha", &["origin"], false);
    let no_remote = fake_root("gamma", &[], true);
    let held = session(vec![
        row(&detached, true),
        row(&first_push, true),
        build_row(&no_remote, true, None, Ok(Vec::new())),
    ]);

    let plan = held.payload();
    assert_eq!(
        plan.blocked
            .iter()
            .map(|(id, obstacle)| (id.name(), *obstacle))
            .collect::<Vec<(String, Obstacle)>>(),
        vec![
            ("detached".to_string(), Obstacle::Detached),
            ("gamma".to_string(), Obstacle::NoRemote),
        ],
        "consented and unpushable is a legal pair: the plan names both halves"
    );
    assert_eq!(
        names(&plan.targets),
        vec!["alpha".to_string()],
        "a first push is unlistable, not unpushable"
    );
    assert!(
        plan.targets[0].set_upstream,
        "git's own default refuses the first push without it"
    );
    assert_eq!(
        held.row(&first_push.id).expect("row").obstacle,
        Some(Obstacle::UnlistedFirstPush)
    );
    assert!(!Obstacle::UnlistedFirstPush.blocks_push());
    assert!(Obstacle::Detached.blocks_push());
    assert_eq!(
        Obstacle::UnlistedFirstPush.label(),
        "first push — its commits cannot be listed"
    );
}

#[test]
fn target_line_formats_local_remote_branch() {
    assert_eq!(
        target_line(Some("main"), "origin", "main"),
        "main → origin : main"
    );
    assert_eq!(
        target_line(None, "origin", "topic"),
        "detached → origin : topic",
        "a root with no branch to state still prints its line"
    );
    assert_eq!(
        target_line(Some("main"), "", "main"),
        "main → no remote : main",
        "the missing remote is spelled, never left blank"
    );
}

#[test]
fn a_row_whose_cached_ahead_count_disagrees_with_its_listing_is_stale() {
    let alpha = fake_root("alpha", &["origin"], true);
    let beta = fake_root("beta", &["origin"], true);
    let gamma = fake_root("gamma", &["origin"], true);
    let mut rows = vec![row(&alpha, true), row(&beta, true), row(&gamma, false)];
    rows[2].listed_total = 1;
    // One commit listed in every row; the cache answers 1, 2 and 5.
    let counts: std::collections::HashMap<String, usize> = [
        ("alpha".to_string(), 1usize),
        ("beta".to_string(), 2usize),
        ("gamma".to_string(), 5usize),
    ]
    .into_iter()
    .collect();
    let held = session(rows);

    assert_eq!(
        held.stale_rows(&|id| counts.get(&id.name()).copied()),
        vec![root_id("beta")],
        "the stale row is the consented one whose listed count disagrees by value"
    );
    assert!(
        held.stale_rows(&|_| None).is_empty(),
        "nothing cached means nothing to disagree with"
    );
}

#[test]
fn a_failed_listing_is_carried_as_the_rows_error_not_as_an_empty_list() {
    let alpha = fake_root("alpha", &["origin"], true);
    let held = build_row(&alpha, true, None, Err("no such ref".to_string()));
    assert_eq!(held.error.as_deref(), Some("no such ref"));
    assert!(held.commits.is_empty());
    assert_eq!(
        held.target(),
        seed_push_target(&alpha),
        "a listing that failed still says what the row would push"
    );
}
