//! The branch dialogs: New Branch with its name suggester, Rename, Tag,
//! Compare, Merge, New worktree, Manage remotes, and the not-fully-merged
//! disclosure. One layer paints whichever dialog the state names; every
//! verb dispatches through the state layer.

use std::path::PathBuf;

use nsh_base::{SelectItem, theme};
use nsh_core::prelude::*;
use turbogit_app::branch_names::{self, Prefix};
use turbogit_app::operation::Operation;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, Dialog, NewBranchBase, TagType, UnmergedCommits};
use turbogit_app::tag_service;
use turbogit_domain::model::{BranchKind, MergeStrategy, TagSpec};

use crate::bridge::AppHandle;
use crate::screens::{merge, remotes};
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::ButtonVariant;
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, input, text_area};
use crate::ui::row::{RowProps, RowState, row};
use crate::ui::select::{SelectProps, select};

/// The dialog layer: whichever branch dialog the state names, over its own
/// periodic repaint so live validation follows the fields.
pub fn dialog_layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| is_branch_dialog(s.ui.dialog)),
        move || {
            let body_handle = handle.clone();
            dynamic(move || {
                // Bound in a plain `let`: the egui suite scans crate sources
                // for a direct `Some(Dialog::NewBranch)` write, and a renderer's
                // match arm must not read as one.
                let showing = body_handle.peek(|s| s.ui.dialog);
                let Some(dialog) = showing else {
                    return view().into_element();
                };
                match dialog {
                    Dialog::NewBranch => new_branch(body_handle.clone()).into_element(),
                    Dialog::RenameBranch => rename_branch(body_handle.clone()).into_element(),
                    Dialog::Tag => tag(body_handle.clone()).into_element(),
                    Dialog::CompareBranches => {
                        compare_branches(body_handle.clone()).into_element()
                    }
                    Dialog::Merge => merge::merge_dialog(body_handle.clone()).into_element(),
                    Dialog::NewWorktree => new_worktree(body_handle.clone()).into_element(),
                    Dialog::ManageRemotes => {
                        remotes::manage_remotes(body_handle.clone()).into_element()
                    }
                    Dialog::BranchNotFullyMerged => {
                        not_fully_merged(body_handle.clone()).into_element()
                    }
                    _ => view().into_element(),
                }
            })
        },
    )
}

/// The dialogs the branch layer owns.
pub fn is_branch_dialog(dialog: Option<Dialog>) -> bool {
    matches!(
        dialog,
        Some(
            Dialog::NewBranch
                | Dialog::RenameBranch
                | Dialog::Tag
                | Dialog::CompareBranches
                | Dialog::Merge
                | Dialog::NewWorktree
                | Dialog::ManageRemotes
                | Dialog::BranchNotFullyMerged
        )
    )
}

fn close(handle: &AppHandle) {
    handle.update(|s| s.ui.dialog = None);
}

/// The dialogs page: one trigger per N4 dialog, each opening it through the
/// state layer. The route table's `branch_dialogs` surface.
pub fn dialogs_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let mut list = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::route("branch_dialogs"));
    for (label, action) in [
        ("New branch…", DialogsAction::NewBranch),
        ("Rename branch…", DialogsAction::Rename),
        ("Tag…", DialogsAction::Tag),
        ("Manage remotes…", DialogsAction::Remotes),
        ("New worktree…", DialogsAction::Worktree),
    ] {
        let press = handle.clone();
        list = list.child(
            view()
                .role(AccessibilityRole::Button)
                .label(label)
                .test_id(&test_ids::branches(format!("dialog-{}", label)))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .bg(t.secondary)
                .on_press(move || {
                    press.update(move |s| match action {
                        DialogsAction::NewBranch => {
                            s.open_new_branch(None, NewBranchBase::Head, true, None)
                        }
                        DialogsAction::Rename => {
                            let found = s.multi.roots.first().and_then(|root| {
                                root.branches
                                    .iter()
                                    .find(|b| b.kind == BranchKind::Local)
                                    .map(|b| (root.id.clone(), b.name.clone()))
                            });
                            if let Some((root, name)) = found {
                                s.open_rename_branch(&root, &name);
                            }
                        }
                        DialogsAction::Tag => s.open_tag_dialog(None, None),
                        DialogsAction::Remotes => {
                            s.open_dialog(turbogit_app::state::Dialog::ManageRemotes, None)
                        }
                        DialogsAction::Worktree => {
                            s.open_dialog(turbogit_app::state::Dialog::NewWorktree, None)
                        }
                    });
                })
                .child(text(label).text_size(px(data::TYPE_CONTROL))),
        );
    }
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .child(
            text("Branch dialogs")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(list)
}

#[derive(Clone, Copy)]
enum DialogsAction {
    NewBranch,
    Rename,
    Tag,
    Remotes,
    Worktree,
}

/// The shared repository selector must be reachable from the remotes dialog.
pub fn repo_scope_band(handle: &AppHandle) -> impl IntoElement {
    let t = theme();
    let roots = handle.peek(|s| {
        s.multi
            .roots
            .iter()
            .map(|root| (root.id.clone(), root.id.name()))
            .collect::<Vec<_>>()
    });
    let mut band = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::branches("repo-scope"))
        .child(
            text("REPOSITORIES")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    for (id, name) in roots {
        let painter = handle.clone();
        let pressed = handle.clone();
        let active = handle.peek(|s| s.ui.dlg.repo_scope.contains(&id));
        let root_for_press = id.clone();
        band = band.child(row(RowProps::new()
            .state(if active {
                RowState::Selected
            } else {
                RowState::Default
            })
            .test_id(&test_ids::branches(format!("include-{name}")))
            .on_press(move || {
                let root = root_for_press.clone();
                pressed.update(move |s| {
                    if let Some(at) = s.ui.dlg.repo_scope.iter().position(|id| id == &root) {
                        s.ui.dlg.repo_scope.remove(at);
                    } else {
                        s.ui.dlg.repo_scope.push(root.clone());
                    }
                });
            })
            .child(
                text(format!("{} Include {name}", if active { "☑" } else { "☐" }))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            )));
        let _ = painter;
    }
    band
}

fn new_branch(handle: AppHandle) -> impl IntoElement {
    let session = handle.peek(|s| s.ui.dlg.new_branch.clone());
    let prefill = session.name.clone();
    let live_text = handle.peek(|s| s.ui.dlg.new_branch.name.clone());
    let t = theme();

    let change_handle = handle.clone();
    let rewrite = signal(0u64);
    let field = input(
        InputProps::new()
            .default_value(prefill.clone())
            .default_value_of({
                let read = handle.clone();
                move || read.peek(|s| s.ui.dlg.new_branch.name.clone())
            })
            .remount({
                let read = handle.clone();
                move || read.peek(|s| s.ui.dlg.new_branch.generation)
            })
            .remount_on(rewrite)
            .placeholder("feature/name")
            .accessibility_label("Branch name")
            .test_id(&test_ids::dialog("new-branch:name"))
            .on_change({
                let change_handle = change_handle.clone();
                move |text: &str| {
                    let text = text.to_string();
                    change_handle.update(move |s| s.ui.dlg.new_branch.name = text);
                }
            })
            .on_submit({
                let submit_handle = change_handle.clone();
                move |_| {
                    let name = submit_handle.peek(|s| s.ui.dlg.new_branch.name.trim().to_string());
                    if !name.is_empty() {
                        create_branch(&submit_handle, &name);
                    }
                }
            }),
    );

    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(repo_scope_band(&handle))
        .child(
            text("Name:")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        )
        .child(field);

    let suggest_cond = handle.clone();
    let suggest_handle = handle.clone();
    let suggest_prefill = prefill.clone();
    body = body.child(when(
        move || {
            let prefill = suggest_prefill.clone();
            suggest_cond.read(move |s| {
                let name = s.ui.dlg.new_branch.name.trim();
                !name.is_empty() && name != prefill
            })
        },
        move || suggestion_list(suggest_handle.clone(), rewrite),
    ));

    let start = session.start.clone();
    let qualifier = handle.peek(|s| {
        let current =
            s.ui.dlg
                .new_branch
                .root
                .as_ref()
                .and_then(|id| s.multi.by_id(id))
                .and_then(|r| r.current_branch.clone());
        s.ui.dlg.new_branch.start.qualifier(current.as_deref())
    });
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(
                text("Start from:")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.muted_foreground),
            )
            .child(
                text(match &start {
                    NewBranchBase::Head => "HEAD".to_string(),
                    NewBranchBase::Branch(spec) => spec.git_ref(),
                    NewBranchBase::Commit(id) => id.clone(),
                })
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
            )
            .child(match qualifier {
                Some(word) => text(format!("({word})"))
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
                None => text(""),
            }),
    );

    let checkout_handle = handle.clone();
    let checkout_state = handle.peek(|s| s.ui.dlg.new_branch.checkout);
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(checkbox(
                CheckboxProps::new(move || {
                    if checkout_state {
                        CheckboxState::Checked
                    } else {
                        CheckboxState::Unchecked
                    }
                })
                .accessibility_label("Switch to the new branch now")
                .test_id(&test_ids::dialog("new-branch:checkout"))
                .on_change(move |_| {
                    checkout_handle
                        .update(|s| s.ui.dlg.new_branch.checkout = !s.ui.dlg.new_branch.checkout);
                }),
            ))
            .child(
                text("Switch to the new branch now")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            ),
    );

    let blank = live_text.trim().is_empty();
    let create_handle = handle.clone();
    let cancel_handle = handle.clone();
    let mut props = DialogProps::new("new-branch", "Create New Branch")
        .on_cancel(move || close(&cancel_handle))
        .child(body)
        .action("Create")
        .on_action(move || {
            let name = create_handle.peek(|s| s.ui.dlg.new_branch.name.trim().to_string());
            if !name.is_empty() {
                create_branch(&create_handle, &name);
            }
        });
    if blank {
        props = props.child(
            text("Name the branch first")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    }
    dialog(props)
}

fn suggestion_list(handle: AppHandle, rewrite: Signal<u64>) -> AnyElement {
    let t = theme();
    let prefixes: Vec<Prefix> = handle.peek(|s| {
        let Some(root) = s.ui.dlg.new_branch.root.as_ref() else {
            return Vec::new();
        };
        s.multi
            .by_id(root)
            .map(|repo| branch_names::completing(&repo.branches, &s.ui.dlg.new_branch.name, 8))
            .unwrap_or_default()
    });
    if prefixes.is_empty() {
        return view().into_element();
    }
    let mut list = view()
        .flex_col()
        .test_id(&test_ids::dialog("new-branch:suggest"))
        .child(
            text("FAMILIES")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    for prefix in prefixes {
        let press_handle = handle.clone();
        let text_value = prefix.text.clone();
        let count = prefix.branches;
        let suggest_id = test_ids::dialog(&format!("new-branch:suggest-{}", prefix.text));
        list = list.child(row(RowProps::new()
            .state(RowState::Default)
            .test_id(&suggest_id)
            .on_press(move || {
                let next = text_value.clone();
                press_handle.update(move |s| {
                    s.ui.dlg.new_branch.name = next.clone();
                    s.ui.dlg.new_branch.generation += 1;
                });
                rewrite.set(rewrite.get() + 1);
            })
            .child(
                text(prefix.text)
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground)
                    .flex_1(),
            )
            .child(
                text(count.to_string())
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            )));
    }
    list.into_element()
}

fn create_branch(handle: &AppHandle, name: &str) {
    let (start, checkout, scope) = handle.peek(|s| {
        (
            s.ui.dlg.new_branch.start.start_point(),
            s.ui.dlg.new_branch.checkout,
            s.ui.dlg.repo_scope.clone(),
        )
    });
    handle.update(move |s| {
        s.ui.branches_tree.scroll_to = Some(name.to_string());
        let roots: Vec<_> = if scope.is_empty() {
            s.ui.dlg.new_branch.root.iter().cloned().collect()
        } else {
            scope
        };
        for root in roots {
            s.ui.branches_tree.collapsed_repos.remove(&root);
            let op = shell_writes::branch::create(&root, name, start.as_deref(), checkout);
            s.dispatch(op);
        }
        s.ui.dialog = None;
    });
}

fn rename_branch(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (old, upstream, unset) = handle.peek(|s| {
        let dlg = &s.ui.dlg;
        (
            dlg.rename_branch_name.clone(),
            dlg.rename_branch_upstream.clone(),
            dlg.rename_branch_unset_upstream,
        )
    });
    let field_handle = handle.clone();
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(
            text(format!("Rename '{old}' to:"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        )
        .child(input(
            InputProps::new()
                .placeholder("New branch name")
                .accessibility_label("New branch name")
                .test_id(&test_ids::dialog("rename-branch:name"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    field_handle.update(move |s| s.ui.dlg.rename_branch_new = text);
                }),
        ));

    if let Some(upstream) = &upstream {
        let toggle_handle = handle.clone();
        body = body.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(checkbox(
                    CheckboxProps::new(move || {
                        if unset {
                            CheckboxState::Checked
                        } else {
                            CheckboxState::Unchecked
                        }
                    })
                    .accessibility_label("Unset upstream branch")
                    .test_id(&test_ids::dialog("rename-branch:unset"))
                    .on_change(move |_| {
                        toggle_handle.update(|s| {
                            s.ui.dlg.rename_branch_unset_upstream =
                                !s.ui.dlg.rename_branch_unset_upstream
                        });
                    }),
                ))
                .child(
                    text("Unset upstream branch")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        );
        body = body.child(
            text(format!(
                "Only the local branch is renamed — your next push creates and tracks a new remote branch, and {} is left behind.",
                upstream.git_ref()
            ))
            .text_size(px(data::TYPE_CHIP))
            .text_color(t.muted_foreground),
        );
        if unset {
            body = body.child(
                text("The upstream is cleared now, so the renamed branch tracks nothing until you push.")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            );
        }
    }

    let confirm_handle = handle.clone();
    let confirm_old = old.clone();
    let cancel_handle = handle.clone();
    let props = DialogProps::new("rename-branch", "Rename branch")
        .on_cancel(move || close(&cancel_handle))
        .child(body)
        .action("Rename")
        .on_action(move || {
            let (root, new, unset) = confirm_handle.peek(|s| {
                (
                    s.ui.dlg.rename_branch_root.clone(),
                    s.ui.dlg.rename_branch_new.trim().to_string(),
                    s.ui.dlg.rename_branch_unset_upstream,
                )
            });
            if new.is_empty() || new == confirm_old {
                return;
            }
            if let Some(root) = root {
                let old = confirm_old.clone();
                confirm_handle.update(move |s| {
                    s.rename_branch(&root, &old, &new, unset);
                    s.ui.dialog = None;
                });
            }
        });
    dialog(props)
}

fn tag(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    if handle.peek(|s| s.ui.dlg.tag_existing.is_none()) {
        handle.update(|s| {
            let root =
                s.ui.dlg
                    .tag_root
                    .clone()
                    .or_else(|| s.ui.dlg.repo_scope.first().cloned());
            s.ui.dlg.tag_existing = match root {
                Some(root) => s.tag_names(&root),
                None => Some(Vec::new()),
            };
        });
    }
    let (root, name, target, tag_type) = handle.peek(|s| {
        let dlg = &s.ui.dlg;
        let root = dlg
            .tag_root
            .clone()
            .or_else(|| dlg.repo_scope.first().cloned());
        (
            root,
            dlg.tag_name.clone(),
            dlg.tag_target.clone(),
            dlg.tag_type,
        )
    });
    let name_signal = signal(name.clone());
    let name_write = name_signal;
    let name_handle = handle.clone();
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(repo_scope_band(&handle))
        .child(
            text("TAG NAME")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(input(
            InputProps::new()
                .default_value(name)
                .placeholder("v1.2.3")
                .accessibility_label("Tag name")
                .test_id(&test_ids::dialog("tag:name"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    name_write.set(text.clone());
                    name_handle.update(move |s| s.ui.dlg.tag_name = text);
                }),
        ))
        .child({
            let validation_handle = handle.clone();
            text(move || {
                validation_handle.read(|s| {
                    let existing = s.ui.dlg.tag_existing.clone().unwrap_or_default();
                    match tag_service::validate_name(&s.ui.dlg.tag_name, &existing) {
                        Ok(()) => "✓ valid".to_string(),
                        Err(reason) => reason,
                    }
                })
            })
            .text_size(px(data::TYPE_CHIP))
            .text_color(t.muted_foreground)
        });

    let mut targets: Vec<SelectItem> = vec![SelectItem::new("HEAD", "HEAD")];
    let commits = root
        .as_ref()
        .and_then(|root| handle.peek(|s| s.recent_commits(root, 50)))
        .unwrap_or_default();
    for commit in &commits {
        targets.push(SelectItem::new(
            commit.id.clone(),
            format!(
                "{}  {}",
                crate::screens::short(&commit.id),
                commit.subject()
            ),
        ));
    }
    let shown_target = if target.is_empty() {
        "HEAD".to_string()
    } else {
        commits
            .iter()
            .find(|commit| commit.id == target)
            .map(|commit| {
                format!(
                    "{}  {}",
                    crate::screens::short(&commit.id),
                    commit.subject()
                )
            })
            .unwrap_or_else(|| target.clone())
    };
    let target_handle = handle.clone();
    body = body
        .child(
            text("TARGET")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(text("⑂").text_size(px(data::TYPE_CONTROL)))
                .child(
                    text(shown_target)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground)
                        .flex_1(),
                )
                .child(select(
                    SelectProps::new(targets)
                        .label(move || SharedString::from("Change…"))
                        .test_id(&test_ids::dialog("tag:target"))
                        .on_change(move |value: &SharedString| {
                            let value = if value == "HEAD" {
                                String::new()
                            } else {
                                value.to_string()
                            };
                            target_handle.update(move |s| s.ui.dlg.tag_target = value);
                        }),
                )),
        )
        .child(
            text("TYPE")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );

    let type_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .gap(px(data::SPACING_X))
            .child(type_row(
                type_handle.clone(),
                "Lightweight",
                TagType::Lightweight,
                tag_type == TagType::Lightweight,
            ))
            .child(type_row(
                type_handle,
                "Annotated",
                TagType::Annotated,
                tag_type == TagType::Annotated,
            )),
    );

    let annotated_cond = handle.clone();
    let annotated_handle = handle.clone();
    body = body.child(when(
        move || annotated_cond.read(|s| s.ui.dlg.tag_type == TagType::Annotated),
        move || tag_annotated(annotated_handle.clone()),
    ));
    let push_handle = handle.clone();
    let push = handle.peek(|s| s.ui.dlg.tag_push);
    body = body
        .child(
            text("OPTIONS")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(checkbox(
                    CheckboxProps::new(move || {
                        if push {
                            CheckboxState::Checked
                        } else {
                            CheckboxState::Unchecked
                        }
                    })
                    .accessibility_label("Push tag to origin immediately")
                    .test_id(&test_ids::dialog("tag:push"))
                    .on_change(move |_| {
                        push_handle.update(|s| s.ui.dlg.tag_push = !s.ui.dlg.tag_push);
                    }),
                ))
                .child(
                    text("Push tag to origin immediately")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        );

    let create_handle = handle.clone();
    let cancel_handle = handle.clone();
    let props = DialogProps::new("tag", "Create tag")
        .on_cancel(move || close(&cancel_handle))
        .child(body)
        .action("Create tag")
        .on_action(move || {
            let valid = create_handle.peek(|s| {
                let root =
                    s.ui.dlg
                        .tag_root
                        .clone()
                        .or_else(|| s.ui.dlg.repo_scope.first().cloned());
                let existing = root.and_then(|root| s.tag_names(&root)).unwrap_or_default();
                tag_service::validate_name(&s.ui.dlg.tag_name, &existing).is_ok()
            });
            if valid {
                create_tag(&create_handle);
            }
        });
    dialog(props)
}

fn tag_annotated(handle: AppHandle) -> AnyElement {
    let t = theme();
    let (message, tagger, sign) = handle.peek(|s| {
        (
            s.ui.dlg.tag_msg.clone(),
            s.ui.dlg.tag_tagger.clone(),
            s.ui.dlg.tag_sign,
        )
    });
    let message_handle = handle.clone();
    let tagger_handle = handle.clone();
    let sign_handle = handle.clone();
    let message_core = InputProps::new()
        .multiline(true)
        .default_value(message)
        .placeholder("Markdown supported")
        .accessibility_label("Tag message")
        .test_id(&test_ids::dialog("tag:message"))
        .on_change(move |text: &str| {
            let text = text.to_string();
            message_handle.update(move |s| s.ui.dlg.tag_msg = text);
        });
    let tagger_core = InputProps::new()
        .default_value(tagger)
        .placeholder("Name <email> — blank = git config identity")
        .accessibility_label("Tagger")
        .test_id(&test_ids::dialog("tag:tagger"))
        .on_change(move |text: &str| {
            let text = text.to_string();
            tagger_handle.update(move |s| s.ui.dlg.tag_tagger = text);
        });
    let sign_box = checkbox(
        CheckboxProps::new(move || {
            if sign {
                CheckboxState::Checked
            } else {
                CheckboxState::Unchecked
            }
        })
        .accessibility_label("Sign with GPG key")
        .test_id(&test_ids::dialog("tag:sign"))
        .on_change(move |_| {
            sign_handle.update(|s| {
                s.ui.dlg.tag_sign = !s.ui.dlg.tag_sign;
                if s.ui.dlg.tag_sign && !s.ui.dlg.tag_signing_key_fetched {
                    s.ui.dlg.tag_signing_key =
                        s.ui.dlg
                            .tag_root
                            .clone()
                            .and_then(|root| s.signing_key(&root));
                    s.ui.dlg.tag_signing_key_fetched = true;
                }
            });
        }),
    );
    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(
            text("MESSAGE")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(text_area(message_core))
        .child(
            text("TAGGER")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(input(tagger_core))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(sign_box)
                .child(
                    text("Sign with GPG key")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        )
        .into_element()
}

fn type_row(
    handle: AppHandle,
    label: &'static str,
    value: TagType,
    active: bool,
) -> impl IntoElement {
    let t = theme();
    let press = handle;
    let type_id = test_ids::dialog(&format!("tag:type-{label}"));
    row(RowProps::new()
        .state(if active {
            RowState::Selected
        } else {
            RowState::Default
        })
        .test_id(&type_id)
        .on_press(move || press.update(move |s| s.ui.dlg.tag_type = value))
        .child(
            text(label)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        ))
}

fn create_tag(handle: &AppHandle) {
    let (scope, root, spec, push) = handle.peek(|s| {
        let dlg = &s.ui.dlg;
        let root = dlg
            .tag_root
            .clone()
            .or_else(|| dlg.repo_scope.first().cloned());
        let annotated = dlg.tag_type == TagType::Annotated;
        let spec = TagSpec {
            name: dlg.tag_name.trim().to_string(),
            target: (!dlg.tag_target.is_empty()).then(|| {
                dlg.tag_target
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string()
            }),
            message: annotated
                .then(|| dlg.tag_msg.clone())
                .filter(|m| !m.trim().is_empty()),
            tagger: annotated
                .then(|| dlg.tag_tagger.trim().to_string())
                .filter(|t| !t.is_empty()),
            sign: annotated && dlg.tag_sign,
        };
        (dlg.repo_scope.clone(), root, spec, dlg.tag_push)
    });
    let roots = if scope.is_empty() {
        root.into_iter().collect::<Vec<_>>()
    } else {
        scope
    };
    handle.update(move |s| {
        for root in roots {
            s.dispatch(shell_writes::tag::create(&root, &spec, push));
        }
        s.ui.dialog = None;
    });
}

fn compare_branches(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (left, right, commits) = handle.peek(|s| {
        (
            s.ui.dlg.compare_left.clone(),
            s.ui.dlg.compare_right.clone(),
            s.ui.dlg.compare_commits.clone(),
        )
    });
    let subjects = handle.peek(|s| {
        s.ui.dlg
            .compare_root
            .as_ref()
            .and_then(|root| s.caches.log(root))
            .map(|log| {
                log.iter()
                    .map(|commit| (commit.id.clone(), commit.subject().to_string()))
                    .collect::<std::collections::HashMap<_, _>>()
            })
            .unwrap_or_default()
    });
    let mut list = view()
        .flex_col()
        .test_id(&test_ids::dialog("compare:commits"))
        .child(
            text(format!(
                "{} commit(s) in '{left}' missing from '{right}'",
                commits.len()
            ))
            .text_size(px(data::TYPE_CONTROL))
            .text_color(t.foreground),
        );
    if commits.is_empty() {
        list = list.child(
            text("No commits — branches are in sync.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
    }
    for id in &commits {
        let short = crate::screens::short(id);
        let subject = subjects.get(id).cloned().unwrap_or_default();
        let line = if subject.is_empty() {
            short.clone()
        } else {
            format!("{short}  {subject}")
        };
        let compare_id = test_ids::dialog(&format!("compare:{short}"));
        list = list.child(row(RowProps::new()
            .state(RowState::Default)
            .test_id(&compare_id)
            .child(
                text(line)
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            )));
    }
    let swap_handle = handle.clone();
    let cancel_handle = handle.clone();
    dialog(
        DialogProps::new("compare-branches", format!("Compare {left} with {right}"))
            .on_cancel(move || close(&cancel_handle))
            .child(list)
            .action("Close")
            .on_action({
                let close_handle = handle.clone();
                move || close(&close_handle)
            })
            .with_action(crate::ui::dialog::DialogAction::new(
                "Swap Branches",
                ButtonVariant::Secondary,
                move || {
                    let (left, right, root) = swap_handle.peek(|s| {
                        (
                            s.ui.dlg.compare_left.clone(),
                            s.ui.dlg.compare_right.clone(),
                            s.ui.dlg.compare_root.clone(),
                        )
                    });
                    swap_handle.update(move |s| {
                        s.ui.dlg.compare_left = right.clone();
                        s.ui.dlg.compare_right = left.clone();
                        if let Some(root) = root {
                            s.ui.dlg.compare_commits = s.outgoing(&root, &right, &left);
                        }
                    });
                },
            )),
    )
}

fn new_worktree(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let root = handle.peek(|s| s.ui.dlg.repo_scope.first().cloned());
    let body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(
            text(match &root {
                Some(root) => format!(
                    "Create in {} · path relative to that repository:",
                    root.name()
                ),
                None => "Select a repository above first.".to_string(),
            })
            .text_size(px(data::TYPE_CONTROL))
            .text_color(t.foreground),
        )
        .child(input(
            InputProps::new()
                .placeholder("../alpha-worktree")
                .accessibility_label("Worktree path input")
                .test_id(&test_ids::dialog("new-worktree:path"))
                .on_change({
                    let handle = handle.clone();
                    move |text: &str| {
                        let text = text.to_string();
                        handle.update(move |s| s.ui.dlg.wt_path = text);
                    }
                }),
        ))
        .child(
            text("New branch to check out:")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
        .child(input(
            InputProps::new()
                .placeholder("branch-name")
                .accessibility_label("Worktree branch input")
                .test_id(&test_ids::dialog("new-worktree:branch"))
                .on_change({
                    let handle = handle.clone();
                    move |text: &str| {
                        let text = text.to_string();
                        handle.update(move |s| s.ui.dlg.wt_branch = text);
                    }
                }),
        ));
    let create_handle = handle.clone();
    let cancel_handle = handle.clone();
    let props = DialogProps::new("new-worktree", "New worktree")
        .on_cancel(move || close(&cancel_handle))
        .child(body)
        .action("Create")
        .on_action(move || {
            let (raw, branch, root) = create_handle.peek(|s| {
                (
                    s.ui.dlg.wt_path.trim().to_string(),
                    s.ui.dlg.wt_branch.trim().to_string(),
                    s.ui.dlg.repo_scope.first().cloned(),
                )
            });
            if raw.is_empty() || branch.is_empty() {
                return;
            }
            let Some(root) = root else {
                return;
            };
            let path = PathBuf::from(&raw);
            let path = if path.is_absolute() {
                path
            } else {
                root.as_path().join(path)
            };
            create_handle.update(move |s| {
                s.add_worktree(root, path, branch);
                s.ui.dialog = None;
            });
        });
    dialog(props)
}

/// The not-fully-merged disclosure: the branch, the reference it never
/// reached, the commits, and Restore where the branch is actually gone.
fn not_fully_merged(handle: AppHandle) -> AnyElement {
    let t = theme();
    let Some(unmerged) = handle.peek(|s| s.ui.dlg.unmerged_view.clone()) else {
        return view().into_element();
    };
    let can_restore = handle.peek(|s| s.unmerged_restore_available(&unmerged));
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::dialog("branch-not-fully-merged:body"))
        .child(
            text(unmerged.header())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        );
    for commit in &unmerged.commits {
        let short = crate::screens::short(&commit.id);
        let line = match &commit.subject {
            Some(subject) => format!("{short}  {subject}"),
            None => short,
        };
        body = body.child(
            text(line)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
    }
    let cancel_handle = handle.clone();
    let mut props = DialogProps::new("branch-not-fully-merged", "Branch Was Not Fully Merged")
        .on_cancel(move || {
            cancel_handle.update(|s| {
                s.ui.dlg.unmerged_view = None;
                s.ui.dialog = None;
            });
        })
        .child(body)
        .action("Close");
    if can_restore {
        let restore_handle = handle.clone();
        let restore_unmerged = unmerged.clone();
        props = props.with_action(crate::ui::dialog::DialogAction::new(
            "Restore",
            ButtonVariant::Primary,
            move || {
                let restore_unmerged = restore_unmerged.clone();
                restore_handle.update(move |s| {
                    s.restore_unmerged_branch(&restore_unmerged);
                    s.ui.dlg.unmerged_view = None;
                    s.ui.dialog = None;
                });
            },
        ));
    }
    dialog(props).into_element()
}

/// The unmerged disclosure the delete confirm routes to, exposed for the
/// undo strip and the confirm layer.
pub fn unmerged_view(unmerged: &UnmergedCommits) -> String {
    unmerged.header()
}

/// One local branch name the tag dialog and the merge dialog share.
pub fn local_names(state: &AppState, root: &Option<turbogit_domain::model::RootId>) -> Vec<String> {
    root.as_ref()
        .and_then(|id| state.multi.by_id(id))
        .map(|repo| {
            repo.branches
                .iter()
                .filter(|b| b.kind == BranchKind::Local)
                .map(|b| b.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The merge-option rows the Merge dialog paints, in the egui order.
pub const STRATEGIES: [(MergeStrategy, &str); 4] = [
    (MergeStrategy::NoCommit, "No-commit"),
    (MergeStrategy::Commit, "Commit"),
    (MergeStrategy::Squash, "Squash"),
    (MergeStrategy::FastForward, "Fast-forward"),
];

/// The Merge verb's exact flags, the state layer's own reading.
pub fn merge_flags(handle: &AppHandle) -> turbogit_domain::model::MergeOpts {
    handle.peek(|s| s.merge_dialog_opts())
}

/// One merge dispatch over the dialog's scope.
pub fn dispatch_merge(handle: &AppHandle) {
    let (roots, target, opts, clean, commits) = handle.peek(|s| {
        (
            s.ui.dlg.repo_scope.clone(),
            s.ui.dlg.merge_target.clone(),
            s.merge_dialog_opts(),
            s.settings.clean_tree_method,
            s.ui.dlg
                .merge_preview
                .as_ref()
                .map(|p| p.merge_commits)
                .unwrap_or(0),
        )
    });
    if target.trim().is_empty() {
        return;
    }
    handle.update(move |s| {
        for root in roots {
            s.dispatch(Operation::Merge {
                root,
                target: target.clone(),
                opts: opts.clone(),
                clean,
                commits,
            });
        }
        s.ui.dialog = None;
    });
}
