//! The preflight a targeted history verb shows before it runs (ticket 10, both
//! verbs from ticket 11).
//!
//! One briefing for Drop commit and Reword commit: which commit the verb acts
//! on, which commits are replayed on top of it, what the history becomes, what is
//! likely to conflict, how long it takes, and how to get back. The VERB is a
//! parameter ([`HistoryVerb`]) rather than a second dialog: the two rewrites cost
//! the same replay, warn the same way and are undone the same way, so the only
//! honest difference is what the verb does to the commit the dialog is about —
//! one removes it, one rewrites its message — and that is two lines of copy.
//!
//! Nothing here runs git — the plan and its cautions are built when the verb is
//! confirmed ([`AppState::history_verb_plan`] and [`AppState::rebase_cautions`])
//! and this module only paints them and dispatches the guarded operation on
//! confirm.
//!
//! Not a [`PendingConfirm`](turbogit_app::state::PendingConfirm): that is a flat
//! label with OK and Cancel and nowhere to put a plan. Not
//! [`crate::ui::interactive_rebase`] either — that is a 1080px editor for
//! building a plan by hand, and these verbs have already built one.

use egui::{Align, Layout, RichText, Ui};
use turbogit_app::operation::Operation;
use turbogit_app::state::{AppState, HistoryVerb, RewritePreflight};
use turbogit_domain::model::RebasePlanEntry;
use turbogit_services::history_editor::{self, PlanPreview, plan_preview};
use turbogit_services::reselection::{RewrittenAnchor, subject_of_message};

use crate::theme::Palette;
use crate::ui::widgets;

/// How many replayed commits the list shows before it elides.
///
/// Twelve is a briefing, not a transcript: a 300-commit history would otherwise
/// open a dialog taller than the window, and the developer's decision does not
/// turn on reading 300 rows. Both ENDS are always shown, because the edges of
/// the blast radius are the facts — the oldest commit that moves, and the tip it
/// moves to — and the elided middle is named with its own count, so the visible
/// rows never lie about how much is being replayed.
const REPLAYED_ROWS: usize = 12;

/// The three things the verb names in this dialog. Copy lives HERE, not on
/// [`HistoryVerb`], because the app state is data and this is wording: one
/// table, three answers, and a fourth verb could not be added without someone
/// finding this list.
fn window_title(verb: &HistoryVerb) -> &'static str {
    match verb {
        HistoryVerb::Drop => "Drop commit",
        HistoryVerb::Reword { .. } => "Reword commit",
    }
}

/// What the acted-on group is called: a drop's group names a commit that is
/// going, a reword's names a commit that is being rewritten in place.
fn group_title(verb: &HistoryVerb) -> &'static str {
    match verb {
        HistoryVerb::Drop => "DROPPED COMMIT",
        HistoryVerb::Reword { .. } => "REWORDED COMMIT",
    }
}

/// The badge that states the SCALE of the replay, in the verb's own honest form.
///
/// A drop is a subtraction: a commit leaves the history and the count falls, so
/// the plan editor's `16 → 15 COMMITS` is exactly right and is used unchanged. A
/// reword is not a subtraction — the count cannot change, and the same arrow form
/// reads as "nothing happens" in the one dialog whose job is to say what it will.
/// So it states the scale and the verb: how many commits get REWRITTEN, which is
/// the cost the equal numbers would otherwise hide.
///
/// The noun follows the number the way [`widgets::cautions_rail`] follows "1
/// file" and "1 author": one commit is "1 COMMIT REWRITTEN", never "1 COMMITS".
fn counts_badge(verb: &HistoryVerb, preview: &PlanPreview) -> String {
    match verb {
        HistoryVerb::Drop => format!("{} → {} COMMITS", preview.before, preview.after),
        HistoryVerb::Reword { .. } => {
            let kept = preview.kept.len();
            format!(
                "{kept} {} REWRITTEN",
                if kept == 1 { "COMMIT" } else { "COMMITS" }
            )
        }
    }
}

/// The one button that runs the rewrite, naming the verb rather than the cost —
/// the counts are already in the middle of the dialog.
fn confirm_label(verb: &HistoryVerb) -> &'static str {
    match verb {
        HistoryVerb::Drop => "Drop this commit",
        HistoryVerb::Reword { .. } => "Reword this commit",
    }
}

/// Render the preflight while `ui.dialog` is
/// [`Dialog::RewritePreflight`](turbogit_app::state::Dialog::RewritePreflight).
/// The window titles itself with the verb, so a developer with both dialogs in
/// their history can tell which one they are looking at.
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

    // The cautions and the way back are the SHARED rewrite rail — the same two
    // functions the plan editor's right column paints, so "what is likely to go
    // wrong" and "how do I get back" are one wording in both places, and in both
    // verbs.
    widgets::cautions_rail(ui, &pf.cautions);
    widgets::recovery_note(ui, &state.rewrite_backup_ref());

    ui.add_space(6.0);
    footer(ui, state, pf);
}

/// The commit this verb acts on, named: its short reference in the data face
/// beside the subject it has TODAY, which is how the tag dialog's TARGET row
/// names a commit.
///
/// A drop's work is done by that one line. A reword's work is a DIFFERENCE, so
/// the new subject is stated underneath it under the same arrow a diff would use
/// — the dialog is the last place before the commit is re-created, and "what is
/// about to happen to this message" is the one fact the developer cannot recover
/// afterwards. The body is not repeated: the developer wrote it one step ago.
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

/// The commit set that will be replayed, in plan order — oldest first, the order
/// a rebase reads and the order the plan itself is in — under a badge that states
/// the scale in the verb's own form ([`counts_badge`]).
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

/// The footer: the estimate beside the modal's shared rule, then Cancel and the
/// one button that rewrites history. The confirm names what the verb will do to
/// the commit, which is named at the top of the dialog and counted in the middle.
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

/// Arm the reselection, THEN dispatch — in that order, because the operation
/// runs on a worker and one that finished inside this frame would settle before
/// anything was waiting for the post-rewrite log.
///
/// The dispatch is the guarded one the operation owns, which is what writes the
/// backup ref and reports the outcome: a failure part-way is a reported failure,
/// not a swallowed one. Which anchor the reselect is armed with is the ONE thing
/// the two verbs disagree about, and it comes from the verb: a reword names the
/// subject the row will carry, because the row survives and its old subject is
/// what an impostor would be carrying.
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
    let operation = match &pf.verb {
        HistoryVerb::Drop => Operation::drop_commit(&pf.root, &branch, &pf.commit, &settings),
        HistoryVerb::Reword { message } => {
            Operation::reword_commit(&pf.root, &branch, &pf.commit, message, &settings)
        }
    };
    state.dispatch(operation);
}
