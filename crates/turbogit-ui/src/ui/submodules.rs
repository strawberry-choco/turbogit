//! Submodules tool tab (issue 14, screen 01): a real browser over the
//! focused root's registered submodules — path, checked-out vs recorded
//! commit, lifecycle status — with init/update and deinit actions. Reads the
//! cached submodule list (`RootCaches::submodules`), which the shell keeps
//! filled for the focused root.
//!
//! The pane's chrome is entirely borrowed, exactly as the worktrees pane's is:
//! the header is [`widgets::pane_header`], the labels are the shared
//! [`widgets::column_header`] row, the offsets both of those and the rows below
//! read are the one [`SUBMODULE_COLUMNS`] table, the status is the shared mark
//! pair [`components::state_mark`], and the status's colour comes from
//! [`crate::theme::RepoState::color`] like every other state in the app. Nothing
//! here decides a colour, a radius or an x offset of its own.

use egui::{Align, Layout, RichText, ScrollArea, Ui, Vec2};
use turbogit_app::state::{AppState, PendingConfirm};
use turbogit_domain::model::{Submodule, SubmoduleState};

use crate::theme::{Palette, RepoState, TYPE_BODY, TYPE_CONTROL, data_font};
use crate::ui::components;
use crate::ui::widgets::{self, PaneColumn};

/// The Submodules pane's columns: one table, read by **both** the shared
/// column-header row and every data row below it, for the reason
/// [`crate::ui::worktrees::WORKTREE_COLUMNS`] records — a column cannot be
/// narrow in the header and wide in the rows when there is one number for both.
///
/// **COMMIT became two columns.** It used to be one, and the one cell it held
/// was a sentence: "pinned a1b2c3d → recorded e4f5a6b". A sentence cannot be
/// scanned down a column, and it cannot be coloured on one side, because there
/// is only one galley. Splitting it into CHECKED OUT and RECORDED puts the two
/// values under their own labels, which is what makes "these two disagree, and
/// it is the checkout that moved" visible at a glance — and it costs the words
/// "pinned" and "recorded", which is the trade this ticket makes on purpose: the
/// columns *are* the labels now, so the sentence was saying twice what the header
/// says once.
///
/// The offsets follow the worktrees pane's rhythm (a 8pt path gutter, a 100pt
/// column pitch) so the two panes' tables read as the same table.
const SUBMODULE_COLUMNS: [PaneColumn; 5] = [
    PaneColumn::start("PATH", 8.0),
    PaneColumn::start("CHECKED OUT", 244.0),
    PaneColumn::start("RECORDED", 344.0),
    PaneColumn::start("STATUS", 444.0),
    PaneColumn::end("ACTIONS", 0.0),
];

/// Air between two adjacent cells' columns, so a long cell in one column does
/// not read as belonging to the next. Applied to the *path* cell, the one cell
/// allowed to be as wide as the table gives it.
const CELL_GAP: f32 = 8.0;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    // Read **before** the header, because the header's count chip is this list's
    // own length and not a second opinion about it. Painted only once the list
    // has landed: a count that appears as 0 and becomes 3 a frame later is a
    // flickering number, and the pane already says "Loading submodules…"
    // underneath, which is the honest thing to show while the answer is unknown.
    let root = state.selected_root.clone();
    let loaded: Option<Vec<Submodule>> = root
        .as_ref()
        .and_then(|id| state.caches.submodules(id))
        .map(<[Submodule]>::to_vec);
    let count = loaded.as_ref().map(|subs| subs.len().to_string());

    widgets::pane_header(ui, "SUBMODULES", count.as_deref(), |_ui| {});
    ui.add_space(4.0);

    let Some(id) = root else {
        return;
    };
    let submodules = loaded.unwrap_or_default();
    if state.caches.submodules(&id).is_none() {
        ui.weak("Loading submodules…");
    }

    widgets::column_header(ui, ui.available_rect_before_wrap(), &SUBMODULE_COLUMNS);
    ui.add_space(4.0);

    ScrollArea::vertical().show(ui, |ui| {
        if submodules.is_empty() && state.caches.submodules(&id).is_some() {
            ui.weak("No registered submodules in this repository.");
        }
        for sub in &submodules {
            submodule_row(ui, state, sub);
        }
    });
}

/// One submodule row: path, checked-out commit, recorded commit, status, and
/// the init/update and deinit actions.
///
/// The cells are read from [`SUBMODULE_COLUMNS`] through
/// [`widgets::column_cell`] and never from a number written here, which is what
/// keeps a row's cells under the header's labels. The trailing ACTIONS column is
/// placed by the row's own right-aligned layout, which lands on the same
/// trailing edge the header's label for it is measured back from.
fn submodule_row(ui: &mut Ui, state: &mut AppState, sub: &Submodule) {
    let name = sub
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<submodule>")
        .to_string();
    // Whether the checkout has moved off the record. `git submodule status`
    // spells this `+`, and it is always the *checked-out* side that moved, so
    // that is the column that gets coloured — see [`commit_cell`].
    let diverged = diverged(sub);
    // …and when the two agree, the record is **not** painted a second time. Two
    // cells holding one commit read as two facts about two things; the honest
    // rendering of "the checkout is where the index says it is" is one ref and
    // one empty cell, which is also what makes an empty cell on a *diverged*
    // row mean something when it appears.
    let recorded = match (&sub.head, &sub.recorded) {
        (Some(h), Some(r)) if h == r => None,
        _ => sub.recorded.as_deref(),
    };
    ui.horizontal(|ui| {
        widgets::column_cell(ui, &SUBMODULE_COLUMNS, 0, |ui| path_cell(ui, sub));
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
            // **The positions do not move with the verb.** A right-to-left
            // cluster puts the first control written here at the RIGHT edge, so
            // Deinit is the rightmost button on every row and the
            // init/update action is the one to its left, on every state. What
            // changes is the word on that left-hand button: see `action_verb`.
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

/// The PATH cell: the row's **primary column**, in the data face.
///
/// Primary ink because the path is what tells one row from another — it is the
/// one thing in the row the reader is looking for. The monospaced data face
/// because a path is data, and because a monospaced column of paths is
/// scannable in a way a proportional one is not.
///
/// The width is **derived, never stated**: it is the gap from this cell's origin
/// to the next column's, both read from [`SUBMODULE_COLUMNS`], so the path takes
/// exactly the room the other columns leave it and no more. A long path elides
/// rather than pushing the columns beside it out of line.
fn path_cell(ui: &mut Ui, sub: &Submodule) {
    let next_column = SUBMODULE_COLUMNS[1].origin(ui.max_rect());
    let width = (next_column - ui.cursor().left() - CELL_GAP).max(0.0);
    // Laid out in a band of its own rather than through `add_sized`, because
    // `add_sized` **centres** its widget in the size it is given: a path would
    // then start wherever its own length left it, and a column whose cells do
    // not start at the column are not a column.
    ui.allocate_ui_with_layout(
        Vec2::new(width, widgets::CHIP_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(sub.path.display().to_string())
                        .font(data_font(TYPE_BODY))
                        .color(Palette::INK),
                )
                .truncate(),
            );
        },
    );
}

/// True when the checkout is at a different commit than the superproject
/// records — `git submodule status`'s `+`.
///
/// **Both sides have to be present.** An uninitialised submodule has a record
/// and no checkout, which is not a divergence: there is nothing on the checkout
/// side to have moved, and colouring the record as though it had would report a
/// disagreement where the pane knows only an absence.
fn diverged(sub: &Submodule) -> bool {
    matches!((&sub.head, &sub.recorded), (Some(h), Some(r)) if h != r)
}

/// One commit cell, or nothing.
///
/// **The empty cell is a real state, and it is the point of the split.** A
/// submodule whose two commits agree shows the short ref in CHECKED OUT and
/// leaves RECORDED empty; it does not print the same ref twice, which would read
/// as two facts about two different things when there is one. The same is true
/// of a submodule with no checkout at all: the record goes in RECORDED, CHECKED
/// OUT stays empty, and the reader can see which side is missing.
///
/// **`divergent` colours the side that moved**, and it is a parameter so that
/// the value is the one map's answer rather than a second table: the checkout
/// has left the record, which is the submodule's divergence, so it wears
/// [`RepoState::Diverged`]. The recorded side stays in secondary ink — it is the
/// side that did *not* move, and colouring both would say nothing.
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
/// The operation was already an init on an uninitialised submodule — the row has
/// always passed `init: true` for that state — so the control was labelled
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

/// The status cell: a leading dot and the state in words (issue 14), through
/// the shared mark pair.
///
/// Not a chip, and deliberately not one: it paints **coloured text with no
/// background at all**, so it shares no geometry, no radius and no fill with the
/// shared chip vocabulary. Turning it into a real chip would introduce a
/// background where none exists and would newly register an accessibility node
/// for a piece of status text — a design change that needs its own ticket, not a
/// consolidation. See `docs/design-system-roles.md`.
///
/// **The dot in front of the word is a mark, not a chip**, and that is what keeps
/// this comment true of the code it is attached to rather than half-stale. A
/// chip is a bounded container: a fill rect, a radius, a padding box, and a node
/// of its own. This is a filled circle of [`components::STATE_DOT_R`] followed
/// by the word, with nothing between them and nothing behind them, so a column of
/// statuses is scannable without any of that coming back. Its one addition to
/// the old cell is the dot; it removed no fill, because there was never one.
///
/// The word and the dot are the **same** colour, and that colour is the one
/// repository-state map's answer for this state — see [`repo_state`]. They were
/// previously two separate `match`es: four status tones and one unrelated dot
/// grammar, in a view that is supposed to say the same thing the rest of the app
/// says about a state.
fn status_mark(ui: &mut Ui, state: SubmoduleState) {
    let word = match state {
        SubmoduleState::UpToDate => "Up to date",
        SubmoduleState::NeedsUpdate => "Needs update",
        SubmoduleState::Uninitialized => "Uninitialized",
        SubmoduleState::Conflicted => "Conflicts",
    };
    components::state_mark(ui, word, repo_state(state).color());
}

/// Which repository state a submodule state is, so the status's ink is
/// [`RepoState::color`] and not a fourth answer to "what colour is a state".
///
/// **A translation, not a palette.** [`SubmoduleState`] is its own domain type
/// and does not map one-to-one onto the repository states — a submodule can be
/// uninitialised, which no repository can be — so this function says *which
/// repository state a submodule state is*, and the colour comes from the one map
/// through [`repo_state(state).color()`]. It is the same shape as
/// `crate::ui::shell`'s status-bar aggregate, and for the same reason: a local
/// type that named a colour could pick one, and a second `match` returning
/// `Palette::…` is exactly the fork this keeps shut.
///
/// The one arm the map could not say, and now can.
/// [`SubmoduleState::Uninitialized`] is not clean (the working copy is absent),
/// not a conflict, and not a divergence, and none of those is a *severity* it
/// should wear — it is work the user is owed: the recorded commit is in the
/// index and has not been checked out. So it takes [`RepoState::Uninitialized`],
/// whose answer is the muted ink rather than one of the three state tones.
///
/// **Why it could not keep leaning on [`RepoState::Unpulled`].** That lean put a
/// submodule with no working copy in the same reserved orange as "N commits
/// incoming", and those are different facts: the counter tone is documented as
/// dirt and unpushed counts only, so a missing checkout wearing it is a
/// borrowed severity rather than the thing it is. The arm exists so this
/// translation says which state a submodule state is without the view picking a
/// colour, and so the new fact never has to be spelled by borrowing an old one.
fn repo_state(state: SubmoduleState) -> RepoState {
    match state {
        SubmoduleState::UpToDate => RepoState::Clean,
        SubmoduleState::NeedsUpdate => RepoState::Diverged,
        SubmoduleState::Conflicted => RepoState::Conflict,
        SubmoduleState::Uninitialized => RepoState::Uninitialized,
    }
}
