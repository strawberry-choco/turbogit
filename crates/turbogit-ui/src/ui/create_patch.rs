use egui::Ui;

use turbogit_app::create_patch;
use turbogit_app::patch_create;
use turbogit_app::state::{AppState, Dialog, Toast};
use turbogit_domain::model::{DisplayName, PatchEncoding};

use crate::theme::{Palette, TYPE_BODY, data_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::patch_chrome;
use crate::ui::widgets;

pub const VERB: &str = "Create Patch";
pub const LOCAL_CHANGES_VERB: &str = "Create Patch from Local Changes";
pub const CLIPBOARD_VERB: &str = "Copy as Patch to Clipboard";
pub const SETTINGS_TITLE: &str = "Patch File Settings";
pub const COMMENT_PROMPT: &str = "Enter a commit comment (it will be used as the patch file name)";

pub fn show(ui: &mut Ui, state: &mut AppState) {
    match state.ui.dialog {
        Some(Dialog::CreatePatchComment) => comment_step(ui, state),
        Some(Dialog::PatchFileSettings) => settings(ui, state),
        _ => {}
    }
}

/// Drain the pending clipboard payload, the application's only clipboard write.
///
/// **Unconditional**, and called from [`crate::ui::render`] rather than from [`show`]:
/// `Copy as Patch to Clipboard` is defined by opening no dialog, so a drain that lived
/// in the dialog's own paint frame could never run for the one verb that needs it. Both
/// routes fill the same slot, so neither can hand over a different artifact, and a
/// success reported before `copy_text` has run would describe an act nobody performed.
pub fn drain_clipboard(ctx: &egui::Context, state: &mut AppState) {
    if let Some(text) = create_patch::take_clipboard(state) {
        ctx.copy_text(text);
        state.show_patch_toast(Toast::success("Patch copied to the clipboard"));
    }
}

fn comment_step(ui: &mut Ui, state: &mut AppState) {
    let mut open = true;
    egui::Window::new(LOCAL_CHANGES_VERB)
        .open(&mut open)
        .default_width(520.0)
        .show(&ui.ctx().clone(), |ui| {
            ui.label(COMMENT_PROMPT);
            ui.add(
                egui::TextEdit::multiline(&mut state.ui.dlg.create_patch_comment)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            let name = patch_create::file_name(&state.ui.dlg.create_patch_comment.clone());
            ui.horizontal(|ui| {
                ui.label("Patch file name");
                ui.label(
                    egui::RichText::new(name)
                        .font(data_font(TYPE_BODY))
                        .color(Palette::INK_2),
                );
            });
            if let Some(error) = state.ui.dlg.create_patch_error.clone() {
                widgets::inline_error(ui, error);
            }
            widgets::dialog_footer(ui, |ui| {
                if ui.button("Cancel").clicked() {
                    close(state);
                }
                if kit_button(ui, KitButton::Primary, "OK").clicked() {
                    create_patch::from_comment(state);
                }
            });
        });
    if !open {
        close(state);
    }
}

fn settings(ui: &mut Ui, state: &mut AppState) {
    create_patch::sync(state);
    let mut open = true;
    let root = state
        .ui
        .dlg
        .create_patch_root
        .as_ref()
        .map(|r| r.name())
        .unwrap_or_default();
    egui::Window::new(format!("{SETTINGS_TITLE} · {root}"))
        .open(&mut open)
        .default_width(660.0)
        .show(&ui.ctx().clone(), |ui| {
            body(ui, state);
        });
    if !open {
        close(state);
    }
}

fn body(ui: &mut Ui, state: &mut AppState) {
    patch_chrome::repository_chooser(
        ui,
        state,
        |s| s.ui.dlg.create_patch_root.as_ref(),
        create_patch::set_root,
    );

    widgets::group_title(ui, "PATCH FILE LOCATION");
    let to_clipboard = state.ui.dlg.create_patch_to_clipboard;
    ui.horizontal(|ui| {
        ui.label("Directory");
        ui.add_enabled_ui(!to_clipboard, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.ui.dlg.create_patch_location)
                    .hint_text("/path/to/write/the/patch")
                    .desired_width(300.0),
            );
            if ui.button("Save to…").clicked() {
                create_patch::choose_location(state);
            }
        });
        ui.label("Name");
        ui.add(
            egui::TextEdit::singleline(&mut state.ui.dlg.create_patch_name)
                .desired_width(170.0)
                .id_salt("patch_file_name"),
        );
    });
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut state.ui.dlg.create_patch_to_clipboard, "To clipboard")
            .changed()
        {
            state.ui.dlg.create_patch_error = None;
        }
    });

    widgets::group_title(ui, "BASE PATH");
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.ui.dlg.create_patch_base_path)
                .hint_text("the project directory")
                .desired_width(360.0),
        );
    });
    ui.label("Paths inside the patch are written relative to this directory.");

    ui.horizontal(|ui| {
        ui.checkbox(&mut state.ui.dlg.create_patch_reverse, "Reverse patch");
    });

    ui.horizontal(|ui| {
        ui.label("Encoding");
        let current = state.ui.dlg.create_patch_encoding;
        egui::ComboBox::from_id_salt("patch_file_encoding")
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for encoding in PatchEncoding::ALL {
                    ui.selectable_value(
                        &mut state.ui.dlg.create_patch_encoding,
                        encoding,
                        encoding.label(),
                    );
                }
            });
    });

    directory_paths(ui, state);
    paths(ui, state);
    if let Some(error) = state.ui.dlg.create_patch_error.clone() {
        widgets::inline_error(ui, error);
    }
    footer(ui, state);
}

fn directory_paths(ui: &mut Ui, state: &mut AppState) {
    widgets::group_title(ui, "CHANGE DIRECTORY PATHS");
    let removed = state.ui.dlg.create_patch_strip_leading;
    let bound = state.ui.dlg.create_patch_removable;
    ui.horizontal(|ui| {
        ui.add_enabled_ui(removed < bound, |ui| {
            if ui
                .button(format!("Remove Leading Directory [{removed}]"))
                .clicked()
            {
                create_patch::remove_leading_directory(state);
            }
            if ui.button("Remove All Leading Directories").clicked() {
                create_patch::remove_all_leading_directories(state);
            }
        });
    });
    ui.horizontal(|ui| {
        ui.add_enabled_ui(removed > 0, |ui| {
            if ui.button("Restore Leading Directory").clicked() {
                create_patch::restore_leading_directory(state);
            }
            if ui.button("Restore All Leading Directories").clicked() {
                create_patch::restore_all_leading_directories(state);
            }
        });
    });
}

fn paths(ui: &mut Ui, state: &mut AppState) {
    let Some(patch) = create_patch::preview(state) else {
        return;
    };
    patch_chrome::files_the_patch_names(ui, &patch);
}

fn footer(ui: &mut Ui, state: &mut AppState) {
    let busy = state.ui.busy;
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        if kit_button(ui, KitButton::Primary, "OK").clicked() && !busy {
            create_patch::finish(state);
        }
    });
}

fn close(state: &mut AppState) {
    state.ui.dialog = None;
    state.ui.dlg.create_patch_origin = None;
}

pub fn created_patch_ask(ui: &egui::Ui, state: &mut AppState) {
    let Some(ask) = state.ui.patch_created.clone() else {
        return;
    };
    let ctx = ui.ctx().clone();
    let name = ask.path.display_name_lossy_or(&ask.path.to_string_lossy());
    let root = ask.root.name();
    let mut open = true;
    egui::Window::new("Patch created")
        .open(&mut open)
        .show(&ctx, |ui| {
            ui.label(format!("Wrote “{name}” from {root}."));
            ui.label("Nothing has been done with it yet.");
            ui.horizontal(|ui| {
                if kit_button(ui, KitButton::Secondary, "Import to shelf")
                    .on_hover_text("Park this patch on the shelf without applying it")
                    .clicked()
                {
                    create_patch::import_created_to_shelf(state);
                }
                if kit_button(ui, KitButton::Quiet, "Apply patch\u{2026}")
                    .on_hover_text("Open the patch in the window that applies it")
                    .clicked()
                {
                    create_patch::apply_created(state);
                }
                if ui.button("Do nothing").clicked() {
                    create_patch::dismiss_created(state);
                }
            });
        });
    if !open {
        create_patch::dismiss_created(state);
    }
}
