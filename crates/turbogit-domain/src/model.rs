use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::git_path_quoting::{QuoteTrigger, quote_c};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RootId(pub Arc<Path>);

pub trait DisplayName {
    /// The final component when it is UTF-8, otherwise `fallback`.
    fn display_name_or(&self, fallback: &str) -> String;

    /// The lossily-decoded final component, otherwise `fallback`.
    fn display_name_lossy_or(&self, fallback: &str) -> String;

    /// The final component when it is UTF-8, borrowed, for a site that wants
    /// the name's *presence* rather than a label.
    fn utf8_name(&self) -> Option<&str>;
}

impl DisplayName for Path {
    fn display_name_or(&self, fallback: &str) -> String {
        self.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(fallback)
            .to_owned()
    }

    fn display_name_lossy_or(&self, fallback: &str) -> String {
        self.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| fallback.to_owned())
    }

    fn utf8_name(&self) -> Option<&str> {
        self.file_name().and_then(|s| s.to_str())
    }
}

impl DisplayName for PathBuf {
    fn display_name_or(&self, fallback: &str) -> String {
        self.as_path().display_name_or(fallback)
    }

    fn display_name_lossy_or(&self, fallback: &str) -> String {
        self.as_path().display_name_lossy_or(fallback)
    }

    fn utf8_name(&self) -> Option<&str> {
        self.as_path().utf8_name()
    }
}

impl DisplayName for RootId {
    fn display_name_or(&self, fallback: &str) -> String {
        self.as_path().display_name_or(fallback)
    }

    fn display_name_lossy_or(&self, fallback: &str) -> String {
        self.as_path().display_name_lossy_or(fallback)
    }

    fn utf8_name(&self) -> Option<&str> {
        self.as_path().utf8_name()
    }
}

impl RootId {
    pub fn as_path(&self) -> &Path {
        &self.0
    }
    pub fn name(&self) -> String {
        self.as_path()
            .display_name_lossy_or(&self.as_path().to_string_lossy())
    }
}

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
    /// exact (`Path` equality). Windows paths remain case-sensitive.
    pub fn resolve_change(&self, path: &Path) -> Option<&Change> {
        self.status
            .changes
            .iter()
            .find(|c| c.path == path || self.id.0.join(&c.path) == *path)
    }

    pub fn canonical_key(&self, change: &Change) -> PathBuf {
        self.id.0.join(&change.path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChangeRef {
    pub root: RootId,
    pub path: PathBuf,
}

impl ChangeRef {
    pub fn new(root: RootId, path: impl Into<PathBuf>) -> Self {
        Self {
            root,
            path: path.into(),
        }
    }
}

pub type BranchRef = String;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    pub name: String,
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BranchKind {
    Local,
    Remote,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upstream {
    pub remote: String,
    pub branch: String,
}

impl Upstream {
    /// git's `<remote>/<branch>` form, for the argv and rev-spec positions that
    /// need it. Not a thing to compare two upstreams by. A branch tracking
    /// *another local branch* has git's `.` remote, whose rev-spec is the plain
    /// branch name.
    pub fn git_ref(&self) -> String {
        if self.remote == "." {
            self.branch.clone()
        } else {
            format!("{}/{}", self.remote, self.branch)
        }
    }

    /// Read git's spelling back. Only the first slash separates, because branch
    /// names contain them; a name with no remote part is not an upstream.
    pub fn from_git_ref(s: &str) -> Option<Upstream> {
        let (remote, branch) = s.split_once('/')?;
        if remote.is_empty() || branch.is_empty() {
            return None;
        }
        Some(Upstream {
            remote: remote.to_string(),
            branch: branch.to_string(),
        })
    }
}

/// A branch named the way git can be handed it: a local branch by its own name,
/// a remote one by `<remote>/<name>`, which is NOT its `name`. A row under a
/// remote carries both halves separately, so the only sound way to build one is
/// from the row itself; `From<&Branch>` is that way, and a hand-written name
/// cannot carry a remote it does not mention.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSpec {
    pub name: String,
    /// `None` for a local branch. `Some("origin")` for a row under a remote.
    /// `kind` is deliberately not carried: `remote.is_some()` already says it,
    /// and two fields would be two places to keep in step.
    pub remote: Option<String>,
}

impl BranchSpec {
    pub fn local(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            remote: None,
        }
    }

    pub fn remote(remote: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            remote: Some(remote.into()),
        }
    }

    /// The one spelling both the start-point argv and the stating row read.
    /// `refs/remotes/<remote>/<name>` is addressable as `<remote>/<name>`, and a
    /// local ref is addressable as its own name.
    pub fn git_ref(&self) -> String {
        match &self.remote {
            Some(remote) => format!("{remote}/{}", self.name),
            None => self.name.clone(),
        }
    }
}

impl From<&Branch> for BranchSpec {
    fn from(branch: &Branch) -> Self {
        Self {
            name: branch.name.clone(),
            remote: branch.remote.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub kind: BranchKind,
    pub tracking: Option<Upstream>,
    pub favorite: bool,
    pub protected: bool,
    pub exists: bool,
    #[serde(default)]
    pub ahead: usize,
    #[serde(default)]
    pub behind: usize,
    #[serde(default)]
    pub gone: bool,
    #[serde(default)]
    pub last_touched: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub tip: Option<BranchTip>,
    #[serde(default)]
    pub remote: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchTip {
    pub short_hash: String,
    pub message: String,
    pub author: String,
    pub time: chrono::DateTime<chrono::Utc>,
}

impl BranchTip {
    pub fn from_parts(
        short_hash: impl Into<String>,
        subject: impl Into<String>,
        author: impl Into<String>,
        time: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            short_hash: short_hash.into(),
            message: subject.into(),
            author: author.into(),
            time,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentCheckout {
    pub name: String,
    pub at: i64,
}

/// The three conflict markers this parser understands and both painters draw.
/// They are git wire-format facts with no service policy in them, so they live
/// in the leaf crate every other layer already sees: `services` parses them,
/// `ui` paints them, and neither can name a different marker than the other.
/// Deliberately three, and that is a fact about the conflict style in use rather
/// than an oversight. git can also write a *common ancestor* marker,
/// `||||||| <base>`, and does so when the merge is configured for the `diff3` or
/// `zdiff3` conflict style. Nothing in this tree parses or draws that form. If
/// support for it is ever added it must be added here first: a parser that grew
/// a fourth marker while the two painters still drew three would show a user a
/// conflict block with no base section and no way to tell why.
pub const CONFLICT_MARKER_OURS: &str = "<<<<<<<";
pub const CONFLICT_MARKER_SEPARATOR: &str = "=======";
pub const CONFLICT_MARKER_THEIRS: &str = ">>>>>>>";

pub type CommitId = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitRefKind {
    Branch,
    Remote,
    Tag,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRef {
    pub kind: GitRefKind,
    pub name: String,
    /// Sync state of the decoration: a remote-tracking ref is
    /// [`RefState::Gone`] when its upstream branch was deleted on the remote; a
    /// tag is [`RefState::Pushed`] or [`RefState::LocalOnly`]; everything else
    /// is [`RefState::Default`] (no marker).
    #[serde(default)]
    pub state: RefState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefState {
    #[default]
    Default,
    Gone,
    Pushed,
    LocalOnly,
}

impl CommitRef {
    pub fn new(kind: GitRefKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            state: RefState::Default,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub name: String,
    pub email: String,
    pub time: i64,
}

/// GPG signature state of a commit. The CLI adapter derives it
/// from `git log`'s `%G?`; libgit2 can only see a signature's presence, so
/// a signed commit surfaces as [`SignatureState::Unverified`] there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureState {
    #[default]
    Unsigned,
    Good,
    Bad,
    Unverified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub id: CommitId,
    pub parents: Vec<CommitId>,
    pub author: Signature,
    pub committer: Signature,
    pub message: String,
    pub time: i64,
    pub root: RootId,
    pub signature: SignatureState,
}

pub const SHORT_COMMIT_REF_CHARS: usize = 7;

pub fn short_commit_ref(reference: &str) -> String {
    reference.chars().take(SHORT_COMMIT_REF_CHARS).collect()
}

pub fn subject_of_message(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}

impl Commit {
    pub fn subject(&self) -> &str {
        subject_of_message(&self.message)
    }
}

/// A path in git's spelling: forward slashes throughout, so a path string means the
/// same thing on every platform.
/// This is the **plain** rewrite, separators become `/` and nothing else moves:
/// `a//b` stays `a//b`, and a filename that genuinely contains a `\` is rewritten too.
/// The two places wanting the *other* answer keep their own spelling: `diff_engine`'s
/// patch headers normalise through `Path::components`, and the sidebar's collapse keys
/// add a project-prefix strip on top.
pub fn forward_slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chunk {
    pub start_line: usize,
    pub end_line: usize,
    pub selected: bool,
    pub staged: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub path: PathBuf,
    pub status: ChangeStatus,
    pub chunks: Vec<Chunk>,
    pub staged: bool,
    /// Whether the worktree still differs from the index (porcelain v2
    /// `Y ≠ '.'`). With [`Change::staged`] this distinguishes a partially
    /// staged file (`MM`) from a fully staged one (`M.`).
    #[serde(default)]
    pub unstaged: bool,
    #[serde(default)]
    pub orig_path: Option<PathBuf>,
}

/// The **file-level clash kind** of one conflicted path, which sides of the
/// merge touched it, as `git status`'s short-format unmerged pairs report them
/// (`DD` both deleted, `AA` both added, `UU` both modified, `DU` deleted by us,
/// `UD` deleted by them, `AU` added by us, `UA` added by them). This is a
/// different axis from the per-hunk marker vocabulary an editor names
/// (`Modified line` / `Conflicting lines`): one is about the FILE, one about a
/// BLOCK inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictKind {
    BothDeleted,
    BothAdded,
    BothModified,
    DeletedByUs,
    DeletedByThem,
    AddedByUs,
    AddedByThem,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Changelist {
    pub name: String,
    pub active: bool,
    pub changes: Vec<Change>,
    pub root: RootId,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RootStatus {
    pub changes: Vec<Change>,
    pub conflicted: Vec<PathBuf>,
    #[serde(default)]
    pub conflict_kinds: Vec<(PathBuf, ConflictKind)>,
}

impl RootStatus {
    pub fn conflict_kind(&self, path: &Path) -> Option<ConflictKind> {
        self.conflict_kinds
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, kind)| *kind)
    }

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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conflict {
    pub path: PathBuf,
    pub base: PathBuf,
    pub local: PathBuf,
    pub incoming: PathBuf,
    pub resolved: bool,
    pub root: RootId,
}

/// The three index versions of a conflicted path, as the **Git engine** answers
/// them for one path, the glossary's **Conflict**. Named for the sides, not for
/// git's stage numbering, and the engine that reads the index is the one that
/// knows which stage is which.
/// A side is `None` when the index holds no version of it: an add/add conflict
/// has no base, and a one-sided delete has no side. That is not the same answer
/// as an empty one, and a resolver that conflates them merges against a phantom.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConflictVersions {
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shelf {
    pub name: String,
    pub changes: Vec<Change>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub root: RootId,
    /// Where this shelf's **content** actually lives, see [`ShelfSource`].
    /// A row written before an imported patch could be parked has no key for this and
    /// reads back as [`ShelfSource::Stash`], which is exactly what every such row is;
    /// the store's own mirror (`StoredShelf`) carries that default so this field stays
    /// required in memory.
    #[serde(default)]
    pub source: ShelfSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShelfSource {
    #[default]
    Stash,
    Patch {
        file: String,
    },
}

impl ShelfSource {
    pub fn file_name(&self) -> Option<&str> {
        match self {
            Self::Stash => None,
            Self::Patch { file } => Some(file),
        }
    }

    pub fn is_stash(&self) -> bool {
        matches!(self, Self::Stash)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stash {
    pub message: String,
    pub root: RootId,
    pub index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
    /// Whether the worktree has uncommitted changes.
    /// `None` while the per-worktree dirty probe has not run: the worktree
    /// list is decoupled from the probe, so a freshly listed worktree's
    /// dirtiness is unknown, not clean.
    pub dirty: Option<bool>,
    pub root: RootId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubmoduleState {
    UpToDate,
    NeedsUpdate,
    Uninitialized,
    Conflicted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Submodule {
    pub path: PathBuf,
    pub head: Option<String>,
    pub recorded: Option<String>,
    pub state: SubmoduleState,
    pub root: RootId,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MultiRootManager {
    pub roots: Vec<Root>,
    pub synchronous_branches: bool,
}

impl MultiRootManager {
    pub fn by_id(&self, id: &RootId) -> Option<&Root> {
        self.roots.iter().find(|r| &r.id == id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirMapping {
    pub directory: PathBuf,
    pub vcs: Vcs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Vcs {
    Git,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UpdateMethod {
    #[default]
    Merge,
    Rebase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CleanTreeMethod {
    #[default]
    Stash,
    Shelve,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IncomingCheckInterval {
    Min15,
    Min30,
    #[default]
    Min60,
}

impl IncomingCheckInterval {
    pub fn minutes(self) -> u64 {
        match self {
            IncomingCheckInterval::Min15 => 15,
            IncomingCheckInterval::Min30 => 30,
            IncomingCheckInterval::Min60 => 60,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DateFormat {
    #[default]
    Relative,
    Absolute,
    Iso,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsSettings {
    pub git_executable: String,
    pub synchronous_branches: bool,
    pub update_method: UpdateMethod,
    pub clean_tree_method: CleanTreeMethod,
    #[serde(default)]
    pub incoming_poll: bool,
    #[serde(default)]
    pub incoming_interval: IncomingCheckInterval,
    pub protected_branch_patterns: Vec<String>,
    pub warn_crlf: bool,
    pub warn_detached: bool,
    pub restore_workspace: bool,
    pub gutter_markers: bool,
    pub date_format: DateFormat,
    #[serde(default = "default_true")]
    pub in_process_diffs: bool,
    #[serde(default)]
    pub auto_update_on_push_rejected: bool,
    /// `When patch is created:`, what a create route does with the file it wrote.
    /// `serde(default)` is load-bearing here, not tidiness: this store is read at
    /// launch and rewritten on every settings apply, so a required new key would make
    /// every `state.ron` written before it existed fail to parse and hand the writer
    /// an empty settings block to save back over the user's own. The default is the
    /// captured **Ask**, so the arm a legacy file lands on is the arm that asks.
    #[serde(default)]
    pub when_patch_is_created: PatchCreatedPolicy,
}

fn default_true() -> bool {
    true
}

impl Default for VcsSettings {
    fn default() -> Self {
        Self {
            git_executable: String::new(),
            synchronous_branches: false,
            update_method: UpdateMethod::default(),
            clean_tree_method: CleanTreeMethod::default(),
            incoming_poll: false,
            incoming_interval: IncomingCheckInterval::default(),
            protected_branch_patterns: vec!["main".to_string(), "master".to_string()],
            warn_crlf: true,
            warn_detached: true,
            restore_workspace: false,
            gutter_markers: true,
            date_format: DateFormat::default(),
            in_process_diffs: true,
            auto_update_on_push_rejected: false,
            when_patch_is_created: PatchCreatedPolicy::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectState {
    pub mappings: Vec<DirMapping>,
    pub settings: VcsSettings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogOpts {
    pub max_count: Option<usize>,
    /// Entries to discard from the *front* of the listing before the page is
    /// cut, the position half of log paging. The walk still starts at HEAD,
    /// so the union of consecutive pages is a prefix of the uncapped listing
    /// and can never have holes behind a merge's second parent. `None` (and
    /// `Some(0)`) leave the listing untouched.
    pub skip: Option<usize>,
    pub branch: Option<String>,
    pub path: Option<PathBuf>,
    pub pickaxe: Option<String>,
}

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagSpec {
    pub name: String,
    pub target: Option<String>,
    pub message: Option<String>,
    pub tagger: Option<String>,
    pub sign: bool,
}

/// Split a "Name <email>" identity string into its parts. The
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergeStrategy {
    #[default]
    NoCommit,
    Commit,
    Squash,
    FastForward,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RebaseOpts {
    pub onto: Option<String>,
    pub rebase_merges: bool,
    pub keep_empty: bool,
    pub root: bool,
    pub update_refs: bool,
    pub autosquash: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RebaseMode {
    #[default]
    Interactive,
    Standard,
    Autosquash,
}

pub fn git_binary(settings: &VcsSettings) -> String {
    if settings.git_executable.trim().is_empty() {
        "git".to_string()
    } else {
        settings.git_executable.clone()
    }
}

/// The `Ignore Differences` policy: which whitespace differences count as a
/// change at all.
/// The five values are a **ladder**, and each rung has to be distinguishable from
/// the one below it or the ladder has one value and five names:
/// [`Self::TrimWhitespace`] ignores only a line's outer whitespace,
/// [`Self::IgnoreWhitespace`] ignores it anywhere within a line,
/// [`Self::IgnoreWhitespaceAndEmptyLines`] additionally ignores a change that only
/// adds or removes whole lines of nothing, or that splits/joins lines without
/// touching their non-whitespace parts, and
/// [`Self::IgnoreImportsAndFormatting`] adds import-block changes while
/// *respecting* whitespace inside a string literal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiffWhitespace {
    #[default]
    None,
    TrimWhitespace,
    IgnoreWhitespace,
    IgnoreWhitespaceAndEmptyLines,
    IgnoreImportsAndFormatting,
}

impl DiffWhitespace {
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::TrimWhitespace,
        Self::IgnoreWhitespace,
        Self::IgnoreWhitespaceAndEmptyLines,
        Self::IgnoreImportsAndFormatting,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::TrimWhitespace => "Trim whitespaces",
            Self::IgnoreWhitespace => "Ignore whitespaces",
            Self::IgnoreWhitespaceAndEmptyLines => "Ignore whitespaces and empty lines",
            Self::IgnoreImportsAndFormatting => "Ignore imports and formatting",
        }
    }
}

/// The `Highlighting Differences` policy: how finely a changed line's highlight
/// is cut up.
/// [`Self::Lines`] highlights each changed row as one block;
/// [`Self::Words`] and [`Self::Characters`] highlight only the differing word or
/// character runs inside it; [`Self::SplitChanges`] cuts a multi-line change into
/// the separate changes it is made of, so a line that merely moved inside the
/// change is not highlighted at all; [`Self::None`] highlights nothing, which is
/// the value the reference recommends for a heavily modified file, where
/// highlighting adds difficulty instead of removing it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiffHighlighting {
    Words,
    #[default]
    Lines,
    SplitChanges,
    Characters,
    None,
}

impl DiffHighlighting {
    pub const ALL: [Self; 5] = [
        Self::Words,
        Self::Lines,
        Self::SplitChanges,
        Self::Characters,
        Self::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Words => "Words",
            Self::Lines => "Lines",
            Self::SplitChanges => "Split changes",
            Self::Characters => "Characters",
            Self::None => "None",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DiffPolicies {
    pub whitespace: DiffWhitespace,
    pub highlighting: DiffHighlighting,
}

#[derive(Clone, Debug, Default)]
pub struct DiffOpts {
    pub staged: bool,
    pub commit: Option<String>,
    pub left: Option<String>,
    pub right: Option<String>,
    pub path: Option<PathBuf>,
    pub ignore_whitespace: bool,
    pub stat: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub path: PathBuf,
    /// Lines added. `None` is git's `-`: a **Binary change** has no line count,
    /// which is a different answer from a count of zero. This is the one place
    /// that decision lives; callers that want a number read `unwrap_or(0)`.
    pub insertions: Option<usize>,
    pub deletions: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeStats {
    pub files: Vec<FileStat>,
}

impl ChangeStats {
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn insertions(&self) -> usize {
        self.files.iter().filter_map(|f| f.insertions).sum()
    }

    pub fn deletions(&self) -> usize {
        self.files.iter().filter_map(|f| f.deletions).sum()
    }
}

/// The question the engine's `change_stats` read answers. One read serves both
/// asks because both are the same question about a different pair.
/// Rename detection is inherited, not pinned: git's own defaults decide whether
/// a copied file's stat is the same question as a modified file's, and the
/// engine follows whatever the repository configures, the same policy the
/// glossary's **Rename header** entry states.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChangeQuestion {
    Commit { commit: String },
    MergeIntoHead { target: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchLine {
    pub kind: PatchLineKind,
    pub text: String,
    pub no_newline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    /// The section heading git appends after the second `@@`, the enclosing
    /// function name. Carried, because re-emitting a hunk without it produces
    /// text that differs from git's byte for byte, and because the viewer shows
    /// it. `None` when git found no heading.
    pub heading: Option<String>,
    pub lines: Vec<PatchLine>,
}

/// One metadata line of a file section, kept in the order git wrote it.
/// Each variant is a fact the viewer and the staging path ask about, not a
/// prefix to test. The order is part of the value because the diff view paints
/// these lines as section metadata, in git's sequence, and a **Rename header**
/// leads the content it describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchHeaderLine {
    Index {
        old: String,
        new: String,
        mode: Option<String>,
    },
    Similarity {
        percent: u8,
    },
    RenameFrom {
        path: String,
    },
    RenameTo {
        path: String,
    },
    NewFile {
        mode: String,
    },
    DeletedFile {
        mode: String,
    },
    OldMode {
        mode: String,
    },
    NewMode {
        mode: String,
    },
    Binary,
    /// The `--- <old>` / `+++ <new>` source pair, each side git's own form
    /// (`a/path`, `b/path`, or `/dev/null` for the side that does not exist)
    /// with quoting already resolved. Which side is `/dev/null` is what makes a
    /// composed patch appliable, so the pair is one fact: a patch's two sides
    /// are one question.
    Sources {
        old: String,
        new: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PatchFile {
    pub old_path: String,
    pub new_path: String,
    /// This is git's **combined** view of an unmerged path (`diff --cc`), which
    /// names one path and compares it against every parent at once. Its hunks
    /// keep the first parent's range and the new side's, because the value has
    /// two sides; the staging verbs refuse a conflicted file anyway, so the
    /// reading is for the pane, not for `git apply`.
    pub combined: bool,
    pub headers: Vec<PatchHeaderLine>,
    pub hunks: Vec<PatchHunk>,
}

impl PatchFile {
    fn has(&self, want: impl Fn(&PatchHeaderLine) -> bool) -> bool {
        self.headers.iter().any(want)
    }

    fn first<T>(&self, want: impl Fn(&PatchHeaderLine) -> Option<T>) -> Option<T> {
        self.headers.iter().find_map(want)
    }

    pub fn similarity(&self) -> Option<u8> {
        self.first(|l| match l {
            PatchHeaderLine::Similarity { percent } => Some(*percent),
            _ => None,
        })
    }

    pub fn renamed(&self) -> bool {
        self.has(|l| matches!(l, PatchHeaderLine::RenameFrom { .. }))
    }

    pub fn new_file(&self) -> bool {
        self.has(|l| matches!(l, PatchHeaderLine::NewFile { .. }))
    }

    pub fn deleted_file(&self) -> bool {
        self.has(|l| matches!(l, PatchHeaderLine::DeletedFile { .. }))
    }

    pub fn binary(&self) -> bool {
        self.has(|l| matches!(l, PatchHeaderLine::Binary))
    }

    pub fn old_mode(&self) -> Option<&str> {
        self.headers.iter().find_map(|l| match l {
            PatchHeaderLine::OldMode { mode } | PatchHeaderLine::DeletedFile { mode } => {
                Some(mode.as_str())
            }
            _ => None,
        })
    }

    pub fn new_mode(&self) -> Option<&str> {
        self.headers.iter().find_map(|l| match l {
            PatchHeaderLine::NewMode { mode } | PatchHeaderLine::NewFile { mode } => {
                Some(mode.as_str())
            }
            _ => None,
        })
    }

    pub fn mode_only(&self) -> bool {
        self.hunks.is_empty()
            && !self.binary()
            && !self.new_file()
            && !self.deleted_file()
            && !self.renamed()
            && (self.old_mode().is_some() || self.new_mode().is_some())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchOrigin {
    LocalChanges { paths: Vec<PathBuf> },
    Commit { id: String },
    FileRevision { commit: String, path: PathBuf },
}

impl PatchOrigin {
    pub fn needs_comment(&self) -> bool {
        matches!(self, Self::LocalChanges { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PatchCreatedPolicy {
    #[default]
    Ask,
    ShowInFiles,
    DoNothing,
}

impl PatchCreatedPolicy {
    pub const ALL: [Self; 3] = [Self::Ask, Self::ShowInFiles, Self::DoNothing];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::ShowInFiles => "Show in files",
            Self::DoNothing => "Do nothing",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PatchEncoding {
    #[default]
    Utf8,
    Utf8WithBom,
    Latin1,
}

impl PatchEncoding {
    pub const ALL: [Self; 3] = [Self::Utf8, Self::Utf8WithBom, Self::Latin1];

    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8WithBom => "UTF-8 with BOM",
            Self::Latin1 => "ISO-8859-1",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Patch {
    pub files: Vec<PatchFile>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn hunk_count(&self) -> usize {
        self.files.iter().map(|f| f.hunks.len()).sum()
    }
}

impl PatchLine {
    pub const NO_NEWLINE_MARKER: &'static str = "\\ No newline at end of file";
}

impl PatchHunk {
    pub fn header_line(&self) -> String {
        let mut out = String::from("@@ -");
        out.push_str(&span_text(self.old_start, self.old_count));
        out.push_str(" +");
        out.push_str(&span_text(self.new_start, self.new_count));
        out.push_str(" @@");
        if let Some(heading) = &self.heading {
            out.push(' ');
            out.push_str(heading);
        }
        out
    }
}

impl PatchFile {
    pub fn section_header_line(&self) -> String {
        if self.combined {
            return format!(
                "diff --cc {}",
                quote_c(&self.new_path, QuoteTrigger::GitSpelling)
            );
        }
        format!(
            "diff --git {} {}",
            git_side("a/", &self.old_path),
            git_side("b/", &self.new_path)
        )
    }
}

impl PatchHeaderLine {
    pub fn lines(&self, file: &PatchFile) -> Vec<String> {
        match self {
            PatchHeaderLine::Index { old, new, mode } => {
                let mut line = format!("index {old}..{new}");
                if let Some(mode) = mode {
                    line.push(' ');
                    line.push_str(mode);
                }
                vec![line]
            }
            PatchHeaderLine::Similarity { percent } => {
                vec![format!("similarity index {percent}%")]
            }
            PatchHeaderLine::RenameFrom { path } => {
                vec![format!(
                    "rename from {}",
                    quote_c(path, QuoteTrigger::GitSpelling)
                )]
            }
            PatchHeaderLine::RenameTo { path } => {
                vec![format!(
                    "rename to {}",
                    quote_c(path, QuoteTrigger::GitSpelling)
                )]
            }
            PatchHeaderLine::NewFile { mode } => vec![format!("new file mode {mode}")],
            PatchHeaderLine::DeletedFile { mode } => vec![format!("deleted file mode {mode}")],
            PatchHeaderLine::OldMode { mode } => vec![format!("old mode {mode}")],
            PatchHeaderLine::NewMode { mode } => vec![format!("new mode {mode}")],
            PatchHeaderLine::Binary => vec![format!(
                "Binary files {} and {} differ",
                if file.new_file() {
                    "/dev/null".to_string()
                } else {
                    git_side("a/", &file.old_path)
                },
                if file.deleted_file() {
                    "/dev/null".to_string()
                } else {
                    git_side("b/", &file.new_path)
                }
            )],
            PatchHeaderLine::Sources { old, new } => {
                vec![
                    format!("--- {}", quote_side(old)),
                    format!("+++ {}", quote_side(new)),
                ]
            }
        }
    }
}

fn span_text(start: usize, count: usize) -> String {
    if count == 1 {
        format!("{start}")
    } else {
        format!("{start},{count}")
    }
}

fn git_side(prefix: &str, path: &str) -> String {
    quote_side(&format!("{prefix}{path}"))
}

fn quote_side(side: &str) -> String {
    if side == "/dev/null" {
        return side.to_string();
    }
    quote_c(side, QuoteTrigger::GitSpelling)
}

impl std::fmt::Display for Patch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for file in &self.files {
            writeln!(f, "{}", file.section_header_line())?;
            for header in &file.headers {
                for line in header.lines(file) {
                    writeln!(f, "{line}")?;
                }
            }
            for hunk in &file.hunks {
                writeln!(f, "{}", hunk.header_line())?;
                for line in &hunk.lines {
                    let prefix = match line.kind {
                        PatchLineKind::Context => ' ',
                        PatchLineKind::Added => '+',
                        PatchLineKind::Removed => '-',
                    };
                    writeln!(f, "{prefix}{}", line.text)?;
                    if line.no_newline {
                        writeln!(f, "{}", PatchLine::NO_NEWLINE_MARKER)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    pub commit: CommitId,
    pub author: String,
    pub time: i64,
    pub line_no: usize,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebaseAction {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
    UpdateRef,
    /// The parser's fallback for a REBASE-TODO verb it does not recognise: a
    /// row can exist that the plan editor did not create, so it survives with
    /// this action and renders as `Unknown`. It must never reach git as
    /// another verb, coercing it to `pick` would produce a valid-looking
    /// wrong plan, so the engine refuses a plan that carries one.
    Unknown,
}

/// A row of the interactive rebase plan.
/// `message` is the replacement commit message a [`RebaseAction::Reword`] row
/// carries, and it is the reason a reword is not a no-op: git's own rebase todo
/// has nowhere to put a message, so it rides on the plan row instead and the
/// engine hands it to git's editor at execution (ADR-0025). `None` on every
/// other row, and `parse_todo` can never produce one, rendering a plan to todo
/// text drops the message deliberately, so the editor's buffer stays a list of
/// verbs rather than becoming a message store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebasePlanEntry {
    pub action: RebaseAction,
    pub commit: CommitId,
    pub subject: String,
    pub message: Option<String>,
}
