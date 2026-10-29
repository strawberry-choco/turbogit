use egui::{RichText, Ui};
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, RewriteUndo, RewriteUndoVerb};

use crate::theme::{Palette, TYPE_CONTROL, chrome_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::widgets;

const UNDO_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

pub fn success_title(verb: RewriteUndoVerb, commit: &str) -> String {
    match verb {
        RewriteUndoVerb::Drop => {
            format!("Dropped commit {}", widgets::short_commit_ref(commit))
        }
        RewriteUndoVerb::Reword => "Commit message changed".to_string(),
    }
}

pub fn cannot_undo(verb: RewriteUndoVerb) -> &'static str {
    match verb {
        RewriteUndoVerb::Drop => "Cannot undo drop — the repository has changed since",
        RewriteUndoVerb::Reword => {
            "Cannot undo commit message edit — the repository has changed since"
        }
    }
}

pub use turbogit_app::shell_writes::rewrite::undo_label;

pub fn banner(ui: &mut Ui, state: &mut AppState) {
    let Some(offer) = state.ui.rewrite_undo.clone() else {
        return;
    };
    if offer.created_at.elapsed() > UNDO_WINDOW {
        state.ui.rewrite_undo = None;
        return;
    }
    ui.horizontal(|ui| {
        if offer.withheld {
            ui.label(
                RichText::new(cannot_undo(offer.verb))
                    .font(chrome_font(TYPE_CONTROL))
                    .color(Palette::T_SECONDARY),
            );
        } else {
            ui.label(
                RichText::new(success_title(offer.verb, &offer.commit))
                    .font(chrome_font(TYPE_CONTROL))
                    .color(Palette::INK),
            );
            if kit_button(ui, KitButton::Primary, "Undo").clicked() {
                dispatch_undo(state, &offer);
            }
        }
    });
    ui.add_space(4.0);
}

fn dispatch_undo(state: &mut AppState, offer: &RewriteUndo) {
    state.ui.rewrite_undo = None;
    state.dispatch(shell_writes::rewrite::undo(
        &offer.root,
        offer.verb,
        &offer.tip_sha,
    ));
}
