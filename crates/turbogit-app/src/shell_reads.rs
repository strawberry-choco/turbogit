//! The reads a tool window asks for that are not cached values.
//!
//! The Shell's two git interfaces are *dispatch an Operation* and *read a
//! cached value* (ADR-0020). This module covers the remainder: facts the engine
//! answers one at a time — is this branch merged, what is this tag's signing
//! key, what would a dry-run push say — which have no cache to live in and no
//! mutation to dispatch as. They were `state.executor` calls written in
//! presentation code; they are `AppState` methods now, so the Git engine stays
//! below the app boundary and the Shell cannot reach it at all.
//!
//! Each is a synchronous read on the calling thread, exactly where it was
//! before. Batching, TTLs and a keyed-read slot are separate work; nothing
//! here changes *what* is asked or *when*.

use std::path::{Path, PathBuf};

use turbogit_domain::model::{
    Commit, CommitId, ConflictVersions, LogOpts, MergeStrategy, RebaseAction, RebasePlanEntry,
    RefState, Root, RootId,
};
use turbogit_services::history_editor::RebaseCaution;
use turbogit_services::integrate_service::MergePreview;

use crate::state::AppState;

impl AppState {
    /// Commits on `branch` that are not on the current branch — the number a
    /// delete confirmation discloses before it destroys them (issue 12). The
    /// range is the engine's to spell; this names the question.
    pub fn unmerged_commit_count(&self, root: &RootId, branch: &str) -> Option<usize> {
        self.executor
            .commit_count_between(root.as_path(), "HEAD", branch)
            .ok()
    }

    /// The sha `branch` points at, captured so a deletion can be undone inside
    /// the short window even though the ref is gone (issue 12).
    pub fn branch_tip(&self, root: &RootId, branch: &str) -> Option<String> {
        self.executor.resolve_revision(root.as_path(), branch).ok()
    }

    /// The reference an aborted history rewrite returns to, named by the
    /// **Git engine** so a surface can disclose it without guessing the spelling.
    pub fn rewrite_backup_ref(&self) -> String {
        self.executor.rewrite_backup_ref().to_string()
    }

    /// The names of `root`'s tags. Both branch trees and the tag dialog ask;
    /// each keeps its own caching policy, so this is the bare read. `None`
    /// when git could not answer, which lets a caller that caches the list
    /// ask again next frame rather than cache an empty one.
    pub fn tag_names(&self, root: &RootId) -> Option<Vec<String>> {
        self.executor.tag_list(root.as_path()).ok()
    }

    /// Warm the shared per-root tag list for every registered root (plan D10):
    /// whichever tool window opens first fills it for both. The bare read
    /// carries no decoration states, so a surface holding real ref
    /// decorations upgrades them.
    pub fn warm_branch_tags(&mut self) {
        let missing: Vec<RootId> = self
            .multi
            .roots
            .iter()
            .filter(|r| !self.ui.branches_tags.contains_key(&r.id))
            .map(|r| r.id.clone())
            .collect();
        for id in missing {
            let tags = self
                .tag_names(&id)
                .unwrap_or_default()
                .into_iter()
                .map(|name| (name, RefState::Default))
                .collect();
            self.ui.branches_tags.insert(id, tags);
        }
    }

    /// Whether `ref_name` is an ancestor of `branch` — the merged test behind
    /// the delete confirmation's warning. `None` when git could not say, which
    /// the caller treats as "do not warn".
    pub fn is_ancestor(&self, root: &RootId, ref_name: &str, branch: &str) -> Option<bool> {
        self.executor
            .is_ancestor(root.as_path(), ref_name, branch)
            .ok()
    }

    /// The three sides of one conflicted path, for the structured merge editor.
    pub fn merge_versions(&self, root: &RootId, path: &Path) -> Option<ConflictVersions> {
        turbogit_services::conflict::read_versions(self.executor.as_ref(), root.as_path(), path)
            .ok()
    }

    /// Files git already merged cleanly while `root` is mid-conflict, so the
    /// resolver can list them apart from the ones needing a decision. `None`
    /// when git could not answer, which leaves the list the resolver already
    /// painted rather than blanking it.
    pub fn auto_merged_files(&self, root: &RootId, conflicted: &[PathBuf]) -> Option<Vec<PathBuf>> {
        self.executor
            .merge_auto_merged_files(root.as_path(), conflicted)
            .ok()
    }

    /// The ahead-of-upstream warning a delete-branch confirmation shows
    /// (issue #02). The branch's tracking upstream is read from git config and
    /// tested with `is_ancestor`; `Some(message)` when the branch carries
    /// commits its upstream does not, `None` when it is fully merged or when
    /// the query fails.
    ///
    pub fn branch_ahead_warning(&self, branch: &str) -> Option<String> {
        let root = self.selected_root.clone()?;
        // The engine answers the upstream as the pair it is; `git_ref` is the
        // only place the two halves meet again, and a branch tracking another
        // local branch already answers as its plain name.
        let upstream = self
            .executor
            .branch_upstream(root.as_path(), branch)
            .ok()??;
        if self
            .is_ancestor(&root, &upstream.git_ref(), branch)
            .unwrap_or(false)
        {
            return None;
        }
        Some(format!(
            "'{branch}' is ahead of '{}' and not merged",
            upstream.git_ref()
        ))
    }

    /// What a merge of `target` into the current branch would bring in
    /// (issue 09's preview box).
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

    /// The commit `commit` was built on — the base an interactive rebase of it
    /// would replay from (issue 30).
    pub fn commit_base_ref(&self, root: &RootId, commit: &str) -> Option<String> {
        turbogit_services::history_editor::base_of(self.executor.as_ref(), root.as_path(), commit)
    }

    /// The `base..HEAD` plan an interactive rebase would replay, oldest first
    /// (issue 30, screen 17).
    pub fn rebase_plan(&self, root: &RootId, base: &str) -> Option<Vec<RebasePlanEntry>> {
        turbogit_services::history_editor::build_plan(self.executor.as_ref(), root.as_path(), base)
            .ok()
    }

    /// The plan one targeted history verb would run for `commit` (ticket 10):
    /// from its first parent to the tip, with that commit's own row set to
    /// `action`. The verb is the preflight's subject and the operation's, so
    /// both read the plan from here and neither guesses it.
    ///
    /// An `Err` is the SERVICE'S OWN MESSAGE, not a paraphrase of it, and the
    /// caller shows it as-is. That is the difference from the `Option` reads
    /// above: those answer "is there one?" and absence is a normal answer, but a
    /// refusal here is a sentence the service already wrote for a developer —
    /// "has no first parent to rewrite from", "is not on the current branch" —
    /// and throwing it away leaves a caller able to say only that something is
    /// wrong. Keyed the same way as `outgoing_per_root`.
    /// `message` is the replacement a reword writes and a drop has none, so it
    /// rides the plan row for the verb rather than being a second read: git's
    /// rebase todo has no slot for it (ADR-0025), and the row is what the
    /// operation hands the engine.
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

    /// The safety warnings on a plan — protected or shared branches it
    /// rewrites (issue 30's RECOVERY rail).
    pub fn rebase_cautions(&self, root: &RootId, plan: &[RebasePlanEntry]) -> Vec<RebaseCaution> {
        turbogit_services::history_editor::cautions(self.executor.as_ref(), root.as_path(), plan)
    }

    /// The `user.signingkey` git would sign an annotated tag with (issue 31).
    pub fn signing_key(&self, root: &RootId) -> Option<String> {
        turbogit_services::tag_service::signing_key(self.executor.as_ref(), root.as_path())
    }

    /// The newest `max` commits of `root`, uncapped by the log window's batches:
    /// the tag dialog's target picker lists commits the cached window may not
    /// have reached yet.
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

    /// The whole uncapped log of `root`, for hydrating ids that a batch window
    /// cannot be relied on to contain.
    pub fn full_log(&self, root: &RootId) -> Vec<Commit> {
        self.executor
            .log(root.as_path(), &LogOpts::default())
            .unwrap_or_default()
    }

    /// Commits reachable from `to` but not from `from` (issue 15's compare,
    /// issue #20's push dialog).
    pub fn outgoing(&self, root: &RootId, from: &str, to: &str) -> Vec<CommitId> {
        self.executor
            .outgoing_commits(root.as_path(), from, to)
            .unwrap_or_default()
    }

    /// Every root's outgoing commits, in registration order (issue #20). An
    /// `Err` names the root that refused to answer rather than dropping it.
    pub fn outgoing_per_root(&self) -> Vec<(RootId, Result<Vec<CommitId>, String>)> {
        turbogit_services::sync_service::outgoing_per_root(self.executor.as_ref(), &self.multi)
            .into_iter()
            .map(|(id, res)| (id, res.map_err(|e| e.to_string())))
            .collect()
    }

    /// What `git push --dry-run` says for the dialog's Preview button
    /// (issue #25). One root through the dialog's own Remote/Branch fields;
    /// several through the per-root service, which resolves each remote and
    /// gates protected branches. The reports are keyed by whatever the engine
    /// named, so the caller does the naming.
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

    /// The branch checked out at `path`, for the Welcome screen's project rows
    /// (issue 13). Detached HEAD and a missing directory both answer `None`.
    pub fn current_branch_of(&self, path: &Path) -> Option<String> {
        self.executor.current_branch(path).ok().flatten()
    }
}
