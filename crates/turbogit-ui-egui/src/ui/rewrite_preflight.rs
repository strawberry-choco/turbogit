use egui::{Align, Layout, RichText, Ui};
use turbogit_app::history_editor::{self, PlanPreview, plan_preview};
use turbogit_app::operation::Operation;
use turbogit_app::reselection::{RewrittenAnchor, subject_of_message};
use turbogit_app::state::{AppState, HistoryVerb, RewritePreflight};
use turbogit_domain::model::RebasePlanEntry;

use crate::theme::Palette;
use crate::ui::components;
use crate::ui::widgets;

const REPLAYED_ROWS: usize = 12;

fn window_title(verb: &HistoryVerb) -> &'static str {
    match verb {
        HistoryVerb::Drop => "Drop commit",
        HistoryVerb::Reword { .. } => "Reword commit",
    }
}

fn group_title(verb: &HistoryVerb) -> &'static str {
    match verb {
        HistoryVerb::Drop => "DROPPED COMMIT",
        HistoryVerb::Reword { .. } => "REWORDED COMMIT",
    }
}

fn counts_badge(verb: &HistoryVerb, preview: &PlanPreview) -> String {
    match verb {
        HistoryVerb::Drop => format!("{} → {} COMMITS", preview.before, preview.after),
        HistoryVerb::Reword { .. } => {
            let kept = preview.kept.len();
            format!(
                "{} REWRITTEN",
                components::counted(kept, "COMMIT", "COMMITS")
            )
        }
    }
}

fn confirm_label(verb: &HistoryVerb) -> &'static str {
    match verb {
        HistoryVerb::Drop => "Drop this commit",
        HistoryVerb::Reword { .. } => "Reword this commit",
    }
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let Some(pf) = state.ui.dlg.rewrite_preflight.clone() else {
        return;
    };
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new(window_title(&pf.verb))
        .open(&mut open)
        .default_width(460.0)
        .show(&ctx, |ui| body(ui, state, &pf));
    if !open {
        close(state);
    }
}

fn close(state: &mut AppState) {
    state.ui.dialog = None;
    state.ui.dlg.rewrite_preflight = None;
}

fn body(ui: &mut Ui, state: &mut AppState, pf: &RewritePreflight) {
    let preview = plan_preview(&pf.plan);

    acted_commit(ui, pf);
    ui.add_space(8.0);
    replayed_commits(ui, &pf.verb, &preview);
    ui.add_space(6.0);

    widgets::cautions_rail(ui, &pf.cautions);
    widgets::recovery_note(ui, &state.rewrite_backup_ref());

    ui.add_space(6.0);
    footer(ui, state, pf);
}

fn acted_commit(ui: &mut Ui, pf: &RewritePreflight) {
    widgets::group_title(ui, group_title(&pf.verb));
    let subject = pf
        .plan
        .iter()
        .find(|e| e.commit == pf.commit)
        .map(|e| e.subject.clone())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(widgets::short_commit_ref(&pf.commit))
                .font(crate::theme::data_font(crate::theme::TYPE_BODY))
                .color(Palette::INK),
        );
        ui.label(RichText::new(subject).color(Palette::INK));
    });
    if let Some(new_subject) = pf.verb.reworded_subject() {
        ui.horizontal(|ui| {
            ui.label(RichText::new("→").color(Palette::INK_3));
            ui.label(RichText::new(new_subject).color(Palette::INK));
        });
    }
}

fn replayed_commits(ui: &mut Ui, verb: &HistoryVerb, preview: &PlanPreview) {
    ui.horizontal(|ui| {
        widgets::group_title(ui, &format!("REPLAYED COMMITS · {}", preview.kept.len()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let counts = counts_badge(verb, preview);
            ui.label(RichText::new(counts).monospace().color(Palette::BRAND));
        });
    });

    let kept = &preview.kept;
    let elided = kept.len().saturating_sub(REPLAYED_ROWS);
    let head = REPLAYED_ROWS / 2;
    let rows: Vec<&RebasePlanEntry> = if elided == 0 {
        kept.iter().collect()
    } else {
        kept.iter()
            .take(head)
            .chain(kept.iter().skip(kept.len() - (REPLAYED_ROWS - head)))
            .collect()
    };
    for (index, entry) in rows.iter().enumerate() {
        if elided > 0 && index == head {
            ui.label(
                RichText::new(format!("… {elided} commits in between …")).color(Palette::INK_3),
            );
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(widgets::short_commit_ref(&entry.commit)).monospace());
            ui.label(RichText::new(&entry.subject).color(Palette::INK));
        });
    }
}

fn footer(ui: &mut Ui, state: &mut AppState, pf: &RewritePreflight) {
    widgets::dialog_footer(ui, |ui| {
        ui.label(RichText::new(history_editor::estimate(&pf.plan)).color(Palette::INK_3));
        if widgets::compact_button(ui, "Cancel").clicked() {
            close(state);
        }
        if widgets::compact_button(ui, confirm_label(&pf.verb)).clicked() {
            dispatch(state, pf);
            close(state);
        }
    });
}

fn dispatch(state: &mut AppState, pf: &RewritePreflight) {
    let branch = state
        .multi
        .by_id(&pf.root)
        .and_then(|r| r.current_branch.clone())
        .unwrap_or_default();
    let settings = state.settings.clone();
    let anchor = match &pf.verb {
        HistoryVerb::Drop => RewrittenAnchor::Dropped,
        HistoryVerb::Reword { message } => RewrittenAnchor::Reworded {
            subject: subject_of_message(message).to_owned(),
        },
    };
    state.arm_reselect_after_rewrite(pf.root.clone(), pf.plan.clone(), pf.commit.clone(), anchor);
    state.ui.rewrite_undo_pending = Some(turbogit_app::state::RewriteUndoPending {
        root: pf.root.clone(),
        verb: match &pf.verb {
            HistoryVerb::Drop => turbogit_app::state::RewriteUndoVerb::Drop,
            HistoryVerb::Reword { .. } => turbogit_app::state::RewriteUndoVerb::Reword,
        },
        commit: pf.commit.clone(),
    });
    let operation = match &pf.verb {
        HistoryVerb::Drop => Operation::drop_commit(&pf.root, &branch, &pf.commit, &settings),
        HistoryVerb::Reword { message } => {
            Operation::reword_commit(&pf.root, &branch, &pf.commit, message, &settings)
        }
    };
    state.dispatch(operation);
}
