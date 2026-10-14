//! The **Unshelve dialog**: the window that asks which shelf to put back, and
//! whether the shelf survives the apply.
//!
//! The reference's dialog lists five fields. Two of them describe a choice this
//! application can actually make — *which* of this repository's shelves, and
//! *whether the shelved copy is dropped afterwards* — and both are here, in the
//! reference's order. The other three (`Name` as "enter a name for a destination
//! to be created", `Set active`, `Track context`, and the `Comment` that describes
//! such a destination) are changelist verbs: they ask which named bucket the work
//! should arrive in and which of those buckets should be the active one. This app
//! stages into buckets and has no changelist model, so this window paints **no
//! control** for them — a checkbox that ticks and changes nothing is worse than an
//! absent one — and says plainly where the work does land instead.
//!
//! The affirmative's label is not decoration. The reference spells its two verbs
//! `Unshelve` and `Unshelve and Remove` because the consequence has to travel in
//! the name, and one button that renames and recolors itself with the option keeps
//! that rule inside a dialog that has only one action slot: the destroying verb is
//! never wearable by accident.
//!
//! What the click does is exactly what the pane's row verbs do — hand the intent to
//! [`AppState::unshelve`] — so the keep/remove decision is settled by the
//! `OpKind::Unshelve` **success** arm and its write-then-commit shelf removal, not
//! by this window advancing its own state. A refused index rewrite leaves the shelf
//! listed, and an apply that never landed never takes the row with it.

use egui::Ui;
use turbogit_app::state::AppState;
use turbogit_domain::model::DisplayName;

use crate::theme::Palette;
use crate::ui::components::{KitButton, counted, kit_button};
use crate::ui::widgets;

/// The reference's own spelling of the choice that makes shelf deletion survivable.
///
/// It is a `const` rather than inline because the panes' two verbs and this option
/// state one fact in two surfaces, and the sentence the option is named by has to be
/// the same sentence in both.
const REMOVE_OPTION: &str = "Remove successfully applied files from the shelf";

/// The dialog's other verb label: the option ticked turns the single affirmative
/// into the destructive sibling, spelled exactly as the pane's danger button spells it.
const UNSHELVE_KEEP: &str = "Unshelve";
const UNSHELVE_REMOVE: &str = "Unshelve and Remove";

/// Render the window while `state.ui.dialog` is `Dialog::Unshelve`.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    // The title names the repository, because that is the fact the whole window is
    // about: a shelf's content is parked in one repository's stash list, and the
    // list below only ever offers shelves of the root this dialog was opened on.
    let title = match state.ui.dlg.unshelve_root.as_ref() {
        Some(root) => {
            let path = root.as_path();
            format!(
                "{UNSHELVE_KEEP} · {}",
                path.display_name_lossy_or(&path.display().to_string())
            )
        }
        None => UNSHELVE_KEEP.to_string(),
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
    // Snapshot the frame's inputs so the row and footer handlers can take
    // `&mut AppState` freely (the deferred-intent shape this shell already uses).
    let root = state.ui.dlg.unshelve_root.clone();
    let shelves = root
        .as_ref()
        .map(|id| state.shelves_for(id))
        .unwrap_or_default();
    let stored_pick = state.ui.dlg.unshelve_shelf.clone();
    let remove = state.ui.dlg.unshelve_remove;

    // A pick is resolved against the list every frame rather than trusted: a shelf
    // deleted elsewhere while this window stayed open falls back to the newest row
    // instead of leaving the affirmative aimed at a name nothing can apply.
    let picked = stored_pick
        .filter(|name| shelves.iter().any(|shelf| &shelf.name == name))
        .or_else(|| shelves.first().map(|shelf| shelf.name.clone()));

    widgets::group_title(ui, "Name");
    let mut chosen: Option<String> = None;
    if shelves.is_empty() {
        widgets::empty_state(
            ui,
            "Nothing is shelved in this repository. Use “Shelve…” to park the selected \
             changes, then unshelve from here.",
        );
    } else {
        for shelf in &shelves {
            let selected = picked.as_deref() == Some(shelf.name.as_str());
            ui.horizontal(|ui| {
                if ui.selectable_label(selected, &shelf.name).clicked() {
                    chosen = Some(shelf.name.clone());
                }
                let files = counted(shelf.changes.len(), "file", "files");
                let shelved = shelf.created_at.format("%Y-%m-%d %H:%M").to_string();
                ui.label(egui::RichText::new(format!("{files} · {shelved}")).color(Palette::INK_2));
            });
        }
    }
    if let Some(name) = chosen {
        state.ui.dlg.unshelve_shelf = Some(name);
    }

    // Where the work actually goes, in one line: the field this reference window
    // spends three controls on is answered here by a sentence, because there is no
    // named destination to choose.
    widgets::note(ui, None, |ui| {
        ui.label(
            "The shelved work comes back into this repository's working tree. Nothing here \
             sorts it into a named list, because this application has none.",
        );
    });

    widgets::group_title(ui, "Options");
    ui.checkbox(&mut state.ui.dlg.unshelve_remove, REMOVE_OPTION);
    if state.ui.dlg.unshelve_remove {
        // The consequence beside the option, stated as the settlement states it: the
        // shelf goes only once the apply lands, so a failure is still a listed row.
        widgets::note(ui, Some(Palette::DANGER), |ui| {
            ui.label(
                "When the apply lands, the shelf record and git's copy of the content are both \
                 dropped. An apply that fails leaves the row listed and the content in the \
                 stash list.",
            );
        });
    }

    widgets::dialog_footer(ui, |ui| {
        // Right-to-left, so the painted order is `Cancel` then the affirmative,
        // right-aligned like the rest of this app's confirm dialogs.
        //
        // Withheld rather than greyed when there is nothing to apply: the accent fill
        // survives a disabled state, so a greyed `Unshelve` would still paint a promise
        // the click cannot keep.
        if root.is_some() && picked.is_some() {
            let (kind, label) = if remove {
                (KitButton::Danger, UNSHELVE_REMOVE)
            } else {
                (KitButton::Primary, UNSHELVE_KEEP)
            };
            if kit_button(ui, kind, label).clicked()
                && let (Some(root), Some(name)) = (root.clone(), picked.clone())
            {
                // The app layer owns both halves: git's apply and, on its success, the
                // shelf record's removal through the store's own write-then-commit.
                state.unshelve(root, name, remove);
                state.ui.dialog = None;
            }
        }
        if ui.button("Cancel").clicked() {
            state.ui.dialog = None;
        }
    });
}
