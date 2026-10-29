use std::collections::HashMap;
use std::path::{Path, PathBuf};
use turbogit_domain::model::{
    Change, Commit, CommitId, CommitRef, DiffOpts, LogOpts, Patch, RecentCheckout, RefState,
    RootId, Stash, Submodule, Worktree, forward_slash_path,
};
use turbogit_engine_api::GitExecutor;
use turbogit_services::hunk_stats::{self, FileHunks};

use crate::events::LogBatchMode;
use crate::state::LOG_BATCH_SIZE;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Affected {
    All,
    Root(RootId),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LogScope {
    Path(PathBuf),
    Ref(String),
    Search(String),
}

impl LogScope {
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
    pub fn root(&self) -> Option<&RootId> {
        match self {
            Affected::Root(id) => Some(id),
            Affected::All => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LogRead {
    Commits(RootId),
    Scope(RootId, LogScope),
    Changes(RootId, CommitId),
    Details(RootId, CommitId),
}

impl LogRead {
    fn root(&self) -> &RootId {
        match self {
            Self::Commits(root)
            | Self::Scope(root, _)
            | Self::Changes(root, _)
            | Self::Details(root, _) => root,
        }
    }
}

#[derive(Default)]
pub struct RootCaches {
    log_cache: HashMap<RootId, LogWindow>,
    ref_cache: HashMap<RootId, HashMap<CommitId, Vec<CommitRef>>>,
    files_cache: HashMap<(RootId, CommitId), Vec<Change>>,
    file_stats_cache: HashMap<(RootId, CommitId), Vec<FileStat>>,
    log_path_cache: HashMap<(RootId, PathBuf), LogWindow>,
    log_ref_cache: HashMap<(RootId, String), LogWindow>,
    search_cache: HashMap<(RootId, String), LogWindow>,
    log_failed: HashMap<LogRead, String>,
    diff_failed: HashMap<(RootId, StatsView), String>,
    ahead_behind: HashMap<RootId, (usize, usize)>,
    worktree_cache: HashMap<RootId, Vec<Worktree>>,
    submodule_cache: HashMap<RootId, Vec<Submodule>>,
    stash_cache: HashMap<RootId, Vec<Stash>>,
    tag_cache: HashMap<RootId, Vec<(String, RefState)>>,
    recent_cache: HashMap<RootId, Vec<RecentCheckout>>,
    hunk_stats: HashMap<RootId, RootHunkStats>,
    revision: u64,
}

#[derive(Default)]
struct LogWindow {
    commits: Vec<Commit>,
    has_more: bool,
}

impl LogWindow {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogBatchSettle {
    Settled { has_more: bool },
    Torn,
}

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

/// git's `--skip` counts commits in the traversal, and a `-S` pickaxe filter is
/// applied AFTER it, so a skip measured in matches is not a skip in the listing:
/// a batch asked for at the end of a 50-row match window comes back leading with
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

pub(crate) fn scoped_batch_plan(
    scope: &LogScope,
    have: Option<&[Commit]>,
) -> (usize, Option<usize>, LogBatchMode) {
    match scope {
        LogScope::Search(_) => search_batch_plan(have),
        LogScope::Path(_) | LogScope::Ref(_) => log_batch_plan(have),
    }
}

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

#[derive(Clone, Debug, Default)]
pub struct RootHunkStats {
    pub repo: Vec<FileHunks>,
    pub staged: Vec<FileHunks>,
    pub local: Vec<FileHunks>,
    pub line_sets: Vec<crate::staged_lines::FileLineSets>,
}

impl RootHunkStats {
    pub fn file(&self, pick: StatsView, path: &Path) -> Option<&FileHunks> {
        let view = match pick {
            StatsView::Repo => &self.repo,
            StatsView::Staged => &self.staged,
            StatsView::Local => &self.local,
        };
        let want = forward_slash_path(path);
        view.iter().find(|f| f.path == want)
    }

    pub fn row_is_staged(&self, path: &Path, row: &crate::diff_model::Row) -> Option<bool> {
        crate::staged_lines::row_is_staged(&self.line_sets, path, row)
    }
}

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
        map.remove(key);
    }
    settle
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatsView {
    Repo,
    Staged,
    Local,
}

pub use turbogit_domain::model::FileStat;

/// `None` when there is nothing to render: the path is absent, the counts are
/// still in flight, or git measured no line counts at all, a **Binary change**
/// has `-` where the numbers would be. A row with no stat renders neither
/// number, never a `+0 −0` that claims a measured zero.
pub fn file_stat(stats: &[FileStat], path: &Path) -> Option<(usize, usize)> {
    stats
        .iter()
        .find(|f| f.path == path)
        .and_then(|f| Some((f.insertions?, f.deletions?)))
}

impl RootCaches {
    pub fn log(&self, root: &RootId) -> Option<&[Commit]> {
        self.log_cache.get(root).map(|w| w.commits.as_slice())
    }

    pub fn log_has_more(&self, root: &RootId) -> bool {
        self.log_cache.get(root).is_some_and(|w| w.has_more)
    }

    pub fn scoped_log(&self, root: &RootId, scope: &LogScope) -> Option<&[Commit]> {
        self.scope_window(root, scope).map(|w| w.commits.as_slice())
    }

    pub fn scoped_log_has_more(&self, root: &RootId, scope: &LogScope) -> bool {
        self.scope_window(root, scope).is_some_and(|w| w.has_more)
    }

    pub fn scoped_log_can_grow(&self, root: &RootId, scope: &LogScope) -> bool {
        self.scope_window(root, scope).is_none_or(|w| w.has_more)
    }

    fn scope_window(&self, root: &RootId, scope: &LogScope) -> Option<&LogWindow> {
        match scope {
            LogScope::Path(target) => self.log_path_cache.get(&(root.clone(), target.clone())),
            LogScope::Ref(ref_name) => self.log_ref_cache.get(&(root.clone(), ref_name.clone())),
            LogScope::Search(query) => self.search_cache.get(&(root.clone(), query.clone())),
        }
    }

    pub fn path_log(&self, root: &RootId, path: &Path) -> Option<&[Commit]> {
        self.scoped_log(root, &LogScope::Path(path.to_path_buf()))
    }

    pub fn ref_log(&self, root: &RootId, ref_name: &str) -> Option<&[Commit]> {
        self.scoped_log(root, &LogScope::Ref(ref_name.to_string()))
    }

    pub fn search_log(&self, root: &RootId, query: &str) -> Option<&[Commit]> {
        self.scoped_log(root, &LogScope::Search(query.to_string()))
    }

    pub fn refs_for(&self, root: &RootId, commit: &CommitId) -> &[CommitRef] {
        self.ref_cache
            .get(root)
            .and_then(|m| m.get(commit))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn ref_groups(&self, root: &RootId) -> impl Iterator<Item = &[CommitRef]> + '_ {
        self.ref_cache
            .get(root)
            .into_iter()
            .flatten()
            .map(|(_, refs)| refs.as_slice())
    }

    pub fn refs_loaded(&self, root: &RootId) -> bool {
        self.ref_cache.contains_key(root)
    }

    pub fn files_for(&self, root: &RootId, commit: &CommitId) -> Option<&[Change]> {
        self.files_cache
            .get(&(root.clone(), commit.clone()))
            .map(|v| v.as_slice())
    }

    pub fn file_stats_for(&self, root: &RootId, commit: &CommitId) -> Option<&[FileStat]> {
        self.file_stats_cache
            .get(&(root.clone(), commit.clone()))
            .map(|v| v.as_slice())
    }

    pub fn file_stats_loaded(&self, root: &RootId, commit: &CommitId) -> bool {
        self.file_stats_cache
            .contains_key(&(root.clone(), commit.clone()))
    }

    pub fn ahead_behind(&self, root: &RootId) -> Option<(usize, usize)> {
        self.ahead_behind.get(root).copied()
    }

    pub fn worktrees(&self, root: &RootId) -> Option<&[Worktree]> {
        self.worktree_cache.get(root).map(|v| v.as_slice())
    }

    pub fn submodules(&self, root: &RootId) -> Option<&[Submodule]> {
        self.submodule_cache.get(root).map(|v| v.as_slice())
    }

    /// The cached stash listing of `root`, if loaded. Git's own
    /// order, newest first, the pane numbers rows by `Stash::index`, not by
    /// position, because `stash@{n}` is what the verbs hand the engine.
    pub fn stashes(&self, root: &RootId) -> Option<&[Stash]> {
        self.stash_cache.get(root).map(|v| v.as_slice())
    }

    pub fn tags(&self, root: &RootId) -> Option<&[(String, RefState)]> {
        self.tag_cache.get(root).map(|v| v.as_slice())
    }

    pub fn tags_by_root(&self) -> &HashMap<RootId, Vec<(String, RefState)>> {
        &self.tag_cache
    }

    pub fn recents(&self, root: &RootId) -> Option<&[RecentCheckout]> {
        self.recent_cache.get(root).map(|v| v.as_slice())
    }

    pub fn recents_by_root(&self) -> &HashMap<RootId, Vec<RecentCheckout>> {
        &self.recent_cache
    }

    pub fn hunk_stats(&self, root: &RootId) -> Option<&RootHunkStats> {
        self.hunk_stats.get(root)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn log_failure(&self, read: &LogRead) -> Option<&str> {
        self.log_failed.get(read).map(String::as_str)
    }

    pub fn has_log_failure(&self, read: &LogRead) -> bool {
        self.log_failed.contains_key(read)
    }

    pub fn record_log_failure(&mut self, read: LogRead, message: impl Into<String>) {
        self.log_failed.insert(read, message.into());
    }

    pub fn clear_log_failure(&mut self, read: &LogRead) {
        self.log_failed.remove(read);
    }

    pub fn diff_view_failure(&self, root: &RootId, view: StatsView) -> Option<&str> {
        let key = (root.clone(), view);
        self.diff_failed.get(&key).map(String::as_str)
    }

    pub fn has_diff_view_failure(&self, root: &RootId, view: StatsView) -> bool {
        self.diff_failed.contains_key(&(root.clone(), view))
    }

    pub fn record_diff_view_failure(
        &mut self,
        root: &RootId,
        view: StatsView,
        message: impl Into<String>,
    ) {
        self.diff_failed
            .insert((root.clone(), view), message.into());
    }

    pub fn clear_diff_view_failure(&mut self, root: &RootId, view: StatsView) {
        self.diff_failed.remove(&(root.clone(), view));
    }

    pub fn forget_log_read(&mut self, read: &LogRead) {
        self.log_failed.remove(read);
        match read {
            LogRead::Commits(root) => {
                self.log_cache.remove(root);
            }
            LogRead::Scope(root, scope) => match scope {
                LogScope::Path(target) => {
                    self.log_path_cache.remove(&(root.clone(), target.clone()));
                }
                LogScope::Ref(ref_name) => {
                    self.log_ref_cache.remove(&(root.clone(), ref_name.clone()));
                }
                LogScope::Search(query) => {
                    self.search_cache.remove(&(root.clone(), query.clone()));
                }
            },
            LogRead::Changes(root, cid) => {
                self.files_cache.remove(&(root.clone(), cid.clone()));
            }
            LogRead::Details(root, cid) => {
                self.file_stats_cache.remove(&(root.clone(), cid.clone()));
            }
        }
        self.bump();
    }

    pub fn ensure_files(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        commit: &CommitId,
    ) -> &[Change] {
        let key = (root.clone(), commit.clone());
        if !self.files_cache.contains_key(&key) {
            let read = LogRead::Changes(root.clone(), commit.clone());
            match exec.commit_files(&root.0, commit) {
                Ok(files) => {
                    self.log_failed.remove(&read);
                    self.files_cache.insert(key.clone(), files);
                }
                Err(e) => {
                    self.log_failed.insert(read, e.to_string());
                    self.files_cache.insert(key.clone(), Vec::new());
                }
            }
        }
        self.files_cache
            .get(&key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn ensure_tags(&mut self, exec: &dyn GitExecutor, root: &RootId) {
        if self.tag_cache.contains_key(root) {
            return;
        }
        let tags = exec
            .tag_list(&root.0)
            .unwrap_or_default()
            .into_iter()
            .map(|name| (name, RefState::Default))
            .collect();
        self.tag_cache.insert(root.clone(), tags);
    }

    pub fn ensure_recents(&mut self, exec: &dyn GitExecutor, root: &RootId) {
        if self.recent_cache.contains_key(root) {
            return;
        }
        let history = exec.recent_checkouts(&root.0).unwrap_or_default();
        self.recent_cache.insert(root.clone(), history);
    }

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

    pub fn fetch_log_batch(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        scope: &LogScope,
    ) -> LogBatchSettle {
        let (rows, skip, mode) = scoped_batch_plan(scope, self.scoped_log(root, scope));
        let read = LogRead::Scope(root.clone(), scope.clone());
        let page = match exec.log(&root.0, &scope.batch_opts(rows, skip)) {
            Ok(page) => {
                self.log_failed.remove(&read);
                page
            }
            Err(e) => {
                self.log_failed.insert(read, e.to_string());
                Vec::new()
            }
        };
        let settle = self.settle_scope(root, scope, mode, page);
        self.bump();
        settle
    }

    fn settle_scope(
        &mut self,
        root: &RootId,
        scope: &LogScope,
        mode: LogBatchMode,
        page: Vec<Commit>,
    ) -> LogBatchSettle {
        match scope {
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
        }
    }

    pub fn settle_scoped_log_batch(
        &mut self,
        root: &RootId,
        scope: &LogScope,
        mode: LogBatchMode,
        page: Vec<Commit>,
    ) -> LogBatchSettle {
        let settle = self.settle_scope(root, scope, mode, page);
        self.bump();
        settle
    }

    pub fn ensure_hunk_stats(&mut self, exec: &dyn GitExecutor, root: &RootId) {
        if self.hunk_stats.contains_key(root) {
            return;
        }
        let repo = self.read_diff_view(
            exec,
            root,
            StatsView::Repo,
            &DiffOpts {
                left: Some("HEAD".to_owned()),
                ..DiffOpts::default()
            },
        );
        let staged = self.read_diff_view(
            exec,
            root,
            StatsView::Staged,
            &DiffOpts {
                staged: true,
                ..DiffOpts::default()
            },
        );
        let local = self.read_diff_view(exec, root, StatsView::Local, &DiffOpts::default());
        self.hunk_stats.insert(
            root.clone(),
            RootHunkStats {
                repo: hunk_stats::file_hunks(&repo),
                staged: hunk_stats::file_hunks(&staged),
                local: hunk_stats::file_hunks(&local),
                line_sets: crate::staged_lines::derive_line_sets(staged, local),
            },
        );
    }

    fn read_diff_view(
        &mut self,
        exec: &dyn GitExecutor,
        root: &RootId,
        view: StatsView,
        opts: &DiffOpts,
    ) -> Patch {
        match exec.diff_patch(&root.0, opts) {
            Ok(patch) => {
                self.clear_diff_view_failure(root, view);
                patch
            }
            Err(e) => {
                self.record_diff_view_failure(root, view, e.to_string());
                Patch::default()
            }
        }
    }

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

    pub fn append_log(&mut self, root: RootId, commits: Vec<Commit>) {
        let window = self.log_cache.entry(root).or_default();
        for commit in commits {
            if !window.commits.iter().any(|held| held.id == commit.id) {
                window.commits.push(commit);
            }
        }
        self.bump();
    }

    pub fn set_log_has_more(&mut self, root: &RootId, has_more: bool) {
        self.log_cache.entry(root.clone()).or_default().has_more = has_more;
        self.bump();
    }

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
            self.log_cache.remove(root);
        }
        settle
    }

    pub fn store_refs(&mut self, root: RootId, deco: Vec<(CommitId, Vec<CommitRef>)>) {
        self.ref_cache.insert(root, deco.into_iter().collect());
    }

    pub fn store_file_stats(&mut self, root: RootId, commit: CommitId, stats: Vec<FileStat>) {
        self.file_stats_cache.insert((root, commit), stats);
    }

    pub fn store_ahead_behind(&mut self, root: RootId, ab: (usize, usize)) {
        self.ahead_behind.insert(root, ab);
    }

    pub fn store_worktrees(&mut self, root: RootId, worktrees: Vec<Worktree>) {
        self.worktree_cache.insert(root, worktrees);
    }

    pub fn update_worktree_dirty(&mut self, root: &RootId, path: &Path, dirty: bool) {
        if let Some(list) = self.worktree_cache.get_mut(root)
            && let Some(wt) = list.iter_mut().find(|w| w.path == path)
        {
            wt.dirty = Some(dirty);
        }
    }

    pub fn store_submodules(&mut self, root: RootId, submodules: Vec<Submodule>) {
        self.submodule_cache.insert(root, submodules);
    }

    pub fn store_stashes(&mut self, root: RootId, stashes: Vec<Stash>) {
        self.stash_cache.insert(root, stashes);
    }

    pub fn store_tags(&mut self, root: RootId, tags: Vec<(String, RefState)>) {
        self.tag_cache.insert(root, tags);
    }

    pub fn store_recents(&mut self, root: RootId, recents: Vec<RecentCheckout>) {
        self.recent_cache.insert(root, recents);
    }

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
                self.stash_cache.remove(root);
                self.tag_cache.remove(root);
                self.recent_cache.remove(root);
                self.hunk_stats.remove(root);
                self.log_failed.retain(|read, _| read.root() != root);
                self.diff_failed.retain(|(r, _), _| r != root);
            }
        }
    }

    pub fn invalidate_worktrees(&mut self, root: &RootId) {
        self.worktree_cache.remove(root);
    }

    pub fn invalidate_all_worktrees(&mut self) {
        self.worktree_cache.clear();
    }

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
        self.stash_cache.clear();
        self.tag_cache.clear();
        self.recent_cache.clear();
        self.hunk_stats.clear();
        self.log_failed.clear();
        self.diff_failed.clear();
    }

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
            && self.stash_cache.is_empty()
            && self.tag_cache.is_empty()
            && self.recent_cache.is_empty()
            && self.hunk_stats.is_empty()
            && self.log_failed.is_empty()
            && self.diff_failed.is_empty()
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

    type Write = fn(&mut RootCaches);

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
