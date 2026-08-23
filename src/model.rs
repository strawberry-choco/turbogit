//! Domain data model for TurboGit.
//!
//! Mirrors the concrete Rust sketch in `product-spec.md` §10. Every mutable git
//! state is **scoped to a `Root`** — single-root code never assumes a global
//! "the repository". All types are `Clone + Debug + Serialize/Deserialize` so
//! the UI can store/restore them and the persistence layer can serialize
//! settings/state under `.turbogit/`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Identity of a git repository root: its absolute path.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RootId(pub PathBuf);

impl RootId {
    pub fn as_path(&self) -> &Path {
        &self.0
    }
    pub fn name(&self) -> String {
        self.0
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.0.to_string_lossy().into_owned())
    }
}

/// A single git repository registered in the project.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Root {
    pub id: RootId,
    pub path: PathBuf,
    pub remotes: Vec<Remote>,
    pub branches: Vec<Branch>,
    pub current_branch: Option<BranchRef>,
    pub head: Option<CommitId>,
    pub status: RootStatus,
}

/// Reference to a branch by name (local or fully-qualified remote).
pub type BranchRef = String;

/// A configured remote (`origin`, …).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

/// Local vs remote branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BranchKind {
    Local,
    Remote,
}

/// A git branch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub kind: BranchKind,
    /// Upstream tracking branch (e.g. `origin/main`), if any.
    pub tracking: Option<String>,
    pub favorite: bool,
    pub protected: bool,
    /// Whether the branch currently exists on disk (false for a "create" preview).
    pub exists: bool,
}

/// SHA-1 hex string.
pub type CommitId = String;

/// Kind of a git ref decoration attached to a commit (issue #12 ref chips).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitRefKind {
    /// Local branch (`refs/heads/…`).
    Branch,
    /// Remote-tracking branch (`refs/remotes/<remote>/<name>`).
    Remote,
    /// Tag (`refs/tags/…`).
    Tag,
}

/// One named ref pointing at a commit (branch / remote branch / tag).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRef {
    pub kind: GitRefKind,
    pub name: String,
}

/// An author / committer signature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub name: String,
    pub email: String,
    /// Seconds since epoch.
    pub time: i64,
}

/// A single commit, tied to the root it belongs to (for unified multi-root log).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub id: CommitId,
    pub parents: Vec<CommitId>,
    pub author: Signature,
    pub committer: Signature,
    pub message: String,
    pub time: i64,
    pub root: RootId,
}

/// Status of one file in a working tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Unversioned,
    Ignored,
    Conflicted,
}

impl ChangeStatus {
    pub fn short(&self) -> &'static str {
        match self {
            ChangeStatus::Modified => "M",
            ChangeStatus::Added => "A",
            ChangeStatus::Deleted => "D",
            ChangeStatus::Renamed => "R",
            ChangeStatus::Copied => "C",
            ChangeStatus::Unversioned => "?",
            ChangeStatus::Ignored => "!",
            ChangeStatus::Conflicted => "U",
        }
    }
}

/// A contiguous hunk of changes (used for partial commits / gutter markers).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chunk {
    /// 1-based line range in the file (new side).
    pub start_line: usize,
    pub end_line: usize,
    /// Whether this chunk is selected for the next commit (partial commit).
    pub selected: bool,
    /// Whether the chunk is staged.
    pub staged: bool,
}

/// A single changed file (or unversioned / ignored / conflicted file).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub path: PathBuf,
    pub status: ChangeStatus,
    pub chunks: Vec<Chunk>,
    pub staged: bool,
}

/// A named, user-organized bucket of local changes (IntelliJ changelist model).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Changelist {
    pub name: String,
    pub active: bool,
    pub changes: Vec<Change>,
    pub root: RootId,
}

/// Per-root working-tree status summary.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RootStatus {
    pub changes: Vec<Change>,
    pub conflicted: Vec<PathBuf>,
}

impl RootStatus {
    pub fn modified(&self) -> usize {
        self.changes
            .iter()
            .filter(|c| {
                matches!(
                    c.status,
                    ChangeStatus::Modified
                        | ChangeStatus::Added
                        | ChangeStatus::Deleted
                        | ChangeStatus::Renamed
                        | ChangeStatus::Copied
                )
            })
            .count()
    }
    pub fn unversioned(&self) -> usize {
        self.changes
            .iter()
            .filter(|c| c.status == ChangeStatus::Unversioned)
            .count()
    }
    pub fn ignored(&self) -> usize {
        self.changes
            .iter()
            .filter(|c| c.status == ChangeStatus::Ignored)
            .count()
    }
}

/// A 3-way conflict awaiting resolution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conflict {
    pub path: PathBuf,
    pub base: PathBuf,
    pub local: PathBuf,
    pub incoming: PathBuf,
    pub resolved: bool,
    pub root: RootId,
}

/// IDE patch store entry (shelve).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shelf {
    pub name: String,
    pub changes: Vec<Change>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A git-native stash entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stash {
    pub message: String,
    pub root: RootId,
    pub index: usize,
}

/// A linked working tree (shares the object store).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
    pub root: RootId,
}

/// Aggregates all roots + the synchronous-branch flag. Provides batch &
/// synchronous-branch semantics on top of single-root services.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MultiRootManager {
    pub roots: Vec<Root>,
    /// "Execute branch operations on all roots".
    pub synchronous_branches: bool,
}

impl MultiRootManager {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register_root(&mut self, root: Root) {
        if !self.roots.iter().any(|r| r.id == root.id) {
            self.roots.push(root);
        }
    }
    pub fn by_id(&self, id: &RootId) -> Option<&Root> {
        self.roots.iter().find(|r| &r.id == id)
    }
}

/// A directory → VCS mapping (the `.idea/vcs.xml` equivalent).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirMapping {
    pub directory: PathBuf,
    pub vcs: Vcs,
}

/// Supported VCS backends. v1 is Git-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Vcs {
    Git,
    None,
}

/// Update-project strategy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UpdateMethod {
    #[default]
    Merge,
    Rebase,
}

/// What to do with a dirty tree when updating.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CleanTreeMethod {
    #[default]
    Stash,
    Shelve,
}

/// When to explicitly check remotes for incoming commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IncomingCheckMode {
    #[default]
    Auto,
    Always,
    Never,
}

/// Timestamp rendering style in the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DateFormat {
    #[default]
    Relative,
    Absolute,
    Iso,
}

/// Project + per-root settings, serialized under `.turbogit/`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsSettings {
    /// Path to the git executable (empty = resolve from PATH).
    pub git_executable: String,
    /// Enable Git's index (staging-area) UI instead of changelists.
    pub staging_area: bool,
    /// Synchronous branch control across roots.
    pub synchronous_branches: bool,
    /// Update method.
    pub update_method: UpdateMethod,
    /// Clean working tree using stash or shelf.
    pub clean_tree_method: CleanTreeMethod,
    /// Explicit incoming-check mode.
    pub incoming_check: IncomingCheckMode,
    /// Local protected-branch patterns (e.g. `main`, `release/*`).
    pub protected_branch_patterns: Vec<String>,
    /// Warn before committing CRLF.
    pub warn_crlf: bool,
    /// Warn when committing in detached HEAD / rebase.
    pub warn_detached: bool,
    /// Commit message template path (`.git commit.template`).
    pub commit_template: String,
    /// Restore workspace context on branch switch.
    pub restore_workspace: bool,
    /// Highlight modified lines in the gutter.
    pub gutter_markers: bool,
    /// Date format for the log.
    pub date_format: DateFormat,
    /// IDE-wide "do not run git commit hooks".
    pub no_commit_hooks: bool,
}

impl Default for VcsSettings {
    fn default() -> Self {
        Self {
            git_executable: String::new(),
            staging_area: false,
            synchronous_branches: false,
            update_method: UpdateMethod::default(),
            clean_tree_method: CleanTreeMethod::default(),
            incoming_check: IncomingCheckMode::default(),
            protected_branch_patterns: vec!["main".to_string(), "master".to_string()],
            warn_crlf: true,
            warn_detached: true,
            commit_template: String::new(),
            restore_workspace: false,
            gutter_markers: true,
            date_format: DateFormat::default(),
            no_commit_hooks: false,
        }
    }
}

/// On-disk project state persisted under `.turbogit/`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectState {
    pub mappings: Vec<DirMapping>,
    pub settings: VcsSettings,
}

/// Options for a log query.
#[derive(Clone, Debug, Default)]
pub struct LogOpts {
    pub max_count: Option<usize>,
    pub branch: Option<String>,
    pub path: Option<PathBuf>,
}

/// Options for a merge.
#[derive(Clone, Debug, Default)]
pub struct MergeOpts {
    pub no_ff: bool,
    pub ff_only: bool,
    pub squash: bool,
    pub no_commit: bool,
    pub no_verify: bool,
    pub allow_unrelated: bool,
    pub message: Option<String>,
}

/// Options for a rebase.
#[derive(Clone, Debug, Default)]
pub struct RebaseOpts {
    pub onto: Option<String>,
    pub rebase_merges: bool,
    pub keep_empty: bool,
    pub root: bool,
    pub update_refs: bool,
    pub autosquash: bool,
}

/// Helper: resolve a git executable path (settings override, else `git` on PATH).
pub fn git_binary(settings: &VcsSettings) -> String {
    if settings.git_executable.trim().is_empty() {
        "git".to_string()
    } else {
        settings.git_executable.clone()
    }
}

/// Options for a diff query.
#[derive(Clone, Debug, Default)]
pub struct DiffOpts {
    /// Show the staged (cached) diff instead of the working-tree diff.
    pub staged: bool,
    /// Diff `commit` against its parent (or the working tree if `right` set).
    pub commit: Option<String>,
    /// Two-dot diff `left..right` (or `left` vs working tree when `right` is None).
    pub left: Option<String>,
    pub right: Option<String>,
    /// Restrict the diff to a single path.
    pub path: Option<PathBuf>,
    /// Ignore whitespace changes.
    pub ignore_whitespace: bool,
    /// Produce a `--stat` summary instead of a full patch.
    pub stat: bool,
}

/// One line of `git blame` output, tied to the commit that introduced it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    pub commit: CommitId,
    pub author: String,
    pub time: i64,
    pub line_no: usize,
    pub content: String,
}

/// One entry in the interactive-rebase plan (F5 / I-series history editing).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebaseAction {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

/// A row of the interactive rebase plan.
#[derive(Clone, Debug)]
pub struct RebasePlanEntry {
    pub action: RebaseAction,
    pub commit: CommitId,
    pub subject: String,
}

/// A parsed diff line for the viewer (color-coded by prefix).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLine {
    /// Context / header / hunk marker (no sign).
    Meta(String),
    /// Added line.
    Add(String),
    /// Removed line.
    Del(String),
}
