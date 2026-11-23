//! Unshelve: the shelf picker grouped by repository, the keep-or-remove
//! option, and the apply that lands the work in its own working tree.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::state::Dialog;
use turbogit_domain::model::RootId;

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::tree::{TreeEntryProps, TreeProps, tree};

const REMOVE_OPTION: &str = "Remove successfully applied files from the shelf";

pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.dialog == Some(Dialog::Unshelve)),
        move || {
            let body = handle.clone();
            dynamic(move || {
                body.read(|_| ());
                unshelve_dialog(body.clone()).into_element()
            })
        },
    )
}

fn close(handle: &AppHandle) {
    handle.update(|s| s.ui.dialog = None);
}

fn unshelve_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let scope = handle.peek(|s| s.ui.dlg.repo_scope.clone());
    let mut choices: Vec<(RootId, String, String)> = Vec::new();
    for root in &scope {
        for shelf in handle.peek(|s| s.shelves_for(root)) {
            choices.push((shelf.root.clone(), shelf.name.clone(), shelf.root.name()));
        }
    }
    let (stored_root, stored_name) = handle.peek(|s| {
        (
            s.ui.dlg.unshelve_root.clone(),
            s.ui.dlg.unshelve_shelf.clone(),
        )
    });
    let picked = stored_root
        .clone()
        .zip(stored_name.clone())
        .filter(|(root, name)| {
            choices
                .iter()
                .any(|(choice_root, choice_name, _)| choice_root == root && choice_name == name)
        })
        .or_else(|| {
            choices
                .first()
                .map(|(root, name, _)| (root.clone(), name.clone()))
        });
    let remove = handle.peek(|s| s.ui.dlg.unshelve_remove);

    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::unshelve("body"))
        .child(
            text("NAME")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    if choices.is_empty() {
        body = body.child(
            text("Nothing is shelved in the selected repositories. Use “Shelve…” to park the selected changes, then unshelve from here.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground)
                .test_id(&test_ids::unshelve("empty")),
        );
    } else {
        let mut roots: Vec<(RootId, String)> = Vec::new();
        for (root, _, name) in &choices {
            if !roots.iter().any(|(id, _)| id == root) {
                roots.push((root.clone(), name.clone()));
            }
        }
        let mut items = Vec::new();
        for (root, name) in &roots {
            let mut entry =
                TreeEntryProps::new(format!("root:{}", root.name()), name.to_uppercase());
            for (at, (choice_root, choice_name, _)) in choices.iter().enumerate() {
                if choice_root == root {
                    entry = entry.child(TreeEntryProps::new(at.to_string(), choice_name.clone()));
                }
            }
            items.push(entry);
        }
        let select_handle = handle.clone();
        let select_choices = choices.clone();
        let mut props = TreeProps::new(items)
            .accessibility_label("Shelves")
            .test_id(&test_ids::unshelve("shelves"))
            .on_select(move |id: &str| {
                let Ok(at) = id.parse::<usize>() else {
                    return;
                };
                let Some((root, name, _)) = select_choices.get(at).cloned() else {
                    return;
                };
                select_handle.update(move |s| {
                    s.ui.dlg.unshelve_root = Some(root.clone());
                    s.ui.dlg.unshelve_shelf = Some(name.clone());
                });
            });
        for (root, _) in &roots {
            props = props.default_expanded(format!("root:{}", root.name()));
        }
        if let Some((root, name, _)) = &choices.iter().find(|(root, name, _)| {
            picked.as_ref().is_some_and(|(picked_root, picked_name)| {
                picked_root == root && picked_name == name
            })
        }) && let Some(at) = choices
            .iter()
            .position(|(choice_root, choice_name, _)| choice_root == root && choice_name == name)
        {
            props = props.default_selected(at.to_string());
        }
        body = body.child(tree(props));
    }
    body = body.child(
        text("The shelved work comes back into this repository's working tree. Nothing here sorts it into a named list, because this application has none.")
            .text_size(px(data::TYPE_CHIP))
            .text_color(t.muted_foreground),
    );
    body = body.child(
        text("Options")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    let remove_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(checkbox(
                CheckboxProps::new(move || {
                    if remove {
                        CheckboxState::Checked
                    } else {
                        CheckboxState::Unchecked
                    }
                })
                .accessibility_label(REMOVE_OPTION)
                .test_id(&test_ids::unshelve("remove-option"))
                .on_change(move |_| {
                    remove_handle.update(|s| s.ui.dlg.unshelve_remove = !s.ui.dlg.unshelve_remove);
                }),
            ))
            .child(
                text(REMOVE_OPTION)
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            ),
    );
    if remove {
        body = body.child(
            text("When the apply lands, the shelf record and git's copy of the content are both dropped. An apply that fails leaves the row listed and the content in the stash list.")
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::DANGER),
        );
    }
    let cancel = handle.clone();
    let mut props = DialogProps::new("unshelve", "Unshelve shelf")
        .width(520.0)
        .on_cancel(move || close(&cancel))
        .child(body);
    if let Some((root, name)) = picked {
        let confirm = handle.clone();
        props = props
            .action(if remove {
                "Unshelve and Remove"
            } else {
                "Unshelve"
            })
            .destructive(remove)
            .on_action(move || {
                let root = root.clone();
                let name = name.clone();
                confirm.update(move |s| {
                    let remove = s.ui.dlg.unshelve_remove;
                    s.unshelve(root.clone(), name.clone(), remove);
                    s.ui.dialog = None;
                });
            });
    } else {
        props = props.action("Unshelve").hide_action(true);
    }
    dialog(props).into_element()
}

pub fn open(handle: &AppHandle) {
    handle.update(|s| {
        s.open_unshelve_dialog();
        s.ui.dlg.repo_scope = s.multi.roots.iter().map(|root| root.id.clone()).collect();
    });
}

pub fn unshelve_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let open_handle = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("unshelve"))
        .child(
            text("Unshelve")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(button(
            ButtonProps::new("Unshelve…")
                .variant(ButtonVariant::Primary)
                .test_id(&test_ids::unshelve("open"))
                .on_press(move || open(&open_handle)),
        ))
        .child(
            text("Pick a shelf and bring its work back into the tree that owns it.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
}
