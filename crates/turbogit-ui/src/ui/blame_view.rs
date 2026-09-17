//! Blame view (issue 18): line-by-line attribution — commit, author, age —
//! for one file at one revision, shown in place of the commit graph while
//! open. Reached from the changed-files pane footer ("Blame", screen 09)
//! and the changed-file context menu; a row's commit navigates back to the
//! log with that commit selected, and the current path filter is untouched
//! by opening or closing the view.

use crate::theme::Palette;
use crate::ui::widgets;
use chrono::{DateTime, Local, TimeZone, Utc};
use egui::{
    CornerRadius, FontFamily, FontId, Pos2, RichText, ScrollArea, Sense, Ui, Vec2, WidgetInfo,
    WidgetType,
};
use turbogit_app::state::AppState;
use turbogit_domain::model::BlameLine;

/// Blame row height (slightly tighter than log rows — a file's lines).
const ROW_HEIGHT: f32 = 20.0;
/// Uppercase micro text (§3.3) — shared control role (T2).
const MICRO_TEXT: f32 = crate::theme::TYPE_CONTROL;
/// Mono cell font size — shared body role (T2).
const MONO_TEXT: f32 = crate::theme::TYPE_BODY;

/// Cell x offsets within a row, mirroring the log table's rhythm.
const HASH_X: f32 = 4.0;
const AUTHOR_X: f32 = 84.0;
const AGE_X: f32 = 194.0;
const CONTENT_X: f32 = 264.0;

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

fn short(id: &str) -> String {
    id.chars().take(7).collect()
}

/// Render the blame surface over the graph pane's region. Data flows
/// through [`AppState::ensure_blame`] (worker thread → `BlameReady`), so
/// this is a pure render over the cached lines.
pub fn show_blame(ui: &mut Ui, state: &mut AppState) {
    state.ensure_blame();

    // Split the mutable state from the borrowed data up front: rows render
    // against the cached lines and clicks are applied after the pass ends.
    let target = state.ui.blame.clone();
    let Some(target) = target else { return };
    let key = blame_view_key(&target);
    let lines: Option<&Vec<BlameLine>> = state
        .ui
        .blame_cache
        .as_ref()
        .filter(|(k, _)| k == &key)
        .map(|(_, lines)| lines);

    // The header's close affordance sets a flag; like every interaction in
    // the log workspace it is applied after the borrow-heavy render ends.
    let mut close = false;
    widgets::toolwindow_header(ui, "Blame", |ui| {
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
            short(&target.rev)
        ))
        .font(micro_font())
        .color(Palette::BRAND),
    );
    ui.add_space(2.0);

    // Column micro-headers aligned with the cells below.
    let left = ui.cursor().left();
    let top = ui.cursor().top();
    for (title, dx) in [("COMMIT", HASH_X), ("AUTHOR", AUTHOR_X), ("AGE", AGE_X)] {
        let galley = ui
            .painter()
            .layout_no_wrap(title.to_owned(), micro_font(), Palette::INK_3);
        ui.painter()
            .galley(Pos2::new(left + dx, top + 2.0), galley, Palette::INK_3);
    }
    ui.add_space(16.0);

    if let Some(err) = &state.ui.blame_error {
        ui.colored_label(Palette::STATE_ERROR, err);
        return;
    }
    let Some(lines) = lines else {
        ui.spinner();
        ui.label("Computing blame…");
        return;
    };

    // Row clicks are deferred (plan §1.3): rows borrow the cache until the
    // scroll pass ends.
    let mut clicked: Option<String> = None;
    ScrollArea::vertical().show(ui, |ui| {
        for line in lines {
            if blame_row(ui, line, &target.rev) {
                clicked = Some(line.commit.clone());
            }
        }
    });

    if let Some(commit) = clicked {
        // Navigate to the line's commit in the log (issue 18): select it,
        // close the blame view, and keep the path filter untouched.
        state.ui.selected_commit = Some(commit);
        state.ui.log_selected_file = None;
        state.ui.blame = None;
    }
    if close {
        state.ui.blame = None;
    }
}

fn blame_view_key(target: &turbogit_app::state::BlameTarget) -> String {
    format!(
        "{}|{}|{}",
        target.root.0.display(),
        target.rev,
        target.path.display()
    )
}

/// One blamed line: highlight when the blamed-at commit introduced it,
/// paint hash | author | age | content, and report whether it was clicked.
fn blame_row(ui: &mut Ui, line: &BlameLine, rev: &str) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());

    if line.commit == rev {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(2), Palette::selection_bg());
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(2), Palette::SURFACE_2);
    }

    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let hash = painter.layout_no_wrap(short(&line.commit), mono_font(), Palette::BRAND);
    painter.galley(
        Pos2::new(rect.left() + HASH_X, cy - hash.size().y / 2.0),
        hash,
        Palette::BRAND,
    );
    let author: String = line.author.chars().take(12).collect();
    let author_g = painter.layout_no_wrap(author, body_font(), Palette::INK_2);
    painter.galley(
        Pos2::new(rect.left() + AUTHOR_X, cy - author_g.size().y / 2.0),
        author_g,
        Palette::INK_2,
    );
    let age = painter.layout_no_wrap(fmt_age(line.time), micro_font(), Palette::INK_3);
    painter.galley(
        Pos2::new(rect.left() + AGE_X, cy - age.size().y / 2.0),
        age,
        Palette::INK_3,
    );
    let content: String = line.content.trim_end().to_owned();
    let content_g = painter.layout_no_wrap(content.clone(), mono_font(), Palette::INK);
    painter.galley(
        Pos2::new(rect.left() + CONTENT_X, cy - content_g.size().y / 2.0),
        content_g,
        Palette::INK,
    );

    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("{} {}", short(&line.commit), content),
        )
    });
    response.clicked()
}
