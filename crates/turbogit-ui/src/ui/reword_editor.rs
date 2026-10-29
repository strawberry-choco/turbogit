use egui::{RichText, TextEdit, Ui, WidgetInfo, WidgetType};
use turbogit_app::state::{AppState, HistoryVerb, RewordDraft};

use crate::theme::Palette;
use crate::ui::widgets;

const EDITOR_LABEL: &str = "REWORD-MESSAGE";

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

fn body(ui: &mut Ui, state: &mut AppState, draft: &RewordDraft) {
    ui.label(
        RichText::new(widgets::short_commit_ref(&draft.commit))
            .font(crate::theme::data_font(crate::theme::TYPE_BODY))
            .color(Palette::INK_2),
    );
    ui.add_space(4.0);

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
