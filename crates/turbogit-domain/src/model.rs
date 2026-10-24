//! Domain data model for TurboGit.
//!
//! Every mutable git state is **scoped to a `Root`** — single-root code never
//! assumes a global "the repository". All types are `Clone + Debug +
//! Serialize/Deserialize` so the UI can store/restore them and the persistence
//! layer can serialize settings/state under `.turbogit/`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::git_path_quoting::{QuoteTrigger, quote_c};

/// Identity of a git repository root: its absolute path.
///
/// The path is shared through an [`Arc`] so the dozens of identity clones per
/// operation burst (cache keys, event payloads, `'static` worker captures)
/// are refcount bumps instead of heap allocations. `Arc<Path>` hashes and
/// compares exactly like the underlying [`Path`], so map keys are unaffected.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RootId(pub Arc<Path>);

/// What a path calls itself on screen — the one owner of the rule, with each
/// surface's placeholder carried in as an argument.
///
/// The hazard the code cannot show: **sixteen** call sites in this workspace
/// chose the strict half and **fourteen** chose the lossy half, and nobody
/// chose that split — it was whichever combinator the last editor reached for.
/// Both halves ship, and they disagree about a repository directory whose name
/// is not valid UTF-8: [`DisplayName::display_name_or`] drops the name and shows
/// the caller's fallback, while [`DisplayName::display_name_lossy_or`] keeps it
/// mangled but present. Two methods rather than one entry plus a policy
/// argument, so the divergence reads as a word at each call site; that is the
/// only reason it can ever be ruled on.
///
/// Which placeholder a surface shows is the one decision that is genuinely its
/// own, so it stays an argument: `"<workspace>"`, `"<repo>"`, `"<worktree>"`,
/// `"?"`, `"Project"`, or the path itself. Callers pass the string they passed
/// before, and the rendered bytes are unchanged by using this.
///
/// Two callers consume the **borrowed strict** entry on purpose, because their
/// `None` means "this path has no readable name" and they consume that `None`:
/// the bulk-run summary's `filter_map` drops such a root from the toast, and
/// the incoming-check poller records it as a workspace-wide event. Both losses
/// are preserved deliberately and pinned; do not point either at the lossy
/// entry or hand it a placeholder without the ruling those pins are waiting for.
pub trait DisplayName {
    /// The final component when it is UTF-8, otherwise `fallback`.
    fn display_name_or(&self, fallback: &str) -> String;

    /// The lossily-decoded final component, otherwise `fallback`.
    fn display_name_lossy_or(&self, fallback: &str) -> String;

    /// The final component when it is UTF-8, borrowed — for a site that wants
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

// `PathBuf` and `RootId` forward to the `Path` rule rather than re-deriving it:
// a call site reads the same one method call whichever shape it holds.
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
    /// A root's name is its directory's name, lossily decoded — and the whole
    /// path when it has no directory name at all. Its fallback is this type's
    /// own decision rather than a caller's, so it is spelled here and the rule
    /// itself is the owner's.
    pub fn name(&self) -> String {
        self.as_path()
            .display_name_lossy_or(&self.as_path().to_string_lossy())
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

/// One change, tied to the repository that owns it. A repo-relative path does
/// not name a change in a multi-root workspace, because two roots can hold the
/// same relative path.
///
/// Resolution goes through the root it names, and nowhere else:
/// [`Root::resolve_change`] and [`Root::canonical_key`] stay the only owners of
/// the path-matching and path-joining rules.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChangeRef {
    pub root: RootId,
    /// Repo-relative, exactly as [`Change::path`].
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

/// A branch's upstream: the remote it tracks and the branch name there. Stored
/// as the pair it always was in the app's own terms — git's `<remote>/<branch>`
/// spelling is assembled only where argv wants it, so no surface splits one
/// string to get half of it.
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

/// A git branch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub kind: BranchKind,
    /// The upstream this branch tracks, as the **Git engine**'s branch read
    /// answers it.
    pub tracking: Option<Upstream>,
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
    /// The tip commit this branch points at, captured on the same listing
    /// read (issue 02 — no extra git calls per branch). Drives
    /// search-by-message and the detail panel's latest-commit block.
    #[serde(default)]
    pub tip: Option<BranchTip>,
    /// The remote a remote-tracking branch belongs to (e.g. `origin`), when
    /// kind is [`BranchKind::Remote`] (issue 13 — remote rows group under
    /// their remote's name). `None` for local branches.
    #[serde(default)]
    pub remote: Option<String>,
}

/// The tip commit of a branch (issue 02), gathered on the listing read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchTip {
    /// Short SHA of the tip commit (git's minimal abbreviation).
    pub short_hash: String,
    /// First line of the tip commit message (the subject).
    pub message: String,
    /// Author name of the tip commit.
    pub author: String,
    /// Committer time of the tip commit.
    pub time: chrono::DateTime<chrono::Utc>,
}

impl BranchTip {
    /// Build a tip from the four parts both engines gather, so the shape is written
    /// once rather than as a field-for-field literal in each adapter.
    ///
    /// `short_hash` is the caller's *already-shortened* reference on purpose: the CLI
    /// reads `%(objectname:short)` out of `for-each-ref` while libgit2 shortens a
    /// full OID through [`short_commit_ref`]. Folding that in would move a rule, not
    /// share a shape.
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

/// One checkout the Git engine's reflog read answers with: the ref HEAD moved
/// to, and the unix timestamp of the move.
///
/// It lives in the leaf crate with every other [`crate::model`] answer type
/// because two layers name it — the port it answers and the branches view that
/// groups by it — and neither of them may reach through the other to say the
/// same thing in a tuple.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentCheckout {
    /// The ref the checkout moved to. A checkout of a tag or a raw sha lands
    /// here too, because the reflog records the move whatever its target was;
    /// joining the name against the branch snapshot is the caller's job.
    pub name: String,
    /// Unix timestamp of the checkout — the reflog entry's own, so two
    /// checkouts inside the same second tie and the caller keeps the engine's
    /// newest-first order for ties.
    pub at: i64,
}

/// The three conflict markers this parser understands and both painters draw.
/// They are git wire-format facts with no service policy in them, so they live
/// in the leaf crate every other layer already sees: `services` parses them,
/// `ui` paints them, and neither can name a different marker than the other.
///
/// Deliberately three, and that is a fact about the conflict style in use rather
/// than an oversight. git can also write a *common ancestor* marker,
/// `||||||| <base>`, and does so when the merge is configured for the `diff3` or
/// `zdiff3` conflict style. Nothing in this tree parses or draws that form. If
/// support for it is ever added it must be added here first: a parser that grew
/// a fourth marker while the two painters still drew three would show a user a
/// conflict block with no base section and no way to tell why.
pub const CONFLICT_MARKER_OURS: &str = "<<<<<<<";
/// The divider between the two sides of one conflict block.
pub const CONFLICT_MARKER_SEPARATOR: &str = "=======";
/// The marker that closes a conflict block, carrying the other side's label.
pub const CONFLICT_MARKER_THEIRS: &str = ">>>>>>>";

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

/// Default number of Unicode characters retained from a commit reference.
pub const SHORT_COMMIT_REF_CHARS: usize = 7;

/// Format a commit reference for compact display without splitting a Unicode
/// scalar value. References at or below the default length pass through intact.
pub fn short_commit_ref(reference: &str) -> String {
    reference.chars().take(SHORT_COMMIT_REF_CHARS).collect()
}

/// The subject a MESSAGE will read as — the first line, which is git's own definition
/// of a commit's subject and this rule's. One definition, because it is a *rule*: a
/// caller that re-derives it can disagree with whatever compares subjects.
pub fn subject_of_message(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}

impl Commit {
    /// A commit's subject, read the way every surface reads it: the first line
    /// of the message.
    pub fn subject(&self) -> &str {
        subject_of_message(&self.message)
    }
}

/// A path in git's spelling: forward slashes throughout, so a path string means the
/// same thing on every platform.
///
/// This is the **plain** rewrite — separators become `/` and nothing else moves:
/// `a//b` stays `a//b`, and a filename that genuinely contains a `\` is rewritten too.
/// The two places wanting the *other* answer keep their own spelling: `diff_engine`'s
/// patch headers normalise through `Path::components`, and the sidebar's collapse keys
/// add a project-prefix strip on top.
pub fn forward_slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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

/// The **file-level clash kind** of one conflicted path — which sides of the
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
    /// Each conflicted path's file-level clash kind, in the same order as
    /// [`RootStatus::conflicted`]. Filled by the engine's status parse from the
    /// same `u` line that fills `conflicted` — one push site, so the two
    /// cannot drift — and empty from engines (or seeded fakes) that do not
    /// report kinds; [`RootStatus::conflict_kind`] then answers `None`, and a
    /// surface that cannot name the kind must not invent one.
    #[serde(default)]
    pub conflict_kinds: Vec<(PathBuf, ConflictKind)>,
}

impl RootStatus {
    /// The file-level clash kind of one conflicted path, as the engine's
    /// status parse classified it. `None` when the path is not conflicted or
    /// the engine does not report kinds.
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

/// The three index versions of a conflicted path, as the **Git engine** answers
/// them for one path — the glossary's **Conflict**. Named for the sides, not for
/// git's stage numbering, and the engine that reads the index is the one that
/// knows which stage is which.
///
/// A side is `None` when the index holds no version of it: an add/add conflict
/// has no base, and a one-sided delete has no side. That is not the same answer
/// as an empty one, and a resolver that conflates them merges against a phantom.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConflictVersions {
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
}

/// IDE patch store entry (shelve).
///
/// **A shelf belongs to one root**, and the root is part of the record rather
/// than a fact about where the file was written: two roots of one project can
/// hold shelves with the same name, and an unshelve of one of them moves work in
/// exactly one repository. A shelf without a root is a name that the Shelf pane
/// could only ever list globally, which is the multi-root collapse the parity
/// programme forbids.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shelf {
    pub name: String,
    pub changes: Vec<Change>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The root whose changes this shelf holds and whose git stash carries its
    /// content.
    pub root: RootId,
    /// Where this shelf's **content** actually lives — see [`ShelfSource`].
    ///
    /// A row written before an imported patch could be parked has no key for this and
    /// reads back as [`ShelfSource::Stash`], which is exactly what every such row is;
    /// the store's own mirror (`StoredShelf`) carries that default so this field stays
    /// required in memory.
    #[serde(default)]
    pub source: ShelfSource,
}

/// Where one shelf's parked content lives, and therefore what `Unshelve` has to do.
///
/// Two mechanisms, because the surface has two sources of work. **A shelved change is
/// content git already holds**: `shelve` parks the working tree with
/// `git stash push -m <shelf name>`, so the record only has to name it and the stash
/// list is the payload. **An imported patch is content no repository holds** — the
/// reference's whole point is that the patch moves to the shelf **before being
/// applied** — so there is nothing to stash: stashing it would mean applying it first,
/// which is the act the developer deferred. Its payload is therefore the patch text,
/// written by the shelf store beside the index.
///
/// The split is not cosmetic: it decides which side of the boundary the verb touches.
/// `Stash` goes to git (`stash apply`/`pop`, found **by name**), `Patch` goes to the
/// apply engine, and both keep the row's [`Shelf::root`] as the only tree they may
/// touch, because a parked patch put into the wrong repository moves work in the wrong
/// tree.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShelfSource {
    /// The content is this repository's stash entry whose message names the shelf.
    #[default]
    Stash,
    /// The content is the imported patch's own bytes, in the shelf payload file
    /// [`ShelfSource::file_name`] names.
    Patch { file: String },
}

impl ShelfSource {
    /// The payload file an imported shelf reads and writes, or `None` for a row whose
    /// content is git's stash entry. One accessor for both verb halves so neither can
    /// guess the other's spelling.
    pub fn file_name(&self) -> Option<&str> {
        match self {
            Self::Stash => None,
            Self::Patch { file } => Some(file),
        }
    }

    /// Whether this row's content is a stash entry — the spelling legacy rows arrive
    /// as, and the case the store keeps writing **without** a key for.
    pub fn is_stash(&self) -> bool {
        matches!(self, Self::Stash)
    }
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
    /// Whether the worktree has uncommitted changes (issue 14 status column).
    /// `None` while the per-worktree dirty probe has not run: the worktree
    /// list is decoupled from the probe (ticket 01), so a freshly listed
    /// worktree's dirtiness is unknown, not clean.
    pub dirty: Option<bool>,
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
    Min15,
    Min30,
    #[default]
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

/// Which **Git engine** adapter performs git operations. There are two, and
/// they differ in how much is answered in-process — not in whether libgit2 can
/// run the whole job, which no adapter does. `InProcessReads` is the default
/// and the spelling `Auto` and `Libgit2` in an older `state.ron` still load
/// onto: both of those built this same object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GitBackend {
    /// Shell out to the system `git` binary for every operation.
    Cli,
    /// Answers the reads `git2` has in-process and delegates the rest to the
    /// CLI adapter. Not a libgit2-only path: it holds no fallback *policy*, and
    /// most of it is CLI delegation.
    #[default]
    #[serde(alias = "Auto")]
    #[serde(alias = "Libgit2")]
    InProcessReads,
}

/// Project + per-root settings, serialized under `.turbogit/`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsSettings {
    /// Path to the git executable (empty = resolve from PATH).
    pub git_executable: String,
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
    /// Restore workspace context on branch switch.
    pub restore_workspace: bool,
    /// Highlight modified lines in the gutter.
    pub gutter_markers: bool,
    /// Date format for the log.
    pub date_format: DateFormat,
    /// Which **Git engine** adapter runs — see [`GitBackend`].
    #[serde(default)]
    pub backend: GitBackend,
    /// Compute unified diffs in-process with `similar` instead of
    /// post-processing CLI diff text (Phase L1). `false` rolls back to the
    /// CLI-produced text path.
    #[serde(default = "default_true")]
    pub in_process_diffs: bool,
    /// Update silently when a push of the current branch is
    /// rejected, using `update_method` — set by the Push dialog's
    /// "Remember the update method choice" checkbox, so the opt-in is a
    /// settings write-back the user can reverse in settings, never a per-push
    /// accident. Off by default: a rejection asks.
    #[serde(default)]
    pub auto_update_on_push_rejected: bool,
    /// `When patch is created:` — what a create route does with the file it wrote.
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
            backend: GitBackend::default(),
            in_process_diffs: true,
            auto_update_on_push_rejected: false,
            when_patch_is_created: PatchCreatedPolicy::default(),
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogOpts {
    pub max_count: Option<usize>,
    /// Entries to discard from the *front* of the listing before the page is
    /// cut — the position half of log paging. The walk still starts at HEAD,
    /// so the union of consecutive pages is a prefix of the uncapped listing
    /// and can never have holes behind a merge's second parent. `None` (and
    /// `Some(0)`) leave the listing untouched.
    pub skip: Option<usize>,
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

/// The `Ignore Differences` policy: which whitespace differences count as a
/// change at all.
///
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
    /// The default: white space is significant and every difference shows.
    #[default]
    None,
    /// Leading and trailing whitespace of a line is insignificant.
    TrimWhitespace,
    /// Whitespace is insignificant wherever it sits in a line.
    IgnoreWhitespace,
    /// Also: whitespace-only added/removed lines, and lines split or joined
    /// without changing their non-whitespace content.
    IgnoreWhitespaceAndEmptyLines,
    /// Also: changes confined to import statements. Whitespace inside a string
    /// literal stays significant.
    IgnoreImportsAndFormatting,
}

impl DiffWhitespace {
    /// Menu order, reference spelling: the ladder above is the ladder shown.
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
///
/// [`Self::Lines`] highlights each changed row as one block;
/// [`Self::Words`] and [`Self::Characters`] highlight only the differing word or
/// character runs inside it; [`Self::SplitChanges`] cuts a multi-line change into
/// the separate changes it is made of, so a line that merely moved inside the
/// change is not highlighted at all; [`Self::None`] highlights nothing, which is
/// the value the reference recommends for a heavily modified file — where
/// highlighting adds difficulty instead of removing it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiffHighlighting {
    Words,
    /// TurboGit's pre-existing rendering: the whole changed row.
    #[default]
    Lines,
    SplitChanges,
    Characters,
    None,
}

impl DiffHighlighting {
    /// Menu order, reference spelling.
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

/// The diff viewer's two view policies, carried together: they are decided at the
/// same moment (one menu), they invalidate the same cached display model, and
/// neither changes a byte of what git is asked or what staging applies.
///
/// `Default` is the shipped posture: the reference's `None` for whitespace, and
/// TurboGit's own pre-existing whole-row highlighting. The reference documents no
/// default for either, so highlighting keeps the behaviour the surface had before
/// the menu existed rather than inventing one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DiffPolicies {
    pub whitespace: DiffWhitespace,
    pub highlighting: DiffHighlighting,
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

/// One file's line counts in a change, as the **Git engine** answers them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub path: PathBuf,
    /// Lines added. `None` is git's `-`: a **Binary change** has no line count,
    /// which is a different answer from a count of zero. This is the one place
    /// that decision lives; callers that want a number read `unwrap_or(0)`.
    pub insertions: Option<usize>,
    /// Lines removed, with the same `None`-means-binary rule.
    pub deletions: Option<usize>,
}

/// How much a change brings, one row per file touched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeStats {
    pub files: Vec<FileStat>,
}

impl ChangeStats {
    /// Files the change touches. A **Binary change** counts.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Lines added across every file; the ones git reported as `-` add nothing.
    pub fn insertions(&self) -> usize {
        self.files.iter().filter_map(|f| f.insertions).sum()
    }

    /// Lines removed across every file, same rule.
    pub fn deletions(&self) -> usize {
        self.files.iter().filter_map(|f| f.deletions).sum()
    }
}

/// The question the engine's `change_stats` read answers. One read serves both
/// asks because both are the same question about a different pair.
///
/// Rename detection is inherited, not pinned: git's own defaults decide whether
/// a copied file's stat is the same question as a modified file's, and the
/// engine follows whatever the repository configures — the same policy the
/// glossary's **Rename header** entry states.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChangeQuestion {
    /// What one commit introduced against its first parent (a parentless
    /// commit against the empty tree) — the log's per-file stats.
    Commit { commit: String },
    /// What merging `target` into the current `HEAD` would bring in: the merge
    /// base to `target`. The three-dot spelling belongs to the engine.
    MergeIntoHead { target: String },
}

/// One line of a patch hunk's body, with what git's prefix character meant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchLine {
    pub kind: PatchLineKind,
    /// The line's text, prefix character removed.
    pub text: String,
    /// git's `\ No newline at end of file` marker, attached to the line it
    /// qualifies rather than left as a line of its own.
    pub no_newline: bool,
}

/// What a patch body line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchLineKind {
    Context,
    Added,
    Removed,
}

/// One hunk: where it lands on each side, and its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchHunk {
    /// 1-based first line on the old side.
    pub old_start: usize,
    pub old_count: usize,
    /// 1-based first line on the new side.
    pub new_start: usize,
    pub new_count: usize,
    /// The section heading git appends after the second `@@` — the enclosing
    /// function name. Carried, because re-emitting a hunk without it produces
    /// text that differs from git's byte for byte, and because the viewer shows
    /// it. `None` when git found no heading.
    pub heading: Option<String>,
    pub lines: Vec<PatchLine>,
}

/// One metadata line of a file section, kept in the order git wrote it.
///
/// Each variant is a fact the viewer and the staging path ask about — not a
/// prefix to test. The order is part of the value because the diff view paints
/// these lines as section metadata, in git's sequence, and a **Rename header**
/// leads the content it describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchHeaderLine {
    /// `index <old>..<new>[ <mode>]` — the blob pair git computed.
    Index {
        old: String,
        new: String,
        mode: Option<String>,
    },
    /// `similarity index N%`
    Similarity { percent: u8 },
    /// `rename from <path>`
    RenameFrom { path: String },
    /// `rename to <path>`
    RenameTo { path: String },
    /// `new file mode <mode>`
    NewFile { mode: String },
    /// `deleted file mode <mode>`
    DeletedFile { mode: String },
    /// `old mode <mode>`
    OldMode { mode: String },
    /// `new mode <mode>`
    NewMode { mode: String },
    /// `Binary files a/x and b/y differ` — a **Binary change**. Carries no
    /// paths of its own: the section already names both sides, and git's
    /// marker is those two paths in one fixed sentence.
    Binary,
    /// The `--- <old>` / `+++ <new>` source pair, each side git's own form
    /// (`a/path`, `b/path`, or `/dev/null` for the side that does not exist)
    /// with quoting already resolved. Which side is `/dev/null` is what makes a
    /// composed patch appliable, so the pair is one fact: a patch's two sides
    /// are one question.
    Sources { old: String, new: String },
}

/// One file's section of a patch.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PatchFile {
    /// Repository-relative, git's `a/` prefix removed and its quoted form
    /// unescaped. Never split out of a header line by a caller.
    pub old_path: String,
    /// Repository-relative, `b/` removed.
    pub new_path: String,
    /// This is git's **combined** view of an unmerged path (`diff --cc`), which
    /// names one path and compares it against every parent at once. Its hunks
    /// keep the first parent's range and the new side's, because the value has
    /// two sides; the staging verbs refuse a conflicted file anyway, so the
    /// reading is for the pane, not for `git apply`.
    pub combined: bool,
    /// The section's metadata lines, in git's order.
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

    /// The rename similarity percent, when git detected a rename. Follows
    /// git's own defaults, unpinned (CONTEXT.md **Rename header**).
    pub fn similarity(&self) -> Option<u8> {
        self.first(|l| match l {
            PatchHeaderLine::Similarity { percent } => Some(*percent),
            _ => None,
        })
    }

    /// A `rename from`/`rename to` pair is present. A rename with no hunks is
    /// expressible here, which the text format can only say by absence.
    pub fn renamed(&self) -> bool {
        self.has(|l| matches!(l, PatchHeaderLine::RenameFrom { .. }))
    }

    pub fn new_file(&self) -> bool {
        self.has(|l| matches!(l, PatchHeaderLine::NewFile { .. }))
    }

    pub fn deleted_file(&self) -> bool {
        self.has(|l| matches!(l, PatchHeaderLine::DeletedFile { .. }))
    }

    /// A **Binary change**: the section describes it and carries no hunks.
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

    /// A file whose only change is its mode: no content hunks, not binary, not
    /// created or deleted, and not the no-content case of a rename.
    pub fn mode_only(&self) -> bool {
        self.hunks.is_empty()
            && !self.binary()
            && !self.new_file()
            && !self.deleted_file()
            && !self.renamed()
            && (self.old_mode().is_some() || self.new_mode().is_some())
    }
}

/// Which change a **created** patch describes.
///
/// The reference reaches one `Patch File Settings` dialog from three sources, so
/// the source is one value rather than three dialog shapes: uncommitted changes,
/// a revision in the Log, and one file's revision in a file history. Named
/// *origin* and not *source* because the patch surface already uses `PatchSource`
/// for the other half of the pair — how a patch **arrives** to be applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchOrigin {
    /// The root's tracked uncommitted changes: all of them when `paths` is empty,
    /// or the one file / the selection the verb was reached on.
    LocalChanges { paths: Vec<PathBuf> },
    /// One commit against its first parent — the commit's own change and none of
    /// its ancestors'.
    Commit { id: String },
    /// One file's change in one revision: what a row of a **file history** is.
    FileRevision { commit: String, path: PathBuf },
}

impl PatchOrigin {
    /// Whether this origin needs the comment step, where the comment becomes the
    /// file name. A commit and a file revision already name themselves — a commit
    /// has a subject — and only uncommitted changes have no name to be asked for.
    pub fn needs_comment(&self) -> bool {
        matches!(self, Self::LocalChanges { .. })
    }
}

/// `Patch File Settings`' last field: how the patch file's bytes are written.
///
/// What the application does with a patch **after** a create route has written it.
///
/// The reference's `When patch is created:` field — `Ask / Show in {0} / Do nothing`
/// — and the fourth instance of that policy shape in this product. The captured
/// default is **Ask**, which is the whole reason the field exists: today a created
/// patch always reports the one way the build happened to implement, so the setting
/// would be a decoration if `Ask` were not the arm that leaves the decision with the
/// developer.
///
/// `Show in {0}` names the operating system's file manager, and this build has no
/// reveal seam for it (`rfd` is wired for folders and for patch paths, never for
/// `show`). The arm is therefore not dropped: it takes the file to the surface that
/// reads patch files, which is where a patch's own content is visible here. That is
/// a recorded divergence, not a renamed `Do nothing` — the two arms stay distinct.
///
/// The clipboard route is outside this policy: it creates **no file**, so there is no
/// artifact to ask about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PatchCreatedPolicy {
    /// Name the created file and wait for the developer to say what to do with it.
    /// The captured default.
    #[default]
    Ask,
    /// Take the created patch to the surface that shows patch content.
    ShowInFiles,
    /// Report nothing in the moment. The durable activity feed still gets the fact.
    DoNothing,
}

impl PatchCreatedPolicy {
    /// Every arm, in the reference's order, each with the label the settings page
    /// paints and the palette route uses.
    pub const ALL: [Self; 3] = [Self::Ask, Self::ShowInFiles, Self::DoNothing];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::ShowInFiles => "Show in files",
            Self::DoNothing => "Do nothing",
        }
    }
}

/// Three members and no more, because each is a rule the surface can actually
/// apply to bytes: `Utf8` writes git's own output **verbatim** — it is not a
/// re-encode, and that is the point. A patch's body carries its file's bytes,
/// which git does not guarantee are text in any encoding, so the default must be
/// the one that cannot corrupt them. The other two can only be honoured for a
/// patch that *is* valid UTF-8, and say so when it is not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PatchEncoding {
    /// The bytes git produced, unchanged. The shipped default.
    #[default]
    Utf8,
    /// `Utf8` with a leading byte-order mark, which is what a Windows editor
    /// needs to read the patch as UTF-8 at all.
    Utf8WithBom,
    /// ISO-8859-1: one byte per code point, the single-byte legacy case.
    Latin1,
}

impl PatchEncoding {
    /// Every encoding the field offers, in drop-down order.
    pub const ALL: [Self; 3] = [Self::Utf8, Self::Utf8WithBom, Self::Latin1];

    /// The label the drop-down paints. Spelled as the reference's list spells
    /// them, and held here because the field's list, its default and its
    /// transcode rule must not be three places that can disagree.
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8WithBom => "UTF-8 with BOM",
            Self::Latin1 => "ISO-8859-1",
        }
    }
}

/// A whole patch: one section per file. What the **Git engine** answers where
/// unified-diff text used to be the interface.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Patch {
    pub files: Vec<PatchFile>,
}

impl Patch {
    /// An empty diff is an answer, not a missing one (spec R2).
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Total hunks across every file — the count whole-**File** staging gates on.
    pub fn hunk_count(&self) -> usize {
        self.files.iter().map(|f| f.hunks.len()).sum()
    }
}

// --- saying it back in git's syntax ------------------------------------------
//
// The engine's adapters read git's unified diff into the value; only two places
// still need git's *text*, and both are the value's own business rather than a
// layer's: the index is fed bytes by `git apply`, and the viewer paints header
// lines as labels. Writing them here keeps the format's rules — when a count is
// omitted, when a path is quoted, which side is `/dev/null` — in one file
// instead of in every caller that ever had to arrange a string to look like a
// diff (`ADR-0022`).

impl PatchLine {
    /// git's own marker text, which the diff viewer paints verbatim.
    pub const NO_NEWLINE_MARKER: &'static str = "\\ No newline at end of file";
}

impl PatchHunk {
    /// `@@ -<old> +<new> @@[ <heading>]`, with a count of one omitted the way
    /// `git diff` omits it.
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
    /// The section's opening line: git's two-path form, or the one name a
    /// **combined** section carries.
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
    /// This metadata line in git's syntax. The source pair is two lines and
    /// every other header is one; `file` supplies the paths that git's binary
    /// marker states instead of storing.
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
                // git spells a side that does not exist `/dev/null` here too;
                // which side that is, the section's own mode header already says.
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

/// A hunk span as git spells it: `<start>` when the count is one, `<start>,0`
/// when a side is empty.
fn span_text(start: usize, count: usize) -> String {
    if count == 1 {
        format!("{start}")
    } else {
        format!("{start},{count}")
    }
}

/// One side of a `diff --git` line: the `a/` or `b/` prefix joined to the
/// repository-relative path, then quoted as one name.
fn git_side(prefix: &str, path: &str) -> String {
    quote_side(&format!("{prefix}{path}"))
}

/// One side of the source pair, which git spells with its prefix already on it
/// — or names nothing at all.
fn quote_side(side: &str) -> String {
    if side == "/dev/null" {
        return side.to_string();
    }
    quote_c(side, QuoteTrigger::GitSpelling)
}

impl std::fmt::Display for Patch {
    /// The whole patch as `git apply` reads it.
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
    /// The row marks that the refs touching this replay follow the rewritten
    /// commits. Git's todo has no per-commit slot for that — the engine maps
    /// the row to the command-level `--update-refs` — but the plan can carry
    /// it per row, and the editor offers it like any other verb.
    UpdateRef,
    /// The parser's fallback for a REBASE-TODO verb it does not recognise: a
    /// row can exist that the plan editor did not create, so it survives with
    /// this action and renders as `Unknown`. It must never reach git as
    /// another verb — coercing it to `pick` would produce a valid-looking
    /// wrong plan — so the engine refuses a plan that carries one.
    Unknown,
}

/// A row of the interactive rebase plan.
///
/// `message` is the replacement commit message a [`RebaseAction::Reword`] row
/// carries, and it is the reason a reword is not a no-op: git's own rebase todo
/// has nowhere to put a message, so it rides on the plan row instead and the
/// engine hands it to git's editor at execution (ADR-0025). `None` on every
/// other row, and `parse_todo` can never produce one — rendering a plan to todo
/// text drops the message deliberately, so the editor's buffer stays a list of
/// verbs rather than becoming a message store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebasePlanEntry {
    pub action: RebaseAction,
    pub commit: CommitId,
    pub subject: String,
    pub message: Option<String>,
}
