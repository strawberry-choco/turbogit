//! The Remotes dialog: every registered root's remotes with fetch and push
//! URLs, the add band, and the per-row rename, edit, set-upstream, and
//! remove verbs. Every verb is a `shell_writes::remote` operation.

use std::time::Duration;

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::shell_writes::remote as writes;
use turbogit_app::state::{AppState, RemoteRowAction};
use turbogit_domain::model::{BranchKind, Root, RootId};

use crate::bridge::AppHandle;
use crate::screens::branch_dialogs::repo_scope_band;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, input};
use crate::ui::row::{RowProps, RowState, row};

/// The Manage remotes dialog.
pub fn manage_remotes(handle: AppHandle) -> AnyElement {
    // The body repaints when the remote data or the open row editor moves,
    // never on a keystroke: a rebuilt input loses the rest of the typist's
    // characters (the bridge's rule about inputs that echo state).
    let key = signal(handle.peek(remote_key));
    let poll = handle.clone();
    let held = key;
    set_interval(Duration::from_millis(160), move || {
        let next = poll.peek(remote_key);
        if next != held.get() {
            held.set(next);
        }
    });
    dynamic(move || {
        let _ = key.get();
        remotes_body(handle.clone()).into_element()
    })
    .into_element()
}

fn remote_key(state: &AppState) -> (u64, Option<(RootId, String, RemoteRowAction)>) {
    (
        state.caches.revision(),
        state.ui.dlg.remotes_row_action.clone(),
    )
}

fn remotes_body(handle: AppHandle) -> AnyElement {
    let t = theme();
    let body_handle = handle;
    let close_handle = body_handle.clone();
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::remote("body"))
        .child(repo_scope_band(&body_handle));
    let roots: Vec<Root> = body_handle.peek(|s| s.multi.roots.clone());
    if roots.is_empty() {
        body = body.child(
            text("No repository registered.")
                .text_size(px(data::TYPE_BODY))
                .text_color(t.muted_foreground),
        );
    }
    for root in &roots {
        body = body.child(
            text(root.id.name())
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
        if root.remotes.is_empty() {
            body = body.child(
                text("No remotes configured.")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.muted_foreground),
            );
        }
        for remote in &root.remotes {
            body = body.child(remote_row(body_handle.clone(), root, remote.name.clone()));
        }
    }

    body = body
        .child(
            text("ADD REMOTE · added to every repository in scope")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(add_band(body_handle.clone()));

    dialog(
        DialogProps::new("manage-remotes", "Manage remotes")
            .on_cancel(move || close_handle.update(|s| s.ui.dialog = None))
            .child(body)
            .action("Close")
            .on_action({
                let handle = body_handle.clone();
                move || handle.update(|s| s.ui.dialog = None)
            }),
    )
    .into_element()
}

fn add_band(handle: AppHandle) -> impl IntoElement {
    let name_handle = handle.clone();
    let fetch_handle = handle.clone();
    let push_handle = handle.clone();
    let add_handle = handle.clone();
    let mut band = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::remote("add-band"))
        .child(input(
            InputProps::new()
                .placeholder("Remote name")
                .accessibility_label("Remote name")
                .test_id(&test_ids::remote("add-name"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    name_handle.update(move |s| s.ui.dlg.remotes_add_name = text);
                }),
        ))
        .child(input(
            InputProps::new()
                .placeholder("Fetch URL")
                .accessibility_label("Remote fetch URL")
                .test_id(&test_ids::remote("add-fetch"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    fetch_handle.update(move |s| s.ui.dlg.remotes_add_fetch = text);
                }),
        ))
        .child(input(
            InputProps::new()
                .placeholder("Push URL (blank = fetch)")
                .accessibility_label("Remote push URL")
                .test_id(&test_ids::remote("add-push"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    push_handle.update(move |s| s.ui.dlg.remotes_add_push = text);
                }),
        ));
    band = band.child(button(
        ButtonProps::new("Add remote")
            .variant(ButtonVariant::Primary)
            .test_id(&test_ids::remote("add"))
            .on_press(move || {
                let (name, fetch, push, scope) = add_handle.peek(|s| {
                    (
                        s.ui.dlg.remotes_add_name.trim().to_string(),
                        s.ui.dlg.remotes_add_fetch.trim().to_string(),
                        s.ui.dlg.remotes_add_push.trim().to_string(),
                        s.ui.dlg.repo_scope.clone(),
                    )
                });
                if name.is_empty() || fetch.is_empty() {
                    return;
                }
                add_handle.update(move |s| {
                    for id in scope {
                        if let Some(root) = s.multi.by_id(&id) {
                            let path = root.path.clone();
                            s.dispatch(writes::add(&id, &path, &name, &fetch, &push));
                        }
                    }
                });
            }),
    ));
    band
}

fn remote_row(handle: AppHandle, root: &Root, name: String) -> impl IntoElement {
    let t = theme();
    let id = root.id.clone();
    let root_path = root.path.clone();
    let action = handle.peek(|s| s.ui.dlg.remotes_row_action.clone());
    let remote = root.remotes.iter().find(|r| r.name == name);
    let fetch_url = remote
        .and_then(|r| r.fetch_url.clone())
        .unwrap_or_else(|| "—".to_string());
    let push_url = remote.and_then(|r| r.push_url.clone());

    let mut body = view()
        .flex_col()
        .gap(px(2.0))
        .test_id(&test_ids::remote(format!("row-{name}")))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text(name.clone())
                        .text_size(px(data::TYPE_CONTROL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground),
                )
                .child(row_actions(
                    handle.clone(),
                    root,
                    name.clone(),
                    action.clone(),
                )),
        )
        .child(
            text(format!("fetch: {fetch_url}"))
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    if let Some(push) = push_url {
        body = body.child(
            text(format!("push: {push}"))
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    }

    let editor_cond = handle.clone();
    let row_id = id.clone();
    let row_name = name.clone();
    let editor_handle = handle.clone();
    let root_for_editor = root.clone();
    body.child(when(
        move || {
            editor_cond.read(|s| {
                s.ui.dlg
                    .remotes_row_action
                    .as_ref()
                    .is_some_and(|(r, n, _)| r == &row_id && *n == row_name)
            })
        },
        move || {
            let which = editor_handle
                .peek(|s| s.ui.dlg.remotes_row_action.as_ref().map(|(_, _, w)| *w))
                .unwrap_or(RemoteRowAction::None);
            match which {
                RemoteRowAction::Rename => {
                    rename_editor(editor_handle.clone(), &id, &root_path, name.clone())
                        .into_element()
                }
                RemoteRowAction::EditUrl => {
                    url_editor(editor_handle.clone(), &id, &root_path, name.clone()).into_element()
                }
                RemoteRowAction::SetUpstream => {
                    upstream_editor(editor_handle.clone(), &root_for_editor, name.clone())
                        .into_element()
                }
                RemoteRowAction::None => view().into_element(),
            }
        },
    ))
}

fn row_actions(
    handle: AppHandle,
    root: &Root,
    name: String,
    action: Option<(RootId, String, RemoteRowAction)>,
) -> impl IntoElement {
    let t = theme();
    let id = root.id.clone();
    let path = root.path.clone();
    let editing = action
        .as_ref()
        .is_some_and(|(row_root, n, _)| row_root == &id && *n == name);
    let mut strip = view().flex_row().items_center().gap(px(4.0));
    if editing {
        let cancel = handle.clone();
        let cancel_id = id.clone();
        let cancel_name = name.clone();
        strip = strip.child(
            view()
                .role(AccessibilityRole::Button)
                .label("Cancel")
                .test_id(&test_ids::remote("cancel"))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .on_press(move || {
                    let id = cancel_id.clone();
                    let name = cancel_name.clone();
                    cancel.update(move |s| {
                        if s.ui
                            .dlg
                            .remotes_row_action
                            .as_ref()
                            .is_some_and(|(r, n, _)| r == &id && *n == name)
                        {
                            s.ui.dlg.remotes_row_action = None;
                        }
                    });
                })
                .child(
                    text("Cancel")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                ),
        );
        return strip;
    }
    for (label, which, key) in [
        ("Set upstream…", RemoteRowAction::SetUpstream, "upstream"),
        ("Rename", RemoteRowAction::Rename, "rename"),
        ("Edit URL", RemoteRowAction::EditUrl, "edit"),
    ] {
        let press = handle.clone();
        let id = id.clone();
        let name = name.clone();
        let path = path.clone();
        let fetch = root
            .remotes
            .iter()
            .find(|r| r.name == name)
            .and_then(|r| r.fetch_url.clone())
            .unwrap_or_default();
        let push = root
            .remotes
            .iter()
            .find(|r| r.name == name)
            .and_then(|r| r.push_url.clone())
            .unwrap_or_default();
        strip = strip.child(
            view()
                .role(AccessibilityRole::Button)
                .label(label)
                .test_id(&test_ids::remote(format!("{key}-{name}")))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .hover(|s| s.bg(t.muted))
                .on_press(move || {
                    let id = id.clone();
                    let name = name.clone();
                    let fetch = fetch.clone();
                    let push = push.clone();
                    let _ = &path;
                    press.update(move |s| {
                        s.ui.dlg.remotes_rename_new = name.clone();
                        s.ui.dlg.remotes_edit_fetch = fetch.clone();
                        s.ui.dlg.remotes_edit_push = push.clone();
                        s.ui.dlg.remotes_row_action = Some((id.clone(), name.clone(), which));
                    });
                })
                .child(
                    text(label)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                ),
        );
    }
    let remove = handle;
    let id = id.clone();
    let name_for_remove = name.clone();
    strip.child(
        view()
            .role(AccessibilityRole::Button)
            .label("Remove")
            .test_id(&test_ids::remote(format!("remove-{name}")))
            .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
            .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
            .rounded(px(data::CONTROL_RADIUS))
            .hover(|s| s.bg(t.muted))
            .on_press(move || {
                let root = id.clone();
                let name = name_for_remove.clone();
                remove.update(move |s| {
                    if let Some(repo) = s.multi.by_id(&root) {
                        let path = repo.path.clone();
                        s.dispatch(writes::remove(&root, &path, &name));
                    }
                });
            })
            .child(
                text("Remove")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(data::DANGER),
            ),
    )
}

fn rename_editor(
    handle: AppHandle,
    root: &RootId,
    path: &std::path::Path,
    name: String,
) -> impl IntoElement {
    let edit_handle = handle.clone();
    let save_handle = handle.clone();
    let root = root.clone();
    let path = path.to_path_buf();
    let name_for_save = name.clone();
    let value_handle = handle.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .test_id(&test_ids::remote("rename-row"))
        .child(input(
            InputProps::new()
                .default_value_of(move || {
                    value_handle.peek(|s| s.ui.dlg.remotes_rename_new.clone())
                })
                .accessibility_label("New remote name")
                .test_id(&test_ids::remote("rename-input"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    edit_handle.update(move |s| s.ui.dlg.remotes_rename_new = text);
                }),
        ))
        .child(
            view()
                .role(AccessibilityRole::Button)
                .label("Rename")
                .test_id(&test_ids::remote("rename-save"))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .bg(theme().secondary)
                .on_press(move || {
                    let peek_root = root.clone();
                    let peek_path = path.clone();
                    let (new, root, path) = save_handle.peek(move |s| {
                        (
                            s.ui.dlg.remotes_rename_new.trim().to_string(),
                            peek_root.clone(),
                            peek_path.clone(),
                        )
                    });
                    if !new.is_empty() && new != name_for_save {
                        let name_for_save = name_for_save.clone();
                        save_handle.update(move |s| {
                            s.dispatch(writes::rename(&root, &path, &name_for_save, &new));
                            s.ui.dlg.remotes_row_action = None;
                        });
                    }
                })
                .child(text("Rename").text_size(px(data::TYPE_CONTROL))),
        )
}

fn url_editor(
    handle: AppHandle,
    root: &RootId,
    path: &std::path::Path,
    name: String,
) -> impl IntoElement {
    let t = theme();
    let fetch_handle = handle.clone();
    let push_handle = handle.clone();
    let save_handle = handle.clone();
    let root = root.clone();
    let path = path.to_path_buf();
    let value_handle = handle.clone();
    view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::remote("edit-row"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text("Fetch:")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                )
                .child(input(
                    InputProps::new()
                        .default_value_of({
                            let handle = value_handle.clone();
                            move || handle.peek(|s| s.ui.dlg.remotes_edit_fetch.clone())
                        })
                        .accessibility_label("Edit fetch URL")
                        .test_id(&test_ids::remote("edit-fetch"))
                        .on_change(move |text: &str| {
                            let text = text.to_string();
                            fetch_handle.update(move |s| s.ui.dlg.remotes_edit_fetch = text);
                        }),
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text("Push:")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                )
                .child(input(
                    InputProps::new()
                        .default_value_of(move || {
                            value_handle.peek(|s| s.ui.dlg.remotes_edit_push.clone())
                        })
                        .accessibility_label("Edit push URL")
                        .test_id(&test_ids::remote("edit-push"))
                        .on_change(move |text: &str| {
                            let text = text.to_string();
                            push_handle.update(move |s| s.ui.dlg.remotes_edit_push = text);
                        }),
                )),
        )
        .child(
            view()
                .role(AccessibilityRole::Button)
                .label("Save")
                .test_id(&test_ids::remote("edit-save"))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .bg(theme().secondary)
                .on_press(move || {
                    let peek_root = root.clone();
                    let peek_path = path.clone();
                    let name = name.clone();
                    let (fetch, push, root, path) = save_handle.peek(move |s| {
                        (
                            s.ui.dlg.remotes_edit_fetch.clone(),
                            s.ui.dlg.remotes_edit_push.clone(),
                            peek_root.clone(),
                            peek_path.clone(),
                        )
                    });
                    save_handle.update(move |s| {
                        s.dispatch(writes::update(&root, &path, &name, &fetch, &push));
                        s.ui.dlg.remotes_row_action = None;
                    });
                })
                .child(text("Save").text_size(px(data::TYPE_CONTROL))),
        )
}

fn upstream_editor(handle: AppHandle, root: &Root, name: String) -> impl IntoElement {
    let t = theme();
    let id = root.id.clone();
    let path = root.path.clone();
    let local: Vec<String> = root
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local)
        .map(|b| b.name.clone())
        .collect();
    let mut list = view()
        .flex_col()
        .gap(px(2.0))
        .test_id(&test_ids::remote("upstream-row"))
        .child(
            text("Set upstream for:")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    for branch in local {
        let press = handle.clone();
        let id = id.clone();
        let path = path.clone();
        let remote = name.clone();
        let upstream = format!("{name}/{branch}");
        let label = format!("{branch} → {upstream}");
        list = list.child(row(RowProps::new()
            .state(RowState::Default)
            .test_id(&test_ids::remote(format!("upstream-{branch}")))
            .on_press(move || {
                let root = id.clone();
                let path = path.clone();
                let branch = branch.clone();
                let upstream = format!("{remote}/{}", branch);
                press.update(move |s| {
                    s.dispatch(writes::set_upstream(&root, &path, &branch, &upstream));
                    s.ui.dlg.remotes_row_action = None;
                });
            })
            .child(text(label).text_size(px(data::TYPE_CONTROL)))));
    }
    list
}

/// The Remotes route: the same manager over its own page.
pub fn remotes_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    view()
        .flex_1()
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("remotes"))
        .child(manage_remotes(handle))
}
