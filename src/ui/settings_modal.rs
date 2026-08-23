//! Settings modal (issue #16, spec §8.8): a ~768px category-list dialog
//! opened ONLY from the toolbar gear — the tab-strip Settings page was
//! deleted outright (spec §9.1 correction).
//!
//! Editing model: opening snapshots the loaded [`VcsSettings`] into
//! [`UiState::settings_draft`]; rows edit the draft only. The footer follows
//! IDE semantics — **Reset** restores the loaded values, **Cancel** (and the
//! window X) discards the draft, **Apply** persists via `persistence.rs`,
//! rebuilds the engine behind the seam (ADR-0001) and stays open. Apply is
//! disabled until the draft differs from the loaded settings.
//!
//! Backed rows bind to persisted fields; unbacked placeholders (CRLF
//! conversion, commit checks, Manage Remotes, Notifications/Keymap
//! categories) render visible-but-disabled with tooltips and never persist.

use std::sync::Arc;

use egui::{
    Align, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, RichText, Sense, TextEdit, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::widgets;
use crate::model::{CleanTreeMethod, DateFormat, UpdateMethod, VcsSettings};
use crate::state::AppState;
use crate::theme::Palette;

/// Spec §8.8: large modal, ~768px wide.
const MODAL_WIDTH: f32 = 768.0;
/// Left category list width (spec §8.8: 176px).
const CATEGORY_WIDTH: f32 = 176.0;
/// Right pane width: whatever the modal body offers beyond the category
/// column and the 1px divider. Pinned explicitly — an unconstrained
/// ScrollArea inside a Window feeds back on itself and grows forever.
const PANEL_WIDTH: f32 = MODAL_WIDTH - CATEGORY_WIDTH - 40.0;
/// Fixed body height so both columns align and the window keeps its size.
const PANEL_HEIGHT: f32 = 420.0;
const ROW_HEIGHT: f32 = 26.0;
const INPUT_WIDTH: f32 = 300.0;

/// Render the modal when open; a closed window is a no-op.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.settings_open {
        return;
    }
    // Snapshot the loaded values once per open: the dirty reference every
    // footer decision compares against.
    if state.ui.settings_draft.is_none() {
        state.ui.settings_draft = Some(state.settings.clone());
    }

    let ctx = ui.ctx().clone();
    let mut open = state.ui.settings_open;
    egui::Window::new("Settings")
        .open(&mut open)
        .default_width(MODAL_WIDTH)
        .resizable(false)
        .show(&ctx, |ui| body(ui, state));
    // Two close paths meet here: the window X / Esc flip `open`, the footer
    // Cancel cleared `settings_open` from inside the body. Either discards.
    let body_cancelled = !state.ui.settings_open;
    state.ui.settings_open = open && !body_cancelled;
    if !state.ui.settings_open {
        state.ui.settings_draft = None;
    }
}

// --- Body ---------------------------------------------------------------------

fn body(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        category_list(ui);
        paint_vertical_line(ui, PANEL_HEIGHT);
        settings_panel(ui, state);
    });
    ui.add_space(10.0);
    footer(ui, state);
}

// --- Left column: category list -------------------------------------------------

fn category_list(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(CATEGORY_WIDTH, PANEL_HEIGHT), Sense::hover());
    let mut col = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(Align::Min)),
    );
    col.add_space(4.0);

    // v1 ships one backed page; Notifications/Keymap are disabled
    // placeholders until their features land (never selectable/persisted).
    category_row(&mut col, "Version Control", true, true);
    category_row(&mut col, "Notifications", false, false);
    category_row(&mut col, "Keymap", false, false);
}

/// One category row. Returns `true` only when an enabled row was clicked.
fn category_row(ui: &mut Ui, label: &str, selected: bool, enabled: bool) -> bool {
    let width = ui.available_width();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), sense);

    let hovered = enabled && response.hovered();
    let fill = widgets::row_fill(selected, hovered);
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    }
    let ink = if selected {
        Palette::BRAND_INK
    } else if !enabled {
        Palette::INK_3
    } else if hovered {
        Palette::INK
    } else {
        Palette::INK_2
    };
    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::new(13.0, FontFamily::Proportional),
        ink,
    );

    let closure_label = label.to_owned();
    response.widget_info(move || {
        WidgetInfo::labeled(WidgetType::Button, enabled, closure_label.clone())
    });

    if !enabled {
        response.on_disabled_hover_text("No configurable settings here yet");
        return false;
    }
    response.clicked()
}

// --- Right pane: setting rows ---------------------------------------------------

fn settings_panel(ui: &mut Ui, state: &mut AppState) {
    let Some(draft) = state.ui.settings_draft.as_mut() else {
        return;
    };
    // Fixed-size pane: the ScrollArea scrolls, it never resizes the window.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(PANEL_WIDTH, PANEL_HEIGHT), Sense::hover());
    let mut pane = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(Align::Min)),
    );
    egui::ScrollArea::vertical()
        .max_height(PANEL_HEIGHT)
        .show(&mut pane, |ui| version_control_page(ui, draft));
}

/// Every backed row of the Version Control page plus the visible-but-
/// disabled placeholders. All edits land on the draft only.
fn version_control_page(ui: &mut Ui, s: &mut VcsSettings) {
    setting_row(
        ui,
        "Git executable path",
        "Blank resolves git from PATH.",
        |ui| {
            labeled_text_input(ui, "Git executable path", &mut s.git_executable);
        },
    );

    ui.add_space(4.0);
    ui.checkbox(
        &mut s.staging_area,
        "Use staging area instead of classic commit",
    );
    ui.checkbox(
        &mut s.synchronous_branches,
        "Sync branch operations across roots",
    );

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Update method:");
        egui::ComboBox::from_id_salt("settings_update_method")
            .selected_text(update_method_label(s.update_method))
            .width(140.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut s.update_method, UpdateMethod::Merge, "Merge");
                ui.selectable_value(&mut s.update_method, UpdateMethod::Rebase, "Rebase");
            });
    });
    ui.horizontal(|ui| {
        ui.label("Clean-tree method:");
        egui::ComboBox::from_id_salt("settings_clean_tree_method")
            .selected_text(clean_tree_label(s.clean_tree_method))
            .width(140.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut s.clean_tree_method, CleanTreeMethod::Stash, "Stash");
                ui.selectable_value(&mut s.clean_tree_method, CleanTreeMethod::Shelve, "Shelve");
            });
    });

    ui.add_space(6.0);
    setting_row(
        ui,
        "Protected branches patterns",
        "Comma separated, e.g. main, release/*",
        |ui| {
            let mut pats = s.protected_branch_patterns.join(", ");
            let resp = labeled_text_input(ui, "Protected branches patterns", &mut pats);
            if resp.changed() {
                s.protected_branch_patterns = pats
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
            }
        },
    );

    ui.add_space(4.0);
    let remotes = ui
        .scope(|ui| {
            ui.disable();
            widgets::ghost_button(ui, None, "Manage Remotes")
        })
        .inner;
    remotes.on_disabled_hover_text("The remote manager is not available yet");

    ui.separator();
    ui.checkbox(&mut s.warn_crlf, "Warn before committing CRLF");

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Date format:");
        egui::ComboBox::from_id_salt("settings_date_format")
            .selected_text(date_format_label(s.date_format))
            .width(140.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut s.date_format, DateFormat::Relative, "Relative");
                ui.selectable_value(&mut s.date_format, DateFormat::Absolute, "Absolute");
                ui.selectable_value(&mut s.date_format, DateFormat::Iso, "ISO");
            });
    });

    ui.add_space(6.0);
    ui.label(RichText::new("CRLF conversion").weak());
    let tooltip = "Not backed by persisted settings yet";
    for (checked, label) in [
        (true, "Convert to LF on commit"),
        (false, "Convert to CRLF on checkout"),
        (false, "No conversion"),
    ] {
        let resp = ui.add_enabled(false, egui::RadioButton::new(checked, label));
        resp.on_disabled_hover_text(tooltip);
    }

    ui.add_space(6.0);
    ui.label(RichText::new("Commit checks").weak());
    for label in ["Run git commit hooks", "Sign-off commits"] {
        let mut off = false;
        let resp = ui.add_enabled(false, egui::Checkbox::new(&mut off, label));
        resp.on_disabled_hover_text(tooltip);
    }
}

fn update_method_label(m: UpdateMethod) -> &'static str {
    match m {
        UpdateMethod::Merge => "Merge",
        UpdateMethod::Rebase => "Rebase",
    }
}

fn clean_tree_label(m: CleanTreeMethod) -> &'static str {
    match m {
        CleanTreeMethod::Stash => "Stash",
        CleanTreeMethod::Shelve => "Shelve",
    }
}

fn date_format_label(f: DateFormat) -> &'static str {
    match f {
        DateFormat::Relative => "Relative",
        DateFormat::Absolute => "Absolute",
        DateFormat::Iso => "ISO",
    }
}

/// Row chrome: bold-ish label with optional description on the left, control
/// right-aligned (spec §8.8 row anatomy).
fn setting_row(ui: &mut Ui, label: &str, description: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(label);
            ui.label(RichText::new(description).small().weak());
        });
        ui.with_layout(egui::Layout::right_to_left(Align::Center), control);
    });
}

/// Single-line input made queryable by an explicit accessible label (the
/// repo-wide kittest pattern, cf. `widgets::input_frame`).
fn labeled_text_input(ui: &mut Ui, label: &str, buf: &mut String) -> egui::Response {
    let resp = ui.add(TextEdit::singleline(buf).desired_width(INPUT_WIDTH));
    let closure_label = label.to_owned();
    resp.widget_info(move || {
        WidgetInfo::labeled(WidgetType::TextEdit, true, closure_label.clone())
    });
    resp
}

fn paint_vertical_line(ui: &mut Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::LINE);
}

// --- Footer ---------------------------------------------------------------------

fn footer(ui: &mut Ui, state: &mut AppState) {
    let dirty = state
        .ui
        .settings_draft
        .as_ref()
        .is_some_and(|d| *d != state.settings);
    widgets::dialog_footer(ui, |ui| {
        // right-to-left layout: the first button renders rightmost.
        let applied = ui
            .scope(|ui| {
                if !dirty {
                    ui.disable();
                }
                widgets::compact_button(ui, "Apply")
            })
            .inner;
        if applied.clicked() {
            apply_settings(state);
        }
        if widgets::compact_button(ui, "Cancel").clicked() {
            // `show` reconciles this with the window X and drops the draft.
            state.ui.settings_open = false;
        }
        if widgets::compact_button(ui, "Reset").clicked() {
            reset_settings(state);
        }
    });
}

/// Persist the draft into live state + `.turbogit/state.ron`, rebuild the
/// engine behind the seam (ADR-0001: a changed git binary applies live), and
/// keep the modal open with a clean draft.
fn apply_settings(state: &mut AppState) {
    let Some(draft) = state.ui.settings_draft.clone() else {
        return;
    };
    state.settings = draft;
    let _ = crate::persistence::save_settings(&state.project_dir, &state.settings);
    state.executor = Arc::new(crate::engine::cli::CliExecutor {
        settings: state.settings.clone(),
    });
    state.persist_ui();
    state.ui.toast = Some("✓ Settings saved".into());
}

/// Restore the draft from the loaded values (footer Reset).
fn reset_settings(state: &mut AppState) {
    state.ui.settings_draft = Some(state.settings.clone());
}
