//! Bulk operations over a selection of roots (issue 09, screens 02 + 04):
//! the preflight matrix that predicts — per repo — what will run and why a
//! repo would be skipped, plus the scoped fan-out that executes one operation
//! across exactly the roots the user confirmed.
//!
//! Skip policy per operation:
//! - **Fetch all** never skips: fetching is safe on any tree.
//! - **Pull all** skips dirty worktrees (local changes could be clobbered),
//!   diverged roots (a merge needs a human decision first), and roots with
//!   no upstream to pull from.
//! - **Push all** skips diverged roots (the remote would reject a
//!   non-fast-forward) and roots with no upstream to push to; a dirty tree
//!   does not block a push.
//! - **Stash all** is the inverse of pull: it skips clean trees (nothing to
//!   stash) and runs wherever there are local changes.

use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;

/// The fleet operations offered by the operations grid (screen 04).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BulkOp {
    FetchAll,
    PullAll,
    PushAll,
    StashAll,
    /// Cascade create-&-checkout branch (issue 11): the branch name rides
    /// in [`BulkPlan::branch`].
    CreateBranch,
    /// User-typed git command across the selection (issue 13): the command
    /// text rides in [`BulkPlan::command`].
    Custom,
    /// Cascade commit across the selection (issue 21): the message + amend
    /// flag ride in the per-step closure (not in [`BulkPlan`]) because the
    /// pure plan only carries preflight scope; the actual git call is
    /// [`crate::commit_across::run_one`].
    Commit,
    /// Cascade merge (issue 28): the merge dialog's "Cascade with N other
    /// repos" hand-off. The source branch rides in [`BulkPlan::branch`]
    /// (same as `CreateBranch`); the dialog's chosen [`MergeOpts`] are
    /// passed to [`run_step`] directly.
    Merge,
}

impl BulkOp {
    /// The grid label (screen 04).
    pub fn label(self) -> &'static str {
        match self {
            BulkOp::FetchAll => "Fetch all",
            BulkOp::PullAll => "Pull all",
            BulkOp::PushAll => "Push all",
            BulkOp::StashAll => "Stash all",
            BulkOp::CreateBranch => "Create & checkout branch",
            BulkOp::Custom => "Custom command…",
            BulkOp::Commit => "Commit",
            BulkOp::Merge => "Merge",
        }
    }
}

/// Why a repo will not take part in the run (the preflight matrix's STATUS
/// column, screen 02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// Uncommitted local changes make the operation unsafe.
    DirtyWorktree,
    /// The branch and its upstream both have commits (ahead > 0, behind > 0).
    Diverged,
    /// The current branch has no tracking upstream (or there is no current
    /// branch at all).
    NoUpstream,
    /// A clean tree has nothing to stash.
    CleanTree,
}

impl SkipReason {
    /// The matrix's parenthesized reason ("dirty worktree", …).
    pub fn label(self) -> &'static str {
        match self {
            SkipReason::DirtyWorktree => "dirty worktree",
            SkipReason::Diverged => "diverged",
            SkipReason::NoUpstream => "no upstream",
            SkipReason::CleanTree => "clean tree",
        }
    }
}

/// One row of the preflight matrix: a selected repo, its current state, and
/// whether it will run or be skipped (with reason).
#[derive(Debug, Clone)]
pub struct PreflightRow {
    pub root: RootId,
    /// Repo display name (the directory's file name).
    pub name: String,
    pub branch: Option<String>,
    /// Modified + unversioned paths.
    pub dirty: usize,
    pub ahead: usize,
    pub behind: usize,
    /// `Ok(())` = will run; `Err(reason)` = skipped.
    pub outcome: Result<(), SkipReason>,
}

/// The preflight matrix for one operation over the selected roots.
#[derive(Debug, Clone, Default)]
pub struct Preflight {
    /// One row per selected root, in the caller's order.
    pub rows: Vec<PreflightRow>,
}

impl Preflight {
    /// The header's "N of M will run" numerator: rows the operation will run on.
    pub fn will_run(&self) -> usize {
        self.rows.iter().filter(|r| r.outcome.is_ok()).count()
    }

    /// The matrix's row count (the "M" in "N of M").
    pub fn total(&self) -> usize {
        self.rows.len()
    }

    /// Skipped rows totaled per reason, most frequent first — the header
    /// line's "2 skipped (dirty worktree) · 1 skipped (diverged)".
    pub fn skip_totals(&self) -> Vec<(SkipReason, usize)> {
        let mut totals: Vec<(SkipReason, usize)> = Vec::new();
        for r in &self.rows {
            if let Err(reason) = r.outcome {
                match totals.iter_mut().find(|(k, _)| *k == reason) {
                    Some((_, n)) => *n += 1,
                    None => totals.push((reason, 1)),
                }
            }
        }
        totals.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.label().cmp(b.0.label())));
        totals
    }
}

/// Compute the preflight matrix: classify every selected root for `op`.
/// `ahead_behind` supplies each root's ↑/↓ counts vs its upstream (the root
/// caches in the app; test fixtures otherwise).
pub fn preflight(
    op: BulkOp,
    roots: &[Root],
    ahead_behind: &dyn Fn(&RootId) -> (usize, usize),
) -> Preflight {
    Preflight {
        rows: roots
            .iter()
            .map(|root| {
                let (ahead, behind) = ahead_behind(&root.id);
                let dirty = root.status.changes.len();
                let upstream = root
                    .current_branch
                    .as_deref()
                    .and_then(|branch| {
                        root.branches
                            .iter()
                            .find(|b| b.name == branch)
                            .and_then(|b| b.tracking.as_deref())
                    })
                    .is_some();
                let outcome = classify(op, dirty, ahead, behind, upstream);
                PreflightRow {
                    root: root.id.clone(),
                    name: root
                        .path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string(),
                    branch: root.current_branch.clone(),
                    dirty,
                    ahead,
                    behind,
                    outcome,
                }
            })
            .collect(),
    }
}

/// The skip policy of one operation against one root's state. The
/// create-&-checkout cascade classifies through [`branch_preflight`] —
/// never here.
fn classify(
    op: BulkOp,
    dirty: usize,
    ahead: usize,
    behind: usize,
    upstream: bool,
) -> Result<(), SkipReason> {
    match op {
        BulkOp::FetchAll => Ok(()),
        // An arbitrary command has no per-repo precondition TurboGit could
        // reason about, so — like fetch — it never skips; the safety gate
        // is the destructive confirmation, not the matrix.
        BulkOp::Custom => Ok(()),
        BulkOp::PullAll => {
            if dirty > 0 {
                Err(SkipReason::DirtyWorktree)
            } else if ahead > 0 && behind > 0 {
                Err(SkipReason::Diverged)
            } else if !upstream {
                Err(SkipReason::NoUpstream)
            } else {
                Ok(())
            }
        }
        // A cascade merge (issue 28) only needs a clean tree: the target is
        // the source branch picked in the merge dialog, not an upstream, so
        // divergence and tracking are irrelevant.
        BulkOp::Merge => {
            if dirty > 0 {
                Err(SkipReason::DirtyWorktree)
            } else {
                Ok(())
            }
        }
        BulkOp::PushAll => {
            if ahead > 0 && behind > 0 {
                Err(SkipReason::Diverged)
            } else if !upstream {
                Err(SkipReason::NoUpstream)
            } else {
                Ok(())
            }
        }
        BulkOp::StashAll => {
            if dirty == 0 {
                Err(SkipReason::CleanTree)
            } else {
                Ok(())
            }
        }
        BulkOp::CreateBranch => Ok(()),
        // Cascade commit (issue 21): always "will run" at the preflight
        // level — the per-repo skip lives in [`crate::commit_across::plan`]
        // (clean roots are seeded as `Skipped` rows), not here. The
        // `BulkOp` enum exists only so the cascade-run monitor can label
        // the running command (`git commit` vs `git commit --amend`).
        BulkOp::Commit => Ok(()),
    }
}

/// A confirmed preflight: one operation over exactly the roots that will run
/// (the matrix's non-skipped rows), plus the per-op policy. `rebase` is the
/// pull policy for [`BulkOp::PullAll`] and the apply-broadly (base on the
/// upstream when behind) policy for [`BulkOp::CreateBranch`]; `branch`
/// carries the target branch name of a create-&-checkout run (empty for
/// every other op).
#[derive(Debug, Clone)]
pub struct BulkPlan {
    pub op: BulkOp,
    /// The roots the operation runs on — preflight-approved scope only.
    pub roots: Vec<RootId>,
    /// Pull policy: `true` rebases onto the upstream instead of merging.
    pub rebase: bool,
    /// The cascade's target branch name (`BulkOp::CreateBranch` only).
    pub branch: String,
    /// The user-typed git command (`BulkOp::Custom` only, verbatim as
    /// entered — a leading "git" is tolerated by [`parse_command`]).
    pub command: String,
}

/// Execute a confirmed plan across the fleet: one `(RootId, result)` per
/// planned root, in plan order, continuing after individual failures.
/// Out-of-scope roots are never touched. Push resolves each root's upstream
/// remote the way [`sync_service::push_all`] does and honors the
/// protected-branch gate; stash parks each root's changes under a common
/// message.
pub fn run_bulk(
    vcs: &dyn turbogit_engine_api::GitExecutor,
    mgr: &MultiRootManager,
    settings: &VcsSettings,
    plan: &BulkPlan,
) -> Vec<(RootId, TgResult<()>)> {
    run_bulk_with_merge(vcs, mgr, settings, plan, &MergeOpts::default())
}

/// [`run_bulk`] with the merge dialog's chosen options (issue 28): the
/// cascade-merge plan executes with exactly the strategy and option rows the
/// user confirmed in the dialog.
pub fn run_bulk_with_merge(
    vcs: &dyn turbogit_engine_api::GitExecutor,
    mgr: &MultiRootManager,
    settings: &VcsSettings,
    plan: &BulkPlan,
    merge: &MergeOpts,
) -> Vec<(RootId, TgResult<()>)> {
    plan.roots
        .iter()
        .filter_map(|rid| mgr.by_id(rid).map(|root| (rid, root)))
        .map(|(rid, root)| {
            let result = run_step(plan.op, vcs, root, plan, settings, merge);
            (rid.clone(), result)
        })
        .collect()
}

// -- Custom command (issue 13, screen 04) -------------------------------------

/// Parse the command a user typed into the custom-command modal into git
/// arguments. The executor always runs the git binary, so a leading "git"
/// the user naturally types is dropped and the rest is taken verbatim —
/// whitespace-separated, with single- or double-quoted runs grouping into
/// one argument. `None` when nothing remains to run (empty input, or just
/// the bare "git").
pub fn parse_command(input: &str) -> Option<Vec<String>> {
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in input.trim().chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            None => {
                cur.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        args.push(cur);
    }
    if args.first().map(String::as_str) == Some("git") {
        args.remove(0);
    }
    if args.is_empty() { None } else { Some(args) }
}

/// Why a parsed command counts as destructive-looking and needs the extra
/// confirmation before it may run across the fleet (`None` = ordinary
/// command, one confirmation is enough). The subcommand decides: `reset`
/// moves branches and can discard commits, `clean` deletes untracked files,
/// `rebase`/`filter-branch`/`filter-repo` rewrite history, a `push` with a
/// force flag rewrites the remote, and `branch -D` force-deletes branches.
pub fn destructive_reason(args: &[String]) -> Option<&'static str> {
    let first = args.first().map(String::as_str)?;
    let has_flag = |flags: &[&str]| args.iter().any(|a| flags.contains(&a.as_str()));
    match first {
        "reset" => Some("reset moves branches and can discard commits"),
        "clean" => Some("clean deletes untracked files"),
        "rebase" | "filter-branch" | "filter-repo" => Some("rebase rewrites history"),
        "push" if has_flag(&["-f", "--force", "--force-with-lease"]) => {
            Some("force push rewrites the remote")
        }
        "branch" if has_flag(&["-D"]) => Some("force delete discards the branch"),
        _ => None,
    }
}

/// Execute one root's part of a bulk operation — the per-root step the
/// cascade engine dispatches on workers (issue 10) and [`run_bulk`] runs
/// serially. Push resolves the root's upstream remote the way
/// [`sync_service::push_all`] does and honors the protected-branch gate;
/// stash parks the root's changes under a common message. A create-&-checkout
/// step checks an existing matching branch out instead of recreating it and,
/// under the apply-broadly policy, bases a missing branch on the upstream
/// tip when the repo is behind.
pub fn run_step(
    op: BulkOp,
    vcs: &dyn turbogit_engine_api::GitExecutor,
    root: &Root,
    plan: &BulkPlan,
    settings: &VcsSettings,
    merge: &MergeOpts,
) -> TgResult<()> {
    let rebase = plan.rebase;
    let branch = plan.branch.as_str();
    let command = plan.command.as_str();
    match op {
        BulkOp::FetchAll => super::sync_service::fetch(vcs, &root.path, None),
        BulkOp::PullAll => super::sync_service::pull(vcs, &root.path, rebase),
        BulkOp::PushAll => push_root(vcs, root, settings),
        BulkOp::StashAll => {
            super::shelve_stash::stash(vcs, &root.path, "TurboGit bulk stash", false)
        }
        BulkOp::CreateBranch => run_branch_step(vcs, root, branch, rebase),
        BulkOp::Custom => {
            let args = parse_command(command)
                .ok_or_else(|| TgError::Other("empty custom command".into()))?;
            vcs.run_raw(&root.path, &args).map(|_| ())
        }
        // Cascade commit is dispatched through its own per-step closure
        // (see `AppState::run_commit_across`); reaching this arm means a
        // caller routed `BulkOp::Commit` through the generic bulk pipeline
        // by mistake. Refuse loudly rather than silently no-op — `message`
        // and `amend` are not in `BulkPlan`.
        BulkOp::Commit => Err(TgError::Other(
            "BulkOp::Commit must be dispatched through run_commit_across".into(),
        )),
        BulkOp::Merge => {
            let target = branch.trim();
            if target.is_empty() {
                return Err(TgError::Other("empty merge target".into()));
            }
            super::integrate_service::smart_merge(
                vcs,
                &root.path,
                target,
                merge,
                settings.clean_tree_method,
            )
        }
    }
}
/// One repo's part of a cascade create-&-checkout run (issue 11): an
/// existing local branch with the target name is checked out as-is; a
/// missing one is created (with checkout) from the upstream tip when the
/// apply-broadly policy is on and the current branch is behind it, and from
/// HEAD otherwise.
fn run_branch_step(
    vcs: &dyn turbogit_engine_api::GitExecutor,
    root: &Root,
    name: &str,
    from_upstream: bool,
) -> TgResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(TgError::Other("empty branch name".into()));
    }
    let exists = root
        .branches
        .iter()
        .any(|b| b.kind == BranchKind::Local && b.name == name);
    if exists {
        return super::branch_service::checkout(vcs, &root.path, name);
    }
    let start_point: Option<String> = if from_upstream {
        let tracking = root.current_branch.as_deref().and_then(|cur| {
            root.branches
                .iter()
                .find(|b| b.name == cur)
                .and_then(|b| b.tracking.clone())
        });
        match (root.current_branch.as_deref(), tracking) {
            (Some(cur), Some(up))
                if vcs
                    .ahead_behind(&root.path, cur, &up)
                    .map(|(_, behind)| behind > 0)
                    .unwrap_or(false) =>
            {
                Some(up)
            }
            _ => None,
        }
    } else {
        None
    };
    super::branch_service::create(vcs, &root.path, name, start_point.as_deref(), true)
}

/// Push one root's current branch, resolving the upstream remote from the
/// branch's tracking ref when present, else the first remote, else "origin"
/// (the [`sync_service::push_all`] resolution, scoped to one root).
fn push_root(
    vcs: &dyn turbogit_engine_api::GitExecutor,
    root: &Root,
    settings: &VcsSettings,
) -> TgResult<()> {
    let branch = root
        .current_branch
        .as_deref()
        .ok_or_else(|| TgError::Other("no current branch to push".into()))?;
    let remote = match root
        .branches
        .iter()
        .find(|b| b.name == branch)
        .and_then(|b| b.tracking.as_deref())
    {
        Some(t) if t.contains('/') => t.split('/').next().unwrap_or("origin"),
        _ => root
            .remotes
            .first()
            .map(|r| r.name.as_str())
            .unwrap_or("origin"),
    };
    super::sync_service::push(
        vcs, &root.path, remote, branch, false, false, false, false, None, settings,
    )
}

// -- Cascade create & checkout branch (issue 11, screen 02) ------------------

/// The predicted per-repo action of a cascade create-&-checkout run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchAction {
    /// A local branch with the target name already exists: check it out,
    /// never recreate it.
    CheckoutExisting,
    /// Clean and current: create the branch from HEAD and check it out.
    CreateFromHead,
    /// Clean but behind its upstream: base the new branch on the upstream
    /// tip (`from`) so it is not cut from stale code — the "Apply broadly"
    /// policy.
    CreateFromUpstream { from: String },
    /// Skipped before the run.
    Skip(SkipReason),
}

/// One row of the cascade create-&-checkout preflight matrix.
#[derive(Debug, Clone)]
pub struct BranchPreflightRow {
    pub root: RootId,
    /// Repo display name (the directory's file name).
    pub name: String,
    pub branch: Option<String>,
    /// Modified + unversioned paths.
    pub dirty: usize,
    /// The branch every running repo lands on (the TARGET column).
    pub target: String,
    pub action: BranchAction,
}

/// The cascade create-&-checkout preflight matrix for one branch name over
/// the selected roots.
#[derive(Debug, Clone)]
pub struct BranchPreflight {
    /// The requested branch name, trimmed.
    pub name: String,
    /// One row per selected root, in the caller's order.
    pub rows: Vec<BranchPreflightRow>,
}

impl BranchPreflight {
    /// The header's "N of M will run" numerator.
    pub fn will_run(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| !matches!(r.action, BranchAction::Skip(_)))
            .count()
    }

    /// The matrix's row count (the "M" in "N of M").
    pub fn total(&self) -> usize {
        self.rows.len()
    }

    /// Skipped rows totaled per reason, most frequent first — the same
    /// header summary the bulk preflight paints.
    pub fn skip_totals(&self) -> Vec<(SkipReason, usize)> {
        let mut totals: Vec<(SkipReason, usize)> = Vec::new();
        for r in &self.rows {
            if let BranchAction::Skip(reason) = r.action {
                match totals.iter_mut().find(|(k, _)| *k == reason) {
                    Some((_, n)) => *n += 1,
                    None => totals.push((reason, 1)),
                }
            }
        }
        totals.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.label().cmp(b.0.label())));
        totals
    }

    /// The roots the run executes on — the non-skipped rows.
    pub fn plan_roots(&self) -> Vec<RootId> {
        self.rows
            .iter()
            .filter(|r| !matches!(r.action, BranchAction::Skip(_)))
            .map(|r| r.root.clone())
            .collect()
    }
}

/// Predict the cascade create-&-checkout outcome for every selected root.
/// `ahead_behind` supplies each root's ↑/↓ counts vs its upstream (the root
/// caches in the app; test fixtures otherwise). Classification order: a
/// dirty worktree and a diverged branch skip the run regardless of
/// anything else; an existing local branch is checked out; the "apply
/// broadly" policy bases the new branch on the upstream when the repo is
/// behind; everything else is created from HEAD.
pub fn branch_preflight(
    roots: &[Root],
    name: &str,
    from_upstream: bool,
    ahead_behind: &dyn Fn(&RootId) -> (usize, usize),
) -> BranchPreflight {
    BranchPreflight {
        name: name.to_string(),
        rows: roots
            .iter()
            .map(|root| {
                let (ahead, behind) = ahead_behind(&root.id);
                let dirty = root.status.changes.len();
                let tracking = root.current_branch.as_deref().and_then(|branch| {
                    root.branches
                        .iter()
                        .find(|b| b.name == branch)
                        .and_then(|b| b.tracking.clone())
                });
                let exists = root
                    .branches
                    .iter()
                    .any(|b| b.kind == BranchKind::Local && b.name == name);
                let action = if dirty > 0 {
                    BranchAction::Skip(SkipReason::DirtyWorktree)
                } else if ahead > 0 && behind > 0 {
                    BranchAction::Skip(SkipReason::Diverged)
                } else if exists {
                    BranchAction::CheckoutExisting
                } else if from_upstream && behind > 0 {
                    match tracking {
                        Some(up) => BranchAction::CreateFromUpstream { from: up },
                        None => BranchAction::CreateFromHead,
                    }
                } else {
                    BranchAction::CreateFromHead
                };
                BranchPreflightRow {
                    root: root.id.clone(),
                    name: root
                        .path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string(),
                    branch: root.current_branch.clone(),
                    dirty,
                    target: name.to_string(),
                    action,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A root with the given branch/tracking and dirty-file count.
    fn root(path: &str, branch: Option<&str>, tracking: Option<&str>, dirty: usize) -> Root {
        let id = RootId(PathBuf::from(path).into());
        let branches = match (branch, tracking) {
            (Some(b), Some(t)) => vec![Branch {
                name: b.to_string(),
                kind: BranchKind::Local,
                tracking: Some(t.to_string()),
                favorite: false,
                protected: false,
                exists: true,
                ahead: 0,
                behind: 0,
                gone: false,
                last_touched: None,
                tip: None,
                remote: None,
            }],
            (Some(b), None) => vec![Branch {
                name: b.to_string(),
                kind: BranchKind::Local,
                tracking: None,
                favorite: false,
                protected: false,
                exists: true,
                ahead: 0,
                behind: 0,
                gone: false,
                last_touched: None,
                tip: None,
                remote: None,
            }],
            _ => vec![],
        };
        Root {
            id,
            path: PathBuf::from(path),
            remotes: vec![],
            branches,
            current_branch: branch.map(str::to_string),
            head: None,
            status: RootStatus {
                changes: (0..dirty)
                    .map(|i| Change {
                        path: PathBuf::from(format!("f{i}.txt")),
                        status: ChangeStatus::Modified,
                        chunks: vec![],
                        staged: false,
                        unstaged: false,
                        orig_path: None,
                    })
                    .collect(),
                conflicted: vec![],
            },
        }
    }

    /// A four-repo fleet: clean+synced, dirty, diverged, no-upstream.
    fn fleet() -> Vec<Root> {
        vec![
            root("/w/alpha", Some("main"), Some("origin/main"), 0),
            root("/w/ui", Some("main"), Some("origin/main"), 5),
            root("/w/engine", Some("dev"), Some("origin/dev"), 0),
            root("/w/lib", Some("topic"), None, 0),
        ]
    }

    fn ab(root: &RootId) -> (usize, usize) {
        match root.0.to_str().unwrap() {
            "/w/engine" => (2, 1),
            _ => (0, 0),
        }
    }

    /// Assert the outcome pattern per row, by repo name.
    fn assert_outcomes(pf: &Preflight, expected: &[(&str, bool, Option<SkipReason>)]) {
        assert_eq!(pf.rows.len(), expected.len(), "one row per selected root");
        for (row, (name, runs, reason)) in pf.rows.iter().zip(expected) {
            assert_eq!(row.name, *name, "row order follows the caller's order");
            match (&row.outcome, runs, reason) {
                (Ok(()), true, _) => {}
                (Err(r), false, Some(want)) => assert_eq!(r, want, "wrong skip reason for {name}"),
                other => panic!("wrong outcome for {name}: {other:?}"),
            }
        }
    }

    #[test]
    fn preflight_classifies_every_selected_repo_per_operation() {
        let roots = fleet();

        // Fetch never skips.
        let pf = preflight(BulkOp::FetchAll, &roots, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", true, None),
                ("ui", true, None),
                ("engine", true, None),
                ("lib", true, None),
            ],
        );

        // Pull skips dirty, diverged, and no-upstream roots.
        let pf = preflight(BulkOp::PullAll, &roots, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", true, None),
                ("ui", false, Some(SkipReason::DirtyWorktree)),
                ("engine", false, Some(SkipReason::Diverged)),
                ("lib", false, Some(SkipReason::NoUpstream)),
            ],
        );

        // Push only needs an upstream; a dirty tree does not block it.
        let pf = preflight(BulkOp::PushAll, &roots, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", true, None),
                ("ui", true, None),
                ("engine", false, Some(SkipReason::Diverged)),
                ("lib", false, Some(SkipReason::NoUpstream)),
            ],
        );

        // Stash is the inverse of pull: clean trees are skipped.
        let pf = preflight(BulkOp::StashAll, &roots, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", false, Some(SkipReason::CleanTree)),
                ("ui", true, None),
                ("engine", false, Some(SkipReason::CleanTree)),
                ("lib", false, Some(SkipReason::CleanTree)),
            ],
        );
    }

    #[test]
    fn preflight_rows_carry_the_per_repo_state_the_matrix_renders() {
        let roots = fleet();
        let pf = preflight(BulkOp::PullAll, &roots, &ab);

        let ui_row = pf.rows.iter().find(|r| r.name == "ui").unwrap();
        assert_eq!(ui_row.branch.as_deref(), Some("main"));
        assert_eq!(ui_row.dirty, 5);
        assert_eq!((ui_row.ahead, ui_row.behind), (0, 0));

        let engine_row = pf.rows.iter().find(|r| r.name == "engine").unwrap();
        assert_eq!((engine_row.ahead, engine_row.behind), (2, 1));
        assert_eq!(engine_row.branch.as_deref(), Some("dev"));
    }

    #[test]
    fn preflight_totals_state_what_will_run_and_why_roots_are_skipped() {
        let roots = fleet();

        let pf = preflight(BulkOp::PullAll, &roots, &ab);
        assert_eq!(pf.will_run(), 1, "only alpha runs");
        assert_eq!(pf.total(), 4);
        // Skip reasons totaled for the header line ("2 skipped (dirty
        // worktree) · 1 skipped (diverged) · …"), most frequent first.
        let totals: Vec<(SkipReason, usize)> = pf.skip_totals();
        assert_eq!(
            totals,
            vec![
                (SkipReason::DirtyWorktree, 1),
                (SkipReason::Diverged, 1),
                (SkipReason::NoUpstream, 1),
            ]
        );

        // A fleet where everything runs totals to nothing.
        let clean = vec![root("/w/alpha", Some("main"), Some("origin/main"), 0)];
        let pf = preflight(BulkOp::PullAll, &clean, &|_| (0, 0));
        assert_eq!(pf.will_run(), 1);
        assert_eq!(pf.skip_totals().len(), 0);
    }

    // -- Cascade create & checkout branch (issue 11) -------------------------

    /// A five-repo fleet for the branch cascade: clean+synced, dirty,
    /// diverged, a repo that already has the target branch locally, and a
    /// repo behind its upstream.
    fn branch_fleet() -> Vec<Root> {
        let mut roots = vec![
            root("/w/app", Some("main"), Some("origin/main"), 0),
            root("/w/ui", Some("main"), Some("origin/main"), 3),
            root("/w/engine", Some("dev"), Some("origin/dev"), 0),
            root("/w/lib", Some("main"), Some("origin/main"), 0),
            root("/w/cli", Some("main"), Some("origin/main"), 0),
        ];
        // lib already carries release/q3 locally — it must be checked out,
        // never recreated.
        roots[3].branches.push(Branch {
            name: "release/q3".to_string(),
            kind: BranchKind::Local,
            tracking: None,
            favorite: false,
            protected: false,
            exists: true,
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: None,
            tip: None,
            remote: None,
        });
        roots
    }

    /// engine is diverged; cli is behind its upstream; the rest are synced.
    fn branch_ab(root: &RootId) -> (usize, usize) {
        match root.0.to_str().unwrap() {
            "/w/engine" => (1, 2),
            "/w/cli" => (0, 2),
            _ => (0, 0),
        }
    }

    #[test]
    fn branch_preflight_predicts_the_per_repo_action_for_the_cascade() {
        let roots = branch_fleet();

        // Apply broadly (default): repos behind their upstream base the new
        // branch on the upstream tip instead of stale HEAD.
        let pf = branch_preflight(&roots, "release/q3", true, &branch_ab);
        let actions: Vec<(&str, &BranchAction)> = pf
            .rows
            .iter()
            .map(|r| (r.name.as_str(), &r.action))
            .collect();
        assert_eq!(
            actions,
            vec![
                ("app", &BranchAction::CreateFromHead),
                ("ui", &BranchAction::Skip(SkipReason::DirtyWorktree)),
                ("engine", &BranchAction::Skip(SkipReason::Diverged)),
                ("lib", &BranchAction::CheckoutExisting),
                (
                    "cli",
                    &BranchAction::CreateFromUpstream {
                        from: "origin/main".to_string()
                    }
                ),
            ]
        );

        assert_eq!(pf.will_run(), 3, "app, lib, and cli run");
        assert_eq!(pf.total(), 5);
        // Every row targets the same new branch (the TARGET column).
        assert!(pf.rows.iter().all(|r| r.target == "release/q3"));
        // Skips are totaled for the header line.
        assert_eq!(
            pf.skip_totals(),
            vec![(SkipReason::DirtyWorktree, 1), (SkipReason::Diverged, 1)]
        );
    }

    #[test]
    fn branch_preflight_without_apply_broadly_creates_from_head_everywhere() {
        let roots = branch_fleet();
        let pf = branch_preflight(&roots, "release/q3", false, &branch_ab);
        let cli = pf.rows.iter().find(|r| r.name == "cli").unwrap();
        assert_eq!(cli.action, BranchAction::CreateFromHead);
        assert_eq!(
            pf.will_run(),
            3,
            "the policy only changes the base, not the scope"
        );
    }

    #[test]
    fn custom_command_preflight_runs_on_every_selected_repo() {
        // An arbitrary command has no per-repo precondition TurboGit could
        // reason about, so — like fetch — custom commands never skip. The
        // safety gate is the destructive confirmation, not the matrix.
        let roots = fleet();
        let pf = preflight(BulkOp::Custom, &roots, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", true, None),
                ("ui", true, None),
                ("engine", true, None),
                ("lib", true, None),
            ],
        );
        assert_eq!(pf.will_run(), 4);
        assert!(pf.skip_totals().is_empty());
    }

    // -- Custom command (issue 13) -------------------------------------------

    #[test]
    fn a_typed_command_parses_into_git_args_with_the_leading_git_dropped() {
        // The executor always runs the git binary, so the leading "git" the
        // user naturally types is dropped; the rest is verbatim.
        assert_eq!(
            parse_command("git gc"),
            Some(vec!["gc".to_string()]),
            "the leading git is dropped"
        );
        assert_eq!(
            parse_command("remote prune origin"),
            Some(vec![
                "remote".to_string(),
                "prune".to_string(),
                "origin".to_string()
            ])
        );
        assert_eq!(
            parse_command("  git   gc  "),
            Some(vec!["gc".to_string()]),
            "surrounding and repeated whitespace collapses"
        );
    }

    #[test]
    fn quoted_arguments_survive_parsing() {
        assert_eq!(
            parse_command("commit -m \"two words\""),
            Some(vec![
                "commit".to_string(),
                "-m".to_string(),
                "two words".to_string()
            ]),
            "double quotes group words into one argument"
        );
        assert_eq!(
            parse_command("commit -m 'a \"quoted\" bit'"),
            Some(vec![
                "commit".to_string(),
                "-m".to_string(),
                "a \"quoted\" bit".to_string()
            ]),
            "single quotes protect inner double quotes"
        );
    }

    #[test]
    fn an_empty_or_git_only_command_parses_to_nothing() {
        assert_eq!(parse_command(""), None, "empty input is no command");
        assert_eq!(parse_command("   "), None, "whitespace is no command");
        assert_eq!(
            parse_command("git"),
            None,
            "the bare binary name is no command"
        );
    }

    #[test]
    fn destructive_looking_commands_are_flagged_with_a_reason() {
        let reason =
            |cmd: &str| destructive_reason(&parse_command(cmd).expect("test command parses"));
        assert!(
            reason("reset --hard HEAD~1").is_some(),
            "reset is destructive"
        );
        assert!(reason("git clean -fd").is_some(), "clean is destructive");
        assert!(
            reason("rebase -i main").is_some(),
            "rebase rewrites history"
        );
        assert!(
            reason("filter-branch --all").is_some(),
            "filter-branch rewrites history"
        );
        assert!(
            reason("push --force origin main").is_some(),
            "force push rewrites the remote"
        );
        assert!(
            reason("push -f").is_some(),
            "the short force flag is caught too"
        );
        assert!(
            reason("branch -D abandoned").is_some(),
            "force branch deletion is destructive"
        );
        assert_eq!(
            reason("force-with-lease push"),
            None,
            "an argument that merely contains a flag word is not a flag"
        );
    }

    #[test]
    fn everyday_maintenance_commands_are_not_flagged_destructive() {
        let reason =
            |cmd: &str| destructive_reason(&parse_command(cmd).expect("test command parses"));
        assert_eq!(reason("gc"), None, "the issue's own example is safe");
        assert_eq!(reason("remote prune origin"), None);
        assert_eq!(reason("fetch --all"), None);
        assert_eq!(
            reason("push origin main"),
            None,
            "a plain push is not a force push"
        );
        assert!(
            reason("push --force-with-lease origin main").is_some(),
            "force-with-lease still rewrites the remote"
        );
    }
}
