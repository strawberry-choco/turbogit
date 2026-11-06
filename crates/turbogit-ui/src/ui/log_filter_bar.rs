use crate::theme::{CONTROL_RADIUS, GROUP_ROW_HEIGHT, Palette, TYPE_CONTROL, chrome_font};
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use chrono::Utc;
use egui::{
    Align, Color32, CornerRadius, Id, Key, Layout, Popup, PopupCloseBehavior, Pos2, Response,
    ScrollArea, Sense, TextEdit, Ui, Vec2, WidgetInfo, WidgetType,
};
use std::path::PathBuf;
use turbogit_app::log_filters::{
    DateFilter, LABEL_AFTER, LABEL_ALL, LABEL_BEFORE, LABEL_BRANCH, LABEL_DATE, LABEL_LAST_DAY,
    LABEL_LAST_WEEK, LABEL_PATHS, LABEL_ROOTS, LABEL_SELECT, LABEL_SELECT_PERIOD, LABEL_USER,
    parse_day,
};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

const SALT_BRANCH: &str = "branch";
const SALT_USER: &str = "user";
const SALT_DATE: &str = "date";
const SALT_PATHS: &str = "paths";
const SALT_DATE_AFTER: &str = "date_after";
const SALT_DATE_BEFORE: &str = "date_before";

const APPLY: &str = "Apply";
const CLEAR: &str = "Clear";
const ALL_ROOTS: &str = "All roots";
const DATE_HINT: &str = "YYYY-MM-DD";

const BUTTON_PAD_X: f32 = 8.0;
const CARET_SIZE: f32 = 12.0;
const POPUP_WIDTH: f32 = 240.0;
const LIST_MAX_HEIGHT: f32 = 200.0;

// Matches log_window.rs's private graph swatches so a root reads the same in both panes.
const ROOT_COLORS: &[Color32] = &[
    Color32::from_rgb(80, 140, 230),
    Color32::from_rgb(220, 120, 140),
    Color32::from_rgb(120, 200, 130),
    Color32::from_rgb(200, 170, 90),
    Color32::from_rgb(170, 130, 220),
    Color32::from_rgb(90, 190, 200),
    Color32::from_rgb(230, 150, 90),
    Color32::from_rgb(150, 200, 220),
];

fn root_color(idx: usize) -> Color32 {
    ROOT_COLORS[idx % ROOT_COLORS.len()]
}

fn open_id() -> Id {
    Id::new("tg_log_filter_bar_open")
}

fn button_id(salt: &str) -> Id {
    Id::new(("tg_log_filter_button", salt))
}

fn popup_id(salt: &str) -> Id {
    Id::new(("tg_log_filter_popup", salt))
}

fn field_id(salt: &str) -> Id {
    Id::new(("tg_log_filter_field", salt))
}

fn open_salt(ui: &Ui) -> Option<&'static str> {
    ui.ctx()
        .data(|d| d.get_temp::<Option<&'static str>>(open_id()).flatten())
}

fn set_open(ui: &Ui, salt: Option<&'static str>) {
    ui.ctx().data_mut(|d| d.insert_temp(open_id(), salt));
}

/// One horizontal row of IntelliJ's four Git Log filter dropdowns and Clear.
pub fn filter_bar(ui: &mut egui::Ui, state: &mut AppState) {
    let branch = state.ui.log_ref_scope.as_ref().map(|(_, n)| n.clone());
    let user = state.ui.log_author_filter.clone();
    let date = match state.ui.log_date_filter {
        DateFilter::All => None,
        DateFilter::Period { .. } => {
            let after = state.ui.log_date_after.trim();
            let before = state.ui.log_date_before.trim();
            Some(match (after.is_empty(), before.is_empty()) {
                (false, false) => format!("{after} - {before}"),
                (false, true) => format!("Since {after}"),
                (true, false) => format!("Until {before}"),
                (true, true) => LABEL_SELECT_PERIOD.to_owned(),
            })
        }
        other => Some(other.label().to_owned()),
    };
    let paths = state
        .ui
        .log_path_scope
        .as_ref()
        .map(|p| p.display().to_string());

    filter_dropdown(ui, state, LABEL_BRANCH, branch, SALT_BRANCH, branch_menu);
    filter_dropdown(ui, state, LABEL_USER, user, SALT_USER, user_menu);
    filter_dropdown(ui, state, LABEL_DATE, date, SALT_DATE, date_menu);
    filter_dropdown(ui, state, LABEL_PATHS, paths, SALT_PATHS, paths_menu);
    clear_button(ui, state);
}

// One open dropdown lives in Ui memory; IgnoreClicks keeps a Select… field alive while typing.
fn filter_dropdown(
    ui: &mut Ui,
    state: &mut AppState,
    base: &'static str,
    value: Option<String>,
    salt: &'static str,
    contents: fn(&mut Ui, &mut AppState) -> bool,
) {
    let active = value.is_some();
    let display = match &value {
        Some(v) => format!("{base}: {v}"),
        None => base.to_owned(),
    };
    let was_open = open_salt(ui) == Some(salt);
    let response = dropdown_button(ui, salt, &display, active, base);

    let mut open = was_open;
    if response.clicked() {
        open = !open;
    }

    let popup = Popup::from_response(&response)
        .id(popup_id(salt))
        .open_bool(&mut open)
        .close_behavior(PopupCloseBehavior::IgnoreClicks)
        .frame(widgets::menu_surface(ui))
        .width(POPUP_WIDTH)
        .gap(4.0)
        .layout(Layout::top_down_justified(Align::Min));

    let shown = popup.show(|popup_ui| contents(popup_ui, state));
    let mut close_requested = false;
    if let Some(inner) = shown {
        close_requested = inner.inner;
        let popup_rect = inner.response.rect;
        if was_open
            && ui.input(|i| {
                i.key_pressed(Key::Escape)
                    || (i.pointer.any_click()
                        && i.pointer
                            .interact_pos()
                            .is_some_and(|p| !popup_rect.contains(p) && !response.rect.contains(p)))
            })
        {
            open = false;
        }
    }
    if close_requested {
        open = false;
    }

    // Only the owning control writes the shared slot, so a closed sibling cannot clobber it.
    if was_open || response.clicked() {
        set_open(ui, open.then_some(salt));
    }
}

fn dropdown_button(
    ui: &mut Ui,
    salt: &'static str,
    display: &str,
    active: bool,
    base: &'static str,
) -> Response {
    let ink = if active {
        Palette::BRAND_INK
    } else {
        Palette::INK_2
    };
    let galley = ui
        .painter()
        .layout_no_wrap(display.to_owned(), chrome_font(TYPE_CONTROL), ink);
    let width = galley.size().x + 3.0 * BUTTON_PAD_X + CARET_SIZE;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, GROUP_ROW_HEIGHT), Sense::hover());
    let response = ui.interact(rect, button_id(salt), Sense::click());

    let fill = if active {
        Palette::SELECTION
    } else if response.hovered() {
        Palette::SURFACE_3
    } else {
        Palette::SURFACE_2
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    painter.galley(
        Pos2::new(
            rect.left() + BUTTON_PAD_X,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    let caret_ink = if active {
        Palette::BRAND
    } else {
        Palette::INK_3
    };
    icons::paint_icon(
        painter,
        Pos2::new(
            rect.right() - BUTTON_PAD_X - CARET_SIZE,
            rect.center().y - CARET_SIZE / 2.0,
        ),
        CARET_SIZE,
        Icon::CHEVRON_DOWN,
        caret_ink,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, base));
    widgets::focus_ring(ui, &response);
    response
}

fn menu_row(ui: &mut Ui, label: &str, active: bool, swatch: Option<Color32>) -> Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, GROUP_ROW_HEIGHT), Sense::click());

    let fill = if active {
        Palette::SELECTION
    } else if response.hovered() {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }
    let ink = if active {
        Palette::BRAND_INK
    } else {
        Palette::INK
    };
    let mut text_x = rect.left() + BUTTON_PAD_X;
    if let Some(color) = swatch {
        ui.painter().circle_filled(
            Pos2::new(rect.left() + BUTTON_PAD_X + 4.0, rect.center().y),
            3.5,
            color,
        );
        text_x += 14.0;
    }
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), chrome_font(TYPE_CONTROL), ink);
    ui.painter().galley(
        Pos2::new(text_x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    widgets::focus_ring(ui, &response);
    response
}

fn value_list(
    ui: &mut Ui,
    salt: &'static str,
    names: &[String],
    current: Option<&str>,
    noun: &str,
) -> Option<String> {
    if names.is_empty() {
        widgets::empty_state(ui, "No values available.");
        return None;
    }
    widgets::group_title(
        ui,
        &components::counted(names.len(), noun, &format!("{noun}s")),
    );
    let mut picked = None;
    ScrollArea::vertical()
        .id_salt(("tg_log_filter_list", salt))
        .max_height(LIST_MAX_HEIGHT)
        .show(ui, |ui| {
            for name in names {
                if menu_row(ui, name, current == Some(name.as_str()), None).clicked() {
                    picked = Some(name.clone());
                }
            }
        });
    picked
}

fn select_group(ui: &mut Ui, id: Id, text: &mut String, hint: &'static str) -> bool {
    let focus = menu_row(ui, LABEL_SELECT, false, None).clicked();
    let enter = ui.input(|i| i.key_pressed(Key::Enter));
    let mut commit = false;
    ui.horizontal(|ui| {
        let field = ui.add(
            TextEdit::singleline(text)
                .id(id)
                .hint_text(hint)
                .desired_width(POPUP_WIDTH - 96.0),
        );
        if focus {
            field.request_focus();
        }
        let apply = ui.button(APPLY);
        apply.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, APPLY));
        if apply.clicked() || (enter && (field.has_focus() || field.lost_focus())) {
            commit = true;
        }
    });
    commit
}

fn date_field(ui: &mut Ui, label: &str, text: &mut String, id: Id, enter: bool) -> bool {
    let mut commit = false;
    ui.horizontal(|ui| {
        ui.label(label);
        let field = ui.add(
            TextEdit::singleline(text)
                .id(id)
                .hint_text(DATE_HINT)
                .desired_width(POPUP_WIDTH - 110.0),
        );
        commit = enter && (field.has_focus() || field.lost_focus());
    });
    commit
}

fn branch_names(state: &AppState) -> Vec<String> {
    let mut names: Vec<String> = state
        .multi
        .roots
        .iter()
        .flat_map(|root| root.branches.iter().map(|b| b.name.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn author_names(state: &AppState) -> Vec<String> {
    let mut names: Vec<String> = state
        .multi
        .roots
        .iter()
        .filter_map(|root| state.caches.log(&root.id))
        .flat_map(|commits| commits.iter().map(|c| c.author.name.clone()))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn set_branch(state: &mut AppState, name: &str) {
    let root = state
        .selected_root
        .clone()
        .or_else(|| state.multi.roots.first().map(|r| r.id.clone()));
    state.ui.log_ref_scope = root.map(|r| (r, name.to_owned()));
}

fn apply_branch(state: &mut AppState) {
    let text = state.ui.log_branch_input.trim().to_owned();
    if text.is_empty() {
        state.ui.log_ref_scope = None;
    } else {
        set_branch(state, &text);
    }
}

fn apply_date(state: &mut AppState, now: i64) -> bool {
    let (Ok(after), Ok(before)) = (
        parse_day(&state.ui.log_date_after),
        parse_day(&state.ui.log_date_before),
    ) else {
        return false;
    };
    // A blank bound is an open end: epoch for "after", now for "before".
    state.ui.log_date_filter = match (after, before) {
        (None, None) => DateFilter::All,
        (a, b) => DateFilter::Period {
            after: a.unwrap_or(0),
            before: b.unwrap_or(now),
        },
    };
    true
}

fn branch_menu(ui: &mut Ui, state: &mut AppState) -> bool {
    let mut close = false;
    if menu_row(ui, LABEL_ALL, state.ui.log_ref_scope.is_none(), None).clicked() {
        state.ui.log_ref_scope = None;
        close = true;
    }
    widgets::menu_rule(ui);
    if select_group(
        ui,
        field_id(SALT_BRANCH),
        &mut state.ui.log_branch_input,
        "Branch name",
    ) {
        apply_branch(state);
        close = true;
    }
    widgets::menu_rule(ui);
    let names = branch_names(state);
    let current = state.ui.log_ref_scope.as_ref().map(|(_, n)| n.as_str());
    if let Some(name) = value_list(ui, SALT_BRANCH, &names, current, "branch") {
        set_branch(state, &name);
        close = true;
    }
    close
}

fn user_menu(ui: &mut Ui, state: &mut AppState) -> bool {
    let mut close = false;
    if menu_row(ui, LABEL_ALL, state.ui.log_author_filter.is_none(), None).clicked() {
        state.ui.log_author_filter = None;
        close = true;
    }
    widgets::menu_rule(ui);
    if select_group(
        ui,
        field_id(SALT_USER),
        &mut state.ui.log_author_input,
        "User name",
    ) {
        let text = state.ui.log_author_input.trim().to_owned();
        state.ui.log_author_filter = (!text.is_empty()).then_some(text);
        close = true;
    }
    widgets::menu_rule(ui);
    let names = author_names(state);
    let current = state.ui.log_author_filter.as_deref();
    if let Some(name) = value_list(ui, SALT_USER, &names, current, "user") {
        state.ui.log_author_filter = Some(name);
        close = true;
    }
    close
}

fn date_menu(ui: &mut Ui, state: &mut AppState) -> bool {
    let now = Utc::now().timestamp();
    let current = state.ui.log_date_filter;
    let mut close = false;

    if menu_row(ui, LABEL_ALL, current.is_all(), None).clicked() {
        state.ui.log_date_filter = DateFilter::All;
        close = true;
    }
    widgets::menu_rule(ui);
    if menu_row(
        ui,
        LABEL_LAST_DAY,
        matches!(current, DateFilter::Last24Hours { .. }),
        None,
    )
    .clicked()
    {
        state.ui.log_date_filter = DateFilter::Last24Hours { since: now - 86400 };
        close = true;
    }
    if menu_row(
        ui,
        LABEL_LAST_WEEK,
        matches!(current, DateFilter::Last7Days { .. }),
        None,
    )
    .clicked()
    {
        state.ui.log_date_filter = DateFilter::Last7Days {
            since: now - 7 * 86400,
        };
        close = true;
    }
    widgets::menu_rule(ui);
    widgets::group_title(ui, LABEL_SELECT_PERIOD);

    let enter = ui.input(|i| i.key_pressed(Key::Enter));
    let after_commit = date_field(
        ui,
        LABEL_AFTER,
        &mut state.ui.log_date_after,
        field_id(SALT_DATE_AFTER),
        enter,
    );
    let before_commit = date_field(
        ui,
        LABEL_BEFORE,
        &mut state.ui.log_date_before,
        field_id(SALT_DATE_BEFORE),
        enter,
    );
    let malformed = parse_day(&state.ui.log_date_after).is_err()
        || parse_day(&state.ui.log_date_before).is_err();
    if malformed {
        widgets::inline_error(ui, "Use YYYY-MM-DD.");
    }
    let apply = ui.button(APPLY);
    apply.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, APPLY));
    if (apply.clicked() || after_commit || before_commit) && apply_date(state, now) {
        close = true;
    }
    close
}

fn paths_menu(ui: &mut Ui, state: &mut AppState) -> bool {
    let mut close = false;
    if menu_row(ui, LABEL_ALL, state.ui.log_path_scope.is_none(), None).clicked() {
        state.ui.log_path_scope = None;
        close = true;
    }
    widgets::menu_rule(ui);
    if select_group(
        ui,
        field_id(SALT_PATHS),
        &mut state.ui.log_path_input,
        "Path",
    ) {
        let text = state.ui.log_path_input.trim().to_owned();
        state.ui.log_path_scope = (!text.is_empty()).then(|| PathBuf::from(text.as_str()));
        close = true;
    }
    widgets::menu_rule(ui);
    widgets::group_title(ui, LABEL_ROOTS);
    if menu_row(ui, ALL_ROOTS, state.ui.log_root_filter.is_none(), None).clicked() {
        state.ui.log_root_filter = None;
        close = true;
    }
    let roots: Vec<(Color32, String, RootId)> = state
        .multi
        .roots
        .iter()
        .enumerate()
        .map(|(idx, root)| (root_color(idx), root.id.name(), root.id.clone()))
        .collect();
    for (color, name, id) in roots {
        let active = state.ui.log_root_filter.as_ref() == Some(&id);
        if menu_row(ui, &name, active, Some(color)).clicked() {
            state.ui.log_root_filter = Some(id);
            close = true;
        }
    }
    close
}

fn filters_active(state: &AppState) -> bool {
    state.ui.log_ref_scope.is_some()
        || state.ui.log_author_filter.is_some()
        || !state.ui.log_date_filter.is_all()
        || state.ui.log_path_scope.is_some()
        || state.ui.log_root_filter.is_some()
        || !state.ui.log_filter.trim().is_empty()
}

fn clear_button(ui: &mut Ui, state: &mut AppState) {
    let active = filters_active(state);
    let response = ui.add_enabled(active, egui::Button::new(CLEAR));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, active, CLEAR));
    if response.clicked() {
        state.reset_log_filters();
    }
}
