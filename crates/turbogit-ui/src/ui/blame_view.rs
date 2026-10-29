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

const MICRO_TEXT: f32 = crate::theme::TYPE_CONTROL;
const MONO_TEXT: f32 = crate::theme::TYPE_BODY;

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

pub fn show_blame(ui: &mut Ui, state: &mut AppState) {
    let Some(target) = state.ui.blame.clone() else {
        return;
    };
    let mut open = true;
    egui::Window::new("Blame")
        .open(&mut open)
        .vscroll(false)
        .default_size(egui::vec2(760.0, 520.0))
        .show(ui.ctx(), |ui| blame_body(ui, state, &target));
    if !open {
        state.ui.blame = None;
    }
}

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

    let available = ui.available_rect_before_wrap();
    widgets::column_header(
        ui,
        Rect::from_min_max(
            Pos2::new(available.left(), available.top()),
            Pos2::new(available.right(), available.bottom()),
        ),
        &BLAME_COLUMNS,
    );

    let mut clicked: Option<String> = None;
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
        state.ui.blame_caret_line = None;
    }

    ui.add_space(6.0);
    widgets::dialog_footer(ui, |ui| {
        if widgets::compact_button(ui, "Close").clicked() {
            state.ui.blame = None;
        }
    });

    if let Some(commit) = clicked {
        state.ui.selected_commit = Some(commit.clone());
        state.ui.log_selected_file = None;
        state.ui.blame = None;
        state.ui.log_scroll_to = Some(commit);
        if state.ui.tab != Tab::Log {
            crate::ui::shell::switch_tab(state, Tab::Log);
        }
    }
}

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
