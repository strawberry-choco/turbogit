//! Submodules tool tab (issue 14, screen 01): a real browser over the
//! focused root's registered submodules — path, pinned vs recorded commit,
//! lifecycle status — with update and deinit actions. Reads the cached
//! submodule list (`RootCaches::submodules`), which the shell keeps filled
//! for the focused root.

use egui::{Align, FontFamily, FontId, Layout, RichText, ScrollArea, Ui};
use turbogit_app::state::{AppState, PendingConfirm};
use turbogit_domain::model::{Submodule, SubmoduleState};

use crate::theme::Palette;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    ui.add_space(8.0);
    ui.label(
        RichText::new("SUBMODULES")
            .strong()
            .font(FontId::new(11.0, FontFamily::Proportional))
            .color(Palette::INK_3),
    );
    ui.add_space(4.0);

    let Some(id) = state.selected_root.clone() else {
        return;
    };
    // Snapshot the list so the row callbacks can borrow `state` mutably.
    let submodules: Vec<Submodule> = state.caches.submodules(&id).unwrap_or(&[]).to_vec();
    let loaded = state.caches.submodules(&id).is_some();
    if !loaded {
        ui.weak("Loading submodules…");
    }

    ScrollArea::vertical().show(ui, |ui| {
        if submodules.is_empty() && loaded {
            ui.weak("No registered submodules in this repository.");
        }
        for sub in &submodules {
            submodule_row(ui, state, sub);
        }
    });
}

/// One submodule row: path, pinned vs recorded commit, status, and the
/// update / deinit actions.
fn submodule_row(ui: &mut Ui, state: &mut AppState, sub: &Submodule) {
    let name = sub
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<submodule>")
        .to_string();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            RichText::new(sub.path.display().to_string())
                .font(FontId::new(12.0, FontFamily::Proportional))
                .color(Palette::INK),
        );
        commit_summary(ui, sub);
        status_chip(ui, sub.state);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button(format!("Deinit {name}")).clicked() {
                state.ui.confirm = Some(PendingConfirm::DeinitSubmodule {
                    path: sub.path.clone(),
                });
            }
            if ui.button(format!("Update {name}")).clicked() {
                let init = sub.state == SubmoduleState::Uninitialized;
                state.update_submodule(sub.path.clone(), init);
            }
        });
    });
    ui.add_space(2.0);
}

/// The pinned-vs-recorded commit summary: one short sha when the checkout
/// matches the record, both sides labeled when they diverge.
fn commit_summary(ui: &mut Ui, sub: &Submodule) {
    let text = match (&sub.head, &sub.recorded) {
        (Some(h), Some(r)) if h != r => {
            format!("pinned {} → recorded {}", short(h), short(r))
        }
        (Some(h), _) => short(h).to_string(),
        (None, Some(r)) => format!("recorded {}", short(r)),
        (None, None) => String::new(),
    };
    if !text.is_empty() {
        ui.label(
            RichText::new(text)
                .font(FontId::new(11.0, FontFamily::Monospace))
                .color(Palette::INK_2),
        );
    }
}

/// Status chip color/label per submodule state (issue 14).
fn status_chip(ui: &mut Ui, state: SubmoduleState) {
    let (text, color) = match state {
        SubmoduleState::UpToDate => ("Up to date", Palette::STATE_SUCCESS),
        SubmoduleState::NeedsUpdate => ("Needs update", Palette::STATE_WARNING),
        SubmoduleState::Uninitialized => ("Uninitialized", Palette::INK_3),
        SubmoduleState::Conflicted => ("Conflicts", Palette::STATE_ERROR),
    };
    ui.label(
        RichText::new(text)
            .font(FontId::new(11.0, FontFamily::Proportional))
            .color(color),
    );
}

/// First 7 hex chars of a commit sha (the git-short sha the UI shows).
fn short(sha: &str) -> &str {
    &sha[..7.min(sha.len())]
}
