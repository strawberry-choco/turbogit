//! Blame view (issue 18): line-by-line attribution — commit, author, age —
//! for one file at one revision, shown in place of the commit graph while
//! open. Reached from the changed-files pane footer ("Blame", screen 09)
//! and the changed-file context menu; a row's commit navigates back to the
//! log with that commit selected, and the current path filter is untouched
//! by opening or closing the view.
//!
//! **This view replaces the commit table; it does not sit beside it**
//! (conformance issue 15). It occupies the same slot in the same region, so a
//! reader who switches between the two is looking at the same table twice — and
//! anything this file decided for itself was a visual reset at the moment of
//! switching. Everything it used to spell out for itself is now read from
//! [`log_window::CommitTable`]: the row height, the leading gutter the ROOTS
//! column owns, the hash / author / wide-column offsets, the trailing column's
//! right inset, and the two row inks. Its rows go through the same
//! [`log_window::paint_log_row`], so the band and the one accent rail are the
//! commit table's rather than a second answer to "what is a chosen row".
//!
//! The two cells that are *not* shared, and why: the wide column holds a source
//! line here rather than a subject (so it is labelled `LINE`, and it is measured
//! from the same offset the subject is), and the trailing column is a relative
//! age rather than a formatted date. The offsets and the inks are still the
//! table's.

use crate::theme::Palette;
use crate::ui::log_window::{
    CommitTable, paint_log_row, row_meta_ink, row_name_ink, shows_root_gutter, table_content_left,
};
use crate::ui::widgets;
use chrono::{DateTime, Local, TimeZone, Utc};
use egui::{
    FontFamily, FontId, Pos2, Rect, RichText, ScrollArea, Sense, Ui, Vec2, WidgetInfo, WidgetType,
};
use turbogit_app::keyed_read::Read;
use turbogit_app::state::AppState;
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

/// Render the blame surface over the graph pane's region. Data flows through
/// the keyed read of the open [`turbogit_app::state::BlameTarget`] (worker
/// thread → `BlameReady`), so this is a pure render over the verdict: which of
/// its four answers paints is the read's question, not one this view can forget
/// to ask.
pub fn show_blame(ui: &mut Ui, state: &mut AppState) {
    let Some(target) = state.ui.blame.clone() else {
        return;
    };
    let verdict = state.read(target.clone());

    // The header's close affordance sets a flag; like every interaction in
    // the log workspace it is applied after the borrow-heavy render ends.
    let mut close = false;
    widgets::pane_header(ui, "BLAME", None, |ui| {
        if ui
            .link("Close blame")
            .on_hover_text("Back to the commit log")
            .clicked()
        {
            close = true;
        }
    });
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

    // Whether the ROOTS column's gutter exists in this listing. Read from the
    // commit table's own predicate, not re-decided here: a blame view that
    // reserved the gutter on a different rule than the table it replaced would
    // put every one of its columns 5px out from under the table's the moment the
    // user switched back.
    let multi_root = shows_root_gutter(state);

    // The column header row, from the shared column chrome and the same
    // `[BLAME_COLUMNS]` table the rows measure their cells from (R7), at the
    // commit table's own content edge so its labels sit over the commit table's
    // cells. It replaces three hand-laid galleys at the control size with no
    // rule under them, which is how this pane had a header no other pane could
    // be compared against.
    let available = ui.available_rect_before_wrap();
    widgets::column_header(
        ui,
        Rect::from_min_max(
            Pos2::new(table_content_left(available, multi_root), available.top()),
            Pos2::new(available.right(), available.bottom()),
        ),
        &BLAME_COLUMNS,
    );

    let lines = match verdict {
        Read::Fresh(lines) if lines.is_empty() => {
            widgets::empty_state(ui, "No blame lines for this revision.");
            if close {
                state.ui.blame = None;
            }
            return;
        }
        Read::Fresh(lines) => lines,
        Read::Empty => {
            widgets::empty_state(ui, "No blame lines for this revision.");
            if close {
                state.ui.blame = None;
            }
            return;
        }
        Read::Waiting => {
            widgets::keyed_read_presentation(
                ui,
                widgets::KeyedReadPresentation::Waiting("Computing blame…"),
            );
            return;
        }
        Read::Failed(message) => {
            widgets::keyed_read_presentation(ui, widgets::KeyedReadPresentation::Failed(&message));
            return;
        }
    };

    // Row clicks are deferred (plan §1.3): the handle keeps the rows alive
    // across the scroll pass, which is what the owned verdict is for.
    let mut clicked: Option<String> = None;
    ScrollArea::vertical().show(ui, |ui| {
        for line in lines.iter() {
            if blame_row(ui, line, &target.rev, multi_root) {
                clicked = Some(line.commit.clone());
            }
        }
    });

    if let Some(commit) = clicked {
        // Navigate to the line's commit in the log (issue 18): select it,
        // close the blame view, and keep the path filter untouched.
        state.ui.selected_commit = Some(commit.clone());
        state.ui.log_selected_file = None;
        state.ui.blame = None;
        // The log is what the user sees next, and the commit they picked can be
        // anywhere in its history — so the list is asked to show it (issue 08).
        state.ui.log_scroll_to = Some(commit);
    }
    if close {
        state.ui.blame = None;
    }
}

/// One blamed line: highlight when the blamed-at commit introduced it, paint
/// hash | author | age | line, and report whether it was clicked.
///
/// **The row is the commit table's row.** Its height is
/// [`CommitTable::ROW_HEIGHT`] and its band and rail are
/// [`paint_log_row`]'s, so a line the blamed commit introduced takes the same
/// selected band and the same 2px accent rail a chosen commit row takes — a
/// tint alone, as this used to paint, is a selection without the rail every
/// other chosen row in the app wears.
///
/// Its cells are measured from [`table_content_left`] at the commit table's
/// offsets, which is what makes the **ROOTS column line up**: on a multi-root
/// listing this view reserves the gutter the column owns rather than painting
/// over it, so its hash starts where the commit table's hash started. The gutter
/// stays empty here on purpose — a blame target names one repository and one
/// file, so a per-root swatch repeated down a screen of lines would name a root
/// the view is already scoped to, and it would make this a fifth site asking
/// `root_color` for a value the ROOTS column has already said.
fn blame_row(ui: &mut Ui, line: &BlameLine, rev: &str, multi_root: bool) -> bool {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, CommitTable::ROW_HEIGHT), Sense::click());

    paint_log_row(ui, rect, line.commit == rev, response.hovered());

    let content_left = table_content_left(rect, multi_root);
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
    response.clicked()
}
