use egui::{Align, Layout, RichText, Ui};
use turbogit_app::state::{AppState, PendingConfirm};
use turbogit_domain::model::{DisplayName, Submodule, SubmoduleState};

use crate::theme::{Palette, RepoState, TYPE_CONTROL, data_font};
use crate::ui::column_table::{self, PaneTable};
use crate::ui::components;
use crate::ui::widgets::{self, PaneColumn};

const SUBMODULE_COLUMNS: [PaneColumn; 5] = [
    PaneColumn::start("PATH", 8.0),
    PaneColumn::start("CHECKED OUT", 244.0),
    PaneColumn::start("RECORDED", 344.0),
    PaneColumn::start("STATUS", 444.0),
    PaneColumn::end("ACTIONS", 0.0),
];

pub fn show(ui: &mut Ui, state: &mut AppState) {
    column_table::column_table_pane(
        ui,
        state,
        PaneTable {
            title: "SUBMODULES",
            columns: &SUBMODULE_COLUMNS,
            loading: "Loading submodules…",
            empty: "No registered submodules in this repository.",
            cached: |state, id| state.caches.submodules(id).map(<[_]>::to_vec),
            actions: &mut |_ui, _state| {},
            row: &mut submodule_row,
        },
    );
}

fn submodule_row(ui: &mut Ui, state: &mut AppState, sub: &Submodule) {
    let name = sub.path.display_name_or("<submodule>");
    let diverged = diverged(sub);
    let recorded = match (&sub.head, &sub.recorded) {
        (Some(h), Some(r)) if h == r => None,
        _ => sub.recorded.as_deref(),
    };
    ui.horizontal(|ui| {
        widgets::column_cell(ui, &SUBMODULE_COLUMNS, 0, |ui| {
            column_table::path_cell(ui, &SUBMODULE_COLUMNS, &sub.path)
        });
        widgets::column_cell(ui, &SUBMODULE_COLUMNS, 1, |ui| {
            commit_cell(ui, sub.head.as_deref(), diverged)
        });
        widgets::column_cell(ui, &SUBMODULE_COLUMNS, 2, |ui| {
            commit_cell(ui, recorded, false)
        });
        widgets::column_cell(ui, &SUBMODULE_COLUMNS, 3, |ui| {
            status_mark(ui, sub.state);
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button(format!("Deinit {name}")).clicked() {
                state.ui.confirm = Some(PendingConfirm::DeinitSubmodule {
                    path: sub.path.clone(),
                });
            }
            let verb = action_verb(sub.state);
            if ui.button(format!("{verb} {name}")).clicked() {
                let init = sub.state == SubmoduleState::Uninitialized;
                state.update_submodule(sub.path.clone(), init);
            }
        });
    });
    ui.add_space(2.0);
}

/// True when the checkout is at a different commit than the superproject
/// records, `git submodule status`'s `+`.
///
/// **Both sides have to be present.** An uninitialised submodule has a record
/// and no checkout, which is not a divergence: there is nothing on the checkout
/// side to have moved, and colouring the record as though it had would report a
/// disagreement where the pane knows only an absence.
fn diverged(sub: &Submodule) -> bool {
    matches!((&sub.head, &sub.recorded), (Some(h), Some(r)) if h != r)
}

fn commit_cell(ui: &mut Ui, reference: Option<&str>, divergent: bool) {
    let Some(reference) = reference else {
        return;
    };
    let ink = if divergent {
        RepoState::Diverged.color()
    } else {
        Palette::INK_2
    };
    ui.label(
        RichText::new(widgets::short_commit_ref(reference))
            .font(data_font(TYPE_CONTROL))
            .color(ink),
    );
}

/// The row action's verb, which is the whole of the Init/Update decision.
///
/// The operation was already an init on an uninitialised submodule, the row has
/// always passed `init: true` for that state, so the control was labelled
/// `Update` while doing something else, and a reader had to know that to predict
/// what pressing it would do. The label now says what the press does.
///
/// It is a function of the **state**, not of the divergence: an uninitialised
/// submodule is the one state whose update *is* an init, and every other state
/// keeps the update verb, including the needs-update one whose whole purpose is
/// to check the record back out.
fn action_verb(state: SubmoduleState) -> &'static str {
    match state {
        SubmoduleState::Uninitialized => "Init",
        _ => "Update",
    }
}

fn status_mark(ui: &mut Ui, state: SubmoduleState) {
    let word = match state {
        SubmoduleState::UpToDate => "Up to date",
        SubmoduleState::NeedsUpdate => "Needs update",
        SubmoduleState::Uninitialized => "Uninitialized",
        SubmoduleState::Conflicted => "Conflicts",
    };
    components::state_mark(ui, word, repo_state(state).color());
}

fn repo_state(state: SubmoduleState) -> RepoState {
    match state {
        SubmoduleState::UpToDate => RepoState::Clean,
        SubmoduleState::NeedsUpdate => RepoState::Diverged,
        SubmoduleState::Conflicted => RepoState::Conflict,
        SubmoduleState::Uninitialized => RepoState::Uninitialized,
    }
}
