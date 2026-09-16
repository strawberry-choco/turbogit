//! VCS Operations Popup (M1) + Command Palette / "Find Action" (Epic F5).
//!
//! `Alt+\`` opens the context-sensitive VCS operations list. `Ctrl+Shift+A`
//! opens the command palette: a fuzzy-searchable list of every action, the
//! IntelliJ "Find Action" hallmark.

use crate::theme::Palette;
use egui::{Pos2, RichText, Ui};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use turbogit_app::granular;
use turbogit_app::recents::{RecentKind, RecentProject};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, Tab, TagType, Toast};

/// Every globally-invokable action, reused by both the VCS popup and the
/// command palette — and by the shell's Git menu (issue #9).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Fetch,
    Pull,
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
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Refresh => "Refresh",
            Action::Fetch => "Fetch",
            Action::Pull => "Pull",
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
        }
    }

    /// The VCS operations popup's action set: exactly the pre-existing list
    /// (issue #22 keeps it unchanged while the palette grows).
    pub fn all() -> &'static [Action] {
        &[
            Action::Refresh,
            Action::Fetch,
            Action::Pull,
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
        ]
    }
}

pub fn run_action(state: &mut AppState, action: Action) {
    let root = state.selected_path();
    match action {
        // Manual refresh (decision 8): the full scoped refresh — drops every
        // cache entry (decorations and path history included) and rescans.
        Action::Refresh => state.refresh(Affected::All),
        Action::Fetch => {
            let r = root.clone();
            state.run_git(
                "Fetch".into(),
                Affected::from_optional_root(r.as_deref()),
                move |v| {
                    if let Some(r) = &r {
                        v.fetch(r, None)
                    } else {
                        Ok(())
                    }
                },
            );
        }
        Action::Pull => {
            let r = root.clone();
            let rebase =
                state.settings.update_method == turbogit_domain::model::UpdateMethod::Rebase;
            state.run_git(
                "Pull".into(),
                Affected::from_optional_root(r.as_deref()),
                move |v| {
                    if let Some(r) = &r {
                        v.pull(r, rebase)
                    } else {
                        Ok(())
                    }
                },
            );
        }
        Action::Push => state.ui.dialog = Some(Dialog::Push),
        Action::Branches => state.ui.branches_popup = true,
        Action::NewBranch => state.ui.dialog = Some(Dialog::NewBranch),
        Action::Merge => state.ui.dialog = Some(Dialog::Merge),
        Action::Rebase => state.ui.dialog = Some(Dialog::Rebase),
        Action::Stash => state.ui.dialog = Some(Dialog::Stash),
        Action::Shelve => state.ui.dialog = Some(Dialog::Shelve),
        // Screen 16: the tag dialog re-opens with Annotated selected and
        // fresh caches; the typed name/message survive the round trip.
        Action::Tag => {
            state.ui.dlg.tag_type = TagType::Annotated;
            state.ui.dlg.tag_target_picker_open = false;
            state.ui.dlg.tag_candidates = None;
            state.ui.dlg.tag_existing = None;
            state.ui.dlg.tag_signing_key = None;
            state.ui.dlg.tag_signing_key_fetched = false;
            state.ui.dialog = Some(Dialog::Tag);
        }
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
        // Topbar workspace picker (issue #34): the same surface the selector
        // opens. No anchor on this route — the fallback position is used.
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
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.spacing_mut().button_padding = egui::vec2(8.0, 2.0);
            ui.spacing_mut().interact_size.y = 16.0;
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
                                ui.label("No matching actions.");
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

/// Topbar workspace picker (issue #34): the recents list, plus the two
/// folder-picker flows the Welcome screen offers. Reachable from the topbar
/// workspace selector and from the palette's [`Action::SwitchWorkspace`].
///
/// A state-driven [`egui::Window`] rather than a response-bound
/// `Popup::menu` (ADR-0017): `Popup::menu` derives its open state from a
/// click on a response that only exists during the topbar's own render, so
/// the palette could never open it.
pub fn workspace_picker(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    // egui's own dropdown idiom (containers/popup.rs): a click outside only
    // dismisses once the surface was visible on the PREVIOUS frame, so the
    // selector click that opens the picker cannot also close it this frame.
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

    let pos = state
        .ui
        .workspace_picker_anchor
        .map(|(x, y)| Pos2::new(x, y))
        .unwrap_or_else(|| Pos2::new(160.0, super::shell::TOPBAR_HEIGHT + 4.0));
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
                ui.label(RichText::new("No recent workspaces").color(Palette::INK_3));
            }
            ui.spacing_mut().item_spacing.y = 2.0;
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
