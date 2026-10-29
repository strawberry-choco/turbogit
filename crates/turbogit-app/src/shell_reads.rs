use std::collections::HashMap;
use std::path::{Path, PathBuf};

use turbogit_domain::model::{
    Commit, CommitId, ConflictVersions, LogOpts, MergeStrategy, RebaseAction, RebasePlanEntry,
    Root, RootId, Upstream,
};
use turbogit_services::history_editor::RebaseCaution;
use turbogit_services::integrate_service::MergePreview;

use crate::state::{AppState, UnmergedCommit, UnmergedCommits};

impl AppState {
    pub fn unmerged_commit_count(&self, root: &RootId, branch: &str) -> Option<usize> {
        self.executor
            .commit_count_between(root.as_path(), "HEAD", branch)
            .ok()
    }

    pub fn unmerged_commits(&self, root: &RootId, branch: &str) -> Option<UnmergedCommits> {
        let merged_target = self
            .multi
            .by_id(root)
            .and_then(|r| r.current_branch.clone())?;
        let shas = self
            .executor
            .outgoing_commits(root.as_path(), branch, "HEAD")
            .ok()?;
        if shas.is_empty() {
            return None;
        }
        let subjects: HashMap<CommitId, String> = self
            .executor
            .log(
                root.as_path(),
                &LogOpts {
                    max_count: Some(shas.len()),
                    branch: Some(format!("HEAD..{branch}")),
                    ..Default::default()
                },
            )
            .map(|log| {
                log.iter()
                    .map(|c| (c.id.clone(), c.subject().to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        Some(UnmergedCommits {
            root: root.clone(),
            branch: branch.to_string(),
            merged_target,
            tip_sha: self.branch_tip(root, branch),
            commits: shas
                .iter()
                .map(|sha| UnmergedCommit {
                    id: sha.clone(),
                    subject: subjects.get(sha).cloned(),
                })
                .collect(),
        })
    }

    pub fn branch_upstream(&self, root: &RootId, branch: &str) -> Option<Upstream> {
        self.executor
            .branch_upstream(root.as_path(), branch)
            .ok()
            .flatten()
    }

    pub fn branch_tip(&self, root: &RootId, branch: &str) -> Option<String> {
        self.executor.resolve_revision(root.as_path(), branch).ok()
    }

    pub fn rewrite_backup_ref(&self) -> String {
        self.executor.rewrite_backup_ref().to_string()
    }

    pub fn tag_names(&self, root: &RootId) -> Option<Vec<String>> {
        self.executor.tag_list(root.as_path()).ok()
    }

    pub fn warm_branch_tags(&mut self) {
        let ids: Vec<RootId> = self.multi.roots.iter().map(|r| r.id.clone()).collect();
        let exec = self.executor.clone();
        for id in ids {
            self.caches.ensure_tags(exec.as_ref(), &id);
        }
    }

    pub fn warm_branch_recents(&mut self) {
        let ids: Vec<RootId> = self.multi.roots.iter().map(|r| r.id.clone()).collect();
        let exec = self.executor.clone();
        for id in ids {
            self.caches.ensure_recents(exec.as_ref(), &id);
        }
    }

    /// the delete confirmation's warning. `None` when git could not say, which
    /// the caller treats as "do not warn".
    pub fn is_ancestor(&self, root: &RootId, ref_name: &str, branch: &str) -> Option<bool> {
        self.executor
            .is_ancestor(root.as_path(), ref_name, branch)
            .ok()
    }

    pub fn merge_versions(&self, root: &RootId, path: &Path) -> Option<ConflictVersions> {
        turbogit_services::conflict::read_versions(self.executor.as_ref(), root.as_path(), path)
            .ok()
    }

    pub fn auto_merged_files(&self, root: &RootId, conflicted: &[PathBuf]) -> Option<Vec<PathBuf>> {
        self.executor
            .merge_auto_merged_files(root.as_path(), conflicted)
            .ok()
    }

    pub fn branch_ahead_warning(&self, root: &RootId, branch: &str) -> Option<String> {
        let upstream = self
            .executor
            .branch_upstream(root.as_path(), branch)
            .ok()??;
        if self
            .is_ancestor(root, &upstream.git_ref(), branch)
            .unwrap_or(false)
        {
            return None;
        }
        Some(format!(
            "'{branch}' is ahead of '{}' and not merged",
            upstream.git_ref()
        ))
    }

    pub fn merge_preview(
        &self,
        root: &RootId,
        target: &str,
        strategy: MergeStrategy,
    ) -> Option<MergePreview> {
        turbogit_services::integrate_service::merge_preview(
            self.executor.as_ref(),
            root.as_path(),
            target,
            strategy,
        )
        .ok()
    }

    pub fn commit_base_ref(&self, root: &RootId, commit: &str) -> Option<String> {
        turbogit_services::history_editor::base_of(self.executor.as_ref(), root.as_path(), commit)
    }

    pub fn rebase_plan(&self, root: &RootId, base: &str) -> Option<Vec<RebasePlanEntry>> {
        turbogit_services::history_editor::build_plan(self.executor.as_ref(), root.as_path(), base)
            .ok()
    }

    pub fn revision_exists(&self, root: &RootId, rev: &str) -> bool {
        let rev = rev.trim();
        !rev.is_empty() && self.executor.resolve_revision(root.as_path(), rev).is_ok()
    }

    /// An `Err` is the SERVICE'S OWN MESSAGE, not a paraphrase of it, and the
    /// caller shows it as-is. That is the difference from the `Option` reads
    /// above: those answer "is there one?" and absence is a normal answer, but a
    pub fn history_verb_plan(
        &self,
        root: &RootId,
        commit: &str,
        action: RebaseAction,
        message: Option<String>,
    ) -> Result<Vec<RebasePlanEntry>, String> {
        turbogit_services::history_editor::targeted_plan(
            self.executor.as_ref(),
            root.as_path(),
            commit,
            action,
            message,
        )
        .map_err(|e| e.to_string())
    }

    pub fn rebase_cautions(&self, root: &RootId, plan: &[RebasePlanEntry]) -> Vec<RebaseCaution> {
        turbogit_services::history_editor::cautions(self.executor.as_ref(), root.as_path(), plan)
    }

    pub fn signing_key(&self, root: &RootId) -> Option<String> {
        turbogit_services::tag_service::signing_key(self.executor.as_ref(), root.as_path())
    }

    pub fn recent_commits(&self, root: &RootId, max: usize) -> Option<Vec<Commit>> {
        self.executor
            .log(
                root.as_path(),
                &LogOpts {
                    max_count: Some(max),
                    ..Default::default()
                },
            )
            .ok()
    }

    pub fn full_log(&self, root: &RootId) -> Vec<Commit> {
        self.executor
            .log(root.as_path(), &LogOpts::default())
            .unwrap_or_default()
    }

    pub fn outgoing(&self, root: &RootId, from: &str, to: &str) -> Vec<CommitId> {
        self.executor
            .outgoing_commits(root.as_path(), from, to)
            .unwrap_or_default()
    }

    pub fn outgoing_per_root(&self) -> Vec<(RootId, Result<Vec<CommitId>, String>)> {
        turbogit_services::sync_service::outgoing_per_root(self.executor.as_ref(), &self.multi)
            .into_iter()
            .map(|(id, res)| (id, res.map_err(|e| e.to_string())))
            .collect()
    }

    pub fn push_dry_run_reports(
        &self,
        roots: &[Root],
        single: Option<(&str, &str)>,
        force: bool,
        narrowed_sha: Option<&str>,
    ) -> Vec<(String, Result<String, String>)> {
        let exec = self.executor.as_ref();
        let verbatim = |e: turbogit_domain::error::TgError| match e {
            turbogit_domain::error::TgError::Cli { stderr, .. } => stderr,
            other => other.to_string(),
        };
        match (roots, single) {
            ([root], Some((remote, branch))) => {
                let res = exec
                    .push_dry_run(&root.path, remote, branch, force)
                    .map_err(verbatim);
                vec![(remote.to_string(), res)]
            }
            _ => {
                let refs: Vec<&Root> = roots.iter().collect();
                turbogit_services::sync_service::push_dry_run_roots(
                    exec,
                    &refs,
                    force,
                    narrowed_sha,
                )
                .into_iter()
                .map(|(remote, res)| (remote, res.map_err(verbatim)))
                .collect()
            }
        }
    }

    pub fn current_branch_of(&self, path: &Path) -> Option<String> {
        self.executor.current_branch(path).ok().flatten()
    }
}
