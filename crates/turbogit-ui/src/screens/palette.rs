//! The command palette: the action table, filtered, over the command
//! control. Every entry runs a state verb; the entries that open another
//! surface go through that surface's own open path, never a local copy.

use nsh_base::{CommandItem, theme};
use nsh_core::prelude::*;
use turbogit_app::granular;
use turbogit_app::operation::{LifecycleVerb, Operation, PushRequest};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, NewBranchBase, PatchSource, Tab, Toast};
use turbogit_app::sync_service::{self, DirectPushTarget};

use crate::bridge::AppHandle;
use crate::screens::settings;
use crate::screens::shell::{TabSwitcher, target_for};
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::command::{CommandProps, command};
use crate::ui::dialog::{DialogProps, dialog};

/// One palette action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    FetchAll,
    PullAll,
    PushAll,
    CreateBranchAll,
    StashAll,
    CustomAll,
    /// The N5/N6 surfaces a command opens: the pages the migration's action
    /// band used to host, now one searchable entry each.
    SurfaceRewrite,
    SurfaceConflicts,
    SurfaceRebase,
    SurfaceRebasePlan,
    SurfaceCherryPick,
    SurfaceGetFromBranch,
    SurfacePatches,
    SurfaceShelves,
    SurfaceBulk,
    SurfaceBlame,
    SurfaceCustomCommand,
    SurfaceActionList,
}

/// Every action, in palette order.
pub const ACTIONS: [Action; 47] = [
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
    Action::FetchAll,
    Action::PullAll,
    Action::PushAll,
    Action::CreateBranchAll,
    Action::StashAll,
    Action::CustomAll,
    Action::SurfaceRewrite,
    Action::SurfaceConflicts,
    Action::SurfaceRebase,
    Action::SurfaceRebasePlan,
    Action::SurfaceCherryPick,
    Action::SurfaceGetFromBranch,
    Action::SurfacePatches,
    Action::SurfaceShelves,
    Action::SurfaceBulk,
    Action::SurfaceBlame,
    Action::SurfaceCustomCommand,
    Action::SurfaceActionList,
];

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
            Action::ApplyPatch => "Apply patch…",
            Action::ApplyPatchFromClipboard => "Apply patch from Clipboard",
            Action::Unshelve => "Unshelve…",
            Action::CreatePatch => "Create Patch from Local Changes",
            Action::CopyPatchToClipboard => "Copy as Patch to Clipboard",
            Action::FetchAll => "Fetch All Repositories",
            Action::PullAll => "Pull All Repositories",
            Action::PushAll => "Push All Repositories",
            Action::CreateBranchAll => "Create Branch in All Repositories",
            Action::StashAll => "Stash All Repositories",
            Action::CustomAll => "Custom Command in All Repositories",
            Action::SurfaceRewrite => "Rewrite",
            Action::SurfaceConflicts => "Conflicts",
            Action::SurfaceRebase => "Rebase",
            Action::SurfaceRebasePlan => "Edit Rebase Plan",
            Action::SurfaceCherryPick => "Cherry-pick",
            Action::SurfaceGetFromBranch => "Get from Branch",
            Action::SurfacePatches => "Patches",
            Action::SurfaceShelves => "Shelves",
            Action::SurfaceBulk => "Bulk Operations",
            Action::SurfaceBlame => "Blame",
            Action::SurfaceCustomCommand => "Custom Command",
            Action::SurfaceActionList => "All Actions",
        }
    }

    /// The selector-safe id the command row reports.
    fn slug(self) -> &'static str {
        match self {
            Action::Refresh => "refresh",
            Action::Fetch => "fetch",
            Action::Pull => "pull",
            Action::PushSimple => "push-simple",
            Action::Push => "push",
            Action::Branches => "branches",
            Action::NewBranch => "new-branch",
            Action::Merge => "merge",
            Action::Rebase => "rebase",
            Action::Stash => "stash",
            Action::Shelve => "shelve",
            Action::Tag => "tag",
            Action::CommitTab => "commit-tab",
            Action::Settings => "settings",
            Action::Clone => "clone",
            Action::GoToLog => "go-to-log",
            Action::OpenWelcome => "open-welcome",
            Action::SwitchWorkspace => "switch-workspace",
            Action::StageHunk => "stage-hunk",
            Action::UnstageHunk => "unstage-hunk",
            Action::FilterFiles => "filter-files",
            Action::AbortOperation => "abort-operation",
            Action::ContinueOperation => "continue-operation",
            Action::SkipCommit => "skip-commit",
            Action::ApplyPatch => "apply-patch",
            Action::ApplyPatchFromClipboard => "apply-patch-clipboard",
            Action::Unshelve => "unshelve",
            Action::CreatePatch => "create-patch",
            Action::CopyPatchToClipboard => "copy-patch",
            Action::FetchAll => "fetch-all",
            Action::PullAll => "pull-all",
            Action::PushAll => "push-all",
            Action::CreateBranchAll => "create-branch-all",
            Action::StashAll => "stash-all",
            Action::CustomAll => "custom-all",
            Action::SurfaceRewrite => "surface-rewrite",
            Action::SurfaceConflicts => "surface-conflicts",
            Action::SurfaceRebase => "surface-rebase",
            Action::SurfaceRebasePlan => "surface-rebase-plan",
            Action::SurfaceCherryPick => "surface-cherry-pick",
            Action::SurfaceGetFromBranch => "surface-get-from-branch",
            Action::SurfacePatches => "surface-patches",
            Action::SurfaceShelves => "surface-shelves",
            Action::SurfaceBulk => "surface-bulk",
            Action::SurfaceBlame => "surface-blame",
            Action::SurfaceCustomCommand => "surface-custom-command",
            Action::SurfaceActionList => "surface-action-list",
        }
    }

    fn from_slug(slug: &str) -> Option<Action> {
        ACTIONS.iter().copied().find(|action| action.slug() == slug)
    }

    /// The route a surface command opens; a verb names none.
    fn surface_route(self) -> Option<&'static str> {
        Some(match self {
            Action::SurfaceRewrite => "rewrite",
            Action::SurfaceConflicts => "conflicts",
            Action::SurfaceRebase => "rebase",
            Action::SurfaceRebasePlan => "rebase_editor",
            Action::SurfaceCherryPick => "cherry_pick",
            Action::SurfaceGetFromBranch => "get_from_branch",
            Action::SurfacePatches => "patches",
            Action::SurfaceShelves => "unshelve",
            Action::SurfaceBulk => "bulk",
            Action::SurfaceBlame => "blame",
            Action::SurfaceCustomCommand => "custom_command",
            Action::SurfaceActionList => "palette",
            _ => return None,
        })
    }

    /// The list group a command reads under.
    fn group(self) -> &'static str {
        if self.surface_route().is_some() {
            "Surfaces"
        } else {
            "Actions"
        }
    }
}

/// Runs one entry through the state verbs it names.
pub fn run(
    handle: &AppHandle,
    switcher: &TabSwitcher,
    route: Signal<Option<String>>,
    action: Action,
) {
    if let Some(id) = action.surface_route() {
        route.set(Some(id.to_string()));
        return;
    }
    match action {
        Action::CommitTab => {
            route.set(None);
            switcher.select(Tab::Commit);
        }
        Action::GoToLog => {
            route.set(None);
            handle.update(|s| s.ui.welcome_visible = false);
            switcher.select(Tab::Log);
        }
        Action::FilterFiles => {
            handle.update(|s| {
                s.ui.welcome_visible = false;
                s.ui.focus_file_filter = true;
            });
            route.set(None);
            switcher.select(Tab::Commit);
        }
        Action::Branches => {
            route.set(None);
            switcher.select(Tab::Branches);
        }
        Action::Settings => settings::open(handle),
        Action::CustomAll => {
            let custom = handle.clone();
            custom.update(|s| s.open_bulk_preflight(turbogit_app::bulk_ops::BulkOp::Custom, &[]));
        }
        other => {
            let handle = handle.clone();
            handle.update(|s| run_state(s, other));
        }
    }
}

fn run_state(state: &mut AppState, action: Action) {
    let root = state.action_root();
    match action {
        Action::Refresh => state.refresh(Affected::All),
        Action::Fetch => {
            let roots = match root {
                Some(root) => vec![root],
                None => state.multi.roots.iter().map(|r| r.id.clone()).collect(),
            };
            if !roots.is_empty() {
                state.dispatch(Operation::Fetch { roots });
            }
        }
        Action::Pull => match root {
            Some(root) => state.dispatch_pull("Pull", root, None),
            None => fleet(state, turbogit_app::bulk_ops::BulkOp::PullAll),
        },
        Action::PushSimple => push_without_dialog(state),
        Action::Push => state.open_push(),
        Action::Branches => state.ui.branches_popup = true,
        Action::NewBranch => {
            state.open_new_branch(root, NewBranchBase::Head, true, None);
        }
        Action::Merge => state.open_dialog(Dialog::Merge, None),
        Action::Rebase => state.open_dialog(Dialog::Rebase, None),
        Action::Stash => state.open_dialog(Dialog::Stash, None),
        Action::Shelve => state.open_shelve_dialog(None),
        Action::Tag => state.open_tag_dialog(None, None),
        Action::Clone => {
            state.ui.toast = Some(Toast::info("Clone opens from the Welcome screen."));
        }
        Action::OpenWelcome => {
            state.ui.welcome_visible = true;
        }
        Action::SwitchWorkspace => state.ui.workspace_picker_open = true,
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
        Action::AbortOperation => lifecycle(state, LifecycleVerb::Abort),
        Action::ContinueOperation => lifecycle(state, LifecycleVerb::Continue),
        Action::SkipCommit => lifecycle(state, LifecycleVerb::Skip),
        Action::ApplyPatch => turbogit_app::apply_patch::open(state, PatchSource::File),
        Action::ApplyPatchFromClipboard => {
            turbogit_app::apply_patch::open(state, PatchSource::Clipboard)
        }
        Action::Unshelve => state.open_unshelve_dialog(),
        Action::CreatePatch => turbogit_app::create_patch::open_from_selection(state),
        Action::CopyPatchToClipboard => turbogit_app::create_patch::copy_from_selection(state),
        Action::FetchAll => fleet(state, turbogit_app::bulk_ops::BulkOp::FetchAll),
        Action::PullAll => fleet(state, turbogit_app::bulk_ops::BulkOp::PullAll),
        Action::PushAll => fleet(state, turbogit_app::bulk_ops::BulkOp::PushAll),
        Action::CreateBranchAll => fleet(state, turbogit_app::bulk_ops::BulkOp::CreateBranch),
        Action::StashAll => fleet(state, turbogit_app::bulk_ops::BulkOp::StashAll),
        Action::CustomAll => fleet(state, turbogit_app::bulk_ops::BulkOp::Custom),
        // Surface commands never reach the state verbs: `run` opens their
        // route before this match.
        Action::SurfaceRewrite
        | Action::SurfaceConflicts
        | Action::SurfaceRebase
        | Action::SurfaceRebasePlan
        | Action::SurfaceCherryPick
        | Action::SurfaceGetFromBranch
        | Action::SurfacePatches
        | Action::SurfaceShelves
        | Action::SurfaceBulk
        | Action::SurfaceBlame
        | Action::SurfaceCustomCommand
        | Action::SurfaceActionList => {}
        Action::CommitTab | Action::GoToLog | Action::FilterFiles | Action::Settings => {}
    }
}

fn fleet(state: &mut AppState, op: turbogit_app::bulk_ops::BulkOp) {
    state.open_bulk_preflight(op, &[]);
}

fn lifecycle(state: &mut AppState, verb: LifecycleVerb) {
    let selected = state.selected_change_roots();
    match target_for(state, &selected, verb) {
        Some((root, owner)) => state.dispatch(Operation::Resolve { root, owner, verb }),
        None => {
            state.ui.toast = Some(Toast::info(format!(
                "Nothing in flight to {}",
                verb.label()
            )))
        }
    }
}

/// The one-click target for the workspace as it stands, or `None` when only
/// the dialog can decide.
pub fn direct_target(state: &AppState) -> Option<DirectPushTarget> {
    let selection = state.selected_change_roots();
    let selected = (selection.len() == 1).then(|| selection[0].clone());
    let set: std::collections::HashSet<_> = selection.into_iter().collect();
    sync_service::direct_push_target(&state.multi.roots, selected.as_ref(), &set, &state.settings)
}

fn push_without_dialog(state: &mut AppState) {
    let Some(target) = direct_target(state) else {
        state.open_push();
        return;
    };
    let mut request =
        PushRequest::single(target.root, target.remote, target.branch, &state.settings);
    request.set_upstream = target.set_upstream;
    state.dispatch_push(request, None);
}

/// The dialog layer: the filterable action list while `ui.command_palette`.
pub fn layer(
    handle: AppHandle,
    switcher: TabSwitcher,
    route: Signal<Option<String>>,
) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.command_palette),
        move || {
            let query = handle.peek(|s| s.ui.command_query.clone());
            let close = handle.clone();
            let select = handle.clone();
            let select_switcher = switcher.clone();
            let select_route = route;
            let change = handle.clone();
            let list = command(
                CommandProps::new(command_items())
                    .default_query(query)
                    .placeholder("Search actions")
                    .accessibility_label("Command palette")
                    .search_accessibility_label("Search actions")
                    .test_id(&test_ids::palette("search"))
                    .on_query_change(move |text: &str| {
                        let text = text.to_string();
                        change.update(move |s| s.ui.command_query = text);
                    })
                    .on_cancel(move || close.update(|s| s.ui.command_palette = false))
                    .on_select(move |id: &str| {
                        let Some(action) = Action::from_slug(id) else {
                            return;
                        };
                        select.update(|s| s.ui.command_palette = false);
                        run(&select, &select_switcher, select_route, action);
                    }),
            );
            let cancel = handle.clone();
            let empty_handle = handle.clone();
            let empty = when(
                move || {
                    let query = empty_handle.read(|s| s.ui.command_query.clone());
                    !query.is_empty()
                        && !ACTIONS
                            .iter()
                            .any(|action| matches_query(&query, action.label()))
                },
                move || {
                    let t = theme();
                    text("No matching actions.")
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground)
                        .p(px(data::PANEL_PADDING))
                },
            );
            dialog(
                DialogProps::new("command-palette", "Find Action")
                    .width(440.0)
                    .cancel_label("Close")
                    .hide_action(true)
                    .on_cancel(move || cancel.update(|s| s.ui.command_palette = false))
                    .child(view().flex_col().max_h(px(360.0)).child(list).child(empty)),
            )
        },
    )
}

/// Whether `label` holds every whitespace-separated word of `query`, the
/// empty state's reading of the command control's own filter.
fn matches_query(query: &str, label: &str) -> bool {
    let label = label.to_lowercase();
    query
        .split_whitespace()
        .all(|word| label.contains(&word.to_lowercase()))
}

fn command_items() -> Vec<CommandItem> {
    ACTIONS
        .iter()
        .map(|action| CommandItem::new(action.slug(), action.label()).group(action.group()))
        .collect()
}

/// The route's screen: the action list as a page, each entry running its
/// verb. The palette dialog is what a key and the toolbar open.
pub fn palette_screen(
    handle: AppHandle,
    switcher: TabSwitcher,
    route: Signal<Option<String>>,
) -> impl IntoElement {
    let t = theme();
    let mut list = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("palette"));
    for action in ACTIONS {
        let press = handle.clone();
        let press_switcher = switcher.clone();
        let press_route = route;
        list = list.child(
            view()
                .role(AccessibilityRole::Button)
                .label(action.label())
                .test_id(&test_ids::palette(format!("action:{}", action.slug())))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .hover(|s| s.bg(t.muted))
                .on_press(move || run(&press, &press_switcher, press_route, action))
                .child(
                    text(action.label())
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        );
    }
    let open = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(button(
            ButtonProps::new("Find action…")
                .variant(ButtonVariant::Primary)
                .test_id(&test_ids::palette("open"))
                .on_press(move || {
                    open.update(|s| {
                        s.ui.command_query.clear();
                        s.ui.command_palette = true;
                    })
                }),
        ))
        .child(scroll().max_h(px(520.0)).child(list))
}
