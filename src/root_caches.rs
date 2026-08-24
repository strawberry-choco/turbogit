//! Root caches (CONTEXT.md "Root caches"): the in-memory cache layer keyed
//! by repository root — commit logs, ref decorations, changed-file lists,
//! path-scoped logs, and ahead/behind counts.
//!
//! One deep module with a small interface: the five maps are private,
//! lazy-fill logic lives *inside* (`ensure_*`), and callers get typed
//! readers plus event-fed writers. Every invalidation drops all five maps
//! uniformly for its scope ([`Affected::Root`] or everything) — the
//! invariant is never poked field-by-field by callers.
//!
//! The module depends on the [`GitExecutor`] trait only; no UI types.

use crate::engine::GitExecutor;
use crate::model::{Change, Commit, CommitId, CommitRef, LogOpts, RootId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Which roots an operation's results affect — declared at every
/// [`AppState::run_git`](crate::state::AppState::run_git) call site and used
/// to scope cache invalidation and rescans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Affected {
    /// Every registered root (batch operations, unknown scope).
    All,
    /// Only this root.
    Root(RootId),
}

impl Affected {
    /// Scope for an operation that targets an optionally-selected root and
    /// no-ops when none is selected: [`Affected::Root`] when present,
    /// [`Affected::All`] otherwise (harmless — the closure no-ops too).
    pub fn from_optional_root(root: Option<&Path>) -> Self {
        root.map(|p| Affected::Root(RootId(p.to_path_buf())))
            .unwrap_or(Affected::All)
    }
}

/// The five root-keyed caches behind one interface.
#[derive(Default)]
pub struct RootCaches {
    /// Commit logs keyed by root (refreshed on demand / after ops).
    log_cache: HashMap<RootId, Vec<Commit>>,
    /// Ref decorations keyed by root, then commit id (issue #12).
    ref_cache: HashMap<RootId, HashMap<CommitId, Vec<CommitRef>>>,
    /// Changed-file lists keyed by (root, commit id) (issue #12).
    files_cache: HashMap<(RootId, CommitId), Vec<Change>>,
    /// Path-scoped logs keyed by (root, scoped path) (issue #19).
    log_path_cache: HashMap<(RootId, PathBuf), Vec<Commit>>,
    /// Ahead/behind of each root's current branch vs its upstream (Epic D3).
    ahead_behind: HashMap<RootId, (usize, usize)>,
}

impl RootCaches {
    // --- Readers -----------------------------------------------------------

    /// The cached commit log for `root`, if loaded.
    pub fn log(&self, root: &RootId) -> Option<&[Commit]> {
        self.log_cache.get(root).map(|v| v.as_slice())
    }

    /// The cached path-scoped log for `(root, path)`, if loaded (issue #19).
    pub fn path_log(&self, root: &RootId, path: &Path) -> Option<&[Commit]> {
        self.log_path_cache
            .get(&(root.clone(), path.to_path_buf()))
            .map(|v| v.as_slice())
    }

    /// Cached ref decorations for one commit of `root` (empty when absent).
    pub fn refs_for(&self, root: &RootId, commit: &CommitId) -> Vec<CommitRef> {
        self.ref_cache
            .get(root)
            .and_then(|m| m.get(commit))
            .cloned()
            .unwrap_or_default()
    }

    /// Every cached decoration group of `root` (empty when not loaded) —
    /// feeds the branches pane's LOCAL / REMOTE / TAGS union.
    pub fn ref_groups(&self, root: &RootId) -> impl Iterator<Item = &[CommitRef]> + '_ {
        self.ref_cache
            .get(root)
            .into_iter()
            .flatten()
            .map(|(_, refs)| refs.as_slice())
    }

    /// The cached changed-file list of `(root, commit)`, if loaded (issue #12).
    pub fn files_for(&self, root: &RootId, commit: &CommitId) -> Option<&[Change]> {
        self.files_cache
            .get(&(root.clone(), commit.clone()))
            .map(|v| v.as_slice())
    }

    /// Ahead/behind of `root`'s current branch vs its upstream, if known.
    pub fn ahead_behind(&self, root: &RootId) -> Option<(usize, usize)> {
        self.ahead_behind.get(root).copied()
    }

    // --- Compute-on-miss ---------------------------------------------------

    /// Load `root`'s ref decorations through the engine seam unless cached.
    pub fn ensure_refs(&mut self, exec: &dyn GitExecutor, root: &RootId) {
        if self.ref_cache.contains_key(root) {
            return;
        }
        let deco = exec.ref_decorations(&root.0).unwrap_or_default();
        self.ref_cache
            .insert(root.clone(), deco.into_iter().collect());
    }

    /// The changed files of `(root, commit)`, computed through the engine
    /// seam on miss and cached.
    pub fn ensure_files(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        commit: &CommitId,
    ) -> Vec<Change> {
        let key = (root.clone(), commit.clone());
        if let Some(files) = self.files_cache.get(&key) {
            return files.clone();
        }
        let files = exec.commit_files(&root.0, commit).unwrap_or_default();
        self.files_cache.insert(key, files.clone());
        files
    }

    /// The commits touching `path` in `root` (`git log -- <path>`),
    /// computed through the engine seam on miss and cached (issue #19).
    pub fn ensure_path_log(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        path: &Path,
    ) -> Vec<Commit> {
        let key = (root.clone(), path.to_path_buf());
        if let Some(commits) = self.log_path_cache.get(&key) {
            return commits.clone();
        }
        let commits = exec
            .log(
                &root.0,
                &LogOpts {
                    path: Some(key.1.clone()),
                    ..Default::default()
                },
            )
            .unwrap_or_default();
        self.log_path_cache.insert(key, commits.clone());
        commits
    }

    // --- Event-fed writes (called from drain_events) ------------------------

    /// Store a freshly loaded commit log for `root`.
    pub fn store_log(&mut self, root: RootId, commits: Vec<Commit>) {
        self.log_cache.insert(root, commits);
    }

    /// Store freshly computed ahead/behind counts for `root`.
    pub fn store_ahead_behind(&mut self, root: RootId, ab: (usize, usize)) {
        self.ahead_behind.insert(root, ab);
    }

    // --- Invalidation -------------------------------------------------------

    /// Drop all five caches' entries for the affected scope. There are no
    /// per-cache exceptions (policy uniformity): even immutable-by-commit-id
    /// entries go.
    pub fn invalidate(&mut self, affected: Affected) {
        match affected {
            Affected::All => self.invalidate_all(),
            Affected::Root(ref root) => {
                self.log_cache.remove(root);
                self.ref_cache.remove(root);
                self.files_cache.retain(|(r, _), _| r != root);
                self.log_path_cache.retain(|(r, _), _| r != root);
                self.ahead_behind.remove(root);
            }
        }
    }

    /// Drop every entry in all five caches.
    pub fn invalidate_all(&mut self) {
        self.log_cache.clear();
        self.ref_cache.clear();
        self.files_cache.clear();
        self.log_path_cache.clear();
        self.ahead_behind.clear();
    }

    /// True when every map is empty — the invalidation invariant's
    /// diagnostic surface (tests, assertions).
    pub fn is_empty(&self) -> bool {
        self.log_cache.is_empty()
            && self.ref_cache.is_empty()
            && self.files_cache.is_empty()
            && self.log_path_cache.is_empty()
            && self.ahead_behind.is_empty()
    }
}
