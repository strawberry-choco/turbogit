//! Application state: owns the Git engine (the [`GitExecutor`] seam), the
//! multi-root model, canonical settings, the project directory, the event
//! channel, and all UI-only ephemeral state. The UI reads from here and never
//! calls git directly; long ops are dispatched to worker threads via
//! [`AppState::run_git`].
use crate::events::AppEvent;
use crate::granular;
use crate::root_caches::{Affected, RootCaches};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine::build_executor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::branch_service;
use turbogit_services::bulk_ops::{self, BulkOp, BulkPlan, Preflight};
use turbogit_services::bulk_run;
use turbogit_services::changes;
use turbogit_services::commit_across;

/// One root's outgoing commits for the push dialog tree (issue #20).
#[derive(Clone)]
pub struct OutgoingRoot {
    pub id: RootId,
    pub name: String,
    /// Enriched commits newest-first, or the engine error string.
    pub commits: Result<Vec<Commit>, String>,
}

/// Aggregated dry-run preview for the push dialog (issue #25 extends #21's
/// single-root report): the "N commits → R remotes · X refs · Y rejected"
/// summary plus one verbatim git report per root in scope.
#[derive(Clone)]
pub struct PushPreview {
    /// Total outgoing commits in scope.
    pub commits: usize,
    /// Distinct remotes the per-root dry-runs targeted.
    pub remotes: usize,
    /// Ref-update lines across the verbatim reports.
    pub refs: usize,
    /// Roots whose dry-run git rejected.
    pub rejected: usize,
    /// (root display name, verbatim report on accept, stderr on reject).
    pub reports: Vec<(String, Result<String, String>)>,
}

/// Maximum recent custom commands kept per workspace (issue 13).
pub const MAX_RECENT_CUSTOM_COMMANDS: usize = 8;

/// Commits fetched per log page (issue 17): the initial log load takes one
/// page and `load_more_log` widens the fetch by another page, so a huge
/// history never blocks the first paint.
pub const LOG_PAGE_SIZE: usize = 200;

/// Persistent input fields for the modal dialogs (kept across redraws).
#[derive(Default)]
pub struct DialogState {
    // Push
    pub push_remote: String,
    pub push_branch: String,
    pub force_push: bool,
    /// The PUSH SCOPE segmented control (issue #25, screen 10): which roots
    /// the dialog pushes. Supersedes the old "Push current branch only"
    /// checkbox; `ThisRepo` keeps that path's explicit Remote/Branch fields.
    pub push_scope: turbogit_services::sync_service::PushScope,
    /// Remediation exclusions (issue #25): roots the user removed from scope
    /// via the protected-branch banner. Cleared when the scope changes or the
    /// dialog closes.
    pub push_scope_excluded: HashSet<RootId>,
    /// Outgoing-commit tree snapshot built once when the dialog opens.
    pub push_outgoing: Option<Vec<OutgoingRoot>>,
    /// Which root node the user clicked — filters the changed-files PREVIEW
    /// only, never the batch push scope (ADR-0006).
    pub push_preview_root: Option<RootId>,
    /// Verbatim `git push --dry-run` output captured by the Preview button
    /// (issue #21): `Ok(report)` when git accepted the push, `Err(git
    /// stderr)` when it rejected it. Rendered as-is, never paraphrased.
    /// Commits the user has currently ticked in the Push dialog's outgoing
    /// list (issue #24). On dialog open the snapshot builder fills this with
    /// every outgoing SHA, so an untouched dialog pushes everything ahead.
    /// Unchecking an older commit forces the dialog to fall back to a full
    /// push (see `push_dialog` for the suffix-only selection rule).
    pub push_selected_commits: Vec<String>,
    /// Last subset-push computation: which outgoing commits would actually
    /// land on the remote if Push were clicked right now. Issue #24.
    pub push_subset: turbogit_services::sync_service::SubsetPushState,
    /// `git push --tags` (issue #24).
    pub push_tags: bool,
    /// `git push --no-verify` (issue #24).
    pub push_no_verify: bool,
    /// `git push --set-upstream` (issue #24).
    pub push_set_upstream: bool,
    /// Aggregated `git push --dry-run` preview (issues #21/#25): the scope
    /// summary plus one verbatim report per root in scope.
    pub push_preview_output: Option<PushPreview>,
    // Merge
    pub merge_target: String,
    pub merge_no_ff: bool,
    pub merge_no_verify: bool,
    // Merge dialog upgrade (issue 28, screen 14)
    /// The STRATEGY segmented control: which mutually exclusive merge mode
    /// runs, mapped to executor flags by
    /// [`turbogit_services::integrate_service::merge_flags`].
    pub merge_strategy: turbogit_domain::model::MergeStrategy,
    /// Verify GPG signatures on the incoming commits
    /// (`--verify-signatures`).
    pub merge_verify_signatures: bool,
    /// Accept unrelated histories (`--allow-unrelated-histories`).
    pub merge_allow_unrelated: bool,
    /// Whether the source-branch picker's branch list is expanded.
    pub merge_source_picker_open: bool,
    /// Last computed pre-merge preview (the "Will create …" box), cached
    /// with [`Self::merge_preview_key`] so it recomputes only when the
    /// source branch or strategy changes.
    pub merge_preview: Option<turbogit_services::integrate_service::MergePreview>,
    /// The (source branch, strategy) pair the cached preview was computed for.
    pub merge_preview_key: Option<(String, turbogit_domain::model::MergeStrategy)>,
    // Rebase dialog upgrade (issue 29, screen 15)
    pub rebase_onto: String,
    /// The MODE segmented control: which rebase invocation runs, mapped by
    /// [`turbogit_services::integrate_service::rebase_mode_opts`].
    pub rebase_mode: turbogit_domain::model::RebaseMode,
    /// Whether the onto-branch picker's branch list is expanded.
    pub rebase_onto_picker_open: bool,
    /// Option rows layered on top of the mode (Autosquash mode forces the
    /// autosquash flag on regardless of the row).
    pub rebase_keep_empty: bool,
    pub rebase_update_refs: bool,
    pub rebase_autosquash: bool,
    /// Last computed commits-to-rebase list (the "N COMMITS TO REBASE"
    /// box), cached with [`Self::rebase_preview_key`] so it recomputes only
    /// when the onto branch changes.
    pub rebase_preview: Option<Vec<turbogit_domain::model::RebasePlanEntry>>,
    /// The onto branch the cached preview was computed for.
    pub rebase_preview_key: Option<String>,
    pub new_branch_name: String,
    pub new_branch_start: String,
    pub new_branch_checkout: bool,
    // Rename branch (issue 32, branches popup row action).
    pub rename_branch_root: Option<RootId>,
    pub rename_branch_name: String,
    pub rename_branch_new: String,
    // Compare branches (issue 32, branches popup row action + footer): the
    // two sides and the snapshot of commits on `compare_left` missing from
    // `compare_right`, captured once when the dialog opens (E9).
    pub compare_root: Option<RootId>,
    pub compare_left: String,
    pub compare_right: String,
    pub compare_commits: Vec<CommitId>,
    // Cherry-pick target picker (issue 15): the selected log commit awaiting
    // the target-branch choice.
    pub cherry_pick_commit: Option<String>,
    // Cherry-pick across repositories (issue 16): source root, the full
    // candidate list in application order (oldest first), the checked
    // commits, the checked target repos, the search text, the conflict
    // policy, the preview rail's focused commit with its loaded patch, and
    // the per-target prediction table for the current selection.
    pub cherry_source: Option<RootId>,
    /// The source log's candidate commits in application order (oldest
    /// first); carries subject/author so the UI's checklist and search never
    /// touch git.
    pub cherry_candidates: Vec<Commit>,
    pub cherry_commits: Vec<String>,
    pub cherry_targets: Vec<RootId>,
    pub cherry_search: String,
    pub cherry_stop_on_conflict: bool,
    pub cherry_focus: Option<String>,
    pub cherry_preview: Option<Result<String, String>>,
    /// Focused file of the patch preview rail (0-based, clamped at render).
    pub cherry_preview_file: usize,
    pub cherry_forecast: Option<Vec<turbogit_services::cherry_across::TargetForecast>>,
    // Tag
    pub tag_name: String,
    pub tag_msg: String,
    pub tag_push: bool,
    // Tag dialog upgrade (issue 31, screen 16)
    /// The TYPE segmented control: annotated tags carry a message, a tagger
    /// and the GPG option; lightweight skips all three.
    pub tag_type: TagType,
    /// The picked tag point as a commit-ish; empty = HEAD.
    pub tag_target: String,
    /// Whether the target picker's recent-commit list is expanded.
    pub tag_target_picker_open: bool,
    /// The picker's recent commits, fetched from the live repository the
    /// first time the picker opens; plain data so the list never touches git.
    pub tag_candidates: Option<Vec<Commit>>,
    /// The tagger identity field ("Name <email>"), annotated tags only.
    pub tag_tagger: String,
    /// "Sign with GPG key" (`-s`), annotated tags only.
    pub tag_sign: bool,
    /// The repository's existing tags, fetched once per dialog lifetime for
    /// the live duplicate check.
    pub tag_existing: Option<Vec<String>>,
    /// The configured signing key (`git config user.signingkey`), fetched
    /// once per dialog lifetime for the fingerprint chip next to the sign
    /// option.
    pub tag_signing_key: Option<String>,
    /// Whether [`Self::tag_signing_key`] has been fetched (distinguishes
    /// "no key configured" from "not looked up yet").
    pub tag_signing_key_fetched: bool,
    // Shelve / Stash
    pub shelve_name: String,
    pub stash_msg: String,
    pub stash_keep: bool,
    // New worktree (issue 14): relative-or-absolute path of the worktree
    // and the branch to create & check out in it.
    pub wt_path: String,
    pub wt_branch: String,
    // Interactive rebase plan (built on open)
    pub rebase_plan: Option<Vec<turbogit_domain::model::RebasePlanEntry>>,
    pub rebase_base: Option<String>,
    // Interactive rebase editor (issue 30, screen 17)
    /// The active editor tab: the Plan rows, the RESULT PREVIEW, or the raw
    /// REBASE-TODO (the "Log" tab).
    pub rebase_tab: RebaseEditorTab,
    /// The focused plan row the ⇧↑/⇧↓ and p/s/f/d shortcuts address.
    pub rebase_selected: usize,
    /// CAUTIONS computed once when the plan is built (conflict likelihood,
    /// mixed committer identities); plain data so the rail never touches git.
    pub rebase_cautions: Vec<turbogit_services::history_editor::RebaseCaution>,
    /// The editable REBASE-TODO buffer behind the Log tab; refreshed from
    /// the plan when the editor opens, re-parsed back into the plan on edit.
    pub rebase_todo: String,
    /// Ephemeral drag state of the plan rows (issue 30): the index a
    /// pointer-drag started on, while the button is down. Never persisted.
    pub rebase_drag_from: Option<usize>,
    /// The last REBASE-TODO parse error, shown under the Log tab's editor
    /// until the text parses again.
    pub rebase_todo_error: Option<String>,
    // Manage remotes (issue 33): the add-remote form, the active per-row
    // action, and the multi-root apply scope + outcome report.
    pub remotes_add_name: String,
    pub remotes_add_fetch: String,
    pub remotes_add_push: String,
    /// The remote row currently being edited: (remote name, action).
    pub remotes_row_action: Option<(String, RemoteRowAction)>,
    /// New-name buffer for a Rename row action.
    pub remotes_rename_new: String,
    /// Fetch/push URL buffers for an Edit URL row action.
    pub remotes_edit_fetch: String,
    pub remotes_edit_push: String,
    /// Which roots the pending change applies to (defaults to the focused
    /// root; a full checklist in multi-root projects).
    pub remotes_apply_scope: std::collections::HashSet<RootId>,
    /// The per-repo outcomes of the last multi-root apply, display-ready, in
    /// selection order: `(repo name, Ok(()) | Err(message))`.
    pub remotes_apply_results: Option<Vec<(String, Result<(), String>)>>,
    /// The pending apply's mode: add a remote or update an existing one.
    pub remotes_apply_update: bool,
}

/// The row action the Manage Remotes dialog is editing on one remote.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemoteRowAction {
    #[default]
    None,
    /// Editing a new name (Rename).
    Rename,
    /// Editing the fetch/push URLs (Edit URL).
    EditUrl,
    /// The branch picker for setting a branch's upstream (Set upstream…).
    SetUpstream,
}

/// The tag dialog's TYPE segmented control (issue 31, screen 16). Annotated
/// tags carry a message, a tagger and the GPG option; lightweight skips all
/// three. Annotated is the default.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TagType {
    Lightweight,
    #[default]
    Annotated,
}

/// Which tab of the interactive rebase editor (issue 30, screen 17) is open.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RebaseEditorTab {
    /// The plan rows: action chips, reordering, shortcuts.
    #[default]
    Plan,
    /// RESULT PREVIEW: the post-plan commit sequence with fold/drop
    /// summaries.
    Preview,
    /// The raw generated REBASE-TODO, editable as text and re-parsed back
    /// into the plan.
    Log,
}

/// Which central tab is active.
///
/// Settings left the strip in issue #16 (spec §9.1 correction): it is a
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    /// Commit window (label "Changes" in the new center-tabs strip).
    #[default]
    Commit,
    Log,
    Branches,
    Worktrees,
    Submodules,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommitSubTab {
    /// Tracked modifications + merge conflicts + the Unversioned Files group
    /// (issue 04 merged the old Unversioned Files sub-tab into the one tree).
    #[default]
    LocalChanges,
    Shelf,
    Stash,
}

/// A modal dialog currently open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dialog {
    Push,
    Merge,
    Rebase,
    InteractiveRebase,
    NewBranch,
    Tag,
    Shelve,
    Stash,
    /// New worktree (issue 14 Worktrees tab add action).
    NewWorktree,
    /// Cherry-pick target-branch picker (issue 15 log commit actions).
    CherryPickTarget,
    /// Cherry-pick across repositories (issue 16): multi-commit,
    /// multi-repo picker with per-target predictions.
    CherryPickAcross,
    /// Rename a branch (issue 32 branches popup row action).
    RenameBranch,
    /// Compare two branches' commit lists (issue 32, spec E9).
    CompareBranches,
    /// Manage remotes (issue 33, screen 13 "Manage remotes…"): list, add,
    /// edit URLs, rename, remove, and set branch upstreams — plus the
    /// multi-root apply across a selection.
    ManageRemotes,
}

/// A destructive action awaiting explicit confirmation (Epic C8 / Epic H3).
#[derive(Clone)]
pub enum PendingConfirm {
    Discard {
        changes: Vec<Change>,
    },
    DeleteLocalBranch {
        name: String,
    },
    DeleteRemoteBranch {
        remote: String,
        name: String,
    },
    InitHere,
    CloneRepo,
    /// Remove a linked worktree of the focused root (issue 14).
    RemoveWorktree {
        path: PathBuf,
    },
    /// De-init a submodule of the focused root (issue 14).
    DeinitSubmodule {
        path: PathBuf,
    },
    /// Revert the selected log commit with an inverse commit (issue 15).
    RevertCommit {
        commit: String,
    },
}

/// What the diff viewer should display.
#[derive(Clone)]
pub struct DiffTarget {
    pub root: RootId,
    pub left: Option<String>,
    pub right: Option<String>,
    pub path: Option<PathBuf>,
}

/// What the blame view should display (issue 18): one file at one revision
/// in one root. Set by the changed-files pane's footer link / context menu;
/// [`AppState::ensure_blame`] fetches its data off the frame path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameTarget {
    pub root: RootId,
    pub path: PathBuf,
    pub rev: String,
}

/// Which working-tree comparison the diff viewer shows (issue #13, spec §8.4
/// revision chips). Each variant maps to one documented git pair:
///
/// | Chip    | Pair               | Engine call            |
/// |---------|--------------------|------------------------|
/// | `Repo`  | HEAD ↔ worktree    | `git diff HEAD`        |
/// | `Staged`| HEAD ↔ index       | `git diff --cached`    |
/// | `Local` | index ↔ worktree   | `git diff`             |
///
/// Only used when the viewer renders a working-tree comparison; explicit
/// commit-to-commit targets (Git Log) keep their fixed revision pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DiffComparison {
    /// HEAD ↔ worktree.
    Repo,
    /// HEAD ↔ index.
    Staged,
    /// index ↔ worktree.
    #[default]
    Local,
}

/// Staging granularity (issue 19, screen 06): what one selection actuation
/// on the diff surface addresses. `File` — the whole file's diff is the
/// unit; `Hunk` — whole hunks (the pre-existing whole-hunk protocol);
/// `Line` — individual changed lines plus drag-selected character ranges.
/// The default preserves the historical protocol (sub-hunk line toggling).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Granularity {
    File,
    Hunk,
    #[default]
    Line,
}

/// The active character-range selection (issue 19): one drag inside one
/// changed line of the previewed diff. `ord`/`start`/`end` follow
/// `HunkSelection::Chars` semantics. Enter stages it; Esc clears it; it
/// dies with the granularity switch and with the path's diff cache entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharSelection {
    pub path: PathBuf,
    pub hunk: usize,
    pub ord: usize,
    /// Char offsets into the line body (end exclusive).
    pub start: usize,
    pub end: usize,
}

/// Semantic category of a transient feedback message (issue #22). Drives the
/// toast's icon and STATE_* color; replaces the old ✓/✗ string-prefix
/// sniffing with a typed kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    /// Operation succeeded (`STATE_SUCCESS`, check icon).
    Success,
    /// Attention needed but nothing failed (`STATE_WARNING`).
    Warning,
    /// Operation failed (`STATE_ERROR`).
    Error,
    /// Neutral information (`STATE_INFO`).
    Info,
}

/// One transient feedback message with its semantic kind (issue #22).
#[derive(Clone, Debug)]
pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
    /// Optional action surfaced as a `Retry` button on the toast. None
    /// paints only `Dismiss`; `Some(action)` paints `Retry | Dismiss` and
    /// `AppState::retry` re-dispatches it on click (issue #02).
    pub retry: Option<RetryAction>,
}

/// One re-dispatchable operation that a toast's `Retry` button can replay
/// (issue #02). Every variant carries enough state for
/// [`AppState::retry`] to construct the same `run_git` call the original
/// op used — without reaching for stored closures (which are not
/// `Clone + Debug + Eq` and so cannot live inside `Toast`).
#[derive(Clone, Debug)]
pub enum RetryAction {
    /// `git fetch [<remote>]`.
    Fetch {
        root: PathBuf,
        remote: Option<String>,
    },
    /// `git pull [--rebase]`.
    Pull { root: PathBuf, rebase: bool },
    /// `git push <remote> <branch>` with the listed flags and (when given)
    /// a subset-refspec that only pushes the range from `selected_oldest`
    /// through the local tip (issue #24).
    Push {
        root: PathBuf,
        remote: String,
        branch: String,
        force: bool,
        tags: bool,
        no_verify: bool,
        set_upstream: bool,
        selected_oldest: Option<String>,
    },
    /// `git stash push` capturing the listed changes by path. The
    /// destructive `discard` is the natural pair (issue #02 Shelve-first).
    Shelve {
        root: PathBuf,
        changes: Vec<Change>,
        message: String,
    },
}

impl Toast {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Success,
            message: message.into(),
            retry: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Warning,
            message: message.into(),
            retry: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Error,
            message: message.into(),
            retry: None,
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Info,
            message: message.into(),
            retry: None,
        }
    }
}

/// Selected Settings-modal category (issue #26). Ephemeral: never persisted;
/// the modal always opens on [`SettingsCategory::General`].
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsCategory {
    #[default]
    General,
    Git,
    UpdateMethod,
    MultiRoot,
    ProtectedBranches,
    Appearance,
    Advanced,
}

impl SettingsCategory {
    pub fn is_general(self) -> bool {
        self == Self::General
    }

    /// The left-list label (screen 11).
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Git => "Git",
            Self::UpdateMethod => "Update Method",
            Self::MultiRoot => "Multi-Root",
            Self::ProtectedBranches => "Protected Branches",
            Self::Appearance => "Appearance",
            Self::Advanced => "Advanced",
        }
    }
}

/// UI-only ephemeral state (never persisted).
#[derive(Default)]
pub struct UiState {
    /// Persistent inputs for the modal dialogs.
    pub dlg: DialogState,
    // Commit tab
    pub commit_message: String,
    pub amend: bool,
    pub selected: HashSet<PathBuf>,
    pub recent_messages: Vec<String>,
    /// Active sub-tab inside the Commit tool window (issue #18).
    pub commit_subtab: CommitSubTab,
    /// Repo groups the user has manually expanded beyond the focus rule
    /// (issue 04): the selected root is always expanded; non-selected roots
    /// show their file rows only while they are in this set. Cleared
    /// whenever the focused root changes, so a sidebar selection collapses
    /// every other group ("focus = expand").
    pub changes_expanded: HashSet<RootId>,
    /// The root the collapse state was last normalized for (issue 04): when
    /// it diverges from [`Self::selected_root`], `changes_expanded` resets
    /// to the empty set and this marker follows the focus.
    pub changes_focus: Option<RootId>,
    /// Inline filter over the changed-file list (spec R7, CONTEXT.md "File
    /// filter"): shared by both active Commit sub-tabs, matched
    /// case-insensitively against file paths. Persists across root switches
    /// and refreshes within the session; Esc while focused or manual edits
    /// clear it.
    pub file_filter: String,
    /// Set by `/` or the Filter Files palette action; the Commit window
    /// focuses the filter input on the next render and clears the flag.
    pub focus_file_filter: bool,
    // tabs / popups
    pub tab: Tab,
    pub branches_popup: bool,
    pub branch_filter: String,
    pub log_filter: String,
    // Git Log four-pane workspace (issue #12)
    /// Live search text for the branches pane.
    pub log_branch_filter: String,
    /// Roots filter: `None` shows every root's commits, `Some(id)` narrows.
    pub log_root_filter: Option<RootId>,
    /// File selected in the changed-files pane.
    pub log_selected_file: Option<PathBuf>,
    /// Active path scope in Git Log (issue #19): `Some(path)` narrows the
    /// graph to only the commits touching that path (set from the
    /// changed-files pane's "Show history for file..." context menu).
    pub log_path_scope: Option<PathBuf>,
    /// Log pagination (issue 17): 0-based loaded-page index. The fetch
    /// window is `(log_page + 1) * LOG_PAGE_SIZE` commits per root;
    /// [`AppState::load_more_log`] widens it. The cache holds only the
    /// fetched window — the UI's "Load more" appears while any visible
    /// root's cached log fills its whole window.
    pub log_page: usize,
    pub selected_commit: Option<CommitId>,
    pub diff: Option<DiffTarget>,
    /// Open blame view (issue 18): `Some` renders the blame surface for the
    /// target file at the target revision; `None` shows the commit graph.
    pub blame: Option<BlameTarget>,
    /// Blame lines fetched for the open [`Self::blame`] target, keyed like
    /// `diff_cache` to drop stale results. Dropped wholesale on root
    /// refreshes so the view refetches after operations.
    pub blame_cache: Option<(String, Vec<BlameLine>)>,
    pub blame_loading: bool,
    pub blame_error: Option<String>,
    pub dialog: Option<Dialog>,
    pub vcs_popup: bool,
    pub settings_open: bool,
    /// The Settings modal category currently shown (issue #26, screen 11).
    pub settings_category: crate::state::SettingsCategory,
    /// Settings Git page (issue #26): the live `git --version` result behind
    /// the draft executable path — `Ok(version)` paints the "2.47.1 ✓"
    /// badge, `Err(reason)` the "✗" error state. `None` until first checked;
    /// recomputed whenever the draft path changes.
    pub git_version_badge: Option<Result<String, String>>,
    /// Draft executable path the current [`Self::git_version_badge`] was
    /// computed for, so the check reruns only when the path changes.
    pub git_version_checked_path: Option<String>,
    /// Settings Protected-Branches page (issue #26): the in-progress text of
    /// the "+ Add pattern" input. Cleared on add; never persisted.
    pub settings_new_pattern: String,
    /// Draft copy of the loaded [`VcsSettings`] while the Settings modal is
    /// open (issue #16). Edited in place, compared against
    /// [`AppState::settings`] for dirty-gating; Reset restores it from the
    /// loaded values and Cancel/close drops it without persisting.
    pub settings_draft: Option<VcsSettings>,
    // 3-way merge editor (Epic E6; redesigned in issue #15)
    pub conflict_open: Option<PathBuf>,
    pub conflict_segs: Vec<(String, String, bool)>, // (text, other_text, is_conflict)
    /// Per-conflict resolution: `None` = unresolved, `Some(0)` = ours,
    /// `Some(1)` = theirs, `Some(2)` = both ("Ignore").
    pub conflict_res: Vec<Option<u8>>,
    /// Read-only composed result shown in the editor's Result pane
    /// (free-text editing is explicitly deferred).
    /// Read-only composed result shown in the editor's Result pane
    /// (free-text editing is explicitly deferred).
    pub conflict_text: String,
    // Redesigned conflict resolver (issue #22, screen 07)
    /// Whether the dedicated Resolve Conflicts tool window is open. Set by
    /// the Commit tool window's "Resolve…" entry and from the bulk monitor.
    pub conflict_resolver_open: bool,
    /// Whether the user is mid-merge (`MERGE_HEAD` exists). Drives the
    /// visibility of the Abort merge / Continue merge header actions.
    pub merge_in_progress: bool,
    /// Currently focused conflict ordinal in the resolver; Prev/Next move it.
    pub conflict_resolver_active_idx: usize,
    /// Auto-advance toggle: when on, resolving the active conflict moves
    /// the cursor to the next still-unresolved conflict in the file.
    pub conflict_resolver_auto_advance: bool,
    /// Per-conflict undo history (last resolution per conflict ordinal).
    /// `None` means no resolution to undo (still unresolved).
    pub conflict_resolver_undo: Vec<Option<u8>>,
    /// Whether the Result pane has been edited by the user (vs. auto-
    /// composed from per-conflict resolutions). When true, the free-text
    /// edit IS the resolved content on Apply.
    pub conflict_resolver_edited: bool,
    /// Files git auto-resolved during the in-progress merge (source:
    /// `merge_auto_merged_files`). Listed separately from conflicted files.
    pub auto_merged_files: Vec<PathBuf>,
    /// Currently selected file in the resolver's left list (defaults to
    /// the first conflicted file when the resolver opens).
    pub conflict_resolver_selected: Option<PathBuf>,
    pub shelves: Vec<Shelf>,
    // commit-tab inline diff preview (Epic C3)
    pub preview_change: Option<PathBuf>,
    // diff viewer (Epic E: async cache + layout)
    pub diff_cache: Option<(String, String)>,
    pub diff_loading: bool,
    pub diff_error: Option<String>,
    pub diff_side_by_side: bool,
    /// Non-text diff pane results (decoded image bytes + binary sizes, spec
    /// R8) keyed by load key — the plain-data cache lives in
    /// [`crate::diff_data`], not the UI module. Bounded few entries, evicts
    /// oldest; invalidated wholesale with root refreshes like `diff_cache`
    /// (CONTEXT.md "Root caches" philosophy). GPU textures are a UI-layer
    /// concern: they are keyed by [`Self::pane_generation`], never stored
    /// here.
    pub pane_bytes: crate::diff_data::PaneCache,
    /// Generation of [`Self::pane_bytes`]: bumped on every wholesale clear so
    /// the UI layer can drop its lazily-uploaded GPU textures without the
    /// application state naming any egui type (DDD split issue 04).
    pub pane_generation: u64,
    /// Load key currently being fetched on a worker thread — one in-flight
    /// non-text pane load at a time (mirrors `diff_loading`).
    pub pane_bytes_loading: Option<String>,
    /// The single hunk of the open diff that all hunk navigation and
    /// granular verbs act on (CONTEXT.md "Current hunk"): buttons, hover,
    /// and keyboard navigation set it; stage/unstage consume it. Reset to
    /// the first hunk whenever a fresh diff load starts.
    pub diff_current_hunk: usize,
    /// Hunks currently collapsed in the diff viewer (issue 20, screen 06):
    /// hunk indices of the open diff whose body rows hide behind an
    /// "N hidden" band. Ephemeral: cleared whenever a fresh diff load
    /// starts, alongside [`Self::diff_current_hunk`].
    pub diff_collapsed: HashSet<usize>,
    // diff viewer working-tree comparison chips + whitespace toggle (issue #13)
    pub diff_comparison: DiffComparison,
    pub diff_ignore_whitespace: bool,
    /// Armed edge press for F7/Shift+F7 cross-file navigation (spec R7): the
    /// direction and instant of the last edge nudge. A same-direction repeat
    /// inside [`crate::diff_data::EDGE_WINDOW`] crosses to the adjacent
    /// changed file; anything else re-arms.
    pub hunk_nav_armed_edge: Option<(crate::diff_data::Dir, std::time::Instant)>,
    /// The preview path a granular stage/unstage was last dispatched for
    /// (spec R2 story 9). Consumed by the op-completion handler to decide
    /// whether the file just left the changelist.
    pub pending_granular: Option<PathBuf>,
    /// Paths whose last granular op completed with nothing unstaged left
    /// (spec R2 story 9): they stop being listed in the changelist buckets.
    /// Ephemeral by design — entries drop as soon as the path regains
    /// unstaged changes or disappears from status entirely.
    pub granularly_completed: HashSet<PathBuf>,
    /// Sub-hunk line selections (spec R2 story 3): preview path → hunk index
    /// → ordinals over that hunk's +/- lines (`HunkSelection::Lines`
    /// semantics). Ephemeral: cleared whenever the path's diff cache entry
    /// changes — which also covers every successful granular op, since ops
    /// invalidate the cache and force a reload.
    pub line_selections: HashMap<PathBuf, BTreeMap<usize, BTreeSet<usize>>>,
    /// Staging granularity of the diff surface (issue 19): File | Hunk |
    /// Line. Governs what line clicks select and what the granular verbs
    /// address; switching it clears accumulated selections.
    pub diff_granularity: Granularity,
    /// The active drag-selected character range (issue 19): at most one,
    /// inside one changed line of the previewed diff. Enter stages it, Esc
    /// clears it.
    pub char_selection: Option<CharSelection>,
    /// Transient drag draft (issue 19): the anchor of the in-flight char
    /// drag — `(path, hunk, ord, anchor char)` — so the range can widen in
    /// either direction while the pointer moves. Set on drag start, cleared
    /// on drag end.
    pub char_drag_anchor: Option<(PathBuf, usize, usize, usize)>,
    // command palette (Epic F5)
    pub command_palette: bool,
    pub command_query: String,
    // transient
    /// Last feedback message with its semantic kind (issue #22).
    pub toast: Option<Toast>,
    pub toast_shown_at: Option<f64>,
    pub busy: bool,
    // confirmation-gated destructive actions (Epic C8)
    pub confirm: Option<crate::state::PendingConfirm>,
    // recently opened repositories (Epic J4)
    pub recent_repos: Vec<PathBuf>,
    // Branches popup (issue #14): recently checked-out branches (≤5, newest first).
    pub recent_branches: Vec<String>,
    /// Keyboard cursor into the branches popup's flattened selectable rows
    /// (issue #14: ↑/↓ move it, Enter checks the highlighted row out).
    pub branches_cursor: usize,
    // IDE shell visibility model (issue #9, spec §9.2): true → the central
    // body routes to the Welcome page instead of the active tool window.
    // Derived true whenever no root is open (`AppState::show_welcome`).
    pub welcome_visible: bool,
    // Workspace tree sidebar (issue #05): live filter text matched against
    // repo names, paths, and branch names; collapsed group keys.
    pub sidebar_filter: String,
    pub sidebar_collapsed: HashSet<String>,
    /// "What's new" changelog overlay visibility on the Welcome screen
    /// (issue #34): toggled by the header link, dismissed via Close.
    pub show_changelog: bool,
    /// The active smart-group filter (issue #06): `Some(label)` narrows the
    /// projects tree to that group's member repos; `None` shows everything.
    /// Set by clicking a smart-group row in the sidebar.
    pub sidebar_smart_group: Option<String>,
    /// User-defined smart group rules (issue #07), evaluated alongside the
    /// built-ins every frame and persisted with the workspace through
    /// [`Self::persist_ui`] / the launch path.
    pub smart_group_rules: Vec<crate::smart_rules::SmartGroupRule>,
    // Multi-repo selection (issue #08): the checked repos of the workspace
    // tree — plain data here so the app crate never names a UI type; the
    // tri-state rules live in the ui crate over the sidebar tree.
    pub repo_selection: HashSet<RootId>,
    // Bulk operations grid (issue 09): `bulk_op` is `Some` while the
    // preflight matrix modal is open for that operation; `bulk_rebase` is
    // the pull policy control, seeded from settings on open.
    pub bulk_op: Option<BulkOp>,
    pub bulk_rebase: bool,
    // Cascade create-&-checkout branch (issue 11): the branch-name input
    // and the apply-broadly policy (base on the upstream when behind),
    // seeded to `true` when the cascade modal opens.
    pub bulk_branch_name: String,
    pub bulk_from_upstream: bool,
    // Custom command (issue 13): the command input of the custom-command
    // preflight modal, and the armed flag of its two-stage destructive
    // confirmation — the first Run click on a destructive-looking command
    // only arms; the explicit confirm click dispatches.
    pub bulk_command: String,
    pub bulk_command_armed: bool,
    /// Recent custom commands (issue 13), newest first, deduplicated, capped
    /// at [`crate::state::MAX_RECENT_CUSTOM_COMMANDS`]; re-selectable in the
    /// modal and persisted with the workspace.
    pub recent_custom_commands: Vec<String>,
    /// Live cascade-run monitor (issue 10, screen 03): `Some` from the
    /// moment a confirmed plan dispatches until the user closes it; rows,
    /// tallies, and ETA advance through [`AppEvent::BulkRunProgress`].
    pub bulk_run: Option<crate::bulk_run_view::BulkRunView>,
    /// Live cherry-pick-across run monitor (issue 16): same pool events as
    /// the bulk monitor, one row per target repo whose step applies the
    /// picked commits in order.
    pub cherry_run: Option<crate::cherry_run_view::CherryRunView>,
    /// Pinned selections ("Pin as view", issue #08), persisted with the
    /// workspace through [`Self::persist_ui`] / the launch path.
    pub pinned_views: Vec<crate::pinned_views::PinnedView>,
    /// Recent bulk operations (issue 12, screen 04): one record per
    /// completed bulk/cascade run, persisted with the workspace through
    /// [`Self::persist_ui`] / the launch path.
    pub bulk_history: Vec<crate::bulk_history::BulkRunRecord>,
    /// The history record whose Details drill-down is expanded (issue 12),
    /// keyed by record id.
    pub bulk_history_open: Option<u64>,
    /// Rule editor modal (issue #07). `smart_rule_editing` is `None` while
    /// creating a new rule, `Some(ix)` while editing that rule; the draft
    /// copy under edit follows the Settings-modal pattern — rows edit the
    /// draft only, Save commits to [`Self::smart_group_rules`] + disk,
    /// Cancel/X discards it.
    pub smart_rule_editor_open: bool,
    pub smart_rule_editing: Option<usize>,
    pub smart_rule_draft: Option<crate::smart_rules::SmartGroupRule>,
    // Welcome page (issue #10): in-memory copy of the global recents store
    // (ADR-0005), loaded at launch and refreshed on every recorded open.
    pub recent_projects: Vec<crate::recents::RecentProject>,
    /// In-memory branch-indicator cache for visible recents: path →
    /// `(branch, computed_at)`. Computed live at render, never persisted.
    pub welcome_branch_cache: HashMap<PathBuf, (Option<String>, std::time::Instant)>,
    // Inline clone form on the Welcome page (issue #10).
    pub welcome_clone_url: String,
    pub welcome_shallow: bool,
    /// Set by the Clone action card; focuses the URL input on the next render.
    pub welcome_focus_clone: bool,
    /// User-toggleable shell regions (View menu); not persisted in v1.
    pub show_toolbar: bool,
    pub show_status_bar: bool,
    /// Inline banner for the active tool surface (issue #02). Surfaces
    /// set this with a `Banner` (severity + message + deep-link
    /// actions) and `ui::render` paints it above the content.
    pub banner: Option<crate::banner::Banner>,
    /// Session-durable activity feed (issue #04): every dispatched git
    /// operation and its outcome, appended on `OpCompleted`. Filterable
    /// by repo and time window; survives collapse/expand and outlives
    /// toasts.
    pub activity: crate::activity::ActivityLog,
}
pub struct AppState {
    pub project_dir: PathBuf,
    /// The Git engine. This interface is the seam (ADR-0001).
    pub executor: Arc<dyn GitExecutor>,
    /// Canonical engine settings (git binary path, update method, …).
    pub settings: VcsSettings,
    pub multi: MultiRootManager,
    pub tx: Sender<AppEvent>,
    pub rx: Receiver<AppEvent>,
    pub selected_root: Option<RootId>,
    pub clone_url: String,
    pub last_error: Option<String>,
    pub ui: UiState,
    /// The root-keyed cache layer (logs, ref decorations, changed files,
    /// path-scoped logs, ahead/behind) behind one interface (CONTEXT.md
    /// "Root caches").
    pub caches: RootCaches,
    /// Override for the OS config dir hosting the global recents file
    /// (ADR-0005). `None` → `recents::default_config_dir()`. Tests inject a
    /// temp dir so the real user configuration is never touched.
    pub recents_config_dir: Option<PathBuf>,
    /// Native folder-picker seam for the Welcome Open/Initialize flows.
    /// Production wires `rfd`; tests inject closures returning fixed paths.
    pub dir_picker: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    /// Resolved git version (e.g. `2.47.1`) for the topbar header line,
    /// computed once at launch (issue #34) so the header never spawns `git`
    /// per frame. `"unknown"` when the binary cannot be resolved.
    pub git_version: String,
    /// Headless-harness mode: completed ops refresh root status synchronously
    /// instead of spawning background rescans (see `for_roots`).
    pub(crate) sync_refresh: bool,
    /// In-flight worktree-list fetches per root (issue 14): guards the
    /// fetch-on-miss trigger against re-dispatching every frame while a
    /// fetch is in flight (the cache entry only lands with the event).
    fetching_worktrees: HashSet<RootId>,
    /// In-flight submodule-list fetches per root (issue 14).
    fetching_submodules: HashSet<RootId>,
    /// Last dispatch instant of the background incoming check (issue #27);
    /// `None` while disabled or never polled.
    pub(crate) incoming_poll_last: Option<std::time::Instant>,
    /// Roots with an in-flight incoming-check poll (issue #27): the guard
    /// keeps the scheduler from stacking fetches on the same repo; entries
    /// only clear with the poll's settlement.
    pub(crate) incoming_poll_inflight: HashSet<RootId>,
}

impl AppState {
    pub fn new(project_dir: PathBuf) -> Self {
        Self::launch(Some(project_dir))
    }

    /// Launch with an explicit project directory: roots are discovered and
    /// the shell is entered directly — `turbogit <path>` (ADR-0004). `None`
    /// lands on the Welcome screen without scanning any directory.
    pub fn launch(project_dir: Option<PathBuf>) -> Self {
        Self::launch_in(project_dir, None)
    }

    /// Launch flow (ADR-0004): with a project directory the shell opens
    /// straight away; without one the Welcome screen is the landing surface
    /// and no CWD scan happens. `recents_config_dir` overrides the OS config
    /// dir hosting the global recents file (tests inject a temp dir).
    pub fn launch_in(project_dir: Option<PathBuf>, recents_config_dir: Option<PathBuf>) -> Self {
        let (tx, rx) = unbounded();
        let settings = VcsSettings::default();
        let executor = build_executor(&settings);
        let git_version =
            turbogit_engine::resolve_git_version(&settings).unwrap_or_else(|_| "unknown".into());
        let mut state = Self {
            project_dir: project_dir.clone().unwrap_or_default(),
            executor,
            settings,
            multi: MultiRootManager::default(),
            tx,
            rx,
            selected_root: None,
            clone_url: String::new(),
            last_error: None,
            git_version,
            // Shell regions start visible; View-menu toggles flip these.
            ui: UiState {
                show_toolbar: true,
                show_status_bar: true,
                ..UiState::default()
            },
            caches: RootCaches::default(),
            recents_config_dir,
            dir_picker: None,
            sync_refresh: false,
            fetching_worktrees: HashSet::new(),
            fetching_submodules: HashSet::new(),
            incoming_poll_last: None,
            incoming_poll_inflight: HashSet::new(),
        };

        // Global recents (ADR-0005) load before anything is open so the
        // Welcome screen can list them.
        if let Some(cfg) = state.recents_config() {
            state.ui.recent_projects = crate::recents::load(&cfg).projects;
        }

        match project_dir {
            Some(dir) => {
                state.project_dir = dir;
                state.rescan();

                // Restore persisted UI state (Epic J4): active tab, draft message, recent repos.
                let ui = crate::persistence::load_ui_state(&state.project_dir);
                state.ui.tab = match ui.tab.as_str() {
                    "Log" => Tab::Log,
                    "Branches" => Tab::Branches,
                    "Worktrees" => Tab::Worktrees,
                    "Submodules" => Tab::Submodules,
                    // Legacy persisted "History" (removed in issue #19)
                    // gracefully falls back to the Commit tab.
                    _ => Tab::Commit,
                };
                state.ui.recent_repos = ui.recent_repos;
                state.ui.smart_group_rules = ui.smart_group_rules;
                state.ui.pinned_views = ui.pinned_views;
                state.ui.bulk_history = ui.bulk_history;
                state.ui.recent_custom_commands = ui.recent_custom_commands;
            }
            None => {
                // No project directory supplied: land on Welcome (ADR-0004).
                state.ui.welcome_visible = true;
            }
        }
        state
    }

    /// Headless-harness constructor (see CONTEXT.md "Headless harness"): a
    /// deterministic [`AppState`] over explicit repository roots.
    ///
    /// Roots are registered synchronously through the same registration path
    /// production uses ([`turbogit_services::multi_root::register_all`]); no background
    /// threads are spawned, and completed operations refresh root status
    /// synchronously instead of rescanning on workers. Panics if any root cannot
    /// be snapshotted — a broken test fixture should fail at construction.
    pub fn for_roots(project_dir: &Path, roots: &[PathBuf]) -> Self {
        let (tx, rx) = unbounded();
        let settings = VcsSettings::default();
        let executor = build_executor(&settings);
        let git_version =
            turbogit_engine::resolve_git_version(&settings).unwrap_or_else(|_| "unknown".into());
        let mut state = Self {
            project_dir: project_dir.to_path_buf(),
            executor,
            settings,
            multi: MultiRootManager::default(),
            tx,
            rx,
            selected_root: None,
            clone_url: String::new(),
            last_error: None,
            git_version,
            // Bare UiState defaults (toolbar/status bar hidden), matching what
            // the headless suites assert against — NOT launch_in's visible shell.
            ui: UiState::default(),
            caches: RootCaches::default(),
            recents_config_dir: None,
            dir_picker: None,
            sync_refresh: true,
            fetching_worktrees: HashSet::new(),
            fetching_submodules: HashSet::new(),
            incoming_poll_last: None,
            incoming_poll_inflight: HashSet::new(),
        };
        let results = turbogit_services::multi_root::register_all(
            state.executor.as_ref(),
            &mut state.multi,
            roots,
        );
        for r in results {
            if let Err(e) = r {
                panic!("for_roots: failed to snapshot root: {e}");
            }
        }
        state.selected_root = state.multi.roots.first().map(|r| r.id.clone());
        state
    }

    /// Override the Git engine (e.g. a recording test double).
    pub fn with_executor(mut self, executor: Arc<dyn GitExecutor>) -> Self {
        self.executor = executor;
        self
    }

    /// Override canonical engine settings (e.g. protected-branch patterns).
    pub fn with_settings(mut self, settings: VcsSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Rebuild the engine from the current settings behind the seam
    /// (ADR-0001: a changed git binary or backend applies live). The
    /// composition-root factory lives in `turbogit-engine`, so the app
    /// crate owns this call and the UI crate reaches it through here
    /// instead of depending on the adapters.
    pub fn rebuild_executor(&mut self) {
        self.executor = build_executor(&self.settings);
    }

    /// The effective config dir for the global recents file (ADR-0005).
    fn recents_config(&self) -> Option<PathBuf> {
        self.recents_config_dir
            .clone()
            .or_else(crate::recents::default_config_dir)
    }

    /// Persist lightweight UI state (active tab, draft message, recent repos).
    pub fn persist_ui(&self) {
        let ui = crate::persistence::UiPersist {
            tab: match self.ui.tab {
                Tab::Log => "Log",
                Tab::Branches => "Branches",
                Tab::Worktrees => "Worktrees",
                Tab::Submodules => "Submodules",
                _ => "Commit",
            }
            .to_string(),
            recent_repos: self.ui.recent_repos.clone(),
            draft_message: self.ui.commit_message.clone(),
            smart_group_rules: self.ui.smart_group_rules.clone(),
            pinned_views: self.ui.pinned_views.clone(),
            bulk_history: self.ui.bulk_history.clone(),
            recent_custom_commands: self.ui.recent_custom_commands.clone(),
        };
        let _ = crate::persistence::save_ui_state(&self.project_dir, &ui);
    }
    /// dispatch a fresh asynchronous status scan for every registered root.
    pub fn rescan(&mut self) {
        let paths = turbogit_services::multi_root::discover_roots(
            self.executor.as_ref(),
            &self.project_dir,
        );
        let results = turbogit_services::multi_root::register_all(
            self.executor.as_ref(),
            &mut self.multi,
            &paths,
        );
        for r in &results {
            if let Err(e) = r {
                self.last_error = Some(e.to_string());
            }
        }
        if self.selected_root.is_none() {
            self.selected_root = self.multi.roots.first().map(|r| r.id.clone());
        }

        let executor = self.executor.clone();
        let tx = self.tx.clone();
        for root in &self.multi.roots {
            let root_path = root.id.0.clone();
            let exec = executor.clone();
            let tx_status = tx.clone();
            std::thread::spawn(move || {
                let res = exec.status(&root_path);
                let _ = tx_status.send(AppEvent::StatusScanned {
                    root: RootId(root_path),
                    status: res,
                });
            });
            // Ahead/behind of the current branch vs its upstream (Epic D3).
            let exec2 = executor.clone();
            let tx2 = tx.clone();
            let rp = root.id.0.clone();
            std::thread::spawn(move || {
                if let Ok((ahead, behind)) = current_branch_ahead_behind(exec2.as_ref(), &rp) {
                    let _ = tx2.send(AppEvent::AheadBehind {
                        root: RootId(rp),
                        ahead,
                        behind,
                    });
                }
            });
        }
    }

    /// Fetch (and cache) the commit log for a root on a worker thread. The
    /// fetch is page-sized (issue 17): `(ui.log_page + 1) * LOG_PAGE_SIZE`
    /// commits, widened by [`Self::load_more_log`] — never the uncapped
    /// whole history.
    pub fn fetch_log(&mut self, root: RootId) {
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        let limit = (self.ui.log_page + 1) * LOG_PAGE_SIZE;
        std::thread::spawn(move || {
            let res = executor.log(
                &root.0,
                &LogOpts {
                    max_count: Some(limit),
                    ..Default::default()
                },
            );
            let _ = tx.send(AppEvent::LogLoaded { root, commits: res });
        });
    }

    /// Widen the log fetch window by one page (issue 17's "Load more") and
    /// refetch every registered root's log through the worker path. The
    /// wider fetch *replaces* the cached window — there is no append, so no
    /// duplicates can accumulate.
    pub fn load_more_log(&mut self) {
        self.ui.log_page += 1;
        for root in self.multi.roots.clone() {
            self.fetch_log(root.id);
        }
    }

    /// Cache key for an open blame target: root | revision | path. A cache
    /// entry under a different key is stale and never shown.
    fn blame_key(target: &BlameTarget) -> String {
        format!(
            "{}|{}|{}",
            target.root.0.display(),
            target.rev,
            target.path.display()
        )
    }

    /// Fetch the open blame target's lines on a worker thread (issue 18),
    /// mirroring the diff viewer's ensure pattern: a no-op when the cache
    /// already matches the target or a fetch is in flight; the result lands
    /// as [`AppEvent::BlameReady`]. A refresh drops the cache, so the view's
    /// next ensure refetches after any operation.
    pub fn ensure_blame(&mut self) {
        let Some(target) = self.ui.blame.clone() else {
            return;
        };
        let key = Self::blame_key(&target);
        if self.ui.blame_loading
            || self
                .ui
                .blame_cache
                .as_ref()
                .map(|(k, _)| *k == key)
                .unwrap_or(false)
        {
            return;
        }
        self.ui.blame_loading = true;
        self.ui.blame_error = None;
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = turbogit_services::history_service::blame(
                executor.as_ref(),
                &target.root.0,
                &target.path,
                Some(&target.rev),
            );
            let _ = tx.send(AppEvent::BlameReady { key, result: res });
        });
    }

    /// Fetch (and cache) the linked-worktree list for a root on a worker
    /// thread (issue 14 Worktrees tab). One fetch per root in flight —
    /// `fetch_worktrees` is a no-op while an earlier fetch for the same
    /// root is still pending.
    pub fn fetch_worktrees(&mut self, root: RootId) {
        if !self.fetching_worktrees.insert(root.clone()) {
            return;
        }
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = executor.worktree_list(&root.0);
            let _ = tx.send(AppEvent::WorktreesLoaded {
                root,
                worktrees: res,
            });
        });
    }

    /// Fetch (and cache) the submodule list for a root on a worker thread
    /// (issue 14 Submodules tab). One fetch per root in flight, mirroring
    /// [`Self::fetch_worktrees`].
    pub fn fetch_submodules(&mut self, root: RootId) {
        if !self.fetching_submodules.insert(root.clone()) {
            return;
        }
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let res = executor.submodule_status(&root.0);
            let _ = tx.send(AppEvent::SubmodulesLoaded {
                root,
                submodules: res,
            });
        });
    }

    /// The one refresh seam for completed operations and manual refresh
    /// (root-caches deepening, decision 7): drop the affected roots' cache
    /// entries, refresh those roots' snapshots + ahead/behind — no root
    /// DISCOVERY here (clone/init/open have their own paths) — and refetch
    /// the selected root's log iff it is in scope.
    ///
    /// Snapshot refresh goes through [`turbogit_services::multi_root::register_all`],
    /// which replaces the registered snapshot per id (branches / HEAD /
    /// status) without scanning for new roots — what kept branch indicators
    /// fresh after checkouts pre-refactor, now scoped to the affected roots.
    ///
    /// Production computes ahead/behind on worker threads; the headless
    /// harness (`sync_refresh`) mirrors the same steps synchronously.
    pub fn refresh(&mut self, affected: Affected) {
        self.caches.invalidate(&affected);
        // The diff viewer caches raw patch text outside the root caches;
        // a completed op may have changed exactly what it shows (spec R2
        // story 8), so drop it and let the viewer reload asynchronously.
        self.ui.diff_cache = None;
        // The blame view follows the same wholesale rule (issue 18): a
        // completed op may have changed exactly what the open target blames,
        // so drop it and let the view's next ensure refetch.
        self.ui.blame_cache = None;
        // Non-text pane bytes follow the same wholesale rule (spec R8):
        // dropped with root refreshes, never poked per field. Bumping the
        // generation tells the UI layer to drop its GPU textures too — the
        // plain-data cache never holds an egui type itself (DDD split
        // issue 04).
        self.ui.pane_bytes.clear();
        self.ui.pane_generation += 1;
        self.ui.pane_bytes_loading = None;
        // Granular exclusions follow a refresh-scoped lifetime rule owned by
        // the granular module: they only hold while the path is still fully
        // staged.
        granular::prune_on_refresh(self);
        // Only roots that are actually registered take part in the refresh.
        // The ids' shared `Arc<Path>` handles clone by refcount here and flow
        // into `register_all` / the ahead-behind refresh unchanged.
        let paths: Vec<std::sync::Arc<Path>> = match &affected {
            Affected::All => self.multi.roots.iter().map(|r| r.id.0.clone()).collect(),
            Affected::Root(id) => self
                .multi
                .roots
                .iter()
                .filter(|r| &r.id == id)
                .map(|r| r.id.0.clone())
                .collect(),
        };
        let results = turbogit_services::multi_root::register_all(
            self.executor.as_ref(),
            &mut self.multi,
            &paths,
        );
        for r in &results {
            if let Err(e) = r {
                self.last_error = Some(e.to_string());
            }
        }

        // Ahead/behind of each affected root's current branch vs upstream.
        if self.sync_refresh {
            // Headless harness: refresh synchronously, no threads.
            let executor = self.executor.clone();
            for path in paths {
                if let Ok(ab) = current_branch_ahead_behind(executor.as_ref(), &path) {
                    self.caches.store_ahead_behind(RootId(path), ab);
                }
            }
        } else {
            let executor = self.executor.clone();
            let tx = self.tx.clone();
            for rp in paths {
                let exec2 = executor.clone();
                let tx2 = tx.clone();
                std::thread::spawn(move || {
                    if let Ok((ahead, behind)) = current_branch_ahead_behind(exec2.as_ref(), &rp) {
                        let _ = tx2.send(AppEvent::AheadBehind {
                            root: RootId(rp),
                            ahead,
                            behind,
                        });
                    }
                });
            }
        }

        // Refetch the selected root's log iff it is inside the scope.
        if let Some(sel) = self.selected_root.clone() {
            let in_scope = match &affected {
                Affected::All => true,
                Affected::Root(id) => *id == sel,
            };
            if in_scope {
                if self.sync_refresh {
                    if let Ok(commits) = self.executor.log(&sel.0, &LogOpts::default()) {
                        self.caches.store_log(sel, commits);
                    }
                } else {
                    self.fetch_log(sel);
                }
            }
        }
    }

    /// Dispatch a git operation on a worker thread. `work` receives the
    /// engine (`GitExecutor`) and returns a `TgResult<()>`; the result is posted as an
    /// `OpCompleted` event and the affected roots' caches and status are
    /// refreshed on completion ([`AppState::refresh`]). Every call site
    /// declares its scope via `affected`.
    pub fn run_git<W>(&mut self, label: String, affected: Affected, work: W)
    where
        W: FnOnce(&dyn GitExecutor) -> TgResult<()> + Send + 'static,
    {
        self.run_git_with_retry(label, affected, None, work);
    }

    /// Like [`Self::run_git`] but attaches `retry` to the `OpCompleted`
    /// event so a failure surfaces a `Retry` button on the toast
    /// (issue #02).
    pub fn run_git_with_retry<W>(
        &mut self,
        label: String,
        affected: Affected,
        retry: Option<RetryAction>,
        work: W,
    ) where
        W: FnOnce(&dyn GitExecutor) -> TgResult<()> + Send + 'static,
    {
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        self.ui.busy = true;
        std::thread::spawn(move || {
            let res = work(executor.as_ref());
            let _ = tx.send(AppEvent::OpCompleted {
                label,
                affected,
                result: res,
                retry,
            });
        });
    }

    /// Re-dispatch a previously-failed operation (issue #02). Called by
    /// the toast's `Retry` button via [`crate::state::Toast::retry`]. The
    /// action is consumed and a fresh worker thread is spawned; the
    /// completion will surface as a normal `OpCompleted` toast (success
    /// or a new error toast, this time without a retry by default — a
    /// second retry belongs to the user).
    pub fn retry(&mut self, action: RetryAction) {
        match action {
            RetryAction::Fetch { root, remote } => {
                let label = match &remote {
                    Some(r) => format!("Fetch from {r}"),
                    None => "Fetch".to_string(),
                };
                let affected = Affected::from_optional_root(Some(&root));
                self.run_git(label, affected, move |v| v.fetch(&root, remote.as_deref()));
            }
            RetryAction::Pull { root, rebase } => {
                let label = if rebase { "Pull --rebase" } else { "Pull" }.to_string();
                let affected = Affected::from_optional_root(Some(&root));
                self.run_git(label, affected, move |v| v.pull(&root, rebase));
            }
            RetryAction::Push {
                root,
                remote,
                branch,
                force,
                tags,
                no_verify,
                set_upstream,
                selected_oldest,
            } => {
                let label = format!("Push {remote}/{branch}");
                let affected = Affected::from_optional_root(Some(&root));
                let sha = selected_oldest;
                self.run_git(label, affected, move |v| {
                    v.push(
                        &root,
                        &remote,
                        &branch,
                        force,
                        tags,
                        no_verify,
                        set_upstream,
                        sha.as_deref(),
                    )
                });
            }
            RetryAction::Shelve {
                root,
                changes,
                message,
            } => {
                let paths: Vec<PathBuf> = changes.iter().map(|c| c.path.clone()).collect();
                let affected = Affected::from_optional_root(Some(&root));
                self.run_git("Shelve".to_string(), affected, move |v| {
                    turbogit_services::shelve_stash::stash(v, &root, &message, false)?;
                    // The stash only captures worktree changes; a clean
                    // discard follows for the paths the user explicitly
                    // chose. Best-effort: stash failure already errored.
                    let _ = turbogit_services::changes::discard_changes(v, &root, &changes);
                    let _ = paths; // kept to document intent
                    Ok(())
                });
            }
        }
    }

    /// Apply a remote change (add or update) across `selected` roots (issue
    /// 33 "apply to selection"): the port ops run on a worker, and the
    /// per-repo outcomes come back through the bulk-completed pipeline —
    /// aggregate toast naming failed repos, `refresh(Affected::All)`, and an
    /// activity record — plus the display-ready rows stored on the dialog
    /// state for the manager to render.
    pub fn apply_remote_change(
        &mut self,
        selected: Vec<RootId>,
        change: turbogit_services::remote_service::RemoteChange,
    ) {
        let snapshots: Vec<(RootId, PathBuf)> = selected
            .iter()
            .filter_map(|id| self.multi.by_id(id).map(|r| (id.clone(), r.path.clone())))
            .collect();
        if snapshots.is_empty() {
            return;
        }
        let label = match &change {
            turbogit_services::remote_service::RemoteChange::Add { name, .. } => {
                format!("Add remote {name}")
            }
            turbogit_services::remote_service::RemoteChange::SetUrl { name, .. } => {
                format!("Update remote {name}")
            }
        };
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        self.ui.busy = true;
        std::thread::spawn(move || {
            let results = turbogit_services::remote_service::apply_to_paths(
                executor.as_ref(),
                &snapshots,
                &change,
            );
            let _ = tx.send(AppEvent::BulkCompleted { label, results });
        });
    }

    /// Execute a confirmed destructive action (Epic C8). The UI gates these
    /// behind a confirmation dialog and only calls this on explicit OK.
    pub fn run_confirmed(&mut self, c: PendingConfirm) {
        match c {
            PendingConfirm::Discard { changes } => {
                let root = self.selected_path();
                let affected = Affected::from_optional_root(root.as_deref());
                self.run_git("Discard changes".into(), affected, move |v| {
                    if let Some(r) = &root {
                        changes::discard_changes(v, r, &changes)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::DeleteLocalBranch { name } => {
                let root = self.selected_path();
                let affected = Affected::from_optional_root(root.as_deref());
                self.run_git(format!("Delete branch {name}"), affected, move |v| {
                    if let Some(r) = &root {
                        v.branch_delete(r, &name, false)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::DeleteRemoteBranch { remote, name } => {
                let root = self.selected_path();
                let affected = Affected::from_optional_root(root.as_deref());
                self.run_git("Delete remote branch".into(), affected, move |v| {
                    if let Some(r) = &root {
                        v.branch_delete_remote(r, &remote, &name)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::InitHere => self.init_repo(),
            PendingConfirm::CloneRepo => self.clone_repo(),
            PendingConfirm::RemoveWorktree { path } => {
                let root = self.selected_path();
                let affected = Affected::from_optional_root(root.as_deref());
                self.run_git("Remove worktree".into(), affected, move |v| {
                    if let Some(r) = &root {
                        v.worktree_remove(r, &path, false)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::DeinitSubmodule { path } => {
                let root = self.selected_path();
                let affected = Affected::from_optional_root(root.as_deref());
                self.run_git("Deinit submodule".into(), affected, move |v| {
                    if let Some(r) = &root {
                        v.submodule_deinit(r, &path, false)
                    } else {
                        Ok(())
                    }
                });
            }
            PendingConfirm::RevertCommit { commit } => {
                let root = self.selected_path();
                let affected = Affected::from_optional_root(root.as_deref());
                let settings = self.settings.clone();
                self.run_git(
                    format!("Revert {}", short_sha(&commit)),
                    affected,
                    move |v| {
                        if let Some(r) = &root {
                            turbogit_services::integrate_service::revert_commit(
                                v, r, &commit, &settings,
                            )
                        } else {
                            Ok(())
                        }
                    },
                );
            }
        }
    }

    /// Cherry-pick `commit` onto the branch `target` for the focused root
    /// (issue 15 log commit action). The service guards the protected target
    /// and a dirty worktree; the original checkout is restored afterwards.
    pub fn cherry_pick_to(&mut self, commit: String, target: String) {
        let root = self.selected_path();
        let affected = Affected::from_optional_root(root.as_deref());
        let settings = self.settings.clone();
        let label = format!("Cherry-pick {} to {target}", short_sha(&commit));
        self.run_git(label, affected, move |v| {
            if let Some(r) = &root {
                turbogit_services::integrate_service::cherry_pick_to(
                    v, r, &commit, &target, &settings,
                )
            } else {
                Ok(())
            }
        });
    }

    // -- Cherry-pick across repositories (issue 16) ---------------------------

    /// The source repo's log, cache-first with a synchronous engine read as
    /// fallback (the dialog can open before the log tab has loaded).
    fn cherry_source_log(&self, source: &RootId) -> Vec<Commit> {
        self.caches
            .log(source)
            .map(|l| l.to_vec())
            .unwrap_or_else(|| {
                self.executor
                    .log(source.0.as_ref(), &LogOpts::default())
                    .unwrap_or_default()
            })
    }

    /// Open the cross-repo cherry-pick dialog (issue 16): the source is the
    /// focused root, the candidate commits its log in application order
    /// (oldest first), and the checked targets default to the current
    /// multi-repo selection — every other root when nothing is selected.
    pub fn open_cherry_across(&mut self) {
        let Some(source) = self.selected_root.clone() else {
            return;
        };
        let mut candidates = self.cherry_source_log(&source);
        candidates.reverse();
        let selection = self.ui.repo_selection.clone();
        let targets: Vec<RootId> = self
            .multi
            .roots
            .iter()
            .filter(|r| r.id != source)
            .filter(|r| selection.is_empty() || selection.contains(&r.id))
            .map(|r| r.id.clone())
            .collect();
        let dlg = &mut self.ui.dlg;
        dlg.cherry_source = Some(source);
        dlg.cherry_candidates = candidates;
        dlg.cherry_commits = Vec::new();
        dlg.cherry_targets = targets;
        dlg.cherry_search.clear();
        dlg.cherry_stop_on_conflict = true;
        dlg.cherry_focus = None;
        dlg.cherry_preview = None;
        dlg.cherry_forecast = None;
        self.cherry_reforecast();
    }

    /// Toggle one candidate commit in the dialog's checkbox list (issue 16);
    /// the selection stays in application order and the forecast recomputes.
    pub fn cherry_toggle_commit(&mut self, id: String) {
        let dlg = &mut self.ui.dlg;
        if let Some(pos) = dlg.cherry_commits.iter().position(|c| *c == id) {
            dlg.cherry_commits.remove(pos);
        } else {
            dlg.cherry_commits.push(id);
        }
        let order = dlg
            .cherry_candidates
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        dlg.cherry_commits
            .sort_by_key(|c| order.iter().position(|o| o == c).unwrap_or(usize::MAX));
        self.cherry_reforecast();
    }

    /// Toggle one target repo in the dialog's targets table (issue 16);
    /// the rows stay in registration order and the forecast recomputes.
    pub fn cherry_toggle_target(&mut self, id: RootId) {
        let dlg = &mut self.ui.dlg;
        if let Some(pos) = dlg.cherry_targets.iter().position(|t| *t == id) {
            dlg.cherry_targets.remove(pos);
        } else {
            dlg.cherry_targets.push(id);
        }
        let order: Vec<RootId> = self.multi.roots.iter().map(|r| r.id.clone()).collect();
        let dlg = &mut self.ui.dlg;
        dlg.cherry_targets
            .sort_by_key(|t| order.iter().position(|o| o == t).unwrap_or(usize::MAX));
        self.cherry_reforecast();
    }

    /// Focus a commit in the dialog's patch preview rail (issue 16) and
    /// load its patch text synchronously — one engine read, like the
    /// preflight's live git reads.
    pub fn cherry_focus_commit(&mut self, id: String) {
        let Some(source) = self.ui.dlg.cherry_source.clone() else {
            return;
        };
        let patch = self.executor.diff(
            source.0.as_ref(),
            &DiffOpts {
                commit: Some(id.clone()),
                ..Default::default()
            },
        );
        let dlg = &mut self.ui.dlg;
        dlg.cherry_focus = Some(id);
        dlg.cherry_preview = Some(patch.map_err(|e| e.to_string()));
    }

    /// Recompute the per-target prediction table from the current selection.
    fn cherry_reforecast(&mut self) {
        let Some(source) = self.ui.dlg.cherry_source.clone() else {
            return;
        };
        let source_path = source.0.to_path_buf();
        let picks: Vec<Commit> = self
            .ui
            .dlg
            .cherry_commits
            .iter()
            .filter_map(|id| {
                self.ui
                    .dlg
                    .cherry_candidates
                    .iter()
                    .find(|c| &c.id == id)
                    .cloned()
            })
            .collect();
        let targets: Vec<(RootId, String, PathBuf)> = self
            .ui
            .dlg
            .cherry_targets
            .iter()
            .filter_map(|id| {
                self.multi
                    .by_id(id)
                    .map(|r| (r.id.clone(), r.id.name(), r.path.clone()))
            })
            .collect();
        let forecast = turbogit_services::cherry_across::forecast(
            self.executor.as_ref(),
            &source_path,
            &picks,
            &targets,
        );
        self.ui.dlg.cherry_forecast = Some(forecast);
    }

    /// Execute the dialog's selection (issue 16): one patch application per
    /// commit × repo on the cascade pool — commits applied in order per
    /// target — reported through the cherry run monitor. Forecast-blocked
    /// repos are seeded as skipped rows and never dispatched; a conflicted
    /// repo stays held for manual resolution, never auto-resolved.
    pub fn run_cherry_across(&mut self) {
        let Some(source) = self.ui.dlg.cherry_source.clone() else {
            return;
        };
        let commits = self.ui.dlg.cherry_commits.clone();
        let stop = self.ui.dlg.cherry_stop_on_conflict;
        let targets = self.ui.dlg.cherry_targets.clone();
        if commits.is_empty() || targets.is_empty() {
            return;
        }
        self.ui.dialog = None;
        self.ui.busy = true;
        let blocked: HashMap<RootId, String> = self
            .ui
            .dlg
            .cherry_forecast
            .as_ref()
            .map(|fc| {
                fc.iter()
                    .filter_map(|t| t.blocked.clone().map(|b| (t.root.clone(), b)))
                    .collect()
            })
            .unwrap_or_default();
        let fleet: Vec<(RootId, String, Option<String>)> = targets
            .iter()
            .map(|id| {
                let name = self
                    .multi
                    .by_id(id)
                    .map(|r| r.id.name())
                    .unwrap_or_else(|| id.name());
                (id.clone(), name, blocked.get(id).cloned())
            })
            .collect();
        let dispatch_roots: Vec<RootId> = targets
            .iter()
            .filter(|id| !blocked.contains_key(id))
            .cloned()
            .collect();
        let workers = bulk_run::DEFAULT_WORKERS;
        let control = bulk_run::RunControl::default();
        self.ui.cherry_run = Some(crate::cherry_run_view::CherryRunView {
            source_name: source.name(),
            commit_count: commits.len(),
            stop_on_conflict: stop,
            rows: bulk_run::monitor_rows(&fleet, workers),
            workers,
            started_at: std::time::Instant::now(),
            elapsed: std::time::Duration::ZERO,
            eta: None,
            control: control.clone(),
            history_id: chrono::Utc::now().timestamp_millis() as u64,
        });
        // Snapshot the dispatched roots: the worker owns the paths, `self`
        // stays behind on the UI thread.
        let snapshots: HashMap<RootId, PathBuf> = dispatch_roots
            .iter()
            .filter_map(|id| self.multi.by_id(id).map(|r| (id.clone(), r.path.clone())))
            .collect();
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        let source_path = source.0.to_path_buf();
        let label = format!("cherry-pick across {} repos", targets.len());
        std::thread::spawn(move || {
            let step = |rid: &RootId| -> TgResult<()> {
                let Some(path) = snapshots.get(rid) else {
                    return Err(TgError::Other("root not registered".into()));
                };
                turbogit_services::cherry_across::apply_to_root(
                    executor.as_ref(),
                    &source_path,
                    path,
                    &commits,
                    stop,
                )
            };
            let outcomes = bulk_run::run_cascade(
                &dispatch_roots,
                workers,
                &step,
                &|event| {
                    let _ = tx.send(AppEvent::BulkRunProgress { event });
                },
                &control,
            );
            let results = outcomes
                .into_iter()
                .filter_map(|(root, outcome)| match outcome {
                    bulk_run::RunOutcome::Done(result) => Some((root, result)),
                    bulk_run::RunOutcome::Stopped => None,
                })
                .collect();
            let _ = tx.send(AppEvent::BulkCompleted { label, results });
        });
    }

    /// Resolve deep link for the cherry run monitor (issue 16): the failed
    /// repo becomes the selected root and the monitor closes; the UI opens
    /// the conflict resolver on top when the snapshot shows unresolved
    /// conflicts.
    pub fn cherry_resolve(&mut self, root: RootId) {
        self.selected_root = Some(root);
        self.ui.repo_selection.clear();
        self.ui.cherry_run = None;
    }

    /// Add a linked worktree on `branch` at `path` for the focused root
    /// (issue 14 Worktrees tab add action).
    pub fn add_worktree(&mut self, path: PathBuf, branch: String) {
        let root = self.selected_path();
        let affected = Affected::from_optional_root(root.as_deref());
        self.run_git(format!("Add worktree {branch}"), affected, move |v| {
            if let Some(r) = &root {
                v.worktree_add(r, &path, &branch, true)
            } else {
                Ok(())
            }
        });
    }

    /// `git submodule update [--init] -- <path>` for the focused root
    /// (issue 14 Submodules tab update action; `init` re-checks out an
    /// uninitialized submodule).
    pub fn update_submodule(&mut self, path: PathBuf, init: bool) {
        let root = self.selected_path();
        let affected = Affected::from_optional_root(root.as_deref());
        let label = if init {
            "Update submodule (init)".to_string()
        } else {
            "Update submodule".to_string()
        };
        self.run_git(label, affected, move |v| {
            if let Some(r) = &root {
                v.submodule_update(r, &path, init)
            } else {
                Ok(())
            }
        });
    }

    /// `git init` at the project dir, persist the mapping, then rescan.
    pub fn init_repo(&mut self) {
        if let Err(e) = self.executor.init(&self.project_dir) {
            self.last_error = Some(e.to_string());
            return;
        }
        let _ = crate::persistence::add_mapping(&self.project_dir, &self.project_dir, Vcs::Git);
        self.rescan();
    }

    /// Open `dir` as the active project (issue #10): retarget the project
    /// directory, rediscover roots from scratch, enter the shell, and record
    /// the project in the global recents store (ADR-0005).
    pub fn open_project(&mut self, dir: &Path) {
        self.project_dir = dir.to_path_buf();
        self.multi = MultiRootManager::default();
        self.selected_root = None;
        // Drop every cache entry: the old project's roots must not leak into
        // the new one (bug fix — only logs/ahead-behind were cleared before).
        self.caches.invalidate_all();
        self.rescan();
        self.ui.welcome_visible = false;
        self.record_recent(dir);
    }

    /// Attach `dir` as a workspace root (Welcome "Attach Workspace Root" card,
    /// issue #34): deep-scan the chosen directory tree for every repository —
    /// at any depth, unlike [`Self::open_project`]'s bounded [`rescan`] — and
    /// register them all, then enter the shell and record the root as a
    /// [`crate::recents::RecentKind::Workspace`] recent carrying the indexed
    /// repo count so Welcome can restore it.
    pub fn attach_workspace(&mut self, dir: &Path) {
        self.project_dir = dir.to_path_buf();
        self.multi = MultiRootManager::default();
        self.selected_root = None;
        self.caches.invalidate_all();

        let roots = turbogit_services::multi_root::scan_deep(self.executor.as_ref(), dir);
        let results = turbogit_services::multi_root::register_all(
            self.executor.as_ref(),
            &mut self.multi,
            &roots,
        );
        for r in &results {
            if let Err(e) = r {
                self.last_error = Some(e.to_string());
            }
        }
        if self.selected_root.is_none() {
            self.selected_root = self.multi.roots.first().map(|r| r.id.clone());
        }
        self.ui.welcome_visible = false;

        let count = self.multi.roots.len();
        if let Some(cfg) = self.recents_config() {
            let recents = crate::recents::record_workspace(&cfg, dir, count);
            self.ui.recent_projects = recents.projects;
        }
    }

    /// Create a real repository at `dir` through the engine seam and enter
    /// it (Welcome "Initialize Repository" card, issue #10).
    pub fn initialize_and_enter(&mut self, dir: &Path) {
        if let Err(e) = self.executor.init(dir) {
            self.last_error = Some(e.to_string());
            return;
        }
        let _ = crate::persistence::add_mapping(dir, dir, Vcs::Git);
        self.open_project(dir);
    }

    /// Close every open project and return to the Welcome screen
    /// (File → Welcome, ADR-0004).
    pub fn close_all_projects(&mut self) {
        self.multi = MultiRootManager::default();
        self.selected_root = None;
        self.caches.invalidate_all();
        self.ui.welcome_visible = true;
    }

    /// Upsert `dir` into the global recents store and refresh the in-memory
    /// copy used by the Welcome page.
    pub fn record_recent(&mut self, dir: &Path) {
        if let Some(cfg) = self.recents_config() {
            let recents = crate::recents::record(&cfg, dir);
            self.ui.recent_projects = recents.projects;
        }
    }

    /// Drop the cached branch indicators so the next render recomputes them
    /// live (ADR-0005: never stored, always fresh at render time).
    pub fn invalidate_welcome_branches(&mut self) {
        self.ui.welcome_branch_cache.clear();
    }

    /// Clone `clone_url` into a sibling directory, then rescan.
    pub fn clone_repo(&mut self) {
        let url = self.clone_url.trim().to_string();
        if url.is_empty() {
            return;
        }
        let name = url
            .rsplit('/')
            .next()
            .unwrap_or("repo")
            .trim_end_matches(".git")
            .to_string();
        let dest = self.project_dir.join(&name);
        if let Err(e) = GitExecutor::clone(&*self.executor, &url, &dest, None) {
            self.last_error = Some(e.to_string());
            return;
        }
        let _ = crate::persistence::add_mapping(&self.project_dir, &dest, Vcs::Git);
        self.rescan();
    }

    /// Drain worker-thread events and apply them to state. Production calls
    /// this every frame from `app.rs`; headless harnesses call it for
    /// production parity (issue #13: async diff tests).
    pub fn drain_events(&mut self) -> usize {
        let mut drained = 0usize;
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppEvent::StatusScanned { root, status } => {
                    if let Some(r) = self.multi.roots.iter_mut().find(|r| r.id == root) {
                        match status {
                            Ok(s) => r.status = s,
                            Err(e) => self.last_error = Some(e.to_string()),
                        }
                    }
                }
                AppEvent::LogLoaded { root, commits } => match commits {
                    Ok(c) => self.caches.store_log(root, c),
                    Err(e) => self.last_error = Some(e.to_string()),
                },
                AppEvent::OpCompleted {
                    label,
                    affected,
                    result,
                    retry,
                } => {
                    self.ui.busy = false;
                    match result {
                        Ok(()) => {
                            self.ui.toast = Some(Toast::success(label.clone()));
                            self.refresh(affected.clone());
                            granular::settle(self);
                            // Activity log (issue #04): the durable record
                            // the toast only summarizes. An op that leaves
                            // conflicts behind is a warning, not a success —
                            // the counts are read from the refreshed
                            // snapshots.
                            let conflicts = conflicted_count(self, &affected);
                            let (kind, message) = if conflicts > 0 {
                                (
                                    crate::activity::ActivityKind::Warning,
                                    format!("{label} · {conflicts} unresolved conflicts"),
                                )
                            } else {
                                (crate::activity::ActivityKind::Success, label.clone())
                            };
                            self.ui.activity.push(crate::activity::ActivityEntry {
                                at: chrono::Local::now(),
                                repo: activity_repo_label(&affected),
                                message,
                                kind,
                            });
                        }
                        Err(e) => {
                            let mut t = Toast::error(format!("{label}: {e}"));
                            // Attach the replay handle to the error toast
                            // so the user can retry the exact same op
                            // (issue #02). None → no Retry button.
                            t.retry = retry;
                            self.ui.toast = Some(t);
                            self.last_error = Some(e.to_string());
                            // Activity log (issue #04): failures are entries
                            // too — the toast vanishes, the feed remembers.
                            self.ui.activity.push(crate::activity::ActivityEntry {
                                at: chrono::Local::now(),
                                repo: activity_repo_label(&affected),
                                message: format!("{label}: {e}"),
                                kind: crate::activity::ActivityKind::Error,
                            });
                            granular::on_op_failed(self);
                        }
                    }
                }
                AppEvent::Error(msg) => {
                    self.ui.busy = false;
                    self.last_error = Some(msg);
                }
                AppEvent::DiffReady { key, result } => {
                    self.ui.diff_loading = false;
                    match result {
                        Ok(text) => {
                            self.ui.diff_error = None;
                            self.ui.diff_cache = Some((key, text));
                        }
                        Err(e) => {
                            self.ui.diff_error = Some(e.to_string());
                            if self
                                .ui
                                .diff_cache
                                .as_ref()
                                .map(|(k, _)| k != &key)
                                .unwrap_or(false)
                            {
                                self.ui.diff_cache = None;
                            }
                        }
                    }
                }
                AppEvent::BlameReady { key, result } => {
                    self.ui.blame_loading = false;
                    match result {
                        Ok(lines) => {
                            self.ui.blame_error = None;
                            self.ui.blame_cache = Some((key, lines));
                        }
                        Err(e) => {
                            self.ui.blame_error = Some(e.to_string());
                            if self
                                .ui
                                .blame_cache
                                .as_ref()
                                .map(|(k, _)| k != &key)
                                .unwrap_or(false)
                            {
                                self.ui.blame_cache = None;
                            }
                        }
                    }
                }
                AppEvent::FileBytesReady { key, old, new } => {
                    // The in-flight slot frees for the next wanted pane even
                    // when this result is already stale.
                    if self.ui.pane_bytes_loading.as_deref() == Some(key.as_str()) {
                        self.ui.pane_bytes_loading = None;
                    }
                    self.ui.pane_bytes.store(
                        key,
                        crate::diff_data::PaneEntry {
                            old: old.map(crate::diff_data::PaneSide::from_blob),
                            new: new.map(crate::diff_data::PaneSide::from_blob),
                        },
                    );
                }
                AppEvent::AheadBehind {
                    root,
                    ahead,
                    behind,
                } => {
                    self.caches.store_ahead_behind(root, (ahead, behind));
                }
                AppEvent::IncomingPolled { root, result } => {
                    self.settle_incoming_poll(root, result);
                }
                AppEvent::WorktreesLoaded { root, worktrees } => {
                    self.fetching_worktrees.remove(&root);
                    match worktrees {
                        Ok(w) => self.caches.store_worktrees(root, w),
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
                AppEvent::SubmodulesLoaded { root, submodules } => {
                    self.fetching_submodules.remove(&root);
                    match submodules {
                        Ok(s) => self.caches.store_submodules(root, s),
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
                AppEvent::BulkCompleted { label, results } => {
                    self.ui.busy = false;
                    self.refresh(Affected::All);
                    let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
                    let kind = if ok == results.len() {
                        ToastKind::Success
                    } else if ok > 0 {
                        ToastKind::Warning
                    } else {
                        ToastKind::Error
                    };
                    let message = bulk_report_message(&label, &results);
                    self.ui.toast = Some(Toast {
                        kind,
                        message: message.clone(),
                        retry: None,
                    });
                    // The remotes manager renders the per-repo outcomes of
                    // its apply (issue 33): display-ready rows in selection
                    // order. Harmless for the bulk grid's own runs.
                    self.ui.dlg.remotes_apply_results = Some(
                        results
                            .iter()
                            .map(|(id, r)| {
                                let name =
                                    id.0.file_name()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("?")
                                        .to_string();
                                (name, r.clone().map_err(|e| e.to_string()))
                            })
                            .collect(),
                    );
                    // The durable record of the fleet run (issue #04
                    // semantics): one entry with the aggregate outcome.
                    self.ui.activity.push(crate::activity::ActivityEntry {
                        at: chrono::Local::now(),
                        repo: Some("project".to_string()),
                        message,
                        kind: match kind {
                            ToastKind::Success => crate::activity::ActivityKind::Success,
                            ToastKind::Warning => crate::activity::ActivityKind::Warning,
                            _ => crate::activity::ActivityKind::Error,
                        },
                    });
                    // Recent bulk operations (issue 12, screen 04): the
                    // completed run lands in the history list — merged into
                    // the same record on a retry pass. The monitor's rows
                    // are the run's final truth (done/failed/skipped).
                    if let Some(view) = self.ui.bulk_run.as_ref() {
                        let at = chrono::Utc::now().timestamp_millis();
                        let record = crate::bulk_history::from_run_view(view, at);
                        crate::bulk_history::record_into(&mut self.ui.bulk_history, record);
                        self.persist_ui();
                    }
                }
                AppEvent::BulkRunProgress { event } => {
                    // The pool event advances whichever run monitor is
                    // live: the bulk grid's run or a cherry-pick-across
                    // run (issue 16).
                    if let Some(view) = self.ui.bulk_run.as_mut() {
                        if advance_run_rows(&mut view.rows, event) {
                            view.update_progress();
                        }
                    } else if let Some(view) = self.ui.cherry_run.as_mut()
                        && advance_run_rows(&mut view.rows, event)
                    {
                        view.update_progress();
                    }
                }
                _ => {}
            }
            drained += 1;
        }
        drained
    }

    /// The currently selected root's path (or None).
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_root.as_ref().map(|r| r.0.to_path_buf())
    }

    /// The push dialog's resolved PUSH SCOPE roots (issue #25), in
    /// registration order, minus any roots the user removed from scope via
    /// the protected-branch remediation banner.
    pub fn push_scope_roots(&self) -> Vec<Root> {
        let excluded = &self.ui.dlg.push_scope_excluded;
        turbogit_services::sync_service::roots_in_scope(
            self.ui.dlg.push_scope,
            &self.multi.roots,
            &self.ui.repo_selection,
            self.selected_root.as_ref(),
            &self.project_dir,
        )
        .into_iter()
        .filter(|r| !excluded.contains(&r.id))
        .cloned()
        .collect()
    }

    /// "Pin as view" (issue #08): save the current multi-repo selection as
    /// a named view and persist it with the workspace. Nothing selected →
    /// nothing pinned.
    pub fn pin_selection(&mut self) {
        if self.ui.repo_selection.is_empty() {
            return;
        }
        let repos: Vec<PathBuf> = self
            .ui
            .repo_selection
            .iter()
            .map(|id| id.0.to_path_buf())
            .collect();
        self.ui.pinned_views.push(crate::pinned_views::PinnedView {
            name: crate::pinned_views::next_name(&self.ui.pinned_views),
            repos,
        });
        self.persist_ui();
    }

    // -- Bulk operations grid (issue 09) ------------------------------------

    /// The selected roots that are registered, in registration order.
    fn bulk_selected_roots(&self) -> Vec<Root> {
        self.multi
            .roots
            .iter()
            .filter(|r| self.ui.repo_selection.contains(&r.id))
            .cloned()
            .collect()
    }

    /// The preflight matrix (issue 09, screen 02) for `op` over the current
    /// selection: per-repo current state, predicted outcome, and skip
    /// reason. Ahead/behind comes from the root caches (a root with no
    /// cached counts reads as (0, 0)).
    pub fn bulk_preflight(&self, op: BulkOp) -> Preflight {
        let roots = self.bulk_selected_roots();
        bulk_ops::preflight(op, &roots, &|id| {
            self.caches.ahead_behind(id).unwrap_or((0, 0))
        })
    }

    /// The cascade create-&-checkout preflight matrix (issue 11, screen 02)
    /// for the branch name typed into the modal: per-repo current branch,
    /// target, and predicted action. Ahead/behind comes from the root
    /// caches, the apply-broadly policy from [`UiState::bulk_from_upstream`].
    pub fn bulk_branch_preflight(&self) -> bulk_ops::BranchPreflight {
        let roots = self.bulk_selected_roots();
        bulk_ops::branch_preflight(
            &roots,
            self.ui.bulk_branch_name.trim(),
            self.ui.bulk_from_upstream,
            &|id| self.caches.ahead_behind(id).unwrap_or((0, 0)),
        )
    }

    // -- Cascade commit (issue 21, screen 06) -------------------------------
    /// The cascade-commit rail footer text (issue 21): the "Commit M hunks"
    /// label that scopes the "Also commit on N selected repos" button. When
    /// at least one selected repo has nothing staged, the footer appends a
    /// "skips repos with nothing staged" clause so the user sees the skip
    /// before they confirm. Reads live status — never cached — so the count
    /// matches what the cascade step will actually find.
    pub fn commit_rail_footer(&self, message: &str) -> String {
        let paths: Vec<PathBuf> = self
            .ui
            .repo_selection
            .iter()
            .filter_map(|id| self.multi.by_id(id))
            .map(|root| root.path.clone())
            .collect();
        if paths.is_empty() {
            return if message.trim().is_empty() {
                "No selected repos".to_string()
            } else {
                "No selected repos — nothing to commit".to_string()
            };
        }
        let rows = commit_across::plan(self.executor.as_ref(), &paths);
        let total: usize = rows.iter().filter_map(|r| r.outcome.as_ref().ok()).sum();
        let skip = rows.iter().filter(|r| r.outcome.is_err()).count();
        let mut label = if total == 0 {
            "Nothing to commit".to_string()
        } else {
            format!(
                "Commit {total} {}",
                if total == 1 { "hunk" } else { "hunks" }
            )
        };
        if skip > 0 {
            label.push_str(" · skips repos with nothing staged");
        }
        label
    }

    /// Execute the cascade-commit step over the current multi-repo selection
    /// (issue 21): one per-repo commit (or amend) per root that has staged
    /// content. Skipped roots (clean index) seed the monitor as
    /// pre-skipped rows and are never dispatched. Per-repo outcomes flow
    /// through the cascade-run pool ([`AppEvent::BulkRunProgress`]) and the
    /// completion summary ([`AppEvent::BulkCompleted`]) — the same pipeline
    /// the operations grid uses (issue 09). `message` and `amend` are
    /// captured for the duration of the worker pool pass.
    pub fn run_commit_across(&mut self, message: &str, amend: bool) {
        let roots = self.bulk_selected_roots();
        if roots.is_empty() {
            return;
        }
        let paths: Vec<PathBuf> = roots.iter().map(|r| r.path.clone()).collect();
        let rows = commit_across::plan(self.executor.as_ref(), &paths);
        let planned_ids: Vec<RootId> = rows
            .iter()
            .filter(|r| r.outcome.is_ok())
            .map(|r| r.root.clone())
            .collect();
        if planned_ids.is_empty() {
            self.ui.toast = Some(Toast {
                kind: ToastKind::Warning,
                message: "No selected repos have anything staged".to_string(),
                retry: None,
            });
            return;
        }
        let fleet: Vec<(RootId, String, Option<String>)> = rows
            .iter()
            .map(|r| {
                (
                    r.root.clone(),
                    r.name.clone(),
                    match r.outcome {
                        Ok(_) => None,
                        Err(reason) => Some(reason.label().to_string()),
                    },
                )
            })
            .collect();
        let planned_for_pool = planned_ids.clone();
        let label = if amend {
            format!("Amend on {len} repos", len = planned_ids.len())
        } else {
            format!("Commit on {len} repos", len = planned_ids.len())
        };
        let executor = self.executor.clone();
        let message = message.to_string();
        let tx = self.tx.clone();
        let workers = bulk_run::DEFAULT_WORKERS;
        let control = bulk_run::RunControl::default();
        let label_for_view = label.clone();
        self.ui.bulk_run = Some(crate::bulk_run_view::BulkRunView {
            op: BulkOp::Commit,
            rebase: amend,
            branch: String::new(),
            command: String::new(),
            merge_opts: MergeOpts::default(),
            rows: bulk_run::monitor_rows(&fleet, workers),
            workers,
            started_at: std::time::Instant::now(),
            elapsed: std::time::Duration::ZERO,
            eta: None,
            control: control.clone(),
            history_id: chrono::Utc::now().timestamp_millis() as u64,
            undo: Vec::new(),
        });
        let label_for_event = label.clone();
        std::thread::spawn(move || {
            let step = |rid: &RootId| -> TgResult<()> {
                let path = roots
                    .iter()
                    .find(|r| r.id == *rid)
                    .map(|r| r.path.clone())
                    .ok_or_else(|| TgError::Other("root not registered".into()))?;
                commit_across::run_one(executor.as_ref(), &path, &message, amend).map(|_| ())
            };
            let on_event = |event: bulk_run::RunEvent| {
                let _ = tx.send(AppEvent::BulkRunProgress { event });
            };
            let outcomes =
                bulk_run::run_cascade(&planned_for_pool, workers, &step, &on_event, &control);
            let results: Vec<(RootId, TgResult<()>)> = outcomes
                .into_iter()
                .map(|(rid, outcome)| {
                    let result = match outcome {
                        bulk_run::RunOutcome::Done(r) => r,
                        bulk_run::RunOutcome::Stopped => Err(TgError::Other("stopped".into())),
                    };
                    (rid, result)
                })
                .collect();
            let _ = tx.send(AppEvent::BulkCompleted {
                label: label_for_event,
                results,
            });
        });
        // Surface a small banner so the user sees the run kicked off even
        // before the monitor paints. The monitor is the durable surface.
        let _ = label_for_view; // silence the unused-binding lint when banner is gated below
    }

    /// Open the preflight matrix modal for `op`, seeding the pull policy
    /// control from the configured update method — and the apply-broadly
    /// policy to on for the create-&-checkout cascade.
    pub fn open_bulk_preflight(&mut self, op: BulkOp) {
        self.ui.bulk_op = Some(op);
        self.ui.bulk_rebase = self.settings.update_method == UpdateMethod::Rebase;
        if op == BulkOp::CreateBranch {
            self.ui.bulk_from_upstream = true;
        }
        if op == BulkOp::Custom {
            // Fresh modal (issue 13): empty command, disarmed confirmation.
            self.ui.bulk_command = String::new();
            self.ui.bulk_command_armed = false;
        }
    }

    /// The merge dialog's chosen executor-facing options (issue 28): the
    /// STRATEGY selection mapped by
    /// [`turbogit_services::integrate_service::merge_flags`] with the
    /// option rows layered on top.
    pub fn merge_dialog_opts(&self) -> MergeOpts {
        turbogit_services::integrate_service::merge_flags(
            self.ui.dlg.merge_strategy,
            self.ui.dlg.merge_no_ff,
            self.ui.dlg.merge_verify_signatures,
            self.ui.dlg.merge_allow_unrelated,
        )
    }

    /// The sibling repos sharing the focused root's current branch (issue
    /// 28's cascade banner): a merge on one of them implies the same merge
    /// on the others. Empty when nothing is focused or no repo shares the
    /// branch.
    pub fn merge_cascade_siblings(&self) -> Vec<RootId> {
        let Some(focused) = self.selected_root.clone() else {
            return Vec::new();
        };
        turbogit_services::integrate_service::cascade_siblings(&self.multi.roots, &focused)
    }

    /// The merge dialog's cascade hand-off (issue 28, screen 14's "View plan
    /// →"): scope the fleet selection to the sibling repos that share the
    /// branch being merged into and open the cascade preflight modal for
    /// them. The focused repo is never part of its own cascade plan.
    pub fn open_merge_cascade_plan(&mut self) {
        let Some(focused) = self.selected_root.clone() else {
            return;
        };
        let siblings =
            turbogit_services::integrate_service::cascade_siblings(&self.multi.roots, &focused);
        if siblings.is_empty() {
            return;
        }
        self.ui.repo_selection = siblings.into_iter().collect();
        self.ui.bulk_op = Some(BulkOp::Merge);
    }

    /// The rebase dialog's chosen executor-facing options (issue 29): the
    /// MODE selection mapped by
    /// [`turbogit_services::integrate_service::rebase_mode_opts`] with the
    /// option rows layered on top.
    pub fn rebase_dialog_opts(&self) -> turbogit_domain::model::RebaseOpts {
        turbogit_services::integrate_service::rebase_mode_opts(
            self.ui.dlg.rebase_mode,
            self.ui.dlg.rebase_update_refs,
            self.ui.dlg.rebase_keep_empty,
            self.ui.dlg.rebase_autosquash,
        )
    }

    /// The repos affected by rewriting the focused root's current branch
    /// (issue 29's cross-repo banner): siblings where the branch is checked
    /// out or tracking-shared. Empty when nothing is focused.
    pub fn rebase_affected_siblings(&self) -> Vec<RootId> {
        let Some(focused) = self.selected_root.clone() else {
            return Vec::new();
        };
        turbogit_services::integrate_service::rebase_affected(&self.multi.roots, &focused)
    }

    /// The rebase dialog's "View affected →" hand-off (issue 29, screen
    /// 15): scope the fleet selection to the affected repos — the
    /// affected-repo list — and close the dialog so the workspace tree
    /// shows exactly which repos the rewrite touches.
    pub fn open_rebase_affected_list(&mut self) {
        let siblings = self.rebase_affected_siblings();
        if siblings.is_empty() {
            return;
        }
        self.ui.repo_selection = siblings.into_iter().collect();
        self.ui.dialog = None;
    }

    /// Whether the focused root's current branch is protected (issue 29):
    /// history rewrites are blocked on protected branches, so the rebase
    /// dialog's start button gates on this.
    pub fn rebase_branch_is_protected(&self) -> bool {
        let Some(id) = &self.selected_root else {
            return false;
        };
        let Some(root) = self.multi.by_id(id) else {
            return false;
        };
        root.current_branch
            .as_deref()
            .is_some_and(|b| turbogit_services::sync_service::is_protected(&self.settings, b))
    }

    // ---- branches popup row actions (issue 32) ----

    /// Open the rename dialog for `name` in `id`, closing the popup so
    /// focus follows cleanly (mirrors the New Branch flow).
    pub fn open_rename_branch(&mut self, id: &RootId, name: &str) {
        self.ui.dlg.rename_branch_root = Some(id.clone());
        self.ui.dlg.rename_branch_name = name.to_string();
        self.ui.dlg.rename_branch_new = String::new();
        self.ui.dialog = Some(Dialog::RenameBranch);
        self.ui.branches_popup = false;
    }

    /// Rename `old` to `new` in `id` through the engine seam and refresh
    /// the root. The current-branch rename is legal; git updates HEAD.
    pub fn rename_branch(&mut self, id: &RootId, old: &str, new: &str) {
        let path = id.0.clone();
        let affected = Affected::Root(id.clone());
        let old = old.to_string();
        let new = new.to_string();
        self.run_git(format!("Rename {old} → {new}"), affected, move |v| {
            v.branch_rename(&path, &old, &new)
        });
    }

    /// Open the compare dialog for `other` vs the root's current branch
    /// (spec E9: commits in the selected branch missing from the current
    /// one). The commit list is snapshotted once, when the dialog opens.
    pub fn open_compare(&mut self, id: &RootId, other: &str) {
        let Some(current) = self.multi.by_id(id).and_then(|r| r.current_branch.clone()) else {
            return; // detached HEAD: nothing to compare against
        };
        let path = id.0.clone();
        let commits = self
            .executor
            .outgoing_commits(&path, other, &current)
            .unwrap_or_default();
        self.ui.dlg.compare_root = Some(id.clone());
        self.ui.dlg.compare_left = other.to_string();
        self.ui.dlg.compare_right = current;
        self.ui.dlg.compare_commits = commits;
        self.ui.dialog = Some(Dialog::CompareBranches);
        self.ui.branches_popup = false;
    }

    /// Protected-row hover quick-action: open the merge dialog preset to
    /// merge `name` into the current branch.
    pub fn open_merge_into(&mut self, _id: &RootId, name: &str) {
        self.ui.dlg.merge_target = name.to_string();
        self.ui.dlg.merge_source_picker_open = false;
        self.ui.dlg.merge_preview = None;
        self.ui.dialog = Some(Dialog::Merge);
        self.ui.branches_popup = false;
    }

    /// Is the command typed into the custom-command modal destructive-looking
    /// (issue 13: reset, clean, history rewrites, force pushes, `-D`)? A
    /// destructive command needs the two-stage confirmation before it may
    /// run across the fleet.
    pub fn bulk_command_is_destructive(&self) -> bool {
        bulk_ops::parse_command(&self.ui.bulk_command)
            .map(|args| bulk_ops::destructive_reason(&args).is_some())
            .unwrap_or(false)
    }

    /// Remember a confirmed custom command (issue 13): newest first,
    /// deduplicated (a re-run moves it back to the front), capped at
    /// [`MAX_RECENT_CUSTOM_COMMANDS`], persisted with the workspace.
    pub fn record_custom_command(&mut self, command: &str) {
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        self.ui.recent_custom_commands.retain(|c| c != command);
        self.ui
            .recent_custom_commands
            .insert(0, command.to_string());
        self.ui
            .recent_custom_commands
            .truncate(MAX_RECENT_CUSTOM_COMMANDS);
        self.persist_ui();
    }

    /// Run a confirmed bulk plan (the preflight matrix's will-run rows) on a
    /// bounded worker pool (issue 10): the live monitor opens immediately —
    /// one row per selected repo, preflight skips seeded with their reason —
    /// and advances through [`AppEvent::BulkRunProgress`] events. Pass
    /// completion arrives as [`AppEvent::BulkCompleted`] and drains into an
    /// aggregate toast + activity entry; the monitor stays open.
    pub fn run_bulk_confirmed(&mut self, plan: BulkPlan) {
        self.ui.bulk_op = None;
        self.ui.busy = true;
        // Recent custom commands (issue 13): a confirmed command is recorded
        // at dispatch time, so it is re-selectable even if the run fails.
        if plan.op == BulkOp::Custom {
            let command = plan.command.clone();
            self.record_custom_command(&command);
        }

        // Seed the monitor rows: one per selected repo (plan roots not in
        // the current selection included), in preflight order. Rows the
        // plan excludes are skipped, labeled by the preflight reason. The
        // create-&-checkout cascade seeds from its own matrix, whose
        // predictions carry the existing-branch checkout and the
        // apply-broadly base.
        let mut fleet: Vec<(RootId, String, Option<String>)> = match plan.op {
            BulkOp::CreateBranch => {
                let roots = self.bulk_selected_roots();
                let bpf = bulk_ops::branch_preflight(&roots, &plan.branch, plan.rebase, &|id| {
                    self.caches.ahead_behind(id).unwrap_or((0, 0))
                });
                bpf.rows
                    .iter()
                    .map(|row| {
                        let skip = if plan.roots.contains(&row.root) {
                            None
                        } else {
                            Some(match row.action {
                                bulk_ops::BranchAction::Skip(reason) => reason.label().to_string(),
                                _ => "out of scope".to_string(),
                            })
                        };
                        (row.root.clone(), row.name.clone(), skip)
                    })
                    .collect()
            }
            _ => {
                let pf = self.bulk_preflight(plan.op);
                pf.rows
                    .iter()
                    .map(|row| {
                        let skip = if plan.roots.contains(&row.root) {
                            None
                        } else {
                            Some(match row.outcome {
                                Err(reason) => reason.label().to_string(),
                                Ok(()) => "out of scope".to_string(),
                            })
                        };
                        (row.root.clone(), row.name.clone(), skip)
                    })
                    .collect()
            }
        };
        for root in &plan.roots {
            if !fleet.iter().any(|(r, _, _)| r == root) {
                fleet.push((root.clone(), root.name(), None));
            }
        }
        let workers = bulk_run::DEFAULT_WORKERS;
        let control = bulk_run::RunControl::default();
        // Undo capture (issue 12): a branch cascade records each repo's
        // prior checkout and whether the run creates the branch, so
        // rollback can delete created branches and restore checkouts.
        let undo: Vec<crate::bulk_history::UndoRow> = if plan.op == BulkOp::CreateBranch {
            let roots = self.bulk_selected_roots();
            let bpf = bulk_ops::branch_preflight(&roots, &plan.branch, plan.rebase, &|id| {
                self.caches.ahead_behind(id).unwrap_or((0, 0))
            });
            bpf.rows
                .iter()
                .filter(|row| plan.roots.contains(&row.root))
                .map(|row| crate::bulk_history::UndoRow {
                    root: row.root.as_path().to_path_buf(),
                    prior_branch: row.branch.clone(),
                    created: !matches!(row.action, bulk_ops::BranchAction::CheckoutExisting),
                })
                .collect()
        } else {
            Vec::new()
        };
        // Cascade merge (issue 28): the options live in the merge dialog's
        // state; snapshot them onto the run so Retry re-dispatches exactly
        // what was confirmed.
        let merge_opts = if plan.op == BulkOp::Merge {
            self.merge_dialog_opts()
        } else {
            MergeOpts::default()
        };
        self.ui.bulk_run = Some(crate::bulk_run_view::BulkRunView {
            op: plan.op,
            rebase: plan.rebase,
            branch: plan.branch.clone(),
            command: plan.command.clone(),
            merge_opts: merge_opts.clone(),
            rows: bulk_run::monitor_rows(&fleet, workers),
            workers,
            started_at: std::time::Instant::now(),
            elapsed: std::time::Duration::ZERO,
            eta: None,
            control: control.clone(),
            history_id: chrono::Utc::now().timestamp_millis() as u64,
            undo,
        });
        let roots = plan.roots.clone();
        self.dispatch_bulk_pass(plan, merge_opts, roots, control);
    }

    /// Dispatch one pool pass of the monitor's operation over `roots`
    /// (issue 10): the initial run or a Retry-skipped re-dispatch. Progress
    /// posts as [`AppEvent::BulkRunProgress`]; the pass's outcome completes
    /// the run with [`AppEvent::BulkCompleted`] (stopped roots report no
    /// result — they are visible in the monitor as skipped). For a
    /// create-&-checkout run `branch` is the target branch name and
    /// `rebase` is the apply-broadly policy.
    fn dispatch_bulk_pass(
        &mut self,
        plan: BulkPlan,
        merge_opts: MergeOpts,
        roots: Vec<RootId>,
        control: bulk_run::RunControl,
    ) {
        let bulk_ops::BulkPlan {
            op,
            rebase,
            branch,
            command,
            ..
        } = plan.clone();
        self.ui.busy = true;
        // Snapshot the planned roots: the worker owns them, `self.multi`
        // stays behind on the UI thread.
        let snapshots: HashMap<RootId, Root> = roots
            .iter()
            .filter_map(|rid| self.multi.by_id(rid).map(|r| (rid.clone(), r.clone())))
            .collect();
        let settings = self.settings.clone();
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        let workers = bulk_run::DEFAULT_WORKERS;
        std::thread::spawn(move || {
            let plan = bulk_ops::BulkPlan {
                op,
                roots: Vec::new(),
                rebase,
                branch,
                command,
            };
            let step = |rid: &RootId| -> TgResult<()> {
                let Some(root) = snapshots.get(rid) else {
                    return Err(TgError::Other("root not registered".into()));
                };
                bulk_ops::run_step(op, executor.as_ref(), root, &plan, &settings, &merge_opts)
            };
            let outcomes = bulk_run::run_cascade(
                &roots,
                workers,
                &step,
                &|event| {
                    let _ = tx.send(AppEvent::BulkRunProgress { event });
                },
                &control,
            );
            let results = outcomes
                .into_iter()
                .filter_map(|(root, outcome)| match outcome {
                    bulk_run::RunOutcome::Done(result) => Some((root, result)),
                    bulk_run::RunOutcome::Stopped => None,
                })
                .collect();
            let _ = tx.send(AppEvent::BulkCompleted {
                label: op.label().to_string(),
                results,
            });
        });
    }

    /// Stop remaining (issue 10, screen 03): halt the in-flight pass's
    /// queued work. Running and completed roots are never undone.
    pub fn bulk_stop_remaining(&mut self) {
        if let Some(view) = &self.ui.bulk_run {
            view.control.stop();
        }
    }

    /// Retry skipped (issue 10, screen 03): re-dispatch every currently
    /// skipped row through a fresh pool pass — preflight skips included,
    /// forcing the attempt the policy refused.
    pub fn bulk_retry_skipped(&mut self) {
        let Some(view) = self.ui.bulk_run.as_ref() else {
            return;
        };
        let roots: Vec<RootId> = view
            .rows
            .iter()
            .filter(|r| matches!(r.state, bulk_run::RowState::Skipped { .. }))
            .map(|r| r.root.clone())
            .collect();
        if roots.is_empty() {
            return;
        }
        let plan = BulkPlan {
            op: view.op,
            roots: Vec::new(),
            rebase: view.rebase,
            branch: view.branch.clone(),
            command: view.command.clone(),
        };
        let merge_opts = view.merge_opts.clone();
        let control = bulk_run::RunControl::default();
        let view = self.ui.bulk_run.as_mut().unwrap();
        view.requeue(&roots);
        view.control = control.clone();
        self.dispatch_bulk_pass(plan, merge_opts, roots, control);
    }

    /// Resolve (issue 10, screen 03): jump from a failed monitor row to the
    /// repo in question — the row's repo becomes the selected root (the
    /// fleet selection clears, so the repo's own surface is shown), and the
    /// monitor closes. When the repo's refreshed snapshot shows unresolved
    /// conflicts, the UI layer opens the conflict resolver on top of the
    /// navigation.
    pub fn bulk_resolve(&mut self, root: RootId) {
        self.selected_root = Some(root);
        self.ui.repo_selection.clear();
        self.ui.bulk_run = None;
    }

    /// Rollback (issue 12, screen 04): undo a completed run's effect for the
    /// operations that are safely reversible — branch cascades. Created
    /// branches are deleted and the prior checkouts restored; a branch that
    /// already existed keeps its tip and only the checkout moves back.
    /// Irreversible operations (fetch/pull/push/stash), unknown ids, and
    /// already-rolled-back records are no-ops. Repos the run failed or
    /// skipped are never touched.
    pub fn bulk_rollback(&mut self, record_id: u64) {
        let Some(rec) = self
            .ui
            .bulk_history
            .iter()
            .find(|r| r.id == record_id)
            .cloned()
        else {
            return;
        };
        if !rec.reversible() || rec.rolled_back {
            return;
        }
        let mut failures = 0usize;
        for row in &rec.repos {
            if row.outcome != crate::bulk_history::RepoOutcome::Done {
                continue;
            }
            let done = if row.created {
                // Restore the prior checkout first so the created branch is
                // not the current one when it is deleted.
                let checked_out = match &row.prior_branch {
                    Some(prior) => {
                        branch_service::checkout(self.executor.as_ref(), &row.root, prior).is_ok()
                    }
                    None => false,
                };
                checked_out
                    && branch_service::delete(self.executor.as_ref(), &row.root, &rec.branch, true)
                        .is_ok()
            } else {
                match &row.prior_branch {
                    Some(prior) => {
                        branch_service::checkout(self.executor.as_ref(), &row.root, prior).is_ok()
                    }
                    None => true,
                }
            };
            if !done {
                failures += 1;
            }
        }
        if failures > 0 {
            self.ui.toast = Some(Toast {
                kind: ToastKind::Error,
                message: format!("Rollback incomplete · {} repo(s) not restored", failures),
                retry: None,
            });
            return;
        }
        if let Some(rec) = self.ui.bulk_history.iter_mut().find(|r| r.id == record_id) {
            rec.rolled_back = true;
        }
        self.ui.toast = Some(Toast {
            kind: ToastKind::Success,
            message: format!("Rolled back · {}", rec.branch),
            retry: None,
        });
        self.refresh(Affected::All);
        self.persist_ui();
    }

    /// Welcome-vs-shell routing (issue #9, spec §9.2): the central body shows
    /// the Welcome page when no repository root is open, or when the user
    /// explicitly returned to it (File → Welcome). A project opened at launch
    /// (`turbogit <path>`) enters the shell directly.
    pub fn show_welcome(&self) -> bool {
        self.multi.roots.is_empty() || self.ui.welcome_visible
    }
}

/// Repo label for one activity entry (issue #04): the affected root's
/// display name, or `None` when the op spanned every root of the project.
fn activity_repo_label(affected: &Affected) -> Option<String> {
    match affected {
        Affected::All => None,
        Affected::Root(id) => Some(crate::activity::root_label(&id.0)),
    }
}

/// Unresolved conflict files across `affected`'s registered roots, read
/// from the (refreshed) snapshots.
fn conflicted_count(state: &AppState, affected: &Affected) -> usize {
    state
        .multi
        .roots
        .iter()
        .filter(|r| match affected {
            Affected::All => true,
            Affected::Root(id) => &r.id == id,
        })
        .map(|r| r.status.conflicted.len())
        .sum()
}

/// First 7 chars of a commit id for op labels (issue 15).
/// Apply one cascade pool event to the run monitor's rows; `true` when a
/// step finished (so the caller refreshes the elapsed/ETA snapshots). The
/// same pool drives the bulk grid's runs and cherry-pick-across runs.
fn advance_run_rows(rows: &mut [bulk_run::RunRow], event: bulk_run::RunEvent) -> bool {
    match event {
        bulk_run::RunEvent::Started { root } => {
            if let Some(row) = rows.iter_mut().find(|r| r.root == root) {
                row.state = bulk_run::RowState::Running;
            }
            false
        }
        bulk_run::RunEvent::Finished {
            root,
            result,
            duration,
        } => {
            if let Some(row) = rows.iter_mut().find(|r| r.root == root) {
                row.state = match result {
                    Ok(()) => bulk_run::RowState::Done,
                    Err(e) => bulk_run::RowState::Failed {
                        error: e.to_string(),
                    },
                };
                row.duration = Some(duration);
            }
            true
        }
        bulk_run::RunEvent::Stopped { root } => {
            if let Some(row) = rows.iter_mut().find(|r| r.root == root) {
                row.state = bulk_run::RowState::Skipped {
                    reason: "stopped".to_string(),
                };
            }
            false
        }
    }
}

fn short_sha(id: &str) -> String {
    id.chars().take(7).collect()
}

/// Ahead/behind of `root`'s current branch vs its upstream (Epic D3);
/// `(0, 0)` when no local branch with an upstream is checked out. Shared by
/// the asynchronous rescan and the synchronous refresh paths so both fill
/// the cache identically.
fn current_branch_ahead_behind(exec: &dyn GitExecutor, root: &Path) -> TgResult<(usize, usize)> {
    match crate::polling::current_branch_upstream(exec, root)? {
        Some((branch, upstream)) => exec.ahead_behind(root, &branch, &upstream),
        None => Ok((0, 0)),
    }
}

/// Aggregate per-repo bulk outcomes into the summary message (issue 09):
/// `"{label} · {ok} of {n} ok"`, plus the failed roots by name when any.
pub fn bulk_report_message(label: &str, results: &[(RootId, TgResult<()>)]) -> String {
    let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
    let mut msg = format!("{label} · {ok} of {} ok", results.len());
    if ok < results.len() {
        let failed: Vec<String> = results
            .iter()
            .filter(|(_, r)| r.is_err())
            .filter_map(|(id, _)| id.0.file_name().and_then(|s| s.to_str()))
            .map(str::to_string)
            .collect();
        msg.push_str(&format!(" · failed: {}", failed.join(", ")));
    }
    msg
}
