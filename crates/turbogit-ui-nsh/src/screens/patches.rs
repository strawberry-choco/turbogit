//! Patches: applying one from a file or the clipboard, creating one from
//! local changes or a revision, importing one to the shelf, and the
//! outcome each route reports.

use nsh_base::{SelectItem, theme};
use nsh_core::prelude::*;
use turbogit_app::apply_patch;
use turbogit_app::create_patch;
use turbogit_app::patch_apply::{ApplyOutcome, Preflight};
use turbogit_app::state::{AppState, Dialog, PatchSource};
use turbogit_domain::model::{PatchEncoding, RootId};

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, input, text_area};
use crate::ui::select::{SelectProps, select};

const APPLY_VERB: &str = "Apply patch";
const SETTINGS_TITLE: &str = "Patch File Settings";
const COMMENT_PROMPT: &str = "Enter a commit comment (it will be used as the patch file name)";

pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || {
            show.read(|s| {
                matches!(
                    s.ui.dialog,
                    Some(
                        Dialog::ApplyPatch
                            | Dialog::CreatePatchComment
                            | Dialog::PatchFileSettings
                            | Dialog::ImportPatches
                    )
                )
            })
        },
        move || {
            let body = handle.clone();
            dynamic(move || {
                body.read(|_| ());
                match body.peek(|s| s.ui.dialog) {
                    Some(Dialog::ApplyPatch) => apply_dialog(body.clone()).into_element(),
                    Some(Dialog::CreatePatchComment) => comment_dialog(body.clone()).into_element(),
                    Some(Dialog::PatchFileSettings) => settings_dialog(body.clone()).into_element(),
                    Some(Dialog::ImportPatches) => import_dialog(body.clone()).into_element(),
                    _ => view().into_element(),
                }
            })
        },
    )
}

fn close(handle: &AppHandle) {
    handle.update(|s| {
        s.ui.dialog = None;
        s.ui.dlg.create_patch_origin = None;
    });
}

fn roots(handle: &AppHandle) -> Vec<(RootId, String)> {
    handle.peek(|s| {
        s.multi
            .roots
            .iter()
            .map(|r| (r.id.clone(), r.id.name()))
            .collect()
    })
}

fn scope_select(
    handle: &AppHandle,
    chosen: Option<&RootId>,
    test_id: &str,
    set_root: impl Fn(&mut AppState, &RootId) + Copy + 'static,
) -> impl IntoElement {
    let items: Vec<SelectItem> = roots(handle)
        .into_iter()
        .map(|(_, name)| SelectItem::new(name.clone(), name))
        .collect();
    let shown = chosen
        .map(|root| root.name())
        .unwrap_or_else(|| "Select repository".to_string());
    let press = handle.clone();
    let shared = std::rc::Rc::new(set_root);
    view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .child(
            text(shown)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(theme().foreground)
                .flex_1(),
        )
        .child(select(
            SelectProps::new(items)
                .label(move || SharedString::from("Change…"))
                .test_id(test_id)
                .on_change(move |value: &SharedString| {
                    let name = value.to_string();
                    let set_root = shared.clone();
                    press.update(move |s| {
                        if let Some(id) = s
                            .multi
                            .roots
                            .iter()
                            .find(|r| r.id.name() == name)
                            .map(|r| r.id.clone())
                        {
                            set_root(s, &id);
                        }
                    });
                }),
        ))
}

fn files_the_patch_names(patch: &turbogit_domain::model::Patch) -> impl IntoElement {
    let t = theme();
    let mut list = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::patch("files"))
        .child(
            text(format!("FILES THE PATCH NAMES · {}", patch.files.len()))
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    for file in &patch.files {
        let path = if file.new_path.is_empty() {
            file.old_path.clone()
        } else {
            file.new_path.clone()
        };
        list = list.child(
            text(path)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        );
    }
    list
}

fn apply_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (
        root,
        source,
        path,
        patch_text,
        author,
        message,
        patch,
        provenance,
        error,
        preflight,
        outcome,
    ) = handle.peek(|s| {
        let d = &s.ui.dlg;
        (
            d.apply_patch_root.clone(),
            d.apply_patch_source,
            d.apply_patch_path.clone(),
            d.apply_patch_text.clone(),
            d.apply_patch_author.clone(),
            d.apply_patch_message.clone(),
            d.apply_patch_patch.clone(),
            d.apply_patch_provenance.clone(),
            d.apply_patch_error.clone(),
            d.apply_patch_preflight.clone(),
            d.apply_patch_outcome.clone(),
        )
    });
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::patch("apply-body"))
        .child(
            text("PATCH SOURCE")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(scope_select(
            &handle,
            root.as_ref(),
            &test_ids::patch("root"),
            apply_patch::set_root,
        ));
    let file_handle = handle.clone();
    let clipboard_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .gap(px(data::SPACING_X))
            .child(button(
                ButtonProps::new("From file")
                    .variant(if source == PatchSource::File {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .test_id(&test_ids::patch("route-file"))
                    .on_press(move || {
                        file_handle.update(|s| apply_patch::set_source(s, PatchSource::File))
                    }),
            ))
            .child(button(
                ButtonProps::new("From clipboard")
                    .variant(if source == PatchSource::Clipboard {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .test_id(&test_ids::patch("route-clipboard"))
                    .on_press(move || {
                        clipboard_handle
                            .update(|s| apply_patch::set_source(s, PatchSource::Clipboard))
                    }),
            )),
    );
    if source == PatchSource::File {
        let change = handle.clone();
        let choose = handle.clone();
        body = body.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text("Path")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                )
                .child(input(
                    InputProps::new()
                        .default_value(path)
                        .placeholder("/path/to/change.patch")
                        .accessibility_label("Patch file path")
                        .test_id(&test_ids::patch("path"))
                        .on_change(move |text: &str| {
                            let text = text.to_string();
                            change.update(move |s| {
                                s.ui.dlg.apply_patch_path = text;
                                apply_patch::sync(s);
                            });
                        }),
                ))
                .child(button(
                    ButtonProps::new("Choose file…")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::patch("choose-file"))
                        .on_press(move || choose.update(apply_patch::choose_file)),
                )),
        );
    } else {
        let change = handle.clone();
        body = body
            .child(
                text("Paste the patch below — no file is read.")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            )
            .child(text_area(
                InputProps::new()
                    .multiline(true)
                    .default_value(patch_text)
                    .accessibility_label("Patch text")
                    .test_id(&test_ids::patch("clipboard-text"))
                    .on_change(move |text: &str| {
                        let text = text.to_string();
                        change.update(move |s| {
                            s.ui.dlg.apply_patch_text = text;
                            apply_patch::sync(s);
                        });
                    }),
            ));
    }
    if let Some(message) = error {
        body = body.child(
            text(message)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::STATE_ERROR)
                .test_id(&test_ids::patch("error")),
        );
    }
    if let Some(patch) = &patch {
        body = body.child(
            text("PATCH PROVENANCE")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
        if provenance.is_none() {
            body = body.child(
                text("This patch carries no commit header, so there is nothing to fill.")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            );
        } else {
            let author_change = handle.clone();
            let message_change = handle.clone();
            body = body
                .child(
                    text(format!("Patch author: {author}"))
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                )
                .child(
                    text(format!("Patch message: {message}"))
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(data::CELL_GAP))
                        .child(
                            text("Author")
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(t.foreground),
                        )
                        .child(input(
                            InputProps::new()
                                .default_value(author)
                                .accessibility_label("Patch author")
                                .test_id(&test_ids::patch("author"))
                                .on_change(move |text: &str| {
                                    let text = text.to_string();
                                    author_change
                                        .update(move |s| s.ui.dlg.apply_patch_author = text);
                                }),
                        )),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(data::CELL_GAP))
                        .child(
                            text("Message")
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(t.foreground),
                        )
                        .child(input(
                            InputProps::new()
                                .default_value(message)
                                .accessibility_label("Patch message")
                                .test_id(&test_ids::patch("message"))
                                .on_change(move |text: &str| {
                                    let text = text.to_string();
                                    message_change
                                        .update(move |s| s.ui.dlg.apply_patch_message = text);
                                }),
                        )),
                );
        }
        body = body.child(files_the_patch_names(patch));
    }
    if let Some(outcome) = outcome {
        body = body.child(outcome_panel(handle.clone(), outcome));
    } else if let Some(preflight) = preflight {
        body = body.child(preflight_panel(preflight));
    }
    let loaded = patch.is_some();
    let show_diff = handle.clone();
    let import = handle.clone();
    let apply = handle.clone();
    let cancel = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(button(
                ButtonProps::new("Show Diff")
                    .variant(ButtonVariant::Secondary)
                    .disabled(!loaded)
                    .test_id(&test_ids::patch("show-diff"))
                    .on_press(move || show_diff.update(apply_patch::preflight)),
            ))
            .child(button(
                ButtonProps::new("Import to shelf")
                    .variant(ButtonVariant::Secondary)
                    .disabled(!loaded)
                    .test_id(&test_ids::patch("import-to-shelf"))
                    .on_press(move || import.update(|s| s.import_patch_from_apply_window())),
            ))
            .child(button(
                ButtonProps::new(APPLY_VERB)
                    .variant(ButtonVariant::Primary)
                    .disabled(!loaded)
                    .test_id(&test_ids::patch("apply"))
                    .on_press(move || apply.update(apply_patch::dispatch)),
            ))
            .child(button(
                ButtonProps::new("Close")
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::patch("close"))
                    .on_press(move || close(&cancel)),
            )),
    );
    let title = format!(
        "{APPLY_VERB} · {}",
        root.as_ref().map(|r| r.name()).unwrap_or_default()
    );
    dialog(
        DialogProps::new("apply-patch", title)
            .width(700.0)
            .on_cancel(move || close(&handle))
            .child(body),
    )
    .into_element()
}

fn preflight_panel(preflight: Preflight) -> impl IntoElement {
    let t = theme();
    let mut panel = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::patch("preflight"))
        .child(
            text("SHOW DIFF")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    match preflight {
        Preflight::Clean { files } => {
            panel = panel.child(
                text(format!(
                    "Applies cleanly — {files} file section(s) would be written."
                ))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::STATE_SUCCESS),
            );
        }
        Preflight::Refused { report } => {
            panel = panel.child(
                text(format!("Nothing to check — {report}"))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(data::STATE_WARNING),
            );
        }
        Preflight::Conflicts { report, paths } => {
            panel = panel.child(
                text("The patch does not apply cleanly. No file has been changed.")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(data::STATE_ERROR),
            );
            for line in report.lines().filter(|line| !line.trim().is_empty()) {
                let conflicts = paths
                    .iter()
                    .any(|p| line.contains(&p.to_string_lossy().into_owned()));
                panel = panel.child(
                    text(line.to_string())
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(if conflicts {
                            data::STATE_ERROR
                        } else {
                            t.muted_foreground
                        })
                        .test_id(&test_ids::patch(format!(
                            "report:{}",
                            line.chars().take(40).collect::<String>()
                        ))),
                );
            }
        }
    }
    panel
}

fn outcome_panel(handle: AppHandle, outcome: ApplyOutcome) -> impl IntoElement {
    let t = theme();
    let mut panel = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::patch("outcome"))
        .child(
            text(outcome.title())
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            text(outcome.message())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(match &outcome {
                    ApplyOutcome::Applied { .. } => data::STATE_SUCCESS,
                    ApplyOutcome::Aborted { .. } => data::STATE_WARNING,
                    ApplyOutcome::RollbackFailed { .. } => data::STATE_ERROR,
                }),
        );
    match &outcome {
        ApplyOutcome::Applied { paths } => {
            panel = panel.child(
                text(format!(
                    "{} file(s) changed in the working tree. Nothing was staged and nothing was committed.",
                    paths.len()
                ))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
            );
        }
        ApplyOutcome::Aborted { reason, restored } => {
            panel = panel.child(
                text(format!("Why: {reason}"))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            );
            if !restored.is_empty() {
                panel = panel.child(
                    text(format!("ROLLED BACK · {}", restored.len()))
                        .text_size(px(data::TYPE_SECTION))
                        .text_color(t.muted_foreground),
                );
                for path in restored {
                    panel = panel.child(
                        text(path.display().to_string())
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(t.muted_foreground),
                    );
                }
            }
        }
        ApplyOutcome::RollbackFailed {
            reason,
            unrestored,
            restored,
        } => {
            panel = panel.child(
                text(format!("Why the patch failed: {reason}"))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            );
            panel = panel.child(
                text(format!("COULD NOT BE RESTORED · {}", unrestored.len()))
                    .text_size(px(data::TYPE_SECTION))
                    .text_color(t.muted_foreground),
            );
            for path in unrestored {
                panel = panel.child(
                    text(path.stated())
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(data::STATE_ERROR),
                );
            }
            if !restored.is_empty() {
                panel = panel.child(
                    text(format!(
                        "{} file(s) were restored and are listed in the Changes view.",
                        restored.len()
                    ))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.muted_foreground),
                );
            }
            let retry = handle.clone();
            panel = panel.child(button(
                ButtonProps::new("Restore recorded content")
                    .variant(ButtonVariant::Primary)
                    .test_id(&test_ids::patch("retry-restore"))
                    .on_press(move || retry.update(apply_patch::retry_restore)),
            ));
        }
    }
    panel
}

fn comment_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let comment = handle.peek(|s| s.ui.dlg.create_patch_comment.clone());
    let name = turbogit_app::patch_create::file_name(&comment);
    let change = handle.clone();
    let cancel = handle.clone();
    let ok = handle.clone();
    let body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::patch("comment-body"))
        .child(
            text(COMMENT_PROMPT)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        )
        .child(text_area(
            InputProps::new()
                .multiline(true)
                .default_value(comment)
                .accessibility_label("Patch comment")
                .test_id(&test_ids::patch("comment"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    change.update(move |s| s.ui.dlg.create_patch_comment = text);
                }),
        ))
        .child(
            text(format!("Patch file name  {name}"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
    dialog(
        DialogProps::new("create-patch-comment", "Create Patch from Local Changes")
            .on_cancel(move || close(&cancel))
            .child(body)
            .action("OK")
            .on_action(move || ok.update(create_patch::from_comment)),
    )
    .into_element()
}

fn settings_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (root, location, name, to_clipboard, base_path, reverse, encoding, stripped, removable) =
        handle.peek(|s| {
            let d = &s.ui.dlg;
            (
                d.create_patch_root.clone(),
                d.create_patch_location.clone(),
                d.create_patch_name.clone(),
                d.create_patch_to_clipboard,
                d.create_patch_base_path.clone(),
                d.create_patch_reverse,
                d.create_patch_encoding,
                d.create_patch_strip_leading,
                d.create_patch_removable,
            )
        });
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::patch("settings-body"))
        .child(scope_select(
            &handle,
            root.as_ref(),
            &test_ids::patch("root"),
            create_patch::set_root,
        ))
        .child(
            text("PATCH FILE LOCATION")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    let location_handle = handle.clone();
    let name_handle = handle.clone();
    let save_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(
                text("Directory")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            )
            .child(input(
                InputProps::new()
                    .default_value(location)
                    .placeholder("/path/to/write/the/patch")
                    .disabled(to_clipboard)
                    .accessibility_label("Patch directory")
                    .test_id(&test_ids::patch("location"))
                    .on_change(move |text: &str| {
                        let text = text.to_string();
                        location_handle.update(move |s| s.ui.dlg.create_patch_location = text);
                    }),
            ))
            .child(button(
                ButtonProps::new("Save to…")
                    .variant(ButtonVariant::Secondary)
                    .disabled(to_clipboard)
                    .test_id(&test_ids::patch("save-to"))
                    .on_press(move || save_handle.update(create_patch::choose_location)),
            ))
            .child(
                text("Name")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            )
            .child(input(
                InputProps::new()
                    .default_value(name)
                    .accessibility_label("Patch file name")
                    .test_id(&test_ids::patch("name"))
                    .on_change(move |text: &str| {
                        let text = text.to_string();
                        name_handle.update(move |s| s.ui.dlg.create_patch_name = text);
                    }),
            )),
    );
    let clipboard_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(checkbox(
                CheckboxProps::new(move || {
                    if to_clipboard {
                        CheckboxState::Checked
                    } else {
                        CheckboxState::Unchecked
                    }
                })
                .accessibility_label("To clipboard")
                .test_id(&test_ids::patch("to-clipboard"))
                .on_change(move |_| {
                    clipboard_handle.update(|s| {
                        s.ui.dlg.create_patch_to_clipboard = !s.ui.dlg.create_patch_to_clipboard;
                        s.ui.dlg.create_patch_error = None;
                    });
                }),
            ))
            .child(
                text("To clipboard")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            ),
    );
    let base_handle = handle.clone();
    body = body
        .child(
            text("BASE PATH")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(input(
            InputProps::new()
                .default_value(base_path)
                .placeholder("the project directory")
                .accessibility_label("Patch base path")
                .test_id(&test_ids::patch("base-path"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    base_handle.update(move |s| {
                        s.ui.dlg.create_patch_base_path = text;
                        create_patch::sync(s);
                    });
                }),
        ))
        .child(
            text("Paths inside the patch are written relative to this directory.")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    let reverse_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(checkbox(
                CheckboxProps::new(move || {
                    if reverse {
                        CheckboxState::Checked
                    } else {
                        CheckboxState::Unchecked
                    }
                })
                .accessibility_label("Reverse patch")
                .test_id(&test_ids::patch("reverse"))
                .on_change(move |_| {
                    reverse_handle.update(|s| {
                        s.ui.dlg.create_patch_reverse = !s.ui.dlg.create_patch_reverse;
                        create_patch::sync(s);
                    });
                }),
            ))
            .child(
                text("Reverse patch")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            ),
    );
    let encoding_handle = handle.clone();
    let encoding_items: Vec<SelectItem> = PatchEncoding::ALL
        .iter()
        .map(|value| SelectItem::new(value.label(), value.label()))
        .collect();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(
                text("Encoding")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            )
            .child(select(
                SelectProps::new(encoding_items)
                    .label(move || SharedString::from(encoding.label()))
                    .test_id(&test_ids::patch("encoding"))
                    .on_change(move |value: &SharedString| {
                        let label = value.to_string();
                        encoding_handle.update(move |s| {
                            if let Some(found) =
                                PatchEncoding::ALL.iter().find(|e| e.label() == label)
                            {
                                s.ui.dlg.create_patch_encoding = *found;
                            }
                        });
                    }),
            )),
    );
    let remove = handle.clone();
    let remove_all = handle.clone();
    let restore = handle.clone();
    let restore_all = handle.clone();
    body = body
        .child(
            text("CHANGE DIRECTORY PATHS")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            view()
                .flex_row()
                .gap(px(data::SPACING_X))
                .child(button(
                    ButtonProps::new(format!("Remove Leading Directory [{stripped}]"))
                        .variant(ButtonVariant::Secondary)
                        .disabled(stripped >= removable)
                        .test_id(&test_ids::patch("remove-leading"))
                        .on_press(move || remove.update(create_patch::remove_leading_directory)),
                ))
                .child(button(
                    ButtonProps::new("Remove All Leading Directories")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::patch("remove-all-leading"))
                        .on_press(move || {
                            remove_all.update(create_patch::remove_all_leading_directories)
                        }),
                ))
                .child(button(
                    ButtonProps::new("Restore Leading Directory")
                        .variant(ButtonVariant::Ghost)
                        .disabled(stripped == 0)
                        .test_id(&test_ids::patch("restore-leading"))
                        .on_press(move || restore.update(create_patch::restore_leading_directory)),
                ))
                .child(button(
                    ButtonProps::new("Restore All Leading Directories")
                        .variant(ButtonVariant::Ghost)
                        .disabled(stripped == 0)
                        .test_id(&test_ids::patch("restore-all-leading"))
                        .on_press(move || {
                            restore_all.update(create_patch::restore_all_leading_directories)
                        }),
                )),
        );
    if let Some(patch) = handle.peek(create_patch::preview) {
        body = body.child(files_the_patch_names(&patch));
    }
    let error = handle.peek(|s| s.ui.dlg.create_patch_error.clone());
    if let Some(error) = error {
        body = body.child(
            text(error)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::STATE_ERROR),
        );
    }
    let cancel = handle.clone();
    let ok = handle.clone();
    body = body.child(button(
        ButtonProps::new("OK")
            .variant(ButtonVariant::Primary)
            .test_id(&test_ids::patch("settings-ok"))
            .on_press(move || ok.update(create_patch::finish)),
    ));
    let title = format!(
        "{SETTINGS_TITLE} · {}",
        root.as_ref().map(|r| r.name()).unwrap_or_default()
    );
    dialog(
        DialogProps::new("patch-file-settings", title)
            .width(700.0)
            .on_cancel(move || close(&cancel))
            .child(body),
    )
    .into_element()
}

fn import_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (path, name, error, root) = handle.peek(|s| {
        (
            s.ui.dlg.import_patch_path.clone(),
            s.ui.dlg.import_patch_name.clone(),
            s.ui.dlg.import_patch_error.clone(),
            s.multi.roots.first().map(|r| r.id.clone()),
        )
    });
    let path_handle = handle.clone();
    let name_handle = handle.clone();
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::patch("import-body"))
        .child(
            text("IMPORT A PATCH TO THE SHELF")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(input(
            InputProps::new()
                .default_value(path)
                .placeholder("/path/to/change.patch")
                .accessibility_label("Patch file to import")
                .test_id(&test_ids::patch("import-path"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    path_handle.update(move |s| s.ui.dlg.import_patch_path = text);
                }),
        ))
        .child(input(
            InputProps::new()
                .default_value(name)
                .placeholder("Name on the shelf")
                .accessibility_label("Imported patch name")
                .test_id(&test_ids::patch("import-name"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    name_handle.update(move |s| s.ui.dlg.import_patch_name = text);
                }),
        ));
    if let Some(error) = error {
        body = body.child(
            text(error)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::STATE_ERROR),
        );
    }
    let import = handle.clone();
    let cancel = handle.clone();
    body = body.child(button(
        ButtonProps::new("Import")
            .variant(ButtonVariant::Primary)
            .test_id(&test_ids::patch("import"))
            .on_press(move || {
                let import = import.clone();
                let root = root.clone();
                let (path, name) = import.peek(|s| {
                    (
                        s.ui.dlg.import_patch_path.trim().to_string(),
                        s.ui.dlg.import_patch_name.trim().to_string(),
                    )
                });
                let Some(root) = root else {
                    return;
                };
                if path.is_empty() {
                    return;
                }
                import.update(move |s| {
                    let result =
                        s.import_patch_file_to_shelf(&root, std::path::Path::new(&path), &name);
                    match result {
                        Ok(shelf) => {
                            s.ui.dlg.import_patch_error = None;
                            s.ui.dialog = None;
                            s.ui.toast = Some(turbogit_app::state::Toast::success(format!(
                                "'{}' is on the shelf — nothing was applied",
                                shelf.name
                            )));
                        }
                        Err(e) => s.ui.dlg.import_patch_error = Some(e.to_string()),
                    }
                });
            }),
    ));
    dialog(
        DialogProps::new("import-patches", "Import patches")
            .width(560.0)
            .on_cancel(move || close(&cancel))
            .child(body),
    )
    .into_element()
}

pub fn patches_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let apply_file = handle.clone();
    let apply_clip = handle.clone();
    let create = handle.clone();
    let import = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("patches"))
        .child(
            text("Patches")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(
            view()
                .flex_row()
                .gap(px(data::SPACING_X))
                .child(button(
                    ButtonProps::new("Apply patch from file…")
                        .variant(ButtonVariant::Primary)
                        .test_id(&test_ids::patch("open-apply-file"))
                        .on_press(move || {
                            apply_file.update(|s| apply_patch::open(s, PatchSource::File))
                        }),
                ))
                .child(button(
                    ButtonProps::new("Apply patch from clipboard…")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::patch("open-apply-clipboard"))
                        .on_press(move || {
                            apply_clip.update(|s| apply_patch::open(s, PatchSource::Clipboard))
                        }),
                ))
                .child(button(
                    ButtonProps::new("Create patch from local changes…")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::patch("open-create"))
                        .on_press(move || create.update(create_patch::open_from_selection)),
                ))
                .child(button(
                    ButtonProps::new("Import patches…")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::patch("open-import"))
                        .on_press(move || {
                            import.update(|s| {
                                s.ui.dlg.import_patch_error = None;
                                s.ui.dialog = Some(Dialog::ImportPatches)
                            })
                        }),
                )),
        )
        .child(
            text("Applying a patch never commits; a malformed patch names its file and line.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
}
