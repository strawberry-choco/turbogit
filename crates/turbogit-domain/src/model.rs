//! Domain data model for TurboGit.
//!
//! Mirrors the concrete Rust sketch in `product-spec.md` §10. Every mutable git
//! state is **scoped to a `Root`** — single-root code never assumes a global
//! "the repository". All types are `Clone + Debug + Serialize/Deserialize` so
//! the UI can store/restore them and the persistence layer can serialize
//! settings/state under `.turbogit/`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Identity of a git repository root: its absolute path.
///
/// The path is shared through an [`Arc`] so the dozens of identity clones per
/// operation burst (cache keys, event payloads, `'static` worker captures)
/// are refcount bumps instead of heap allocations. `Arc<Path>` hashes and
/// compares exactly like the underlying [`Path`], so map keys are unaffected.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RootId(pub Arc<Path>);

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

// Serde is manual because `Path` itself is not `Deserialize`: read a plain
// `PathBuf` and share it through the arc. Serialization goes through `&Path`,
// which emits exactly what the previous `PathBuf` field emitted.
impl Serialize for RootId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_path().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RootId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(RootId(Arc::from(PathBuf::deserialize(deserializer)?)))
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

impl Root {
    /// Resolve a changed file by repo-relative or absolute path. Single owner
    /// of the canonical path-key rule: a query matches either the change's
    /// repo-relative form or its root-joined absolute form. Comparison stays
    /// exact (`Path` equality) — Windows paths remain case-sensitive.
    pub fn resolve_change(&self, path: &Path) -> Option<&Change> {
        self.status
            .changes
            .iter()
            .find(|c| c.path == path || self.id.0.join(&c.path) == *path)
    }

    /// The absolute bucket-key form of `change`'s path — the root-joined form
    /// selection/exclusion sets are keyed by (see [`Root::resolve_change`]).
    pub fn canonical_key(&self, change: &Change) -> PathBuf {
        self.id.0.join(&change.path)
    }
}

/// Reference to a branch by name (local or fully-qualified remote).
pub type BranchRef = String;

/// A configured remote (`origin`, …). Fetch and push URLs are tracked
/// separately because git lets them diverge (`remote set-url --push`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    pub name: String,
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
}

/// Local vs remote branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Commits on this branch missing from its upstream (issue 32 popup
    /// sync markers; zeros for untracked/remote branches).
    #[serde(default)]
    pub ahead: usize,
    /// Commits on the upstream missing from this branch (issue 32 popup
    /// sync markers; zeros for untracked/remote branches).
    #[serde(default)]
    pub behind: usize,
    /// The tracked upstream was deleted on the remote (`[gone]` in
    /// `git branch -vv`). Drives the popup's "gone" marker.
    #[serde(default)]
    pub gone: bool,
    /// Committer time of the branch tip — the popup's stale badge input.
    /// `None` when the engine cannot answer.
    #[serde(default)]
    pub last_touched: Option<chrono::DateTime<chrono::Utc>>,
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
    /// Sync state of the decoration (issue 17): a remote-tracking ref is
    /// [`RefState::Gone`] when its upstream branch was deleted on the
    /// remote; a tag is [`RefState::Pushed`] or [`RefState::LocalOnly`];
    /// everything else is [`RefState::Default`] (no marker).
    #[serde(default)]
    pub state: RefState,
}

/// State of a ref decoration (issue 17) — see [`CommitRef::state`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefState {
    #[default]
    Default,
    /// Remote-tracking ref whose upstream branch no longer exists on the
    /// remote (`git branch -vv`'s `[gone]`).
    Gone,
    /// Tag that exists on at least one remote.
    Pushed,
    /// Tag that exists only on this machine.
    LocalOnly,
}

impl CommitRef {
    /// A decoration in its default state (no gone/pushed marker).
    pub fn new(kind: GitRefKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            state: RefState::Default,
        }
    }
}

/// An author / committer signature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub name: String,
    pub email: String,
    /// Seconds since epoch.
    pub time: i64,
}

/// GPG signature state of a commit (issue 17). The CLI adapter derives it
/// from `git log`'s `%G?`; libgit2 can only see a signature's presence, so
/// a signed commit surfaces as [`SignatureState::Unverified`] there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureState {
    /// No signature (`%G?` = N).
    #[default]
    Unsigned,
    /// Signed and verified good (`%G?` = G).
    Good,
    /// Signed and verified bad (`%G?` = B).
    Bad,
    /// Signed but not verifiable (`%G?` = U/E/X, or libgit2 presence-only).
    Unverified,
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
    /// Committer's GPG signature state (issue 17).
    pub signature: SignatureState,
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
    /// Whether the worktree still differs from the index (porcelain v2
    /// `Y ≠ '.'`). With [`Change::staged`] this distinguishes a partially
    /// staged file (`MM`) from a fully staged one (`M.`) — spec R2 story 9.
    #[serde(default)]
    pub unstaged: bool,
    /// Previous path for renames/copies (`R`/`C` status) — the "renamed from"
    /// side of git's own detection driving R8 rename headers. `None` for every
    /// other status.
    #[serde(default)]
    pub orig_path: Option<PathBuf>,
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
    /// The worktree has uncommitted changes (issue 14 status column).
    pub dirty: bool,
    pub root: RootId,
}

/// Lifecycle state of one submodule (issue 14 Submodules tab status column).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubmoduleState {
    /// The submodule's checked-out commit matches the recorded gitlink.
    UpToDate,
    /// The submodule's HEAD moved off the recorded gitlink (`+` in
    /// `git submodule status`).
    NeedsUpdate,
    /// Registered but not initialized (`-`): no working copy checked out.
    Uninitialized,
    /// Merge conflicts inside the submodule (`U`).
    Conflicted,
}

/// A registered submodule of a repository (issue 14 Submodules tab row).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Submodule {
    /// Path relative to the superproject root.
    pub path: PathBuf,
    /// Commit pinned (checked out) in the submodule's HEAD; `None` when
    /// uninitialized.
    pub head: Option<String>,
    /// Gitlink commit recorded in the superproject's index.
    pub recorded: Option<String>,
    pub state: SubmoduleState,
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

/// How often the background incoming check polls remotes (issue #27,
/// screen 11). Only meaningful while `VcsSettings::incoming_poll` is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IncomingCheckInterval {
    #[default]
    Min15,
    Min30,
    Min60,
}

impl IncomingCheckInterval {
    /// The poll period in whole minutes.
    pub fn minutes(self) -> u64 {
        match self {
            IncomingCheckInterval::Min15 => 15,
            IncomingCheckInterval::Min30 => 30,
            IncomingCheckInterval::Min60 => 60,
        }
    }
}

/// Timestamp rendering style in the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DateFormat {
    #[default]
    Relative,
    Absolute,
    Iso,
}

/// Which engine implementation performs git operations
/// (library-migration plan Phase L2). `Auto` is the default strategy: reads
/// go to libgit2 and anything libgit2 cannot do falls back to the CLI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GitBackend {
    /// Shell out to the system `git` binary for every operation.
    Cli,
    /// In-process libgit2 via `git2` for supported operations.
    Libgit2,
    /// Reads go to libgit2; anything libgit2 cannot do falls back to the
    /// CLI (issue #26). The factory maps this to the composed executor.
    #[default]
    Auto,
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
    /// Poll remotes in the background for incoming commits (issue #27).
    #[serde(default)]
    pub incoming_poll: bool,
    /// How often the background incoming check runs.
    #[serde(default)]
    pub incoming_interval: IncomingCheckInterval,
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
    /// Git engine backend (library-migration plan Phase L2).
    #[serde(default)]
    pub backend: GitBackend,
    /// Compute unified diffs in-process with `similar` instead of
    /// post-processing CLI diff text (Phase L1). `false` rolls back to the
    /// CLI-produced text path.
    #[serde(default = "default_true")]
    pub in_process_diffs: bool,
}

fn default_true() -> bool {
    true
}

impl Default for VcsSettings {
    fn default() -> Self {
        Self {
            git_executable: String::new(),
            staging_area: false,
            synchronous_branches: false,
            update_method: UpdateMethod::default(),
            clean_tree_method: CleanTreeMethod::default(),
            incoming_poll: false,
            incoming_interval: IncomingCheckInterval::default(),
            protected_branch_patterns: vec!["main".to_string(), "master".to_string()],
            warn_crlf: true,
            warn_detached: true,
            commit_template: String::new(),
            restore_workspace: false,
            gutter_markers: true,
            date_format: DateFormat::default(),
            no_commit_hooks: false,
            backend: GitBackend::default(),
            in_process_diffs: true,
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
    /// Pickaxe search (issue 17): `Some(s)` scopes the log to commits where
    /// the occurrence count of `s` in the tracked content changed — how
    /// commit search covers code changes. Maps to `git log -S`.
    pub pickaxe: Option<String>,
}

/// Options for a merge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergeOpts {
    pub no_ff: bool,
    pub ff_only: bool,
    pub squash: bool,
    pub no_commit: bool,
    pub no_verify: bool,
    pub verify_signatures: bool,
    pub allow_unrelated: bool,
    pub message: Option<String>,
}

/// Options for creating a tag (issue 31, screen 16).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagSpec {
    pub name: String,
    /// Commit-ish the tag points at; `None` = HEAD.
    pub target: Option<String>,
    /// Annotation message; `Some` = an annotated tag (`-a -m`), `None` =
    /// lightweight.
    pub message: Option<String>,
    /// Tagger identity override ("Name <email>"); `None` = the git config
    /// identity.
    pub tagger: Option<String>,
    /// GPG-sign the tag (`-s`).
    pub sign: bool,
}

/// Split a "Name <email>" identity string into its parts (issue 31). The
/// email is the bracketed segment; everything before it is the name. A
/// string without brackets is treated as a bare name; blank input yields
/// `(None, None)`.
pub fn parse_identity(s: &str) -> (Option<String>, Option<String>) {
    let s = s.trim();
    if s.is_empty() {
        return (None, None);
    }
    match (s.find('<'), s.find('>')) {
        (Some(l), Some(r)) if l < r => {
            let name = s[..l].trim();
            let email = s[l + 1..r].trim();
            (
                (!name.is_empty()).then(|| name.to_string()),
                (!email.is_empty()).then(|| email.to_string()),
            )
        }
        _ => (Some(s.to_string()), None),
    }
}

/// The merge dialog's STRATEGY segmented control (issue 28, screen 14): the
/// one choice that drives which mutually exclusive merge flags are sent, as
/// mapped by `integrate_service::merge_flags`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergeStrategy {
    /// `--no-commit`: merge and stage, leave the commit to the user.
    #[default]
    NoCommit,
    /// A plain merge (commit created by git).
    Commit,
    /// `--squash`: fold the incoming changes into one non-merge commit.
    Squash,
    /// `--ff-only`: refuse unless the merge is a fast-forward.
    FastForward,
}

/// Options for a rebase.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RebaseOpts {
    pub onto: Option<String>,
    pub rebase_merges: bool,
    pub keep_empty: bool,
    pub root: bool,
    pub update_refs: bool,
    pub autosquash: bool,
}

/// The rebase dialog's MODE segmented control (issue 29, screen 15). The
/// mode decides which rebase invocation runs, as mapped by
/// `integrate_service::rebase_mode_opts`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RebaseMode {
    /// Replay `onto..HEAD` through the interactive plan (all-pick).
    #[default]
    Interactive,
    /// A plain `git rebase <onto>` with the chosen options.
    Standard,
    /// `git rebase --autosquash <onto>`: fixup!/squash! commits folded.
    Autosquash,
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
