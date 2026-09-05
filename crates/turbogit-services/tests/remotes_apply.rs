//! Issue 33 — Manage remotes surface, services seam.
//!
//! The manager applies a remote change across a selection of roots and
//! reports per-repo outcomes. These tests pin `remote_service::apply_to_selection`
//! (and the manager refresh) against the real CLI engine over temporary
//! repositories.

use std::path::{Path, PathBuf};
use std::process::Command;
use turbogit_domain::error::TgError;
use turbogit_domain::model::{MultiRootManager, Remote, RootId};
use turbogit_engine::cli::CliExecutor;
use turbogit_services::multi_root::{build_root, register};
use turbogit_services::remote_service::{RemoteChange, apply_to_selection};

/// Run `git <args>` in `dir`, asserting success.
fn run_git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Fresh repo on `main` with local identity and one committed file.
fn fresh_repo(tmp: &Path, name: &str) -> PathBuf {
    let repo = tmp.join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    run_git(&repo, &["init", "-q", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("seed.txt"), "seed\n").expect("seed");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-q", "-m", "seed"]);
    repo
}

/// A registered manager over `repos`, through the production registration path.
fn manager(engine: &CliExecutor, repos: &[PathBuf]) -> MultiRootManager {
    let mut mgr = MultiRootManager::default();
    for repo in repos {
        register(&mut mgr, build_root(engine, repo).expect("root snapshot"));
    }
    mgr
}

fn id(repo: &Path) -> RootId {
    RootId(repo.into())
}

/// The remotes recorded on the manager's root for `rid`.
fn manager_remotes<'a>(mgr: &'a MultiRootManager, rid: &RootId) -> &'a [Remote] {
    &mgr.by_id(rid).expect("root registered").remotes
}

#[test]
fn apply_add_remote_across_the_selection_reports_per_repo_outcomes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = fresh_repo(tmp.path(), "alpha");
    let beta = fresh_repo(tmp.path(), "beta");
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let mut mgr = manager(&engine, &[alpha.clone(), beta.clone()]);
    let selected = vec![id(&alpha), id(&beta)];

    let outcomes = apply_to_selection(
        &engine,
        &mut mgr,
        &selected,
        &RemoteChange::Add {
            name: "origin".to_string(),
            url: "https://fetch.git/repo.git".to_string(),
        },
    );

    assert_eq!(outcomes.len(), 2, "one outcome per selected root");
    assert!(
        outcomes.iter().all(|(_, r)| r.is_ok()),
        "both roots take the change: {outcomes:?}"
    );

    // The manager refreshed each root's remotes so the list renders the add.
    for rid in &selected {
        let remotes = manager_remotes(&mgr, rid);
        assert_eq!(remotes.len(), 1, "root {rid:?} lists the new remote");
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(
            remotes[0].fetch_url.as_deref(),
            Some("https://fetch.git/repo.git")
        );
    }
}

#[test]
fn apply_add_continues_after_a_repo_fails_and_reports_the_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = fresh_repo(tmp.path(), "alpha");
    let beta = fresh_repo(tmp.path(), "beta");
    // A remote named `origin` already exists on alpha, so its add must fail.
    run_git(
        &alpha,
        &["remote", "add", "origin", "https://old.git/repo.git"],
    );
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let mut mgr = manager(&engine, &[alpha.clone(), beta.clone()]);
    let selected = vec![id(&alpha), id(&beta)];

    let outcomes = apply_to_selection(
        &engine,
        &mut mgr,
        &selected,
        &RemoteChange::Add {
            name: "origin".to_string(),
            url: "https://new.git/repo.git".to_string(),
        },
    );

    let (_, alpha_out) = outcomes.iter().find(|(rid, _)| rid == &id(&alpha)).unwrap();
    match alpha_out {
        Err(TgError::Cli { stderr, .. }) => {
            assert!(
                stderr.contains("already exists"),
                "the git failure surfaces per repo: {stderr:?}"
            );
        }
        other => panic!("expected the alpha add to fail, got {other:?}"),
    }
    let (_, beta_out) = outcomes.iter().find(|(rid, _)| rid == &id(&beta)).unwrap();
    assert!(beta_out.is_ok(), "the other root still takes the change");

    // The failing root's manager entry is untouched; the success is refreshed.
    assert_eq!(
        manager_remotes(&mgr, &id(&alpha)).len(),
        1,
        "alpha unchanged"
    );
    let beta_remotes = manager_remotes(&mgr, &id(&beta));
    assert_eq!(beta_remotes.len(), 1);
    assert_eq!(
        beta_remotes[0].fetch_url.as_deref(),
        Some("https://new.git/repo.git")
    );
}

#[test]
fn apply_set_url_updates_a_remote_across_the_selection() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = fresh_repo(tmp.path(), "alpha");
    let beta = fresh_repo(tmp.path(), "beta");
    for repo in [&alpha, &beta] {
        run_git(
            repo,
            &["remote", "add", "origin", "https://old.git/repo.git"],
        );
    }
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let mut mgr = manager(&engine, &[alpha.clone(), beta.clone()]);
    let selected = vec![id(&alpha), id(&beta)];

    let outcomes = apply_to_selection(
        &engine,
        &mut mgr,
        &selected,
        &RemoteChange::SetUrl {
            name: "origin".to_string(),
            fetch_url: Some("https://new.git/repo.git".to_string()),
            push_url: None,
        },
    );

    assert!(outcomes.iter().all(|(_, r)| r.is_ok()));
    for rid in &selected {
        let remotes = manager_remotes(&mgr, rid);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(
            remotes[0].fetch_url.as_deref(),
            Some("https://new.git/repo.git"),
            "root {rid:?} refreshed to the new URL"
        );
    }
}

#[test]
fn apply_respects_the_selection_and_leaves_other_roots_untouched() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = fresh_repo(tmp.path(), "alpha");
    let beta = fresh_repo(tmp.path(), "beta");
    let gamma = fresh_repo(tmp.path(), "gamma");
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let mut mgr = manager(&engine, &[alpha.clone(), beta.clone(), gamma.clone()]);

    let outcomes = apply_to_selection(
        &engine,
        &mut mgr,
        &[id(&alpha)],
        &RemoteChange::Add {
            name: "origin".to_string(),
            url: "https://fetch.git/repo.git".to_string(),
        },
    );

    assert_eq!(outcomes.len(), 1, "only the selected root runs");
    assert!(outcomes[0].1.is_ok());
    assert_eq!(manager_remotes(&mgr, &id(&alpha)).len(), 1, "alpha took it");
    assert!(
        manager_remotes(&mgr, &id(&beta)).is_empty()
            && manager_remotes(&mgr, &id(&gamma)).is_empty(),
        "out-of-selection roots are untouched"
    );
}

#[test]
fn apply_skips_roots_that_are_not_registered_in_the_manager() {
    let engine = CliExecutor {
        settings: Default::default(),
    };
    let mut mgr = MultiRootManager::default();

    let outcomes = apply_to_selection(
        &engine,
        &mut mgr,
        &[RootId(PathBuf::from("/nowhere").into())],
        &RemoteChange::Add {
            name: "origin".to_string(),
            url: "https://fetch.git/repo.git".to_string(),
        },
    );

    // The selection always comes from the manager, so an unknown root is a
    // caller bug: it is skipped the way run_bulk skips unregistered roots.
    assert!(outcomes.is_empty(), "no registered root, no outcome");
}
