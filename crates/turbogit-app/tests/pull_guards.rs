//! A pull that rebases is a rewrite like any other, so every user-initiated one
//! pays the same two guards the rejected-push recovery pays: a protected branch
//! refuses *before* git is touched, and a rebase that runs records the rewrite
//! backup the Undo verb reads.
//!
//! The seam is [`AppState::dispatch_pull`], the one dispatch every pull verb in
//! the shell routes through — the palette's `Pull`, the branch menu's `Pull` and
//! `Checkout and pull`, the branch row's hover `Pull`. The verbs differ only in
//! label and in whether a checkout precedes the update, so the guarded behaviour
//! is asserted once per shape here, and the source sweep at the bottom of this
//! file is what says no surface bypasses the seam.
//!
//! The pairing is deliberate: each refusing case sits next to the case that must
//! still run, so a guard that widened to "never integrate into a protected
//! branch by pull" or a backup that leaked onto a plain merge fails here too.
//!
//! - protected branch, `Rebase`: refuses, history untouched, not even a fetch,
//!   and the refusal is *recorded* — the activity feed carries it after the toast
//!   has gone, which is the half a green toast assertion cannot see.
//! - unprotected branch, `Rebase`: rebases, and the pre-rebase tip is at the
//!   backup ref.
//! - protected branch, `Merge`: merges, because a merge rewrites nothing and so
//!   owes neither guard.
//! - checkout-then-pull: refuses before the worktree moves, and otherwise
//!   guards the branch the update lands on.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use test_support::git_seed::{commit, git, repo_with_origin};
use turbogit_app::activity::ActivityKind;
use turbogit_app::state::AppState;
use turbogit_domain::model::{UpdateMethod, VcsSettings};

// ---------------------------------------------------------------- fixtures --

/// The repository a pull runs in: `main` published to a bare `origin`, then
/// genuinely diverged — one local commit, one commit landed on the remote by a
/// second clone. Real divergence, because the only proof that a rebasing pull
/// *ran* is a replayed local commit, and the only proof that a refused one did
/// not is a history the remote's commit never entered.
///
/// The working repository has not fetched, so its `refs/remotes/origin/main` is
/// still the commit it pushed: a change to that ref is evidence of a fetch.
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

/// The [`diverged_repo`] shape with `main` left at its own local commit and HEAD
/// moved to a `feature` branch carrying one commit of its own: the shape a
/// `Checkout and pull` of `main` is asked for.
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

/// The branch HEAD is on — how a test says "the checkout never ran".
fn branch_of(dir: &Path) -> String {
    git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string()
}

/// Whether `ancestor` is in `rev`'s history — the way to say "the remote's
/// commit was integrated" without naming a merge or a rebase.
fn is_ancestor(dir: &Path, ancestor: &str, rev: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, rev])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Whether a reference exists at all — the rewrite backup's slot.
fn ref_exists(dir: &Path, name: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", name])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `origin`'s tip, read from the bare repository itself: the remote's real state,
/// which the working repository only learns about when something fetches.
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

/// Settings with the update method the case names and the protected list either
/// covering `main` (the shipped default) or empty.
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

/// A state over the fixture's single root, with the failure record cleared so a
/// test can read the outcome of exactly one dispatch.
fn state_over(project: &Path, work: &Path, method: UpdateMethod, protect_main: bool) -> AppState {
    let roots = [work.to_path_buf()];
    let mut state =
        AppState::for_roots(project, &roots).with_settings(settings(method, protect_main));
    state.last_error = None;
    state
}

/// One dispatch, settled: drain until the state stops moving, the headless form
/// of the frames a live Shell spends between the verb and its notification.
fn settle(state: &mut AppState) {
    for _ in 0..6 {
        if state.drain_events() == 0 {
            break;
        }
    }
}

/// Whether the completion the developer sees is a success.
fn succeeded(state: &AppState) -> bool {
    match &state.ui.toast {
        Some(toast) => matches!(toast.kind, turbogit_app::state::ToastKind::Success),
        None => false,
    }
}

const BACKUP: &str = "refs/turbogit/preflight-backup";

/// The activity feed's messages, oldest first — the record a settlement left
/// behind, as every other app suite reads it.
fn feed(state: &AppState) -> Vec<String> {
    state
        .ui
        .activity
        .entries
        .iter()
        .map(|e| e.message.clone())
        .collect()
}

/// A refusal is durable before it is visible: the toast expires and the feed is
/// what the developer can read afterwards, so the guard owes an entry naming the
/// verb it refused, the branch it refused to rebase and the repository it was
/// refused in. Asserted against the entry itself, never against `last_error` —
/// an error string on the state says nothing about whether the refusal was
/// written down anywhere.
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

/// The other half of the same claim: a pull that *ran* records a success, so the
/// entry above can only be there because the operation was refused — it is not
/// what any error string on the state happens to produce.
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

// -------------------------------------------------------------------- tests --

/// `Pull` with `Rebase` as the update method on a protected branch would rewrite
/// that branch, so it refuses — and refuses before any git command: no fetch
/// moved the remote-tracking ref, no backup claimed the repository's single
/// backup slot, and no commit was replayed.
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
    // A refusal is an answer, never silence: the same text every other rewrite's
    // guard produces, on the surface the developer is looking at.
    let refusal = state.last_error.clone().expect("a refused pull says so");
    assert!(
        refusal.contains("protected branch"),
        "the refusal names the guard, got {refusal}"
    );
    assert_refusal_recorded(&state, "Pull");
    assert!(!state.ui.busy, "the refusal settles rather than hanging");
}

/// The same pull on an unprotected branch is allowed — and must record the state
/// it is about to destroy at the backup reference, the same net every other
/// rewrite pays. Without it a rebasing pull has nothing to undo.
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

/// The guard is on the rewrite, not on the branch: a protected branch pulled by
/// a merge is not rewritten, so the verb still runs and owes no backup.
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

/// Checkout-then-pull names the branch it means to rebase before anything runs,
/// so the refusal is answered *before* the worktree moves: a developer whose
/// `Checkout and pull` was refused is still on the branch they started on.
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

/// The same composite on an unprotected branch runs, and the rewrite it performs
/// is the one after the checkout — which is why the guard and the backup are read
/// against the branch the update lands on, not the one the verb started on.
#[test]
fn a_checkout_and_pull_of_an_unprotected_branch_rebases_and_records_its_backup() {
    let tmp: TempDir = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let (work, bare) = feature_above_a_behind_main(&project, "alpha");
    let mut state = state_over(&project, &work, UpdateMethod::Rebase, false);
    let root = state.multi.roots[0].id.clone();

    // What `main` is about to be rewritten away from, and what the pull has to
    // integrate — neither is HEAD, which is on `feature` when the verb is clicked.
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

// ---------------------------------------------------------------- the sweep --

/// The claim "every user-initiated pull is guarded" is checked against the
/// sources rather than against a list a later verb can forget to join.
///
/// It is checked as an **exact allow-list**, because the two looser shapes this
/// sweep used to have both lie: a scope of one crate cannot see a direct port
/// call added to another, and a file exempted by name lets that file's merge-only
/// pull flip its rebase flag to `true` and stay green — which is exactly the
/// protected-branch rebase the guarded seam exists to prevent. So every crate
/// that can integrate an upstream is walked, each call of `pull` must match a
/// permitted site *by its own call text*, and a permitted site that has gone is
/// itself a failure, so the list cannot silently outgrow the code.
///
/// The port layer is walked too, deliberately: forwarding `rebase` unchanged is
/// the port's job, and pinning those lines one by one is what makes the port's
/// exclusion a named decision rather than a blind spot.
#[test]
fn no_surface_pulls_the_executor_beyond_the_guarded_seam() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let walked = [
        "turbogit-app/src",
        "turbogit-services/src",
        "turbogit-ui/src",
        // The port and its adapters: called here so the list below says *which*
        // sites are the port's, rather than the sweep not looking at them.
        "turbogit-engine/src",
        "turbogit-engine-api/src",
        "test-support/src",
    ];

    let mut found: Vec<(String, String)> = Vec::new();
    for dir in walked {
        for (file, src) in test_support::srcscan::sources_under(crates.join(dir)) {
            // `pull` and its guarded form: a site that calls `pull_guarded`
            // is on the seam, but it must still be *named* here — the sweep
            // is the list of every executor pull any surface can reach.
            for name in ["pull", "pull_guarded"] {
                for line in test_support::srcscan::call_lines(&src, name) {
                    let text = src
                        .lines()
                        .nth(line - 1)
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    found.push((format!("{dir}/{file}"), text));
                }
            }
        }
    }

    // `(site, its call text)`. The text is the pin: a site may be edited into a
    // different call, but not into one that rewrites a branch under guard.
    let permitted: &[(&str, &str)] = &[
        // The guarded seam — the only place a rebasing update may be integrated,
        // refusing a protected branch and saving the backup before either call.
        (
            "turbogit-services/src/sync_service.rs",
            "pull(vcs, root, false)",
        ),
        (
            "turbogit-services/src/sync_service.rs",
            "pull(vcs, root, true)",
        ),
        // The service-level forward onto the port: no policy of its own, the
        // rebase flag is its caller's, which is why calling it is a bypass.
        (
            "turbogit-services/src/sync_service.rs",
            "vcs.pull(root, rebase)",
        ),
        // Calls of the guarded seam itself — every site that reaches a pull
        // through the guard is named here.
        (
            "turbogit-services/src/sync_service.rs",
            "pull_guarded(vcs, root, method, settings, branch)",
        ),
        ("turbogit-app/src/state.rs", "sync_service::pull_guarded("),
        // The bulk run's `PullAll`, now guarded like every other site: the
        // matrix refuses at preflight, and a forced (Retry) step refuses
        // again in `run_step`, which is why this line is on the seam.
        (
            "turbogit-services/src/bulk_ops.rs",
            "sync_service::pull_guarded(",
        ),
        // The sidebar's `Pull selection`: merge-only *by construction*, and the
        // construction is what is pinned here — flip this `false` to `true` and
        // the site stops being listed.
        ("turbogit-ui/src/ui/sidebar.rs", "v.pull(p, false)?"),
        // The port's own adapters, each a pass-through of the caller's flag.
        (
            "turbogit-engine/src/git2_exec.rs",
            "self.cli.pull(root, rebase)",
        ),
        ("test-support/src/lib.rs", "self.inner.pull(root, rebase)"),
    ];

    let unlisted: Vec<String> = found
        .iter()
        .filter(|(file, text)| {
            !permitted
                .iter()
                .any(|&(pin_file, pin_text)| file.as_str() == pin_file && text.contains(pin_text))
        })
        .map(|(file, text)| format!("{file}: {text}"))
        .collect();
    let stale: Vec<String> = permitted
        .iter()
        .filter(|&&(pin_file, pin_text)| {
            !found
                .iter()
                .any(|(file, text)| file.as_str() == pin_file && text.contains(pin_text))
        })
        .map(|&(file, needle)| format!("{file}: {needle}"))
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
    // The walk is not empty and the list is not vacuous: the guard's own seam is
    // among the sites found, so a scan reading nothing would land in `stale`.
    assert!(
        found.len() >= permitted.len(),
        "the sweep found {} call sites for {} pinned ones",
        found.len(),
        permitted.len()
    );
}
