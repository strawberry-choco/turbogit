use crate::theme::{CONTROL_RADIUS, GROUP_ROW_HEIGHT, Palette, TYPE_CONTROL, chrome_font};
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use egui::{
    Align, Color32, CornerRadius, Id, Key, Layout, Order, Popup, PopupCloseBehavior, Pos2,
    Response, ScrollArea, Sense, TextEdit, Ui, Vec2, WidgetInfo, WidgetType,
};
use std::path::PathBuf;
use turbogit_app::log_filters::{
    DateFilter, Fold, LABEL_AFTER, LABEL_ALL, LABEL_BEFORE, LABEL_BRANCH, LABEL_DATE,
    LABEL_LAST_DAY, LABEL_LAST_WEEK, LABEL_PATHS, LABEL_ROOTS, LABEL_SELECT, LABEL_SELECT_PERIOD,
    LABEL_USER, RefBand, ref_bands,
};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;

const SALT_BRANCH: &str = "branch";
const SALT_USER: &str = "user";
const SALT_DATE: &str = "date";
const SALT_PATHS: &str = "paths";

const APPLY: &str = "Apply";
const CLEAR: &str = "Clear";
const ALL_REPOS: &str = "All repositories";

const BUTTON_PAD_X: f32 = 8.0;
const CARET_SIZE: f32 = 12.0;
const POPUP_WIDTH: f32 = 240.0;
const LIST_MAX_HEIGHT: f32 = 200.0;
const BRANCH_LIST_MAX_HEIGHT: f32 = 320.0;
const ROOTS_LIST_MAX_HEIGHT: f32 = 200.0;

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
    let branch = state
        .ui
        .log_ref_scope
        .as_ref()
        .map(|name| match carrier_count(state) {
            Some((carriers, selected)) => format!("{name} · {carriers} of {selected} repos"),
            None => name.clone(),
        });
    let user = state.ui.log_author_filter.clone();
    let date = state.ui.log_date_filter.readback();
    let paths = paths_readback(state);

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
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let click_pos = ui.input(|i| {
            i.pointer
                .any_click()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        // A nested picker paints in its own foreground layer; a click inside it
        // is not a click away from this dropdown.
        let clicked_away = click_pos.is_some_and(|p| {
            !popup_rect.contains(p)
                && !response.rect.contains(p)
                && !ui
                    .ctx()
                    .layer_id_at(p)
                    .is_some_and(|layer| layer.order != Order::Background)
        });
        if was_open && (escape || clicked_away) {
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

fn set_branch(state: &mut AppState, rev: &str) {
    // A typed rev IS the scope: the read fans out to the carriers at paint, so
    // the text needs no root and no resolution against the bands.
    state.ui.log_ref_scope = Some(rev.to_owned());
    state.ui.log_branch_input.clear();
    state.ui.selected_commit = None;
    state.ui.log_selected_file = None;
}

/// Some((carriers, selected)) while some selected repositories do NOT carry the
/// scoped name; `None` at rest or when every selection carries it.
fn carrier_count(state: &AppState) -> Option<(usize, usize)> {
    let selected = state.ui.log_roots.selected(&state.multi.roots).len();
    let carriers = turbogit_app::log_display::listing_roots(state).len();
    (carriers < selected).then_some((carriers, selected))
}

fn paths_readback(state: &AppState) -> Option<String> {
    let roots_note = if state.ui.log_roots.is_all() {
        None
    } else {
        let count = state.ui.log_roots.selected(&state.multi.roots).len();
        Some(if count == 0 {
            "No repos".to_owned()
        } else {
            format!("{count} of {} repos", state.multi.roots.len())
        })
    };
    match (&state.ui.log_path_scope, roots_note) {
        (Some((_, path)), None) => Some(path.display().to_string()),
        (Some((_, path)), Some(note)) => Some(format!("{} · {note}", path.display())),
        (None, note) => note,
    }
}

fn branch_bands(state: &mut AppState) -> Vec<RefBand> {
    state.request_branch_tags();
    let selected = state.ui.log_roots.selected(&state.multi.roots);
    ref_bands(
        &selected,
        state.caches.tags_by_root(),
        &state.ui.log_ref_folds,
    )
}

/// The Branch menu's own width, fitted to the refs its open bands paint. A folded band
/// paints no row, so it must not count; the reveal that adds one is what widens the menu.
fn branch_menu_width(ui: &Ui, bands: &[RefBand]) -> f32 {
    let widest = std::iter::once(LABEL_ALL)
        .chain(
            bands
                .iter()
                .filter(|band| band.open)
                .flat_map(|band| band.revs.iter().map(String::as_str)),
        )
        .map(|text| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), chrome_font(TYPE_CONTROL), Palette::INK)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    // The text starts `BUTTON_PAD_X` in from the row's left and the same inset stays on
    // the right; the frame's margin lives outside this content width.
    let margin = ui.style().spacing.menu_margin.sum().x;
    let floor = POPUP_WIDTH - margin;
    let ceiling = (ui.ctx().content_rect().width() - margin).max(floor);
    (widest + 2.0 * BUTTON_PAD_X).clamp(floor, ceiling)
}

enum BandRow<'a> {
    Header(&'a RefBand),
    Rev(&'a str),
}

// Both row kinds are exactly `GROUP_ROW_HEIGHT` tall because `show_rows` gives every
// row one pitch; a header that grew taller than a rev row would drift the whole list.
fn band_rows(bands: &[RefBand]) -> Vec<BandRow<'_>> {
    let mut rows = Vec::new();
    for band in bands {
        rows.push(BandRow::Header(band));
        if band.open {
            rows.extend(band.revs.iter().map(|rev| BandRow::Rev(rev)));
        }
    }
    rows
}

fn branch_menu(ui: &mut Ui, state: &mut AppState) -> bool {
    let bands = branch_bands(state);
    // Set every frame so the `Area` follows a reveal or a fold, not only its first size.
    ui.set_width(branch_menu_width(ui, &bands));

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
        let text = state.ui.log_branch_input.trim().to_owned();
        if text.is_empty() {
            state.ui.log_ref_scope = None;
            state.ui.log_branch_input.clear();
        } else {
            set_branch(state, &text);
        }
        close = true;
    }
    widgets::menu_rule(ui);
    if bands.is_empty() {
        widgets::empty_state(ui, "No values available.");
        return close;
    }
    let scope = state.ui.log_ref_scope.clone();
    let rows = band_rows(&bands);
    // The list box is reserved rather than fitted to its contents. An `Area` keeps the size
    // it was first laid out with, so a popup that grows when a folded band is revealed
    // leaves its new rows outside the popup's own rect, where they paint but take no click.
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), BRANCH_LIST_MAX_HEIGHT),
        Layout::top_down(Align::Min),
        |ui| {
            ScrollArea::vertical()
                .id_salt(("tg_log_filter_list", SALT_BRANCH))
                .auto_shrink([false, false])
                .show_rows(ui, GROUP_ROW_HEIGHT, rows.len(), |ui, visible| {
                    for index in visible {
                        match rows[index] {
                            BandRow::Header(band) => {
                                let header = components::section_header(
                                    ui,
                                    band.title,
                                    band.revs.len(),
                                    band.open,
                                    |_| {},
                                );
                                if header.clicked() {
                                    let fold = if band.open { Fold::Closed } else { Fold::Open };
                                    state.ui.log_ref_folds.insert(band.group, fold);
                                }
                            }
                            BandRow::Rev(rev) => {
                                let active = scope.as_deref() == Some(rev);
                                if menu_row(ui, rev, active, None).clicked() {
                                    set_branch(state, rev);
                                    close = true;
                                }
                            }
                        }
                    }
                });
        },
    );
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundSlot {
    After,
    Before,
}

fn bound_of(filter: DateFilter, slot: BoundSlot) -> Option<DateTime<Utc>> {
    match slot {
        BoundSlot::After => filter.after(),
        BoundSlot::Before => filter.before(),
    }
}

fn bound_seed(slot: BoundSlot, now: DateTime<Utc>) -> NaiveDateTime {
    let (h, m, s) = match slot {
        BoundSlot::After => (0, 0, 0),
        // `--until` is inclusive. End the seeded upper bound at 23:59:59, not midnight.
        BoundSlot::Before => (23, 59, 59),
    };
    let local = now.with_timezone(&Local);
    local
        .date_naive()
        .and_hms_opt(h, m, s)
        .unwrap_or_else(|| local.naive_local())
}

fn wall_of(filter: DateFilter, slot: BoundSlot) -> Option<NaiveDateTime> {
    bound_of(filter, slot).map(|instant| instant.with_timezone(&Local).naive_local())
}

fn utc_of(wall: NaiveDateTime) -> DateTime<Utc> {
    // A local wall time can be ambiguous or missing across a DST change, so take
    // the earliest reading and fall back to interpreting it as UTC.
    Local
        .from_local_datetime(&wall)
        .earliest()
        .unwrap_or_else(|| Local.from_utc_datetime(&wall))
        .with_timezone(&Utc)
}

fn bound_row(ui: &mut Ui, slot: BoundSlot, active: bool, value: Option<DateTime<Utc>>) -> bool {
    let label = match slot {
        BoundSlot::After => LABEL_AFTER,
        BoundSlot::Before => LABEL_BEFORE,
    };
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
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), chrome_font(TYPE_CONTROL), ink);
    ui.painter().galley(
        Pos2::new(
            rect.left() + BUTTON_PAD_X,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    let value_text = value
        .map(|v| v.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "Any".to_owned());
    let value_galley = ui
        .painter()
        .layout_no_wrap(value_text, chrome_font(TYPE_CONTROL), ink);
    ui.painter().galley(
        Pos2::new(
            rect.right() - BUTTON_PAD_X - value_galley.size().x,
            rect.center().y - value_galley.size().y / 2.0,
        ),
        value_galley,
        ink,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    widgets::focus_ring(ui, &response);
    response.clicked()
}

fn date_menu(ui: &mut Ui, state: &mut AppState) -> bool {
    let now = Utc::now();
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
        state.ui.log_date_filter = DateFilter::Last24Hours {
            since: now.timestamp() - 86400,
        };
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
            since: now.timestamp() - 7 * 86400,
        };
        close = true;
    }
    widgets::menu_rule(ui);
    widgets::group_title(ui, &format!("{LABEL_SELECT_PERIOD} (local time)"));

    let slot_id = Id::new("tg_log_filter_date_active_bound");
    let mut slot = ui
        .ctx()
        .data(|d| d.get_temp::<BoundSlot>(slot_id))
        .unwrap_or(BoundSlot::After);
    let after = current.after();
    let before = current.before();
    if bound_row(ui, BoundSlot::After, slot == BoundSlot::After, after) {
        slot = BoundSlot::After;
    }
    if bound_row(ui, BoundSlot::Before, slot == BoundSlot::Before, before) {
        slot = BoundSlot::Before;
    }
    ui.ctx().data_mut(|d| d.insert_temp(slot_id, slot));

    ui.horizontal(|ui| {
        if ui
            .add_enabled(after.is_some(), egui::Button::new("Clear after"))
            .clicked()
        {
            state.ui.log_date_filter = DateFilter::from_bounds(None, before);
        }
        if ui
            .add_enabled(before.is_some(), egui::Button::new("Clear before"))
            .clicked()
        {
            state.ui.log_date_filter = DateFilter::from_bounds(after, None);
        }
    });

    let wall = wall_of(current, slot).unwrap_or_else(|| bound_seed(slot, now));
    if let Some(picked) = widgets::instant_picker(ui, field_id(SALT_DATE), wall) {
        let instant = utc_of(picked);
        let (after, before) = (current.after(), current.before());
        state.ui.log_date_filter = match slot {
            BoundSlot::After => DateFilter::from_bounds(Some(instant), before),
            BoundSlot::Before => DateFilter::from_bounds(after, Some(instant)),
        };
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
        // A typed path names no repository: read the one the row selection is
        // about, and fall back to the first registered root at rest.
        let root = state
            .ui
            .selected_commit
            .as_ref()
            .map(|(root, _)| root.clone())
            .or_else(|| state.multi.roots.first().map(|root| root.id.clone()));
        state.ui.log_path_scope = if text.is_empty() {
            None
        } else {
            root.map(|root| (root, PathBuf::from(text.as_str())))
        };
        close = true;
    }
    widgets::menu_rule(ui);
    widgets::group_title(ui, LABEL_ROOTS);
    // The Roots block edits a set and never closes; the Path axis commits one
    // value and closes. The box is reserved like the branch list because the
    // popup is now long-lived and a repository attached while it is open grows
    // these rows below the area's first-frame size.
    let roots: Vec<(Color32, String, RootId)> = state
        .multi
        .roots
        .iter()
        .enumerate()
        .map(|(idx, root)| (root_color(idx), root.id.name(), root.id.clone()))
        .collect();
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), ROOTS_LIST_MAX_HEIGHT),
        Layout::top_down(Align::Min),
        |ui| {
            ScrollArea::vertical()
                .id_salt(("tg_log_filter_list", SALT_PATHS))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // The `All` row is a command, not a checkbox: a checked box
                    // whose click only re-selects everything breaks the
                    // contract its four checkbox neighbours keep.
                    if menu_row(ui, ALL_REPOS, state.ui.log_roots.is_all(), None).clicked() {
                        state.ui.log_roots.select_all();
                    }
                    for (color, name, id) in &roots {
                        let checked = state.ui.log_roots.carries(id);
                        if widgets::check_row(ui, name, checked, Some(*color)).clicked() {
                            state.ui.log_roots.toggle(id, &state.multi.roots);
                        }
                    }
                });
        },
    );
    close
}

fn filters_active(state: &AppState) -> bool {
    state.ui.log_ref_scope.is_some()
        || state.ui.log_author_filter.is_some()
        || !state.ui.log_date_filter.is_all()
        || state.ui.log_path_scope.is_some()
        || !state.ui.log_roots.is_all()
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
