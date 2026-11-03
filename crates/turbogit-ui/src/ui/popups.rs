use egui::{Pos2, Ui};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use turbogit_app::granular;
use turbogit_app::operation::{LifecycleVerb, Operation};
use turbogit_app::recents::{RecentKind, RecentProject};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, NewBranchBase, PatchSource, Tab, Toast};
use turbogit_domain::model::DisplayName;

use crate::ui::components;
use crate::ui::icons::Icon;
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

/// What one answer from the picker means for the shell.
enum WorkspaceMenuAction {
    /// The row that answered is the workspace already open, so the picker closes
    /// and nothing re-dispatches.
    Stay,
    Enter(RecentProject),
    OpenProject,
    AttachRoot,
}

fn workspace_menu(ui: &mut Ui, rows: &[WorkspaceRow]) -> Option<WorkspaceMenuAction> {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_min_width(296.0);
    ui.set_max_width(320.0);

    let mut picked = None;
    widgets::group_title(ui, "Current");
    for (idx, row) in rows.iter().enumerate() {
        let is_current = row.is_current;
        let count = if row.recent.kind == RecentKind::Workspace {
            row.recent
                .repo_count
                .map(|n| components::counted(n, "repo", "repos"))
        } else {
            None
        };
        let full_path = row.recent.path.display().to_string();
        // The current row skips the path column: the rail header it hangs under
        // already states where this workspace is.
        let path = (!is_current).then_some(full_path.as_str());
        // `menu_item` puts `label` in the accessibility tree and paints `shortcut`
        // beside it, and the "current" caption is paint only, so the name has to
        // carry the fact for a reader that cannot see the caption.
        let open_mark = if is_current { " · current" } else { "" };
        let accessible = match &count {
            Some(count) => format!("{} · {count}{open_mark}", row.recent.name),
            None => format!("{}{open_mark}", row.recent.name),
        };
        let response = widgets::menu_item(
            ui,
            widgets::MenuItemProps {
                icon: Icon::FOLDER_GIT,
                label: &row.recent.name,
                data: path,
                shortcut: count.as_deref(),
                state: widgets::MenuItemState::enabled(),
                kind: widgets::MenuItemKind::Default,
            },
        )
        .on_hover_text(full_path);
        // The closure is stored as `Fn`, not consumed once, so it clones rather
        // than moves.
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, is_current, accessible.clone())
        });
        if response.clicked() {
            picked = Some(if is_current {
                WorkspaceMenuAction::Stay
            } else {
                WorkspaceMenuAction::Enter(row.recent.clone())
            });
        }
        // The reference's project widget heads each half of its list with a
        // caption, so the open workspace is named by its group rather than
        // marked inside its own row.
        if is_current && idx + 1 < rows.len() {
            widgets::menu_rule(ui);
            widgets::group_title(ui, "Recent");
        }
    }

    widgets::menu_rule(ui);
    for (icon, label, action) in [
        (
            Icon::FOLDER_OPEN,
            "Open Project…",
            WorkspaceMenuAction::OpenProject,
        ),
        (
            Icon::PLUS,
            "Attach Workspace Root…",
            WorkspaceMenuAction::AttachRoot,
        ),
    ] {
        if widgets::menu_item(
            ui,
            widgets::MenuItemProps {
                icon,
                label,
                data: None,
                shortcut: None,
                state: widgets::MenuItemState::enabled(),
                kind: widgets::MenuItemKind::Default,
            },
        )
        .clicked()
        {
            picked = Some(action);
        }
    }
    picked
}

pub fn workspace_picker(ui: &mut Ui, state: &mut AppState) {
    if state.show_welcome() {
        state.ui.workspace_picker_open = false;
    }
    if !state.ui.workspace_picker_open {
        return;
    }
    // The palette route has no trigger rect to hang under, so it aims at the
    // corner of the rail the header occupies.
    let anchor = state
        .ui
        .workspace_picker_anchor
        .map_or(Pos2::new(160.0, 4.0), |(x, y)| Pos2::new(x, y));
    let menu = widgets::MenuId::new("workspace_picker", "");
    widgets::menu_host::note_anchor(ui, menu, anchor);

    let rows = workspace_rows(state);
    let mut dismiss = false;
    let picked =
        widgets::menu_host::host_menu(ui, menu, true, &mut dismiss, |ui| workspace_menu(ui, &rows))
            .flatten();

    if dismiss {
        state.ui.workspace_picker_open = false;
        return;
    }
    match picked {
        None => {}
        Some(WorkspaceMenuAction::Stay) => state.ui.workspace_picker_open = false,
        Some(WorkspaceMenuAction::Enter(recent)) => {
            state.ui.workspace_picker_open = false;
            state.open_recent(&recent);
        }
        Some(WorkspaceMenuAction::OpenProject) => {
            if let Some(dir) = super::welcome::pick_dir_public(state, "Open Project") {
                state.open_project(&dir);
            }
        }
        Some(WorkspaceMenuAction::AttachRoot) => {
            if let Some(dir) = super::welcome::pick_dir_public(state, "Attach Workspace Root") {
                state.attach_workspace(&dir);
            }
        }
    }
}
