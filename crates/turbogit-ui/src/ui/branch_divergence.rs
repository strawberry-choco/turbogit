//! The follow-up offer after a multi-root branch run left the roots disagreeing.
//!
//! A run that lands in one repository and fails in another has not produced a
//! cosmetic difference: the roots now stand on different branches, and nothing
//! puts that back except rolling the landed ones back by hand. So the surface
//! states both halves of the fact, names each repository, and offers the one
//! verb that repairs it.
//!
//! # Why this is a floating window and not a [`turbogit_app::state::Dialog`]
//!
//! It arrives from a completion event, the same moment the Push dialog has
//! already closed — which is the reason [`super::push_rejected`] is not a dialog
//! variant either, and the same reason applies here. A `Dialog` slot holds one
//! modal at a time, so an offer that stole the slot from a dialog the developer
//! still has open would destroy their input; the rejection surface already
//! avoids that, and this follows it.
//!
//! # The hazard: an offer that cannot be dismissed
//!
//! A declined divergence stays a divergence, so the window has to close on
//! *every* path: both controls and the frame's own close button clear
//! [`AppState`]'s hold on it. The close path is why `open` is read back after
//! `show` rather than ignored — a surface that repaints from state it never
//! clears is on screen forever, and the decline is the case that regresses.

use crate::theme::Palette;
use crate::ui::components::{KitButton, kit_button};
use crate::ui::widgets;
use egui::{RichText, Ui};
use turbogit_app::state::{AppState, BranchDivergence, DivergentRoot};

/// Paint the divergence offer, whenever a completed branch run left the roots
/// on different branches and a rollback is still owed an answer.
pub fn render(ui: &mut Ui, state: &mut AppState) {
    let Some(offer) = state.ui.branch_divergence.clone() else {
        return;
    };
    // Staleness is judged on state, never on what was painted last frame: a
    // dropped root means a rollback that would act on a repository the app no
    // longer has a snapshot of. The divergence itself stays in the activity
    // feed, which is where it was recorded when the run completed.
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
        // The frame's close button is a decline, not a vanishing: the answer has
        // to be as durable as the answer to either control, or a later reader
        // cannot tell a kept divergence from an unnoticed one.
        state.keep_divergent_branches();
    }
}

/// The controls: the repair, and the refusal to take it.
///
/// The two paint from different fills on purpose — [`KitButton::Primary`] is
/// `Palette::ACCENT`, [`KitButton::Secondary`] the raised surface rung — because
/// the choice being offered is between repairing the fleet and leaving it split.
/// Identical rects would make the riskier answer indistinguishable.
fn controls(ui: &mut Ui, state: &mut AppState, offer: &BranchDivergence) {
    let rollback = rollback_label(offer);
    widgets::dialog_footer(ui, |ui| {
        // The footer lays out right-to-left, so the control added FIRST sits at
        // the row's end — which is where the captured `Cancel | Create` order
        // puts the affirmative verb, and why the rollback is added before the
        // refusal rather than after it in the code.
        if kit_button(ui, KitButton::Primary, &rollback).clicked() {
            // `rollback_divergent_roots` clears the offer, refreshes every root
            // and records the outcome; an incomplete rollback leaves the offer
            // spent on purpose, because the retry lives on the run's history
            // row — the record stays un-rolled-back, so its `Rollback` control
            // is still painted there.
            state.rollback_divergent_roots();
            return;
        }
        if kit_button(ui, KitButton::Secondary, "Keep divergent").clicked() {
            state.keep_divergent_branches();
        }
    });
}

/// One section: its lead sentence, then one line per root.
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

/// The count-pluralised noun, because the bundle's own sentences are pluralised
/// and a hardcoded "repositories" reads wrong on the two-root case that is the
/// common one.
fn repositories(n: usize) -> &'static str {
    if n == 1 { "repository" } else { "repositories" }
}

fn counted(n: usize) -> String {
    format!("{n} {}", repositories(n))
}

/// The headline: what was attempted, and that it did not land everywhere.
fn summary(offer: &BranchDivergence) -> String {
    format!(
        "{} did not land in {} of {}.",
        offer.operation,
        offer.failed.len(),
        offer.failed.len() + offer.changed.len()
    )
}

/// The partial-success sentence — the reference's own shape: `However, checkout
/// has succeeded for the following {1 repository|2 repositories}`.
fn succeeded_sentence(offer: &BranchDivergence) -> String {
    format!(
        "However, {} has succeeded for the following {}:",
        offer.operation,
        counted(offer.changed.len())
    )
}

/// The other half, stated as its own list so the two states are never merged
/// into one "partially failed" sentence nobody can act on.
fn failed_sentence(offer: &BranchDivergence) -> String {
    format!(
        "It failed in the following {}:",
        counted(offer.failed.len())
    )
}

/// The reason the offer gives itself, which is the whole point of the
/// reference's wording: the rollback is not a retry, it is what stops the
/// branches from diverging.
fn rollback_reason(offer: &BranchDivergence) -> String {
    format!(
        "You may roll back the {} where it succeeded, so the branches do not diverge.",
        repositories(offer.changed.len())
    )
}

/// The affirmative control's label, naming how many roots it is about to move —
/// the number the sentence above just made concrete.
fn rollback_label(offer: &BranchDivergence) -> String {
    format!("Roll back {}", counted(offer.changed.len()))
}

/// Every root the offer names is still a registered root.
fn is_stale(state: &AppState, offer: &BranchDivergence) -> bool {
    offer
        .changed
        .iter()
        .chain(offer.failed.iter())
        .any(|r| state.multi.by_id(&r.root).is_none())
}
