//! Root caches (CONTEXT.md "Root caches"): the in-memory cache layer keyed
//! by repository root — commit logs, ref decorations, changed-file lists,
//! path-scoped logs, ahead/behind counts, worktree lists, and submodule
//! lists (issue 14).
//!
//! One deep module with a small interface: the maps are private,
//! lazy-fill logic lives *inside* (`ensure_*`), and callers get typed
//! readers plus event-fed writers. Every invalidation drops all maps
//! uniformly for its scope ([`Affected::Root`] or everything) — the
//! invariant is never poked field-by-field by callers.
//!
//! The module depends on the [`GitExecutor`] trait only; no UI types.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use turbogit_domain::model::{
    Change, Commit, CommitId, CommitRef, DiffOpts, LogOpts, RootId, Submodule, Worktree,
};
use turbogit_engine_api::GitExecutor;
use turbogit_services::hunk_stats::{self, FileHunks};

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
        root.map(|p| Affected::Root(RootId(p.into())))
            .unwrap_or(Affected::All)
    }
}

/// The root-keyed caches behind one interface.
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
    /// Ref-scoped logs keyed by (root, ref) (branch-tree extraction, plan
    /// D9) — mirrors the path-scoped cache: same shape, same cost model.
    log_ref_cache: HashMap<(RootId, String), Vec<Commit>>,
    /// Pickaxe search results keyed by (root, query) (issue 17).
    search_cache: HashMap<(RootId, String), Vec<Commit>>,
    /// Ahead/behind of each root's current branch vs its upstream (Epic D3).
    ahead_behind: HashMap<RootId, (usize, usize)>,
    /// Linked worktrees per root (issue 14).
    worktree_cache: HashMap<RootId, Vec<Worktree>>,
    /// Registered submodules per root (issue 14).
    submodule_cache: HashMap<RootId, Vec<Submodule>>,
    /// Per-root hunk-span statistics over the three working-tree diffs
    /// (issue 20): HEAD↔worktree, HEAD↔index, and index↔worktree, each
    /// parsed into per-file hunk spans.
    hunk_stats: HashMap<RootId, RootHunkStats>,
}

/// The three working-tree diff views of one root, parsed into per-file hunk
/// spans (issue 20): hunk-count badges, per-hunk staged state, and the
/// staged-hunk chip rail all read from this one snapshot.
#[derive(Clone, Debug, Default)]
pub struct RootHunkStats {
    /// HEAD↔worktree (`git diff HEAD`): every change, staged or not.
    pub repo: Vec<FileHunks>,
    /// HEAD↔index (`git diff --cached`): the staged hunks.
    pub staged: Vec<FileHunks>,
    /// index↔worktree (`git diff`): the unstaged hunks.
    pub local: Vec<FileHunks>,
}

impl RootHunkStats {
    /// The per-file hunk spans of `path` in the view `pick` selects.
    pub fn file(&self, pick: StatsView, path: &Path) -> Option<&FileHunks> {
        let view = match pick {
            StatsView::Repo => &self.repo,
            StatsView::Staged => &self.staged,
            StatsView::Local => &self.local,
        };
        let want = path.to_string_lossy().replace('\\', "/");
        view.iter().find(|f| f.path == want)
    }
}

/// Which of the three working-tree diff views [`RootHunkStats::file` reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsView {
    Repo,
    Staged,
    Local,
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

    /// The cached ref-scoped log for `(root, ref)`, if loaded (plan D9).
    pub fn ref_log(&self, root: &RootId, ref_name: &str) -> Option<&[Commit]> {
        self.log_ref_cache
            .get(&(root.clone(), ref_name.to_string()))
            .map(|v| v.as_slice())
    }

    /// The cached pickaxe search result for `(root, query)`, if loaded
    /// (issue 17).
    pub fn search_log(&self, root: &RootId, query: &str) -> Option<&[Commit]> {
        self.search_cache
            .get(&(root.clone(), query.to_string()))
            .map(|v| v.as_slice())
    }

    /// Cached ref decorations for one commit of `root` (empty when absent).
    /// Borrows the cache (plan §1.2): called per visible log row per frame,
    /// so it must not clone the decoration list.
    pub fn refs_for(&self, root: &RootId, commit: &CommitId) -> &[CommitRef] {
        self.ref_cache
            .get(root)
            .and_then(|m| m.get(commit))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
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

    /// The cached worktree list of `root`, if loaded (issue 14).
    pub fn worktrees(&self, root: &RootId) -> Option<&[Worktree]> {
        self.worktree_cache.get(root).map(|v| v.as_slice())
    }

    /// The cached submodule list of `root`, if loaded (issue 14).
    pub fn submodules(&self, root: &RootId) -> Option<&[Submodule]> {
        self.submodule_cache.get(root).map(|v| v.as_slice())
    }

    /// The cached hunk-span statistics of `root`, if loaded (issue 20).
    pub fn hunk_stats(&self, root: &RootId) -> Option<&RootHunkStats> {
        self.hunk_stats.get(root)
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
    /// seam on miss and cached. Returns a borrow of the cached list (plan
    /// §1.2): neither path deep-clones the file list anymore. Callers must
    /// not touch the caches (or any other `&mut AppState` state) while
    /// holding the slice.
    pub fn ensure_files(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        commit: &CommitId,
    ) -> &[Change] {
        let key = (root.clone(), commit.clone());
        if !self.files_cache.contains_key(&key) {
            let files = exec.commit_files(&root.0, commit).unwrap_or_default();
            self.files_cache.insert(key.clone(), files);
        }
        // Reborrow after the optional fill so the returned slice always
        // aliases the cache. The tuple key cannot be probed in borrowed form
        // (`HashMap` has no mixed-reference `Borrow` for tuples), so the
        // owned key is built once up front and cloned only for the insert.
        self.files_cache
            .get(&key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// The commits touching `path` in `root` (`git log -- <path>`),
    /// computed through the engine seam on miss and cached (issue #19).
    /// Returns a borrow of the cached list — same fill-and-reborrow shape
    /// as [`RootCaches::ensure_files`] (plan §1.2).
    pub fn ensure_path_log(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        path: &Path,
    ) -> &[Commit] {
        let key = (root.clone(), path.to_path_buf());
        if !self.log_path_cache.contains_key(&key) {
            let commits = exec
                .log(
                    &root.0,
                    &LogOpts {
                        path: Some(key.1.clone()),
                        ..Default::default()
                    },
                )
                .unwrap_or_default();
            self.log_path_cache.insert(key.clone(), commits);
        }
        self.log_path_cache
            .get(&key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// The commits of `ref_name` in `root` (`git log <ref>`), computed
    /// through the engine seam on miss and cached (plan D9). Returns a
    /// borrow of the cached list — same fill-and-reborrow shape as
    /// [`RootCaches::ensure_files`] (plan §1.2).
    pub fn ensure_ref_log(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        ref_name: &str,
    ) -> &[Commit] {
        let key = (root.clone(), ref_name.to_string());
        if !self.log_ref_cache.contains_key(&key) {
            let commits = exec
                .log(
                    &root.0,
                    &LogOpts {
                        branch: Some(key.1.clone()),
                        ..Default::default()
                    },
                )
                .unwrap_or_default();
            self.log_ref_cache.insert(key.clone(), commits);
        }
        self.log_ref_cache
            .get(&key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// The commits where the occurrence count of `query` changed (pickaxe,
    /// issue 17), computed through the engine seam on miss and cached.
    /// Returns a borrow of the cached list — same fill-and-reborrow shape
    /// as [`RootCaches::ensure_files`] (plan §1.2).
    pub fn ensure_search_log(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        query: &str,
    ) -> &[Commit] {
        let key = (root.clone(), query.to_string());
        if !self.search_cache.contains_key(&key) {
            let commits = exec
                .log(
                    &root.0,
                    &LogOpts {
                        pickaxe: Some(key.1.clone()),
                        ..Default::default()
                    },
                )
                .unwrap_or_default();
            self.search_cache.insert(key.clone(), commits);
        }
        self.search_cache
            .get(&key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    // --- Event-fed writes (called from drain_events) ------------------------

    /// Compute `root`'s hunk-span statistics through the engine seam on miss
    /// and cache them (issue 20). Three whole-root diffs per fill — the same
    /// compute-on-miss shape as [`RootCaches::ensure_files`].
    pub fn ensure_hunk_stats(&mut self, exec: &dyn GitExecutor, root: &RootId) {
        if self.hunk_stats.contains_key(root) {
            return;
        }
        let repo = exec
            .diff(
                &root.0,
                &DiffOpts {
                    left: Some("HEAD".to_owned()),
                    ..DiffOpts::default()
                },
            )
            .unwrap_or_default();
        let staged = exec
            .diff(
                &root.0,
                &DiffOpts {
                    staged: true,
                    ..DiffOpts::default()
                },
            )
            .unwrap_or_default();
        let local = exec.diff(&root.0, &DiffOpts::default()).unwrap_or_default();
        self.hunk_stats.insert(
            root.clone(),
            RootHunkStats {
                repo: hunk_stats::parse_file_hunks(&repo),
                staged: hunk_stats::parse_file_hunks(&staged),
                local: hunk_stats::parse_file_hunks(&local),
            },
        );
    }

    /// Store a freshly loaded commit log for `root`.
    pub fn store_log(&mut self, root: RootId, commits: Vec<Commit>) {
        self.log_cache.insert(root, commits);
    }

    /// Store freshly computed ahead/behind counts for `root`.
    pub fn store_ahead_behind(&mut self, root: RootId, ab: (usize, usize)) {
        self.ahead_behind.insert(root, ab);
    }

    /// Store a freshly loaded worktree list for `root` (issue 14).
    pub fn store_worktrees(&mut self, root: RootId, worktrees: Vec<Worktree>) {
        self.worktree_cache.insert(root, worktrees);
    }

    /// Store a freshly loaded submodule list for `root` (issue 14).
    pub fn store_submodules(&mut self, root: RootId, submodules: Vec<Submodule>) {
        self.submodule_cache.insert(root, submodules);
    }

    // --- Invalidation -------------------------------------------------------

    /// Drop all five caches' entries for the affected scope. There are no
    /// per-cache exceptions (policy uniformity): even immutable-by-commit-id
    /// entries go. Borrows `affected` — it is only read (plan Phase 3).
    pub fn invalidate(&mut self, affected: &Affected) {
        match affected {
            Affected::All => self.invalidate_all(),
            Affected::Root(root) => {
                self.log_cache.remove(root);
                self.ref_cache.remove(root);
                self.files_cache.retain(|(r, _), _| r != root);
                self.log_path_cache.retain(|(r, _), _| r != root);
                self.log_ref_cache.retain(|(r, _), _| r != root);
                self.search_cache.retain(|(r, _), _| r != root);
                self.ahead_behind.remove(root);
                self.worktree_cache.remove(root);
                self.submodule_cache.remove(root);
                self.hunk_stats.remove(root);
            }
        }
    }

    /// Drop every entry in all caches.
    pub fn invalidate_all(&mut self) {
        self.log_cache.clear();
        self.ref_cache.clear();
        self.files_cache.clear();
        self.log_path_cache.clear();
        self.log_ref_cache.clear();
        self.search_cache.clear();
        self.ahead_behind.clear();
        self.worktree_cache.clear();
        self.submodule_cache.clear();
        self.hunk_stats.clear();
    }

    /// True when every map is empty — the invalidation invariant's
    /// diagnostic surface (tests, assertions).
    pub fn is_empty(&self) -> bool {
        self.log_cache.is_empty()
            && self.ref_cache.is_empty()
            && self.files_cache.is_empty()
            && self.log_path_cache.is_empty()
            && self.log_ref_cache.is_empty()
            && self.search_cache.is_empty()
            && self.ahead_behind.is_empty()
            && self.worktree_cache.is_empty()
            && self.submodule_cache.is_empty()
            && self.hunk_stats.is_empty()
    }
}
