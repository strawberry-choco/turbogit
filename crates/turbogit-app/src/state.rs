use crate::events::{AppEvent, LogBatchMode};
use crate::fetch_admission::InflightFetch;
use crate::granular;
use crate::operation::Operation;
use crate::root_caches::{
    Affected, LogBatchSettle, LogRead, LogScope, RootCaches, log_batch_plan as window_batch_plan,
    scoped_batch_plan,
};
use crossbeam_channel::{Receiver, Sender, unbounded};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine::build_executor;
use turbogit_engine_api::{GitExecutor, OwnedOperation};
use turbogit_services::branch_service;
use turbogit_services::bulk_ops::{self, BulkOp, BulkPlan, Preflight};
use turbogit_services::bulk_run;
use turbogit_services::commit_across;

enum Pump {
    Spawned,
    Inline,
}

#[cfg(test)]
type HeldRead = Box<dyn FnOnce(Arc<dyn GitExecutor>, Sender<AppEvent>) + Send>;

#[derive(Clone)]
pub struct OutgoingRoot {
    pub id: RootId,
    pub name: String,
    pub commits: Result<Vec<Commit>, String>,
}

#[derive(Clone)]
pub struct PushPreview {
    pub commits: usize,
    pub remotes: usize,
    pub refs: usize,
    pub rejected: usize,
    pub reports: Vec<(String, Result<String, String>)>,
}

pub const MAX_RECENT_CUSTOM_COMMANDS: usize = 8;

pub const LOG_BATCH_SIZE: usize = 50;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NewBranchBase {
    #[default]
    Unset,
    Branch(String),
    Commit(CommitId),
}

impl NewBranchBase {
    pub fn start_point(&self) -> Option<&str> {
        match self {
            Self::Unset => None,
            Self::Branch(name) => Some(name.as_str()),
            Self::Commit(id) => Some(id.as_str()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PatchSource {
    #[default]
    File,
    Clipboard,
}

#[derive(Default)]
pub struct DialogState {
    pub push_remote: String,
    pub push_branch: String,
    pub push_define_remote: bool,
    pub push_new_remote_name: String,
    pub push_new_remote_url: String,
    pub push_remote_add_error: Option<String>,
    pub force_push: bool,
    pub push_scope: turbogit_services::sync_service::PushScope,
    pub push_scope_excluded: HashSet<RootId>,
    pub push_outgoing: Option<Vec<OutgoingRoot>>,
    pub push_preview_root: Option<RootId>,
    pub push_selected_commits: Vec<String>,
    pub push_subset: turbogit_services::sync_service::SubsetPushState,
    pub push_tags: bool,
    pub push_no_verify: bool,
    pub push_set_upstream: bool,
    pub push_preview_output: Option<PushPreview>,
    pub merge_target: String,
    pub merge_no_ff: bool,
    pub merge_no_verify: bool,
    pub merge_strategy: turbogit_domain::model::MergeStrategy,
    pub merge_verify_signatures: bool,
    pub merge_allow_unrelated: bool,
    pub merge_source_picker_open: bool,
    pub merge_preview: Option<turbogit_services::integrate_service::MergePreview>,
    pub merge_preview_key: Option<(String, turbogit_domain::model::MergeStrategy)>,
    pub rebase_onto: String,
    pub rebase_mode: turbogit_domain::model::RebaseMode,
    pub rebase_onto_picker_open: bool,
    pub rebase_keep_empty: bool,
    pub rebase_update_refs: bool,
    pub rebase_autosquash: bool,
    pub rebase_onto_enabled: bool,
    pub rebase_new_base: String,
    pub rebase_old_base: String,
    pub rebase_new_base_check: Option<(String, bool)>,
    pub rebase_old_base_check: Option<(String, bool)>,
    pub rebase_preview: Option<Vec<turbogit_domain::model::RebasePlanEntry>>,
    pub rebase_preview_key: Option<String>,
    pub new_branch_name: String,
    pub new_branch_checkout: bool,
    pub new_branch_base: NewBranchBase,
    pub new_branch_base_picker_open: bool,
    pub rename_branch_root: Option<RootId>,
    pub rename_branch_name: String,
    pub rename_branch_new: String,
    pub rename_branch_upstream: Option<Upstream>,
    pub rename_branch_unset_upstream: bool,
    pub unmerged_view: Option<UnmergedCommits>,
    pub compare_root: Option<RootId>,
    pub compare_left: String,
    pub compare_right: String,
    pub compare_commits: Vec<CommitId>,
    pub get_from_branch_root: Option<RootId>,
    pub get_from_branch_source: String,
    pub get_from_branch_current: String,
    pub get_from_branch_swapped: bool,
    pub get_from_branch_rows: Vec<turbogit_services::integrate_service::BranchFileRow>,
    pub get_from_branch_error: Option<String>,
    pub apply_patch_root: Option<RootId>,
    pub apply_patch_source: PatchSource,
    pub apply_patch_path: String,
    pub apply_patch_text: String,
    pub apply_patch_loaded: String,
    pub apply_patch_loaded_path: String,
    pub apply_patch_patch: Option<turbogit_domain::model::Patch>,
    pub apply_patch_error: Option<String>,
    pub apply_patch_provenance: Option<turbogit_engine_api::patch::PatchProvenance>,
    pub apply_patch_author: String,
    pub apply_patch_message: String,
    pub apply_patch_preflight: Option<turbogit_services::patch_apply::Preflight>,
    pub apply_patch_outcome: Option<turbogit_services::patch_apply::ApplyOutcome>,
    pub apply_patch_report: Option<crate::operation::ApplyReport>,
    pub create_patch_root: Option<RootId>,
    pub create_patch_origin: Option<PatchOrigin>,
    pub create_patch_comment: String,
    pub create_patch_name: String,
    pub create_patch_location: String,
    pub create_patch_to_clipboard: bool,
    pub create_patch_base_path: String,
    pub create_patch_reverse: bool,
    pub create_patch_encoding: PatchEncoding,
    pub create_patch_strip_leading: usize,
    pub create_patch_removable: usize,
    pub create_patch_raw: Option<Vec<u8>>,
    pub create_patch_error: Option<String>,
    pub create_patch_clipboard: Option<String>,
    pub import_patch_path: String,
    pub import_patch_name: String,
    pub import_patch_error: Option<String>,
    pub rewrite_preflight: Option<RewritePreflight>,
    pub reword: Option<RewordDraft>,
    pub reword_message: String,
    pub cherry_pick_commit: Option<String>,
    pub cherry_source: Option<RootId>,
    pub cherry_candidates: Vec<Commit>,
    pub cherry_commits: Vec<String>,
    pub cherry_targets: Vec<RootId>,
    pub cherry_search: String,
    pub cherry_stop_on_conflict: bool,
    pub cherry_focus: Option<String>,
    pub cherry_preview: Option<Result<turbogit_domain::model::Patch, String>>,
    pub cherry_preview_file: usize,
    pub cherry_forecast: Option<Vec<turbogit_services::cherry_across::TargetForecast>>,
    pub tag_name: String,
    pub tag_msg: String,
    pub tag_push: bool,
    pub tag_type: TagType,
    pub tag_target: String,
    pub tag_target_picker_open: bool,
    pub tag_candidates: Option<Vec<Commit>>,
    pub tag_tagger: String,
    pub tag_sign: bool,
    pub tag_existing: Option<Vec<String>>,
    pub tag_signing_key: Option<String>,
    pub tag_signing_key_fetched: bool,
    pub shelve_name: String,
    pub stash_msg: String,
    pub stash_keep: bool,
    pub unshelve_shelf: Option<String>,
    pub unshelve_root: Option<RootId>,
    pub unshelve_remove: bool,
    pub wt_path: String,
    pub wt_branch: String,
    pub rebase_plan: Option<Vec<turbogit_domain::model::RebasePlanEntry>>,
    pub rebase_base: Option<String>,
    pub rebase_tab: RebaseEditorTab,
    pub rebase_selected: usize,
    pub rebase_cautions: Vec<turbogit_services::history_editor::RebaseCaution>,
    pub rebase_todo: String,
    pub rebase_drag_from: Option<usize>,
    pub rebase_todo_error: Option<String>,
    pub remotes_add_name: String,
    pub remotes_add_fetch: String,
    pub remotes_add_push: String,
    pub remotes_row_action: Option<(String, RemoteRowAction)>,
    pub remotes_rename_new: String,
    pub remotes_edit_fetch: String,
    pub remotes_edit_push: String,
    pub remotes_apply_scope: std::collections::HashSet<RootId>,
    pub remotes_apply_results: Option<Vec<(String, Result<(), String>)>>,
    pub remotes_apply_update: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemoteRowAction {
    #[default]
    None,
    Rename,
    EditUrl,
    SetUpstream,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TagType {
    Lightweight,
    #[default]
    Annotated,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RebaseEditorTab {
    #[default]
    Plan,
    Preview,
    Log,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    #[default]
    Commit,
    Log,
    Branches,
    Worktrees,
    Submodules,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommitSubTab {
    #[default]
    LocalChanges,
    Shelf,
    Stash,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BulkApplyScope {
    #[default]
    ActiveChoice,
    Ours,
    Theirs,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictBulkReport {
    AllResolved,
    NoneResolved,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BranchesGroups {
    pub recent: bool,
    pub local: bool,
    pub remote: bool,
    pub tags: bool,
}

impl Default for BranchesGroups {
    fn default() -> Self {
        Self {
            recent: true,
            local: true,
            remote: true,
            tags: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeState {
    pub groups: BranchesGroups,
    pub show_remotes: bool,
    pub remotes_revealed: HashSet<RootId>,
    pub collapsed_repos: HashSet<RootId>,
    pub collapsed_remotes: HashSet<(RootId, String)>,
    pub selected: Option<String>,
    pub selected_root: Option<RootId>,
    pub renaming: Option<String>,
    pub rename_draft: String,
    pub context_menu: Option<(RootId, String)>,
    pub scroll: f32,
    pub scroll_saved: Option<f32>,
    pub scroll_to: Option<String>,
    pub previous_filter: String,
}

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
    Unshelve,
    NewWorktree,
    CherryPickTarget,
    CherryPickAcross,
    RenameBranch,
    CompareBranches,
    GetFromBranch,
    ApplyPatch,
    CreatePatchComment,
    PatchFileSettings,
    ImportPatches,
    BranchNotFullyMerged,
    ManageRemotes,
    RewritePreflight,
    Reword,
}

#[derive(Clone, Debug)]
pub struct PushRejection {
    pub outcome: turbogit_services::sync_service::PushOutcome,
    pub request: crate::operation::PushRequest,
    pub affected_roots: Vec<RootId>,
    pub update_all: bool,
    pub remember: bool,
}

pub const EMPTY_COMMIT_MESSAGE: &str = "A commit cannot have an empty message";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryVerb {
    Drop,
    Reword { message: String },
}

impl HistoryVerb {
    pub fn action(&self) -> turbogit_domain::model::RebaseAction {
        match self {
            Self::Drop => turbogit_domain::model::RebaseAction::Drop,
            Self::Reword { .. } => turbogit_domain::model::RebaseAction::Reword,
        }
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Drop => None,
            Self::Reword { message } => Some(message),
        }
    }

    pub fn reworded_subject(&self) -> Option<String> {
        match self {
            Self::Drop => None,
            Self::Reword { message } => {
                Some(turbogit_services::reselection::subject_of_message(message).to_owned())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingReselect {
    pub root: RootId,
    pub plan: Vec<turbogit_domain::model::RebasePlanEntry>,
    pub anchor: CommitId,
    pub fate: turbogit_services::reselection::RewrittenAnchor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewritePreflight {
    pub root: RootId,
    pub commit: CommitId,
    pub verb: HistoryVerb,
    pub plan: Vec<turbogit_domain::model::RebasePlanEntry>,
    pub cautions: Vec<turbogit_services::history_editor::RebaseCaution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewordDraft {
    pub root: RootId,
    pub commit: CommitId,
}

#[derive(Clone)]
pub enum PendingConfirm {
    Discard {
        changes: Vec<Change>,
    },
    /// The delete confirms carry their `root` because `AppState::selected_root` is
    /// the workspace focus, not the row's repository: the branches tree never writes
    /// it, so a name-only confirm deletes in whichever repo the user last selected,
    /// including one that holds a same-named branch.
    DeleteLocalBranch {
        root: RootId,
        name: String,
    },
    DeleteRemoteBranch {
        root: RootId,
        remote: String,
        name: String,
    },
    InitHere,
    CloneRepo,
    RemoveWorktree {
        path: PathBuf,
    },
    DeinitSubmodule {
        path: PathBuf,
    },
    RevertCommit {
        commit: String,
    },
    DeleteShelf {
        root: RootId,
        name: String,
    },
    DeleteStash {
        root: RootId,
        index: usize,
    },
    CheckoutDirty {
        root: RootId,
        target: String,
        kind: turbogit_domain::model::BranchKind,
        detach: bool,
    },
    CheckoutInWorktree {
        branch: String,
        worktree: std::path::PathBuf,
    },
    GetFileFromBranch {
        root: RootId,
        source: String,
        path: PathBuf,
    },
}

#[derive(Clone, Debug)]
pub struct UnmergedCommits {
    pub root: RootId,
    pub branch: String,
    pub merged_target: String,
    pub commits: Vec<UnmergedCommit>,
    pub tip_sha: Option<String>,
}

impl UnmergedCommits {
    pub fn link_label(&self) -> String {
        match self.commits.len() {
            1 => "View 1 unmerged commit".to_owned(),
            n => format!("View {n} unmerged commits"),
        }
    }

    pub fn header(&self) -> String {
        format!(
            "The branch '{}' was not fully merged to {}. Below is the list of unmerged commits.",
            self.branch, self.merged_target
        )
    }
}

#[derive(Clone, Debug)]
pub struct UnmergedCommit {
    pub id: CommitId,
    pub subject: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BranchDeletePending {
    pub root: RootId,
    pub name: String,
    pub tip_sha: String,
    pub upstream: Option<Upstream>,
    pub unmerged: Option<UnmergedCommits>,
}

#[derive(Clone, Debug)]
pub struct BranchDeleteUndo {
    pub root: RootId,
    pub name: String,
    pub tip_sha: String,
    pub created_at: std::time::Instant,
    pub upstream: Option<Upstream>,
    pub unmerged: Option<UnmergedCommits>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewriteUndoVerb {
    Drop,
    Reword,
}

#[derive(Clone, Debug)]
pub struct RewriteUndoPending {
    pub root: RootId,
    pub verb: RewriteUndoVerb,
    pub commit: String,
}

#[derive(Clone, Debug)]
pub struct RewriteUndo {
    pub root: RootId,
    pub verb: RewriteUndoVerb,
    pub commit: String,
    pub tip_sha: String,
    pub created_at: std::time::Instant,
    pub withheld: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DivergentRoot {
    pub root: RootId,
    pub name: String,
    pub branch: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchDivergence {
    pub record_id: u64,
    pub operation: String,
    pub branch: String,
    pub changed: Vec<DivergentRoot>,
    pub failed: Vec<DivergentRoot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RollbackOutcome {
    Applied {
        branch: String,
        roots: usize,
    },
    Incomplete {
        branch: String,
        restored: usize,
        unrecovered: usize,
    },
    Refused,
}

#[derive(Clone)]
pub struct DiffTarget {
    pub root: RootId,
    pub left: Option<String>,
    pub right: Option<String>,
    pub path: Option<PathBuf>,
}

impl DiffTarget {
    /// side answers plain `git diff` (index↔worktree), and `commit^!` answers
    /// the same worktree comparison for a parentless commit, so neither form
    /// names a root commit's own change.
    pub fn commit(
        root: RootId,
        parent: Option<String>,
        cid: String,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            root,
            left: Some(parent.unwrap_or_else(|| EMPTY_TREE_OID.to_owned())),
            right: Some(cid),
            path,
        }
    }
}

const EMPTY_TREE_OID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameTarget {
    pub root: RootId,
    pub path: PathBuf,
    pub rev: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum DiffPane {
    Old,
    #[default]
    New,
}

impl DiffPane {
    pub fn opposite(self) -> Self {
        match self {
            DiffPane::Old => DiffPane::New,
            DiffPane::New => DiffPane::Old,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharDrag {
    pub change: ChangeRef,
    pub hunk: usize,
    pub ord: usize,
    pub new_no: usize,
    pub anchor: usize,
    pub cursor: usize,
}

impl CharDrag {
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Success,
    Warning,
    Error,
    Info,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
}

impl Toast {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Success,
            message: message.into(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Warning,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Error,
            message: message.into(),
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            kind: ToastKind::Info,
            message: message.into(),
        }
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchCreated {
    pub path: std::path::PathBuf,
    pub root: RootId,
}

#[derive(Default)]
pub struct UiState {
    pub dlg: DialogState,
    pub commit_message: String,
    pub amend: bool,
    pub commit_no_verify: bool,
    pub commit_repo_unchecks: HashSet<RootId>,
    pub selected: HashSet<PathBuf>,
    pub selection_anchor: Option<PathBuf>,
    pub recent_messages: Vec<String>,
    pub commit_subtab: CommitSubTab,
    pub changes_expanded: HashSet<RootId>,
    pub changes_focus: Option<RootId>,
    pub file_filter: String,
    pub focus_file_filter: bool,
    pub tab: Tab,
    pub branches_popup: bool,
    pub branch_filter: String,
    pub log_filter: String,
    pub branches_filter: String,
    pub branches_view: crate::persistence::BranchesViewOptions,
    pub branches_settings_open: bool,
    pub branches_tree: TreeState,
    pub log_tree: TreeState,
    pub branches_focus_search: bool,
    pub branches_delete_consequence: Option<String>,
    pub branches_delete_unmerged: Option<UnmergedCommits>,
    pub branches_delete_pending: Option<BranchDeletePending>,
    pub branches_undo: Option<BranchDeleteUndo>,
    pub rewrite_undo_pending: Option<RewriteUndoPending>,
    pub rewrite_undo: Option<RewriteUndo>,
    pub branch_divergence: Option<BranchDivergence>,
    pub branches_fetch_before: Vec<(RootId, Vec<String>)>,
    pub branches_last_fetch: Option<chrono::DateTime<chrono::Utc>>,
    pub branches_repo_filter: Option<RootId>,
    pub branches_scope_picker_open: bool,
    pub log_branch_filter: String,
    pub log_root_filter: Option<RootId>,
    pub log_selected_file: Option<PathBuf>,
    pub log_file_filter: String,
    pub log_path_scope: Option<PathBuf>,
    pub log_ref_scope: Option<(RootId, String)>,
    pub selected_commit: Option<CommitId>,
    pub log_scroll_to: Option<CommitId>,
    pub log_hidden_jump: Option<(RootId, CommitId)>,
    pub pending_reselect: Option<PendingReselect>,
    pub log_commit_menu: Option<(RootId, CommitId)>,
    pub log_file_menu: Option<(RootId, CommitId, std::path::PathBuf)>,
    pub diff: Option<DiffTarget>,
    pub blame: Option<BlameTarget>,
    pub blame_cache: Option<(String, std::sync::Arc<[BlameLine]>)>,
    pub dialog: Option<Dialog>,
    pub push_in_flight: Option<crate::operation::PushRequest>,
    pub commit_push_dialog_pending: Option<RootId>,
    pub push_rejection: Option<PushRejection>,
    pub vcs_popup: bool,
    pub workspace_picker_open: bool,
    pub shortcuts_open: bool,
    pub workspace_picker_anchor: Option<(f32, f32)>,
    pub settings_open: bool,
    pub settings_category: crate::state::SettingsCategory,
    pub git_version_badge: Option<Result<String, String>>,
    pub git_version_checked_path: Option<String>,
    pub git_env_check: Option<(String, Result<String, String>)>,
    pub settings_new_pattern: String,
    pub settings_draft: Option<VcsSettings>,
    pub conflict_open: Option<PathBuf>,
    pub conflict_segs: Vec<(String, String, bool)>,
    pub conflict_res: Vec<Option<u8>>,
    pub conflict_text: String,
    pub conflict_resolver_open: bool,
    pub merge_in_progress: bool,
    pub conflict_resolver_active_idx: usize,
    pub conflict_resolver_auto_advance: bool,
    pub conflict_resolver_undo: Vec<Option<u8>>,
    pub conflict_resolver_edited: bool,
    pub auto_merged_files: Vec<PathBuf>,
    pub conflict_resolver_selected: Option<PathBuf>,
    pub conflict_resolver_scope: BulkApplyScope,
    pub conflict_overwrite_pending: Option<(usize, u8)>,
    pub conflict_bulk_report: Option<ConflictBulkReport>,
    pub shelves: Vec<Shelf>,
    pub shelf_removal_pending: Option<(RootId, String)>,
    pub patch_created: Option<PatchCreated>,
    pub preview_change: Option<ChangeRef>,
    pub diff_cache: Option<(String, std::sync::Arc<crate::diff_model::DiffValue>)>,
    pub diff_side_by_side: bool,
    pub pane_bytes: crate::diff_data::PaneCache,
    /// One context read's answer, the landing pad `Reveals` folds its own `Arc`
    /// out of. Never persisted: it is a hand-off, not state the user owns.
    pub context_side: Option<(String, Option<std::sync::Arc<crate::reveals::SideText>>)>,
    pub pane_generation: u64,
    pub diff_current_hunk: usize,
    pub diff_collapsed: HashSet<usize>,
    pub reveals: crate::reveals::Reveals,
    pub diff_policies: DiffPolicies,
    pub diff_pane: DiffPane,
    pub diff_options_open: bool,
    pub blame_caret_line: Option<usize>,
    pub hunk_nav_armed_edge: Option<(crate::diff_data::Dir, std::time::Instant)>,
    pub pending_granular: Option<ChangeRef>,
    pub granularly_completed: HashSet<ChangeRef>,
    pub char_drag: Option<CharDrag>,
    pub command_palette: bool,
    pub command_query: String,
    pub toast: Option<Toast>,
    pub toast_shown_at: Option<f64>,
    pub busy: bool,
    pub confirm: Option<crate::state::PendingConfirm>,
    pub recent_repos: Vec<PathBuf>,
    pub recent_branches: Vec<String>,
    pub branches_cursor: usize,
    pub welcome_visible: bool,
    pub sidebar_filter: String,
    pub sidebar_collapsed: HashSet<String>,
    pub show_changelog: bool,
    pub sidebar_smart_group: Option<String>,
    pub smart_group_rules: Vec<crate::smart_rules::SmartGroupRule>,
    pub repo_selection: HashSet<RootId>,
    pub bulk_op: Option<BulkOp>,
    pub bulk_rebase: bool,
    pub bulk_branch_name: String,
    pub bulk_from_upstream: bool,
    pub bulk_command: String,
    pub bulk_command_armed: bool,
    pub recent_custom_commands: Vec<String>,
    pub bulk_run: Option<crate::bulk_run_view::BulkRunView>,
    pub cherry_run: Option<crate::cherry_run_view::CherryRunView>,
    pub pinned_views: Vec<crate::pinned_views::PinnedView>,
    pub bulk_history: Vec<crate::bulk_history::BulkRunRecord>,
    pub bulk_history_open: Option<u64>,
    pub smart_rule_editor_open: bool,
    pub smart_rule_editing: Option<usize>,
    pub smart_rule_draft: Option<crate::smart_rules::SmartGroupRule>,
    pub recent_projects: Vec<crate::recents::RecentProject>,
    pub welcome_branch_cache: HashMap<PathBuf, (Option<String>, std::time::Instant)>,
    pub welcome_clone_url: String,
    pub welcome_shallow: bool,
    pub welcome_focus_clone: bool,
    pub show_toolbar: bool,
    pub show_status_bar: bool,
    pub banner: Option<crate::banner::Banner>,
    pub activity: crate::activity::ActivityLog,
}
pub struct AppState {
    pub project_dir: PathBuf,
    pub(crate) executor: Arc<dyn GitExecutor>,
    pub settings: VcsSettings,
    pub multi: MultiRootManager,
    pub tx: Sender<AppEvent>,
    pub rx: Receiver<AppEvent>,
    pub selected_root: Option<RootId>,
    pub clone_url: String,
    pub last_error: Option<String>,
    pub ui: UiState,
    pub caches: RootCaches,
    log_display: Option<Arc<crate::log_display::LogDisplay>>,
    pub recents_config_dir: Option<PathBuf>,
    pub dir_picker: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    pub patch_writer: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    pub patch_reader: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    pub git_version: String,
    pump: Pump,
    worktree: crate::worktree_lifecycle::WorktreeLifecycle,
    pub(crate) reads: crate::keyed_read::Reads,
    #[cfg(test)]
    hold_reads: bool,
    #[cfg(test)]
    held_reads: Vec<HeldRead>,
    pub(crate) fetch_admission: crate::fetch_admission::FetchAdmission,
    pub(crate) incoming_poll_last: Option<std::time::Instant>,
    pub(crate) apply_recovery: crate::apply_patch::ApplyRecovery,
    pub(crate) create_patch_memos: crate::create_patch::Memos,
}

impl AppState {
    pub fn new(project_dir: PathBuf) -> Self {
        Self::launch(Some(project_dir))
    }

    pub fn launch(project_dir: Option<PathBuf>) -> Self {
        Self::launch_in(project_dir, None)
    }

    fn with_flat_state(
        project_dir: PathBuf,
        ui: UiState,
        recents_config_dir: Option<PathBuf>,
        pump: Pump,
    ) -> Self {
        let (tx, rx) = unbounded();
        let settings = VcsSettings::default();
        let executor = build_executor(&settings);
        let git_version =
            turbogit_engine::resolve_git_version(&settings).unwrap_or_else(|_| "unknown".into());
        let mut ui = ui;
        let mut last_error = None;
        match turbogit_services::shelve_stash::load_shelves(&project_dir) {
            Ok(shelves) => ui.shelves = shelves,
            Err(e) => last_error = Some(e.to_string()),
        }
        Self {
            project_dir,
            executor,
            settings,
            multi: MultiRootManager::default(),
            tx,
            rx,
            selected_root: None,
            clone_url: String::new(),
            last_error,
            git_version,
            ui,
            caches: RootCaches::default(),
            log_display: None,
            recents_config_dir,
            dir_picker: None,
            patch_writer: None,
            patch_reader: None,
            pump,
            worktree: crate::worktree_lifecycle::WorktreeLifecycle::default(),
            reads: Default::default(),
            #[cfg(test)]
            hold_reads: false,
            #[cfg(test)]
            held_reads: Vec::new(),
            fetch_admission: crate::fetch_admission::FetchAdmission::default(),
            incoming_poll_last: None,
            apply_recovery: Default::default(),
            create_patch_memos: Default::default(),
        }
    }

    pub fn launch_in(project_dir: Option<PathBuf>, recents_config_dir: Option<PathBuf>) -> Self {
        let mut state = Self::with_flat_state(
            project_dir.clone().unwrap_or_default(),
            UiState {
                show_toolbar: true,
                show_status_bar: true,
                ..UiState::default()
            },
            recents_config_dir,
            Pump::Spawned,
        );

        if let Some(cfg) = state.recents_config() {
            state.ui.recent_projects = crate::recents::load(&cfg).projects;
        }

        match project_dir {
            Some(dir) => {
                state.project_dir = dir;
                state.rescan();

                let ui = crate::persistence::load_ui_state(&state.project_dir);
                ui.restore_into(&mut state.ui);
            }
            None => {
                state.ui.welcome_visible = true;
            }
        }
        state
    }

    /// Panics if any root cannot be snapshotted, a broken test fixture should
    /// fail at construction.
    pub fn for_roots(project_dir: &Path, roots: &[PathBuf]) -> Self {
        let mut state = Self::with_flat_state(
            project_dir.to_path_buf(),
            UiState::default(),
            None,
            Pump::Inline,
        );
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

    pub fn with_executor(mut self, executor: Arc<dyn GitExecutor>) -> Self {
        self.executor = executor;
        self
    }

    pub fn with_settings(mut self, settings: VcsSettings) -> Self {
        self.settings = settings;
        self
    }

    pub fn rebuild_executor(&mut self) {
        self.executor = build_executor(&self.settings);
    }

    /// Cheap enough to ask per root per frame for that reason, a `stat` inside
    /// the repository's git directory, never a git process, which is the
    /// property the adapters are held to by `tests/holding_operation.rs`.
    pub fn holding_operation(&self, root: &Path) -> Option<OwnedOperation> {
        self.executor.holding_operation(root)
    }

    fn recents_config(&self) -> Option<PathBuf> {
        self.recents_config_dir
            .clone()
            .or_else(crate::recents::default_config_dir)
    }

    pub fn save_durable_ui(&self) {
        let ui = crate::persistence::UiPersist::capture(&self.ui);
        let _ = crate::persistence::save_ui_state(&self.project_dir, &ui);
    }
    pub fn rescan(&mut self) {
        self.caches.invalidate_all_worktrees();
        self.worktree.clear();
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
        self.hide_project_metadata();

        let roots: Vec<RootId> = self.multi.roots.iter().map(|r| r.id.clone()).collect();
        for root in roots {
            let path = root.as_path().to_path_buf();
            let id = root.clone();
            self.pump_read(move |executor, tx| {
                let res = executor.status(&path);
                let _ = tx.send(AppEvent::StatusScanned {
                    root: id,
                    status: res,
                });
            });
            let path = root.as_path().to_path_buf();
            self.pump_read(move |executor, tx| {
                if let Ok((ahead, behind)) = current_branch_ahead_behind(executor.as_ref(), &path) {
                    let _ = tx.send(AppEvent::AheadBehind {
                        root,
                        ahead,
                        behind,
                    });
                }
            });
        }
    }

    fn hide_project_metadata(&self) {
        let dir = crate::persistence::turbogit_dir(&self.project_dir);
        if let Some(root) = self.multi.roots.iter().find(|r| r.path == self.project_dir) {
            let _ = self.executor.exclude_dir(&root.path, &dir);
        }
    }

    fn log_batch_plan(&self, root: &RootId) -> (LogOpts, LogBatchMode) {
        let (rows, skip, mode) = window_batch_plan(self.caches.log(root));
        (
            LogOpts {
                max_count: Some(rows),
                skip,
                ..Default::default()
            },
            mode,
        )
    }

    pub fn fetch_log(&mut self, root: RootId) {
        if !self.fetch_admission.admit(InflightFetch::Log(root.clone())) {
            return;
        }
        let (opts, mode) = self.log_batch_plan(&root);
        self.pump_read(move |executor, tx| {
            let res = executor.log(&root.0, &opts);
            let _ = tx.send(AppEvent::LogLoaded {
                root,
                commits: res,
                mode,
            });
        });
    }

    fn settle_log_batch(&mut self, root: &RootId, mode: LogBatchMode, batch: Vec<Commit>) {
        if matches!(
            self.caches.settle_log_batch(root, mode, batch),
            LogBatchSettle::Torn
        ) {
            self.fetch_log(root.clone());
        }
    }

    pub fn fetch_refs(&mut self, root: RootId) {
        if !self
            .fetch_admission
            .admit(InflightFetch::Refs(root.clone()))
        {
            return;
        }
        self.pump_read(move |executor, tx| {
            let res = executor.ref_decorations(&root.0);
            let _ = tx.send(AppEvent::RefsLoaded { root, deco: res });
        });
    }

    pub fn fetch_file_stats(&mut self, root: RootId, commit: CommitId) {
        if self.caches.file_stats_loaded(&root, &commit)
            || !self
                .fetch_admission
                .admit(InflightFetch::FileStats(root.clone(), commit.clone()))
        {
            return;
        }
        self.pump_read(move |executor, tx| {
            let res = executor.change_stats(
                &root.0,
                &turbogit_domain::model::ChangeQuestion::Commit {
                    commit: commit.to_string(),
                },
            );
            let _ = tx.send(AppEvent::FileStatsLoaded {
                root,
                commit,
                stats: res,
            });
        });
    }

    pub fn load_more_log(&mut self) {
        for root in self.multi.roots.clone() {
            if self.caches.log_has_more(&root.id) {
                self.fetch_log(root.id);
            }
        }
    }

    /// The returned handle is the SAME allocation the frame before it got for as
    /// long as nothing changed: a rebuild allocates a new one. That identity is
    /// this method's contract, not an accident of the allocation, and it is what
    pub fn sync_log_display(&mut self) -> Arc<crate::log_display::LogDisplay> {
        let revision = self.caches.revision();
        let inputs = crate::log_display::LogInputs::of(self);
        if crate::log_display::needs_rebuild(self.log_display.as_deref(), revision, &inputs) {
            let (loaded, hits) = crate::log_display::sources(&self.caches, &inputs);
            self.log_display = Some(Arc::new(crate::log_display::derive(
                revision, &inputs, loaded, hits,
            )));
        }
        self.log_display
            .clone()
            .expect("a rebuilt window is held; the branch above stored it")
    }

    pub fn fetch_worktrees(&mut self, root: RootId) {
        self.worktree
            .fetch(root, self.executor.clone(), self.tx.clone());
    }

    fn invalidate_worktree_cache(&mut self, root: &RootId) {
        self.worktree.on_mutation(root);
        self.caches.invalidate_worktrees(root);
    }

    pub fn ensure_worktree_probes(&mut self) {
        let Some(id) = self.selected_root.clone() else {
            return;
        };
        let candidates: Vec<PathBuf> = self
            .caches
            .worktrees(&id)
            .unwrap_or(&[])
            .iter()
            .filter(|w| w.dirty.is_none())
            .map(|w| w.path.clone())
            .collect();
        self.worktree
            .ensure_probes(id, candidates, self.executor.clone(), self.tx.clone());
    }

    pub fn fetch_submodules(&mut self, root: RootId) {
        if !self
            .fetch_admission
            .admit(InflightFetch::Submodules(root.clone()))
        {
            return;
        }
        self.pump_read(move |executor, tx| {
            let res = executor.submodule_status(&root.0);
            let _ = tx.send(AppEvent::SubmodulesLoaded {
                root,
                submodules: res,
            });
        });
    }

    pub fn fetch_stashes(&mut self, root: RootId) {
        if self.caches.stashes(&root).is_some()
            || !self
                .fetch_admission
                .admit(InflightFetch::Stashes(root.clone()))
        {
            return;
        }
        self.pump_read(move |executor, tx| {
            let res = turbogit_services::shelve_stash::list(executor.as_ref(), root.as_path());
            let _ = tx.send(AppEvent::StashesLoaded { root, stashes: res });
        });
    }

    pub fn ensure_files(&mut self, root: &RootId, commit: &CommitId) -> &[Change] {
        self.caches
            .ensure_files(self.executor.as_ref(), root, commit)
    }

    pub fn ensure_log(&mut self, root: &RootId, scope: LogScope) -> &[Commit] {
        self.caches.ensure_log(self.executor.as_ref(), root, &scope)
    }

    pub fn retry_log_read(&mut self, read: &LogRead) {
        self.caches.forget_log_read(read);
        match read {
            LogRead::Commits(root) => self.fetch_log(root.clone()),
            LogRead::Scope(root, scope) => {
                self.ensure_log(root, scope.clone());
            }
            LogRead::Changes(root, cid) => {
                self.ensure_files(root, cid);
            }
            LogRead::Details(root, cid) => {
                self.fetch_file_stats(root.clone(), cid.clone());
            }
        }
    }

    pub fn refresh_log_listing(&mut self) {
        for read in crate::log_states::failed_listing_reads(self) {
            self.retry_log_read(&read);
        }
    }

    pub fn refresh_commit_details(&mut self) {
        if let Some(read) = crate::log_states::failed_details_read(self) {
            self.retry_log_read(&read);
        }
    }

    pub fn refresh_commit_changes(&mut self) {
        if let Some(read) = crate::log_states::failed_changes_read(self) {
            self.retry_log_read(&read);
        }
    }

    pub fn reset_log_filters(&mut self) {
        let query = self.ui.log_filter.trim().to_owned();
        let mut roots = crate::log_display::visible_roots(self);
        if let Some((root, _)) = &self.ui.log_ref_scope
            && !roots.iter().any(|r| r == root)
        {
            roots.push(root.clone());
        }
        for root in &roots {
            if !query.is_empty() {
                self.caches.forget_log_read(&LogRead::Scope(
                    root.clone(),
                    LogScope::Search(query.clone()),
                ));
            }
            if let Some(path) = &self.ui.log_path_scope {
                self.caches
                    .forget_log_read(&LogRead::Scope(root.clone(), LogScope::Path(path.clone())));
            }
            if let Some((_, name)) = &self.ui.log_ref_scope {
                self.caches
                    .forget_log_read(&LogRead::Scope(root.clone(), LogScope::Ref(name.clone())));
            }
        }
        self.ui.log_filter.clear();
        self.ui.log_path_scope = None;
        self.ui.log_ref_scope = None;
    }

    fn scoped_log_batch_plan(&self, root: &RootId, scope: &LogScope) -> (LogOpts, LogBatchMode) {
        let (rows, skip, mode) = scoped_batch_plan(scope, self.caches.scoped_log(root, scope));
        (scope.batch_opts(rows, skip), mode)
    }

    pub fn load_more_scoped_log(&mut self, root: &RootId, scope: &LogScope) {
        if !self.caches.scoped_log_can_grow(root, scope) {
            return;
        }
        if !self
            .fetch_admission
            .admit(InflightFetch::ScopedLog(root.clone(), scope.clone()))
        {
            return;
        }
        let (opts, mode) = self.scoped_log_batch_plan(root, scope);
        let (r, s) = (root.clone(), scope.clone());
        self.pump_read(move |executor, tx| {
            let res = executor.log(&r.0, &opts);
            let _ = tx.send(AppEvent::LogBatchLoaded {
                root: r,
                scope: s,
                commits: res,
                mode,
            });
        });
    }

    pub fn ensure_hunk_stats(&mut self, root: &RootId) {
        let exec = self.executor.clone();
        self.caches.ensure_hunk_stats(exec.as_ref(), root);
    }

    pub fn refresh(&mut self, affected: Affected) {
        self.caches.invalidate(&affected);
        if matches!(affected, Affected::All) {
            self.worktree.clear();
        }
        self.ui.diff_cache = None;
        self.reads.invalidate();
        self.ui.blame_cache = None;
        self.ui.pane_bytes.clear();
        self.ui.context_side = None;
        self.ui.pane_generation += 1;
        granular::prune_on_refresh(self);
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
        granular::prune_selection_on_refresh(self);

        for rp in paths {
            self.pump_read(move |executor, tx| {
                if let Ok((ahead, behind)) = current_branch_ahead_behind(executor.as_ref(), &rp) {
                    let _ = tx.send(AppEvent::AheadBehind {
                        root: RootId(rp),
                        ahead,
                        behind,
                    });
                }
            });
        }

        if let Some(sel) = self.selected_root.clone() {
            let in_scope = match &affected {
                Affected::All => true,
                Affected::Root(id) => *id == sel,
            };
            if in_scope {
                self.fetch_log(sel);
            }
        }
    }

    pub fn dispatch(&mut self, op: Operation) {
        if let Operation::Fetch { roots } = &op {
            self.record_fetch_baseline(roots);
        }
        let kind = op.kind();
        let label = op.label();
        let affected = op.affected();
        let mutated = if op.mutates_worktrees() {
            affected.root().cloned()
        } else {
            None
        };
        self.ui.busy = true;
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        let run = move || {
            let res = op.run(executor.as_ref());
            let _ = tx.send(AppEvent::OpCompleted {
                kind,
                label,
                affected,
                result: res,
            });
            if let Some(root) = mutated {
                let _ = tx.send(AppEvent::WorktreesMutated { root });
            }
        };
        match self.pump {
            Pump::Spawned => {
                std::thread::spawn(run);
            }
            Pump::Inline => run(),
        }
    }

    pub fn dispatch_push(
        &mut self,
        request: crate::operation::PushRequest,
        recover: Option<crate::operation::RecoveryPlan>,
    ) {
        let op = match recover {
            Some(plan) => Operation::push_with_recovery(request.clone(), plan),
            None => Operation::push(request.clone()),
        };
        self.ui.push_in_flight = Some(request);
        self.ui.push_rejection = None;
        self.dispatch(op);
    }

    pub fn dispatch_commit(
        &mut self,
        root: &RootId,
        message: &str,
        amend: bool,
        no_verify: bool,
        then_push: bool,
    ) {
        self.ui.commit_push_dialog_pending = then_push.then(|| root.clone());
        self.dispatch(Operation::Commit {
            root: root.clone(),
            message: message.to_string(),
            amend,
            no_verify,
        });
    }

    pub fn dispatch_pull(
        &mut self,
        label: impl Into<String>,
        root: RootId,
        checkout: Option<String>,
    ) {
        let settings = self.settings.clone();
        let rebase = settings.update_method == UpdateMethod::Rebase;
        self.dispatch(Operation::custom(
            label,
            Affected::Root(root.clone()),
            move |v| {
                let path = root.as_path();
                if let Some(branch) = &checkout {
                    if rebase {
                        turbogit_services::integrate_service::ensure_rebase_allowed(
                            &settings, branch,
                        )?;
                    }
                    v.branch_checkout(path, branch)?;
                }
                turbogit_services::sync_service::pull_guarded(
                    v,
                    path,
                    settings.update_method,
                    &settings,
                    checkout.as_deref().unwrap_or_default(),
                )
            },
        ));
    }

    pub(crate) fn pump_read(
        &mut self,
        work: impl FnOnce(Arc<dyn GitExecutor>, Sender<AppEvent>) + Send + 'static,
    ) {
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        match self.pump {
            Pump::Spawned => {
                std::thread::spawn(move || work(executor, tx));
            }
            Pump::Inline => work(executor, tx),
        }
    }

    pub(crate) fn pump_keyed_read(
        &mut self,
        work: impl FnOnce(Arc<dyn GitExecutor>, Sender<AppEvent>) + Send + 'static,
    ) {
        #[cfg(test)]
        if self.hold_reads {
            self.held_reads.push(Box::new(work));
            return;
        }
        self.pump_read(work);
    }

    #[cfg(test)]
    pub(crate) fn hold_reads(&mut self) {
        self.hold_reads = true;
    }

    #[cfg(test)]
    pub(crate) fn held_reads(&self) -> usize {
        self.held_reads.len()
    }

    #[cfg(test)]
    pub(crate) fn release_held_read(&mut self, index: usize) {
        assert!(
            index < self.held_reads.len(),
            "release_held_read({index}) with {} held",
            self.held_reads.len()
        );
        let work = self.held_reads.remove(index);
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        work(executor, tx);
    }

    pub(crate) fn show_toast(&mut self, toast: Toast) {
        self.ui.toast = Some(toast);
        self.ui.toast_shown_at = None;
    }

    pub fn show_patch_toast(&mut self, toast: Toast) {
        self.show_toast(toast);
    }

    fn record_fetch_baseline(&mut self, roots: &[RootId]) {
        for id in roots {
            let before: Vec<String> = self
                .multi
                .by_id(id)
                .map(|r| {
                    r.branches
                        .iter()
                        .filter(|b| b.kind == BranchKind::Remote)
                        .map(|b| b.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            self.ui.branches_fetch_before.push((id.clone(), before));
        }
    }

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
            post_bulk_completed(&tx, label, results);
        });
    }

    pub fn define_remote_for_push(&mut self, root: RootId, name: String, url: String) {
        let Some(path) = self.multi.by_id(&root).map(|r| r.path.clone()) else {
            return;
        };
        let executor = self.executor.clone();
        let tx = self.tx.clone();
        self.ui.busy = true;
        std::thread::spawn(move || {
            let result =
                turbogit_services::remote_service::add(executor.as_ref(), &path, &name, url.trim());
            let _ = tx.send(AppEvent::RemoteDefined { root, name, result });
        });
    }

    pub fn run_confirmed(&mut self, c: PendingConfirm) {
        crate::confirm::run(self, c);
    }

    pub fn shelves_for(&self, root: &RootId) -> Vec<Shelf> {
        let mut rows: Vec<Shelf> = self
            .ui
            .shelves
            .iter()
            .filter(|s| &s.root == root)
            .cloned()
            .collect();
        rows.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        rows
    }

    pub fn open_unshelve_dialog(&mut self) {
        let root = self.selected_root.clone();
        let shelf = root
            .as_ref()
            .and_then(|id| self.shelves_for(id).first().map(|s| s.name.clone()));
        let dlg = &mut self.ui.dlg;
        dlg.unshelve_root = root;
        dlg.unshelve_shelf = shelf;
        dlg.unshelve_remove = false;
        self.ui.dialog = Some(Dialog::Unshelve);
    }

    pub fn unshelve(&mut self, root: RootId, name: impl Into<String>, remove: bool) {
        let name = name.into();
        let patch = self.shelf_patch_payload(&root, &name);
        if remove {
            self.ui.shelf_removal_pending = Some((root.clone(), name.clone()));
        }
        match patch {
            Some(patch) => self.dispatch(Operation::unshelve_patch(root, name, remove, patch)),
            None => self.dispatch(Operation::unshelve(root, name, remove)),
        }
    }

    fn shelf_patch_payload(&self, root: &RootId, name: &str) -> Option<Patch> {
        let row = self
            .ui
            .shelves
            .iter()
            .find(|s| &s.root == root && s.name == name)?;
        let ShelfSource::Patch { file } = &row.source else {
            return None;
        };
        let text =
            turbogit_services::shelve_stash::read_patch_payload(&self.project_dir, file).ok()?;
        let patch = turbogit_engine_api::patch::parse_patch(&text);
        (!patch.is_empty()).then_some(patch)
    }

    pub fn add_shelf(&mut self, shelf: Shelf) -> TgResult<()> {
        let mut kept = self.ui.shelves.clone();
        kept.push(shelf);
        if let Err(e) = turbogit_services::shelve_stash::save_shelves(&self.project_dir, &kept) {
            self.last_error = Some(e.to_string());
            return Err(e);
        }
        self.ui.shelves = kept;
        Ok(())
    }

    pub fn import_patch_to_shelf(
        &mut self,
        root: &RootId,
        name: &str,
        text: &str,
    ) -> TgResult<Shelf> {
        let patch = turbogit_engine_api::patch::parse_patch(text);
        if patch.is_empty() {
            return Err(TgError::Other(
                "this file carries no `diff --git` section, so there is nothing to park"
                    .to_string(),
            ));
        }
        let changes: Vec<Change> = patch
            .files
            .iter()
            .map(|f| Change {
                path: std::path::PathBuf::from(if f.new_path.is_empty() {
                    &f.old_path
                } else {
                    &f.new_path
                }),
                status: turbogit_domain::model::ChangeStatus::Modified,
                chunks: Vec::new(),
                staged: false,
                unstaged: false,
                orig_path: None,
            })
            .collect();
        let created_at = chrono::Utc::now();
        let name = if name.trim().is_empty() {
            "Imported patch".to_string()
        } else {
            name.trim().to_string()
        };
        let file = turbogit_services::shelve_stash::write_patch_payload(
            &self.project_dir,
            &name,
            created_at,
            text,
        )?;
        let shelf = turbogit_services::shelve_stash::make_patch_shelf(
            &name,
            &changes,
            root.clone(),
            created_at,
            &file,
        );
        if let Err(e) = self.add_shelf(shelf.clone()) {
            if let Err(cleanup) =
                turbogit_services::shelve_stash::delete_patch_payload(&self.project_dir, &file)
            {
                self.last_error = Some(format!(
                    "{e} — and its leftover payload {} could not be removed: {cleanup}",
                    turbogit_services::shelve_stash::patch_payload_path(&self.project_dir, &file)
                        .display()
                ));
            }
            return Err(e);
        }
        self.ui.activity.push(crate::activity::ActivityEntry {
            at: chrono::Local::now(),
            repo: crate::settle::activity_repo_label(&Affected::Root(root.clone())),
            message: format!(
                "Patch imported to the shelf · {name} · {} file(s), nothing applied",
                changes.len()
            ),
            kind: crate::activity::ActivityKind::Success,
        });
        Ok(shelf)
    }

    pub fn import_patch_from_apply_window(&mut self) {
        let (Some(root), Some(text)) = (
            self.ui.dlg.apply_patch_root.clone(),
            (!self.ui.dlg.apply_patch_text.is_empty())
                .then(|| self.ui.dlg.apply_patch_text.clone()),
        ) else {
            self.show_toast(Toast::info(
                "Import to shelf needs a patch: choose or paste one first.",
            ));
            return;
        };
        let name = self.apply_window_shelf_name();
        match self.import_patch_to_shelf(&root, &name, &text) {
            Ok(shelf) => {
                self.show_toast(Toast::success(format!(
                    "'{}' is on the shelf — nothing was applied",
                    shelf.name
                )));
                self.ui.dialog = None;
            }
            Err(e) => self.ui.dlg.apply_patch_error = Some(e.to_string()),
        }
    }

    fn apply_window_shelf_name(&self) -> String {
        let message = self.ui.dlg.apply_patch_message.trim();
        if !message.is_empty() {
            return message.to_string();
        }
        let path = self.ui.dlg.apply_patch_path.trim();
        if !path.is_empty()
            && let Some(stem) = std::path::Path::new(path).file_stem()
        {
            return stem.to_string_lossy().into_owned();
        }
        "Imported patch".to_string()
    }

    pub fn import_patch_file_to_shelf(
        &mut self,
        path: &std::path::Path,
        name: &str,
    ) -> TgResult<Shelf> {
        let root = self.selected_root.clone().ok_or_else(|| {
            TgError::Other("a shelf row needs a repository to belong to".to_string())
        })?;
        let text = std::fs::read_to_string(path)?;
        let fallback = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = if name.trim().is_empty() {
            fallback.as_str()
        } else {
            name.trim()
        };
        self.import_patch_to_shelf(&root, name, &text)
    }

    pub fn choose_import_patch_file(&mut self) {
        let Some(reader) = self.patch_reader.take() else {
            self.show_toast(Toast::info("Type the patch file's path instead."));
            return;
        };
        let chosen = reader();
        self.patch_reader = Some(reader);
        let Some(path) = chosen else { return };
        self.ui.dlg.import_patch_path = path.to_string_lossy().into_owned();
    }

    pub fn remove_shelf(&mut self, root: &RootId, name: &str) {
        let kept: Vec<Shelf> = self
            .ui
            .shelves
            .iter()
            .filter(|s| &s.root != root || s.name != name)
            .cloned()
            .collect();
        if kept.len() == self.ui.shelves.len() {
            return;
        }
        if let Err(e) = turbogit_services::shelve_stash::save_shelves(&self.project_dir, &kept) {
            self.last_error = Some(e.to_string());
            return;
        }
        let orphaned: Vec<String> = self
            .ui
            .shelves
            .iter()
            .filter(|s| &s.root == root && s.name == name)
            .filter_map(|s| s.source.file_name().map(|f| f.to_string()))
            .collect();
        self.ui.shelves = kept;
        for file in orphaned {
            if let Err(e) =
                turbogit_services::shelve_stash::delete_patch_payload(&self.project_dir, &file)
            {
                self.last_error = Some(format!(
                    "The shelf row is gone but its patch file {} is not: {e}",
                    turbogit_services::shelve_stash::patch_payload_path(&self.project_dir, &file)
                        .display()
                ));
            }
        }
    }

    pub fn cherry_pick_to(&mut self, commit: String, target: String) {
        let Some(root) = self.selected_root.clone() else {
            return;
        };
        let settings = self.settings.clone();
        self.dispatch(Operation::custom(
            format!("Cherry-pick {} to {target}", short_sha(&commit)),
            Affected::Root(root.clone()),
            move |v| {
                turbogit_services::integrate_service::cherry_pick_to(
                    v,
                    root.as_path(),
                    &commit,
                    &target,
                    &settings,
                )
            },
        ));
    }

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

    pub fn open_new_branch(&mut self, base: NewBranchBase, checkout: bool, name: Option<String>) {
        let dlg = &mut self.ui.dlg;
        dlg.new_branch_name = name.unwrap_or_default();
        dlg.new_branch_base = base;
        dlg.new_branch_base_picker_open = false;
        dlg.new_branch_checkout = checkout;
        self.ui.dialog = Some(Dialog::NewBranch);
    }

    pub fn arm_reselect_after_rewrite(
        &mut self,
        root: RootId,
        plan: Vec<RebasePlanEntry>,
        anchor: CommitId,
        fate: turbogit_services::reselection::RewrittenAnchor,
    ) {
        self.ui.pending_reselect = Some(PendingReselect {
            root,
            plan,
            anchor,
            fate,
        });
    }

    fn apply_pending_reselect(&mut self, root: &RootId) {
        let Some(pending) = self.ui.pending_reselect.clone() else {
            return;
        };
        if &pending.root != root {
            return;
        }
        let Some(window) = self.caches.log(root).map(<[Commit]>::to_vec) else {
            return;
        };
        if window.iter().any(|c| c.id == pending.anchor) {
            return;
        }
        self.ui.pending_reselect = None;
        self.ui.selected_commit = turbogit_services::reselection::reselect_after_rewrite(
            &pending.plan,
            &window,
            &pending.anchor,
            &pending.fate,
        );
        self.ui.log_selected_file = None;
        self.ui.log_commit_menu = None;
    }

    pub(crate) fn clear_pending_reselect(&mut self, affected: &Affected) {
        if self
            .ui
            .pending_reselect
            .as_ref()
            .is_some_and(|p| match affected {
                Affected::All => true,
                Affected::Root(root) => &p.root == root,
            })
        {
            self.ui.pending_reselect = None;
        }
    }

    pub fn open_rewrite_preflight(&mut self, root: &RootId, commit: &str, verb: HistoryVerb) {
        if let HistoryVerb::Reword { message } = &verb
            && message.trim().is_empty()
        {
            self.ui.toast = Some(Toast::error(EMPTY_COMMIT_MESSAGE.to_owned()));
            return;
        }
        let plan = match self.history_verb_plan(
            root,
            commit,
            verb.action(),
            verb.message().map(str::to_owned),
        ) {
            Ok(plan) => plan,
            Err(reason) => {
                self.ui.toast = Some(Toast::error(reason));
                return;
            }
        };
        let cautions = self.rebase_cautions(root, &plan);
        self.ui.dlg.rewrite_preflight = Some(RewritePreflight {
            root: root.clone(),
            commit: commit.to_owned(),
            verb,
            plan,
            cautions,
        });
        self.ui.dialog = Some(Dialog::RewritePreflight);
    }

    pub fn open_reword_editor(&mut self, root: &RootId, commit: &str) {
        let Some(found) = self
            .caches
            .log(root)
            .and_then(|commits| commits.iter().find(|c| c.id == commit))
        else {
            self.ui.toast = Some(Toast::error(format!(
                "{} is no longer in this log",
                short_sha(commit)
            )));
            return;
        };
        let dlg = &mut self.ui.dlg;
        dlg.reword = Some(RewordDraft {
            root: root.clone(),
            commit: commit.to_owned(),
        });
        dlg.reword_message = found.message.clone();
        self.ui.dialog = Some(Dialog::Reword);
    }

    /// Offer the checked repos' shared head message into a blank commit buffer
    /// for an amend.
    pub fn prefill_amend_message(&mut self) {
        // Amend seeds only a blank buffer: reword's target is a dialog field
        // the user just opened for one commit, this is the persisted shared draft.
        if !self.ui.commit_message.trim().is_empty() {
            return;
        }
        let paths: Vec<PathBuf> = self
            .commit_targets()
            .iter()
            .map(|r| r.path.clone())
            .collect();
        if let Some(message) = commit_across::shared_head_message(self.executor.as_ref(), &paths) {
            self.ui.commit_message = message;
        }
    }

    pub fn open_tag_dialog(&mut self, target: Option<&str>) {
        let dlg = &mut self.ui.dlg;
        dlg.tag_type = TagType::Annotated;
        dlg.tag_target_picker_open = false;
        dlg.tag_candidates = None;
        dlg.tag_existing = None;
        dlg.tag_signing_key = None;
        dlg.tag_signing_key_fetched = false;
        if let Some(target) = target {
            dlg.tag_target = target.to_owned();
        }
        self.ui.dialog = Some(Dialog::Tag);
    }

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

    pub fn cherry_focus_commit(&mut self, id: String) {
        let Some(source) = self.ui.dlg.cherry_source.clone() else {
            return;
        };
        let patch = self.executor.diff_patch(
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
        let control = bulk_run::RunControl::default();
        self.ui.cherry_run =
            Some(RunSeed::start(&fleet, &control).cherry(source.name(), commits.len(), stop));
        let snapshots: HashMap<RootId, PathBuf> = dispatch_roots
            .iter()
            .filter_map(|id| self.multi.by_id(id).map(|r| (id.clone(), r.path.clone())))
            .collect();
        let executor = self.executor.clone();
        let source_path = source.0.to_path_buf();
        let label = format!("cherry-pick across {} repos", targets.len());
        let step = move |rid: &RootId| -> TgResult<()> {
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
        self.spawn_cascade_pass(dispatch_roots, control, label, step, done_rows_only);
    }

    pub fn cherry_resolve(&mut self, root: RootId) {
        self.selected_root = Some(root);
        self.ui.repo_selection.clear();
        self.ui.cherry_run = None;
    }

    pub fn add_worktree(&mut self, path: PathBuf, branch: String) {
        if let Some(root) = self.selected_root.clone() {
            self.dispatch(Operation::WorktreeAdd { root, path, branch });
        }
    }

    pub fn update_submodule(&mut self, path: PathBuf, init: bool) {
        let Some(root) = self.selected_root.clone() else {
            return;
        };
        let label = if init {
            "Update submodule (init)".to_string()
        } else {
            "Update submodule".to_string()
        };
        self.dispatch(Operation::custom(
            label,
            Affected::Root(root.clone()),
            move |v| v.submodule_update(root.as_path(), &path, init),
        ));
    }

    pub fn init_repo(&mut self) {
        if let Err(e) = self.executor.init(&self.project_dir) {
            self.last_error = Some(e.to_string());
            return;
        }
        let _ = crate::persistence::add_mapping(&self.project_dir, &self.project_dir, Vcs::Git);
        self.rescan();
    }

    pub fn open_project(&mut self, dir: &Path) {
        self.project_dir = dir.to_path_buf();
        self.multi = MultiRootManager::default();
        self.selected_root = None;
        self.ui.preview_change = None;
        self.ui.workspace_picker_open = false;
        self.caches.invalidate_all();
        self.worktree.clear();
        self.rescan();
        self.ui.welcome_visible = false;
        self.record_recent(dir);
    }

    pub fn attach_workspace(&mut self, dir: &Path) {
        self.project_dir = dir.to_path_buf();
        self.multi = MultiRootManager::default();
        self.selected_root = None;
        self.ui.preview_change = None;
        self.ui.workspace_picker_open = false;
        self.caches.invalidate_all();
        self.worktree.clear();

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
        self.hide_project_metadata();
        self.ui.welcome_visible = false;

        let count = self.multi.roots.len();
        if let Some(cfg) = self.recents_config() {
            let recents = crate::recents::record_workspace(&cfg, dir, count);
            self.ui.recent_projects = recents.projects;
        }
    }

    pub fn open_recent(&mut self, recent: &crate::recents::RecentProject) {
        if !recent.path.is_dir() {
            self.ui.toast = Some(crate::state::Toast::error(format!(
                "Workspace no longer exists: {}",
                recent.path.display()
            )));
            return;
        }
        match recent.kind {
            crate::recents::RecentKind::Workspace => self.attach_workspace(&recent.path),
            crate::recents::RecentKind::Project => self.open_project(&recent.path),
        }
    }

    pub fn initialize_and_enter(&mut self, dir: &Path) {
        if turbogit_services::multi_root::is_inside_work_tree(dir) {
            self.ui.toast = Some(Toast::error(format!(
                "The selected directory {} is already under Git.",
                dir.display()
            )));
            return;
        }
        if let Err(e) = self.executor.init(dir) {
            self.last_error = Some(e.to_string());
            return;
        }
        let _ = crate::persistence::add_mapping(dir, dir, Vcs::Git);
        self.open_project(dir);
    }

    pub fn close_all_projects(&mut self) {
        self.multi = MultiRootManager::default();
        self.selected_root = None;
        self.ui.preview_change = None;
        self.ui.workspace_picker_open = false;
        self.caches.invalidate_all();
        self.worktree.clear();
        self.ui.welcome_visible = true;
    }

    pub fn record_recent(&mut self, dir: &Path) {
        if let Some(cfg) = self.recents_config() {
            let recents = crate::recents::record(&cfg, dir);
            self.ui.recent_projects = recents.projects;
        }
    }

    pub fn remove_recent(&mut self, path: &Path) -> bool {
        let Some(index) = self.ui.recent_projects.iter().position(|p| p.path == path) else {
            return false;
        };
        let removed = self.ui.recent_projects.remove(index);
        self.ui.welcome_branch_cache.remove(path);
        if let Some(cfg) = self.recents_config() {
            crate::recents::remove(&cfg, path);
        }
        self.ui.toast = Some(Toast::info(format!(
            "Removed '{}' from recent projects.",
            removed.name
        )));
        true
    }

    pub fn invalidate_welcome_branches(&mut self) {
        self.ui.welcome_branch_cache.clear();
    }

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

    pub fn clone_into(&mut self, url: &str, dest: &Path, depth: Option<usize>) {
        match GitExecutor::clone(&*self.executor, url, dest, depth) {
            Ok(()) => {
                self.ui.welcome_clone_url.clear();
                self.open_project(dest);
                self.ui.toast = Some(Toast::success("Repository cloned"));
            }
            Err(e) => {
                self.last_error = Some(e.to_string());
                self.ui.toast = Some(Toast::error(format!("Clone failed: {e}")));
            }
        }
    }

    pub fn git_env_check(&mut self) -> Result<String, String> {
        let key = self.settings.git_executable.clone();
        if let Some((cached, res)) = &self.ui.git_env_check
            && *cached == key
        {
            return res.clone();
        }
        let res = turbogit_engine::resolve_git_version(&self.settings).map_err(|e| e.to_string());
        self.ui.git_env_check = Some((key, res.clone()));
        res
    }

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
                AppEvent::LogLoaded {
                    root,
                    commits,
                    mode,
                } => {
                    self.fetch_admission
                        .release(InflightFetch::Log(root.clone()));
                    match commits {
                        Ok(batch) => {
                            self.caches
                                .clear_log_failure(&LogRead::Commits(root.clone()));
                            self.settle_log_batch(&root, mode, batch);
                            self.apply_pending_reselect(&root);
                        }
                        Err(e) => {
                            let message = e.to_string();
                            self.caches.record_log_failure(
                                LogRead::Commits(root.clone()),
                                message.clone(),
                            );
                            self.last_error = Some(message);
                        }
                    }
                }
                AppEvent::LogBatchLoaded {
                    root,
                    scope,
                    commits,
                    mode,
                } => {
                    self.fetch_admission
                        .release(InflightFetch::ScopedLog(root.clone(), scope.clone()));
                    match commits {
                        Ok(batch) => {
                            self.caches
                                .clear_log_failure(&LogRead::Scope(root.clone(), scope.clone()));
                            if matches!(
                                self.caches
                                    .settle_scoped_log_batch(&root, &scope, mode, batch),
                                LogBatchSettle::Torn
                            ) {
                                self.load_more_scoped_log(&root, &scope);
                            }
                        }
                        Err(e) => {
                            let message = e.to_string();
                            self.caches
                                .record_log_failure(LogRead::Scope(root, scope), message.clone());
                            self.last_error = Some(message);
                        }
                    }
                }
                AppEvent::RefsLoaded { root, deco } => {
                    self.fetch_admission
                        .release(InflightFetch::Refs(root.clone()));
                    match deco {
                        Ok(d) => self.caches.store_refs(root, d),
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
                AppEvent::FileStatsLoaded {
                    root,
                    commit,
                    stats,
                } => {
                    self.fetch_admission
                        .release(InflightFetch::FileStats(root.clone(), commit.clone()));
                    match stats {
                        Ok(s) => {
                            self.caches
                                .clear_log_failure(&LogRead::Details(root.clone(), commit.clone()));
                            self.caches.store_file_stats(root, commit, s.files)
                        }
                        Err(e) => {
                            let message = e.to_string();
                            self.caches.record_log_failure(
                                LogRead::Details(root.clone(), commit.clone()),
                                message.clone(),
                            );
                            self.last_error = Some(message);
                            self.caches.store_file_stats(root, commit, Vec::new());
                        }
                    }
                }
                AppEvent::OpCompleted {
                    kind,
                    label,
                    affected,
                    result,
                } => {
                    self.ui.busy = false;
                    self.guard_rewrite_undo(&affected);
                    match crate::settle::op_completed(self, kind, label, affected, result) {
                        crate::settle::Control::Continue => continue,
                        crate::settle::Control::Done => {}
                    }
                }
                AppEvent::Error(msg) => {
                    self.ui.busy = false;
                    self.last_error = Some(msg);
                }
                AppEvent::DiffReady {
                    key,
                    policies,
                    result,
                } => {
                    crate::keyed_read::settle_diff(self, key, policies, result);
                }
                AppEvent::BlameReady { key, result } => {
                    crate::keyed_read::settle_blame(self, key, result);
                }
                AppEvent::FileBytesReady { key, old, new } => {
                    crate::keyed_read::settle_pane(self, key, old, new);
                }
                AppEvent::ContextReady {
                    key,
                    comparison,
                    file,
                    result,
                } => {
                    crate::keyed_read::settle_context(self, key, comparison, file, result);
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
                AppEvent::WorktreesLoaded {
                    root,
                    worktrees,
                    epoch,
                } => {
                    if !self.worktree.settle(&root, epoch) {
                        continue;
                    }
                    match worktrees {
                        Ok(w) => self.caches.store_worktrees(root, w),
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
                AppEvent::WorktreesMutated { root } => {
                    self.invalidate_worktree_cache(&root);
                }
                AppEvent::WorktreeDirty { root, path, dirty } => {
                    self.worktree.settle_dirty(&root, &path);
                    match dirty {
                        Ok(v) => self.caches.update_worktree_dirty(&root, &path, v),
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
                AppEvent::SubmodulesLoaded { root, submodules } => {
                    self.fetch_admission
                        .release(InflightFetch::Submodules(root.clone()));
                    match submodules {
                        Ok(s) => self.caches.store_submodules(root, s),
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
                AppEvent::StashesLoaded { root, stashes } => {
                    self.fetch_admission
                        .release(InflightFetch::Stashes(root.clone()));
                    match stashes {
                        Ok(s) => self.caches.store_stashes(root, s),
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
                AppEvent::BulkCompleted { label, results } => {
                    self.ui.busy = false;
                    crate::settle::bulk_completed(self, label, results);
                }
                AppEvent::RemoteDefined { root, name, result } => {
                    self.ui.busy = false;
                    let (message, kind, activity) = match result {
                        Ok(()) => {
                            self.ui.dlg.push_define_remote = false;
                            self.ui.dlg.push_new_remote_url.clear();
                            self.ui.dlg.push_remote = name.clone();
                            self.ui.dlg.push_remote_add_error = None;
                            self.refresh(Affected::Root(root.clone()));
                            (
                                format!("Add remote {name}"),
                                ToastKind::Success,
                                crate::activity::ActivityKind::Success,
                            )
                        }
                        Err(e) => {
                            let detail = e.to_string();
                            self.ui.dlg.push_remote_add_error = Some(detail.clone());
                            (
                                format!("Add remote {name}: {detail}"),
                                ToastKind::Error,
                                crate::activity::ActivityKind::Error,
                            )
                        }
                    };
                    self.show_toast(Toast {
                        kind,
                        message: message.clone(),
                    });
                    self.ui.activity.push(crate::activity::ActivityEntry {
                        at: chrono::Local::now(),
                        repo: crate::settle::activity_repo_label(&Affected::Root(root)),
                        message,
                        kind: activity,
                    });
                }
                AppEvent::BulkRunProgress { event } => {
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
            }
            drained += 1;
        }
        drained
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_root.as_ref().map(|r| r.0.to_path_buf())
    }

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
        self.save_durable_ui();
    }

    fn bulk_selected_roots(&self) -> Vec<Root> {
        self.multi
            .roots
            .iter()
            .filter(|r| self.ui.repo_selection.contains(&r.id))
            .cloned()
            .collect()
    }

    pub fn bulk_preflight(&self, op: BulkOp) -> Preflight {
        let roots = self.bulk_selected_roots();
        bulk_ops::preflight(op, &roots, &self.settings, self.ui.bulk_rebase, &|id| {
            self.caches.ahead_behind(id).unwrap_or((0, 0))
        })
    }

    pub fn bulk_branch_preflight(&self) -> bulk_ops::BranchPreflight {
        let roots = self.bulk_selected_roots();
        bulk_ops::branch_preflight(
            &roots,
            self.ui.bulk_branch_name.trim(),
            self.ui.bulk_from_upstream,
            &|id| self.caches.ahead_behind(id).unwrap_or((0, 0)),
        )
    }

    pub fn commit_targets(&self) -> Vec<Root> {
        self.multi
            .roots
            .iter()
            .filter(|r| self.is_commit_target(r))
            .cloned()
            .collect()
    }

    pub fn has_commit_target(&self) -> bool {
        self.multi.roots.iter().any(|r| self.is_commit_target(r))
    }

    fn is_commit_target(&self, root: &Root) -> bool {
        !self.ui.commit_repo_unchecks.contains(&root.id)
            && root.status.changes.iter().any(|c| c.staged)
    }

    pub fn run_commit_across(&mut self, message: &str, amend: bool) {
        let roots = self.bulk_selected_roots();
        self.run_commit_across_roots(roots, message, amend);
    }

    pub fn run_commit_across_roots(&mut self, roots: Vec<Root>, message: &str, amend: bool) {
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
        let label = if amend {
            format!("Amend on {len} repos", len = planned_ids.len())
        } else {
            format!("Commit on {len} repos", len = planned_ids.len())
        };
        let executor = self.executor.clone();
        let message = message.to_string();
        let control = bulk_run::RunControl::default();
        self.ui.bulk_run = Some(RunSeed::start(&fleet, &control).bulk(
            BulkOp::Commit,
            amend,
            String::new(),
            String::new(),
            MergeOpts::default(),
            Vec::new(),
        ));
        let step = move |rid: &RootId| -> TgResult<()> {
            let path = roots
                .iter()
                .find(|r| r.id == *rid)
                .map(|r| r.path.clone())
                .ok_or_else(|| TgError::Other("root not registered".into()))?;
            commit_across::run_one(executor.as_ref(), &path, &message, amend).map(|_| ())
        };
        self.spawn_cascade_pass(planned_ids, control, label, step, stopped_as_failed);
    }

    pub fn open_bulk_preflight(&mut self, op: BulkOp) {
        self.ui.bulk_op = Some(op);
        self.ui.bulk_rebase = self.settings.update_method == UpdateMethod::Rebase;
        if op == BulkOp::CreateBranch {
            self.ui.bulk_from_upstream = true;
        }
        if op == BulkOp::Custom {
            self.ui.bulk_command = String::new();
            self.ui.bulk_command_armed = false;
        }
    }

    pub fn merge_dialog_opts(&self) -> MergeOpts {
        turbogit_services::integrate_service::merge_flags(
            self.ui.dlg.merge_strategy,
            self.ui.dlg.merge_no_ff,
            self.ui.dlg.merge_verify_signatures,
            self.ui.dlg.merge_allow_unrelated,
        )
    }

    pub fn merge_cascade_siblings(&self) -> Vec<RootId> {
        let Some(focused) = self.selected_root.clone() else {
            return Vec::new();
        };
        turbogit_services::integrate_service::cascade_siblings(&self.multi.roots, &focused)
    }

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

    pub fn rebase_dialog_opts(&self) -> turbogit_domain::model::RebaseOpts {
        turbogit_services::integrate_service::rebase_mode_opts(
            self.ui.dlg.rebase_mode,
            self.ui.dlg.rebase_update_refs,
            self.ui.dlg.rebase_keep_empty,
            self.ui.dlg.rebase_autosquash,
        )
    }

    pub fn rebase_affected_siblings(&self) -> Vec<RootId> {
        let Some(focused) = self.selected_root.clone() else {
            return Vec::new();
        };
        turbogit_services::integrate_service::rebase_affected(&self.multi.roots, &focused)
    }

    pub fn open_rebase_affected_list(&mut self) {
        let siblings = self.rebase_affected_siblings();
        if siblings.is_empty() {
            return;
        }
        self.ui.repo_selection = siblings.into_iter().collect();
        self.ui.dialog = None;
    }

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

    pub fn open_rename_branch(&mut self, id: &RootId, name: &str) {
        let tracking = self
            .multi
            .by_id(id)
            .and_then(|r| r.branches.iter().find(|b| b.name == name))
            .and_then(|b| b.tracking.clone());
        self.ui.dlg.rename_branch_root = Some(id.clone());
        self.ui.dlg.rename_branch_name = name.to_string();
        self.ui.dlg.rename_branch_new = String::new();
        self.ui.dlg.rename_branch_upstream = tracking;
        self.ui.dlg.rename_branch_unset_upstream = false;
        self.ui.dialog = Some(Dialog::RenameBranch);
        self.ui.branches_popup = false;
    }

    pub fn rename_branch(&mut self, id: &RootId, old: &str, new: &str, unset_upstream: bool) {
        let root = id.clone();
        let old = old.to_string();
        let new = new.to_string();
        self.dispatch(Operation::custom(
            format!("Rename {old} → {new}"),
            Affected::Root(root.clone()),
            move |v| branch_service::rename(v, root.as_path(), &old, &new, unset_upstream),
        ));
    }

    pub fn open_compare(&mut self, id: &RootId, other: &str) {
        let Some(current) = self.multi.by_id(id).and_then(|r| r.current_branch.clone()) else {
            return;
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

    pub fn open_get_from_branch(&mut self, id: &RootId, source: &str) {
        let Some(current) = self.multi.by_id(id).and_then(|r| r.current_branch.clone()) else {
            return;
        };
        self.ui.dlg.get_from_branch_root = Some(id.clone());
        self.ui.dlg.get_from_branch_source = source.to_string();
        self.ui.dlg.get_from_branch_current = current;
        self.ui.dlg.get_from_branch_swapped = false;
        self.reload_get_from_branch_rows();
        self.ui.dialog = Some(Dialog::GetFromBranch);
        self.ui.branches_popup = false;
    }

    pub fn swap_get_from_branch(&mut self) {
        self.ui.dlg.get_from_branch_swapped = !self.ui.dlg.get_from_branch_swapped;
        self.reload_get_from_branch_rows();
    }

    pub fn reload_get_from_branch_rows(&mut self) {
        let (Some(root), Some(source)) = (
            self.ui.dlg.get_from_branch_root.clone(),
            Some(self.ui.dlg.get_from_branch_source.clone()),
        ) else {
            return;
        };
        if source.is_empty() {
            return;
        }
        let base_is_branch = !self.ui.dlg.get_from_branch_swapped;
        match turbogit_services::integrate_service::branch_file_changes(
            self.executor.as_ref(),
            root.as_path(),
            &source,
            base_is_branch,
        ) {
            Ok(rows) => {
                self.ui.dlg.get_from_branch_rows = rows;
                self.ui.dlg.get_from_branch_error = None;
            }
            Err(e) => {
                self.ui.dlg.get_from_branch_rows = Vec::new();
                self.ui.dlg.get_from_branch_error = Some(e.to_string());
            }
        }
    }

    pub fn open_unmerged_commits(&mut self, unmerged: &UnmergedCommits) {
        self.ui.dlg.unmerged_view = Some(unmerged.clone());
        self.ui.dialog = Some(Dialog::BranchNotFullyMerged);
    }

    pub fn unmerged_restore_available(&self, unmerged: &UnmergedCommits) -> bool {
        unmerged.tip_sha.is_some()
            && self.multi.by_id(&unmerged.root).is_some_and(|r| {
                !r.branches
                    .iter()
                    .any(|b| b.name == unmerged.branch && b.kind == BranchKind::Local)
            })
    }

    pub fn restore_unmerged_branch(&mut self, unmerged: &UnmergedCommits) {
        if !self.unmerged_restore_available(unmerged) {
            return;
        }
        let Some(tip_sha) = unmerged.tip_sha.clone() else {
            return;
        };
        let root = unmerged.root.clone();
        let name = unmerged.branch.clone();
        self.ui.branches_undo = None;
        self.dispatch(Operation::custom(
            format!("Restore branch {name}"),
            Affected::Root(root.clone()),
            move |v| v.branch_create(root.as_path(), &name, false, Some(&tip_sha)),
        ));
    }

    pub fn open_merge_into(&mut self, _id: &RootId, name: &str) {
        self.ui.dlg.merge_target = name.to_string();
        self.ui.dlg.merge_source_picker_open = false;
        self.ui.dlg.merge_preview = None;
        self.ui.dialog = Some(Dialog::Merge);
        self.ui.branches_popup = false;
    }

    pub fn checkout_detached_op(&mut self, root: &RootId, rev: &str) {
        let path = root.clone();
        let rev = rev.to_string();
        self.dispatch(Operation::custom(
            format!("Checkout at {}", short_sha(&rev)),
            Affected::Root(root.clone()),
            move |v| v.checkout_detached(path.as_path(), &rev),
        ));
    }

    pub fn checkout_detached_set_aside(&mut self, root: &RootId, rev: &str) {
        let path = root.clone();
        let rev = rev.to_string();
        self.dispatch(Operation::custom(
            format!("Set aside changes · Checkout at {}", short_sha(&rev)),
            Affected::Root(root.clone()),
            move |v| {
                v.stash_push(path.as_path(), "set aside before checkout", false)?;
                v.checkout_detached(path.as_path(), &rev)
            },
        ));
    }

    pub fn checkout_branch_op(&mut self, root: &RootId, kind: BranchKind, name: &str) {
        let affected = Affected::Root(root.clone());
        let nm = name.to_string();
        match kind {
            BranchKind::Local => {
                let path = root.clone();
                self.dispatch(Operation::custom(
                    format!("Checkout {nm}"),
                    affected,
                    move |v| v.branch_checkout(path.as_path(), &nm),
                ));
            }
            BranchKind::Remote => {
                let path = root.clone();
                self.dispatch(Operation::custom(
                    format!("Checkout {nm} (new local)"),
                    affected,
                    move |v| {
                        let start = format!("origin/{nm}");
                        v.branch_create(path.as_path(), &nm, true, Some(&start))?;
                        v.set_branch_upstream(path.as_path(), &nm, &start)
                    },
                ));
            }
        }
    }

    pub fn checkout_branch_set_aside(&mut self, root: &RootId, kind: BranchKind, name: &str) {
        let affected = Affected::Root(root.clone());
        let nm = name.to_string();
        match kind {
            BranchKind::Local => {
                let path = root.clone();
                self.dispatch(Operation::custom(
                    format!("Set aside changes · Checkout {nm}"),
                    affected,
                    move |v| {
                        v.stash_push(path.as_path(), "set aside before checkout", false)?;
                        v.branch_checkout(path.as_path(), &nm)
                    },
                ));
            }
            BranchKind::Remote => {
                let path = root.clone();
                self.dispatch(Operation::custom(
                    format!("Set aside changes · Checkout {nm} (new local)"),
                    affected,
                    move |v| {
                        let start = format!("origin/{nm}");
                        v.stash_push(path.as_path(), "set aside before checkout", false)?;
                        v.branch_create(path.as_path(), &nm, true, Some(&start))?;
                        v.set_branch_upstream(path.as_path(), &nm, &start)
                    },
                ));
            }
        }
    }

    pub fn rebase_branch_onto_current(&mut self, root: &RootId, branch: &str, current: &str) {
        self.dispatch(Operation::rebase_branch_onto(root, branch, current));
    }

    pub(crate) fn fetch_report(&mut self, affected: &Affected) -> String {
        let baselines: Vec<(RootId, Vec<String>)> = match affected {
            Affected::All => std::mem::take(&mut self.ui.branches_fetch_before),
            Affected::Root(id) => {
                let pos = self
                    .ui
                    .branches_fetch_before
                    .iter()
                    .position(|(rid, _)| rid == id);
                pos.map(|i| vec![self.ui.branches_fetch_before.remove(i)])
                    .unwrap_or_default()
            }
        };
        let new: usize = baselines
            .iter()
            .map(|(id, before)| {
                self.multi
                    .roots
                    .iter()
                    .filter(|r| &r.id == id)
                    .flat_map(|r| {
                        r.branches
                            .iter()
                            .filter(|b| b.kind == BranchKind::Remote)
                            .map(|b| b.name.clone())
                    })
                    .filter(|name| !before.contains(name))
                    .count()
            })
            .sum();
        self.ui.branches_last_fetch = Some(chrono::Utc::now());
        if new > 0 {
            format!("Fetch · {new} new remote branches")
        } else {
            "Fetch · nothing changed".to_string()
        }
    }

    pub fn bulk_command_is_destructive(&self) -> bool {
        bulk_ops::parse_command(&self.ui.bulk_command)
            .map(|args| bulk_ops::destructive_reason(&args).is_some())
            .unwrap_or(false)
    }

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
        self.save_durable_ui();
    }

    fn spawn_cascade_pass(
        &self,
        roots: Vec<RootId>,
        control: bulk_run::RunControl,
        label: String,
        step: impl Fn(&RootId) -> TgResult<()> + Send + Sync + 'static,
        fold: impl FnOnce(Vec<(RootId, bulk_run::RunOutcome)>) -> Vec<(RootId, TgResult<()>)>
        + Send
        + 'static,
    ) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let on_event = |event: bulk_run::RunEvent| {
                let _ = tx.send(AppEvent::BulkRunProgress { event });
            };
            let outcomes = bulk_run::run_cascade(
                &roots,
                bulk_run::DEFAULT_WORKERS,
                &step,
                &on_event,
                &control,
            );
            post_bulk_completed(&tx, label, fold(outcomes));
        });
    }

    pub fn run_bulk_confirmed(&mut self, plan: BulkPlan) {
        self.ui.bulk_op = None;
        self.ui.busy = true;
        if plan.op == BulkOp::Custom {
            let command = plan.command.clone();
            self.record_custom_command(&command);
        }

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
                let roots = self.bulk_selected_roots();
                let pf = bulk_ops::preflight(plan.op, &roots, &self.settings, plan.rebase, &|id| {
                    self.caches.ahead_behind(id).unwrap_or((0, 0))
                });
                for row in pf.rows.iter().filter(|r| {
                    r.outcome == Err(bulk_ops::SkipReason::ProtectedBranch)
                        && !plan.roots.contains(&r.root)
                }) {
                    self.ui.activity.push(crate::activity::ActivityEntry {
                        at: chrono::Local::now(),
                        repo: Some(row.name.clone()),
                        message: format!(
                            "{}: '{}' is a protected branch — skipped",
                            plan.op.label(),
                            row.branch.as_deref().unwrap_or("?")
                        ),
                        kind: crate::activity::ActivityKind::Warning,
                    });
                }
                pf.rows
                    .iter()
                    .map(|row| {
                        let skip = if plan.roots.contains(&row.root) {
                            None
                        } else {
                            Some(match row.outcome {
                                Err(bulk_ops::SkipReason::ProtectedBranch) => format!(
                                    "{} — {}",
                                    bulk_ops::SkipReason::ProtectedBranch.label(),
                                    row.branch.as_deref().unwrap_or("?")
                                ),
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
        let control = bulk_run::RunControl::default();
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
        let merge_opts = if plan.op == BulkOp::Merge {
            self.merge_dialog_opts()
        } else {
            MergeOpts::default()
        };
        let op = plan.op;
        let rebase = plan.rebase;
        let branch = plan.branch.clone();
        let command = plan.command.clone();
        let merge_for_view = merge_opts.clone();
        self.ui.bulk_run = Some(RunSeed::start(&fleet, &control).bulk(
            op,
            rebase,
            branch,
            command,
            merge_for_view,
            undo,
        ));
        let roots = plan.roots.clone();
        self.dispatch_bulk_pass(plan, merge_opts, roots, control);
    }

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
        let snapshots: HashMap<RootId, Root> = roots
            .iter()
            .filter_map(|rid| self.multi.by_id(rid).map(|r| (rid.clone(), r.clone())))
            .collect();
        let settings = self.settings.clone();
        let executor = self.executor.clone();
        let step_plan = bulk_ops::BulkPlan {
            op,
            roots: Vec::new(),
            rebase,
            branch,
            command,
        };
        let step = move |rid: &RootId| -> TgResult<()> {
            let Some(root) = snapshots.get(rid) else {
                return Err(TgError::Other("root not registered".into()));
            };
            bulk_ops::run_step(
                op,
                executor.as_ref(),
                root,
                &step_plan,
                &settings,
                &merge_opts,
            )
        };
        self.spawn_cascade_pass(roots, control, op.label().to_string(), step, done_rows_only);
    }

    pub fn bulk_stop_remaining(&mut self) {
        if let Some(view) = &self.ui.bulk_run {
            view.control.stop();
        }
    }

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

    pub fn bulk_resolve(&mut self, root: RootId) {
        self.selected_root = Some(root);
        self.ui.repo_selection.clear();
        self.ui.bulk_run = None;
    }

    pub fn bulk_rollback(&mut self, record_id: u64) -> RollbackOutcome {
        let Some(rec) = self
            .ui
            .bulk_history
            .iter()
            .find(|r| r.id == record_id)
            .cloned()
        else {
            return RollbackOutcome::Refused;
        };
        if !rec.reversible() || rec.rolled_back {
            return RollbackOutcome::Refused;
        }
        let mut failures = 0usize;
        let mut restored = 0usize;
        for row in &rec.repos {
            if row.outcome != crate::bulk_history::RepoOutcome::Done {
                continue;
            }
            let done = if row.created {
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
            if done {
                restored += 1;
            } else {
                failures += 1;
            }
        }
        if failures > 0 {
            let message = format!("Rollback incomplete · {} repo(s) not restored", failures);
            self.ui.toast = Some(Toast {
                kind: ToastKind::Error,
                message: message.clone(),
            });
            self.ui.activity.push(crate::activity::ActivityEntry {
                at: chrono::Local::now(),
                repo: Some("project".to_string()),
                message,
                kind: crate::activity::ActivityKind::Error,
            });
            return RollbackOutcome::Incomplete {
                branch: rec.branch.clone(),
                restored,
                unrecovered: failures,
            };
        }
        if let Some(rec) = self.ui.bulk_history.iter_mut().find(|r| r.id == record_id) {
            rec.rolled_back = true;
        }
        let message = format!("Rolled back · {}", rec.branch);
        self.ui.toast = Some(Toast {
            kind: ToastKind::Success,
            message: message.clone(),
        });
        self.ui.activity.push(crate::activity::ActivityEntry {
            at: chrono::Local::now(),
            repo: Some("project".to_string()),
            message: format!("{message} · {restored} repo(s) restored"),
            kind: crate::activity::ActivityKind::Success,
        });
        self.refresh(Affected::All);
        self.save_durable_ui();
        RollbackOutcome::Applied {
            branch: rec.branch.clone(),
            roots: restored,
        }
    }

    pub fn rollback_divergent_roots(&mut self) -> RollbackOutcome {
        let Some(offer) = self.ui.branch_divergence.take() else {
            return RollbackOutcome::Refused;
        };
        let outcome = self.bulk_rollback(offer.record_id);
        if outcome == RollbackOutcome::Refused {
            self.ui.activity.push(crate::activity::ActivityEntry {
                at: chrono::Local::now(),
                repo: Some("project".to_string()),
                message: format!(
                    "Rollback refused · {} · the run's undo record is no longer available",
                    offer.branch
                ),
                kind: crate::activity::ActivityKind::Warning,
            });
        }
        outcome
    }

    pub fn keep_divergent_branches(&mut self) {
        let Some(offer) = self.ui.branch_divergence.take() else {
            return;
        };
        self.ui.activity.push(crate::activity::ActivityEntry {
            at: chrono::Local::now(),
            repo: Some("project".to_string()),
            message: crate::settle::divergence_report(&offer, "kept divergent"),
            kind: crate::activity::ActivityKind::Warning,
        });
    }

    pub fn show_welcome(&self) -> bool {
        self.multi.roots.is_empty() || self.ui.welcome_visible
    }

    pub(crate) fn arm_rewrite_undo(&mut self, pending: RewriteUndoPending) {
        let tip = self
            .executor
            .resolve_revision(pending.root.as_path(), "HEAD");
        self.ui.rewrite_undo = tip.ok().map(|tip_sha| RewriteUndo {
            root: pending.root,
            verb: pending.verb,
            commit: pending.commit,
            tip_sha,
            created_at: std::time::Instant::now(),
            withheld: false,
        });
    }

    fn guard_rewrite_undo(&mut self, affected: &Affected) {
        let Some(offer) = self.ui.rewrite_undo.as_ref() else {
            return;
        };
        let covered = match affected {
            Affected::All => true,
            Affected::Root(id) => id == &offer.root,
        };
        if !covered {
            return;
        }
        let root = offer.root.clone();
        let tip = offer.tip_sha.clone();
        let moved = self
            .executor
            .resolve_revision(root.as_path(), "HEAD")
            .map(|head| head != tip)
            .unwrap_or(true);
        if let Some(offer) = self.ui.rewrite_undo.as_mut() {
            offer.withheld = moved;
        }
    }
}

struct RunSeed {
    rows: Vec<bulk_run::RunRow>,
    workers: usize,
    started_at: std::time::Instant,
    elapsed: std::time::Duration,
    eta: Option<std::time::Duration>,
    control: bulk_run::RunControl,
    history_id: u64,
}

impl RunSeed {
    fn start(fleet: &[(RootId, String, Option<String>)], control: &bulk_run::RunControl) -> Self {
        let workers = bulk_run::DEFAULT_WORKERS;
        Self {
            rows: bulk_run::monitor_rows(fleet, workers),
            workers,
            started_at: std::time::Instant::now(),
            elapsed: std::time::Duration::ZERO,
            eta: None,
            control: control.clone(),
            history_id: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    fn bulk(
        self,
        op: BulkOp,
        rebase: bool,
        branch: String,
        command: String,
        merge_opts: MergeOpts,
        undo: Vec<crate::bulk_history::UndoRow>,
    ) -> crate::bulk_run_view::BulkRunView {
        crate::bulk_run_view::BulkRunView {
            op,
            rebase,
            branch,
            command,
            merge_opts,
            rows: self.rows,
            workers: self.workers,
            started_at: self.started_at,
            elapsed: self.elapsed,
            eta: self.eta,
            control: self.control,
            history_id: self.history_id,
            undo,
        }
    }

    fn cherry(
        self,
        source_name: String,
        commit_count: usize,
        stop_on_conflict: bool,
    ) -> crate::cherry_run_view::CherryRunView {
        crate::cherry_run_view::CherryRunView {
            source_name,
            commit_count,
            stop_on_conflict,
            rows: self.rows,
            workers: self.workers,
            started_at: self.started_at,
            elapsed: self.elapsed,
            eta: self.eta,
            control: self.control,
        }
    }
}

fn done_rows_only(outcomes: Vec<(RootId, bulk_run::RunOutcome)>) -> Vec<(RootId, TgResult<()>)> {
    outcomes
        .into_iter()
        .filter_map(|(root, outcome)| match outcome {
            bulk_run::RunOutcome::Done(result) => Some((root, result)),
            bulk_run::RunOutcome::Stopped => None,
        })
        .collect()
}

fn stopped_as_failed(outcomes: Vec<(RootId, bulk_run::RunOutcome)>) -> Vec<(RootId, TgResult<()>)> {
    outcomes
        .into_iter()
        .map(|(root, outcome)| {
            let result = match outcome {
                bulk_run::RunOutcome::Done(r) => r,
                bulk_run::RunOutcome::Stopped => Err(TgError::Other("stopped".into())),
            };
            (root, result)
        })
        .collect()
}

fn post_bulk_completed(tx: &Sender<AppEvent>, label: String, results: Vec<(RootId, TgResult<()>)>) {
    let _ = tx.send(AppEvent::BulkCompleted { label, results });
}

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
                    Err(e) => {
                        let protected_branch = matches!(&e, TgError::ProtectedBranch { .. });
                        bulk_run::RowState::Failed {
                            error: e.to_string(),
                            protected_branch,
                        }
                    }
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

pub(crate) fn short_sha(id: &str) -> String {
    short_commit_ref(id)
}

fn current_branch_ahead_behind(exec: &dyn GitExecutor, root: &Path) -> TgResult<(usize, usize)> {
    match crate::polling::current_branch_upstream(exec, root)? {
        Some((branch, upstream)) => exec.ahead_behind(root, &branch, &upstream),
        None => Ok((0, 0)),
    }
}

pub fn bulk_report_message(label: &str, results: &[(RootId, TgResult<()>)]) -> String {
    let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
    let mut msg = format!("{label} · {ok} of {} ok", results.len());
    if ok < results.len() {
        let failed: Vec<String> = results
            .iter()
            .filter(|(_, r)| r.is_err())
            .filter_map(|(id, _)| id.utf8_name())
            .map(str::to_string)
            .collect();
        msg.push_str(&format!(" · failed: {}", failed.join(", ")));
    }
    msg
}
