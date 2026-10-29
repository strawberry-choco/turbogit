use egui::Ui;

use turbogit_app::state::AppState;
use turbogit_domain::model::{Patch, RootId};

use crate::theme::{Palette, TYPE_BODY, data_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::widgets;

pub(crate) fn repository_chooser(
    ui: &mut Ui,
    state: &mut AppState,
    open_root: impl for<'a> Fn(&'a AppState) -> Option<&'a RootId>,
    set_root: fn(&mut AppState, &RootId),
) {
    let roots: Vec<RootId> = state.multi.roots.iter().map(|r| r.id.clone()).collect();
    if roots.len() > 1 {
        widgets::group_title(ui, "REPOSITORY");
        ui.horizontal(|ui| {
            for id in &roots {
                let chosen = open_root(state) == Some(id);
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
                    set_root(state, id);
                }
            }
        });
    }
}

pub(crate) fn files_the_patch_names(ui: &mut Ui, patch: &Patch) {
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
