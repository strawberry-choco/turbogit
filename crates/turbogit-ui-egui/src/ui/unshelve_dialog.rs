use egui::Ui;
use turbogit_app::state::AppState;
use turbogit_domain::model::{DisplayName, RootId};

use crate::theme::Palette;
use crate::ui::components::{KitButton, counted, kit_button};
use crate::ui::widgets;

const REMOVE_OPTION: &str = "Remove successfully applied files from the shelf";

const UNSHELVE_KEEP: &str = "Unshelve";
const UNSHELVE_REMOVE: &str = "Unshelve and Remove";

/// Render the window while `state.ui.dialog` is `Dialog::Unshelve`.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    let title = match state.ui.dlg.unshelve_root.as_ref() {
        Some(root) => {
            let path = root.as_path();
            format!(
                "{UNSHELVE_KEEP} · {}",
                path.display_name_lossy_or(&path.display().to_string())
            )
        }
        None => "Unshelve shelf".to_string(),
    };
    egui::Window::new(title)
        .open(&mut open)
        .default_width(460.0)
        .show(&ctx, |ui| body(ui, state));
    if !open {
        state.ui.dialog = None;
    }
}

fn body(ui: &mut Ui, state: &mut AppState) {
    let scope = state.ui.dlg.repo_scope.clone();
    let shelves: Vec<_> = scope
        .iter()
        .flat_map(|root| state.shelves_for(root))
        .collect();
    let stored = state
        .ui
        .dlg
        .unshelve_root
        .clone()
        .zip(state.ui.dlg.unshelve_shelf.clone())
        .filter(|(root, name)| {
            shelves
                .iter()
                .any(|shelf| &shelf.root == root && &shelf.name == name)
        });
    let picked = stored.or_else(|| {
        shelves
            .first()
            .map(|shelf| (shelf.root.clone(), shelf.name.clone()))
    });
    let remove = state.ui.dlg.unshelve_remove;

    widgets::group_title(ui, "Name");
    let mut chosen: Option<(RootId, String)> = None;
    if shelves.is_empty() {
        widgets::empty_state(
            ui,
            "Nothing is shelved in the selected repositories. Use “Shelve…” to park the \
             selected changes, then unshelve from here.",
        );
    } else {
        let mut current_root: Option<&RootId> = None;
        for shelf in &shelves {
            if current_root != Some(&shelf.root) {
                widgets::group_title(ui, &shelf.root.name());
                current_root = Some(&shelf.root);
            }
            let selected = picked
                .as_ref()
                .is_some_and(|(root, name)| root == &shelf.root && name == &shelf.name);
            ui.horizontal(|ui| {
                if ui.selectable_label(selected, &shelf.name).clicked() {
                    chosen = Some((shelf.root.clone(), shelf.name.clone()));
                }
                let files = counted(shelf.changes.len(), "file", "files");
                let shelved = shelf.created_at.format("%Y-%m-%d %H:%M").to_string();
                ui.label(egui::RichText::new(format!("{files} · {shelved}")).color(Palette::INK_2));
            });
        }
    }
    if let Some((root, name)) = chosen {
        state.ui.dlg.unshelve_root = Some(root);
        state.ui.dlg.unshelve_shelf = Some(name);
    }

    widgets::note(ui, None, |ui| {
        ui.label(
            "The shelved work comes back into this repository's working tree. Nothing here \
             sorts it into a named list, because this application has none.",
        );
    });

    widgets::group_title(ui, "Options");
    ui.checkbox(&mut state.ui.dlg.unshelve_remove, REMOVE_OPTION);
    if state.ui.dlg.unshelve_remove {
        widgets::note(ui, Some(Palette::DANGER), |ui| {
            ui.label(
                "When the apply lands, the shelf record and git's copy of the content are both \
                 dropped. An apply that fails leaves the row listed and the content in the \
                 stash list.",
            );
        });
    }

    widgets::dialog_footer(ui, |ui| {
        if picked.is_some() {
            let (kind, label) = if remove {
                (KitButton::Danger, UNSHELVE_REMOVE)
            } else {
                (KitButton::Primary, UNSHELVE_KEEP)
            };
            if kit_button(ui, kind, label).clicked()
                && let Some((root, name)) = picked.clone()
            {
                state.unshelve(root, name, remove);
                state.ui.dialog = None;
            }
        }
        if ui.button("Cancel").clicked() {
            state.ui.dialog = None;
        }
    });
}
