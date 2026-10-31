use egui::{Pos2, Ui};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use turbogit_app::granular;
use turbogit_app::operation::{LifecycleVerb, Operation};
use turbogit_app::recents::{RecentKind, RecentProject};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, NewBranchBase, PatchSource, Tab, Toast};
use turbogit_domain::model::DisplayName;

use crate::ui::widgets;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Fetch,
    Pull,
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
    GoToLog,
    OpenWelcome,
    SwitchWorkspace,
    StageHunk,
    UnstageHunk,
    FilterFiles,
    AbortOperation,
    ContinueOperation,
    SkipCommit,
    ApplyPatch,
    ApplyPatchFromClipboard,
    Unshelve,
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
        Action::PushSimple => {
            super::push_control::push_without_dialog(state);
        }
        Action::Push => state.ui.dialog = Some(Dialog::Push),
        Action::Branches => state.ui.branches_popup = true,
        Action::NewBranch => state.open_new_branch(NewBranchBase::Head, true, None),
        Action::Merge => state.ui.dialog = Some(Dialog::Merge),
        Action::Rebase => state.ui.dialog = Some(Dialog::Rebase),
        Action::Stash => state.ui.dialog = Some(Dialog::Stash),
        Action::Shelve => state.ui.dialog = Some(Dialog::Shelve),
        Action::Tag => state.open_tag_dialog(None),
        Action::CommitTab => state.ui.tab = Tab::Commit,
        Action::Settings => state.ui.settings_open = true,
        Action::Clone => {
            state.ui.toast = Some(Toast::info("Clone opens from the Welcome screen."));
        }
        Action::GoToLog => {
            state.ui.welcome_visible = false;
            state.ui.tab = Tab::Log;
        }
        Action::OpenWelcome => state.ui.welcome_visible = true,
        Action::SwitchWorkspace => {
            state.ui.workspace_picker_open = true;
            state.ui.workspace_picker_anchor = None;
        }
        Action::StageHunk | Action::UnstageHunk => {
            let dir = if action == Action::StageHunk {
                granular::Direction::Stage
            } else {
                granular::Direction::Unstage
            };
            let Some(change) = state.ui.preview_change.clone() else {
                return;
            };
            let hunk = state.ui.diff_current_hunk;
            granular::dispatch(state, change, granular::HunkTarget::Whole(hunk, dir));
        }
        Action::FilterFiles => {
            state.ui.welcome_visible = false;
            state.ui.tab = Tab::Commit;
            state.ui.focus_file_filter = true;
        }
        Action::AbortOperation => run_lifecycle_verb(state, LifecycleVerb::Abort),
        Action::ContinueOperation => run_lifecycle_verb(state, LifecycleVerb::Continue),
        Action::SkipCommit => run_lifecycle_verb(state, LifecycleVerb::Skip),
        Action::ApplyPatch => turbogit_app::apply_patch::open(state, PatchSource::File),
        Action::ApplyPatchFromClipboard => {
            turbogit_app::apply_patch::open(state, PatchSource::Clipboard)
        }
        Action::CreatePatch => turbogit_app::create_patch::open_from_selection(state),
        Action::CopyPatchToClipboard => turbogit_app::create_patch::copy_from_selection(state),
        Action::Unshelve => state.open_unshelve_dialog(),
    }
}

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
    egui::Window::new("VCS Operations")
        .open(&mut open)
        .resizable(false)
        .show(&ctx, |ui| {
            for a in Action::all() {
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

fn palette_matches(query: &str) -> Vec<Action> {
    let actions = Action::palette_actions();
    if query.is_empty() {
        return actions.to_vec();
    }
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

pub fn command_palette(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.command_palette {
        return;
    }
    let ctx = ui.ctx().clone();

    let row_h = 24.0;
    let matches = palette_matches(&state.ui.command_query);
    let view_h = ctx.input(|i| i.viewport_rect().height());
    let list_h = ((matches.len() as f32 + 1.0) * row_h).min((view_h - 220.0).max(120.0));

    let mut open = true;
    egui::Window::new("Find Action")
        .open(&mut open)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
        .default_width(420.0)
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
                        .id_salt("palette_matches")
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

struct WorkspaceRow {
    recent: RecentProject,
    is_current: bool,
}

fn workspace_rows(state: &AppState) -> Vec<WorkspaceRow> {
    let current = RecentProject {
        path: state.project_dir.clone(),
        name: state
            .project_dir
            .display_name_lossy_or(&state.project_dir.to_string_lossy()),
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

    if state.show_welcome() {
        state.ui.workspace_picker_open = false;
        return;
    }

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
    let escaped = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if escaped || (was_open_last_frame && window.response.clicked_elsewhere()) {
        state.ui.workspace_picker_open = false;
    }
}
