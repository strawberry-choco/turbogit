//! `Apply patch`: the window that writes a patch into ONE
//! repository's working tree, and the surface that states what happened to it.
//!
//! Two routes, because the reference has two verbs: `Git | Patch | Apply patch…`
//! reads a file, and `Apply patch from Clipboard` **skips the file path entirely**.
//! TurboGit has no main menu bar, so the command palette carries both routes — the
//! substitution the palette already makes for the lifecycle verbs. The clipboard
//! route pastes into the window rather than reading the system clipboard
//! unprompted: the patch text ends up visible in the field it was typed into, which
//! is the same act with its payload on screen, and it is what makes the route
//! testable without a clipboard the CI runner does not own.
//!
//! `Show Diff` here is a **preflight, not a confirmation** ("to check if your patch
//! can be applied without conflicts"). It asks git and writes nothing, so it can be
//! pressed as often as the developer likes; the apply does not require it to have
//! been pressed, because a verb that only runs after a check the user was forced
//! through would be a confirmation wearing a preview's name.
//!
//! The window's third job is the state it leaves behind. An apply has three
//! endings, and the two titled ones are what makes the atomicity claim honest:
//! `Apply patch aborted` says every file the apply changed is back to the content
//! it held, and `Rollback failed` — a state painted **instead**, never beside it —
//! names the files that are not, holds what they contained, and offers the one
//! action that can still put them back.

use egui::{Key, Ui};

use turbogit_app::state::{AppState, PatchSource};
use turbogit_services::patch_apply::{ApplyOutcome, Preflight};

use crate::theme::{Palette, TYPE_BODY, data_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::widgets;

/// The verb's name in one place: the palette route, the window's title and its
/// affirmative button all say the same thing.
pub const VERB: &str = "Apply patch";

/// Render the window while `state.ui.dialog` is `Dialog::ApplyPatch`.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    // Every field the developer has since touched is brought in line before
    // anything is painted, so a frame never shows a verdict about a patch that is
    // no longer the one in the field.
    state.sync_apply_patch();
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
    // The per-root choice, painted whenever the workspace has more than one tree
    // an apply could mean. One apply writes one working tree; a window that
    // silently took the focused root would be the multi-root collapse the
    // programme forbids wearing a dialog.
    let roots: Vec<turbogit_domain::model::RootId> =
        state.multi.roots.iter().map(|r| r.id.clone()).collect();
    if roots.len() > 1 {
        widgets::group_title(ui, "REPOSITORY");
        ui.horizontal(|ui| {
            for id in &roots {
                let chosen = state.ui.dlg.apply_patch_root.as_ref() == Some(id);
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
                    state.set_apply_patch_root(id);
                }
            }
        });
    }

    route(ui, state);
    provenance(ui, state);
    touched(ui, state);
    outcome(ui, state);
    footer(ui, state);
}

/// Which of the two routes is live, and the payload it reads. The clipboard route
/// does not paint the path field at all — that absence is the verb.
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
                // Which payload the switch drops is the app's rule, not this
                // frame's: the other route's patch text goes, the working tree's
                // unfinished rollback stays.
                state.set_apply_patch_source(variant);
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
                state.choose_apply_patch_file();
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

/// The patch's own provenance, and the fields it fills. A git-format patch carries
/// the commit's author and subject ahead of its first `diff --git` section, and the
/// reference reads them rather than discarding them; these two fields are where
/// that lands.
fn provenance(ui: &mut Ui, state: &mut AppState) {
    if state.ui.dlg.apply_patch_patch.is_none() {
        return;
    }
    widgets::group_title(ui, "PATCH PROVENANCE");
    if state.ui.dlg.apply_patch_provenance.is_none() {
        // Stated rather than left as two empty fields: a plain `git diff` patch
        // carries no author and no subject, and silence here reads as a window
        // that forgot to look.
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

/// The files the patch names — the blast radius, stated from the parsed value
/// rather than from git's answer, so a developer sees what will be written before
/// anything is written.
fn touched(ui: &mut Ui, state: &mut AppState) {
    let Some(patch) = state.ui.dlg.apply_patch_patch.clone() else {
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

/// `Show Diff`'s answer: git's verdict, with the lines that name a patch path
/// painted in the error ink — the reference's "the corresponding lines will be
/// highlighted in red", stated as the ink rule it is.
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

/// The ending the last apply left, painted as its titled state.
///
/// The two titled branches are mutually exclusive **by construction**: the outcome
/// is one value with three variants, so a tree the rollback could not finish paints
/// `Rollback failed` and has no way to also paint `Apply patch aborted`. That is what
/// the reference's separately-named failure state is for, and what an implementation
/// with one prose string for both would lose.
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
            // The one recovery action the state carries. Not a dismiss: the
            // recorded content is in hand, the write was refused, and retrying it
            // is the only thing that can still be true about these files. Whether
            // it works, the window says which state the tree is in afterwards.
            let busy = state.ui.busy;
            if kit_button(ui, KitButton::Primary, "Restore recorded content").clicked() && !busy {
                state.retry_apply_patch_restore();
            }
        }
    }
    preflight_panel(ui, state);
}

fn footer(ui: &mut Ui, state: &mut AppState) {
    let loaded = state.ui.dlg.apply_patch_patch.is_some();
    let inflight = state.ui.busy;
    // The reference's ⌘D. Checked here rather than on the button's `Response`
    // because this window is the only surface that reads it, and it is read only
    // while the window is painted — a global binding would fire under a text
    // field that is mid-keystroke.
    let command_d = ui.input(|i| i.modifiers.command && i.key_pressed(Key::D));
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Close").clicked() {
            state.ui.dialog = None;
        }
        // `Show Diff` is a read of git's verdict and nothing else.
        let show_diff = ui.button("Show Diff").clicked() || command_d;
        if show_diff && loaded && !inflight {
            state.preflight_apply_patch();
        }
        if command_d {
            ui.label("⌘D");
        }
        // `Import to shelf` is the deferral, and a deferral offered only in a corner
        // case is not a deferral: it is enabled on exactly the condition the apply is,
        // and it is a **different fill** because two buttons that look identical offer
        // two things the developer has to tell apart before pressing one. It writes no
        // file into the working tree — the patch goes to the shelf as bytes, which is
        // the point of parking it *before* applying.
        let import = kit_button(ui, KitButton::Secondary, "Import to shelf");
        if import.clicked() && loaded && !inflight {
            state.import_patch_from_apply_window();
        }
        let apply = kit_button(ui, KitButton::Primary, VERB);
        if apply.clicked() && loaded && !inflight {
            state.dispatch_apply_patch();
        }
    });
}
