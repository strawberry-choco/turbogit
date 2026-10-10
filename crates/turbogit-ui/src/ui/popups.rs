//! VCS Operations Popup (M1) + Command Palette / "Find Action" (Epic F5).
//!
//! `Alt+\`` opens the context-sensitive VCS operations list. `Ctrl+Shift+A`
//! opens the command palette: a fuzzy-searchable list of every action, the
//! IntelliJ "Find Action" hallmark.

use egui::{Pos2, Ui};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use turbogit_app::granular;
use turbogit_app::operation::{LifecycleVerb, Operation};
use turbogit_app::recents::{RecentKind, RecentProject};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, PatchSource, Tab, Toast};

use crate::ui::widgets;

/// Every globally-invokable action, reused by both the VCS popup and the
/// command palette — and by the shell's Git menu (issue #9).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Fetch,
    Pull,
    /// The push that runs without the dialog, for the workspace the panel would
    /// only confirm. Absent from the push row when the push is not one
    /// unambiguous verb — see [`super::push_control`].
    PushSimple,
    Push,
    Branches,
    NewBranch,
    Merge,
    Rebase,
    Stash,
    Shelve,
    Tag,
    CommitTab,
    Settings,
    Clone,
    // Shell-navigation extras (issue #22 / ADR-0011): palette-only. The VCS
    // operations popup keeps its exact pre-existing action set.
    GoToLog,
    OpenWelcome,
    // Topbar workspace picker (issue #34): palette-only like its siblings —
    // the VCS operations popup set stays frozen (pinned by
    // `feedback_chrome.rs`, which fails loudly if it changes).
    SwitchWorkspace,
    // Partial-staging verbs (spec R2): palette-only, operating on the diff
    // viewer's current hunk. The VCS operations popup set stays frozen.
    StageHunk,
    UnstageHunk,
    // File filter (spec R7): palette-only discoverability for the `/`
    // shortcut over the Commit tool window's changed-file list.
    FilterFiles,
    // In-flight operation lifecycle: the reference's own docs say the
    // recovery survives a dismissed notification — "from the main menu choose
    // `Git | Continue rebase` to resume it". TurboGit has no main menu bar, so
    // the palette is its menu route for these three verbs, and the status bar's
    // clickable item is the other. They are PALETTE-ONLY on purpose: the VCS
    // popup's action set is a frozen pin (`feedback_chrome.rs`), and the
    // announcement itself lives in the bar rather than in that popup.
    AbortOperation,
    ContinueOperation,
    SkipCommit,
    // `Git | Patch | Apply patch…` and its clipboard twin.
    // PALETTE-ONLY for the same reason as the three verbs above: the reference's
    // route is a main menu bar this app does not have, and the VCS popup's action
    // set is a frozen pin. The two are separate entries because the second is
    // defined by the path it does not ask for.
    ApplyPatch,
    ApplyPatchFromClipboard,
    // `Unshelve…` (surface 07): the reference reaches this dialog from a menu, and the
    // palette is this app's menu. PALETTE-ONLY like the verbs above — the VCS popup's
    // action set is a frozen pin — because the Shelf pane's own header already carries
    // the verb where the shelves are listed.
    Unshelve,
    // The same menu's other half: `Git | Patch | Create Patch from Local Changes`
    // and its dialog-free clipboard twin. PALETTE-ONLY for
    // the reason the two above are, and the clipboard entry is separate because it
    // is defined by the dialog it does not open.
    CreatePatch,
    CopyPatchToClipboard,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Refresh => "Refresh",
            Action::Fetch => "Fetch",
            Action::Pull => "Pull",
            Action::PushSimple => "Push",
            Action::Push => "Push…",
            Action::Branches => "Branches…",
            Action::NewBranch => "New Branch…",
            Action::Merge => "Merge…",
            Action::Rebase => "Rebase…",
            Action::Stash => "Stash…",
            Action::Shelve => "Shelve…",
            Action::Tag => "Tag…",
            Action::CommitTab => "Go to Commit",
            Action::Settings => "Settings…",
            Action::Clone => "Clone…",
            Action::GoToLog => "Go to Log",
            Action::OpenWelcome => "Open Welcome",
            Action::SwitchWorkspace => "Switch Workspace…",
            Action::StageHunk => "Stage Hunk",
            Action::UnstageHunk => "Unstage Hunk",
            Action::FilterFiles => "Filter files (/)",
            Action::AbortOperation => "Abort Operation",
            Action::ContinueOperation => "Continue Operation",
            Action::SkipCommit => "Skip Commit",
            Action::ApplyPatch => "Apply patch\u{2026}",
            Action::ApplyPatchFromClipboard => "Apply patch from Clipboard",
            Action::Unshelve => "Unshelve\u{2026}",
            Action::CreatePatch => crate::ui::create_patch::LOCAL_CHANGES_VERB,
            Action::CopyPatchToClipboard => crate::ui::create_patch::CLIPBOARD_VERB,
        }
    }

    /// The VCS operations popup's action set: the pre-existing list plus the
    /// direct push verb, which the reference ranks as the DEFAULT of its three
    /// push actions; nothing else in the list changes while the palette grows.
    pub fn all() -> &'static [Action] {
        &[
            Action::Refresh,
            Action::Fetch,
            Action::Pull,
            Action::PushSimple,
            Action::Push,
            Action::Branches,
            Action::NewBranch,
            Action::Merge,
            Action::Rebase,
            Action::Stash,
            Action::Shelve,
            Action::Tag,
            Action::CommitTab,
            Action::Settings,
            Action::Clone,
        ]
    }

    /// The command palette's action set (ADR-0011): every existing entry plus
    /// the shell-navigation actions the new shell makes meaningful.
    pub fn palette_actions() -> &'static [Action] {
        &[
            Action::Refresh,
            Action::Fetch,
            Action::Pull,
            Action::PushSimple,
            Action::Push,
            Action::Branches,
            Action::NewBranch,
            Action::Merge,
            Action::Rebase,
            Action::Stash,
            Action::Shelve,
            Action::Tag,
            Action::CommitTab,
            Action::Settings,
            Action::Clone,
            Action::GoToLog,
            Action::OpenWelcome,
            Action::SwitchWorkspace,
            Action::StageHunk,
            Action::UnstageHunk,
            Action::FilterFiles,
            Action::AbortOperation,
            Action::ContinueOperation,
            Action::SkipCommit,
            Action::ApplyPatch,
            Action::ApplyPatchFromClipboard,
            Action::Unshelve,
            Action::CreatePatch,
            Action::CopyPatchToClipboard,
        ]
    }
}

pub fn run_action(state: &mut AppState, action: Action) {
    let root = state.selected_root.clone();
    match action {
        // Manual refresh (decision 8): the full scoped refresh — drops every
        // cache entry (decorations and path history included) and rescans.
        Action::Refresh => state.refresh(Affected::All),
        Action::Fetch => {
            if let Some(r) = root {
                state.dispatch(Operation::Fetch { roots: vec![r] });
            }
        }
        Action::Pull => {
            if let Some(r) = root {
                state.dispatch_pull("Pull", r, None);
            }
        }
        // The direct verb falls back to the panel when the workspace needs it:
        // a palette entry that did nothing would be a dead verb, and the panel is
        // the surface that can still answer.
        Action::PushSimple => {
            super::push_control::push_without_dialog(state);
        }
        Action::Push => state.ui.dialog = Some(Dialog::Push),
        Action::Branches => state.ui.branches_popup = true,
        Action::NewBranch => state.ui.dialog = Some(Dialog::NewBranch),
        Action::Merge => state.ui.dialog = Some(Dialog::Merge),
        Action::Rebase => state.ui.dialog = Some(Dialog::Rebase),
        Action::Stash => state.ui.dialog = Some(Dialog::Stash),
        Action::Shelve => state.ui.dialog = Some(Dialog::Shelve),
        // Screen 16: the tag dialog re-opens with Annotated selected and fresh
        // reads; the typed name/message survive the round trip. The reset is
        // `AppState::open_tag_dialog`, shared with the log's New tag item so the
        // two entry points cannot drift into opening different dialogs.
        Action::Tag => state.open_tag_dialog(None),
        Action::CommitTab => state.ui.tab = Tab::Commit,
        Action::Settings => state.ui.settings_open = true,
        Action::Clone => {
            // The clone flow lives on the Welcome screen (ticket #10).
            state.ui.toast = Some(Toast::info("Clone opens from the Welcome screen."));
        }
        // Shell navigation (issue #22 / ADR-0011).
        Action::GoToLog => {
            state.ui.welcome_visible = false;
            state.ui.tab = Tab::Log;
        }
        Action::OpenWelcome => state.ui.welcome_visible = true,
        // Workspace picker (issue #34). No anchor on this route — the
        // palette is the only entry point since the topbar's selector was
        // deleted, so the fallback position is the only one left.
        Action::SwitchWorkspace => {
            state.ui.workspace_picker_open = true;
            state.ui.workspace_picker_anchor = None;
        }
        // Partial-staging verbs (spec R2/R7):
        // CURRENT hunk — the single selection buttons, hover, and keyboard
        // navigation all aim (CONTEXT.md "Current hunk"). The preview target
        // must exist, or the verb is a silent no-op; conflicted files are
        // blocked by the core op.
        Action::StageHunk | Action::UnstageHunk => {
            let stage = action == Action::StageHunk;
            let Some(path) = state.ui.preview_change.clone() else {
                return;
            };
            let hunk = state.ui.diff_current_hunk;
            granular::dispatch(state, path, granular::HunkTarget::Whole(hunk), stage);
        }
        // File filter (spec R7): surface the Commit tool window and arm
        // next-frame focus of its inline filter input.
        Action::FilterFiles => {
            state.ui.welcome_visible = false;
            state.ui.tab = Tab::Commit;
            state.ui.focus_file_filter = true;
        }
        // The in-flight lifecycle verbs: the menu route to exactly the
        // operation the status bar's clickable item runs.
        Action::AbortOperation => run_lifecycle_verb(state, LifecycleVerb::Abort),
        Action::ContinueOperation => run_lifecycle_verb(state, LifecycleVerb::Continue),
        Action::SkipCommit => run_lifecycle_verb(state, LifecycleVerb::Skip),
        // The two apply routes, both reaching the same window: what differs is only
        // whether the developer is asked for a file.
        Action::ApplyPatch => state.open_apply_patch(PatchSource::File),
        Action::ApplyPatchFromClipboard => state.open_apply_patch(PatchSource::Clipboard),
        // The two create routes, and the difference between them is the number of
        // dialogs: one ends in `Patch File Settings`, the other hands the bytes to
        // the clipboard and asks nothing on the way.
        Action::CreatePatch => state.open_create_patch_from_selection(),
        Action::CopyPatchToClipboard => state.copy_patch_from_selection(),
        // The unshelve window opens through the app's own opener, shared with the
        // Shelf pane's header action, so the two routes cannot arm different defaults.
        Action::Unshelve => state.open_unshelve_dialog(),
    }
}

/// Run one lifecycle verb on the root that is holding an operation which offers
/// it.
///
/// The root comes from [`super::status_inflight::target_for`], which asks the
/// engine which operation holds each root: the focused root when it offers the
/// verb, otherwise the root that does. Nothing in flight says so out loud — a
/// palette entry that silently did nothing is a dead verb, and these three are
/// the documented way back after the notification carrying the same verbs has
/// been dismissed.
fn run_lifecycle_verb(state: &mut AppState, verb: LifecycleVerb) {
    match super::status_inflight::target_for(state, verb) {
        Some((root, owner)) => state.dispatch(Operation::Resolve { root, owner, verb }),
        None => {
            state.ui.toast = Some(Toast::info(format!(
                "Nothing in flight to {}",
                verb.label()
            )))
        }
    }
}

pub fn vcs_operations(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.vcs_popup {
        return;
    }
    let ctx = ui.ctx().clone();
    let mut open = state.ui.vcs_popup;
    // Popup chrome (issue #22, spec §10): SURFACE fill, LINE border, radius 8
    // — mapped centrally via `theme::configure_style`; fixed-size like the
    // palette. The action list itself is untouched.
    egui::Window::new("VCS Operations")
        .open(&mut open)
        .resizable(false)
        .show(&ctx, |ui| {
            for a in Action::all() {
                // The two push verbs share one row rather than two list entries:
                // which of them exists depends on the workspace, and a flat list
                // cannot show that one of a pair is the safe default.
                if *a == Action::PushSimple {
                    super::push_control::popup_row(ui, state);
                    continue;
                }
                if *a == Action::Push {
                    continue;
                }
                if ui.button(a.label()).clicked() {
                    run_action(state, *a);
                }
            }
        });
    if !open {
        state.ui.vcs_popup = false;
    }
}

/// Fuzzy-rank the palette action set against `query` (L3: nucleo-matcher
/// replaces the old lowercase-substring filter). An empty query keeps every
/// action in the default order; otherwise non-matching entries are dropped
/// and survivors sort by descending score, ties broken by default order.
fn palette_matches(query: &str) -> Vec<Action> {
    let actions = Action::palette_actions();
    if query.is_empty() {
        return actions.to_vec();
    }
    // Built per invocation: the palette is a single small list, so reusing a
    // matcher across frames buys nothing and UI state must stay untouched.
    let mut matcher = Matcher::new(Config::DEFAULT);
    // `Pattern::new` splits on whitespace into independently fuzzy-matched
    // words ("go log" finds "Go to Log") without the `!`/`^`/`'` syntax of
    // `Pattern::parse`; case-insensitive like the filter it replaces.
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize)> = actions
        .iter()
        .enumerate()
        .filter_map(|(i, &a)| {
            let haystack = Utf32Str::new(a.label(), &mut buf);
            pattern
                .score(haystack, &mut matcher)
                .map(|score| (score, i))
        })
        .collect();
    scored.sort_by_key(|&(score, i)| (std::cmp::Reverse(score), i));
    scored.into_iter().map(|(_, i)| actions[i]).collect()
}

/// Command palette (Epic F5 / "Find Action"). Searchable, keyboard-friendly.
pub fn command_palette(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.command_palette {
        return;
    }
    let ctx = ui.ctx().clone();

    // Compact rows so the full action set fits unscrolled (issue #22: every
    // entry must stay listed and reachable, ADR-0011).
    let row_h = 24.0;
    let matches = palette_matches(&state.ui.command_query);
    let view_h = ctx.input(|i| i.viewport_rect().height());
    let list_h = ((matches.len() as f32 + 1.0) * row_h).min((view_h - 220.0).max(120.0));

    let mut open = true;
    egui::Window::new("Find Action")
        .open(&mut open)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
        .default_width(420.0)
        // Floor the outer window height so the Resize state's remembered
        // size can never clamp the list below the full action set (the
        // Branches-popup lesson, applied at the Window level).
        .min_height(105.0 + list_h)
        .resizable(false)
        .show(&ctx, |ui| {
            ui.text_edit_singleline(&mut state.ui.command_query)
                .request_focus();
            ui.separator();
            ui.spacing_mut().item_spacing.y = crate::theme::DENSITY_LIST_GAP;
            ui.spacing_mut().button_padding = crate::theme::DENSITY_COMMAND_BUTTON;
            ui.spacing_mut().interact_size.y = crate::theme::DENSITY_COMMAND_ROW_HEIGHT;
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), list_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(list_h)
                        .show(ui, |ui| {
                            for &a in &matches {
                                if ui.selectable_label(false, a.label()).clicked() {
                                    run_action(state, a);
                                    state.ui.command_palette = false;
                                }
                            }
                            if matches.is_empty() {
                                widgets::empty_state(ui, "No matching actions.");
                            }
                        });
                },
            );
        });
    if !open {
        state.ui.command_palette = false;
    }
}

// ------------------------------------------------------ workspace picker ---

/// One row of the workspace picker: a recents entry — or the synthesized
/// current workspace — with its inert/active state.
struct WorkspaceRow {
    recent: RecentProject,
    is_current: bool,
}

/// Current workspace first (synthesized — `launch_in` registers roots but
/// does not record a recent, so a workspace opened from the CLI would
/// otherwise be missing from its own picker), then the recents, deduped by
/// path.
fn workspace_rows(state: &AppState) -> Vec<WorkspaceRow> {
    let current = RecentProject {
        path: state.project_dir.clone(),
        name: turbogit_app::recents::project_name(&state.project_dir),
        last_opened: i64::MAX,
        kind: if state.multi.roots.len() > 1 {
            RecentKind::Workspace
        } else {
            RecentKind::Project
        },
        repo_count: Some(state.multi.roots.len()),
    };
    let mut rows: Vec<WorkspaceRow> = vec![WorkspaceRow {
        recent: current,
        is_current: true,
    }];
    rows.extend(
        state
            .ui
            .recent_projects
            .iter()
            .filter(|r| r.path != state.project_dir)
            .cloned()
            .map(|recent| WorkspaceRow {
                recent,
                is_current: false,
            }),
    );
    rows
}

/// Workspace picker (issue #34): the recents list, plus the two
/// folder-picker flows the Welcome screen offers. Two routes open it — the
/// palette's [`Action::SwitchWorkspace`] and the sidebar's workspace header
/// row, which records the anchor the dropdown hangs from.
///
/// A state-driven [`egui::Window`] rather than a response-bound
/// `Popup::menu` (ADR-0017): `Popup::menu` derives its open state from a
/// click on a response that only exists during the trigger's own render, so
/// neither the palette nor a later frame could open it.
pub fn workspace_picker(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    // egui's own dropdown idiom (containers/popup.rs): a click outside only
    // dismisses once the surface was visible on the PREVIOUS frame, so the
    // trigger click that opens the picker cannot also close it this frame.
    let was_open_id = egui::Id::new("turbogit_workspace_picker_was_open");
    if !state.ui.workspace_picker_open {
        ctx.memory_mut(|m| m.data.insert_temp(was_open_id, false));
        return;
    }
    let was_open_last_frame = ctx.memory(|m| m.data.get_temp::<bool>(was_open_id) == Some(true));
    ctx.memory_mut(|m| m.data.insert_temp(was_open_id, true));

    // The Welcome page's own four cards already own these flows.
    if state.show_welcome() {
        state.ui.workspace_picker_open = false;
        return;
    }

    // Position: the sidebar's workspace header records an anchor at click
    // time (ADR-0017) so the dropdown hangs under the row. `None` — the
    // palette route, which has no trigger rect — falls back to the fixed
    // position below.
    let pos = state
        .ui
        .workspace_picker_anchor
        .map(|(x, y)| Pos2::new(x, y))
        .unwrap_or_else(|| Pos2::new(160.0, 4.0));
    let rows = workspace_rows(state);

    let mut open = true;
    let window = egui::Window::new("Switch Workspace")
        .open(&mut open)
        .fixed_pos(pos)
        .title_bar(false)
        .resizable(false)
        .min_width(260.0)
        .show(&ctx, |ui| {
            if rows.is_empty() {
                widgets::empty_state(ui, "No recent workspaces");
            }
            ui.spacing_mut().item_spacing.y = crate::theme::DENSITY_LIST_GAP;
            for row in &rows {
                let mut label = row.recent.name.clone();
                if row.recent.kind == RecentKind::Workspace
                    && let Some(n) = row.recent.repo_count
                {
                    label.push_str(&format!("  ·  {n} repos"));
                }
                if row.is_current {
                    label.push_str("  ·  current");
                }
                let resp = ui
                    .button(label)
                    .on_hover_text(row.recent.path.display().to_string());
                if resp.clicked() {
                    // The current row is inert: re-dispatching would reset
                    // `selected_root` and drop every cache for no
                    // user-visible gain.
                    if !row.is_current {
                        state.open_recent(&row.recent);
                    }
                    state.ui.workspace_picker_open = false;
                }
            }
            ui.separator();
            if ui.button("Open Project…").clicked() {
                state.ui.workspace_picker_open = false;
                if let Some(dir) = super::welcome::pick_dir_public(state, "Open Project") {
                    state.open_project(&dir);
                }
            }
            if ui.button("Attach Workspace Root…").clicked() {
                state.ui.workspace_picker_open = false;
                if let Some(dir) = super::welcome::pick_dir_public(state, "Attach Workspace Root") {
                    state.attach_workspace(&dir);
                }
            }
        });
    if !open {
        state.ui.workspace_picker_open = false;
    }
    let Some(window) = window else {
        return;
    };
    // Dismissal: Esc, or a click outside the dropdown (see the idiom above —
    // only after the picker has been visible for a frame).
    let escaped = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if escaped || (was_open_last_frame && window.response.clicked_elsewhere()) {
        state.ui.workspace_picker_open = false;
    }
}
