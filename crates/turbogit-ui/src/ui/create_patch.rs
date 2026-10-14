//! `Create patch`: the two dialogs the create routes end in,
//! and the clipboard route that ends in neither.
//!
//! The reference reaches **one** `Patch File Settings` dialog from three sources —
//! uncommitted changes, a Log commit, a file revision — and its four fields are the same
//! whichever one opened it: location (or `To clipboard`), base path, `Reverse patch`,
//! encoding. They are painted in that order because the order is the discovery: the
//! destination first, then the two things that decide what the paths inside say, then the
//! encoding that decides what the bytes are.
//!
//! The comment step comes first for **local changes only**, and it is not a confirmation:
//! its text becomes the patch file's name ("enter a commit comment (it will be used as the
//! patch file name)"), so the name the settings dialog shows is *derived* from it rather
//! than typed twice. A commit and a file revision already carry a name — their subject —
//! which is why those routes open the settings directly.
//!
//! `Change Directory Paths` is the counting repair pair, and the count is the point.
//! `Remove Leading Directory` takes one segment off every path in the patch and states how
//! many it has taken **in square brackets on its own label**, so the state of a repeated
//! press is visible in the frame after the press; `Restore Leading Directory` gives one
//! back, one step at a time, and `Restore All Leading Directories` gives every one back. A
//! patch that nearly applies is repaired by counting segments rather than by retyping a
//! prefix — and a count that reset between frames, or only appeared when the dialog
//! closed, would leave nothing to count.
//!
//! The clipboard is the one act here this layer must perform itself: `AppState` decides
//! WHAT is copied and holds it in a slot, because only a frame holding an
//! `egui::Context` can copy. Every route — dialog or no dialog — drains the same slot, so
//! the two cannot hand over different artifacts.

use egui::Ui;

use turbogit_app::patch_create;
use turbogit_app::state::{AppState, Dialog, Toast};
use turbogit_domain::model::{DisplayName, PatchEncoding, RootId};

use crate::theme::{Palette, TYPE_BODY, data_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::widgets;

/// The verb's name in one place: the palette route and the dialogs all say the same thing.
pub const VERB: &str = "Create Patch";
/// The local-changes route, named for what it patches — which is not what the commit
/// window's own "Changes" tab is called for, so the two are never confused.
pub const LOCAL_CHANGES_VERB: &str = "Create Patch from Local Changes";
/// The route with **no dialog at all**.
pub const CLIPBOARD_VERB: &str = "Copy as Patch to Clipboard";
/// The settings dialog's title, and the one place its name is spelled.
pub const SETTINGS_TITLE: &str = "Patch File Settings";
/// The comment step's question, in the reference's own words: the comment is not a label
/// on the patch, it IS the patch file's name.
pub const COMMENT_PROMPT: &str = "Enter a commit comment (it will be used as the patch file name)";

/// Render whichever create surface is open.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    match state.ui.dialog {
        Some(Dialog::CreatePatchComment) => comment_step(ui, state),
        Some(Dialog::PatchFileSettings) => settings(ui, state),
        _ => {}
    }
}

/// Drain the pending clipboard payload — the application's only clipboard write.
///
/// **Unconditional**, and called from [`crate::ui::render`] rather than from [`show`]:
/// `Copy as Patch to Clipboard` is defined by opening no dialog, so a drain that lived
/// in the dialog's own paint frame could never run for the one verb that needs it. Both
/// routes fill the same slot, so neither can hand over a different artifact — and a
/// success reported before `copy_text` has run would describe an act nobody performed.
pub fn drain_clipboard(ctx: &egui::Context, state: &mut AppState) {
    if let Some(text) = state.take_create_patch_clipboard() {
        ctx.copy_text(text);
        state.show_patch_toast(Toast::success("Patch copied to the clipboard"));
    }
}

/// The comment step: one field, and the name it will produce.
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
            // Shown rather than left implicit: the comment is about to become a file
            // name, and the sanitizing that makes that possible is a transformation the
            // developer can only agree to seeing its result.
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
                    state.create_patch_from_comment();
                }
            });
        });
    if !open {
        close(state);
    }
}

/// `Patch File Settings`: the four fields in order, the counting pair, and the paths the
/// settings leave behind — so a removal count or a base path is seen against the paths it
/// produced rather than against a description of them.
fn settings(ui: &mut Ui, state: &mut AppState) {
    // The fields are brought in line with the current origin, reversal and base path
    // before anything is painted, so a frame never shows a count measured against a patch
    // that is no longer the one in the dialog.
    state.sync_create_patch();
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
    // One patch is generated from one working tree, and a multi-root workspace has more
    // than one it could mean. Painted whenever there is a choice, exactly as the apply
    // window does it: collapsing this to the focused root would be the per-root
    // rule reduced to a guess.
    let roots: Vec<RootId> = state.multi.roots.iter().map(|r| r.id.clone()).collect();
    if roots.len() > 1 {
        widgets::group_title(ui, "REPOSITORY");
        ui.horizontal(|ui| {
            for id in &roots {
                let chosen = state.ui.dlg.create_patch_root.as_ref() == Some(id);
                let btn = kit_button(
                    ui,
                    if chosen {
                        KitButton::Primary
                    } else {
                        KitButton::Quiet
                    },
                    &id.name(),
                );
                if btn.clicked() {
                    state.set_create_patch_root(id);
                }
            }
        });
    }

    // --- 1. location, or `To clipboard` ---
    widgets::group_title(ui, "PATCH FILE LOCATION");
    let to_clipboard = state.ui.dlg.create_patch_to_clipboard;
    ui.horizontal(|ui| {
        // The destination is chosen, not hidden, when the other destination is taken: a
        // path the developer has typed stays on screen so switching back is not a retype.
        ui.label("Directory");
        ui.add_enabled_ui(!to_clipboard, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.ui.dlg.create_patch_location)
                    .hint_text("/path/to/write/the/patch")
                    .desired_width(300.0),
            );
            if ui.button("Save to…").clicked() {
                state.choose_create_patch_location();
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
            // Choosing a destination is a decision: an error left over from the other one
            // was about a file this route will not write.
            state.ui.dlg.create_patch_error = None;
        }
    });

    // --- 2. base path ---
    widgets::group_title(ui, "BASE PATH");
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.ui.dlg.create_patch_base_path)
                .hint_text("the project directory")
                .desired_width(360.0),
        );
    });
    ui.label("Paths inside the patch are written relative to this directory.");

    // --- 3. reverse patch ---
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.ui.dlg.create_patch_reverse, "Reverse patch");
    });

    // --- 4. encoding ---
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

/// `Change Directory Paths`: four presses on one counter, and the counter painted **on the
/// button that moves it** — the reference's reason for the square brackets, which is that
/// the state of a repeated action should be readable in the same frame it changed.
fn directory_paths(ui: &mut Ui, state: &mut AppState) {
    widgets::group_title(ui, "CHANGE DIRECTORY PATHS");
    let removed = state.ui.dlg.create_patch_strip_leading;
    let bound = state.ui.dlg.create_patch_removable;
    // The removals are inert at the bound rather than clamping a press that would mean
    // something else: past this count a removal would take a path's own file name, and a
    // patch with a nameless path is not a shorter path but a broken artifact.
    ui.horizontal(|ui| {
        ui.add_enabled_ui(removed < bound, |ui| {
            if ui
                .button(format!("Remove Leading Directory [{removed}]"))
                .clicked()
            {
                state.remove_leading_directory();
            }
            if ui.button("Remove All Leading Directories").clicked() {
                state.remove_all_leading_directories();
            }
        });
    });
    // Both restores are inert at zero: there is nothing given back, and a press that did
    // nothing would look like a press that worked.
    ui.horizontal(|ui| {
        ui.add_enabled_ui(removed > 0, |ui| {
            if ui.button("Restore Leading Directory").clicked() {
                state.restore_leading_directory();
            }
            if ui.button("Restore All Leading Directories").clicked() {
                state.restore_all_leading_directories();
            }
        });
    });
}

/// The paths the patch names **as the settings leave them**, read off the value the app
/// layer parsed. The header count is the apply window's own vocabulary for the artifact,
/// so the two halves of this surface describe one kind of thing — and the reading of git's
/// syntax stays in the layer that owns it.
fn paths(ui: &mut Ui, state: &mut AppState) {
    let Some(patch) = state.create_patch_preview() else {
        return;
    };
    widgets::group_title(
        ui,
        &format!("FILES THE PATCH NAMES · {}", patch.files.len()),
    );
    for file in &patch.files {
        let name = if file.new_path.is_empty() {
            &file.old_path
        } else {
            &file.new_path
        };
        ui.label(
            egui::RichText::new(name.clone())
                .font(data_font(TYPE_BODY))
                .color(Palette::INK_2),
        );
    }
}

fn footer(ui: &mut Ui, state: &mut AppState) {
    let busy = state.ui.busy;
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        if kit_button(ui, KitButton::Primary, "OK").clicked() && !busy {
            state.finish_create_patch();
        }
    });
}

/// Closing a create dialog drops the flow's payload with it: the fields are answers about
/// one patch, and a second visit to this surface starts from the source it was opened on.
fn close(state: &mut AppState) {
    state.ui.dialog = None;
    state.ui.dlg.create_patch_origin = None;
}

/// The `When patch is created: Ask` follow-up — the ask the captured default stands for.
///
/// It is a **window rather than a toast** because a toast cannot carry a choice: the
/// reference's `Ask` arm is the one that leaves the decision with the developer, and a
/// transient label would decide for them while pretending not to. It is also not a
/// `Dialog` variant, because the two are mutually exclusive on the one `dialog` slot —
/// the settings step has already closed by the time this opens, and a patch created under
/// `Ask` needs its follow-up to survive the developer pressing `Cancel` on something else.
///
/// The three verbs are the surface's whole patch story in one place: park it (`Import to
/// shelf`, nothing applied), put it in the tree (`Apply patch…`, the window that states
/// what an apply did), or leave it be. The deferral is offered **first** and in the same
/// fill as the other two, because the reference's point about the shelf is that a patch
/// need not be applied the moment it exists.
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
                    state.import_created_patch_to_shelf();
                }
                if kit_button(ui, KitButton::Quiet, "Apply patch\u{2026}")
                    .on_hover_text("Open the patch in the window that applies it")
                    .clicked()
                {
                    state.apply_created_patch();
                }
                if ui.button("Do nothing").clicked() {
                    state.dismiss_created_patch();
                }
            });
        });
    if !open {
        state.dismiss_created_patch();
    }
}
