#![allow(dead_code)]

use std::path::Path;

use test_support::git_seed::git;
use turbogit_domain::model::{MultiRootManager, RebaseMode, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::integrate_service;
use turbogit_services::multi_root::{build_root, register};

fn append_commit(dir: &Path, name: &str, text: &str) -> String {
    let file = dir.join(name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", text]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn rebase_repo(tmp: &Path, name: &str) -> std::path::PathBuf {
    let repo = tmp.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    append_commit(&repo, "base.txt", "base");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    append_commit(&repo, "a.txt", "feature-1");
    append_commit(&repo, "b.txt", "feature-2");
    repo
}

#[test]
fn standard_mode_maps_to_a_plain_rebase() {
    let opts = integrate_service::rebase_mode_opts(RebaseMode::Standard, true, false, false);
    assert!(opts.update_refs);
    assert!(!opts.keep_empty);
    assert!(!opts.autosquash);
}

#[test]
fn autosquash_mode_forces_the_autosquash_flag() {
    let opts = integrate_service::rebase_mode_opts(RebaseMode::Autosquash, false, true, false);
    assert!(opts.autosquash);
    assert!(opts.keep_empty);
    assert!(!opts.update_refs);
}

#[test]
fn options_pass_through_on_top_of_the_standard_mode() {
    let opts = integrate_service::rebase_mode_opts(RebaseMode::Standard, false, true, true);
    assert!(opts.keep_empty);
    assert!(opts.autosquash);
    assert!(!opts.update_refs);
}

fn affected_fixtures(tmp: &Path) -> (Vec<turbogit_domain::model::Root>, RootId, [RootId; 3]) {
    let bare = tmp.join("origin.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);

    let mk = |name: &str, push: bool| -> std::path::PathBuf {
        let repo = tmp.join(name);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Test"]);
        git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
        append_commit(&repo, "base.txt", "base");
        if push {
            git(&repo, &["push", "-q", "-u", "origin", "main"]);
        } else {
            git(&repo, &["fetch", "-q", "origin"]);
        }
        repo
    };

    let focused = mk("focused", true);
    git(&focused, &["checkout", "-q", "-b", "feature"]);
    append_commit(&focused, "a.txt", "feature-1");
    git(&focused, &["push", "-q", "-u", "origin", "feature"]);

    let checked_out = mk("checked-out", false);
    git(
        &checked_out,
        &["checkout", "-q", "-b", "feature", "origin/feature"],
    );

    let tracking_shared = mk("tracking-shared", false);
    git(&tracking_shared, &["branch", "feature", "origin/feature"]);

    let unrelated = mk("unrelated", false);

    let engine = engine();
    let mut mgr = MultiRootManager::default();
    let mut ids = Vec::new();
    for p in [&focused, &checked_out, &tracking_shared, &unrelated] {
        register(&mut mgr, build_root(&engine, p).expect("root snapshot"));
        ids.push(RootId(p.to_path_buf().into()));
    }
    (
        mgr.roots.clone(),
        ids[0].clone(),
        [ids[1].clone(), ids[2].clone(), ids[3].clone()],
    )
}

#[test]
fn affected_repos_cover_checked_out_and_tracking_shared_siblings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (roots, focused, [checked_out, tracking_shared, unrelated]) = affected_fixtures(tmp.path());

    let affected = integrate_service::rebase_affected(&roots, &focused);
    assert!(
        affected.contains(&checked_out),
        "checked-out sibling missed"
    );
    assert!(
        affected.contains(&tracking_shared),
        "tracking-shared sibling missed"
    );
    assert!(!affected.contains(&unrelated), "unrelated sibling counted");
    assert_eq!(affected.len(), 2);
    assert!(
        !affected.contains(&focused),
        "focused counted as its own sibling"
    );
}

#[test]
fn a_focus_with_no_tracking_entry_answers_from_the_current_branch_comparison_only() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (mut roots, _focused, _siblings) = affected_fixtures(tmp.path());

    let lonely = rebase_repo(tmp.path(), "lonely");
    git(&lonely, &["checkout", "-q", "-b", "lonely-branch"]);
    append_commit(&lonely, "c.txt", "lonely-1");
    let engine = engine();
    let subject = build_root(&engine, &lonely).expect("root snapshot");
    let subject_id = subject.id.clone();
    assert!(
        subject
            .branches
            .iter()
            .find(|b| b.name == "lonely-branch")
            .is_some_and(|b| b.tracking.is_none()),
        "the fixture's current branch must carry no tracking entry",
    );
    roots.push(subject);

    let affected = integrate_service::rebase_affected(&roots, &subject_id);
    assert!(
        affected.is_empty(),
        "a focus whose branch tracks nothing and shares its name with no \
         sibling affects nothing; got {affected:?}",
    );
}

#[test]
fn a_focus_whose_branch_list_carries_a_remote_row_of_the_same_short_name_answers_as_before() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (roots, focused, _siblings) = affected_fixtures(tmp.path());

    let focused_root = roots.iter().find(|r| r.id == focused).expect("focused");
    let branch = focused_root
        .current_branch
        .clone()
        .expect("the fixture's focus is on a branch");
    let rows: Vec<_> = focused_root
        .branches
        .iter()
        .filter(|b| b.name == branch)
        .map(|b| (b.kind, b.tracking.is_some()))
        .collect();
    assert!(
        rows.contains(&(turbogit_domain::model::BranchKind::Local, true)),
        "the focus's local row must be the tracked one; got {rows:?}",
    );
    assert!(
        rows.contains(&(turbogit_domain::model::BranchKind::Remote, false)),
        "the focus's branch list must carry the remote row that shares the \
         short name, which is the row the dropped filter existed for; got {rows:?}",
    );

    let affected = integrate_service::rebase_affected(&roots, &focused);
    assert_eq!(
        affected.len(),
        2,
        "the tracked focus still affects its same-branch and same-upstream \
         siblings; got {affected:?}",
    );
}

fn settings_protecting(pattern: &str) -> VcsSettings {
    VcsSettings {
        protected_branch_patterns: vec![pattern.to_string()],
        ..Default::default()
    }
}

#[test]
fn protected_current_branch_refuses_the_rebase() {
    let engine = engine();
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = rebase_repo(tmp.path(), "repo");
    let settings = settings_protecting("feature");

    let err = integrate_service::rebase_current(
        &engine,
        &repo,
        "main",
        &Default::default(),
        &settings,
        "feature",
    )
    .expect_err("a protected branch must refuse the rebase");
    assert!(err.to_string().contains("protected"), "{err}");

    let plan = turbogit_services::history_editor::build_plan(&engine, &repo, "main").expect("plan");
    let err = integrate_service::rebase_plan(&engine, &repo, &plan, &settings, "feature")
        .expect_err("a protected branch must refuse the plan");
    assert!(err.to_string().contains("protected"), "{err}");
}

#[test]
fn unprotected_current_branch_rebases() {
    let engine = engine();
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = rebase_repo(tmp.path(), "repo");
    git(&repo, &["checkout", "-q", "main"]);
    let main_tip = append_commit(&repo, "main.txt", "main-1");
    git(&repo, &["checkout", "-q", "feature"]);
    let settings = settings_protecting("release/*");

    integrate_service::rebase_current(
        &engine,
        &repo,
        "main",
        &Default::default(),
        &settings,
        "feature",
    )
    .expect("an unprotected branch rebases");

    let new_parent = git(&repo, &["rev-parse", "HEAD~2"]).trim().to_string();
    assert_eq!(new_parent, main_tip, "feature was replayed onto main's tip");
}
