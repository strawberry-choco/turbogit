//! Blame view (issue 18): line-by-line attribution — commit, author, age —
//! for one file at one revision, in a floating window over the workspace,
//! closed from its footer or the title bar's X. Reached from the
//! changed-files pane footer ("Blame", screen 09) and the changed-file
//! context menu; a row's commit navigates to the log with that commit
//! selected, and the current path filter is untouched by opening or closing
//! the window.
//!
//! **The window floats; the log keeps its graph.** The chrome is the `diff`
//! window's, so the listing a blame verb came from stays exactly as it was
//! and the graph never yields its region to this view.
//!
//! **Its rows are the commit table's rows** (conformance issue 15): the row
//! height, the hash / author / wide-column offsets, the trailing column's
//! right inset, and the two row inks are [`log_window::CommitTable`]'s, and
//! the rows go through the same [`log_window::paint_log_row`], so the band
//! and the one accent rail are the commit table's rather than a second
//! answer to "what is a chosen row". The ROOTS gutter is not reserved here:
//! a blame target names one repository and one file, and with no table
//! beside the window to line up with, the gutter would name a root the
//! window is already scoped to.
//!
//! The two cells that are *not* shared, and why: the wide column holds a source
//! line here rather than a subject (so it is labelled `LINE`, and it is measured
//! from the same offset the subject is), and the trailing column is a relative
//! age rather than a formatted date. The offsets and the inks are still the
//! table's.

use crate::theme::Palette;
use crate::ui::log_window::{CommitTable, paint_log_row, row_meta_ink, row_name_ink};
use crate::ui::widgets;
use chrono::{DateTime, Local, TimeZone, Utc};
use egui::{
    FontFamily, FontId, Pos2, Rect, RichText, ScrollArea, Sense, Ui, Vec2, WidgetInfo, WidgetType,
};
use turbogit_app::keyed_read::Read;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::BlameLine;

/// Uppercase micro text (§3.3) — shared control role (T2).
const MICRO_TEXT: f32 = crate::theme::TYPE_CONTROL;
/// Mono cell font size — shared body role (T2).
const MONO_TEXT: f32 = crate::theme::TYPE_BODY;

/// The blame pane's columns: one table, read by **both** the shared
/// column-header row and every row below it, at the commit table's own offsets.
///
/// The offsets are [`CommitTable`]'s, not this file's: a blame row's hash sits
/// where a commit row's hash sits, its author where a commit row's author sits,
/// its line where a commit row's subject sits, and its age trails the row on the
/// same right inset the date cell uses. Two tables of offsets is what made
/// switching to blame a reset, and conformance issue 15 is the ticket that ends
/// it, so this file holds no cell offset of its own any more.
///
/// The labels are the content's, not the geometry's: `HASH` and `AUTHOR` say the
/// same thing here as in the commit table, while the wide column is a source
/// line and the trailing one a relative age, so naming those two after the
/// commit table's `MESSAGE` and `DATE` would be a lie about what they hold.
const BLAME_COLUMNS: [widgets::PaneColumn; 4] = [
    widgets::PaneColumn::start("HASH", CommitTable::HASH),
    widgets::PaneColumn::start("AUTHOR", CommitTable::AUTHOR),
    widgets::PaneColumn::start("LINE", CommitTable::MESSAGE),
    widgets::PaneColumn::end("AGE", CommitTable::DATE_RIGHT_PAD),
];

fn mono_font() -> FontId {
    FontId::new(MONO_TEXT, FontFamily::Monospace)
}

fn body_font() -> FontId {
    FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional)
}

fn micro_font() -> FontId {
    FontId::new(MICRO_TEXT, FontFamily::Proportional)
}

/// Relative age for a blame line ("14m ago"), matching the log's relative
/// date format; older than a month falls back to the absolute stamp.
fn fmt_age(t: i64) -> String {
    let now = Local::now().timestamp();
    let d = (now - t).max(0);
    if d < 60 {
        format!("{d}s ago")
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else if d < 86400 {
        format!("{}h ago", d / 3600)
    } else if d < 2592000 {
        format!("{}d ago", d / 86400)
    } else {
        match Utc.timestamp_opt(t, 0) {
            chrono::LocalResult::Single(dt) => {
                let local: DateTime<Local> = DateTime::from(dt);
                local.format("%Y-%m-%d").to_string()
            }
            _ => String::new(),
        }
    }
}

/// The floating blame window the log's blame verbs open (the
/// [`turbogit_app::state::BlameTarget`] in `state.ui.blame`), in the house
/// `egui::Window` chrome, closing from its own footer or the title bar's X.
///
/// Data flows through the keyed read of the open target (worker thread →
/// `BlameReady`), so the body is a pure render over the verdict: which of
/// its four answers paints is the read's question, not one this view can
/// forget to ask.
pub fn show_blame(ui: &mut Ui, state: &mut AppState) {
    let Some(target) = state.ui.blame.clone() else {
        return;
    };
    let mut open = true;
    egui::Window::new("Blame")
        .open(&mut open)
        // The body's ScrollArea owns scrolling; the window's own would leave it
        // a remainder to measure, and the rows would collapse inside it.
        .vscroll(false)
        .default_size(egui::vec2(760.0, 520.0))
        .show(ui.ctx(), |ui| blame_body(ui, state, &target));
    if !open {
        state.ui.blame = None;
    }
}

/// The subject line names what is open, then the blame surface itself and the
/// footer that closes it.
fn blame_body(ui: &mut Ui, state: &mut AppState, target: &turbogit_app::state::BlameTarget) {
    let verdict = state.read(target.clone());

    ui.label(
        RichText::new(format!(
            "{} @ {}",
            target.path.display(),
            widgets::short_commit_ref(&target.rev)
        ))
        .font(micro_font())
        .color(Palette::BRAND),
    );
    ui.add_space(2.0);

    // The column header row, from the shared column chrome and the same
    // `[BLAME_COLUMNS]` table the rows measure their cells from (R7). A blame
    // target names one repository, so there is no ROOTS gutter to reserve.
    let available = ui.available_rect_before_wrap();
    widgets::column_header(
        ui,
        Rect::from_min_max(
            Pos2::new(available.left(), available.top()),
            Pos2::new(available.right(), available.bottom()),
        ),
        &BLAME_COLUMNS,
    );

    // Row clicks are deferred (plan §1.3): the handle keeps the rows alive
    // across the scroll pass, which is what the owned verdict is for.
    let mut clicked: Option<String> = None;
    // The line `F4` carried in from the diff viewer's caret (Jump to Source).
    // Copied out before the scroll pass because the pass owns the `Ui` borrow,
    // and this is the surface's one chance to aim at a row.
    let caret = state.ui.blame_caret_line;
    let mut caret_aimed = false;
    match verdict {
        Read::Fresh(lines) if !lines.is_empty() => {
            ScrollArea::vertical()
                .id_salt("blame_lines")
                .show(ui, |ui| {
                    for line in lines.iter() {
                        let response = blame_row(ui, line, &target.rev);
                        if Some(line.line_no) == caret {
                            // Arrive at the line the reader was looking at, not at the top of
                            // the file. Centre, not start: a line pinned to the window's edge
                            // is a line the reader has to find again.
                            response.scroll_to_me(Some(egui::Align::Center));
                            caret_aimed = true;
                        }
                        if response.clicked() {
                            clicked = Some(line.commit.clone());
                        }
                    }
                });
        }
        Read::Fresh(_) | Read::Empty => {
            widgets::empty_state(ui, "No blame lines for this revision.");
        }
        Read::Waiting => {
            widgets::keyed_read_presentation(
                ui,
                widgets::KeyedReadPresentation::Waiting("Computing blame…"),
            );
        }
        Read::Failed(message) => {
            widgets::keyed_read_presentation(ui, widgets::KeyedReadPresentation::Failed(&message));
        }
    }
    if caret_aimed {
        // One-shot: the aim is this surface's opening position, not a claim on
        // every frame — leaving it set would pin the view against the reader's
        // own scrolling.
        state.ui.blame_caret_line = None;
    }

    ui.add_space(6.0);
    widgets::dialog_footer(ui, |ui| {
        if widgets::compact_button(ui, "Close").clicked() {
            state.ui.blame = None;
        }
    });

    if let Some(commit) = clicked {
        // Navigate to the line's commit in the log (issue 18): select it,
        // close the blame window, and keep the path filter untouched.
        state.ui.selected_commit = Some(commit.clone());
        state.ui.log_selected_file = None;
        state.ui.blame = None;
        // The log is what the user sees next, and the commit they picked can be
        // anywhere in its history — so the list is asked to show it (issue 08).
        state.ui.log_scroll_to = Some(commit);
        // The window floats over any tab, so the log is not necessarily the
        // visible one; the click's promise is still that the log is what the
        // user sees next, so the tab follows it.
        if state.ui.tab != Tab::Log {
            crate::ui::shell::switch_tab(state, Tab::Log);
        }
    }
}

/// One blamed line: highlight when the blamed-at commit introduced it, paint
/// hash | author | age | line, and answer with the row's own response.
///
/// The response travels rather than a `clicked` bool because the row has a second
/// consumer: a blame opened by `F4` from the diff viewer has to **arrive** at the
/// line the caret was on, and asking the surface to show a row is something only
/// that row's response can do.
///
/// **The row is the commit table's row.** Its height is
/// [`CommitTable::ROW_HEIGHT`] and its band and rail are
/// [`paint_log_row`]'s, so a line the blamed commit introduced takes the same
/// selected band and the same 2px accent rail a chosen commit row takes — a
/// tint alone, as this used to paint, is a selection without the rail every
/// other chosen row in the app wears.
///
/// Its cells are measured from [`CommitTable`]'s offsets at the row's own
/// leading edge, which is what keeps the hash, author and wide column at the
/// table's spacing inside whatever width the window is dragged to.
fn blame_row(ui: &mut Ui, line: &BlameLine, rev: &str) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, CommitTable::ROW_HEIGHT), Sense::click());

    paint_log_row(ui, rect, line.commit == rev, response.hovered());

    let content_left = rect.left();
    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let hash = painter.layout_no_wrap(
        widgets::short_commit_ref(&line.commit),
        mono_font(),
        Palette::LINK,
    );
    painter.galley(
        Pos2::new(content_left + CommitTable::HASH, cy - hash.size().y / 2.0),
        hash,
        Palette::LINK,
    );
    let author: String = line.author.chars().take(12).collect();
    let author_g = painter.layout_no_wrap(author, body_font(), row_meta_ink());
    painter.galley(
        Pos2::new(
            content_left + CommitTable::AUTHOR,
            cy - author_g.size().y / 2.0,
        ),
        author_g,
        row_meta_ink(),
    );
    // The age trails the row on the commit table's own right inset, because a
    // right-aligned trailing cell that tracked a different inset in one of the
    // two tables is a column that moved when the view did.
    let age = painter.layout_no_wrap(fmt_age(line.time), body_font(), row_meta_ink());
    let age_x = rect.right() - CommitTable::DATE_RIGHT_PAD - age.size().x;
    painter.galley(
        Pos2::new(age_x, cy - age.size().y / 2.0),
        age,
        row_meta_ink(),
    );
    let content: String = line.content.trim_end().to_owned();
    let content_g = painter.layout_no_wrap(content.clone(), mono_font(), row_name_ink());
    painter.galley(
        Pos2::new(
            content_left + CommitTable::MESSAGE,
            cy - content_g.size().y / 2.0,
        ),
        content_g,
        row_name_ink(),
    );

    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("{} {}", widgets::short_commit_ref(&line.commit), content),
        )
    });
    response
}
