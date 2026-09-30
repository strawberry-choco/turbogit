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
    forward_slash_path,
};
use turbogit_engine_api::GitExecutor;
use turbogit_services::hunk_stats::{self, FileHunks};

use crate::events::LogBatchMode;
use crate::state::LOG_BATCH_SIZE;

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
///
/// The three batch exactly as the unscoped listing does (log-view-scaling 01):
/// each is a window of one listing with its own paging flag, and none of them
/// is ever mixed into another's cache. `Hash` is part of the contract because a
/// scope is half of an in-flight read's key, so two scopes of one root can be
/// read at once (log-view-scaling 02). A search batches like the others too, with
/// its position read in the match stream rather than the traversal
/// (log-view-scaling 10) — see [`scoped_batch_plan`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LogScope {
    /// `git log -- <path>`.
    Path(PathBuf),
    /// `git log <ref>`.
    Ref(String),
    /// `git log -S <query>`.
    Search(String),
}

impl LogScope {
    /// This scope's term, with one batch's position attached: the filter and
    /// the paging are composed by the engine into a single `git log`
    /// (log-view-scaling 01).
    pub(crate) fn batch_opts(&self, rows: usize, skip: Option<usize>) -> LogOpts {
        let mut opts = LogOpts {
            max_count: Some(rows),
            skip,
            ..Default::default()
        };
        match self {
            LogScope::Path(p) => opts.path = Some(p.clone()),
            LogScope::Ref(r) => opts.branch = Some(r.clone()),
            LogScope::Search(q) => opts.pickaxe = Some(q.clone()),
        }
        opts
    }
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
    /// Commit-log windows keyed by root: the fetched batches so far, newest
    /// first, plus what the last batch said about the rest of the history.
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
    /// Path-scoped log windows keyed by (root, scoped path) (issue #19). A
    /// window, not a list: a path scope batches like the unscoped listing, so
    /// its held rows and its paging flag live as one value (log-view-scaling
    /// 01).
    log_path_cache: HashMap<(RootId, PathBuf), LogWindow>,
    /// Ref-scoped log windows keyed by (root, ref) (branch-tree extraction,
    /// plan D9) — mirrors the path-scoped cache: same shape, same cost model.
    log_ref_cache: HashMap<(RootId, String), LogWindow>,
    /// Pickaxe search windows keyed by (root, query) (issue 17).
    search_cache: HashMap<(RootId, String), LogWindow>,
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
    /// A counter moved by every write a derived log window reads: the log store
    /// and its appends, a batch settling under either the unscoped or a scoped
    /// rule, the has-more flag, and invalidation (log-view-scaling 04). It
    /// exists so the log pane's held display window
    /// ([`crate::log_display`]) can ask "did anything the window was derived
    /// from move?" in O(1) instead of re-deriving to find out. A write the
    /// window does not read leaves it alone, so no rebuild is paid for a ref
    /// decoration landing.
    revision: u64,
}

/// One root's commit-log window: the batches fetched so far, newest first, and
/// whether history continues past them. The window is the cached log plus its
/// paging state as one value, so no invalidation can separate the two.
///
/// One type for all four listings: the unscoped window and the three scoped
/// windows differ only in their key, never in the rules they fetch by
/// (log-view-scaling 01).
#[derive(Default)]
struct LogWindow {
    commits: Vec<Commit>,
    has_more: bool,
}

impl LogWindow {
    /// Fold one fetched batch into the window and say what came of it.
    ///
    /// An append batch has to lead with the anchor row it was requested
    /// against — that row is the checksum on the boundary. When it leads with
    /// anything else the listing moved under the request, so the torn batch is
    /// never held, the window is emptied, and [`LogBatchSettle::Torn`] asks
    /// the caller to read the same listing again from the front (P3).
    /// Either way the batch's length against the rows it asked for is what says
    /// whether history continues (P4).
    fn settle(&mut self, mode: LogBatchMode, batch: Vec<Commit>) -> LogBatchSettle {
        let has_more = match mode {
            LogBatchMode::Replace => {
                self.commits = batch;
                self.commits.len() == LOG_BATCH_SIZE
            }
            LogBatchMode::Append { anchor } => {
                if batch.first().map(|c| &c.id) != Some(&anchor) {
                    self.commits.clear();
                    self.has_more = false;
                    return LogBatchSettle::Torn;
                }
                let has_more = batch.len() == LOG_BATCH_SIZE + 1;
                for commit in batch.into_iter().skip(1) {
                    if !self.commits.iter().any(|held| held.id == commit.id) {
                        self.commits.push(commit);
                    }
                }
                has_more
            }
        };
        self.has_more = has_more;
        LogBatchSettle::Settled { has_more }
    }
}

/// What one batch read did to the window it was fetched for (log paging).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogBatchSettle {
    /// The batch folded in; the flag says whether history continues past it.
    Settled {
        /// The window's paging flag after the batch.
        has_more: bool,
    },
    /// The listing moved under the request (P3): the batch was torn, nothing
    /// was held, and the same listing has to be read again from the front.
    Torn,
}

/// The next batch request for a listing window, and the mode the answer folds
/// in under (P5). The window is derived from what is held, never from a
/// stored batch index, so an empty window asks for one batch from the front of
/// the listing and a window holding rows asks for the batch after them —
/// `skip = have - 1` deep and one row longer than a batch, so the row it
/// already holds comes back as the boundary checksum (P3). The walk always
/// starts at HEAD, so no commit can be lost behind a merge's second parent
/// (P2).
///
/// One function for the unscoped window, a path scope and a ref scope: the rules
/// are the same, and only the filter in [`LogOpts`] differs. A search is the one
/// listing with a different POSITION rather than a different rule — see
/// [`scoped_batch_plan`], which is how a caller reaches this.
pub(crate) fn log_batch_plan(have: Option<&[Commit]>) -> (usize, Option<usize>, LogBatchMode) {
    match have {
        None => (LOG_BATCH_SIZE, None, LogBatchMode::Replace),
        Some([]) => (LOG_BATCH_SIZE, None, LogBatchMode::Replace),
        Some(window) => (
            LOG_BATCH_SIZE + 1,
            Some(window.len() - 1),
            LogBatchMode::Append {
                anchor: window[window.len() - 1].id.clone(),
            },
        ),
    }
}

/// The next batch of a SEARCH listing, which cannot be positioned with a `skip`.
///
/// git's `--skip` counts commits in the traversal, and a `-S` pickaxe filter is
/// applied AFTER it, so a skip measured in matches is not a skip in the listing:
/// a batch asked for at the end of a 50-row match window comes back leading with
/// whichever commit sits 49 places down the *traversal*, which is not the
/// window's last row. The app reads that as a torn batch, drops the window and
/// reads it again from the front — the same first batch, forever, which is how
/// capping a search at one batch froze every broad search in a long history.
///
/// So a search is asked from the front of its match stream for everything it
/// holds plus one batch, and the batch is the TAIL of what comes back (see
/// [`batch_within`]). Only the position changes: the mode is still
/// [`LogBatchMode::Append`], so the anchor checksum and the has-more rule are
/// the ones every other listing settles under.
///
/// **The accepted cost**: a search batch re-reads its own already-held prefix,
/// so a search is O(held) per batch rather than O(1). It stays bounded to what
/// is already held plus one batch — never the whole listing, which is the
/// unbounded read this design exists to close — and a search scope is narrow by
/// nature. The first batch is still a plain [`LOG_BATCH_SIZE`] from the front.
fn search_batch_plan(have: Option<&[Commit]>) -> (usize, Option<usize>, LogBatchMode) {
    match have {
        None => (LOG_BATCH_SIZE, None, LogBatchMode::Replace),
        Some([]) => (LOG_BATCH_SIZE, None, LogBatchMode::Replace),
        Some(window) => (
            window.len() + LOG_BATCH_SIZE,
            None,
            LogBatchMode::Append {
                anchor: window[window.len() - 1].id.clone(),
            },
        ),
    }
}

/// The plan for one SCOPED listing: the unscoped rule for a path or a ref, the
/// match-stream rule for a search. One function so the two paths that read a
/// scope's batch — the synchronous cold fill and the off-thread batch — cannot
/// pick different positions for the same window.
pub(crate) fn scoped_batch_plan(
    scope: &LogScope,
    have: Option<&[Commit]>,
) -> (usize, Option<usize>, LogBatchMode) {
    match scope {
        LogScope::Search(_) => search_batch_plan(have),
        LogScope::Path(_) | LogScope::Ref(_) => log_batch_plan(have),
    }
}

/// The batch inside what a scoped read brought back: for everything but a search
/// that is the page as the engine returned it, and for a search it is the TAIL,
/// starting where the window's last row is (see [`search_batch_plan`]).
///
/// The cut is by POSITION, not by looking the anchor up in the page, and that is
/// what keeps the checksum honest. A listing that moved under the request leaves
/// a different row at that position, so the batch does not lead with the anchor
/// and [`LogWindow::settle`] reads it as torn and restarts the window — exactly
/// as it does for a path or a ref. Finding the anchor instead would paper over
/// that: a moved listing would be cut to the new position and stitched, which is
/// the torn window this rule exists to prevent. A page shorter than the cut is
/// no batch at all, which is torn the same way.
fn batch_within(
    scope: &LogScope,
    held: usize,
    mode: &LogBatchMode,
    page: Vec<Commit>,
) -> Vec<Commit> {
    match (scope, mode) {
        (LogScope::Search(_), LogBatchMode::Append { .. }) => {
            page.into_iter().skip(held.saturating_sub(1)).collect()
        }
        _ => page,
    }
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
        let want = forward_slash_path(path);
        view.iter().find(|f| f.path == want)
    }
}

/// Read the next batch of one scoped listing through the engine seam and fold
/// it into its window, under the unscoped window's own rules
/// (log-view-scaling 01). This is the SYNCHRONOUS read — the one a cold
/// [`Self::ensure_log`] makes, because the caller paints what it returns in
/// the same frame. A batch that extends a window is read off the render thread
/// instead (log-view-scaling 02) and arrives through
/// [`Self::settle_scoped_log_batch`].
fn read_scoped_log_batch<K>(
    map: &mut HashMap<K, LogWindow>,
    key: &K,
    scope: &LogScope,
    exec: &dyn GitExecutor,
    repo: &Path,
) -> LogBatchSettle
where
    K: Clone + Eq + std::hash::Hash,
{
    let (rows, skip, mode) = scoped_batch_plan(scope, map.get(key).map(|w| w.commits.as_slice()));
    let page = exec
        .log(repo, &scope.batch_opts(rows, skip))
        .unwrap_or_default();
    settle_scoped_window(map, key, scope, mode, page)
}

/// Fold one fetched batch of one scoped listing into that scope's window and
/// report what came of it (log-view-scaling 02). Written once for all three
/// [`LogScope`]s, which differ only in key type — and in where their batch sits
/// inside what came back, which [`batch_within`] decides from the window's own
/// length.
fn settle_scoped_window<K>(
    map: &mut HashMap<K, LogWindow>,
    key: &K,
    scope: &LogScope,
    mode: LogBatchMode,
    page: Vec<Commit>,
) -> LogBatchSettle
where
    K: Clone + Eq + std::hash::Hash,
{
    let held = map.get(key).map_or(0, |w| w.commits.len());
    let batch = batch_within(scope, held, &mode, page);
    let settle = map.entry(key.clone()).or_default().settle(mode, batch);
    if settle == LogBatchSettle::Torn {
        // A torn batch is not a window: the listing this window was measured
        // against no longer exists, and an emptied window that says "no more
        // history" would be a lie the next read would believe. Dropping the
        // entry leaves the cache exactly where it was before the first read,
        // so the caller's refetch plans batch 0.
        map.remove(key);
    }
    settle
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

    /// The held window of any scoped listing in `root` — a file's history, a
    /// ref's, or a pickaxe search's — if loaded (log-view-scaling 01). One
    /// reader for the three scopes; the [`LogScope::Path`],
    /// [`LogScope::Ref`] and [`LogScope::Search`] forms below are the spelled
    /// out call sites.
    pub fn scoped_log(&self, root: &RootId, scope: &LogScope) -> Option<&[Commit]> {
        self.scope_window(root, scope).map(|w| w.commits.as_slice())
    }

    /// Whether `scope`'s window in `root` stops short of the end of that
    /// listing's history — the same answer [`Self::log_has_more`] gives for the
    /// unscoped window, and per scope (log-view-scaling 01). `false` for a
    /// scope that has never been read.
    pub fn scoped_log_has_more(&self, root: &RootId, scope: &LogScope) -> bool {
        self.scope_window(root, scope).is_some_and(|w| w.has_more)
    }

    /// Whether `scope`'s window in `root` can be asked for another batch: a
    /// scope nobody has read yet, or one that says its history continues past
    /// what it holds. A window that reports the end of its own listing is not
    /// asked again — a short batch is an answer, not a reason to ask twice
    /// (log-view-scaling 02). This is the admission decision for a scoped
    /// read, and it is the per-scope twin of the guard the unscoped
    /// [`Self::log_has_more`] drives.
    pub fn scoped_log_can_grow(&self, root: &RootId, scope: &LogScope) -> bool {
        self.scope_window(root, scope).is_none_or(|w| w.has_more)
    }

    /// The window `scope` names in `root`, whichever of the three maps holds
    /// it. The three stay separate: one scope's batches are never read out of
    /// another's map.
    fn scope_window(&self, root: &RootId, scope: &LogScope) -> Option<&LogWindow> {
        match scope {
            LogScope::Path(target) => self.log_path_cache.get(&(root.clone(), target.clone())),
            LogScope::Ref(ref_name) => self.log_ref_cache.get(&(root.clone(), ref_name.clone())),
            LogScope::Search(query) => self.search_cache.get(&(root.clone(), query.clone())),
        }
    }

    /// The cached path-scoped log for `(root, path)`, if loaded (issue #19).
    pub fn path_log(&self, root: &RootId, path: &Path) -> Option<&[Commit]> {
        self.scoped_log(root, &LogScope::Path(path.to_path_buf()))
    }

    /// The cached ref-scoped log for `(root, ref)`, if loaded (plan D9).
    pub fn ref_log(&self, root: &RootId, ref_name: &str) -> Option<&[Commit]> {
        self.scoped_log(root, &LogScope::Ref(ref_name.to_string()))
    }

    /// The cached pickaxe search result for `(root, query)`, if loaded
    /// (issue 17).
    pub fn search_log(&self, root: &RootId, query: &str) -> Option<&[Commit]> {
        self.scoped_log(root, &LogScope::Search(query.to_string()))
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

    /// The caches' revision counter (log-view-scaling 04): the same answer for
    /// every reader, and the half of the derived log window's staleness check
    /// that lives in the data. Two reads at the same revision mean the same
    /// loaded windows.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Move the revision: called by every write whose result a derived value
    /// reads. Monotonic by construction, so a held value can only ever be
    /// stale, never accidentally current.
    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
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

    /// The held window of one scoped listing in `root`, computed through the
    /// engine seam on miss: a scope that has never been read is read ONE BATCH
    /// of, under the unscoped window's own batch rules (log-view-scaling 01),
    /// and a scope that already holds rows is not asked again here — it grows
    /// through [`Self::fetch_log_batch`]. The three scopes are one operation
    /// apart from which map they fill and which `LogOpts` field carries the
    /// term, so the choice lives here and the batch read lives in
    /// `read_scoped_log_batch`.
    ///
    /// Returns a borrow of the held rows — same rule as
    /// [`RootCaches::ensure_files`] (plan §1.2).
    pub fn ensure_log(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        scope: &LogScope,
    ) -> &[Commit] {
        if self.scope_window(root, scope).is_none() {
            self.fetch_log_batch(exec, root, scope);
        }
        self.scoped_log(root, scope).unwrap_or_default()
    }

    /// Read one batch of one scoped listing in `root` through the engine seam,
    /// synchronously, and fold it into that scope's own window
    /// (log-view-scaling 01).
    ///
    /// The caller decides WHICH batch: the window is derived here, never stored,
    /// so this reads the batch after whatever the window holds, or batch 0 when
    /// it holds nothing. Whether a scope may be read at all is
    /// [`Self::scoped_log_can_grow`]'s answer, and only the caller has the
    /// in-flight guard that decides it — this is the read, not the policy.
    pub fn fetch_log_batch(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        scope: &LogScope,
    ) -> LogBatchSettle {
        let repo = root.0.clone();
        let settle = match scope {
            LogScope::Path(target) => read_scoped_log_batch(
                &mut self.log_path_cache,
                &(root.clone(), target.clone()),
                scope,
                exec,
                &repo,
            ),
            LogScope::Ref(ref_name) => read_scoped_log_batch(
                &mut self.log_ref_cache,
                &(root.clone(), ref_name.clone()),
                scope,
                exec,
                &repo,
            ),
            LogScope::Search(query) => read_scoped_log_batch(
                &mut self.search_cache,
                &(root.clone(), query.clone()),
                scope,
                exec,
                &repo,
            ),
        };
        self.bump();
        settle
    }

    /// Fold one batch of one scoped listing into THAT scope's window and report
    /// what came of it (log-view-scaling 02) — the window write behind
    /// [`AppEvent::LogBatchLoaded`](crate::events::AppEvent::LogBatchLoaded).
    ///
    /// A batch that arrives off-thread obeys the same
    /// [`LogWindow::settle`] the unscoped window settles under, so the
    /// anchor-checksum and has-more rules cannot drift between a batch read on
    /// the asking frame and one read off it. A batch that comes back torn (the
    /// listing moved under the request) is never held and leaves no window at
    /// all, which is what tells the caller to read the listing again from the
    /// front.
    ///
    /// A search's page holds more than its batch — everything the window already
    /// holds plus the batch behind it — so the batch is cut out of it here, by
    /// the window's own length and before the checksum sees it. The cut is
    /// measured against the window as it stands now, which is the same length
    /// the request was planned against unless something moved the window in
    /// between: and a window that did move puts a different row at the cut, so
    /// the checksum still reads the batch as torn.
    pub fn settle_scoped_log_batch(
        &mut self,
        root: &RootId,
        scope: &LogScope,
        mode: LogBatchMode,
        page: Vec<Commit>,
    ) -> LogBatchSettle {
        let settle = match scope {
            LogScope::Path(target) => settle_scoped_window(
                &mut self.log_path_cache,
                &(root.clone(), target.clone()),
                scope,
                mode,
                page,
            ),
            LogScope::Ref(ref_name) => settle_scoped_window(
                &mut self.log_ref_cache,
                &(root.clone(), ref_name.clone()),
                scope,
                mode,
                page,
            ),
            LogScope::Search(query) => settle_scoped_window(
                &mut self.search_cache,
                &(root.clone(), query.clone()),
                scope,
                mode,
                page,
            ),
        };
        self.bump();
        settle
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
        self.bump();
    }

    /// Append a fetched batch onto `root`'s window (log paging): the
    /// newest-first list grows by every commit it does not already hold.
    /// Dedup by [`CommitId`] is what makes an overlapping batch — the anchor
    /// row the pager asks for again, a retry, a race with a refresh —
    /// harmless. Says nothing about `has_more`, which the fetcher states.
    pub fn append_log(&mut self, root: RootId, commits: Vec<Commit>) {
        let window = self.log_cache.entry(root).or_default();
        for commit in commits {
            if !window.commits.iter().any(|held| held.id == commit.id) {
                window.commits.push(commit);
            }
        }
        self.bump();
    }

    /// Record whether `root`'s history continues past the cached window
    /// (log paging, P4): a full batch means more, a short one means the end.
    pub fn set_log_has_more(&mut self, root: &RootId, has_more: bool) {
        self.log_cache.entry(root.clone()).or_default().has_more = has_more;
        self.bump();
    }

    /// Fold one fetched batch of `root`'s unscoped window and report what came
    /// of it (log paging). This is the window write behind
    /// [`AppEvent::LogLoaded`](crate::events::AppEvent::LogLoaded), and it is
    /// the same [`LogWindow::settle`] the three scoped windows settle under —
    /// one implementation of the anchor-checksum and has-more rules, so a
    /// scope cannot drift from the unscoped listing's rules.
    pub fn settle_log_batch(
        &mut self,
        root: &RootId,
        mode: LogBatchMode,
        batch: Vec<Commit>,
    ) -> LogBatchSettle {
        let settle = self
            .log_cache
            .entry(root.clone())
            .or_default()
            .settle(mode, batch);
        self.bump();
        if settle == LogBatchSettle::Torn {
            // A torn batch is no window at all — the same reason the scoped
            // read drops its window, so the refetch plans batch 0.
            self.log_cache.remove(root);
        }
        settle
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
        self.bump();
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
        self.bump();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use turbogit_domain::model::Signature;

    fn root(name: &str) -> RootId {
        RootId(Arc::from(Path::new(name)))
    }

    fn commit(id: &str) -> Commit {
        Commit {
            id: id.to_owned(),
            parents: vec![],
            author: Signature {
                name: "t".to_owned(),
                email: "t@t".to_owned(),
                time: 0,
            },
            committer: Signature {
                name: "t".to_owned(),
                email: "t@t".to_owned(),
                time: 0,
            },
            message: id.to_owned(),
            time: 0,
            root: root("alpha"),
            signature: Default::default(),
        }
    }

    // --- the revision (log-view-scaling 04) --------------------------------------

    /// One cache write, named — the table below is a list of them.
    type Write = fn(&mut RootCaches);

    /// The revision is the caches' own word for "the loaded window moved". Every
    /// write the derived log display window reads has to move it, or the held
    /// window would outlive the batch it was derived from.
    #[test]
    fn every_write_the_log_display_reads_moves_the_revision() {
        let cases: Vec<(&str, Write)> = vec![
            ("the log is stored", |c| {
                c.store_log(root("alpha"), vec![commit("a")])
            }),
            ("a batch is appended", |c| {
                c.append_log(root("alpha"), vec![commit("b")]);
            }),
            ("has-more is stated", |c| {
                c.set_log_has_more(&root("alpha"), true);
            }),
            ("a fetched batch settles", |c| {
                c.settle_log_batch(
                    &root("alpha"),
                    LogBatchMode::Replace,
                    vec![commit("a"), commit("b")],
                );
            }),
            ("a scoped batch settles", |c| {
                c.settle_scoped_log_batch(
                    &root("alpha"),
                    &LogScope::Path(PathBuf::from("f.txt")),
                    LogBatchMode::Replace,
                    vec![commit("a")],
                );
            }),
            ("a pickaxe window settles", |c| {
                c.settle_scoped_log_batch(
                    &root("alpha"),
                    &LogScope::Search("needle".to_owned()),
                    LogBatchMode::Replace,
                    vec![commit("a")],
                );
            }),
            ("a root is invalidated", |c| {
                c.invalidate(&Affected::Root(root("alpha")));
            }),
            ("everything is invalidated", |c| c.invalidate_all()),
        ];
        for (what, write) in cases {
            let mut caches = RootCaches::default();
            let before = caches.revision();
            write(&mut caches);
            assert_ne!(caches.revision(), before, "{what} must move the revision");
        }
    }

    /// A write the display window does not read leaves the revision alone:
    /// ref decorations, line counts, ahead/behind and worktree lists are not
    /// derivation inputs, and moving the revision for them would rebuild the
    /// window over a list that did not change.
    #[test]
    fn a_write_the_display_window_does_not_read_leaves_the_revision_alone() {
        let mut caches = RootCaches::default();
        let before = caches.revision();
        caches.store_refs(root("alpha"), vec![]);
        caches.store_file_stats(root("alpha"), "a".to_owned(), vec![]);
        caches.store_ahead_behind(root("alpha"), (1, 1));
        caches.store_worktrees(root("alpha"), vec![]);
        caches.store_submodules(root("alpha"), vec![]);
        caches.invalidate_worktrees(&root("alpha"));
        caches.invalidate_all_worktrees();
        assert_eq!(
            caches.revision(),
            before,
            "a non-log write must not look like a moved window"
        );
    }

    /// The revision only ever moves forward, so a held window is never mistaken
    /// for a current one by a later comparison.
    #[test]
    fn the_revision_moves_forward_and_never_back() {
        let mut caches = RootCaches::default();
        let mut last = caches.revision();
        for i in 0..5 {
            caches.append_log(root("alpha"), vec![commit(&i.to_string())]);
            assert!(caches.revision() > last);
            last = caches.revision();
        }
    }
}
