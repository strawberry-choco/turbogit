use egui::{Key, Ui};

use turbogit_app::apply_patch;
use turbogit_app::patch_apply::{ApplyOutcome, Preflight};
use turbogit_app::state::{AppState, PatchSource};

use crate::theme::{Palette, TYPE_BODY, data_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::patch_chrome;
use crate::ui::widgets;

pub const VERB: &str = "Apply patch";

/// Render the window while `state.ui.dialog` is `Dialog::ApplyPatch`.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    apply_patch::sync(state);
    let ctx = ui.ctx().clone();
    let mut open = true;
    let root = state
        .ui
        .dlg
        .apply_patch_root
        .as_ref()
        .map(|r| r.name())
        .unwrap_or_default();
    egui::Window::new(format!("{VERB} · {root}"))
        .open(&mut open)
        .default_width(660.0)
        .show(&ctx, |ui| body(ui, state));
    if !open {
        state.ui.dialog = None;
    }
}

fn body(ui: &mut Ui, state: &mut AppState) {
    patch_chrome::repository_chooser(
        ui,
        state,
        |s| s.ui.dlg.apply_patch_root.as_ref(),
        apply_patch::set_root,
    );

    route(ui, state);
    provenance(ui, state);
    touched(ui, state);
    outcome(ui, state);
    footer(ui, state);
}

fn route(ui: &mut Ui, state: &mut AppState) {
    let source = state.ui.dlg.apply_patch_source;
    widgets::group_title(ui, "PATCH SOURCE");
    ui.horizontal(|ui| {
        for (variant, label) in [
            (PatchSource::File, "From file"),
            (PatchSource::Clipboard, "From clipboard"),
        ] {
            let btn = kit_button(
                ui,
                if variant == source {
                    KitButton::Primary
                } else {
                    KitButton::Quiet
                },
                label,
            );
            if btn.clicked() {
                apply_patch::set_source(state, variant);
            }
        }
    });

    if source == PatchSource::File {
        ui.horizontal(|ui| {
            ui.label("Path");
            ui.add(
                egui::TextEdit::singleline(&mut state.ui.dlg.apply_patch_path)
                    .hint_text("/path/to/change.patch")
                    .desired_width(360.0),
            );
            if ui.button("Choose file…").clicked() {
                apply_patch::choose_file(state);
            }
        });
    } else {
        ui.label("Paste the patch below — no file is read.");
        ui.add(
            egui::TextEdit::multiline(&mut state.ui.dlg.apply_patch_text)
                .font(data_font(TYPE_BODY))
                .desired_rows(6)
                .desired_width(f32::INFINITY),
        );
    }
    if let Some(message) = state.ui.dlg.apply_patch_error.clone() {
        widgets::inline_error(ui, message);
    }
}

fn provenance(ui: &mut Ui, state: &mut AppState) {
    if state.ui.dlg.apply_patch_patch.is_none() {
        return;
    }
    widgets::group_title(ui, "PATCH PROVENANCE");
    if state.ui.dlg.apply_patch_provenance.is_none() {
        ui.label("This patch carries no commit header, so there is nothing to fill.");
    } else {
        ui.horizontal(|ui| {
            ui.label("Author");
            ui.add(
                egui::TextEdit::singleline(&mut state.ui.dlg.apply_patch_author)
                    .desired_width(320.0),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Message");
            ui.add(
                egui::TextEdit::singleline(&mut state.ui.dlg.apply_patch_message)
                    .desired_width(420.0),
            );
        });
    }
}

fn touched(ui: &mut Ui, state: &mut AppState) {
    if let Some(patch) = state.ui.dlg.apply_patch_patch.as_ref() {
        patch_chrome::files_the_patch_names(ui, patch);
    }
}

fn preflight_panel(ui: &mut Ui, state: &mut AppState) {
    let Some(preflight) = state.ui.dlg.apply_patch_preflight.clone() else {
        return;
    };
    widgets::group_title(ui, "SHOW DIFF");
    match preflight {
        Preflight::Clean { files } => {
            ui.colored_label(
                Palette::STATE_SUCCESS,
                format!("Applies cleanly — {files} file section(s) would be written."),
            );
        }
        Preflight::Refused { report } => {
            ui.colored_label(
                Palette::STATE_WARNING,
                format!("Nothing to check — {report}"),
            );
        }
        Preflight::Conflicts { report, paths } => {
            ui.colored_label(
                Palette::STATE_ERROR,
                "The patch does not apply cleanly. No file has been changed.",
            );
            for line in report.lines().filter(|l| !l.trim().is_empty()) {
                let conflicts = paths
                    .iter()
                    .any(|p| line.contains(&p.to_string_lossy().into_owned()));
                ui.label(
                    egui::RichText::new(line.to_string())
                        .font(data_font(TYPE_BODY))
                        .color(if conflicts {
                            Palette::STATE_ERROR
                        } else {
                            Palette::INK_2
                        }),
                );
            }
        }
    }
}

fn outcome(ui: &mut Ui, state: &mut AppState) {
    let Some(outcome) = state.ui.dlg.apply_patch_outcome.clone() else {
        preflight_panel(ui, state);
        return;
    };
    match outcome {
        ApplyOutcome::Applied { ref paths } => {
            widgets::group_title(ui, "PATCH APPLIED");
            ui.colored_label(
                Palette::STATE_SUCCESS,
                format!(
                    "{} file(s) changed in the working tree. Nothing was staged and nothing was \
                     committed.",
                    paths.len()
                ),
            );
        }
        ApplyOutcome::Aborted {
            ref reason,
            ref restored,
        } => {
            widgets::group_title(ui, ApplyOutcome::ABORTED_TITLE);
            ui.colored_label(Palette::STATE_WARNING, outcome.message());
            ui.label(format!("Why: {reason}"));
            if !restored.is_empty() {
                widgets::group_title(ui, &format!("ROLLED BACK · {}", restored.len()));
                for path in restored {
                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .font(data_font(TYPE_BODY))
                            .color(Palette::INK_2),
                    );
                }
            }
        }
        ApplyOutcome::RollbackFailed {
            ref reason,
            ref unrestored,
            ref restored,
        } => {
            widgets::group_title(ui, ApplyOutcome::ROLLBACK_FAILED_TITLE);
            ui.colored_label(Palette::STATE_ERROR, outcome.message());
            ui.label(format!("Why the patch failed: {reason}"));
            widgets::group_title(ui, &format!("COULD NOT BE RESTORED · {}", unrestored.len()));
            for path in unrestored {
                ui.label(
                    egui::RichText::new(path.stated())
                        .font(data_font(TYPE_BODY))
                        .color(Palette::STATE_ERROR),
                );
            }
            if !restored.is_empty() {
                ui.label(format!(
                    "{} file(s) were restored and are listed in the Changes view.",
                    restored.len()
                ));
            }
            let busy = state.ui.busy;
            if kit_button(ui, KitButton::Primary, "Restore recorded content").clicked() && !busy {
                apply_patch::retry_restore(state);
            }
        }
    }
    preflight_panel(ui, state);
}

fn footer(ui: &mut Ui, state: &mut AppState) {
    let loaded = state.ui.dlg.apply_patch_patch.is_some();
    let inflight = state.ui.busy;
    let command_d = ui.input(|i| i.modifiers.command && i.key_pressed(Key::D));
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Close").clicked() {
            state.ui.dialog = None;
        }
        let show_diff = ui.button("Show Diff").clicked() || command_d;
        if show_diff && loaded && !inflight {
            apply_patch::preflight(state);
        }
        if command_d {
            ui.label("⌘D");
        }
        let import = kit_button(ui, KitButton::Secondary, "Import to shelf");
        if import.clicked() && loaded && !inflight {
            state.import_patch_from_apply_window();
        }
        let apply = kit_button(ui, KitButton::Primary, VERB);
        if apply.clicked() && loaded && !inflight {
            apply_patch::dispatch(state);
        }
    });
}
