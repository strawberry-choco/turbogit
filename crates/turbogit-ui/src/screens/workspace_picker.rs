//! The workspace picker: the popover over the shell's project trigger, one
//! row per open workspace and recent, plus the two launch verbs. Replaces
//! the egui popups.rs workspace path and its menu_host anchor.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::recents::{RecentKind, RecentProject};
use turbogit_app::state::AppState;
use turbogit_domain::model::DisplayName;

use crate::bridge::AppHandle;
use crate::screens::picks::pick_folder;
use crate::test_ids;
use crate::theme as data;
use crate::ui::popover::{PopoverProps, popover};
use crate::ui::row::{RowProps, RowState, row};

/// The switch-project trigger the shell's top bar mounts: the button named
/// `Switch project` whose painted text is `<project> · N repos`, wrapped in
/// the picker popover.
pub fn picker_trigger(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let trigger_handle = handle.clone();
    let count_handle = handle.clone();
    let open = signal(false);
    let open_for_trigger = open;
    let open_for_change = open;
    let trigger = view()
        .role(AccessibilityRole::Button)
        .label("Switch project")
        .test_id(&test_ids::workspace("switch"))
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .h(px(26.0))
        .px(px(8.0))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(AdaptiveColor::TRANSPARENT)
        .hover(|s| s.bg(t.muted))
        .text_size(px(data::TYPE_CONTROL + 0.5))
        .child(crate::ui::icon::icon(
            crate::ui::icon::Icon::Folder,
            data::ICON_SM,
            t.muted_foreground,
        ))
        .child(
            text(move || {
                SharedString::from(
                    trigger_handle
                        .read(|s| s.project_dir.display_name_or("<workspace>").to_string()),
                )
            })
            .font_weight(FontWeight::MEDIUM)
            .text_color(t.foreground),
        )
        .child(
            // One repo is the name's own fact; the count only earns space
            // when the workspace holds several.
            text(move || {
                SharedString::from(count_handle.read(|s| {
                    if s.multi.roots.len() > 1 {
                        counted(s.multi.roots.len())
                    } else {
                        String::new()
                    }
                }))
            })
            .text_size(px(data::TYPE_CHIP))
            .text_color(t.muted_foreground),
        )
        .child(crate::ui::icon::icon(
            crate::ui::icon::Icon::ChevronDown,
            data::ICON_SM,
            t.muted_foreground,
        ));
    popover(
        PopoverProps::new(trigger)
            .width(320.0)
            .open(Prop::bind(move || Some(open_for_trigger.get())))
            .on_open_change(move |next| open_for_change.set(next))
            .child(picker_body(handle, open)),
    )
}

/// What a row announces: its name, the workspace kind's count, and the
/// current mark.
fn accessible_label(recent: &RecentProject) -> String {
    let count = if recent.kind == RecentKind::Workspace {
        recent.repo_count.map(counted)
    } else {
        None
    };
    match count {
        Some(count) => format!("{} · {count}", recent.name),
        None => recent.name.clone(),
    }
}

/// "N repos" or "1 repo", the one counted spelling.
fn counted(n: usize) -> String {
    if n == 1 {
        format!("{n} repo")
    } else {
        format!("{n} repos")
    }
}

/// The picker's content: the open workspace under Current, the store under
/// Recent, and the two launch verbs.
fn picker_body(handle: AppHandle, open: Signal<bool>) -> impl IntoElement {
    let t = theme();
    let current_handle = handle.clone();
    let recents_handle = handle.clone();
    let recents_open = open;
    let actions_open = open;
    let open_handle = handle.clone();
    let attach_handle = handle.clone();

    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::workspace("picker"))
        .child(
            text("Current")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(current_row(current_handle, open))
        .child(
            text("Recent")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(view().flex_col().max_h(px(420.0)).child(each(
            {
                let recents_handle = recents_handle.clone();
                move || {
                    recents_handle.read(|s| {
                        workspace_rows(s)
                            .into_iter()
                            .filter(|row| !row.is_current)
                            .map(|row| row.recent)
                            .collect::<Vec<_>>()
                    })
                }
            },
            |recent| recent.path.display().to_string(),
            move |recent, _| {
                let t = theme();
                let open_recent = recents_handle.clone();
                let close = recents_open;
                let recent_value = recent.get();
                row(RowProps::new()
                    .state(RowState::Default)
                    .test_id(&test_ids::workspace(format!("row:{}", recent_value.name)))
                    .accessibility_label_of(move || accessible_label(&recent.get()))
                    .on_press(move || {
                        let recent = recent.get();
                        open_recent.update(|s| s.open_recent(&recent));
                        close.set(false);
                    })
                    .child(
                        text(move || SharedString::from(recent.get().name.clone()))
                            .text_size(px(data::TYPE_BODY))
                            .text_color(t.card_foreground),
                    )
                    .child(
                        text(move || recent.get().path.display().to_string())
                            .flex_1()
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.muted_foreground),
                    )
                    .child(
                        text(move || {
                            let recent = recent.get();
                            if recent.kind == RecentKind::Workspace {
                                recent.repo_count.map(counted).unwrap_or_default()
                            } else {
                                String::new()
                            }
                        })
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(data::ACCENT_TEXT),
                    ))
            },
        )))
        .child(action_row(
            "Open Project…",
            test_ids::workspace("open"),
            move |s, dir| s.open_project(&dir),
            open_handle,
            actions_open,
        ))
        .child(action_row(
            "Attach Workspace Root…",
            test_ids::workspace("attach"),
            move |s, dir| s.attach_workspace(&dir),
            attach_handle,
            actions_open,
        ))
}

/// The row of the workspace already open: pressing it closes the picker
/// and re-dispatches nothing.
fn current_row(handle: AppHandle, open: Signal<bool>) -> impl IntoElement {
    let t = theme();
    let count_handle = handle.clone();
    let name_handle = handle.clone();
    row(RowProps::new()
        .state(RowState::Default)
        .test_id(&test_ids::workspace("row-current"))
        .on_press(move || open.set(false))
        .accessibility_label_of(move || {
            handle.peek(|s| {
                format!(
                    "{} · {} · current",
                    s.project_dir.display_name_or("<workspace>"),
                    counted(s.multi.roots.len())
                )
            })
        })
        .child(
            text(move || {
                SharedString::from(
                    name_handle.read(|s| s.project_dir.display_name_or("<workspace>").to_string()),
                )
            })
            .text_size(px(data::TYPE_BODY))
            .text_color(t.card_foreground),
        )
        .child(
            text(move || SharedString::from(count_handle.read(|s| counted(s.multi.roots.len()))))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::ACCENT_TEXT),
        )
        .child(
            text("current")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        ))
}

/// One launch verb of the picker, through the file-dialog seam.
fn action_row(
    label: &'static str,
    id: String,
    enter: fn(&mut AppState, std::path::PathBuf),
    handle: AppHandle,
    open: Signal<bool>,
) -> impl IntoElement {
    let t = theme();
    let press_handle = handle;
    row(RowProps::new()
        .state(RowState::Default)
        .test_id(&id)
        .on_press(move || {
            open.set(false);
            pick_folder(press_handle.clone(), label_for_verb(label), enter);
        })
        .child(
            text(label)
                .text_size(px(data::TYPE_BODY))
                .text_color(t.card_foreground),
        ))
}

/// The seam's purpose label a verb opens the folder dialog under.
fn label_for_verb(label: &str) -> &'static str {
    match label {
        "Attach Workspace Root…" => "Attach Workspace Root",
        _ => "Open Project",
    }
}

/// One row of the picker: the recent project itself, with the workspace
/// kind's repo count.
struct WorkspaceRow {
    recent: RecentProject,
    is_current: bool,
}

/// The open workspace first, then the store's other records, the egui
/// `workspace_rows`.
fn workspace_rows(state: &AppState) -> Vec<WorkspaceRow> {
    let current = RecentProject {
        path: state.project_dir.clone(),
        name: state
            .project_dir
            .display_name_lossy_or(&state.project_dir.to_string_lossy())
            .to_string(),
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
