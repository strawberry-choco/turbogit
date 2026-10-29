use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BulkOp {
    FetchAll,
    PullAll,
    PushAll,
    StashAll,
    CreateBranch,
    Custom,
    Commit,
    Merge,
}

impl BulkOp {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    DirtyWorktree,
    Diverged,
    NoUpstream,
    CleanTree,
    ProtectedBranch,
}

impl SkipReason {
    pub fn label(self) -> &'static str {
        match self {
            SkipReason::DirtyWorktree => "dirty worktree",
            SkipReason::Diverged => "diverged",
            SkipReason::NoUpstream => "no upstream",
            SkipReason::CleanTree => "clean tree",
            SkipReason::ProtectedBranch => "protected branch",
        }
    }
}

fn tally_skips(reasons: impl Iterator<Item = SkipReason>) -> Vec<(SkipReason, usize)> {
    let mut totals: Vec<(SkipReason, usize)> = Vec::new();
    for reason in reasons {
        match totals.iter_mut().find(|(k, _)| *k == reason) {
            Some((_, n)) => *n += 1,
            None => totals.push((reason, 1)),
        }
    }
    totals.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.label().cmp(b.0.label())));
    totals
}

#[derive(Debug, Clone)]
pub struct PreflightRow {
    pub root: RootId,
    pub name: String,
    pub branch: Option<String>,
    pub dirty: usize,
    pub ahead: usize,
    pub behind: usize,
    pub outcome: Result<(), SkipReason>,
}

#[derive(Debug, Clone, Default)]
pub struct Preflight {
    pub rows: Vec<PreflightRow>,
}

impl Preflight {
    pub fn will_run(&self) -> usize {
        self.rows.iter().filter(|r| r.outcome.is_ok()).count()
    }

    pub fn total(&self) -> usize {
        self.rows.len()
    }

    pub fn skip_totals(&self) -> Vec<(SkipReason, usize)> {
        tally_skips(self.rows.iter().filter_map(|r| r.outcome.err()))
    }
}

/// Compute the preflight matrix: classify every selected root for `op`.
/// `ahead_behind` supplies each root's ↑/↓ counts vs its upstream (the root
/// caches in the app; test fixtures otherwise). `settings` carries the
/// protected-branch patterns and `rebase` the pull policy the confirmed run
pub fn preflight(
    op: BulkOp,
    roots: &[Root],
    settings: &VcsSettings,
    rebase: bool,
    ahead_behind: &dyn Fn(&RootId) -> (usize, usize),
) -> Preflight {
    Preflight {
        rows: roots
            .iter()
            .map(|root| {
                let (ahead, behind) = ahead_behind(&root.id);
                let dirty = root.status.changes.len();
                let upstream = super::sync_service::tracked_upstream(root).is_some();
                let protected = root
                    .current_branch
                    .as_deref()
                    .is_some_and(|branch| super::sync_service::is_protected(settings, branch));
                let outcome = classify(op, dirty, ahead, behind, upstream, protected, rebase);
                PreflightRow {
                    root: root.id.clone(),
                    name: root.path.display_name_or("?"),
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

fn classify(
    op: BulkOp,
    dirty: usize,
    ahead: usize,
    behind: usize,
    upstream: bool,
    protected: bool,
    rebase: bool,
) -> Result<(), SkipReason> {
    match op {
        BulkOp::FetchAll => Ok(()),
        BulkOp::Custom => {
            if protected {
                Err(SkipReason::ProtectedBranch)
            } else {
                Ok(())
            }
        }
        BulkOp::PullAll => {
            if rebase && protected {
                Err(SkipReason::ProtectedBranch)
            } else if dirty > 0 {
                Err(SkipReason::DirtyWorktree)
            } else if ahead > 0 && behind > 0 {
                Err(SkipReason::Diverged)
            } else if !upstream {
                Err(SkipReason::NoUpstream)
            } else {
                Ok(())
            }
        }
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
        BulkOp::Commit => Ok(()),
    }
}

#[derive(Debug, Clone)]
pub struct BulkPlan {
    pub op: BulkOp,
    pub roots: Vec<RootId>,
    pub rebase: bool,
    pub branch: String,
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

/// Parse the command a user typed into the custom-command modal into git
/// arguments. The executor always runs the git binary, so a leading "git"
/// the user naturally types is dropped and the rest is taken verbatim,
/// whitespace-separated, with single- or double-quoted runs grouping into
/// one argument. `None` when nothing remains to run (empty input, or just
/// the bare "git").
pub fn parse_command(input: &str) -> Option<Vec<String>> {
    let mut args = tokenize(input)?;
    if args.first().map(String::as_str) == Some(GIT) {
        args.remove(0);
    }
    if args.is_empty() { None } else { Some(args) }
}

const GIT: &str = "git";

pub fn echo_command(input: &str) -> String {
    let typed = input.trim();
    match tokenize(typed) {
        Some(words) if words.first().map(String::as_str) == Some(GIT) => typed.to_string(),
        _ if typed.is_empty() => GIT.to_string(),
        _ => format!("{GIT} {typed}"),
    }
}

fn tokenize(input: &str) -> Option<Vec<String>> {
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
    Some(args)
}

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
        BulkOp::PullAll => {
            let method = if rebase {
                UpdateMethod::Rebase
            } else {
                UpdateMethod::Merge
            };
            super::sync_service::pull_guarded(
                vcs,
                &root.path,
                method,
                settings,
                root.current_branch.as_deref().unwrap_or_default(),
            )
        }
        BulkOp::PushAll => push_root(vcs, root, settings),
        BulkOp::StashAll => {
            super::shelve_stash::stash(vcs, &root.path, "TurboGit bulk stash", false)
        }
        BulkOp::CreateBranch => run_branch_step(vcs, root, branch, rebase),
        BulkOp::Custom => {
            if let Some(branch) = root.current_branch.as_deref()
                && super::sync_service::is_protected(settings, branch)
            {
                return Err(TgError::ProtectedBranch {
                    branch: branch.to_string(),
                    consequence: "custom commands do not run on it",
                });
            }
            let args = parse_command(command)
                .ok_or_else(|| TgError::Other("empty custom command".into()))?;
            vcs.run_raw(&root.path, &args).map(|_| ())
        }
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
        let tracking = super::sync_service::tracked_upstream(root).cloned();
        match (root.current_branch.as_deref(), tracking) {
            (Some(cur), Some(up))
                if vcs
                    .ahead_behind(&root.path, cur, &up.git_ref())
                    .map(|(_, behind)| behind > 0)
                    .unwrap_or(false) =>
            {
                Some(up.git_ref())
            }
            _ => None,
        }
    } else {
        None
    };
    super::branch_service::create(vcs, &root.path, name, start_point.as_deref(), true)
}

fn push_root(
    vcs: &dyn turbogit_engine_api::GitExecutor,
    root: &Root,
    settings: &VcsSettings,
) -> TgResult<()> {
    let branch = root
        .current_branch
        .as_deref()
        .ok_or_else(|| TgError::Other("no current branch to push".into()))?;
    let remote = super::sync_service::push_remote(root);
    super::sync_service::push(
        vcs, &root.path, &remote, branch, false, false, false, false, None, settings,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchAction {
    CheckoutExisting,
    CreateFromHead,
    CreateFromUpstream { from: String },
    Skip(SkipReason),
}

#[derive(Debug, Clone)]
pub struct BranchPreflightRow {
    pub root: RootId,
    pub name: String,
    pub branch: Option<String>,
    pub dirty: usize,
    pub target: String,
    pub action: BranchAction,
}

#[derive(Debug, Clone)]
pub struct BranchPreflight {
    pub name: String,
    pub rows: Vec<BranchPreflightRow>,
}

impl BranchPreflight {
    pub fn will_run(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| !matches!(r.action, BranchAction::Skip(_)))
            .count()
    }

    pub fn total(&self) -> usize {
        self.rows.len()
    }

    pub fn skip_totals(&self) -> Vec<(SkipReason, usize)> {
        tally_skips(self.rows.iter().filter_map(|r| match r.action {
            BranchAction::Skip(reason) => Some(reason),
            _ => None,
        }))
    }

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
                let tracking = super::sync_service::tracked_upstream(root).cloned();
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
                        Some(up) => BranchAction::CreateFromUpstream { from: up.git_ref() },
                        None => BranchAction::CreateFromHead,
                    }
                } else {
                    BranchAction::CreateFromHead
                };
                BranchPreflightRow {
                    root: root.id.clone(),
                    name: root.path.display_name_or("?"),
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

    fn root(path: &str, branch: Option<&str>, tracking: Option<&str>, dirty: usize) -> Root {
        let id = RootId(PathBuf::from(path).into());
        let branches = match (branch, tracking) {
            (Some(b), Some(t)) => vec![Branch {
                name: b.to_string(),
                kind: BranchKind::Local,
                tracking: Upstream::from_git_ref(t),
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
                conflict_kinds: vec![],
            },
        }
    }

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

    fn settings() -> VcsSettings {
        VcsSettings::default()
    }

    #[test]
    fn preflight_classifies_every_selected_repo_per_operation() {
        let roots = fleet();

        let pf = preflight(BulkOp::FetchAll, &roots, &settings(), false, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", true, None),
                ("ui", true, None),
                ("engine", true, None),
                ("lib", true, None),
            ],
        );

        let pf = preflight(BulkOp::PullAll, &roots, &settings(), false, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", true, None),
                ("ui", false, Some(SkipReason::DirtyWorktree)),
                ("engine", false, Some(SkipReason::Diverged)),
                ("lib", false, Some(SkipReason::NoUpstream)),
            ],
        );

        let pf = preflight(BulkOp::PushAll, &roots, &settings(), false, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", true, None),
                ("ui", true, None),
                ("engine", false, Some(SkipReason::Diverged)),
                ("lib", false, Some(SkipReason::NoUpstream)),
            ],
        );

        let pf = preflight(BulkOp::StashAll, &roots, &settings(), false, &ab);
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
        let pf = preflight(BulkOp::PullAll, &roots, &settings(), false, &ab);

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

        let pf = preflight(BulkOp::PullAll, &roots, &settings(), false, &ab);
        assert_eq!(pf.will_run(), 1, "only alpha runs");
        assert_eq!(pf.total(), 4);
        let totals: Vec<(SkipReason, usize)> = pf.skip_totals();
        assert_eq!(
            totals,
            vec![
                (SkipReason::DirtyWorktree, 1),
                (SkipReason::Diverged, 1),
                (SkipReason::NoUpstream, 1),
            ]
        );

        let clean = vec![root("/w/alpha", Some("main"), Some("origin/main"), 0)];
        let pf = preflight(BulkOp::PullAll, &clean, &settings(), false, &|_| (0, 0));
        assert_eq!(pf.will_run(), 1);
        assert_eq!(pf.skip_totals().len(), 0);
    }

    fn branch_fleet() -> Vec<Root> {
        let mut roots = vec![
            root("/w/app", Some("main"), Some("origin/main"), 0),
            root("/w/ui", Some("main"), Some("origin/main"), 3),
            root("/w/engine", Some("dev"), Some("origin/dev"), 0),
            root("/w/lib", Some("main"), Some("origin/main"), 0),
            root("/w/cli", Some("main"), Some("origin/main"), 0),
        ];
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
        assert!(pf.rows.iter().all(|r| r.target == "release/q3"));
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
    fn custom_command_preflight_refuses_protected_roots_and_runs_on_the_rest() {
        let roots = fleet();
        let pf = preflight(BulkOp::Custom, &roots, &settings(), false, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", false, Some(SkipReason::ProtectedBranch)),
                ("ui", false, Some(SkipReason::ProtectedBranch)),
                ("engine", true, None),
                ("lib", true, None),
            ],
        );
        assert_eq!(pf.will_run(), 2);
        assert_eq!(pf.skip_totals(), vec![(SkipReason::ProtectedBranch, 2)]);
        let alpha = pf.rows.iter().find(|r| r.name == "alpha").unwrap();
        assert_eq!(alpha.branch.as_deref(), Some("main"));
    }

    #[test]
    fn pull_all_preflight_skips_protected_roots_only_when_the_run_rebases() {
        let roots = fleet();
        let pf = preflight(BulkOp::PullAll, &roots, &settings(), true, &ab);
        assert_outcomes(
            &pf,
            &[
                ("alpha", false, Some(SkipReason::ProtectedBranch)),
                ("ui", false, Some(SkipReason::ProtectedBranch)),
                ("engine", false, Some(SkipReason::Diverged)),
                ("lib", false, Some(SkipReason::NoUpstream)),
            ],
        );
        assert_eq!(pf.will_run(), 0);
        let pf = preflight(BulkOp::PullAll, &roots, &settings(), false, &ab);
        assert_eq!(pf.will_run(), 1, "alpha runs in merge mode");
    }

    #[test]
    fn a_typed_command_parses_into_git_args_with_the_leading_git_dropped() {
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
            "the binary alone is nothing to run"
        );
    }

    #[test]
    fn an_echoed_command_re_parses_to_the_arguments_that_ran() {
        for typed in [
            "git gc",
            "gc",
            "  git   prune origin ",
            "\"git status\"",
            "git",
            "commit -m 'two words'",
            "remote prune origin",
        ] {
            let echoed = echo_command(typed);
            assert_eq!(
                parse_command(&echoed),
                parse_command(typed),
                "the echo drifted from what runs: {typed:?} → {echoed:?}"
            );
            assert!(
                echoed == "git" || echoed.starts_with("git "),
                "the echo names the binary, which the engine always runs: {echoed:?}"
            );
            assert_ne!(
                echoed
                    .strip_prefix("git ")
                    .and_then(parse_command)
                    .and_then(|args| args.first().map(String::from)),
                Some("git".to_string()),
                "one binary name, not two: the user's own typing is an argument, \
                 not a second chance to say `git`: {echoed:?} from {typed:?}"
            );
        }
        assert_eq!(
            echo_command("commit -m 'two words'"),
            "git commit -m 'two words'"
        );
        assert_eq!(echo_command("\"git status\""), "git \"git status\"");
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
