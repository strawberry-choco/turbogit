//! Issue 29 — rebase dialog upgrade (screen 15): the MODE → invocation
//! mapping, the cross-repo affected detection, and the protected-branch
//! guard.
//!
//! The mode mapping and affected detection are pure functions over the
//! domain model; the guard runs against real git repositories (tempdir +
//! system `git`).
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

use turbogit_domain::model::{MultiRootManager, RebaseMode, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::integrate_service;
use turbogit_services::multi_root::{build_root, register};

// ---------------------------------------------------------------- helpers --

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(dir).output();
    let output = output.expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Append `text` to `<dir>/<name>`, stage, commit, return HEAD SHA.
fn commit(dir: &Path, name: &str, text: &str) -> String {
    let file = dir.join(name);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{text}").expect("appending work file");
    drop(f);
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", text]);
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Repo on `main` (one base commit) with a `feature` branch two commits
/// ahead, checked out — the rebase subject.
fn rebase_repo(tmp: &Path, name: &str) -> std::path::PathBuf {
    let repo = tmp.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "base.txt", "base");
    run_git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(&repo, "a.txt", "feature-1");
    commit(&repo, "b.txt", "feature-2");
    repo
}

// ------------------------------------------------------------------ modes --

#[test]
fn standard_mode_maps_to_a_plain_rebase() {
    let opts = integrate_service::rebase_mode_opts(RebaseMode::Standard, true, false, false);
    assert!(opts.update_refs);
    assert!(!opts.keep_empty);
    assert!(!opts.autosquash);
}

#[test]
fn autosquash_mode_forces_the_autosquash_flag() {
    // The mode wins over a leftover toggle (the merge_flags precedent):
    // Autosquash always folds fixup!/squash! commits, even with the option
    // row unticked.
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

// --------------------------------------------------------- affected repos --

/// The focused repo (feature checked out, tracking `origin/feature` in a
/// shared bare remote) plus three siblings: one with feature checked out,
/// one on main with a local feature tracking the same upstream, and one
/// without any feature branch.
fn affected_fixtures(tmp: &Path) -> (Vec<turbogit_domain::model::Root>, RootId, [RootId; 3]) {
    let bare = tmp.join("origin.git");
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "-q", "--bare", "-b", "main"]);

    // Each repo gets its own unrelated base commit; only the focused repo
    // pushes main (the bare's main is irrelevant — the fixtures share the
    // `feature` ref). The others just fetch.
    let mk = |name: &str, push: bool| -> std::path::PathBuf {
        let repo = tmp.join(name);
        std::fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init", "-q", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test"]);
        run_git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
        commit(&repo, "base.txt", "base");
        if push {
            run_git(&repo, &["push", "-q", "-u", "origin", "main"]);
        } else {
            run_git(&repo, &["fetch", "-q", "origin"]);
        }
        repo
    };

    // Focused: feature branch pushed with upstream tracking, checked out.
    let focused = mk("focused", true);
    run_git(&focused, &["checkout", "-q", "-b", "feature"]);
    commit(&focused, "a.txt", "feature-1");
    run_git(&focused, &["push", "-q", "-u", "origin", "feature"]);

    // Sibling 1: feature checked out too.
    let checked_out = mk("checked-out", false);
    run_git(
        &checked_out,
        &["checkout", "-q", "-b", "feature", "origin/feature"],
    );

    // Sibling 2: on main, but a local feature tracks the same upstream.
    let tracking_shared = mk("tracking-shared", false);
    run_git(&tracking_shared, &["branch", "feature", "origin/feature"]);

    // Sibling 3: no feature branch at all.
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

// -------------------------------------------------------- protected guard --

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

    // The interactive plan path is guarded the same way — a rewrite through
    // any mode must refuse.
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
    // main moves on after feature branched, so replaying feature onto main
    // changes HEAD's parent to main's tip.
    run_git(&repo, &["checkout", "-q", "main"]);
    let main_tip = commit(&repo, "main.txt", "main-1");
    run_git(&repo, &["checkout", "-q", "feature"]);
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

    // feature carried two commits, so after the replay main's tip sits two
    // commits below HEAD.
    let new_parent = run_git(&repo, &["rev-parse", "HEAD~2"]).trim().to_string();
    assert_eq!(new_parent, main_tip, "feature was replayed onto main's tip");
}
