//! The push session: one row per registered repository, each row holding the
//! target it prints and the consent it pushes on. Nothing here knows about
//! painting, and nothing here reads git — the rows are handed the commits some
//! caller listed, and the only way out of this module is the plan `payload`
//! builds from what the rows say.

use turbogit_domain::model::{Branch, BranchKind, Commit, CommitId, Root, RootId};
use turbogit_engine_api::PushTags;
use turbogit_services::sync_service::{
    PushOptions, PushTarget, repository_is_empty, seed_push_target, tracked_upstream,
};

use crate::state::PushPreview;

/// The right pane's subject. Focus never enters the payload — it is what the
/// commit row means in place of a checkbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Focus {
    AllRoots,
    Root(RootId),
    Commit(RootId, CommitId),
}

/// One of the row's two editable target halves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetField {
    /// What this root resolves to when nobody has touched it.
    Resolved(String),
    /// Mid-edit. `seed` is what the field started from, so a draft can be
    /// stated and abandoned without the row losing its own answer.
    Editing { value: String, seed: String },
    /// Nothing to state and nothing to edit yet: a remote the repository does
    /// not have.
    Missing,
}

impl TargetField {
    /// The name a git argument would get, `None` when there is none to give. A
    /// field mid-keystroke is still the row's answer, whitespace and all.
    pub fn value(&self) -> Option<&str> {
        let text = match self {
            Self::Resolved(value) | Self::Editing { value, .. } => value.as_str(),
            Self::Missing => "",
        };
        let trimmed = text.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

/// The inline `Define remote` sub-form, open only on the row that asked for it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddRemoteForm {
    pub name: String,
    pub url: String,
}

/// Why this row cannot be pushed, or cannot be trusted about what it would
/// push. The reference names these states instead of hiding the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Obstacle {
    NoRemote,
    EmptyRepository,
    Detached,
    /// A first push has no upstream to be ahead of, so its commits cannot be
    /// listed — and it is still the repository most in need of a push.
    UnlistedFirstPush,
}

impl Obstacle {
    /// Whether this stops the push rather than only the listing. A row can be
    /// consented *and* unpushable, and which half of that a row is comes from
    /// here, not from the box.
    pub fn blocks_push(self) -> bool {
        !matches!(self, Self::UnlistedFirstPush)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NoRemote => "no remote to push to",
            Self::EmptyRepository => "Empty repository",
            Self::Detached => "detached HEAD",
            Self::UnlistedFirstPush => "first push — its commits cannot be listed",
        }
    }
}

/// One commit of the frozen listing, flattened to what a row prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedCommit {
    pub id: CommitId,
    pub subject: String,
    pub author: String,
    pub time: i64,
    /// The reference's asterisk: the author is not the committer.
    pub author_differs: bool,
}

impl ListedCommit {
    pub fn from_commit(commit: &Commit) -> Self {
        Self {
            id: commit.id.clone(),
            subject: commit.subject().to_string(),
            author: commit.author.name.clone(),
            time: commit.time,
            author_differs: commit.author.name != commit.committer.name
                || commit.author.email != commit.committer.email,
        }
    }
}

/// One repository. Registration order owns the rows; no click ever reorders
/// them, because a list that moves under a checkbox is the ambiguity this
/// dialog is being rebuilt away from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushRow {
    pub id: RootId,
    pub name: String,
    /// `None` is detached or unborn — a repository with no name to state.
    pub local: Option<String>,
    pub remote: TargetField,
    pub branch: TargetField,
    /// An argument of the state the row prints, not of the batch.
    pub set_upstream: bool,
    /// Newest-first, from the listing taken when the dialog opened.
    pub commits: Vec<ListedCommit>,
    /// The only push filter there is. Stored rather than derived because
    /// "consented and unpushable" is a legal state the dialog has to paint: a
    /// protected row is consented and refuses only force, a first-push row is
    /// consented and cannot list its commits.
    pub consented: bool,
    pub expanded: bool,
    /// The listing's full count, kept apart from `commits` so a list the row
    /// shows truncated still answers the staleness question.
    pub listed_total: usize,
    pub add_remote: Option<AddRemoteForm>,
    pub error: Option<String>,
    pub obstacle: Option<Obstacle>,
}

impl PushRow {
    /// The push this row would execute, read off the row's own fields and never
    /// re-derived from HEAD: the branch-menu verb seeds a branch that is not the
    /// current one, and a re-derivation silently drops it.
    pub fn target(&self) -> PushTarget {
        PushTarget {
            root: self.id.clone(),
            remote: self.remote.value().unwrap_or_default().to_string(),
            branch: self.branch.value().unwrap_or_default().to_string(),
            set_upstream: self.set_upstream,
        }
    }

    /// The row's one consent transition. Ticking a repository reveals the
    /// commits it is consenting to, so the two are written together rather than
    /// left as a rule a painter has to remember.
    pub fn toggle_consent(&mut self) {
        self.consented = !self.consented;
        self.expanded = self.expanded || self.consented;
    }
}

/// What the rows add up to: the batch git receives, plus the consented rows it
/// cannot receive, named.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushPlan {
    /// In registration order.
    pub targets: Vec<PushTarget>,
    /// Consented rows whose obstacle blocks the push. Carried, not dropped: a
    /// plan that quietly left one out would put the dialog back to reporting a
    /// push of more than the user ticked.
    pub blocked: Vec<(RootId, Obstacle)>,
    pub force: bool,
    pub tags: PushTags,
    pub no_verify: bool,
}

impl PushPlan {
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// The batch flags, in the shape the service takes. `set_upstream` is not
    /// here because it is not a batch fact. The tags mode is carried whole:
    /// `All` is `--tags` and `Follow` is `--follow-tags`, and collapsing them
    /// into one yes-or-no would put the lying control back.
    pub fn options(&self) -> PushOptions {
        PushOptions {
            force: self.force,
            tags: self.tags,
            no_verify: self.no_verify,
        }
    }
}

/// Everything one opening of the Push dialog knows. Replaced whole on open, so
/// an invocation cannot inherit a tick or a draft target from the last one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushSession {
    /// Bumped by `open`, never reused. It salts the row widgets' egui ids, so a
    /// previous invocation's focus and edit state belong to widgets that no
    /// longer exist.
    pub generation: u64,
    pub rows: Vec<PushRow>,
    pub focus: Option<Focus>,
    /// The dialog's repo selector scope. `None` is every row, which is what a
    /// directly constructed session means; the Push dialog sets it from the
    /// shared selector each frame.
    pub scope: Option<Vec<RootId>>,
    pub editing_all: bool,
    pub preview: Option<PushPreview>,
    pub force: bool,
    pub tags: PushTags,
    pub no_verify: bool,
}

impl PushSession {
    /// The only way to hold a session. No field is read from the previous one.
    pub fn open(generation: u64, rows: Vec<PushRow>, focus: Option<Focus>) -> Self {
        Self {
            generation,
            rows,
            focus,
            scope: None,
            editing_all: false,
            preview: None,
            force: false,
            tags: PushTags::default(),
            no_verify: false,
        }
    }

    /// Whether the selector has this row in play.
    pub fn in_scope(&self, id: &RootId) -> bool {
        self.scope.as_ref().is_none_or(|scope| scope.contains(id))
    }

    /// The only path from row state to git.
    pub fn payload(&self) -> PushPlan {
        let mut plan = PushPlan {
            force: self.force,
            tags: self.tags,
            no_verify: self.no_verify,
            ..PushPlan::default()
        };
        for row in self
            .rows
            .iter()
            .filter(|row| row.consented && self.in_scope(&row.id))
        {
            match row.obstacle {
                Some(obstacle) if obstacle.blocks_push() => {
                    plan.blocked.push((row.id.clone(), obstacle));
                }
                _ => plan.targets.push(row.target()),
            }
        }
        plan
    }

    /// Rows whose listed count disagrees with the ahead count cached since the
    /// dialog opened. The comparison is by value — never by a generation stamp,
    /// which ADR-0021 rejects by name — and only consented rows are asked, since
    /// a row that pushes nothing cannot mislead the button. A row with no cached
    /// count is not stale: nothing has been read for it to disagree with.
    pub fn stale_rows(&self, ahead: &impl Fn(&RootId) -> Option<usize>) -> Vec<RootId> {
        self.rows
            .iter()
            .filter(|row| row.consented && self.in_scope(&row.id))
            .filter(|row| matches!(ahead(&row.id), Some(count) if count != row.listed_total))
            .map(|row| row.id.clone())
            .collect()
    }

    pub fn row(&self, id: &RootId) -> Option<&PushRow> {
        self.rows.iter().find(|row| &row.id == id)
    }
}

/// `local → remote : branch`, the reference's target line. No shared formatter
/// exists for this, and the row prints it for every repository whether or not
/// it is consented.
pub fn target_line(local: Option<&str>, remote: &str, branch: &str) -> String {
    let local = local.unwrap_or("detached");
    let remote = if remote.trim().is_empty() {
        "no remote"
    } else {
        remote
    };
    let branch = if branch.trim().is_empty() {
        "—"
    } else {
        branch
    };
    format!("{local} → {remote} : {branch}")
}

/// The row a root is born with.
///
/// `branch` is the branch the invoking action pointed the row at, `None` for
/// HEAD's own; the two resolve differently and the branch-menu verb is only
/// honest if its branch survives the trip to git. `listing` is what the caller
/// read as this row's outgoing commits, already joined against the log.
pub fn build_row(
    root: &Root,
    consented: bool,
    branch: Option<&str>,
    listing: Result<Vec<Commit>, String>,
) -> PushRow {
    let seed = seed_push_target(root);
    // A branch that is not HEAD brings its own upstream, and so its own remote
    // and its own answer about recording one: HEAD's tracking config would point
    // the verb at a branch nobody clicked.
    let tracked = match branch {
        None => tracked_upstream(root).cloned(),
        Some(name) => local_branch(root, name).and_then(|b| b.tracking.clone()),
    };
    let remote_name = match branch {
        None => seed.remote,
        Some(_) => tracked
            .as_ref()
            .map(|u| u.remote.clone())
            .or_else(|| root.remotes.first().map(|r| r.name.clone()))
            .unwrap_or(seed.remote),
    };
    let branch_name = branch.unwrap_or(&seed.branch).to_string();
    let set_upstream = tracked.is_none();

    let has_remotes = !root.remotes.is_empty();
    let obstacle = if repository_is_empty(root) {
        Some(Obstacle::EmptyRepository)
    } else if !has_remotes {
        Some(Obstacle::NoRemote)
    } else if branch.is_none() && root.current_branch.is_none() {
        Some(Obstacle::Detached)
    } else if !set_upstream {
        None
    } else {
        // The range has no upstream to be ahead of, so the listing this row was
        // handed is empty by construction rather than by the repository being
        // up to date.
        Some(Obstacle::UnlistedFirstPush)
    };

    let (commits, error) = match listing {
        Ok(commits) => (
            commits.iter().map(ListedCommit::from_commit).collect(),
            None,
        ),
        Err(reason) => (Vec::new(), Some(reason)),
    };
    let listed_total = commits.len();

    PushRow {
        id: root.id.clone(),
        name: root.id.name(),
        local: root.current_branch.clone(),
        remote: if has_remotes {
            TargetField::Resolved(remote_name)
        } else {
            TargetField::Missing
        },
        branch: if branch_name.is_empty() {
            TargetField::Missing
        } else {
            TargetField::Resolved(branch_name)
        },
        set_upstream,
        commits,
        consented,
        expanded: consented,
        listed_total,
        add_remote: None,
        error,
        obstacle,
    }
}

fn local_branch<'a>(root: &'a Root, name: &str) -> Option<&'a Branch> {
    root.branches
        .iter()
        .find(|b| b.name == name && b.kind == BranchKind::Local)
}
