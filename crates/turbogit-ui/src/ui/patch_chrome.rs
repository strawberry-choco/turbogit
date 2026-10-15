//! The two blocks the patch windows share: the per-root `REPOSITORY` chooser and
//! the listing of every path a parsed patch names.
//!
//! The two windows reach these with their own open-root field and setter, so the
//! wording and the chooser's rule are stated once rather than in two copies that
//! can drift. The helper only paints: which patch is listed, and which root is
//! open, are the caller's, and no policy decision moves in here.

use egui::Ui;

use turbogit_app::state::AppState;
use turbogit_domain::model::{Patch, RootId};

use crate::theme::{Palette, TYPE_BODY, data_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::widgets;

/// The per-root choice, painted whenever the workspace has more than one tree the
/// verb could mean. One verb writes one working tree; a window that silently took
/// the focused root would be the multi-root collapse wearing a dialog.
///
/// `open_root` is a reader rather than a snapshot: a press on one root repaints
/// the rest of the row against the root it just chose, which is what both copied
/// blocks did.
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

/// The files the patch names — the blast radius, read from the parsed value rather
/// than from git's answer, so the developer sees what will be written before
/// anything is written. The header count is the apply window's own vocabulary for
/// the artifact, which is what makes the two windows describe one kind of thing.
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
