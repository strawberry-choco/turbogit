use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::{GitExecutor, OwnedOperation};

use turbogit_engine_api::patch::parse_patch;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Add(Vec<PathBuf>),
    Unstage(Vec<PathBuf>),
    CommitAll,
    CommitIndex,
    Push {
        root: PathBuf,
        remote: String,
        branch: String,
        force: bool,
        tags: bool,
        no_verify: bool,
        set_upstream: bool,
        selected_oldest: Option<String>,
    },
    PushDryRun {
        root: PathBuf,
        remote: String,
        branch: String,
        force: bool,
    },
    ApplyPatch {
        direction: turbogit_engine_api::ApplyDirection,
    },
    ApplyPatchToWorktree {
        paths: Vec<PathBuf>,
    },
    AddIntentToAdd(Vec<PathBuf>),
    TagCreate {
        spec: TagSpec,
    },
    TagPush {
        root: PathBuf,
        remote: String,
        name: Option<String>,
    },
}

pub struct FakeExecutor {
    pub repos: Mutex<Vec<PathBuf>>,
    pub files: Mutex<HashMap<PathBuf, String>>,
    pub files_bytes: Mutex<HashMap<PathBuf, Vec<u8>>>,
    pub conflicts: HashMap<PathBuf, ConflictVersions>,
    pub change_stats: HashMap<ChangeQuestion, ChangeStats>,
    pub ancestors: HashSet<(String, String)>,
    pub commit_counts: HashMap<(String, String), usize>,
    pub revisions: HashMap<String, CommitId>,
    pub upstreams: HashMap<String, Upstream>,
    pub branches: HashMap<PathBuf, Vec<Branch>>,
    pub recents: HashMap<PathBuf, Vec<RecentCheckout>>,
    pub current_branch: HashMap<PathBuf, Option<String>>,
    pub remotes: HashMap<PathBuf, Vec<Remote>>,
    pub status: HashMap<PathBuf, RootStatus>,
    pub holders: HashMap<PathBuf, OwnedOperation>,
    pub config: HashMap<(PathBuf, String), String>,
    pub diffs: HashMap<PathBuf, String>,
    pub logs: HashMap<PathBuf, Vec<Commit>>,
    pub reject_force_branches: Vec<String>,
    pub calls: Mutex<Vec<Call>>,
}

impl Default for FakeExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeExecutor {
    pub fn new() -> Self {
        Self {
            repos: Mutex::new(Vec::new()),
            files: Mutex::new(HashMap::new()),
            files_bytes: Mutex::new(HashMap::new()),
            conflicts: HashMap::new(),
            diffs: HashMap::new(),
            change_stats: HashMap::new(),
            ancestors: HashSet::new(),
            commit_counts: HashMap::new(),
            revisions: HashMap::new(),
            upstreams: HashMap::new(),
            branches: HashMap::new(),
            recents: HashMap::new(),
            current_branch: HashMap::new(),
            remotes: HashMap::new(),
            status: HashMap::new(),
            holders: HashMap::new(),
            config: HashMap::new(),
            logs: HashMap::new(),
            reject_force_branches: Vec::new(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn local_branch(&self, root: &Path) -> Option<Branch> {
        let cur = self.current_branch.get(root).cloned().flatten()?;
        Some(Branch {
            name: cur,
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
        })
    }
}

impl GitExecutor for FakeExecutor {
    fn status(&self, root: &Path) -> TgResult<RootStatus> {
        Ok(self.status.get(root).cloned().unwrap_or_default())
    }

    fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
        let all = self.logs.get(root).map(Vec::as_slice).unwrap_or_default();
        let rest = all.get(opts.skip.unwrap_or(0)..).unwrap_or_default();
        Ok(match opts.max_count {
            Some(n) => rest.iter().take(n).cloned().collect(),
            None => rest.to_vec(),
        })
    }

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        let mut out = self.branches.get(root).cloned().unwrap_or_default();
        if let Some(cur) = self.local_branch(root)
            && !out
                .iter()
                .any(|b| b.kind == BranchKind::Local && b.name == cur.name)
        {
            out.push(cur);
        }
        Ok(out)
    }

    fn recent_checkouts(&self, root: &Path) -> TgResult<Vec<RecentCheckout>> {
        Ok(self.recents.get(root).cloned().unwrap_or_default())
    }

    fn current_branch(&self, root: &Path) -> TgResult<Option<String>> {
        Ok(self.current_branch.get(root).cloned().flatten())
    }

    fn ahead_behind(
        &self,
        _root: &Path,
        _branch: &str,
        _upstream: &str,
    ) -> TgResult<(usize, usize)> {
        Ok((0, 0))
    }

    fn is_ancestor(&self, _root: &Path, upstream: &str, branch: &str) -> TgResult<bool> {
        Ok(self
            .ancestors
            .contains(&(upstream.to_string(), branch.to_string())))
    }

    fn outgoing_commits(
        &self,
        _root: &Path,
        _branch: &str,
        _upstream: &str,
    ) -> TgResult<Vec<CommitId>> {
        Ok(Vec::new())
    }

    fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>> {
        Ok(self.remotes.get(root).cloned().unwrap_or_default())
    }

    fn stash_list(&self, _root: &Path) -> TgResult<Vec<Stash>> {
        Ok(Vec::new())
    }

    fn worktree_list(&self, _root: &Path) -> TgResult<Vec<Worktree>> {
        Ok(Vec::new())
    }

    fn worktree_dirty(&self, _path: &Path) -> TgResult<bool> {
        Ok(false)
    }

    fn holding_operation(&self, root: &Path) -> Option<OwnedOperation> {
        self.holders.get(root).copied()
    }

    fn submodule_status(&self, _root: &Path) -> TgResult<Vec<Submodule>> {
        Ok(Vec::new())
    }

    fn change_stats(&self, _root: &Path, question: &ChangeQuestion) -> TgResult<ChangeStats> {
        self.change_stats.get(question).cloned().ok_or_else(|| {
            turbogit_domain::error::TgError::Other(format!(
                "fake executor: no change stats seeded for {question:?}"
            ))
        })
    }

    fn commit_count_between(&self, _root: &Path, from: &str, to: &str) -> TgResult<usize> {
        self.commit_counts
            .get(&(from.to_string(), to.to_string()))
            .copied()
            .ok_or_else(|| {
                turbogit_domain::error::TgError::Other(format!(
                    "fake executor: no commit count seeded for {from}..{to}"
                ))
            })
    }

    fn resolve_revision(&self, _root: &Path, name: &str) -> TgResult<CommitId> {
        self.revisions.get(name).cloned().ok_or_else(|| {
            turbogit_domain::error::TgError::Other(format!(
                "fake executor: no revision seeded for `{name}`"
            ))
        })
    }

    fn branch_upstream(&self, _root: &Path, branch: &str) -> TgResult<Option<Upstream>> {
        Ok(self.upstreams.get(branch).cloned())
    }

    fn conflict_versions(&self, _root: &Path, path: &Path) -> TgResult<ConflictVersions> {
        self.conflicts.get(path).cloned().ok_or_else(|| {
            turbogit_domain::error::TgError::Other(format!(
                "fake executor: no conflict seeded for `{}`",
                path.display()
            ))
        })
    }

    fn config_get(&self, root: &Path, key: &str) -> TgResult<Option<String>> {
        Ok(self
            .config
            .get(&(root.to_path_buf(), key.to_string()))
            .cloned())
    }

    fn init(&self, root: &Path) -> TgResult<()> {
        std::fs::create_dir_all(root.join(".git"))?;
        self.repos.lock().unwrap().push(root.to_path_buf());
        Ok(())
    }

    fn clone(&self, _url: &str, dest: &Path, _depth: Option<usize>) -> TgResult<()> {
        std::fs::create_dir_all(dest.join(".git"))?;
        self.repos.lock().unwrap().push(dest.to_path_buf());
        Ok(())
    }

    fn add_remote(&self, _root: &Path, _name: &str, _url: &str) -> TgResult<()> {
        Ok(())
    }

    fn set_remote_url(
        &self,
        _root: &Path,
        _name: &str,
        _fetch_url: Option<&str>,
        _push_url: Option<&str>,
    ) -> TgResult<()> {
        Ok(())
    }

    fn rename_remote(&self, _root: &Path, _old: &str, _new: &str) -> TgResult<()> {
        Ok(())
    }

    fn remove_remote(&self, _root: &Path, _name: &str) -> TgResult<()> {
        Ok(())
    }

    fn set_branch_upstream(&self, _root: &Path, _branch: &str, _upstream: &str) -> TgResult<()> {
        Ok(())
    }

    fn unset_branch_upstream(&self, _root: &Path, _branch: &str) -> TgResult<()> {
        Ok(())
    }

    fn fetch(&self, _root: &Path, _remote: Option<&str>) -> TgResult<()> {
        Ok(())
    }

    fn pull(&self, _root: &Path, _rebase: bool) -> TgResult<()> {
        Ok(())
    }

    fn push(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
        tags: bool,
        no_verify: bool,
        set_upstream: bool,
        selected_oldest: Option<&str>,
    ) -> TgResult<()> {
        if force && self.reject_force_branches.iter().any(|p| p == branch) {
            return Err(turbogit_domain::error::TgError::Other(format!(
                "Refusing force-push to protected branch '{branch}'"
            )));
        }
        self.calls.lock().unwrap().push(Call::Push {
            root: root.to_path_buf(),
            remote: remote.to_string(),
            branch: branch.to_string(),
            force,
            tags,
            no_verify,
            set_upstream,
            selected_oldest: selected_oldest.map(|s| s.to_string()),
        });
        Ok(())
    }

    fn push_dry_run(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> TgResult<String> {
        self.calls.lock().unwrap().push(Call::PushDryRun {
            root: root.to_path_buf(),
            remote: remote.to_string(),
            branch: branch.to_string(),
            force,
        });
        Ok(String::new())
    }

    fn commit(
        &self,
        _root: &Path,
        _message: &str,
        _amend: bool,
        _no_verify: bool,
    ) -> TgResult<CommitId> {
        self.calls.lock().unwrap().push(Call::CommitAll);
        Ok("aaaa".into())
    }

    fn commit_index(
        &self,
        _root: &Path,
        _message: &str,
        _amend: bool,
        _no_verify: bool,
    ) -> TgResult<CommitId> {
        self.calls.lock().unwrap().push(Call::CommitIndex);
        Ok("bbbb".into())
    }

    fn merge(&self, _root: &Path, _target: &str, _opts: &MergeOpts) -> TgResult<()> {
        Ok(())
    }

    fn rebase(&self, _root: &Path, _onto: &str, _opts: &RebaseOpts) -> TgResult<()> {
        Ok(())
    }

    fn cherry_pick(&self, _root: &Path, _commit: &str) -> TgResult<()> {
        Ok(())
    }

    fn abort(&self, _root: &Path, _op: OwnedOperation) -> TgResult<()> {
        Ok(())
    }

    fn rewrite_backup_ref(&self) -> &str {
        "refs/turbogit/fake-preflight-backup"
    }

    fn save_rewrite_backup(&self, _root: &Path) -> TgResult<()> {
        Ok(())
    }

    fn restore_rewrite_backup(&self, _root: &Path) -> TgResult<()> {
        Ok(())
    }

    fn discard_rewrite_backup(&self, _root: &Path) -> TgResult<()> {
        Ok(())
    }

    fn continue_op(&self, _root: &Path, _op: OwnedOperation) -> TgResult<()> {
        Ok(())
    }

    fn rebase_skip(&self, _root: &Path) -> TgResult<()> {
        Ok(())
    }

    fn merge_auto_merged_files(
        &self,
        _root: &Path,
        _conflicted: &[PathBuf],
    ) -> TgResult<Vec<PathBuf>> {
        Ok(Vec::new())
    }

    fn rebase_interactive(&self, _root: &Path, _plan: &[RebasePlanEntry]) -> TgResult<()> {
        Ok(())
    }

    fn stash_push(&self, _root: &Path, _message: &str, _keep_index: bool) -> TgResult<()> {
        Ok(())
    }

    fn stash_pop(&self, _root: &Path, _index: usize) -> TgResult<()> {
        Ok(())
    }

    fn stash_drop(&self, _root: &Path, _index: usize) -> TgResult<()> {
        Ok(())
    }

    fn worktree_add(
        &self,
        _root: &Path,
        _path: &Path,
        _branch: &str,
        _create: bool,
    ) -> TgResult<()> {
        Ok(())
    }

    fn worktree_remove(&self, _root: &Path, _path: &Path, _force: bool) -> TgResult<()> {
        Ok(())
    }

    fn submodule_update(&self, _root: &Path, _path: &Path, _init: bool) -> TgResult<()> {
        Ok(())
    }

    fn submodule_deinit(&self, _root: &Path, _path: &Path, _force: bool) -> TgResult<()> {
        Ok(())
    }

    fn add(&self, _root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.calls.lock().unwrap().push(Call::Add(paths.to_vec()));
        Ok(())
    }

    fn unstage(&self, _root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::Unstage(paths.to_vec()));
        Ok(())
    }

    fn restore(&self, _root: &Path, _paths: &[PathBuf]) -> TgResult<()> {
        Ok(())
    }

    fn apply_patch_to_index(
        &self,
        _root: &Path,
        _patch: &turbogit_domain::model::Patch,
        direction: turbogit_engine_api::ApplyDirection,
    ) -> TgResult<()> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::ApplyPatch { direction });
        Ok(())
    }

    fn apply_patch_to_worktree(&self, _root: &Path, patch: &Patch) -> TgResult<()> {
        let paths: Vec<PathBuf> = patch
            .files
            .iter()
            .flat_map(|f| [f.old_path.as_str(), f.new_path.as_str()])
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect();
        self.calls
            .lock()
            .unwrap()
            .push(Call::ApplyPatchToWorktree { paths });
        Ok(())
    }

    fn add_intent_to_add(&self, _root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::AddIntentToAdd(paths.to_vec()));
        Ok(())
    }

    fn exclude_dir(&self, _root: &Path, _dir: &Path) -> TgResult<()> {
        Ok(())
    }

    fn branch_create(
        &self,
        _root: &Path,
        _name: &str,
        _checkout: bool,
        _start_point: Option<&str>,
    ) -> TgResult<()> {
        Ok(())
    }

    fn branch_checkout(&self, _root: &Path, _name: &str) -> TgResult<()> {
        Ok(())
    }

    fn branch_delete(&self, _root: &Path, _name: &str, _force: bool) -> TgResult<()> {
        Ok(())
    }

    fn branch_delete_remote(&self, _root: &Path, _remote: &str, _name: &str) -> TgResult<()> {
        Ok(())
    }

    fn branch_rename(&self, _root: &Path, _old: &str, _new: &str) -> TgResult<()> {
        Ok(())
    }

    fn tag_create(&self, _root: &Path, spec: &TagSpec) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(Call::TagCreate { spec: spec.clone() });
        Ok(())
    }

    fn tag_list(&self, _root: &Path) -> TgResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn checkout_detached(&self, _root: &Path, _name: &str) -> TgResult<()> {
        Ok(())
    }

    fn tag_push(&self, root: &Path, remote: &str, name: Option<&str>, _all: bool) -> TgResult<()> {
        self.calls.lock().expect("calls mutex").push(Call::TagPush {
            root: root.to_path_buf(),
            remote: remote.to_string(),
            name: name.map(str::to_string),
        });
        Ok(())
    }

    fn diff_patch(&self, root: &Path, _opts: &DiffOpts) -> TgResult<Patch> {
        Ok(parse_patch(
            self.diffs.get(root).cloned().unwrap_or_default().as_str(),
        ))
    }

    fn patch_bytes(&self, root: &Path, _origin: &PatchOrigin, _reverse: bool) -> TgResult<Vec<u8>> {
        Ok(self
            .diffs
            .get(root)
            .cloned()
            .unwrap_or_default()
            .into_bytes())
    }

    fn blame(&self, _root: &Path, _path: &Path, _rev: Option<&str>) -> TgResult<Vec<BlameLine>> {
        Ok(Vec::new())
    }

    fn index_file_bytes(&self, _root: &Path, path: &Path) -> TgResult<Vec<u8>> {
        if let Some(bytes) = self.files_bytes.lock().unwrap().get(path) {
            return Ok(bytes.clone());
        }
        if let Some(text) = self.files.lock().unwrap().get(path) {
            return Ok(text.clone().into_bytes());
        }
        Err(turbogit_domain::error::TgError::Other(format!(
            "fake executor: no index entry for `{}`",
            path.display()
        )))
    }

    fn show_file(&self, _root: &Path, rev: &str, path: &Path) -> TgResult<String> {
        let _ = rev;
        let bytes = self.show_file_bytes(_root, rev, path)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn show_file_bytes(&self, _root: &Path, rev: &str, path: &Path) -> TgResult<Vec<u8>> {
        let _ = rev;
        if let Some(bytes) = self.files_bytes.lock().unwrap().get(path) {
            return Ok(bytes.clone());
        }
        if let Some(text) = self.files.lock().unwrap().get(path) {
            return Ok(text.clone().into_bytes());
        }
        Err(turbogit_domain::error::TgError::Other(format!(
            "fake executor: no content recorded for `{}`",
            path.display()
        )))
    }

    fn revert(&self, _root: &Path, _commit: &str) -> TgResult<()> {
        Ok(())
    }

    fn stash_apply(&self, _root: &Path, _index: usize) -> TgResult<()> {
        Ok(())
    }
}
