//! Application state: owns the Git engine (the [`GitExecutor`] seam), the
//! multi-root model, canonical settings, the project directory, the event
//! channel, and all UI-only ephemeral state. The UI reads from here and never
//! calls git directly; git work is dispatched as an
//! [`Operation`] via [`AppState::dispatch`].
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

/// Where a unit of git work runs.
///
/// A named enum field, not a trait: a `trait Dispatch` with two implementors
/// would be a seam standing in for a third mode that does not exist, and both
/// modes run inside this process. The field is private — no caller and no test
/// constructs a `Pump` or learns the name; a test selects the mode by choosing
/// a constructor ([`AppState::for_roots`] is Inline, [`AppState::launch_in`] is
/// Spawned) and then drives `dispatch` and `drain_events` like the shell does.
enum Pump {
    /// Hand the work to a worker thread; its answer arrives as an event on a
    /// later frame. What the desktop shell runs.
    Spawned,
    /// Run the work on the calling thread and push the same events into the
    /// same channel, then return. The caller settles by draining, exactly as
    /// production does — which is the point: the headless harness stops being a
    /// second, synchronous implementation of settlement. Running the work
    /// rather than settling re-entrantly is what keeps a dispatching method from
    /// needing `&mut self` twice.
    Inline,
}

/// One keyed-read fetch whose work is queued rather than run.
#[cfg(test)]
type HeldRead = Box<dyn FnOnce(Arc<dyn GitExecutor>, Sender<AppEvent>) + Send>;

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

/// Commits per log batch: every [`AppState::fetch_log`] takes exactly one, so a
/// huge history never blocks the first paint, and the pane keeps loading until
/// a batch comes back short (log paging, P1/P4). A scoped listing — a file's
/// history, a ref's, a pickaxe search — is held to the same batch size
/// (log-view-scaling 01).
pub const LOG_BATCH_SIZE: usize = 50;

/// Where a new branch starts.
///
/// Two answers, not one string. A branch name reads as a name and is chosen
/// from the dialog's branch picker; a commit is named by the log's New branch
/// item (ADR-0024) and is shown as a short reference with its subject. Which one
/// it is has to be decidable at paint time — a hash and a branch look nothing
/// alike, and a union string would be rendered by guessing, so a stale snapshot
/// would paint a branch name as a reference.
///
/// [`Self::Unset`] is the third state, and the one most open sites use: the
/// dialog resolves it to the current branch when it opens, which is the default
/// issue 08 asks for (creating from an unexpected base is a silent mistake).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NewBranchBase {
    /// Nothing chosen yet — the dialog fills in the current branch.
    #[default]
    Unset,
    /// A named branch of the focused root, which the branch picker can select.
    Branch(String),
    /// One commit of the focused root, named by the log row that was
    /// right-clicked. No branch picker row can match it.
    Commit(CommitId),
}

impl NewBranchBase {
    /// The base as git's start-point argument, or `None` to let git choose HEAD.
    pub fn start_point(&self) -> Option<&str> {
        match self {
            Self::Unset => None,
            Self::Branch(name) => Some(name.as_str()),
            Self::Commit(id) => Some(id.as_str()),
        }
    }
}

/// Which of the two `Apply patch` routes a patch arrives by.
///
/// Two variants rather than one field that may be empty, because the reference's
/// second verb is defined by what it does NOT ask for: `Apply patch from
/// Clipboard` "skips the file path entirely", and a route that merely hid the
/// field while still reading it would leave a stale path one keystroke from being
/// applied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PatchSource {
    /// Read a patch file chosen through [`AppState::patch_reader`] or typed in
    /// full. The reference's `Git | Patch | Apply patch…`.
    #[default]
    File,
    /// The patch text is pasted into the window and never touches the filesystem.
    Clipboard,
}

/// Persistent input fields for the modal dialogs (kept across redraws).
#[derive(Default)]
pub struct DialogState {
    // Push
    pub push_remote: String,
    pub push_branch: String,
    /// The Push panel's inline `Define remote` entry, open in place of the
    /// remote name for a repository with no remote. The reference never sends
    /// the user to a settings dialog for this: the link and its name+URL entry
    /// live where the remote name would be, and the add saves immediately.
    pub push_define_remote: bool,
    /// The entry's two fields.
    pub push_new_remote_name: String,
    pub push_new_remote_url: String,
    /// Why the last inline add failed, in git's own words — the panel paints
    /// `Could not add remote: {0}` from it and stays open.
    pub push_remote_add_error: Option<String>,
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
    /// `--onto`: reveals the two ordered base fields below the option rows.
    /// Off means the dialog is a plain rebase onto [`Self::rebase_onto`].
    pub rebase_onto_enabled: bool,
    /// `--onto`'s FIRST field: the base the sequence lands on. The order of the
    /// pair is load-bearing — the two are not interchangeable and one "base"
    /// field cannot express the move — so they are two fields, never one.
    pub rebase_new_base: String,
    /// `--onto`'s SECOND field: the cutoff. Everything ABOVE it on the current
    /// branch moves; it and everything below stay.
    pub rebase_old_base: String,
    /// The `(value, resolves)` pair each `--onto` field's existence check was
    /// last run for, so one `git rev-parse` answers a value once rather than
    /// every frame.
    pub rebase_new_base_check: Option<(String, bool)>,
    pub rebase_old_base_check: Option<(String, bool)>,
    /// Last computed commits-to-rebase list (the "N COMMITS TO REBASE"
    /// box), cached with [`Self::rebase_preview_key`] so it recomputes only
    /// when the onto branch changes.
    pub rebase_preview: Option<Vec<turbogit_domain::model::RebasePlanEntry>>,
    /// The onto branch the cached preview was computed for.
    pub rebase_preview_key: Option<String>,
    pub new_branch_name: String,
    pub new_branch_checkout: bool,
    /// Where the new branch starts (issue 08): a branch, or one commit when the
    /// log's menu hands one over (ADR-0024). [`NewBranchBase::Unset`] is what
    /// the open sites leave behind, and the dialog resolves it to the current
    /// branch when it opens.
    pub new_branch_base: NewBranchBase,
    pub new_branch_base_picker_open: bool,
    // Rename branch (issue 32, branches popup row action).
    pub rename_branch_root: Option<RootId>,
    pub rename_branch_name: String,
    pub rename_branch_new: String,
    /// The upstream the branch had when the dialog opened — `None` for a branch
    /// that tracks nothing, which is what withholds the second field and its
    /// consequence line.
    pub rename_branch_upstream: Option<Upstream>,
    /// The dialog's second field: stop tracking that upstream as part of the
    /// rename. Off by default, because keeping the tracking is the case a
    /// routine rename is in.
    pub rename_branch_unset_upstream: bool,
    /// The payload of the unmerged-commits viewer — the record a delete
    /// disclosure linked to, presented as a list. Written by every path that
    /// opens the dialog and dropped by both of its buttons; a window closed by
    /// its ✕ leaves it set until the next link overwrites it, because the
    /// generic host closes windows without consulting payloads, and nothing
    /// reads it while the dialog is gone.
    pub unmerged_view: Option<UnmergedCommits>,
    // Compare branches (issue 32, branches popup row action + footer): the
    // two sides and the snapshot of commits on `compare_left` missing from
    // `compare_right`, captured once when the dialog opens (E9).
    pub compare_root: Option<RootId>,
    pub compare_left: String,
    pub compare_right: String,
    pub compare_commits: Vec<CommitId>,
    // Get from Branch: the file-level comparison the branch row's
    // `Show Diff with Working Tree` opens. Snapshot data only — the
    // rows are the service's answer, captured when the window opens and re-captured
    // when it swaps or a take lands, so the pane never touches git.
    /// The root the window is reading. One root per window: the verb is per root
    /// by construction, so the identity that writes the file is the same identity
    /// that was compared.
    pub get_from_branch_root: Option<RootId>,
    /// The branch the developer clicked — the side a take reads FROM.
    pub get_from_branch_source: String,
    /// The branch checked out in that root when the window opened, named so the
    /// header can say which two sides it is comparing.
    pub get_from_branch_current: String,
    /// `Swap Branches`: which side is the comparison's base. `false` is the flow's
    /// opening posture, where grey means "the branch has it and you do not"; the
    /// swap exchanges which paths sit on the grey and green rungs, and the list's
    /// rung order turns that into the two rows exchanging **positions**.
    pub get_from_branch_swapped: bool,
    /// The rung list itself, in the list's own order.
    pub get_from_branch_rows: Vec<turbogit_services::integrate_service::BranchFileRow>,
    /// Why the list could not be read, in git's words. The window paints it rather
    /// than showing nothing: an empty list and a failed read are different answers.
    pub get_from_branch_error: Option<String>,
    // --- Apply patch ---
    /// The root the apply writes into, carried by the window rather than read from
    /// whichever root is focused at click time. An apply is **per root**: the same
    /// discipline `Get from Branch` states, and for the same reason — a verb that
    /// takes the focused root at the last moment writes the patch into a tree the
    /// developer never named.
    pub apply_patch_root: Option<RootId>,
    /// Which of the two routes the patch arrives by. `Clipboard` is not a flag on
    /// the file route: it skips the path entirely, which is the whole of what the
    /// reference's second verb is for.
    pub apply_patch_source: PatchSource,
    /// The file route's path, as the developer left it — typed or chosen through
    /// the picker seam, and never read unless this route is the live one.
    pub apply_patch_path: String,
    /// The patch's own text: the file's contents on the file route, the pasted
    /// text on the clipboard route. One field because one thing is parsed.
    pub apply_patch_text: String,
    /// The text [`Self::apply_patch_text`] was last parsed from. The parse is
    /// memoized on it so the dialog's per-frame refresh costs a string compare,
    /// not a diff read.
    pub apply_patch_loaded: String,
    /// The path [`Self::apply_patch_text`] was last read FROM, on the file route.
    /// Held beside the loaded text because a stale read is the failure this route
    /// has: a window that applied what it had already parsed after the developer
    /// retyped the path would write a different patch than the one named.
    pub apply_patch_loaded_path: String,
    /// The parsed patch — the value the preflight asks git about and the apply
    /// writes. `None` is "the text names no file section", which the window says
    /// rather than painting an empty list.
    pub apply_patch_patch: Option<turbogit_domain::model::Patch>,
    /// Why the patch could not be read or parsed. Stated separately from an empty
    /// patch because "the file is not there" and "the file holds no diff" are
    /// different things to fix.
    pub apply_patch_error: Option<String>,
    /// The provenance a git-format patch carries, as the engine reads it. Held
    /// beside the two fields it seeds so the seeding can tell an untouched field
    /// from one the developer has since typed over.
    pub apply_patch_provenance: Option<turbogit_engine_api::patch::PatchProvenance>,
    /// The destination's author field, pre-filled from the patch when the patch
    /// has an author. Editable, because an applied patch is a local change and the
    /// local change is the developer's to attribute.
    pub apply_patch_author: String,
    /// The destination's message field, pre-filled from the patch's subject.
    pub apply_patch_message: String,
    /// `Show Diff`'s answer: git's verdict on whether the patch applies, with
    /// nothing written. `None` is "not asked yet", which is not the same as clean.
    pub apply_patch_preflight: Option<turbogit_services::patch_apply::Preflight>,
    /// The last apply's outcome, which is what the window paints its titled state
    /// from. Kept until the next apply or a close: the developer needs to be able
    /// to read the path list after the toast has gone.
    pub apply_patch_outcome: Option<turbogit_services::patch_apply::ApplyOutcome>,
    /// The slot the in-flight apply writes its outcome into, armed at dispatch.
    /// See [`crate::operation::ApplyReport`] for why the outcome cannot ride the
    /// completion event.
    pub apply_patch_report: Option<crate::operation::ApplyReport>,
    // --- Create patch ---
    //
    // One group for the reference's two create dialogs, which share a payload: the
    // comment step that names the file, then `Patch File Settings`. The fields are
    // prefixed `create_patch_` for the same reason the apply group is prefixed
    // `apply_patch_` — every other dialog's fields live in this one struct.
    /// The root the patch is generated FROM. A created patch is per root, like the
    /// apply that consumes one, and the base path, the directory-strip count and
    /// the default location all belong to that root rather
    /// than to whichever tree is focused when the button is pressed.
    pub create_patch_root: Option<RootId>,
    /// Which change the patch describes — the source the verb was reached on. `None`
    /// is "no create flow is open", which is also what clears the group.
    pub create_patch_origin: Option<PatchOrigin>,
    /// The comment step's text. Its first line becomes the file name.
    pub create_patch_comment: String,
    /// The file name derived from the comment, editable in the settings step. Held
    /// apart from the comment because the developer may rename the artifact without
    /// rewriting the words that named it.
    pub create_patch_name: String,
    /// The `Location` field: the directory the patch file is written into.
    pub create_patch_location: String,
    /// `To clipboard`: the settings dialog's other destination, which is not a flag
    /// on the location field — a patch that goes to the clipboard is not written, and
    /// naming a file it will not touch would say otherwise.
    pub create_patch_to_clipboard: bool,
    /// `Base path`: the directory the patch's paths are written relative to. Empty is
    /// the project directory, which is git's own answer and the reference's default.
    pub create_patch_base_path: String,
    /// `Reverse patch`: ask git for the change that UNDOES the origin's.
    pub create_patch_reverse: bool,
    /// `Encoding`: how the patch file's bytes are written.
    pub create_patch_encoding: PatchEncoding,
    /// The `Change Directory Paths` count: leading directories removed from every
    /// path in this patch. State rather than a frame-local, because the reference
    /// brackets it on the button so a repeated press is visible — a count that reset
    /// between frames, or only updated on close, would lose the whole point of showing
    /// it.
    pub create_patch_strip_leading: usize,
    /// How far [`Self::create_patch_strip_leading`] may go for the patch as it is now
    /// configured: the shallowest path's directory count, which moves when the base
    /// path or `Reverse patch` moves.
    pub create_patch_removable: usize,
    /// git's own bytes for the current origin — the unedited patch every transform
    /// starts from, kept as bytes because a patch body is file content. The memo that
    /// says when to re-ask git for them lives in the `create_patch` module.
    pub create_patch_raw: Option<Vec<u8>>,
    /// Why the patch could not be generated or written. Stated rather than left as an
    /// empty dialog: "git answered nothing" and "this encoding cannot hold the patch"
    /// are different things to fix.
    pub create_patch_error: Option<String>,
    /// The text the UI layer drains into the system clipboard. A slot rather than a
    /// call from `AppState`, which has no `egui::Context`: the app decides WHAT is
    /// copied and the frame that owns the clipboard performs it — the same division the
    /// log's Copy hash already holds.
    pub create_patch_clipboard: Option<String>,
    /// `Import Patches…` (Shelf view): the patch file the developer is parking, and
    /// the name it goes in under. The path field is what the `patch_reader` seam
    /// writes into, because a native open dialog has no headless equivalent.
    pub import_patch_path: String,
    pub import_patch_name: String,
    /// Why the last import was refused, kept on the dialog that was refused on.
    pub import_patch_error: Option<String>,
    /// The rewrite preflight's payload, present exactly while its dialog is.
    /// One payload for both targeted verbs (ticket 10 drop, ticket 11 reword).
    pub rewrite_preflight: Option<RewritePreflight>,
    // Reword commit's editor (ticket 11): which commit is being reworded, and
    // the message being edited. The message is live text, so it is a field
    // rather than part of the draft — the same split the plan editor's todo has.
    pub reword: Option<RewordDraft>,
    pub reword_message: String,
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
    pub cherry_preview: Option<Result<turbogit_domain::model::Patch, String>>,
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
    // Unshelve. The three fields are the dialog's whole input: which shelf, in
    // which repository, and whether the shelf survives the apply.
    /// The shelf the dialog will apply, picked from that root's own list. A name
    /// typed into this field cannot create anything: the dialog's `Name` field
    /// selects, because a shelf is made by shelving and this surface has no
    /// destination to invent.
    pub unshelve_shelf: Option<String>,
    /// The root the dialog was **opened** on, held rather than re-read so the
    /// picker cannot follow a focus change mid-dialog and offer — then apply to —
    /// a different repository's shelves.
    pub unshelve_root: Option<RootId>,
    /// `Remove successfully applied files from the shelf`: **off** on every open,
    /// which is why plain unshelve is a keeping verb. Ticked, it makes the
    /// affirmative the destructive sibling, and only the settlement arm decides
    /// whether the shelf actually goes.
    pub unshelve_remove: bool,
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

/// What the conflict resolver's single bulk control (`Apply to all N
/// remaining`) fills the still-unresolved conflicts with. The reference ships
/// two bulk buttons that differ only in whether they touch simple conflicts
/// (`Apply Non-Conflicting Changes` vs `Resolve All Simple Conflicts`); that
/// adjacent pair is a mis-click generator, so TurboGit folds both classes
/// into this one control and the side scope — the active choice vs one side —
/// is an option on it instead.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BulkApplyScope {
    /// The active conflict's recorded choice, ours as fallback — the control's
    /// original behaviour.
    #[default]
    ActiveChoice,
    /// Our (left) side for every remaining conflict.
    Ours,
    /// Their (right) side for every remaining conflict.
    Theirs,
}

/// The outcome a bulk apply has to name. The reference
/// reports the NEGATIVE result (`No conflicts were resolved automatically`)
/// instead of staying silent, and gives the positive one its own sentence —
/// two strings for two outcomes. Within one file a bulk fill resolves either
/// everything still unresolved or nothing, so there is no third outcome to
/// name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictBulkReport {
    /// Every conflict the click had to resolve was resolved.
    AllResolved,
    /// There was nothing left to resolve — reported, not silent.
    NoneResolved,
}

/// Expand/collapse state of the Branches tab's groups: Recent, Local
/// and Remote open by default, Tags collapsed — the design's sensible default.
///
/// `recent` is a fourth flag and not a reuse of the gear's `show_recent`: this
/// one says the group is open, that one says the group exists. A user who hides
/// the group has not asked for it to come back expanded next time it is shown.
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

/// What the branch tree itself remembers between frames (branch-tree-view
/// extraction, plan D2/D3): grouping, collapse, selection, the inline-rename
/// draft, the context menu, and scroll. Plain data only — the application
/// crate cannot name egui types, so no widget ids, rectangles, colors or
/// text-edit state live here. Each tree surface owns one instance (the
/// Branches tool window, the Log window's branches pane), so opening one
/// cannot affect the other.
///
/// Deliberately *not* here — that is surface policy, not tree state: the
/// search buffer and its focus intent, the repo-scope picker, delete-with-care
/// state and the undo window, fetch reporting, and the tag cache (git-read
/// data, which reaches the tree as a prop).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeState {
    /// Expand/collapse of Local / Remote / Tags.
    pub groups: BranchesGroups,
    /// View-wide remotes-on/off switch: when `false`, remote branches
    /// collapse into the per-repo rollup. The Git Log pane forces this on; the
    /// Branches surface reveals remotes per repository instead.
    pub show_remotes: bool,
    /// Repositories whose remotes the person has revealed from the rollup.
    /// Per-root, the same shape as `collapsed_remotes` below, so one repo's
    /// reveal never shows another's branches.
    pub remotes_revealed: HashSet<RootId>,
    /// Per-remote collapse within the expanded remote groups: each entry is
    /// `(root, remote)` whose group is collapsed.
    pub collapsed_remotes: HashSet<(RootId, String)>,
    /// The selected branch, by name, and its owning repository. Two
    /// repositories that both have a `main` are never interchangeable.
    pub selected: Option<String>,
    pub selected_root: Option<RootId>,
    /// Inline-rename target (the branch currently being renamed) and its
    /// draft name, edited on the row itself. The draft is the one deliberate
    /// exception to "events out": egui's text input needs mutable access
    /// while rendering, so only commit/cancel are emitted as events.
    pub renaming: Option<String>,
    pub rename_draft: String,
    /// Which branch row's right-click context menu is open; carries the
    /// owning root so one row's menu never resolves against another repo's
    /// snapshot. The anchor lives in egui memory, not here, because this
    /// crate has no egui dependency.
    pub context_menu: Option<(RootId, String)>,
    /// Current vertical scroll offset of the list (issue 06).
    pub scroll: f32,
    /// The pre-filter scroll offset, while a filter is active.
    pub scroll_saved: Option<f32>,
    /// Scroll the named branch into view on the next render (create/undo).
    pub scroll_to: Option<String>,
    /// The filter text as of the previous frame, so the tree can detect a
    /// filter transition and save/restore scroll itself.
    pub previous_filter: String,
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
    /// The shelf-apply window (`DialogState`'s `unshelve_` group). Of the reference's
    /// five fields, two map onto this app's model and are here: **a destination picked
    /// from a list** and **whether the shelf survives the apply**. The other three —
    /// entering a name for a destination to be created, `Set active`, `Track context`,
    /// and the `Comment` that describes such a destination — are changelist verbs, and
    /// this application has no changelist model: it stages into buckets. They ship as
    /// recorded absences, never as controls that tick and do nothing.
    ///
    /// It is a dialog rather than a pane mode because the reference's own dialog is
    /// where the remove-after-apply choice lives, and the pane's two verbs carry that
    /// choice as their names instead.
    Unshelve,
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
    /// The reference's fifth integration verb's window: the file-level
    /// comparison `Show Diff with Working Tree` opens, listing every path that
    /// differs between one branch and the working tree on the three-rung change
    /// grammar, with `Get from Branch` on the rungs it can be run on. Its payload
    /// is the [`DialogState`] group prefixed `get_from_branch_`.
    ///
    /// A dialog rather than a mode of the Commit window: `UNVERIFIED` in the
    /// reference's own doc, and this surface already hosts its own tree — a second
    /// rung list inside it would be read as staging state.
    GetFromBranch,
    /// `Apply patch`: the window that writes a patch into
    /// ONE root's working tree, with `Show Diff` as a preflight rather than a
    /// confirmation and a three-way outcome that says which state the tree is in
    /// afterwards. Its payload is the [`DialogState`] group prefixed
    /// `apply_patch_`.
    ///
    /// The reference reaches this from `Git | Patch | Apply patch…`; TurboGit has
    /// no main menu bar, so the command palette is its menu route — the same
    /// substitution the palette already makes for the three lifecycle verbs.
    ApplyPatch,
    /// `Create Patch from Local Changes`' first step: the
    /// comment whose words become the patch file's name. Its payload is the
    /// [`DialogState`] group prefixed `create_patch_`, which the settings step that
    /// follows shares.
    ///
    /// A step rather than a field in the settings dialog because the reference puts it
    /// before it: the comment names the file, and the file's location is one of the
    /// settings the dialog then asks about.
    CreatePatchComment,
    /// `Patch File Settings`: the four fields in the reference's order — location (or
    /// `To clipboard`), base path, `Reverse patch`, encoding — and the counting repair
    /// pair `Change Directory Paths` that goes with them. One dialog for all three
    /// create sources, which is why it is reached from the Commit window, from a Log
    /// commit and from a file revision.
    PatchFileSettings,
    /// `Import Patches…` — the Shelf view taking patch FILES as a source. Its payload is
    /// the [`DialogState`] group prefixed `import_patch_`, and it imports into the root
    /// whose pane opened it.
    ImportPatches,
    /// The commits a branch delete discarded, reached from that delete's
    /// disclosure link — before the ref moves and after. Its payload is
    /// [`DialogState::unmerged_view`].
    BranchNotFullyMerged,
    /// Manage remotes (issue 33, screen 13 "Manage remotes…"): list, add,
    /// edit URLs, rename, remove, and set branch upstreams — plus the
    /// multi-root apply across a selection.
    ManageRemotes,
    /// The preflight a targeted history verb shows before it runs: the commit
    /// it acts on, the set replayed on top of it, the cautions, the estimate and
    /// the way back (ticket 10). One dialog for both verbs, parameterised by
    /// [`HistoryVerb`]; its payload is [`DialogState::rewrite_preflight`].
    RewritePreflight,
    /// The commit-message editor Reword commit opens first (ticket 11): the
    /// commit's own message, editable as one string. Its draft is
    /// [`DialogState::reword`] and the live text [`DialogState::reword_message`].
    Reword,
}

/// A push rejection held for the rejection notification: the
/// classified outcome, the request a retry replays verbatim, and the two
/// recovery choices the surface exposes.
///
/// The request is kept rather than rebuilt because the reply to "was rejected"
/// is "do the same push again, after this update" — a rebuilt target could
/// silently differ from the push that failed.
///
/// It is NOT a [`Dialog`] variant: the Push dialog closes the moment it
/// dispatches, so a notification that lived inside it would be gone before the
/// rejection it describes ever arrives.
#[derive(Clone, Debug)]
pub struct PushRejection {
    pub outcome: turbogit_services::sync_service::PushOutcome,
    pub request: crate::operation::PushRequest,
    /// The roots the failed push acted on — what "affected only" means when
    /// the recovery picks its roots.
    pub affected_roots: Vec<RootId>,
    /// "Update all repositories": widen the recovery from the affected roots
    /// to every registered root. Off by default (the conservative reading of
    /// an undocumented reference default).
    pub update_all: bool,
    /// The dialog's "Remember the update method choice and silently update in
    /// the future" checkbox. Applied to settings when an action is taken with
    /// it on, so the write-back is a choice the user made, not a side effect
    /// of the rejection.
    pub remember: bool,
}

/// The one sentence the empty-message bound is stated in.
///
/// Both surfaces that show it read it from here — the editor's field, where the
/// eye is, and this layer's refusal, where the rule is enforced — so a bound
/// cannot end up worded two ways and drift. It states the RULE and not the
/// mechanism: the mechanism (git quietly keeps the old message, so the rewrite
/// would change nothing) is worth knowing once, and it is not what a developer
/// needs at the moment a confirm does nothing.
pub const EMPTY_COMMIT_MESSAGE: &str = "A commit cannot have an empty message";

/// Which targeted history verb the log's commit menu is running.
///
/// The two verbs share one rewrite path, one preflight and one reselection
/// rule, so the verb is a PARAMETER of those rather than a fork of them — the
/// same shape `menu_host` takes a `MenuId` and `commit_menu` takes
/// `CommitFacts`. It is an enum rather than an action plus an optional message
/// so the state that cannot exist is unrepresentable: a drop has no message to
/// carry, and a reword always has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryVerb {
    /// Remove the commit from the current branch, replaying what is above it.
    Drop,
    /// Rewrite the commit's message, its content untouched. Carries the whole
    /// message, subject and body, exactly as the editor holds it.
    Reword { message: String },
}

impl HistoryVerb {
    /// The plan row this verb sets.
    pub fn action(&self) -> turbogit_domain::model::RebaseAction {
        match self {
            Self::Drop => turbogit_domain::model::RebaseAction::Drop,
            Self::Reword { .. } => turbogit_domain::model::RebaseAction::Reword,
        }
    }

    /// The replacement message, for the verbs that have one. It rides the plan
    /// row — git's rebase todo has no slot for it (ADR-0025) — and never the
    /// activity feed, which names the commit and leaves the message to git.
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Drop => None,
            Self::Reword { message } => Some(message),
        }
    }

    /// The subject the acted-on row will carry once this verb has run: the new
    /// one for a reword, and nothing for a drop, which is the fact
    /// [`RewrittenAnchor`](turbogit_services::reselection::RewrittenAnchor)
    /// needs to follow the selection to the commit's new hash.
    pub fn reworded_subject(&self) -> Option<String> {
        match self {
            Self::Drop => None,
            Self::Reword { message } => {
                Some(turbogit_services::reselection::subject_of_message(message).to_owned())
            }
        }
    }
}

/// A rewrite that has been dispatched and whose result the log has not caught up
/// with yet.
///
/// Armed when the operation is dispatched — the plan it will run is the plan the
/// developer was shown — and applied when the post-rewrite log arrives, because
/// that is the first moment the commit that took the acted-on commit's place
/// exists. Cleared when the operation reports failure: a rewrite that failed
/// moved nothing, so there is no new history to follow and the commit the
/// developer was looking at is still there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingReselect {
    /// The repository being rewritten.
    pub root: RootId,
    /// The plan that was run, which is what names the commit that takes the
    /// acted-on commit's place.
    pub plan: Vec<turbogit_domain::model::RebasePlanEntry>,
    /// The commit the rewrite acted on.
    pub anchor: CommitId,
    /// Which rewrite it was — the one thing a drop and a reword disagree about.
    pub fate: turbogit_services::reselection::RewrittenAnchor,
}

/// One targeted history verb's preflight, built when the menu item is picked so
/// nothing the developer is shown is a guess: the repository the commit belongs
/// to, the commit, the plan the verb will run, and the cautions computed from
/// that plan. Plain data — the dialog paints it and the confirm dispatches from
/// it, and neither touches git.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewritePreflight {
    /// The repository the commit belongs to, carried from the menu's own target
    /// rather than re-read from the selection.
    pub root: RootId,
    /// The commit the verb acts on.
    pub commit: CommitId,
    /// Which verb this preflight is briefing, and — for a reword — the message it
    /// will write. Carried rather than re-derived so the dialog, the reselect and
    /// the dispatch all read the same one.
    pub verb: HistoryVerb,
    /// The plan, in the shape the operation will rebuild, with the verb's own
    /// action and message already on the acted-on row.
    pub plan: Vec<turbogit_domain::model::RebasePlanEntry>,
    /// What is likely to go wrong replaying it.
    pub cautions: Vec<turbogit_services::history_editor::RebaseCaution>,
}

/// The commit Reword commit's editor is open on, and the message it started from.
///
/// The message itself is NOT here: it is live text the developer edits every
/// keystroke, so it lives in [`DialogState::reword_message`] the way the plan
/// editor's todo lives in `rebase_todo`, and this is the target it edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewordDraft {
    /// The repository the commit belongs to.
    pub root: RootId,
    /// The commit being reworded.
    pub commit: CommitId,
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
    /// Remove one shelf's record from the Shelf pane (surface 07). Both halves
    /// of the name travel with it — the root, because a shelf belongs to one
    /// repository and never to the project, and the name, because that is the
    /// identity the shelf store is keyed by.
    ///
    /// **CONFIRMED HERE, THOUGH THE REFERENCE MAY NOT BE.** Whether the
    /// reference's shelf `Delete` and stash `Drop` carry a confirmation was never
    /// observed at runtime (its docs describe only the menu path, and `Ask for
    /// confirmation to drop commits` covers rebase drops). The documented
    /// fallback is TurboGit's existing confirming stance, recorded as a
    /// divergence: if the sitting later shows the reference confirms, this becomes
    /// a match and only the ADR note changes.
    DeleteShelf {
        root: RootId,
        name: String,
    },
    /// Remove one stash entry (`git stash drop`) from the Stash pane — the same
    /// confirming stance as [`Self::DeleteShelf`]. The index is the entry's
    /// position in `git stash list`, which is what the pane's row names.
    DeleteStash {
        root: RootId,
        index: usize,
    },
    /// Checkout with a dirty working tree (issue 07): the plain-language
    /// bring-along / set-aside / cancel dialog, with conflict implications
    /// stated before acting.
    CheckoutDirty {
        root: RootId,
        target: String,
        kind: turbogit_domain::model::BranchKind,
        /// `target` is a rev to stand ON rather than a branch to move to, so the
        /// two accept paths detach HEAD. Set by the log's commit menu; a branch
        /// row leaves it `false` and behaves exactly as it did.
        detach: bool,
    },
    /// The target branch is already checked out in another worktree (issue
    /// 07): refused up front, naming the worktree.
    CheckoutInWorktree {
        branch: String,
        worktree: std::path::PathBuf,
    },
    /// `Get from Branch`: one working-tree file about to be replaced by the whole
    /// contents it has on another branch.
    ///
    /// **This confirmation is a deliberate departure, not a match.** The reference
    /// documents the menu item and its result with no confirm step, which reads as
    /// an omission rather than a design: the verb destroys the local file's
    /// contents, and `PendingConfirm`
    /// is where this app puts a whole-file destructive act. Carries its own `root`
    /// rather than reading `selected_root`, because the window is per root and the
    /// focused root is free to have moved between the click and the OK.
    GetFileFromBranch {
        root: RootId,
        source: String,
        path: PathBuf,
    },
}

/// The commits a branch delete discards: the ones on the branch that are not on
/// the reference it was never merged into.
///
/// **Captured before the ref moves, and carried as a snapshot.** After the
/// delete the branch is gone, and re-deriving the list would need a revision
/// that no longer exists; the reference's own viewer shows a *list*, so the
/// list is what has to survive. `merged_target` is named alongside it because
/// "not fully merged" is meaningless without saying *into what*.
#[derive(Clone, Debug)]
pub struct UnmergedCommits {
    pub root: RootId,
    /// The branch whose delete this was asked for.
    pub branch: String,
    /// What `branch` was not fully merged into, named — the current branch,
    /// which is the reference the delete's own count is taken against.
    pub merged_target: String,
    /// The commits, newest-first, as the engine answered them.
    pub commits: Vec<UnmergedCommit>,
    /// The branch tip at capture time, so the viewer's Restore can recreate the
    /// ref without a second read of a branch that may already be gone.
    pub tip_sha: Option<String>,
}

impl UnmergedCommits {
    /// The disclosure link's label: the **count**, in its own plural. Both
    /// places the link appears — the pre-flight confirmation and the
    /// notification after the ref is gone — spell it here, so a number cannot
    /// disagree with the number the confirmation promised.
    pub fn link_label(&self) -> String {
        match self.commits.len() {
            1 => "View 1 unmerged commit".to_owned(),
            n => format!("View {n} unmerged commits"),
        }
    }

    /// The viewer's header sentence: the branch, **and what it was not merged
    /// into** — the reference's own two-parameter shape, because "not fully
    /// merged" without a target is not a fact anyone can act on.
    pub fn header(&self) -> String {
        format!(
            "The branch '{}' was not fully merged to {}. Below is the list of unmerged commits.",
            self.branch, self.merged_target
        )
    }
}

/// One commit a delete discards: its identity plus the subject read while the
/// branch still existed. The subject is carried, not looked up, because every
/// listing the app caches is reachable from a ref the delete just moved — the
/// discarded commits are exactly the ones that are not in it.
#[derive(Clone, Debug)]
pub struct UnmergedCommit {
    pub id: CommitId,
    /// `None` when the engine could not name the commit; the viewer then shows
    /// the hash alone rather than inventing a line.
    pub subject: Option<String>,
}

/// A local branch deletion awaiting completion, with its tip captured so the
/// undo affordance can restore it (issue 12).
#[derive(Clone, Debug)]
pub struct BranchDeletePending {
    pub root: RootId,
    pub name: String,
    pub tip_sha: String,
    /// The upstream it tracked, if any — what the post-delete offer to remove
    /// the remote branch needs, and what no longer exists once the branch is gone.
    pub upstream: Option<Upstream>,
    /// What the delete discards, for the disclosure that outlives the ref.
    pub unmerged: Option<UnmergedCommits>,
}

/// A deleted branch restorable for a short window (issue 12).
#[derive(Clone, Debug)]
pub struct BranchDeleteUndo {
    pub root: RootId,
    pub name: String,
    pub tip_sha: String,
    pub created_at: std::time::Instant,
    /// The deleted branch tracked this, so the notification can offer to remove
    /// the remote branch the local delete left behind. `None` for a branch that
    /// tracked nothing — the offer is a fact about the delete, not chrome.
    pub upstream: Option<Upstream>,
    /// The commits the delete discarded, with the reference they were not
    /// merged into. The link to look at them is the only way they are reachable.
    pub unmerged: Option<UnmergedCommits>,
}

/// Which targeted history verb a rewrite undo restores. Coverage is
/// deliberately asymmetric — drop and reword are undoable, squash is not —
/// matching the reference's own undo lifecycle, which names no squash undo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewriteUndoVerb {
    Drop,
    Reword,
}

/// A dispatched rewrite whose settlement should arm the undo offer.
/// The settlement event carries only the [`OpKind`], which cannot name the
/// commit or the verb, so the dispatch captures them here — the same
/// capture-before-dispatch shape as [`BranchDeletePending`].
#[derive(Clone, Debug)]
pub struct RewriteUndoPending {
    pub root: RootId,
    pub verb: RewriteUndoVerb,
    pub commit: String,
}

/// A completed rewrite that can still be undone from its backup ref.
///
/// `tip_sha` is HEAD as the rewrite finished — the fact the repository-changed
/// guard compares against on every later check. The guard runs at undo time,
/// never at rewrite time: any completed operation touching the root re-runs
/// it, and a moved HEAD flips `withheld`, which the banner answers by painting
/// the impossible reason and NO control at all (withheld, not greyed — the
/// same withheld-vs-inert choice `PendingConfirm` makes).
#[derive(Clone, Debug)]
pub struct RewriteUndo {
    pub root: RootId,
    pub verb: RewriteUndoVerb,
    pub commit: String,
    pub tip_sha: String,
    pub created_at: std::time::Instant,
    pub withheld: bool,
}

/// One repository's side of a diverged multi-root branch run.
///
/// The unit of this data is a ROOT, never the run: an offer that said only
/// "the operation partially failed" could not be acted on, because rolling
/// back means naming which repository is about to move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DivergentRoot {
    pub root: RootId,
    /// The repository's display name — the identifier the offer paints.
    pub name: String,
    /// The branch this root stands on now: the run's target for a root it
    /// changed, the branch the run found it on for a root it failed in.
    pub branch: String,
    /// Why the run did not land, for a failed root; `None` for a changed one,
    /// whose whole story is the branch it moved to.
    pub reason: Option<String>,
}

/// A branch run that changed some roots and failed in others, held for the
/// offer to roll the changed ones back.
///
/// The reference's policy is stated once and is worth quoting, because it is
/// the reason this exists at all: an operation that fails in at least one
/// repository risks branches diverging, so the app suggests rolling the
/// operation back in the repositories where it succeeded. Divergence is not a
/// cosmetic difference between two surfaces — the roots genuinely disagree about
/// which branch they are on, and nothing but a rollback (or hand repair) puts
/// that back.
///
/// This is deliberately the OPPOSITE of the multi-root sync policy, where an
/// unupdatable root is skipped and the run carries on. A skip is safe because
/// the skipped root is unchanged by a disclosed decision; a rollback is
/// necessary because the succeeded roots are changed by an accident. The two
/// must not be unified, so this arms only on a `Failed` row and never on a
/// `Skipped` one.
///
/// Session-transient like [`UiState::branches_undo`]: the offer is answered,
/// dismissed, or replaced by the next run's outcome. What outlives it is the
/// activity-feed entry armed beside it, and the [`crate::bulk_history`] record
/// the rollback reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchDivergence {
    /// The history record the rollback reads — one undo mechanism, shared with
    /// the bulk-history row's `Rollback` control.
    pub record_id: u64,
    /// What the run was (`"Create & checkout branch"`), so the offer names the
    /// operation that diverged instead of hard-coding one verb's wording.
    pub operation: String,
    /// The branch the run tried to put every root on.
    pub branch: String,
    /// The roots the run CHANGED — and the only roots a rollback may act on.
    pub changed: Vec<DivergentRoot>,
    /// The roots the run FAILED in, each with git's reason.
    pub failed: Vec<DivergentRoot>,
}

/// What a rollback attempt did: the divergence offer's accept path, and the
/// bulk-history row's, which share one mechanism.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RollbackOutcome {
    /// `roots` repositories were put back and the record now says so.
    Applied { branch: String, roots: usize },
    /// Some roots could not be put back. The record stays un-rolled-back, so the
    /// attempt is retryable rather than half-confirmed.
    Incomplete {
        branch: String,
        restored: usize,
        unrecovered: usize,
    },
    /// Nothing was attempted: no such record, an operation with no undo, or a
    /// record already rolled back.
    Refused,
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
/// it is also the target [`crate::keyed_read`] reads its lines by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameTarget {
    pub root: RootId,
    pub path: PathBuf,
    pub rev: String,
}

/// Which pane of the diff viewer holds the caret (`Diff.FocusOppositePane`).
///
/// The panes are the two sides of the comparison, not two widgets: `Old` is the
/// `Before` band, `New` the `After` one, and in unified rendering the same pair
/// picks which of a changed line's two rows the caret sits on. The distinction
/// pays for itself at `F4`, which jumps to the line the caret is on — numbered
/// against the side it is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum DiffPane {
    /// The `Before` side: old-file numbering.
    Old,
    /// The `After` side: new-file numbering.
    #[default]
    New,
}

impl DiffPane {
    /// The other pane — the whole of `Diff.FocusOppositePane`, which is what
    /// `⌃⇧⇥` runs.
    pub fn opposite(self) -> Self {
        match self {
            DiffPane::Old => DiffPane::New,
            DiffPane::New => DiffPane::Old,
        }
    }
}

/// The in-flight character-range drag (issue 19): one press inside one changed
/// line of the previewed diff, with `anchor`/`cursor` as char offsets into that
/// line's body (`turbogit_services::partial::HunkSelection::Chars` scheme). It
/// is a draft, not a selection: the band it paints is the live drag, and the
/// range is staged the moment the pointer is released. Nothing armed survives a
/// release, so there is nothing for a key to clear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharDrag {
    pub change: ChangeRef,
    /// The pane's hunk index of the row the press landed on. Identity only: the
    /// draft is matched against the row the pointer is still over.
    pub hunk: usize,
    /// The row's changed-line ordinal within that hunk. Identity only, for the
    /// same reason — the operation is addressed by [`CharDrag::new_no`].
    pub ord: usize,
    /// The row's worktree gutter, which is the number
    /// [`crate::staged_lines`] translates into the appliable section.
    pub new_no: usize,
    /// Char offset the press landed on.
    pub anchor: usize,
    /// Char offset the pointer is at now.
    pub cursor: usize,
}

impl CharDrag {
    /// The dragged range, normalized around the anchor so dragging either way
    /// widens it. A range that never moved is empty, and an empty range is a
    /// line click, not a character stage.
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }
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
///
/// A toast paints `Dismiss` and nothing else: its `Retry` button was deleted
/// as unreachable rather than repaired, because `dispatch` never had a way to
/// hand a toast a replay handle. Re-adding the button is a feature with its own
/// ticket (ADR-0020). There is no replay handle to hand it: retrying means
/// constructing the operation again and dispatching that, which is what the
/// Shelve confirmation already does.
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

/// A patch file a create route wrote, held for the `When patch is created: Ask`
/// follow-up.
///
/// Both halves travel with it: the **path** is what an apply or an import reads, and
/// the **root** is the tree the patch was generated from — the only root an imported
/// shelf row may be parked under, because applying a shelf in the wrong root moves
/// work in the wrong tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchCreated {
    pub path: std::path::PathBuf,
    pub root: RootId,
}

/// UI-only ephemeral state (never persisted).
#[derive(Default)]
pub struct UiState {
    /// Persistent inputs for the modal dialogs.
    pub dlg: DialogState,
    // Commit tab
    pub commit_message: String,
    pub amend: bool,
    /// The Local Changes files the developer is looking at, keyed by
    /// `Root::canonical_key`. A row-body click rebuilds it through the reducer in
    /// `turbogit_ui::ui::changes_selection`: a plain click leaves one file in
    /// it, a Command/Ctrl click moves one in or out, a Shift+Click replaces it
    /// with a painted range. Discard Changes, Shelve and Create Patch read it,
    /// and the commit path does not. No unmerged path is ever in it.
    pub selected: HashSet<PathBuf>,
    /// The shift-range origin for that list: the last row clicked without a
    /// range gesture, including a row that cannot be selected. A range click
    /// deliberately leaves it alone, so repeated shift-clicks reshape one span
    /// instead of walking the selection down the list. A stale anchor (the file
    /// vanished, its group collapsed, the filter hid it) falls back to a plain
    /// click rather than selecting nothing.
    pub selection_anchor: Option<PathBuf>,
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
    // Branches tab (issues 03+): search buffer and the focus/scroll intents.
    pub branches_filter: String,
    /// The Branches surface's four persisted toggles, behind its settings gear.
    /// Durable, not ephemeral: unlike [`BranchesGroups`], which says whether a
    /// group is open *right now*, this says whether a group and a grouping level
    /// exist for this project at all.
    pub branches_view: crate::persistence::BranchesViewOptions,
    /// The gear's toggle list is open. Session-only chrome state, the same kind
    /// as [`Self::branches_scope_picker_open`]: a menu that was open when the app
    /// quit does not come back open.
    pub branches_settings_open: bool,
    /// Expand/collapse of Local / Remote / Tags.
    // The tree-remembered branch view state, one independent block per tree
    // surface (branch-tree-view extraction, plan D2): the Branches tool
    // window and the Log window's branches pane. See [`TreeState`].
    pub branches_tree: TreeState,
    pub log_tree: TreeState,
    /// Focus the search input on the next render (tab open, issue 06/15).
    pub branches_focus_search: bool,
    /// Delete-with-care state (issue 12): the human-terms consequence shown in
    /// the delete confirmation, the pending-delete tip captured for undo, and
    /// the short-window undo affordance itself.
    pub branches_delete_consequence: Option<String>,
    /// The commits the pending delete would discard, captured with the
    /// consequence above so the confirmation can link to the list **before**
    /// anything is deleted — the reference only tells you afterwards, and
    /// TurboGit asks first. The link opens the same viewer the post-delete
    /// notification's link opens, so one body states the fact once.
    pub branches_delete_unmerged: Option<UnmergedCommits>,
    pub branches_delete_pending: Option<BranchDeletePending>,
    pub branches_undo: Option<BranchDeleteUndo>,
    /// Rewrite dispatch awaiting settlement to arm the post-hoc undo.
    pub rewrite_undo_pending: Option<RewriteUndoPending>,
    /// The armed post-hoc undo offer for the last targeted rewrite.
    /// Session-transient like [`Self::branches_undo`]: the banner expires, the
    /// guard withholds, a newer rewrite replaces, and an undo click consumes.
    pub rewrite_undo: Option<RewriteUndo>,
    /// The rollback offer for the last branch run that landed in some roots and
    /// failed in others. Replaced or cleared by the next run's
    /// outcome, and answered by [`AppState::rollback_divergent_roots`] or
    /// [`AppState::keep_divergent_branches`].
    pub branch_divergence: Option<BranchDivergence>,
    /// Fetch reporting (issue 13/14): a callback per in-flight fetch holding
    /// that root's remote-branch snapshot before the fetch, so every repo's
    /// fetch reports what *it* changed ("N new remote branches" vs "nothing
    /// changed") and when it happened (disclosed somewhere visible).
    pub branches_fetch_before: Vec<(RootId, Vec<String>)>,
    pub branches_last_fetch: Option<chrono::DateTime<chrono::Utc>>,
    /// Multi-repo scope (issue 14): a repo filter narrowing the aggregated
    /// list (None = all repos).
    pub branches_repo_filter: Option<RootId>,
    /// Whether the toolbar's repo-scope picker is open (issue 14).
    pub branches_scope_picker_open: bool,
    // Git Log four-pane workspace (issue #12)
    /// Live search text for the branches pane.
    pub log_branch_filter: String,
    /// Roots filter: `None` shows every root's commits, `Some(id)` narrows.
    pub log_root_filter: Option<RootId>,
    /// File selected in the changed-files pane.
    pub log_selected_file: Option<PathBuf>,
    /// The changed-files pane's own filter (logs-panels redesign issue 02): a
    /// case-insensitive substring over the selected commit's paths. Distinct
    /// from [`UiState::file_filter`], which belongs to the Commit tab. It
    /// survives a selection change on purpose — a filter that matches nothing
    /// renders as "no match", not as a commit with no files.
    pub log_file_filter: String,
    /// Active path scope in Git Log (issue #19): `Some(path)` narrows the
    /// graph to only the commits touching that path (set from the
    /// changed-files pane's "Show history for file…" menu item).
    pub log_path_scope: Option<PathBuf>,
    /// Active ref scope in Git Log (branch-tree extraction, plan D9):
    /// `Some((root, ref))` narrows the graph to only that ref's history
    /// (set from the branches pane's row activation). Cleared when the
    /// selected repository changes, so the graph never goes silently empty.
    pub log_ref_scope: Option<(RootId, String)>,
    pub selected_commit: Option<CommitId>,
    /// The commit whose row the log list should reveal (issue 08). Set by the
    /// three surfaces that select a commit from outside the list — the details
    /// pane's parent link, the blame view's row click, and the commit context
    /// menu — and consumed by the commit list, which resolves it to a row index
    /// and aims its scroll area at that row. One-shot by construction: the list
    /// takes it every frame, so a request is answered once and a selection that
    /// is already on screen never yanks the list.
    pub log_scroll_to: Option<CommitId>,
    /// A jump whose target commit is loaded but not in the displayed window,
    /// because the active filters are what hides it. Held across frames rather
    /// than answered in the one frame that asked: the escape has to stay
    /// reachable until the developer takes one, and the two escapes are the
    /// point — silently widening or clearing a filter set to reveal one commit
    /// destroys the context the user was working in.
    ///
    /// Cleared when the commit comes back into the window, when it is no longer
    /// loaded at all, or when either escape is taken.
    pub log_hidden_jump: Option<(RootId, CommitId)>,
    /// A dispatched rewrite waiting for the log to prove it landed, so the
    /// selection can follow the history instead of dangling (ticket 10).
    pub pending_reselect: Option<PendingReselect>,
    /// Which commit row's context menu is open: `(root, commit)`. The menu's own
    /// target, not the selection — so a rewrite or a selection change underneath
    /// an open menu cannot make the pane and the menu describe different
    /// commits (ADR-0024). Right-clicking a row sets this AND selects the row.
    pub log_commit_menu: Option<(RootId, CommitId)>,
    /// Which changed-file row's context menu is open: `(root, commit, path)`.
    /// Carries its own target for the same reason the commit menu does — the row
    /// it opened on, not whatever is selected now.
    pub log_file_menu: Option<(RootId, CommitId, std::path::PathBuf)>,
    pub diff: Option<DiffTarget>,
    /// Open blame view (issue 18): `Some` renders the blame surface for the
    /// target file at the target revision; `None` shows the commit graph.
    pub blame: Option<BlameTarget>,
    /// Blame lines fetched for the open [`Self::blame`] target, keyed like
    /// `diff_cache` to drop stale results, and read only through
    /// [`crate::keyed_read`]. Dropped wholesale on root refreshes so the view
    /// refetches after operations.
    pub blame_cache: Option<(String, std::sync::Arc<[BlameLine]>)>,
    pub dialog: Option<Dialog>,
    /// The push dispatched and not yet settled. Settlement takes it to replay
    /// on a retry and to name the target a rejection reports;
    /// taking it is also what says "no push is in flight".
    pub push_in_flight: Option<crate::operation::PushRequest>,
    /// The root whose landed commit still owes the Push dialog an open. Armed
    /// before [`AppState::dispatch_commit`] and spent, opened, or dropped only by
    /// the completion of the commit it names — an unrelated fetch, stage, or
    /// second commit can neither answer it nor lose it. Session-only, like
    /// [`Self::dialog`] and [`Self::push_in_flight`]: a commit that settled
    /// before a restart does not owe the next session a dialog.
    pub commit_push_dialog_pending: Option<RootId>,
    /// The last push rejection awaiting the user's reply, if any. Session-only
    /// like every dialog: a rejection is about the push that just failed.
    pub push_rejection: Option<PushRejection>,
    pub vcs_popup: bool,
    // Workspace picker (issue #34). Session-only; never persisted.
    /// Whether the floating workspace picker is showing.
    pub workspace_picker_open: bool,
    /// Where the dropdown hangs from, in screen points: the trigger's
    /// bottom-left. `Some` comes from the sidebar's workspace header click;
    /// `None` — the palette route, which has no trigger rect — means the
    /// picker's own fallback position. Plain `f32` pairs — this crate is
    /// egui-free.
    pub workspace_picker_anchor: Option<(f32, f32)>,
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
    /// Onboarding environment check (ticket 19): the resolved git executable's
    /// version, or the reason it could not be resolved, keyed by the
    /// executable path it was computed for. The Welcome clone card is the
    /// consumer — a missing `git` is the most common first-run failure, and
    /// the answer decides whether the clone control is offered at all. The
    /// Settings page keeps its own draft-scoped badge because it re-checks
    /// after every keystroke in the executable field.
    pub git_env_check: Option<(String, Result<String, String>)>,
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
    /// What the `Apply to all N remaining` control fills the still-unresolved
    /// conflicts with. Session-scoped and sticky across
    /// files on purpose — `open_resolver` deliberately does not reset it, so
    /// a user bulk-resolving several files the same way keeps their scope.
    pub conflict_resolver_scope: BulkApplyScope,
    /// Pending `Overwrite Changes` confirm: the
    /// `(conflict ordinal, choice)` a take-ours/take-theirs click wants to
    /// apply while the file already carries merged changes. The dangerous
    /// affirmative (`Discard and Accept`) applies it; the safe option clears
    /// this and does nothing.
    pub conflict_overwrite_pending: Option<(usize, u8)>,
    /// What the last `Apply to all` click resolved: the
    /// reference reports a bulk apply's NEGATIVE result instead of leaving it
    /// silent, so the resolver names both outcomes under the control. Cleared
    /// when another file opens, so a stale verdict never follows a new file.
    pub conflict_bulk_report: Option<ConflictBulkReport>,
    pub shelves: Vec<Shelf>,
    /// The `(root, name)` an in-flight `Unshelve and Remove` wants dropped from
    /// [`Self::shelves`] once the git side lands. Arm at dispatch, take on the
    /// matching [`crate::operation::OpKind::Unshelve`] completion, clear on its
    /// failure — the same one-shot record the branch-delete undo uses, for the
    /// one verb pair on this surface whose two halves live on different sides of
    /// the boundary.
    pub shelf_removal_pending: Option<(RootId, String)>,
    /// The `When patch is created: Ask` follow-up: a patch file a create route has
    /// written, and the root it was generated **from**, waiting for the developer to
    /// say what to do with it.
    ///
    /// This is app state rather than a dialog field because the settings dialog that
    /// produced it has already closed — the ask outlives the window that answered it,
    /// and a value living in `dlg` would be reset by the next visit to the create
    /// group while still painted. Nothing about the patch is decided here until one of
    /// [`crate::create_patch::import_created_to_shelf`],
    /// [`crate::create_patch::apply_created`] or
    /// [`crate::create_patch::dismiss_created`] runs.
    pub patch_created: Option<PatchCreated>,
    // commit-tab inline diff preview (Epic C3). The change list aggregates every
    // registered root, so the previewed row carries the root it belongs to: a
    // repo-relative path alone cannot say which repository to read it from.
    pub preview_change: Option<ChangeRef>,
    // diff viewer (Epic E: async cache + layout)
    /// The diff read's value: one comparison's key and the patch text with its
    /// display model, settled by [`crate::keyed_read`], which is the only code
    /// that reads or compares it (ADR-0021).
    pub diff_cache: Option<(String, std::sync::Arc<crate::diff_model::DiffValue>)>,
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
    /// The viewer's two view policies: which whitespace differences count, and
    /// how finely a change is highlighted. They are part of what the diff read is
    /// keyed on, so a change re-folds the display model once instead of per frame.
    pub diff_policies: DiffPolicies,
    /// Which pane of the diff viewer holds the caret (`Diff.FocusOppositePane`,
    /// `⌃⇧⇥`). The New side starts with it because that is the side a reader is
    /// there to look at.
    pub diff_pane: DiffPane,
    /// The diff viewer's settings menu (`⌃⇧D`, on the file row like the toggle it
    /// replaces) is open.
    pub diff_options_open: bool,
    /// The line `F4` jumped to: the caret the diff viewer held, preserved across
    /// the jump into the blame surface. Cleared by every other route into blame.
    pub blame_caret_line: Option<usize>,
    /// Armed edge press for F7/Shift+F7 cross-file navigation (spec R7): the
    /// direction and instant of the last edge nudge. A same-direction repeat
    /// inside [`crate::diff_data::EDGE_WINDOW`] crosses to the adjacent
    /// changed file; anything else re-arms.
    pub hunk_nav_armed_edge: Option<(crate::diff_data::Dir, std::time::Instant)>,
    /// The change a granular stage/unstage was last dispatched for (spec R2
    /// story 9). Consumed by the op-completion handler to decide whether the file
    /// just left the changelist.
    pub pending_granular: Option<ChangeRef>,
    /// Changes whose last granular op completed with nothing unstaged left
    /// (spec R2 story 9): they stop being listed in the changelist buckets.
    /// Ephemeral by design — entries drop as soon as the path regains
    /// unstaged changes or disappears from status entirely.
    pub granularly_completed: HashSet<ChangeRef>,
    /// The character-range drag in flight on the previewed diff, or `None`
    /// whenever no pointer is down on a row. A release either stages its
    /// range or leaves nothing at all, so this never outlives the gesture.
    pub char_drag: Option<CharDrag>,
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
    // repo names, paths, and branch names. Collapsed folder / repo-with-
    // children state, keyed by **relative path** so same-named folders at
    // different depths collapse independently (sidebar-project-tree 03);
    // persisted with the workspace.
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
    /// [`Self::save_durable_ui`] / the launch path.
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
    /// workspace through [`Self::save_durable_ui`] / the launch path.
    pub pinned_views: Vec<crate::pinned_views::PinnedView>,
    /// Recent bulk operations (issue 12, screen 04): one record per
    /// completed bulk/cascade run, persisted with the workspace through
    /// [`Self::save_durable_ui`] / the launch path.
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
    /// The Git engine. This interface is the seam (ADR-0001), and it is
    /// reachable only from this crate: the Shell's two git interfaces are
    /// [`AppState::dispatch`] and a value the app has already read or cached
    /// (ADR-0020). Tests that need a different engine use
    /// [`AppState::with_executor`], never this field.
    pub(crate) executor: Arc<dyn GitExecutor>,
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
    /// The log pane's derived display window (log-view-scaling 04): the whole
    /// loaded window as an owned, indexable row sequence with its lane index,
    /// held beside the caches it was derived from. `None` until the first frame
    /// asks for it; from then on it is only replaced when a batch lands, a scope
    /// settles, the caches are invalidated, or one of the four UI inputs moves
    /// (see [`crate::log_display::needs_rebuild`]). Private: the log pane goes
    /// through [`Self::sync_log_display`], which owns the staleness rule.
    log_display: Option<Arc<crate::log_display::LogDisplay>>,
    /// Override for the OS config dir hosting the global recents file
    /// (ADR-0005). `None` → `recents::default_config_dir()`. Tests inject a
    /// temp dir so the real user configuration is never touched.
    pub recents_config_dir: Option<PathBuf>,
    /// Native folder-picker seam for the Welcome Open/Initialize flows.
    /// Production wires `rfd`; tests inject closures returning fixed paths.
    pub dir_picker: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    /// The patch-path seam: choose where a commit's patch file is written.
    /// Installed by the composition root beside [`AppState::dir_picker`] for the
    /// same reason — a native save dialog has no headless equivalent, and this is
    /// the highest point at which the developer's choice can be substituted.
    /// `None` means "declined": nothing is written and nothing is confirmed.
    pub patch_writer: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    /// The read twin of [`AppState::patch_writer`]: choose the patch file to
    /// apply. Installed by the composition root for the same reason — a native
    /// open dialog has no headless equivalent — and left `None` in tests, where
    /// the path field takes the path directly.
    ///
    /// Kept separate from `dir_picker` rather than reused: that seam picks a
    /// FOLDER and returns a directory a repository lives in, and a patch path
    /// picked through it would be a directory the apply then refuses to read.
    pub patch_reader: Option<Box<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
    /// Resolved git version (e.g. `2.47.1`) for the status bar's version
    /// line, computed once at launch (issue #34) so the bar never spawns
    /// `git` per frame. `"unknown"` when the binary cannot be resolved.
    pub git_version: String,
    /// Where git work runs. Private by design: the mode is chosen by which
    /// constructor built the state, never by a caller.
    pump: Pump,
    /// own worktree-list admission and freshness policy (ticket 02).
    worktree: crate::worktree_lifecycle::WorktreeLifecycle,
    /// The keyed read's own state (ADR-0021): which key each read has a fetch
    /// in flight for, and which key one failed for. The values stay in
    /// [`Self::ui`]; only the rule lives here.
    pub(crate) reads: crate::keyed_read::Reads,
    /// A keyed-read dispatch held back from the pump, waiting for a test to
    /// answer it. `cfg(test)` by design: production never holds a read, and the
    /// seam that makes lateness observable stays internal to the crate
    /// (ADR-0021).
    #[cfg(test)]
    hold_reads: bool,
    #[cfg(test)]
    held_reads: Vec<HeldRead>,
    /// own admission for the in-flight fetches, keyed by fetch kind (ticket
    /// 02): one fetch per key, released on that fetch's settlement event.
    pub(crate) fetch_admission: crate::fetch_admission::FetchAdmission,
    /// Last dispatch instant of the background incoming check (issue #27);
    /// `None` while disabled or never polled.
    pub(crate) incoming_poll_last: Option<std::time::Instant>,
    /// The per-root rollback records a tree is still waiting on. The apply flow
    /// owns the rules over them: [`crate::apply_patch::ApplyRecovery`].
    pub(crate) apply_recovery: crate::apply_patch::ApplyRecovery,
    /// The create flow's two memos, keyed by origin/reverse and by
    /// origin/reverse/base. The `create_patch` module owns the rules over them;
    /// the values they describe stay in [`Self::ui`].
    pub(crate) create_patch_memos: crate::create_patch::Memos,
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

    /// **`ui` is a parameter on purpose and must not be defaulted here.** The
    /// two constructors disagree deliberately: `launch_in` passes a visible
    /// shell and `for_roots` a bare [`UiState::default()`], because that hidden
    /// chrome is what the headless suites assert against. Unifying it would hide
    /// the toolbar in the real app and show it in every headless suite — and the
    /// suites would stay green.
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
        // The shelf store is this application's own record, and the Shelf pane
        // lists it: a shelf written in an earlier session has to be there to
        // unshelve. A store that will not parse reports itself instead of
        // painting an empty pane over the user's shelved work.
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

    /// Launch flow (ADR-0004): with a project directory the shell opens
    /// straight away; without one the Welcome screen is the landing surface
    /// and no CWD scan happens. `recents_config_dir` overrides the OS config
    /// dir hosting the global recents file (tests inject a temp dir).
    pub fn launch_in(project_dir: Option<PathBuf>, recents_config_dir: Option<PathBuf>) -> Self {
        // Shell regions start visible; View-menu toggles flip these. This is
        // `for_roots`'s deliberate opposite — see `with_flat_state`.
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

        // Global recents (ADR-0005) load before anything is open so the
        // Welcome screen can list them.
        if let Some(cfg) = state.recents_config() {
            state.ui.recent_projects = crate::recents::load(&cfg).projects;
        }

        match project_dir {
            Some(dir) => {
                state.project_dir = dir;
                state.rescan();

                // Restore the durable UI set (Epic J4): the declaration's
                // restore direction, reading the same file.
                let ui = crate::persistence::load_ui_state(&state.project_dir);
                ui.restore_into(&mut state.ui);
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
    /// production uses ([`turbogit_services::multi_root::register_all`]). Git
    /// work then runs inline on the calling thread and posts its results into
    /// the same channel production uses: a test dispatches, calls
    /// [`AppState::drain_events`], and asserts what the state looks like after
    /// settlement — no thread, no deadline, and no second settlement path.
    /// Panics if any root cannot be snapshotted — a broken test fixture should
    /// fail at construction.
    pub fn for_roots(project_dir: &Path, roots: &[PathBuf]) -> Self {
        // Bare UiState defaults (toolbar/status bar hidden), matching what
        // the headless suites assert against — NOT launch_in's visible shell.
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

    /// Which operation, if any, holds `root` — the port's answer, reached through
    /// the engine this app owns.
    ///
    /// The rewrite gate, the status bar's in-flight item and the conflict
    /// resolver all ask it, and none of them may read git's marker files for
    /// themselves: the day an engine answers differently, three surfaces
    /// disagreeing about who holds one repository is the bug.
    ///
    /// Cheap enough to ask per root per frame for that reason — a `stat` inside
    /// the repository's git directory, never a git process — which is the
    /// property the adapters are held to by `tests/holding_operation.rs`.
    pub fn holding_operation(&self, root: &Path) -> Option<OwnedOperation> {
        self.executor.holding_operation(root)
    }

    /// The effective config dir for the global recents file (ADR-0005).
    fn recents_config(&self) -> Option<PathBuf> {
        self.recents_config_dir
            .clone()
            .or_else(crate::recents::default_config_dir)
    }

    /// Write the **durable** set of UI state to `.turbogit/ui.ron`.
    ///
    /// Durable means the ten rows declared by
    /// [`crate::persistence::UiPersist`] — the active tab, the repositories,
    /// and the seven preferences behind their own surfaces — and nothing else:
    /// the other 128 [`UiState`] fields are this session's and are not
    /// written here.
    ///
    /// **When to call it: a change to a durable field is written by the same
    /// call that changes it.** A rule, not an enforcement — nothing stops a
    /// mutation from forgetting this call, and three routes in the tree
    /// (the palette's tab actions, the draft's Clear and Template, the
    /// Branches gear's `&mut` hand-off) do forget it today. That is the rule
    /// the fourteen existing call sites already follow, which is why the
    /// timing of each of them is load-bearing and not incidental.
    pub fn save_durable_ui(&self) {
        let ui = crate::persistence::UiPersist::capture(&self.ui);
        let _ = crate::persistence::save_ui_state(&self.project_dir, &ui);
    }
    /// dispatch a fresh asynchronous status scan for every registered root.
    pub fn rescan(&mut self) {
        // Ticket 02: include lists for roots that discovery no longer returns.
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
            // Ahead/behind of the current branch vs its upstream (Epic D3).
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

    /// The next batch request for `root`'s window, and the mode its answer
    /// folds in under (P5). The window is derived from the cache, never
    /// stored — the scoped listings plan their batches by the same rule (see
    /// [`crate::root_caches::log_batch_plan`]), and only the filter differs.
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

    /// Fetch the next batch of a root's commit log on a worker thread.
    pub fn fetch_log(&mut self, root: RootId) {
        // One fetch per root in flight (log-open perf, D2): the tool-window
        // body asks every frame while the cache is cold; a second fetch for
        // the same root while one is pending is a no-op.
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

    /// Fold one fetched batch into `root`'s window (log paging).
    ///
    /// The rules live on the window itself ([`RootCaches::settle_log_batch`]),
    /// shared with the three scoped listings: a batch that does not lead with
    /// the anchor row it was requested against means the listing moved under
    /// the request, so nothing is held and the window is asked again from the
    /// front rather than growing a torn list (P3). Either way the batch's
    /// length against the rows it asked for is what says whether history
    /// continues (P4).
    fn settle_log_batch(&mut self, root: &RootId, mode: LogBatchMode, batch: Vec<Commit>) {
        if matches!(
            self.caches.settle_log_batch(root, mode, batch),
            LogBatchSettle::Torn
        ) {
            // The tear dropped the window, so the next plan is batch 0 again.
            self.fetch_log(root.clone());
        }
    }

    /// Fetch (and cache) ref decorations for a root on a worker thread
    /// (log-open perf, D1): the Log view's data-ensure step kicks this
    /// instead of running `ref_decorations` synchronously on the render
    /// thread. One fetch per root in flight — a second fetch for the same
    /// root while one is pending is a no-op; the guard releases when the
    /// `RefsLoaded` event drains, on Ok and Err alike.
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

    /// Load one commit's per-file line counts off the render thread
    /// (logs-panels redesign issue 02). A cache hit and an in-flight request
    /// both return without dispatching, so the Log pane may ask every frame;
    /// the guard releases when the `FileStatsLoaded` event drains.
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

    /// Fetch one more batch for every root whose window says history continues
    /// past it — the manual "Load more" button and the log pane's automatic
    /// trigger. A root with nothing cached is NOT loaded here (the Log pane's
    /// data-ensure step owns the first batch), and a root at the end of its
    /// history issues no fetch at all.
    pub fn load_more_log(&mut self) {
        for root in self.multi.roots.clone() {
            if self.caches.log_has_more(&root.id) {
                self.fetch_log(root.id);
            }
        }
    }

    /// The log pane's display window, rebuilt only when the loaded window or one
    /// of the UI-side inputs has moved (log-view-scaling 04). The log pane calls
    /// this once per frame instead of re-deriving; the answer is a handle to the
    /// held value, and cloning it is a refcount bump — never a copy, never a
    /// borrow of the caches, so the frame stays free to mutate `state` while it
    /// paints.
    ///
    /// The returned handle is the SAME allocation the frame before it got for as
    /// long as nothing changed: a rebuild allocates a new one. That identity is
    /// this method's contract, not an accident of the allocation, and it is what
    /// lets the "an idle frame recomputes nothing" rule be stated and checked
    /// without a counter in the production path.
    pub fn sync_log_display(&mut self) -> Arc<crate::log_display::LogDisplay> {
        let revision = self.caches.revision();
        let inputs = crate::log_display::LogInputs::of(self);
        if crate::log_display::needs_rebuild(self.log_display.as_deref(), revision, &inputs) {
            let (loaded, hits) = crate::log_display::sources(&self.caches, &inputs);
            self.log_display = Some(Arc::new(crate::log_display::derive(
                revision, &inputs, loaded, hits,
            )));
        }
        // Just built, or already held: the window is never absent here, so the
        // pane reads one value and never has to special-case "no window yet".
        self.log_display
            .clone()
            .expect("a rebuilt window is held; the branch above stored it")
    }

    /// Fetch (and cache) the linked-worktree list for a root on a worker
    /// thread (issue 14 Worktrees tab). One fetch per root in flight —
    /// `fetch_worktrees` is a no-op while an earlier fetch for the same
    /// root is still pending.
    pub fn fetch_worktrees(&mut self, root: RootId) {
        self.worktree
            .fetch(root, self.executor.clone(), self.tx.clone());
    }

    /// Drop one root's cached worktree list and bump its epoch (ticket 02):
    /// the sanctioned invalidation behind the worktree-mutating flows. The
    /// bump makes any list fetch that started before the mutation stale, so
    /// its late settlement cannot resurrect an outdated list over the fresh
    /// refetch that follows.
    fn invalidate_worktree_cache(&mut self, root: &RootId) {
        if self.worktree.on_mutation(root) {
            self.caches.invalidate_worktrees(root);
        }
    }

    /// Dispatch per-worktree dirty probes for the focused root's listed
    /// worktrees whose flag is still unknown (ticket 03). Visibility-gated:
    /// this is only ever called while the Worktrees tool window is open, and
    /// it never touches the badge or eager-fill paths. The lifecycle module
    /// owns admission (dedup per row), independent worker dispatch, and
    /// settlement; this entry point only gathers the still-unknown rows.
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

    /// Fetch (and cache) the submodule list for a root on a worker thread
    /// (issue 14 Submodules tab). One fetch per root in flight, mirroring
    /// [`Self::fetch_worktrees`].
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

    /// Fetch (and cache) the stash listing for a root (surface 07's Stash
    /// pane), mirroring [`Self::fetch_submodules`]: one read per root in
    /// flight, answered off the render thread on the same seam an operation
    /// dispatches on. A root whose listing is already cached is not re-read —
    /// an operation that could have moved the stash stack drops the entry with
    /// the rest of that root's caches, and the pane asks again after that.
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

    /// The changed files of `(root, commit)`, filled through the Git engine the
    /// app owns on miss (CONTEXT.md "Root caches"). The Shell asks for a value
    /// and gets the cached list; it never hands the cache an engine.
    ///
    /// Borrows the cache: do not touch app state while holding the slice.
    pub fn ensure_files(&mut self, root: &RootId, commit: &CommitId) -> &[Change] {
        self.caches
            .ensure_files(self.executor.as_ref(), root, commit)
    }

    /// One scoped commit listing of `root` — a file's history, a ref's history
    /// or a pickaxe search — filled through the engine the app owns on miss.
    /// The three listings were three near-identical signatures; they are one
    /// call taking a [`LogScope`]. A scope that has never been read is read
    /// ONE BATCH of (log-view-scaling 01), and one that already holds rows
    /// answers from the cache, so the per-frame ask never grows the window by
    /// itself; [`Self::load_more_scoped_log`] is what grows it.
    ///
    /// Borrows the cache: do not touch app state while holding the slice.
    pub fn ensure_log(&mut self, root: &RootId, scope: LogScope) -> &[Commit] {
        self.caches.ensure_log(self.executor.as_ref(), root, &scope)
    }

    /// Answer a log state's `Refresh` recovery: forget the one read that failed
    /// and ask it again, now.
    ///
    /// This is the whole of what the recovery control does, and it is a method
    /// rather than a call site because *which* ask re-reads a given key differs
    /// per read — a listing goes on the pump, a scope's cold window is a
    /// compute-on-miss, and a commit's line counts are guarded by their own
    /// cache entry. Forgetting first is what makes each of those asks real:
    /// every one of them is a no-op against a cached answer, and the cached
    /// answer after a failure is the empty one.
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

    /// The listing state's `Refresh`: re-ask exactly the reads that failed.
    pub fn refresh_log_listing(&mut self) {
        for read in crate::log_states::failed_listing_reads(self) {
            self.retry_log_read(&read);
        }
    }

    /// The details pane's `Refresh`.
    pub fn refresh_commit_details(&mut self) {
        if let Some(read) = crate::log_states::failed_details_read(self) {
            self.retry_log_read(&read);
        }
    }

    /// The changed-files pane's `Refresh`.
    pub fn refresh_commit_changes(&mut self) {
        if let Some(read) = crate::log_states::failed_changes_read(self) {
            self.retry_log_read(&read);
        }
    }

    /// The `Reset filters` recovery, shared by every state that offers it: the
    /// filter set goes, and with it the cached answers and failures the filters
    /// produced — a search that failed for one term must not be remembered as
    /// having failed for the next one that happens to match the same text.
    ///
    /// The windows are dropped rather than kept, because the pane's own ask
    /// fills only what is missing: a held empty result would otherwise be the
    /// answer forever.
    pub fn reset_log_filters(&mut self) {
        let query = self.ui.log_filter.trim().to_owned();
        let mut roots = crate::log_display::visible_roots(self);
        // A ref scope names its own root, which the roots filter may hide, so
        // the scope's read is forgotten under the root that ran it.
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

    /// The batch a scoped read of `root` asks for right now: the scope's own
    /// filter, plus the position the window derives from what it holds
    /// (log-view-scaling 01, 10). The mode comes back with it, so the batch that
    /// arrives off-thread is folded in under the boundary it was measured
    /// against — and a search's page, which holds its batch as a tail, is cut to
    /// the batch itself when it settles.
    fn scoped_log_batch_plan(&self, root: &RootId, scope: &LogScope) -> (LogOpts, LogBatchMode) {
        let (rows, skip, mode) = scoped_batch_plan(scope, self.caches.scoped_log(root, scope));
        (scope.batch_opts(rows, skip), mode)
    }

    /// Fetch the next batch of one scoped listing — the "Load more" of a file's
    /// history, a ref's history, or a pickaxe search — on a worker thread
    /// (log-view-scaling 02), mirroring [`Self::fetch_log`]: the frame that
    /// asks dispatches and draws, and the batch lands through
    /// [`AppEvent::LogBatchLoaded`] when the next drain runs.
    ///
    /// One read per (root, scope) in flight, so a second ask for the same
    /// scope before its batch lands is a no-op, and a scope that reports the
    /// end of its history is not asked at all. The guard releases on Ok and
    /// Err alike, so a failed batch leaves the scope able to page again.
    pub fn load_more_scoped_log(&mut self, root: &RootId, scope: &LogScope) {
        if !self.caches.scoped_log_can_grow(root, scope) {
            return;
        }
        // One fetch per (root, scope) in flight — the scope is half the key,
        // so one scope's slow read cannot hold another scope's batch.
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

    /// `root`'s hunk-span statistics, filled through the engine the app owns on
    /// miss (issue 20). Three whole-root diffs on a cold entry, so this is the
    /// heaviest of the cache reads and the reason the Commit window asks for
    /// only the selected root.
    pub fn ensure_hunk_stats(&mut self, root: &RootId) {
        let exec = self.executor.clone();
        self.caches.ensure_hunk_stats(exec.as_ref(), root);
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
    /// Ahead/behind and the selected root's log refetch go out on the dispatch
    /// seam, so the headless harness answers them the same way production does —
    /// as events the next drain settles.
    pub fn refresh(&mut self, affected: Affected) {
        self.caches.invalidate(&affected);
        if matches!(affected, Affected::All) {
            self.worktree.clear();
        }
        // The diff viewer caches raw patch text outside the root caches;
        // a completed op may have changed exactly what it shows (spec R2
        // story 8), so drop it and let the viewer reload asynchronously.
        self.ui.diff_cache = None;
        // The read's own rule state goes with the values it governs: an
        // in-flight fetch's answer is now about a worktree that no longer
        // exists, and a failure must not keep reporting itself over a
        // refetch that would succeed (ADR-0021).
        self.reads.invalidate();
        // The blame view follows the same wholesale rule (issue 18): a
        // completed op may have changed exactly what the open target blames,
        // so drop it and let the view's next read refetch.
        self.ui.blame_cache = None;
        // Non-text pane bytes follow the same wholesale rule (spec R8):
        // dropped with root refreshes, never poked per field. Bumping the
        // generation tells the UI layer to drop its GPU textures too — the
        // plain-data cache never holds an egui type itself (DDD split
        // issue 04).
        self.ui.pane_bytes.clear();
        self.ui.pane_generation += 1;
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
        // The selection outlives every operation, so it accumulates paths that
        // are no longer rows. Pruned here rather than beside the exclusion
        // prune above because "no longer a row" is only true of the snapshots
        // `register_all` just replaced.
        granular::prune_selection_on_refresh(self);

        // Ahead/behind of each affected root's current branch vs upstream, on
        // the same seam everywhere: under `Spawned` it answers on a later frame,
        // and the headless harness now waits for that frame rather than
        // pretending the answer arrived sooner.
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

        // Refetch the selected root's log iff it is inside the scope.
        if let Some(sel) = self.selected_root.clone() {
            let in_scope = match &affected {
                Affected::All => true,
                Affected::Root(id) => *id == sel,
            };
            // Paging lives in `fetch_log`'s batch plan, so the harness and the
            // shell ask for exactly the same window.
            if in_scope {
                self.fetch_log(sel);
            }
        }
    }

    /// Dispatch an [`Operation`]: run its work on a worker and post its
    /// completion back.
    ///
    /// The operation is consumed to run it, so what rides back is its identity,
    /// label and scope rather than the operation itself (ADR-0020). A
    /// worktree-mutating operation posts its extra invalidation *after* the
    /// completion, which is the ordering ADR-0019 requires: invalidating at
    /// dispatch alone loses the race against the eager per-frame fill and
    /// caches the pre-mutation list, which a root refresh no longer drops.
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

    /// Dispatch a push, remembering the request so a rejection can be
    /// classified and replayed.
    ///
    /// The request is recorded BEFORE the dispatch on purpose: under
    /// [`Pump::Inline`] the completion event is queued synchronously inside
    /// this call, so a rejection settled after the dispatch would still find
    /// the request here — but a push that never recorded one could never
    /// recover at all.
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

    /// Dispatch `root`'s commit, and arm the Push dialog's open for its
    /// settlement when `then_push` says the chain was asked for.
    ///
    /// **The pending is armed before the dispatch** because under the inline pump
    /// the completion is queued synchronously inside [`Self::dispatch`], so
    /// arming after it is a lost race — the same rule
    /// [`Self::unshelve`] states for the shelf record.
    ///
    /// The arm is an **assignment, not a conditional set**: a plain commit clears
    /// an owed open instead of leaving a consent dialog stranded on a later one.
    ///
    /// The dialog is NOT opened here. Under the spawned pump the commit is still
    /// on its worker thread while this frame paints, and the dialog builds its
    /// outgoing snapshot once, on first paint, so opening it here paints a commit
    /// that did land as "No outgoing commits". Only
    /// [`crate::settle`]'s `open_push_dialog_owed` may open it, over the
    /// snapshots the completion has already refreshed.
    pub fn dispatch_commit(&mut self, root: &RootId, message: &str, amend: bool, then_push: bool) {
        self.ui.commit_push_dialog_pending = then_push.then(|| root.clone());
        self.dispatch(Operation::Commit {
            root: root.clone(),
            message: message.to_string(),
            amend,
        });
    }

    /// Dispatch a user-initiated pull of one root, optionally checking
    /// `checkout` out first.
    ///
    /// Every pull verb in the shell arrives here — the palette's `Pull`, the
    /// branch menu's `Pull` and `Checkout and pull`, the branch row's hover
    /// `Pull` — because a pull whose update method is `Rebase` rewrites the
    /// branch it lands on, and a verb that handed the flag straight to the port
    /// rewrote a protected branch with no refusal and no rewrite backup to undo
    /// to. The guards themselves are
    /// [`turbogit_services::sync_service::pull_guarded`]'s; this is the seam that
    /// makes forgetting them impossible from a surface.
    ///
    /// `checkout` is the branch the pull will integrate into, named by the row
    /// the developer clicked, so the protected-branch refusal can be answered
    /// before even that checkout runs.
    ///
    /// One root per dispatch, always: a pull is per-repository work. The rail's
    /// `Pull selection` is the fleet form and a merge-only pull, so it keeps its
    /// own shape.
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
                        // Refuse a protected branch before the checkout, not after
                        // it: a pull that will not be allowed to rebase must not
                        // leave the developer on the branch the verb was going to
                        // move them to. `pull_guarded` checks again against HEAD —
                        // here the two are the same name.
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

    /// Run a cache fill on the same seam an [`Operation`] dispatches on.
    ///
    /// A fill is not an Operation: it has no identity, no label, no invalidation
    /// scope and no settlement arm, and inventing a silent variant for each of
    /// the ten of them would be the empty-variant trap this design rejects. It
    /// is still git work, so it must not run on the render thread under
    /// `Spawned` and must not need a thread under `Inline`. Both modes post into
    /// the same channel and settle through the same `drain_events`.
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

    /// Run a [`Keyed`](crate::keyed_read::Keyed) read's fetch on the same seam
    /// [`Self::pump_read`] uses — unless the reads are being held.
    ///
    /// Nothing reachable through a real adapter can answer *late*: the CLI
    /// blocks until git returns and the fake answers inline, and lateness is the
    /// only window in which staleness lives. Holding queues the dispatch
    /// instead of running it, so a test can answer two reads out of order and
    /// see what the admission rules do with a stale arrival. A held read is
    /// still admitted — its slot is taken, and the frame is told to wait —
    /// which is the whole point.
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

    /// Hold every keyed-read dispatch instead of running it. Internal to the
    /// crate, never named by a caller, and no constructor signature moves — the
    /// reason ADR-0020 declined executor injection.
    #[cfg(test)]
    pub(crate) fn hold_reads(&mut self) {
        self.hold_reads = true;
    }

    /// How many keyed-read dispatches are waiting to be answered, oldest first.
    #[cfg(test)]
    pub(crate) fn held_reads(&self) -> usize {
        self.held_reads.len()
    }

    /// Answer the held dispatch at `index` (0 = oldest) by running it — it
    /// posts into the same channel a worker posts to, and settles nothing here,
    /// so scripted and real answers drain identically.
    ///
    /// # Panics
    ///
    /// If `index` names no held dispatch.
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

    /// Surface a feedback message with its own auto-dismiss window.
    ///
    /// The clock is cleared here rather than left running: a message that
    /// replaces another is a new notice, and inheriting the old one's elapsed
    /// time can dismiss it on the very frame it appeared — so a fast follower
    /// to a completed operation is never seen at all.
    pub(crate) fn show_toast(&mut self, toast: Toast) {
        self.ui.toast = Some(toast);
        self.ui.toast_shown_at = None;
    }

    /// The notification a create-patch route raises from the frame that owns the
    /// clipboard.
    ///
    /// Public because the copy itself can only happen in a frame holding an
    /// `egui::Context`, and the toast saying it happened belongs with the copy rather
    /// than with the state that asked for it: a success reported before `copy_text` has
    /// run would describe an act nobody performed.
    pub fn show_patch_toast(&mut self, toast: Toast) {
        self.show_toast(toast);
    }

    /// Snapshot each root's remote branches before a fetch, so completion can
    /// report what *that* fetch brought in instead of every branch there is.
    /// Owned by dispatch because only the operation knows which roots it
    /// covers — a multi-root fetch needs a baseline per root.
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
            post_bulk_completed(&tx, label, results);
        });
    }

    /// Save a remote named from inside the Push panel's `Define remote` entry.
    ///
    /// The same port op the remotes manager runs, but its outcome comes back to
    /// the panel rather than to the bulk report: the reference's failure path is
    /// inline — `Could not add remote: {0}` in the panel, dialog still open —
    /// and the success path has to repaint the remote name where the link was.
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

    /// Execute a confirmed destructive action (Epic C8). The UI gates these
    /// behind a confirmation dialog and only calls this on explicit OK.
    ///
    /// The verb's effect is [`confirm`](crate::confirm)'s: this stays the entry
    /// because two headless suites call it directly, and it is the same match a
    /// confirm button runs under [`confirm::apply`](crate::confirm::apply).
    pub fn run_confirmed(&mut self, c: PendingConfirm) {
        crate::confirm::run(self, c);
    }

    // -------------------------------------------------- shelf verbs (07) ----

    /// The Shelf pane's rows: **this root's** shelves, newest first. A shelf is
    /// not a project-wide bucket — its content was parked in one repository's
    /// stash list, and an unshelve that ran in another one would apply work to
    /// the wrong tree.
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

    /// Point one shelf's content back at its own tree.
    /// Open the Unshelve dialog: the one route that asks *which* shelf to apply and
    /// *whether the shelf survives it*, where the pane's two verbs answer the second
    /// question by their own labels.
    ///
    /// **The root is stamped at open**, and the newest shelf of that root is
    /// preselected, because a shelf names work in exactly one repository and a
    /// picker that followed the focus could apply a same-named shelf of the wrong
    /// tree. Both defaults are re-armed on every open: `Remove successfully applied
    /// files from the shelf` is **off**, so an open dialog is never one tick away
    /// from a destruction the developer did not ask for.
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

    ///
    /// Two mechanisms behind one verb, chosen by the record rather than by a guess: a
    /// shelved change's content is **git's** stash entry, found again by the shelf's
    /// name, while an imported patch's content is the patch text the shelf store parked,
    /// and putting it back means applying it. The second is the deferral the reference
    /// names — the patch went to the shelf **before** being applied — so an unshelve that
    /// looked for a stash entry it never made would report "nothing shelved as X" over
    /// bytes it has in hand.
    ///
    /// `remove` keeps the meaning both halves share, and it is the captured default's
    /// opposite: `Unshelve` keeps the row, `Unshelve and Remove` destroys it — for a
    /// patch row that means the payload file goes with it, which is the only reclaim an
    /// imported shelf has.
    ///
    /// **The pending is armed before the dispatch** because under the inline pump the
    /// completion is queued synchronously inside `dispatch`, and settlement drops the row
    /// on its [`crate::operation::OpKind::Unshelve`] success arm only if the record is
    /// already waiting there.
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

    /// The parsed patch an imported row parked, or `None` when the row is stash-backed.
    ///
    /// A payload that cannot be read answers `None`, and the verb then looks for a stash
    /// entry that cannot exist and fails with git's own words. That is deliberate: the
    /// delete path is the only verb allowed to drop a record whose content has gone,
    /// because there destroying the reference is the point — an unshelve that quietly
    /// succeeded by applying nothing would be worse.
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

    /// Record a new shelf — the app-side half of the shelve dialog's verb, and the
    /// mirror image of [`Self::remove_shelf`]: the candidate list is written first
    /// and kept only when the store accepts it.
    ///
    /// The refusal is both **stored and returned**, because the caller has a second
    /// half to gate: a row the store refused paints for the rest of this session and
    /// vanishes at the next launch, while the dispatch that follows parks the work in
    /// git's stash list — where an unshelve can find it again only by the name this
    /// record carries.
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

    /// `Import to shelf` — park a patch as a shelf row **without applying it**.
    ///
    /// The reference's deferral ("if you want to move the patch to a temporary storage
    /// (shelf) before applying it") reached from three places: the apply window's footer,
    /// the Shelf view's `Import Patches…`, and the `When patch is created: Ask` follow-up.
    /// All three run this one method, so the deferral cannot mean three different things.
    ///
    /// **The patch bytes are the payload, not a git stash entry.** A shelve can stash
    /// because the work is already in the working tree; an imported patch is work no
    /// repository holds, and stashing it would mean applying it first — the exact act the
    /// developer deferred. So the bytes go to the shelf store's payload file and the row
    /// records which one, and [`Self::unshelve`] reads them back through the apply engine.
    ///
    /// The order is **payload, then index, then memory**, and the index write is the
    /// commit point: `add_shelf` refuses to rewrite a store it cannot read, and a refusal
    /// here deletes the payload it just made rather than leaving bytes no row lists. A
    /// row never reaches the pane except by way of a store that accepted it, which is the
    /// discipline both shelf verbs already hold and the one thing a third way to create a
    /// record must not reinvent.
    ///
    /// `root` is the tree the patch was made against and the only one an unshelve may
    /// later write — the row is per root, and the pane lists it under that root alone.
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
            // The record refused, so the bytes it would have named go back the way they
            // came. Best-effort, and stated either way: a payload that survives this is a
            // file no row lists, and the only reclaim for it is Settings until someone
            // deletes it by hand.
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

    /// `Import to shelf` from the apply window: the patch **as the window holds it**,
    /// including whatever base-path and leading-directory repairs the developer has since
    /// typed. Parking what is on screen is the only defensible reading — parking the file
    /// as git wrote it would defer something other than what they were about to apply.
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

    /// The name a deferral gets when the developer did not give it one: the patch's own
    /// commit subject when git carried one, its file name when it came from disk, and a
    /// plain label otherwise. A shelf row nobody can name is a row nobody can find again.
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

    /// `Import Patches…` from the Shelf view: read the chosen file and park it in the
    /// pane's own root. The read is a plain file read of a path the developer typed or
    /// chose — the same seam rule the apply window states for its own path field.
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

    /// Choose the patch file to import, through the same read seam the apply window
    /// uses: a native open dialog has no headless equivalent, and the path field is
    /// where the developer's choice has to land for the verb to read it.
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

    /// Drop one shelf's record — the app-side half of `Unshelve and Remove` and
    /// of a confirmed shelf delete. The store is rewritten with what is left, so
    /// the pane and the file never disagree about a shelf that has gone.
    ///
    /// **The list moves only once the rewrite lands** (write the candidate, commit
    /// on `Ok`): a refusal that had already dropped the row leaves the pane painting
    /// nothing while the record sits on disk, and the shelf the user deleted returns
    /// at the next launch to disagree with the error they already have on screen.
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
        // Report a refused rewrite instead of discarding it: the store will
        // not truncate itself over rows it has not read, and a silent `let _`
        // there would leave the pane and the file disagreeing with nobody
        // the wiser.
        if let Err(e) = turbogit_services::shelve_stash::save_shelves(&self.project_dir, &kept) {
            self.last_error = Some(e.to_string());
            return;
        }
        // The payload goes **after** the index accepted the shorter list, never before:
        // deleting the bytes first would leave the row still painted and pointing at a
        // file that is gone, which is the stranded-work shape reversed rather than solved.
        let orphaned: Vec<String> = self
            .ui
            .shelves
            .iter()
            .filter(|s| &s.root == root && s.name == name)
            .filter_map(|s| s.source.file_name().map(|f| f.to_string()))
            .collect();
        self.ui.shelves = kept;
        for file in orphaned {
            // A payload that will not delete is a file no row lists — reclaimable by hand
            // and never by an unshelve, so it is reported rather than swallowed.
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

    /// Cherry-pick `commit` onto the branch `target` for the focused root.
    /// The service guards the protected target and a dirty worktree; the original
    /// checkout is restored afterwards.
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

    // -- Cherry-pick across repositories --------------------------------------

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

    /// Open the cross-repo cherry-pick dialog: the source is the
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

    /// Open the new-branch dialog with what the entry point knows.
    ///
    /// ONE place, because six surfaces open this dialog and they mean different
    /// things by it: five of them open it blank and let it default to the
    /// current branch, the branch menu's New branch from opens it at a chosen
    /// branch, and the log's New branch opens it at a commit (ADR-0024). The
    /// fields each of them owns — the name, the base, the checkout decision — are
    /// set here, so an open path cannot forget one (the log's arm did exactly
    /// that once, leaving a stale base behind) and a second copy of this reset
    /// cannot drift.
    pub fn open_new_branch(&mut self, base: NewBranchBase, checkout: bool, name: Option<String>) {
        let dlg = &mut self.ui.dlg;
        dlg.new_branch_name = name.unwrap_or_default();
        dlg.new_branch_base = base;
        dlg.new_branch_base_picker_open = false;
        dlg.new_branch_checkout = checkout;
        self.ui.dialog = Some(Dialog::NewBranch);
    }

    /// Record that a rewrite is in flight, so the selection can follow the
    /// history when the post-rewrite log arrives.
    ///
    /// Armed BEFORE the dispatch, never after: the operation runs on a worker,
    /// and one that finished inside a frame would settle before anything was
    /// waiting for it. The plan handed over is the one the developer was shown,
    /// which is what names the commit that takes the acted-on commit's place.
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

    /// Move the selection onto the commit that took the rewritten commit's
    /// place, once the log says the rewrite has landed. See
    /// [`Self::arm_reselect_after_rewrite`].
    ///
    /// Two conditions, and both are load-bearing:
    ///
    /// * the log must be the one the rewrite produced. A listing that still
    ///   contains the acted-on commit predates the rewrite — a read already in
    ///   flight when the operation finished can deliver exactly that first — so
    ///   the pending reselect waits for the next one instead of selecting a
    ///   commit that is about to change hash again.
    /// * the answer is the SHARED rule ([`turbogit_services::reselection`]), and
    ///   a rule that declines clears the selection rather than guessing: an
    ///   empty pane is honest, a pane pointing at a commit that no longer exists
    ///   is a lie.
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
        // The selected commit changed, so anything hanging off the old one is
        // stale: a changed file that no longer exists, and a menu aimed at it.
        self.ui.log_selected_file = None;
        self.ui.log_commit_menu = None;
    }

    /// Forget a pending reselect for the roots an operation touched. A rewrite
    /// that failed moved nothing, so nothing should follow it.
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

    /// Open the rewrite preflight (ticket 10) for `commit` in `root` running
    /// `verb`: the plan the verb will run, the cautions computed from that plan,
    /// and the dialog. One function for both verbs, because one preflight shows
    /// them and one plan builds them.
    ///
    /// Built here rather than at the call site so the preflight can never show a
    /// plan that is not the one the confirm dispatches, and so the things that
    /// can refuse it — a commit with no first parent, one no longer on the
    /// current branch, and a reword with nothing to say — are answered in one
    /// place.
    ///
    /// The menu states the first two bounds on the item, so reaching either
    /// refusal means the history moved between painting the menu and taking the
    /// item, or a stale surface reached it. The toast therefore repeats what the
    /// service said, verbatim and in full, rather than paraphrasing a refusal the
    /// developer cannot otherwise see. One place formats that message: the
    /// service.
    pub fn open_rewrite_preflight(&mut self, root: &RootId, commit: &str, verb: HistoryVerb) {
        if let HistoryVerb::Reword { message } = &verb
            && message.trim().is_empty()
        {
            // Refused HERE, before a plan is built, because an empty message is
            // not a failure the engine reports: git's reword keeps the ORIGINAL
            // message when the editor leaves it empty, so the rewrite would run
            // in full — every commit above re-created — and change nothing. A
            // silent no-op is the one outcome this verb must not ship, and a
            // disabled button in the editor cannot be the only thing standing
            // between the two. The wording is [`EMPTY_COMMIT_MESSAGE`], the same
            // sentence the field states, so the bound reads identically wherever
            // it is met.
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

    /// Open Reword commit's editor (ticket 11) for `commit` in `root`, seeded
    /// with the commit's own message.
    ///
    /// The message is read from the cached log — the same value the details pane
    /// shows and the same value Copy message would put on the clipboard — so the
    /// developer starts from what the commit actually says, body included, and
    /// corrects it rather than composing something beside it. A commit the cache
    /// does not hold cannot be seeded from anything true, so it is refused
    /// rather than opened empty.
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

    /// Open the tag dialog (issue 31, screen 16) on `target` — a commit the
    /// caller names, or `None` to tag the branch tip.
    ///
    /// ONE place decides what a tag dialog looks like when it opens, because
    /// there are two callers that mean the same thing: the VCS palette's Tag
    /// (screen 16) and the log's New tag menu item (ADR-0024). Everything the
    /// dialog worked out for itself belongs to the visit that closed it and is
    /// cleared here — the type, the two reads it caches (the existing tags it
    /// validates a name against, the commit list its target picker lists), the
    /// signing key it fetched, and whether its picker was left open. The typed
    /// name, message, tagger and push option deliberately survive: those are
    /// what the developer came back for.
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

    /// Toggle one candidate commit in the dialog's checkbox list;
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

    /// Toggle one target repo in the dialog's targets table;
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

    /// Focus a commit in the dialog's patch preview rail (issue 16) and load
    /// its patch synchronously — one engine read, like the preflight's live git
    /// reads. The answer is a patch value; the preview classifies its own lines
    /// rather than re-reading git's text.
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
        let control = bulk_run::RunControl::default();
        self.ui.cherry_run =
            Some(RunSeed::start(&fleet, &control).cherry(source.name(), commits.len(), stop));
        // Snapshot the dispatched roots: the worker owns the paths, `self`
        // stays behind on the UI thread.
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
        if let Some(root) = self.selected_root.clone() {
            self.dispatch(Operation::WorktreeAdd { root, path, branch });
        }
    }

    /// `git submodule update [--init] -- <path>` for the focused root
    /// (issue 14 Submodules tab update action; `init` re-checks out an
    /// uninitialized submodule).
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
        self.ui.preview_change = None;
        self.ui.workspace_picker_open = false;
        // Drop every cache entry: the old project's roots must not leak into
        // the new one (bug fix — only logs/ahead-behind were cleared before).
        self.caches.invalidate_all();
        self.worktree.clear();
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
        self.ui.welcome_visible = false;

        let count = self.multi.roots.len();
        if let Some(cfg) = self.recents_config() {
            let recents = crate::recents::record_workspace(&cfg, dir, count);
            self.ui.recent_projects = recents.projects;
        }
    }

    /// Open a recents row (Welcome row click, workspace picker): a
    /// [`crate::recents::RecentKind::Workspace`] row deep-scans and
    /// re-indexes, a [`crate::recents::RecentKind::Project`] row takes the
    /// bounded [`Self::rescan`] path. Shared by both call sites so there is
    /// exactly one copy of the kind dispatch to keep honest.
    ///
    /// A row whose directory has disappeared surfaces a toast and dispatches
    /// nothing — `open_project` on a missing dir would `rescan` to zero roots
    /// and silently bounce the user to Welcome (see [`Self::show_welcome`]).
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

    /// Create a real repository at `dir` through the engine seam and enter
    /// it (Welcome "Initialize Repository" card, issue #10).
    pub fn initialize_and_enter(&mut self, dir: &Path) {
        // One preflight for the whole onboarding surface (ticket 19): a nested
        // `git init` is almost never intended and is confusing to undo. The
        // check is git's own "am I inside a work tree" — an ancestor marker
        // walk, so a directory inside an existing checkout counts, which is the
        // case a user reaches by picking a subdirectory. `GitExecutor::is_repo`
        // is the wrong question: it is backend-dependent and the git2 adapter
        // does not search upward.
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

    /// Close every open project and return to the Welcome screen
    /// (File → Welcome, ADR-0004).
    pub fn close_all_projects(&mut self) {
        self.multi = MultiRootManager::default();
        self.selected_root = None;
        // These three sites replace every registered root, so a previewed change
        // held here would name a root that no longer exists — and a preview reads
        // its repository out of that id, never out of the focused root.
        self.ui.preview_change = None;
        self.ui.workspace_picker_open = false;
        self.caches.invalidate_all();
        self.worktree.clear();
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

    /// Drop `path` from the global recents store (the Welcome row's remove
    /// control) and refresh the in-memory copy. Returns whether a row was
    /// actually removed.
    ///
    /// **Non-destructive, so nothing to confirm and nothing to undo**: the
    /// directory on disk is untouched and opening the project again re-records
    /// it, which is why a single click can remove a row. The entry's cached
    /// branch indicator goes with it — otherwise every row a user ever removes
    /// stays cached for the rest of the session.
    pub fn remove_recent(&mut self, path: &Path) -> bool {
        // The in-memory list is what the user just clicked on; a path that is
        // not in it has nothing to remove, and the file is left alone rather
        // than rewritten with an identical store.
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

    /// Clone `url` into `dest` from the Welcome screen and enter the result.
    ///
    /// The Welcome flow's success is a different outcome from
    /// [`Self::clone_repo`]'s — this one makes the clone the project
    /// ([`Self::open_project`]) rather than mapping it as a root of the current
    /// one — so it is its own entry point instead of a shared Operation. It is
    /// still synchronous on the calling thread, which is what freezes the
    /// window during a network clone; making it an [`Operation`] needs the two
    /// success meanings settled first.
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

    /// The onboarding environment check (ticket 19): resolve the configured
    /// `git`, caching the outcome against the executable path so a change in
    /// Settings re-runs it and every other frame does not.
    ///
    /// This is the check the Welcome clone card runs before any clone work. It
    /// is worth more in TurboGit than in a client that clones off-thread,
    /// because [`Self::clone_into`] is synchronous on the calling thread — a
    /// missing `git` there is a frozen window rather than a failed call.
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
                AppEvent::LogLoaded {
                    root,
                    commits,
                    mode,
                } => {
                    self.fetch_admission
                        .release(InflightFetch::Log(root.clone()));
                    match commits {
                        Ok(batch) => {
                            // The read answered, so its failure is history. The
                            // record is keyed by the read and its root, never by
                            // the surface that painted it, which is what lets one
                            // pane's failure stop reporting itself the moment the
                            // same read succeeds.
                            self.caches
                                .clear_log_failure(&LogRead::Commits(root.clone()));
                            self.settle_log_batch(&root, mode, batch);
                            // A rewrite's selection follows the history here and
                            // nowhere else: this is the first moment the commit
                            // that took the acted-on one's place exists.
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
                                // The tear left no window, so the next plan is
                                // batch 0 again — read the same listing afresh.
                                self.load_more_scoped_log(&root, &scope);
                            }
                        }
                        // A failed batch surfaces, leaves the held window alone,
                        // and is recorded against the ONE scope that failed: a
                        // search read that died is `Error filtering commits`, not
                        // the listing's problem and not the file's.
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
                        // A failed load stores "no counts" rather than staying
                        // unloaded: the pane asks every frame, so an unloaded
                        // entry would re-dispatch the failing call forever.
                        // Rows render without numbers either way — and the
                        // details pane says `Error loading commit details`,
                        // which is the read that died, not the list beside it.
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
                    // The repository-changed guard re-validates the
                    // armed rewrite undo against the repository as it stands
                    // NOW. Every completed op that touches the offer's root
                    // re-runs it, so a second operation in the same root
                    // withholds the offer before the control is ever painted.
                    self.guard_rewrite_undo(&affected);
                    // Everything from the apply's pre-result read onwards is
                    // settlement's: one completion, one owner.
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
                        // An empty listing stores an answered-empty cache, which
                        // is what keeps the pane on its "Nothing is stashed here"
                        // line instead of reading git every frame.
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
                    // The panel's own half of the outcome: on success the entry
                    // closes and the new name takes the field the link replaced;
                    // on failure the entry stays and `push_remote_add_error`
                    // carries git's reason for `Could not add remote: {0}`.
                    let (message, kind, activity) = match result {
                        Ok(()) => {
                            self.ui.dlg.push_define_remote = false;
                            self.ui.dlg.push_new_remote_url.clear();
                            self.ui.dlg.push_remote = name.clone();
                            self.ui.dlg.push_remote_add_error = None;
                            // The root's `remotes` list is what the panel reads
                            // to decide between the name and the link, so the
                            // entry cannot close on the panel's own say-so — the
                            // snapshot has to be re-read first.
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
        self.save_durable_ui();
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
        bulk_ops::preflight(op, &roots, &self.settings, self.ui.bulk_rebase, &|id| {
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
    ///
    /// The upstream is captured here rather than looked up at paint time: the
    /// dialog's consequence line names the remote branch the rename leaves
    /// behind, and after the rename lands that name is no longer in the list
    /// the lookup would read.
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

    /// Rename `old` to `new` in `id` through the engine seam and refresh
    /// the root. The current-branch rename is legal; git updates HEAD.
    /// `unset_upstream` is the dialog's second field: the rename stays
    /// local-only either way, and this decides whether the branch keeps
    /// tracking the remote it was renamed away from.
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

    /// `Show Diff with Working Tree`: open
    /// the file-level comparison of `source` against the working tree of `id`.
    ///
    /// The same shape as [`AppState::open_compare`] for the same reason: one
    /// synchronous read through the port, captured as plain data, so the window
    /// paints a snapshot rather than asking git every frame. Detached HEAD has no
    /// current side to compare against and opens nothing — the row's own gate says
    /// so before this is reachable.
    pub fn open_get_from_branch(&mut self, id: &RootId, source: &str) {
        let Some(current) = self.multi.by_id(id).and_then(|r| r.current_branch.clone()) else {
            return;
        };
        self.ui.dlg.get_from_branch_root = Some(id.clone());
        self.ui.dlg.get_from_branch_source = source.to_string();
        self.ui.dlg.get_from_branch_current = current;
        // Every open starts in the flow's documented posture: the branch the row
        // named is the base, so grey means "that branch has a file you don't".
        self.ui.dlg.get_from_branch_swapped = false;
        self.reload_get_from_branch_rows();
        self.ui.dialog = Some(Dialog::GetFromBranch);
        self.ui.branches_popup = false;
    }

    /// `Swap Branches`: exchange which side of the comparison is the base.
    ///
    /// Not a rename of the two fields. The differing path set is a symmetric fact,
    /// so the swap is a re-reading of the same list — which side a one-sided path
    /// is missing *from* — and the list's rung order is what makes that visible as
    /// the grey and green rows trading **positions** rather than the same rows
    /// changing tint, which is the inversion this shape exists to avoid.
    pub fn swap_get_from_branch(&mut self) {
        self.ui.dlg.get_from_branch_swapped = !self.ui.dlg.get_from_branch_swapped;
        self.reload_get_from_branch_rows();
    }

    /// Re-read the window's rung list. Called on open, on a swap, and when a take
    /// lands — the three moments on which the answer to "what differs between those
    /// two sides" genuinely changes.
    ///
    /// A no-op once the window's payload is gone, so a late completion of a take
    /// the developer already closed cannot reopen a dialog by itself. The failure
    /// path stores git's words rather than an empty list: "nothing differs" and "the
    /// read died" are different answers, and a window that shows only the first one
    /// makes the second invisible.
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

    /// Open the unmerged-commits viewer for a delete's captured disclosure
    /// — the reference's "Branch Was Not Fully Merged" dialog. The
    /// record is the payload: the branch it names may already be gone, so the
    /// dialog reads nothing new — it presents what the delete took.
    pub fn open_unmerged_commits(&mut self, unmerged: &UnmergedCommits) {
        self.ui.dlg.unmerged_view = Some(unmerged.clone());
        self.ui.dialog = Some(Dialog::BranchNotFullyMerged);
    }

    /// Whether the viewer's `Restore` is a promise it can keep: the branch is
    /// not standing, and its tip was captured. Read from the listed branches,
    /// not from a flag — the same question the list itself answers, so a viewer
    /// opened before the delete cannot offer to restore a branch that is still
    /// there.
    pub fn unmerged_restore_available(&self, unmerged: &UnmergedCommits) -> bool {
        unmerged.tip_sha.is_some()
            && self.multi.by_id(&unmerged.root).is_some_and(|r| {
                !r.branches
                    .iter()
                    .any(|b| b.name == unmerged.branch && b.kind == BranchKind::Local)
            })
    }

    /// Recreate a deleted branch at the tip its disclosure captured. The viewer's `Restore` and the notification's `Undo delete` are one
    /// recovery, so they dispatch identically — and the notification's offer is
    /// spent by this click, because a banner undoing a branch that exists again
    /// is a second route to nothing.
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

    /// Protected-row hover quick-action: open the merge dialog preset to
    /// merge `name` into the current branch.
    pub fn open_merge_into(&mut self, _id: &RootId, name: &str) {
        self.ui.dlg.merge_target = name.to_string();
        self.ui.dlg.merge_source_picker_open = false;
        self.ui.dlg.merge_preview = None;
        self.ui.dialog = Some(Dialog::Merge);
        self.ui.branches_popup = false;
    }

    /// Dispatch a branch checkout through the engine seam (issue 07): local
    /// branches switch directly; remote branches become a new local branch
    /// that tracks them (issue 13). Recent-branch bookkeeping belongs to the
    /// callers. The completion refresh updates the current marker on every
    /// surface together (row, breadcrumb, metadata panel, status bar).
    ///
    /// The upstream is written explicitly rather than left to git: only the
    /// CLI adapter's `checkout -b <name> origin/<name>` sets it by DWIM, and
    /// under the default `Auto` backend `branch_create` is libgit2's, which
    /// never does. [`Self::checkout_branch_set_aside`] tracks for the same
    /// reason, and `tests/branch_checkout.rs` pins the result for both.
    /// Stand the root ON a rev with Detached HEAD — the log's commit menu's
    /// Checkout. The dirty worktree routes through the SAME
    /// [`PendingConfirm::CheckoutDirty`] dialog a branch checkout raises, so the
    /// two checkouts behave identically; only the git call differs.
    pub fn checkout_detached_op(&mut self, root: &RootId, rev: &str) {
        let path = root.clone();
        let rev = rev.to_string();
        self.dispatch(Operation::custom(
            format!("Checkout at {}", short_sha(&rev)),
            Affected::Root(root.clone()),
            move |v| v.checkout_detached(path.as_path(), &rev),
        ));
    }

    /// The dialog's SET ASIDE choice for a detached checkout: stash first, then
    /// stand on the rev.
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

    /// Set the working tree's changes aside (stash) and switch to `name`
    /// (issue 07): the changes stay intact and are restorable afterwards.
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

    /// Rebase the selected branch onto the current branch (issue 09): check
    /// the selected branch out, then replay it onto `current`. The label
    /// states the direction ("Rebase feat onto main"), the cheapest
    /// guardrail against mis-applied direction.
    pub fn rebase_branch_onto_current(&mut self, root: &RootId, branch: &str, current: &str) {
        self.dispatch(Operation::rebase_branch_onto(root, branch, current));
    }

    /// Issue 13: report what a completed fetch changed, in plain terms —
    /// "N new remote branches" or "nothing changed". Never silence, never a
    /// bare success. Also records when the fetch happened for disclosure.
    pub(crate) fn fetch_report(&mut self, affected: &Affected) -> String {
        // Every baseline this fetch owns. A single-root fetch consumes its own
        // snapshot; an all-roots fetch consumes one per root it covered, so a
        // multi-root fetch counts its new branches the same way a single-root
        // one already did rather than reporting nothing.
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
        self.save_durable_ui();
    }

    /// Spawn one cascade pool pass on a worker thread.
    ///
    /// **The send order is the contract, and nothing in the test tree checks
    /// it.** Every [`bulk_run::RunEvent`] goes out as
    /// [`AppEvent::BulkRunProgress`] as the pool emits it; the single
    /// [`AppEvent::BulkCompleted`] goes out only after `run_cascade` has joined
    /// every worker. The monitor's rows, tallies and closing moment follow from
    /// that order.
    ///
    /// `fold` is the caller's, and is what this function does not unify:
    /// [`done_rows_only`] *drops* a root a stop halted (a skipped row);
    /// [`stopped_as_failed`] reports it as an error. Cherry and the bulk grid
    /// take the first, commit-across the second — agreeing is a change to what
    /// the monitor shows, not a cleanup.
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
                let roots = self.bulk_selected_roots();
                // Seed from the plan's own pull policy, not the live modal
                // toggle: the rows must predict what THIS run will execute.
                let pf = bulk_ops::preflight(plan.op, &roots, &self.settings, plan.rebase, &|id| {
                    self.caches.ahead_behind(id).unwrap_or((0, 0))
                });
                // A protected-branch refusal is durable per root: one feed
                // entry naming the branch, outliving the modal and the toast.
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
                                // The row label names the branch the policy
                                // refused, not just the policy.
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
        // The step plan carries no roots: `run_step` is handed the root directly.
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
    ///
    /// **This is the tree's one branch-undo mechanism**, and the divergence
    /// offer deliberately routes through it instead of opening a
    /// second one: the record it reads was scoped by the bulk preflight and
    /// written by the run monitor, so the offer cannot reach a root the run
    /// never covered and cannot bypass either gate. The two callers differ only
    /// in what prompted them — the history row's `Rollback` button, or a run
    /// that left the roots disagreeing.
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
            // A half-restored fleet is exactly the state this path exists to
            // prevent, so it is recorded rather than only toasted — and the
            // record stays un-rolled-back, which is what keeps the retry live.
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

    /// Accept the divergence offer: put the roots the run CHANGED
    /// back the way they were, leaving the roots it failed in untouched — they
    /// never moved, so there is nothing there to undo.
    ///
    /// Delegates the whole undo to [`Self::bulk_rollback`] for the one reason
    /// that matters: the record it reads is the run monitor's own per-root
    /// truth, preflight-scoped, and a second rollback path would have to
    /// re-derive which roots moved and would be free to get it wrong.
    pub fn rollback_divergent_roots(&mut self) -> RollbackOutcome {
        let Some(offer) = self.ui.branch_divergence.take() else {
            return RollbackOutcome::Refused;
        };
        let outcome = self.bulk_rollback(offer.record_id);
        if outcome == RollbackOutcome::Refused {
            // The offer outlived its record (evicted from the history list, or
            // rolled back from the row beside it). Say so durably, rather than
            // leaving the developer to wonder whether the roots were restored.
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

    /// Decline the divergence offer: the roots stay as the run left
    /// them. Declining is an answer, not a dismissal, so it is recorded — the
    /// feed has to show that the divergence was seen and consciously kept, or a
    /// later reader cannot tell it from a divergence nobody noticed.
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

    /// Welcome-vs-shell routing (issue #9, spec §9.2): the central body shows
    /// the Welcome page when no repository root is open, or when the user
    /// explicitly returned to it (File → Welcome). A project opened at launch
    /// (`turbogit <path>`) enters the shell directly.
    pub fn show_welcome(&self) -> bool {
        self.multi.roots.is_empty() || self.ui.welcome_visible
    }

    /// Arm the post-hoc undo for a completed targeted rewrite:
    /// capture the tip the rewrite finished on, which is what the guard later
    /// compares against. The read is a single synchronous `rev-parse`, on the
    /// same footing as [`Self::bulk_rollback`]'s engine calls — settlement is
    /// the one moment the rewrite's own tip is guaranteed to be HEAD.
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

    /// The repository-changed guard, the substance of the undo: an
    /// offer is valid only while HEAD still stands where the rewrite left it.
    /// Re-run on every completed operation whose scope covers the offer's
    /// root — the check is against the repository NOW, never against what it
    /// was at rewrite time. A moved (or unresolvable) HEAD withholds; the
    /// banner paints the reason and no control, so an undo is never
    /// offered-then-failed.
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

/// The fields every monitored run's view shares, built once at dispatch.
///
/// **The field order is load-bearing.** `started_at` and `history_id` each read
/// a clock, and a struct literal is evaluated in source order, so
/// [`std::time::Instant::now()`] precedes the `history_id` wall-clock read.
/// Reordering them, or hoisting `history_id` into a `let`, changes which instant
/// the run is recorded as having started — and nothing in the test tree sees
/// that, so a broken order still passes.
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
    /// Seed a run about to dispatch over `fleet`; the view keeps its own clone
    /// of `control` while the pool gets the original, so the monitor's stop
    /// button and the pool share one flag.
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

    /// The bulk grid's monitor for this run. `undo` is captured by the caller
    /// *before* the plan reaches the pass: a retry pass re-dispatches through
    /// this same view, so it must merge into this same `history_id` — one
    /// history record per run.
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

    /// The cherry-pick-across monitor for this run (issue 16).
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
            history_id: self.history_id,
        }
    }
}

/// Outcome fold for the passes that report only what ran: a root a stop halted
/// while queued contributes no row, so it stays a *skipped* row, not a failure.
fn done_rows_only(outcomes: Vec<(RootId, bulk_run::RunOutcome)>) -> Vec<(RootId, TgResult<()>)> {
    outcomes
        .into_iter()
        .filter_map(|(root, outcome)| match outcome {
            bulk_run::RunOutcome::Done(result) => Some((root, result)),
            bulk_run::RunOutcome::Stopped => None,
        })
        .collect()
}

/// Outcome fold for commit-across, which reports a root a stop halted as a
/// *failed* row — a commit monitor has to say a repo did not get its commit.
/// The deliberate counterpart to [`done_rows_only`], not an oversight.
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

/// Post one multi-root run's completion — the aggregate toast, the activity
/// entry, the recent-bulk-operations record and the full refresh all drain out
/// of this single [`AppEvent::BulkCompleted`].
///
/// The remote manager's apply ends here too, and sets `self.ui.busy` itself: it
/// is a pool spawn with no monitor, no stop handle and no progress events, so
/// [`AppState::spawn_cascade_pass`]'s `RunSeed`/`RunControl` would go unused — and
/// a spinner over every cascade would be a behaviour change.
fn post_bulk_completed(tx: &Sender<AppEvent>, label: String, results: Vec<(RootId, TgResult<()>)>) {
    let _ = tx.send(AppEvent::BulkCompleted { label, results });
}

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
                    Err(e) => {
                        // The refusal is typed here or not at all: `to_string`
                        // is where the step's error stops being a `TgError`, and
                        // the durable per-root entry is decided from this fact.
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

/// The short form of a commit id, in operation labels and toast text.
pub(crate) fn short_sha(id: &str) -> String {
    short_commit_ref(id)
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
///
/// The name filter stays a `filter_map` over a **presence**, not over a label: a
/// root whose directory name is not UTF-8 has no name to print, so it is absent
/// from the `failed:` list rather than shown under a placeholder. That is a
/// preserved data loss with a ruling still owed — see the pin in
/// `tests/bulk_ops.rs`, which fails on purpose when the ruling lands.
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
