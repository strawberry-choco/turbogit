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

/// Which roots an operation's results affect — owned by the
/// [`Operation`](crate::operation::Operation) that produced them and used to
/// scope cache invalidation and rescans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Affected {
    /// Every registered root (batch operations, unknown scope).
    All,
    /// Only this root.
    Root(RootId),
}

/// Which scoped commit listing a cache read fills: a file's history, one ref's
/// history, or a pickaxe search. Each is a `git log` with one term in one
/// `LogOpts` field, so the choice is one value rather than three signatures
/// (issue #19, plan D9, issue 17).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogScope {
    /// `git log -- <path>`.
    Path(PathBuf),
    /// `git log <ref>`.
    Ref(String),
    /// `git log -S <query>`.
    Search(String),
}

impl Affected {
    /// The one root in scope, when the scope is a single root.
    pub fn root(&self) -> Option<&RootId> {
        match self {
            Affected::Root(id) => Some(id),
            Affected::All => None,
        }
    }
}

/// The root-keyed caches behind one interface.
#[derive(Default)]
pub struct RootCaches {
    /// Commit-log windows keyed by root: the fetched pages so far, newest
    /// first, plus what the last page said about the rest of the history.
    /// One entry per root, so invalidating a log cannot leave its paging flag
    /// behind (log paging, P5).
    log_cache: HashMap<RootId, LogWindow>,
    /// Ref decorations keyed by root, then commit id (issue #12).
    ref_cache: HashMap<RootId, HashMap<CommitId, Vec<CommitRef>>>,
    /// Changed-file lists keyed by (root, commit id) (issue #12).
    files_cache: HashMap<(RootId, CommitId), Vec<Change>>,
    /// Per-file line counts keyed by (root, commit id) (logs-panels redesign
    /// issue 02): the changed-files pane's `+N −M` column and the details
    /// pane's churn bar. Loaded off the render thread, one request per commit.
    file_stats_cache: HashMap<(RootId, CommitId), Vec<FileStat>>,
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

/// One root's commit-log window: the pages fetched so far, newest first, and
/// whether history continues past them. The window is the cached log plus its
/// paging state as one value, so no invalidation can separate the two.
#[derive(Default)]
struct LogWindow {
    commits: Vec<Commit>,
    has_more: bool,
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

/// One scoped-log fill: fetch on miss, then hand back a borrow of what the map
/// now holds. Written once for all three [`LogScope`]s, which differ only in
/// their key type. The tuple key cannot be probed in borrowed form (`HashMap`
/// has no mixed-reference `Borrow` for tuples), so the caller builds the owned
/// key up front and clones it only for the insert.
fn fill_scoped_log<K>(
    map: &mut HashMap<K, Vec<Commit>>,
    key: K,
    fetch: impl FnOnce() -> Vec<Commit>,
) -> &[Commit]
where
    K: Clone + Eq + std::hash::Hash,
{
    if !map.contains_key(&key) {
        map.insert(key.clone(), fetch());
    }
    map.get(&key).map(Vec::as_slice).unwrap_or(&[])
}

/// Which of the three working-tree diff views [`RootHunkStats::file` reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsView {
    Repo,
    Staged,
    Local,
}

/// One file's line counts within a change, as the **Git engine** answers them.
/// Re-exported from the domain so the log's panes never name a numstat tuple.
pub use turbogit_domain::model::FileStat;

/// The `(insertions, deletions)` of `path` within one change's stats slice, or
/// `None` when there is nothing to render: the path is absent, the counts are
/// still in flight, or git measured no line counts at all — a **Binary change**
/// has `-` where the numbers would be. A row with no stat renders neither
/// number, never a `+0 −0` that claims a measured zero.
pub fn file_stat(stats: &[FileStat], path: &Path) -> Option<(usize, usize)> {
    stats
        .iter()
        .find(|f| f.path == path)
        .and_then(|f| Some((f.insertions?, f.deletions?)))
}

impl RootCaches {
    // --- Readers -----------------------------------------------------------

    /// The cached commit log for `root`, if loaded.
    pub fn log(&self, root: &RootId) -> Option<&[Commit]> {
        self.log_cache.get(root).map(|w| w.commits.as_slice())
    }

    /// Whether `root`'s cached window stops short of the end of its history —
    /// the authoritative answer the log pane pages on (log paging, P4), `false`
    /// for a root with nothing cached.
    pub fn log_has_more(&self, root: &RootId) -> bool {
        self.log_cache.get(root).is_some_and(|w| w.has_more)
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

    /// True when `root`'s ref decorations have been stored (event-fed writer
    /// path) — the Log view's kick checks this before dispatching a fetch.
    pub fn refs_loaded(&self, root: &RootId) -> bool {
        self.ref_cache.contains_key(root)
    }

    /// The cached changed-file list of `(root, commit)`, if loaded (issue #12).
    pub fn files_for(&self, root: &RootId, commit: &CommitId) -> Option<&[Change]> {
        self.files_cache
            .get(&(root.clone(), commit.clone()))
            .map(|v| v.as_slice())
    }

    /// The cached per-file line counts of `(root, commit)`, if loaded
    /// (logs-panels redesign issue 02). `None` means the request has not
    /// settled yet — rows render without numbers rather than waiting.
    pub fn file_stats_for(&self, root: &RootId, commit: &CommitId) -> Option<&[FileStat]> {
        self.file_stats_cache
            .get(&(root.clone(), commit.clone()))
            .map(|v| v.as_slice())
    }

    /// True when `(root, commit)`'s line counts are stored — the check
    /// [`AppState::fetch_file_stats`](crate::state::AppState::fetch_file_stats)
    /// makes before dispatching, mirroring [`RootCaches::refs_loaded`].
    pub fn file_stats_loaded(&self, root: &RootId, commit: &CommitId) -> bool {
        self.file_stats_cache
            .contains_key(&(root.clone(), commit.clone()))
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

    /// The commits of one scoped listing in `root`, computed through the engine
    /// seam on miss and cached. The three scopes are one operation apart from
    /// which map they fill and which `LogOpts` field carries the term, so the
    /// choice lives here and the fill-and-reborrow body lives in
    /// `fill_scoped_log`.
    ///
    /// Returns a borrow of the cached list — same rule as
    /// [`RootCaches::ensure_files`] (plan §1.2).
    pub fn ensure_log(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        scope: &LogScope,
    ) -> &[Commit] {
        let path = root.0.clone();
        match scope {
            LogScope::Path(target) => {
                let key = (root.clone(), target.clone());
                let term = key.1.clone();
                fill_scoped_log(&mut self.log_path_cache, key, || {
                    exec.log(
                        &path,
                        &LogOpts {
                            path: Some(term),
                            ..Default::default()
                        },
                    )
                    .unwrap_or_default()
                })
            }
            LogScope::Ref(ref_name) => {
                let key = (root.clone(), ref_name.clone());
                let term = key.1.clone();
                fill_scoped_log(&mut self.log_ref_cache, key, || {
                    exec.log(
                        &path,
                        &LogOpts {
                            branch: Some(term),
                            ..Default::default()
                        },
                    )
                    .unwrap_or_default()
                })
            }
            LogScope::Search(query) => {
                let key = (root.clone(), query.clone());
                let term = key.1.clone();
                fill_scoped_log(&mut self.search_cache, key, || {
                    exec.log(
                        &path,
                        &LogOpts {
                            pickaxe: Some(term),
                            ..Default::default()
                        },
                    )
                    .unwrap_or_default()
                })
            }
        }
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
            .diff_patch(
                &root.0,
                &DiffOpts {
                    left: Some("HEAD".to_owned()),
                    ..DiffOpts::default()
                },
            )
            .unwrap_or_default();
        let staged = exec
            .diff_patch(
                &root.0,
                &DiffOpts {
                    staged: true,
                    ..DiffOpts::default()
                },
            )
            .unwrap_or_default();
        let local = exec
            .diff_patch(&root.0, &DiffOpts::default())
            .unwrap_or_default();
        self.hunk_stats.insert(
            root.clone(),
            RootHunkStats {
                repo: hunk_stats::file_hunks(&repo),
                staged: hunk_stats::file_hunks(&staged),
                local: hunk_stats::file_hunks(&local),
            },
        );
    }

    /// Store a freshly loaded commit log for `root`, replacing whatever window
    /// it held. A wholesale write makes no claim about rows beyond itself, so
    /// `has_more` resets with it; the fetcher states the flag from what it
    /// asked for ([`Self::set_log_has_more`]).
    pub fn store_log(&mut self, root: RootId, commits: Vec<Commit>) {
        self.log_cache.insert(
            root,
            LogWindow {
                commits,
                has_more: false,
            },
        );
    }

    /// Append a fetched page onto `root`'s window (log paging): the
    /// newest-first list grows by every commit it does not already hold.
    /// Dedup by [`CommitId`] is what makes an overlapping page — the anchor
    /// row the pager asks for again, a retry, a race with a refresh —
    /// harmless. Says nothing about `has_more`, which the fetcher states.
    pub fn append_log(&mut self, root: RootId, commits: Vec<Commit>) {
        let window = self.log_cache.entry(root).or_default();
        for commit in commits {
            if !window.commits.iter().any(|held| held.id == commit.id) {
                window.commits.push(commit);
            }
        }
    }

    /// Record whether `root`'s history continues past the cached window
    /// (log paging, P4): a full page means more, a short one means the end.
    pub fn set_log_has_more(&mut self, root: &RootId, has_more: bool) {
        self.log_cache.entry(root.clone()).or_default().has_more = has_more;
    }

    /// Store freshly loaded ref decorations for `root` (log-open perf, D1):
    /// the worker-event writer behind `AppEvent::RefsLoaded`. Callers check
    /// [`Self::refs_loaded`] before dispatching a (re)fetch.
    pub fn store_refs(&mut self, root: RootId, deco: Vec<(CommitId, Vec<CommitRef>)>) {
        self.ref_cache.insert(root, deco.into_iter().collect());
    }

    /// Store freshly loaded per-file line counts for one commit (logs-panels
    /// redesign issue 02): the worker-event writer behind
    /// `AppEvent::FileStatsLoaded`, same contract as [`Self::store_refs`].
    pub fn store_file_stats(&mut self, root: RootId, commit: CommitId, stats: Vec<FileStat>) {
        self.file_stats_cache.insert((root, commit), stats);
    }

    /// Store freshly computed ahead/behind counts for `root`.
    pub fn store_ahead_behind(&mut self, root: RootId, ab: (usize, usize)) {
        self.ahead_behind.insert(root, ab);
    }

    /// Store a freshly loaded worktree list for `root` (issue 14).
    pub fn store_worktrees(&mut self, root: RootId, worktrees: Vec<Worktree>) {
        self.worktree_cache.insert(root, worktrees);
    }

    /// Fill in just one worktree row's dirty flag (ticket 03): the per-row
    /// probe result updates that row's entry, leaving every other row — and
    /// the rest of the list — untouched. A row whose path is no longer listed
    /// (e.g. removed mid-probe) is a no-op.
    pub fn update_worktree_dirty(&mut self, root: &RootId, path: &Path, dirty: bool) {
        if let Some(list) = self.worktree_cache.get_mut(root)
            && let Some(wt) = list.iter_mut().find(|w| w.path == path)
        {
            wt.dirty = Some(dirty);
        }
    }

    /// Store a freshly loaded submodule list for `root` (issue 14).
    pub fn store_submodules(&mut self, root: RootId, submodules: Vec<Submodule>) {
        self.submodule_cache.insert(root, submodules);
    }

    // --- Invalidation -------------------------------------------------------

    /// Drop the caches for the affected scope. One sanctioned per-cache
    /// exception (ticket 02): the worktree list is **not** dropped for
    /// [`Affected::Root`] — ordinary operations (commit, checkout, staging,
    /// push…) cannot change a root's linked worktrees, so keeping the list
    /// stops the refetch storm. The list IS dropped for [`Affected::All`]
    /// (project switches, rescans) and, per root, through the explicit
    /// [`RootCaches::invalidate_worktrees`] used by the add/remove flows.
    /// Everything else stays policy-uniform.
    pub fn invalidate(&mut self, affected: &Affected) {
        match affected {
            Affected::All => self.invalidate_all(),
            Affected::Root(root) => {
                self.log_cache.remove(root);
                self.ref_cache.remove(root);
                self.files_cache.retain(|(r, _), _| r != root);
                self.file_stats_cache.retain(|(r, _), _| r != root);
                self.log_path_cache.retain(|(r, _), _| r != root);
                self.log_ref_cache.retain(|(r, _), _| r != root);
                self.search_cache.retain(|(r, _), _| r != root);
                self.ahead_behind.remove(root);
                self.submodule_cache.remove(root);
                self.hunk_stats.remove(root);
            }
        }
    }

    /// Drop only one root's cached worktree list (ticket 02): the sanctioned
    /// targeted invalidation behind the worktree-mutating flows — add and
    /// remove — so their completion settles with fresh data while unrelated
    /// operations keep the list.
    pub fn invalidate_worktrees(&mut self, root: &RootId) {
        self.worktree_cache.remove(root);
    }

    /// clear every root's worktree list on rescan, including removed roots (ticket 02).
    pub fn invalidate_all_worktrees(&mut self) {
        self.worktree_cache.clear();
    }

    /// Drop every entry in all caches.
    pub fn invalidate_all(&mut self) {
        self.log_cache.clear();
        self.ref_cache.clear();
        self.files_cache.clear();
        self.file_stats_cache.clear();
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
            && self.file_stats_cache.is_empty()
            && self.log_path_cache.is_empty()
            && self.log_ref_cache.is_empty()
            && self.search_cache.is_empty()
            && self.ahead_behind.is_empty()
            && self.worktree_cache.is_empty()
            && self.submodule_cache.is_empty()
            && self.hunk_stats.is_empty()
    }
}
