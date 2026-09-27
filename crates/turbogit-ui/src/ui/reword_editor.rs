//! The commit-message editor Reword commit opens (ticket 11).
//!
//! One multiline editor over the commit's REAL message — subject and body in a
//! single string, seeded from the commit itself. Not two fields: a
//! subject/body pair would have to re-join the two with git's blank-line
//! convention, and every bug in that re-join is a corrupted commit message. A
//! developer fixing a typo in a body, a subject, or both is the same edit here,
//! and the value handed to the rewrite is the value on screen.
//!
//! This is the FIRST of the verb's two steps, and deliberately not the only one:
//! the message is written here, and the cost of writing it — every commit above
//! this one re-created — is shown in the preflight this confirm opens. One step
//! would have to either drop the preflight (the ticket requires it) or put a
//! plan in the same dialog, which is the 1080px plan editor's job and not this
//! verb's.
//!
//! The one thing the confirm changes about the value is its trailing newlines,
//! which it drops: git drops them anyway, and a dialog that writes what the
//! field holds byte for byte would be preserving something no git object can
//! hold. Everything else is handed over as typed.
//!
//! Nothing here runs git. The confirm hands the message to
//! [`AppState::open_rewrite_preflight`], which builds the plan and shows what the
//! rewrite costs.

use egui::{RichText, TextEdit, Ui, WidgetInfo, WidgetType};
use turbogit_app::state::{AppState, HistoryVerb, RewordDraft};

use crate::theme::Palette;
use crate::ui::widgets;

/// The accessibility name of the editor, so a test and a screen reader can
/// address it — the plan editor names its todo the same way.
const EDITOR_LABEL: &str = "REWORD-MESSAGE";

/// Render the editor while `ui.dialog` is
/// [`Dialog::Reword`](turbogit_app::state::Dialog::Reword).
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let Some(draft) = state.ui.dlg.reword.clone() else {
        return;
    };
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Reword commit")
        .open(&mut open)
        .default_width(420.0)
        .show(&ctx, |ui| body(ui, state, &draft));
    if !open {
        close(state);
    }
}

fn close(state: &mut AppState) {
    state.ui.dialog = None;
    state.ui.dlg.reword = None;
    state.ui.dlg.reword_message.clear();
}

/// The commit being reworded, named by its short reference in the data face —
/// the same slot and face Copy hash uses, so a hash never reads as a label —
/// and then the message itself.
fn body(ui: &mut Ui, state: &mut AppState, draft: &RewordDraft) {
    ui.label(
        RichText::new(widgets::short_commit_ref(&draft.commit))
            .font(crate::theme::data_font(crate::theme::TYPE_BODY))
            .color(Palette::INK_2),
    );
    ui.add_space(4.0);

    // The first line is the subject, by git's own definition and this app's
    // everywhere else. It cannot be PAINTED differently inside one field — egui
    // styles a TextEdit as a whole, and splitting the value to weight the first
    // line would mean re-joining it, which is the corruption this design exists
    // to avoid. So the convention is STATED instead, once, under the field.
    let response = TextEdit::multiline(&mut state.ui.dlg.reword_message)
        .hint_text("New commit message")
        .desired_rows(7)
        .desired_width(f32::INFINITY)
        .show(ui);
    response
        .response
        .widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, EDITOR_LABEL));
    ui.label(
        RichText::new("The first line is the subject; the rest is the body.").color(Palette::INK_3),
    );

    let has_message = !state.ui.dlg.reword_message.trim().is_empty();
    if !has_message {
        // Stated on the field, not in a box the developer has to find. The
        // refusal that actually matters is in `open_rewrite_preflight`, where it
        // is enforced for every path; this is why the developer is not looking at
        // a dialog that will not open.
        // The same sentence the refusal quotes, from the one constant that owns
        // it: two surfaces, one statement of the bound.
        ui.colored_label(
            Palette::STATE_WARNING,
            turbogit_app::state::EMPTY_COMMIT_MESSAGE,
        );
    }

    ui.add_space(6.0);
    widgets::dialog_footer(ui, |ui| {
        if widgets::compact_button(ui, "Cancel").clicked() {
            close(state);
        }
        if widgets::compact_button_enabled(ui, "Reword this commit", has_message).clicked() {
            let message = state.ui.dlg.reword_message.trim_end().to_owned();
            close(state);
            state.open_rewrite_preflight(
                &draft.root,
                &draft.commit,
                HistoryVerb::Reword { message },
            );
        }
    });
}
