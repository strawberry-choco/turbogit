//! The divergence offer: a cascade that landed in some repositories and
//! failed in others. The offer names both sets and rolls the succeeded ones
//! back, or keeps them.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::state::{AppState, BranchDivergence, DivergentRoot};

use crate::bridge::AppHandle;
use crate::screens::shelf::counted;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::ButtonVariant;
use crate::ui::dialog::{DialogAction, DialogProps, dialog};

fn is_stale(state: &AppState, offer: &BranchDivergence) -> bool {
    offer
        .changed
        .iter()
        .chain(offer.failed.iter())
        .any(|root| state.multi.by_id(&root.root).is_none())
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
    let repositories = if offer.changed.len() == 1 {
        "repository"
    } else {
        "repositories"
    };
    format!(
        "You may roll back the {repositories} where it succeeded, so the branches do not diverge."
    )
}

fn rollback_label(offer: &BranchDivergence) -> String {
    format!(
        "Roll back {}",
        counted(offer.changed.len(), "repository", "repositories")
    )
}

fn root_list(sentence: String, roots: &[DivergentRoot], with_reason: bool) -> impl IntoElement {
    let t = theme();
    let mut list = view().flex_col().gap(px(2.0)).child(
        text(sentence)
            .text_size(px(data::TYPE_CONTROL))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(t.foreground),
    );
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
        list = list.child(
            text(line)
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    }
    list
}

/// The divergence offer, above every other layer.
pub fn divergence_layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.branch_divergence.is_some()),
        move || divergence_body(handle.clone()),
    )
}

fn divergence_body(handle: AppHandle) -> AnyElement {
    let Some(offer) = handle.peek(|s| s.ui.branch_divergence.clone()) else {
        return view().into_element();
    };
    let stale = handle.peek(|s| is_stale(s, &offer));
    if stale {
        let stale_handle = handle.clone();
        stale_handle.update(|s| s.ui.branch_divergence = None);
        return view().into_element();
    }
    let t = theme();
    let body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::divergence("offer"))
        .child(
            text(summary(&offer))
                .text_size(px(data::TYPE_BODY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(data::STATE_WARNING)
                .test_id(&test_ids::divergence("summary")),
        )
        .child(root_list(failed_sentence(&offer), &offer.failed, true))
        .child(root_list(succeeded_sentence(&offer), &offer.changed, false))
        .child(
            text(rollback_reason(&offer))
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    let rollback_handle = handle.clone();
    let rollback = rollback_label(&offer);
    let keep_handle = handle.clone();
    dialog(
        DialogProps::new("branches-diverged", "Branches diverged")
            .on_cancel(move || keep_handle.update(|s| s.keep_divergent_branches()))
            .child(body)
            .with_action(DialogAction::new(
                rollback,
                ButtonVariant::Primary,
                move || {
                    rollback_handle.update(|s| {
                        s.rollback_divergent_roots();
                    });
                },
            ))
            .with_action(DialogAction::new(
                "Keep divergent",
                ButtonVariant::Secondary,
                {
                    let keep = handle.clone();
                    move || keep.update(|s| s.keep_divergent_branches())
                },
            )),
    )
    .into_element()
}

/// The Divergence route: the offer inline, or the empty state.
pub fn divergence_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let show = handle.clone();
    let show2 = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("divergence"))
        .child(
            text("Divergence")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(when(
            move || show.read(|s| s.ui.branch_divergence.is_some()),
            move || {
                let hold = handle.clone();
                view()
                    .flex_col()
                    .gap(px(data::SPACING_Y))
                    .test_id(&test_ids::divergence("inline"))
                    .child(divergence_inline(hold))
            },
        ))
        .child(when(
            move || show2.read(|s| s.ui.branch_divergence.is_none()),
            move || {
                text("No branch divergence to show.")
                    .text_size(px(data::TYPE_BODY))
                    .text_color(t.muted_foreground)
            },
        ))
}

fn divergence_inline(handle: AppHandle) -> impl IntoElement {
    let offer = handle.peek(|s| s.ui.branch_divergence.clone());
    let mut body = view().flex_col().gap(px(data::SPACING_Y));
    if let Some(offer) = offer {
        body = body
            .child(
                text(summary(&offer))
                    .text_size(px(data::TYPE_BODY))
                    .text_color(data::STATE_WARNING),
            )
            .child(root_list(failed_sentence(&offer), &offer.failed, true))
            .child(root_list(succeeded_sentence(&offer), &offer.changed, false));
    }
    body
}
