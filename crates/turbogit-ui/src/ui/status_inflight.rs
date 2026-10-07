//! The status bar's in-flight operation item (rebased-parity ticket 16, surface
//! `04-integrate` §4.8/§5.8).
//!
//! The reference announces an operation a repository is holding **in the status
//! bar, one announcement per root**, and puts its recovery verbs there: the doc
//! says you may "click in the status bar" to abort or continue, and the same
//! verbs stay reachable from a menu once the transient notification has been
//! dismissed. TurboGit had the lifecycle machinery — `OwnedOperation`,
//! `OpKind`, the executor's `--abort` / `--continue` — and no presentation of
//! it, so a paused rebase was only visible in the conflict resolver.
//!
//! ## This is not ticket 11's status-hint channel, and the difference is load-bearing
//!
//! The bar has two text sources now. [`crate::ui::widgets::take_status_hint`] is
//! a **one-frame hover echo**: exactly one producer (a disabled menu row's
//! `insert_temp`) and exactly one consumer (`ui/shell.rs`), and anything that
//! hovers next frame replaces it. This module is the opposite: a **persistent**
//! reading of the repository's own state, painted every frame the bar is painted,
//! with no channel and no slot to fight over. Routing an in-flight operation
//! through the hint slot would make the announcement flicker and drop the moment
//! the pointer moved — and would break the 1-producer/1-consumer invariant that
//! makes the hint provable. It stays here precisely so that slot keeps exactly
//! one producer.
//!
//! ## The state is git's, not ours
//!
//! Each item comes from [`integrate_service::owning_operation`], which reads the
//! marker git itself leaves (`rebase-merge`, `CHERRY_PICK_HEAD`, `MERGE_HEAD`,
//! `REVERT_HEAD`). That is deliberate over an app-side flag: an operation paused
//! on a conflict survives a restart and can be started outside this app, and a
//! flag set at dispatch would announce nothing in both cases. It is also
//! **per root by construction**, which is the property a multi-root client is
//! most likely to get wrong — one global "busy with a rebase" boolean would paint
//! the right words in the wrong root's announcement.
//!
//! Reading it is a filesystem `exists()` on `<gitdir>`, the same source
//! `log_window.rs` and `dialogs.rs` already consult for the rewrite gate; no git
//! process runs here, so the "UI never calls git" rule is intact.

use egui::{Color32, CornerRadius, Response, RichText};

use turbogit_app::operation::{LifecycleVerb, Operation};
use turbogit_app::state::AppState;
use turbogit_domain::model::{Root, RootId};
use turbogit_services::integrate_service::{OwnedOperation, owning_operation};

use crate::theme::{self, Palette, chrome_font};

/// One root's in-flight announcement: who holds it, and what holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InFlight {
    pub root: RootId,
    /// The root's name as the announcement says it.
    pub name: String,
    pub owner: OwnedOperation,
}

/// The roots currently holding an operation, in registration order.
pub fn items(roots: &[Root]) -> Vec<InFlight> {
    items_via(roots, &|root| owning_operation(root.id.as_path()))
}

/// [`items`] over an injected owner read — the same seam
/// `AggregatedStatus::compute` uses for ahead/behind, so the per-root rule is
/// unit-testable without a working tree.
pub fn items_via(
    roots: &[Root],
    owner_of: &dyn Fn(&Root) -> Option<OwnedOperation>,
) -> Vec<InFlight> {
    roots
        .iter()
        .filter_map(|root| {
            owner_of(root).map(|owner| InFlight {
                root: root.id.clone(),
                name: root.id.name(),
                owner,
            })
        })
        .collect()
}

impl InFlight {
    /// The line the bar paints: the bundle's `Cherry-picking in {0}`, one per
    /// root, with the root named because the announcement is per root.
    pub fn announce(&self) -> String {
        let name = &self.name;
        match self.owner {
            OwnedOperation::CherryPick => format!("Cherry-picking in {name}"),
            OwnedOperation::Rebase => format!("Rebasing in {name}"),
            OwnedOperation::Merge => format!("Merging in {name}"),
            OwnedOperation::Revert => format!("Reverting in {name}"),
        }
    }

    /// The verbs this state offers, in paint order.
    pub fn verbs(&self) -> &'static [LifecycleVerb] {
        LifecycleVerb::offered_for(self.owner)
    }
}

/// The root a menu-route verb acts on.
///
/// Two roots can hold operations at once, so the menu route has to name one. The
/// focused root answers first when it holds a state that offers the verb; a
/// focused root that cannot honour it does not **veto** it — the search moves on
/// to the root that can, which is what makes `Skip Commit` do something when the
/// rebase is in the other repository. Nothing offers it: `None`, and the caller
/// says so rather than acting on an arbitrary root.
pub fn target_for(state: &AppState, verb: LifecycleVerb) -> Option<(RootId, OwnedOperation)> {
    let found = items(&state.multi.roots);
    let offers = |item: &InFlight| LifecycleVerb::offered_for(item.owner).contains(&verb);
    found
        .iter()
        .find(|item| state.selected_root.as_ref() == Some(&item.root) && offers(item))
        .or_else(|| found.iter().find(|item| offers(item)))
        .map(|item| (item.root.clone(), item.owner))
}

/// Paint every holding root's item and run its verbs.
///
/// **Two roots mid-operation at the same time** is the layout the reference
/// never states: the string is per root, the bar is one 24 px band
/// ([`super::shell::STATUS_BAR_HEIGHT`]), and stacking a row per root would
/// resize the shell's frozen chrome. The fallback chosen here is **side-by-side
/// in registration order**, each item naming its own root, recorded as a
/// divergence in the ticket.
pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    let mut first = true;
    for item in items(&state.multi.roots) {
        if !first {
            // The bar's one quiet separator ink (`INK_2`, the step up the raised
            // `SURFACE` band forces for every line in it).
            ui.label(
                RichText::new("·")
                    .font(chrome_font(theme::TYPE_CONTROL))
                    .color(Palette::INK_2),
            );
        }
        first = false;
        ui.label(
            RichText::new(item.announce())
                .font(chrome_font(theme::TYPE_CONTROL))
                .color(Palette::STATE_WARNING),
        );
        // The verbs, with the destructive one given its own air ahead of the
        // pair. `item.verbs()` is the state machine: a cherry-pick paints two
        // controls and a rebase paints three, so the bar models which resolution
        // exists instead of decorating the state with every word it knows.
        for (idx, verb) in item.verbs().iter().enumerate() {
            // One gap token, applied once: between the abort and what follows it.
            if idx == 1 {
                ui.add_space(theme::STATUS_INFLIGHT_ABORT_GAP);
            }
            if verb_button(ui, *verb).clicked() {
                state.dispatch(Operation::Resolve {
                    root: item.root.clone(),
                    owner: item.owner,
                    verb: *verb,
                });
            }
        }
    }
}

/// One verb's control.
///
/// Not [`super::components::kit_button`]: that kit is 28 px tall
/// (`KIT_BUTTON_H`) and this band is 24 px, so a kit control here would be
/// clipped top and bottom by the panel's own rect. The fills and inks below are
/// the kit's role vocabulary — accent for the forward-going verb, the raised rung
/// for the third resolution, the danger tone for the one that throws the
/// operation away — read from the token layer, at the dense metrics the band
/// already sets.
fn verb_button(ui: &mut egui::Ui, verb: LifecycleVerb) -> Response {
    let (fill, ink) = role_colors(verb);
    ui.add(
        egui::Button::new(
            RichText::new(verb.label())
                .font(chrome_font(theme::TYPE_CONTROL))
                .color(ink),
        )
        .fill(fill)
        .corner_radius(CornerRadius::same(theme::CONTROL_RADIUS)),
    )
    .on_hover_text(hover(verb))
}

/// The three verbs' fill-and-ink roles, one place each.
fn role_colors(verb: LifecycleVerb) -> (Color32, Color32) {
    match verb {
        LifecycleVerb::Abort => (Palette::SURFACE_INFLIGHT_ABORT, Palette::DANGER),
        LifecycleVerb::Continue => (Palette::ACCENT, Palette::BRAND_INK),
        LifecycleVerb::Skip => (Palette::SURFACE_2, Palette::INK),
    }
}

/// What each verb does to the operation, because the bar's one-word labels
/// cannot say it and the destructive reading of `Abort` is the whole hazard:
/// `Abort` discards the entire operation, `Continue` retries the commit that
/// stopped it, `Skip` abandons THAT commit and replays the rest.
///
/// A free function rather than a method on [`LifecycleVerb`]: the verb type is
/// the app's, and an inherent impl on a foreign type is not this crate's to add.
pub fn hover(verb: LifecycleVerb) -> &'static str {
    match verb {
        LifecycleVerb::Abort => "Abort — give up the whole operation",
        LifecycleVerb::Continue => "Continue — carry on from the commit that stopped it",
        LifecycleVerb::Skip => "Skip — abandon this commit and replay the rest",
    }
}
