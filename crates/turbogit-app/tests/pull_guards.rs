use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use test_support::git_seed::{commit, git, repo_with_origin};
use turbogit_app::activity::ActivityKind;
use turbogit_app::state::AppState;
use turbogit_domain::model::{UpdateMethod, VcsSettings};

fn diverged_repo(project: &Path, name: &str) -> (PathBuf, PathBuf) {
    let work = repo_with_origin(project, name);
    let bare = project.join(format!("{name}.git"));
    commit(&work, "local.txt", "local\n", "local work");

    let other = project.join(format!("{name}-other"));
    git(
        project,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.email", "test@example.com"]);
    git(&other, &["config", "user.name", "Test"]);
    commit(&other, "remote-side.txt", "remote\n", "remote work");
    git(&other, &["push", "-q", "origin", "main"]);

    (work, bare)
}

fn feature_above_a_behind_main(project: &Path, name: &str) -> (PathBuf, PathBuf) {
    let (work, bare) = diverged_repo(project, name);
    git(&work, &["branch", "-f", "feature", "main"]);
    git(&work, &["checkout", "-q", "feature"]);
    commit(&work, "feature.txt", "feature\n", "feature work");
    (work, bare)
}

fn rev(dir: &Path, r: &str) -> String {
    git(dir, &["rev-parse", r]).trim().to_string()
}

fn branch_of(dir: &Path) -> String {
    git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string()
}

fn is_ancestor(dir: &Path, ancestor: &str, rev: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, rev])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ref_exists(dir: &Path, name: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", name])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn bare_tip(bare: &Path, refname: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("--git-dir")
        .arg(bare)
        .args(["rev-parse", "--verify", refname])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn settings(method: UpdateMethod, protect_main: bool) -> VcsSettings {
    VcsSettings {
        update_method: method,
        protected_branch_patterns: if protect_main {
            vec!["main".to_string(), "master".to_string()]
        } else {
            Vec::new()
        },
        ..VcsSettings::default()
    }
}

fn state_over(project: &Path, work: &Path, method: UpdateMethod, protect_main: bool) -> AppState {
    let roots = [work.to_path_buf()];
    let mut state =
        AppState::for_roots(project, &roots).with_settings(settings(method, protect_main));
    state.last_error = None;
    state
}

fn settle(state: &mut AppState) {
    for _ in 0..6 {
        if state.drain_events() == 0 {
            break;
        }
    }
}

fn succeeded(state: &AppState) -> bool {
    match &state.ui.toast {
        Some(toast) => matches!(toast.kind, turbogit_app::state::ToastKind::Success),
        None => false,
    }
}

const BACKUP: &str = "refs/turbogit/preflight-backup";

fn feed(state: &AppState) -> Vec<String> {
    state
        .ui
        .activity
        .entries
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

fn assert_refusal_recorded(state: &AppState, label: &str) {
    let entries = &state.ui.activity.entries;
    assert_eq!(
        entries.len(),
        1,
        "one refused pull settles one feed entry; the feed read {:?}",
        feed(state)
    );
    assert_eq!(
        entries[0].kind,
        ActivityKind::Error,
        "a refusal is recorded as a failure, not as a report of work done; the \
         feed read {:?}",
        feed(state)
    );
    assert_eq!(
        entries[0].message,
        format!("{label}: 'main' is a protected branch — rebasing it is not allowed"),
        "the entry carries the refused verb and the branch the guard named"
    );
    assert_eq!(
        entries[0].repo.as_deref(),
        Some("alpha"),
        "and the root the refusal belongs to; the feed read {:?}",
        feed(state)
    );
}

fn assert_no_refusal_recorded(state: &AppState) {
    assert_eq!(
        state.ui.activity.entries.len(),
        1,
        "one pull, one feed entry; the feed read {:?}",
        feed(state)
    );
    assert_eq!(
        state.ui.activity.entries[0].kind,
        ActivityKind::Success,
        "the pull ran, so nothing in the feed is a refusal; the feed read {:?}",
        feed(state)
    );
}

#[test]
fn a_manual_rebasing_pull_refuses_a_protected_branch_and_touches_nothing() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = diverged_repo(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Rebase, true);
    let root = state.multi.roots[0].id.clone();

    let head_before = rev(&work, "HEAD");
    let fetched_before = rev(&work, "refs/remotes/origin/main");
    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    state.dispatch_pull("Pull", root, None);
    settle(&mut state);

    assert_eq!(
        rev(&work, "HEAD"),
        head_before,
        "a protected branch must not be rewritten by a pull"
    );
    assert!(
        !is_ancestor(&work, &remote_commit, "HEAD"),
        "the remote's commit was integrated anyway — the pull ran"
    );
    assert_eq!(
        rev(&work, "refs/remotes/origin/main"),
        fetched_before,
        "the refusal precedes the fetch, so not even a remote-tracking ref moved"
    );
    assert!(
        !ref_exists(&work, BACKUP),
        "no rewrite ran, so no rewrite may have claimed the single backup slot"
    );
    let refusal = state.last_error.clone().expect("a refused pull says so");
    assert!(
        refusal.contains("protected branch"),
        "the refusal names the guard, got {refusal}"
    );
    assert_refusal_recorded(&state, "Pull");
    assert!(!state.ui.busy, "the refusal settles rather than hanging");
}

#[test]
fn a_manual_rebasing_pull_records_its_backup_before_it_rewrites() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = diverged_repo(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Rebase, false);
    let root = state.multi.roots[0].id.clone();

    let head_before = rev(&work, "HEAD");
    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    state.dispatch_pull("Pull", root, None);
    settle(&mut state);

    assert!(succeeded(&state), "the pull ran, not refused");
    assert!(
        is_ancestor(&work, &remote_commit, "HEAD"),
        "the rebasing pull must have integrated the remote's commit"
    );
    assert_ne!(
        rev(&work, "HEAD"),
        head_before,
        "a rebase replays the local commit, so the tip moves"
    );
    assert_eq!(
        rev(&work, BACKUP),
        head_before,
        "the rewrite backup must hold the pre-rebase tip"
    );
    assert_no_refusal_recorded(&state);
}

#[test]
fn a_manual_merging_pull_still_runs_on_a_protected_branch() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = diverged_repo(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Merge, true);
    let root = state.multi.roots[0].id.clone();

    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    state.dispatch_pull("Pull", root, None);
    settle(&mut state);

    assert!(succeeded(&state), "a merge owes neither guard");
    assert!(
        is_ancestor(&work, &remote_commit, "HEAD"),
        "the merging pull must integrate the remote's commit"
    );
    assert!(
        !ref_exists(&work, BACKUP),
        "a merge rewrites nothing, so it must not consume the backup slot"
    );
    assert_no_refusal_recorded(&state);
}

#[test]
fn a_checkout_and_pull_refuses_a_protected_branch_before_the_checkout() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, _bare) = feature_above_a_behind_main(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Rebase, true);
    let root = state.multi.roots[0].id.clone();
    let head_before = rev(&work, "HEAD");

    state.dispatch_pull("Pull main", root, Some("main".to_string()));
    settle(&mut state);

    assert_eq!(
        branch_of(&work),
        "feature",
        "a refused checkout-and-pull must not leave the user on `main`"
    );
    assert_eq!(rev(&work, "HEAD"), head_before, "nor move it");
    assert!(
        !ref_exists(&work, BACKUP),
        "no rewrite ran, so no backup was claimed"
    );
    let refusal = state.last_error.clone().expect("a refused pull says so");
    assert!(
        refusal.contains("protected branch"),
        "the refusal names the guard, got {refusal}"
    );
    assert_refusal_recorded(&state, "Pull main");
}

#[test]
fn a_checkout_and_pull_of_an_unprotected_branch_rebases_and_records_its_backup() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = feature_above_a_behind_main(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Rebase, false);
    let root = state.multi.roots[0].id.clone();

    let main_before = rev(&work, "refs/heads/main");
    let remote_commit = bare_tip(&bare, "refs/heads/main").expect("origin/main exists");

    state.dispatch_pull("Pull main", root, Some("main".to_string()));
    settle(&mut state);

    assert_eq!(
        branch_of(&work),
        "main",
        "the checkout half of the composite ran"
    );
    assert!(
        succeeded(&state),
        "the pull ran, not refused: {:?}",
        state.ui.toast
    );
    assert!(
        is_ancestor(&work, &remote_commit, "HEAD"),
        "the rebasing pull must have integrated the remote's commit"
    );
    assert_eq!(
        rev(&work, BACKUP),
        main_before,
        "the backup holds the tip the rebase rewrote away from"
    );
    assert_no_refusal_recorded(&state);
}

fn call_site_text(lines: &[&str], line: usize) -> String {
    let start = line - 1;
    let indent = |text: &str| text.len() - text.trim_start().len();
    let mut end = start;
    let mut depth = 0isize;
    while end + 1 < lines.len() {
        depth +=
            lines[end].matches('(').count() as isize - lines[end].matches(')').count() as isize;
        if depth <= 0 {
            break;
        }
        end += 1;
    }
    let mut begin = start;
    while begin > 0 {
        let prior = lines[begin - 1];
        if prior.trim().is_empty() || indent(prior) < indent(lines[start]) {
            break;
        }
        begin -= 1;
    }
    lines[begin..=end]
        .iter()
        .map(|text| text.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn no_surface_pulls_the_executor_beyond_the_guarded_seam() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let walked = [
        "turbogit-app/src",
        "turbogit-services/src",
        "turbogit-engine/src",
        "turbogit-engine-api/src",
        "test-support/src",
    ];

    let mut found: Vec<(String, String)> = Vec::new();
    for dir in walked {
        for (file, src) in test_support::srcscan::sources_under(crates.join(dir)) {
            let lines: Vec<&str> = src.lines().collect();
            for name in ["pull", "pull_guarded"] {
                for line in test_support::srcscan::call_lines(&src, name) {
                    found.push((format!("{dir}/{file}"), call_site_text(&lines, line)));
                }
            }
        }
    }

    let permitted: &[(&str, &[&str])] = &[
        (
            "turbogit-services/src/sync_service.rs",
            &["pull(vcs, root, false)"],
        ),
        (
            "turbogit-services/src/sync_service.rs",
            &["pull(vcs, root, true)"],
        ),
        (
            "turbogit-services/src/sync_service.rs",
            &["vcs.pull(root, rebase)"],
        ),
        (
            "turbogit-services/src/sync_service.rs",
            &["pull_guarded(vcs, root, method, settings, branch)"],
        ),
        (
            "turbogit-app/src/state.rs",
            &["sync_service::pull_guarded("],
        ),
        (
            "turbogit-services/src/bulk_ops.rs",
            &[
                "sync_service::pull_guarded(",
                "if rebase { UpdateMethod::Rebase",
                "vcs, &root.path, method, settings, root.current_branch.as_deref()",
            ],
        ),
        (
            "turbogit-app/src/shell_writes/pull.rs",
            &["v.pull(p, false)?"],
        ),
        (
            "turbogit-engine/src/git2_exec.rs",
            &["self.cli.pull(root, rebase)"],
        ),
        (
            "test-support/src/lib.rs",
            &["self.inner.pull(root, rebase)"],
        ),
    ];

    let unlisted: Vec<String> = found
        .iter()
        .filter(|(file, text)| {
            !permitted.iter().any(|(pin_file, needles)| {
                file.as_str() == *pin_file && needles.iter().all(|needle| text.contains(needle))
            })
        })
        .map(|(file, text)| format!("{file}: {text}"))
        .collect();
    let stale: Vec<String> = permitted
        .iter()
        .filter(|(pin_file, needles)| {
            !found.iter().any(|(file, text)| {
                file.as_str() == *pin_file && needles.iter().all(|n| text.contains(n))
            })
        })
        .map(|(file, needles)| format!("{file}: {:?}", needles))
        .collect();

    assert!(
        unlisted.is_empty(),
        "every pull verb must route through `AppState::dispatch_pull` or be the \
         pinned site this list already names; unguarded `pull` call sites: {unlisted:?}"
    );
    assert!(
        stale.is_empty(),
        "a pinned site that no longer exists leaves the list claiming coverage \
         the code no longer has: {stale:?}"
    );
    assert!(
        found.len() >= permitted.len(),
        "the sweep found {} call sites for {} pinned ones",
        found.len(),
        permitted.len()
    );
}
