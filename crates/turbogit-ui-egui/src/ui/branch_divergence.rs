use crate::theme::Palette;
use crate::ui::components::{KitButton, counted, kit_button};
use crate::ui::widgets;
use egui::{RichText, Ui};
use turbogit_app::state::{AppState, BranchDivergence, DivergentRoot};

pub fn render(ui: &mut Ui, state: &mut AppState) {
    let Some(offer) = state.ui.branch_divergence.clone() else {
        return;
    };
    if is_stale(state, &offer) {
        state.ui.branch_divergence = None;
        return;
    }

    let mut open = true;
    egui::Window::new("Branches diverged")
        .id(egui::Id::new("branch_divergence"))
        .open(&mut open)
        .default_width(520.0)
        .show(&ui.ctx().clone(), |ui| {
            ui.label(
                RichText::new(summary(&offer))
                    .color(Palette::STATE_WARNING)
                    .strong(),
            );
            root_list(ui, &failed_sentence(&offer), &offer.failed, true);
            root_list(ui, &succeeded_sentence(&offer), &offer.changed, false);
            ui.label(RichText::new(rollback_reason(&offer)).color(Palette::T_SECONDARY));
            controls(ui, state, &offer);
        });
    if !open {
        state.keep_divergent_branches();
    }
}

fn controls(ui: &mut Ui, state: &mut AppState, offer: &BranchDivergence) {
    let rollback = rollback_label(offer);
    widgets::dialog_footer(ui, |ui| {
        if kit_button(ui, KitButton::Primary, &rollback).clicked() {
            state.rollback_divergent_roots();
            return;
        }
        if kit_button(ui, KitButton::Secondary, "Keep divergent").clicked() {
            state.keep_divergent_branches();
        }
    });
}

fn root_list(ui: &mut Ui, sentence: &str, roots: &[DivergentRoot], with_reason: bool) {
    ui.label(RichText::new(sentence).strong());
    for root in roots {
        let line = if with_reason {
            format!(
                "· {} — still on {} · {}",
                root.name,
                root.branch,
                root.reason.as_deref().unwrap_or("no reason reported")
            )
        } else {
            format!("· {} — now on {}", root.name, root.branch)
        };
        ui.label(RichText::new(line).color(Palette::T_SECONDARY));
    }
}

fn repositories(n: usize) -> &'static str {
    if n == 1 { "repository" } else { "repositories" }
}

fn summary(offer: &BranchDivergence) -> String {
    format!(
        "{} did not land in {} of {}.",
        offer.operation,
        offer.failed.len(),
        offer.failed.len() + offer.changed.len()
    )
}

fn succeeded_sentence(offer: &BranchDivergence) -> String {
    format!(
        "However, {} has succeeded for the following {}:",
        offer.operation,
        counted(offer.changed.len(), "repository", "repositories")
    )
}

fn failed_sentence(offer: &BranchDivergence) -> String {
    format!(
        "It failed in the following {}:",
        counted(offer.failed.len(), "repository", "repositories")
    )
}

fn rollback_reason(offer: &BranchDivergence) -> String {
    format!(
        "You may roll back the {} where it succeeded, so the branches do not diverge.",
        repositories(offer.changed.len())
    )
}

fn rollback_label(offer: &BranchDivergence) -> String {
    format!(
        "Roll back {}",
        counted(offer.changed.len(), "repository", "repositories")
    )
}

fn is_stale(state: &AppState, offer: &BranchDivergence) -> bool {
    offer
        .changed
        .iter()
        .chain(offer.failed.iter())
        .any(|r| state.multi.by_id(&r.root).is_none())
}
