use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use test_support::git_seed::git;
use turbogit_app::state::AppState;
use turbogit_domain::model::UpdateMethod;
use turbogit_services::bulk_ops::{BulkOp, BulkPlan, SkipReason};

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    let bare = parent.join(format!("{name}.origin"));
    let _ = std::fs::remove_dir_all(&bare);
    git(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&path, &["push", "-q", "origin", "main"]);
    git(&path, &["branch", "--set-upstream-to=origin/main", "main"]);
    path
}

fn project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let ui = temp_repo(&frontend, "ui");
    std::fs::write(ui.join("dirty.txt"), "uncommitted\n").unwrap();
    (tmp, project, alpha, ui)
}

fn app_with_selection(project: &Path, roots: &[PathBuf], selected: &[&PathBuf]) -> AppState {
    let mut state = AppState::for_roots(project, roots);
    state.ui.repo_selection = selected
        .iter()
        .map(|p| turbogit_domain::model::RootId((*p).clone().into()))
        .collect();
    state
}

#[test]
fn bulk_preflight_classifies_the_selected_repos_for_the_operation() {
    let (_tmp, project, alpha, ui) = project("pf-classify");
    let state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);

    let pf = state.bulk_preflight(BulkOp::PullAll);
    assert_eq!(pf.rows.len(), 2, "one row per selected repo");
    let alpha_row = pf.rows.iter().find(|r| r.name == "alpha").unwrap();
    let ui_row = pf.rows.iter().find(|r| r.name == "ui").unwrap();
    assert!(alpha_row.outcome.is_ok(), "alpha should run: {alpha_row:?}");
    assert_eq!(ui_row.outcome, Err(SkipReason::DirtyWorktree));

    let pf = state.bulk_preflight(BulkOp::StashAll);
    let alpha_row = pf.rows.iter().find(|r| r.name == "alpha").unwrap();
    let ui_row = pf.rows.iter().find(|r| r.name == "ui").unwrap();
    assert_eq!(alpha_row.outcome, Err(SkipReason::CleanTree));
    assert!(ui_row.outcome.is_ok(), "dirty ui should stash: {ui_row:?}");

    let pf = state.bulk_preflight(BulkOp::PushAll);
    assert_eq!(pf.will_run(), 2, "both repos are dirty-ok with upstreams");
}

#[test]
fn bulk_preflight_uses_the_cached_ahead_behind_counts_for_divergence() {
    let (_tmp, project, alpha, ui) = project("pf-diverged");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);
    state
        .caches
        .store_ahead_behind(turbogit_domain::model::RootId(alpha.clone().into()), (2, 1));

    let pf = state.bulk_preflight(BulkOp::PullAll);
    let alpha_row = pf.rows.iter().find(|r| r.name == "alpha").unwrap();
    assert_eq!(alpha_row.outcome, Err(SkipReason::Diverged));
    assert_eq!((alpha_row.ahead, alpha_row.behind), (2, 1));

    let pf = state.bulk_preflight(BulkOp::FetchAll);
    assert_eq!(pf.will_run(), 2, "fetch never skips");
}

#[test]
fn bulk_preflight_ignores_roots_outside_the_selection() {
    let (_tmp, project, alpha, ui) = project("pf-scope");
    let state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha]);

    let pf = state.bulk_preflight(BulkOp::PullAll);
    assert_eq!(pf.rows.len(), 1, "only the selected root is in the matrix");
    assert_eq!(pf.rows[0].name, "alpha");
}

#[test]
fn open_bulk_preflight_seeds_the_pull_policy_from_settings() {
    let (_tmp, project, alpha, _ui) = project("pf-open");
    let mut state = app_with_selection(&project, std::slice::from_ref(&alpha), &[&alpha]);

    assert_eq!(state.ui.bulk_op, None, "no preflight modal initially");

    state.settings.update_method = UpdateMethod::Rebase;
    state.open_bulk_preflight(BulkOp::PullAll);
    assert_eq!(state.ui.bulk_op, Some(BulkOp::PullAll));
    assert!(state.ui.bulk_rebase, "policy seeds from settings");

    state.open_bulk_preflight(BulkOp::FetchAll);
    assert_eq!(state.ui.bulk_op, Some(BulkOp::FetchAll));
}

#[test]
fn run_bulk_confirmed_executes_the_plan_and_reports_per_repo_outcomes() {
    let (_tmp, project, alpha, ui) = project("pf-run");
    let mut state = app_with_selection(&project, &[alpha.clone(), ui.clone()], &[&alpha, &ui]);

    std::fs::write(alpha.join("base.txt"), "uncommitted\n").unwrap();

    let plan = BulkPlan {
        op: BulkOp::StashAll,
        roots: vec![turbogit_domain::model::RootId(alpha.clone().into())],
        rebase: false,
        branch: String::new(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);

    assert_eq!(state.ui.bulk_op, None);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        state.drain_events();
        if state.ui.toast.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let stashes = git(&alpha, &["stash", "list"]);
    assert!(!stashes.trim().is_empty(), "alpha's changes were stashed");
    let ui_status = git(&ui, &["status", "--porcelain"]);
    assert!(!ui_status.trim().is_empty(), "out-of-scope ui stayed dirty");

    let toast = state.ui.toast.as_ref().expect("completion toast");
    assert!(
        toast.message.contains("Stash all"),
        "toast names the operation: {:?}",
        toast.message
    );
    assert!(
        toast.message.contains("1 of 1"),
        "toast counts ran vs planned: {:?}",
        toast.message
    );

    let last = state.ui.activity.entries.last().expect("activity entry");
    assert!(
        last.message.contains("Stash all"),
        "activity: {:?}",
        last.message
    );
}

#[test]
fn run_bulk_confirmed_reports_partial_failure_in_the_summary() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = tmp.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();

    let msg = turbogit_app::state::bulk_report_message(
        "Push all",
        &[
            (turbogit_domain::model::RootId(alpha.clone().into()), Ok(())),
            (
                turbogit_domain::model::RootId(PathBuf::from("/w/ui").into()),
                Err(turbogit_domain::error::TgError::Other("rejected".into())),
            ),
        ],
    );
    assert_eq!(msg, "Push all · 1 of 2 ok · failed: ui");
}

#[test]
fn a_failed_root_with_no_utf8_name_is_missing_from_the_summary() {
    let ok_root = turbogit_domain::model::RootId(PathBuf::from("/w/api").into());
    let named_failure = turbogit_domain::model::RootId(PathBuf::from("/w/ui").into());
    let unnamed_failure = turbogit_domain::model::RootId(non_utf8_root().into());

    let msg = turbogit_app::state::bulk_report_message(
        "Push all",
        &[
            (ok_root, Ok(())),
            (
                named_failure,
                Err(turbogit_domain::error::TgError::Other("rejected".into())),
            ),
            (
                unnamed_failure,
                Err(turbogit_domain::error::TgError::Other("rejected".into())),
            ),
        ],
    );

    assert_eq!(
        msg, "Push all · 1 of 3 ok · failed: ui",
        "two roots failed and only one of them is named: the root with a \
         non-UTF-8 directory name has no name to print, so it is absent \
         rather than shown under a placeholder"
    );
}

#[cfg(unix)]
fn non_utf8_root() -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    PathBuf::from(OsStr::from_bytes(b"/w/bad\xffname"))
}

#[cfg(windows)]
fn non_utf8_root() -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;

    PathBuf::from(OsString::from_wide(&[0x5C, 0x77, 0x5C, 0xD800]))
}
