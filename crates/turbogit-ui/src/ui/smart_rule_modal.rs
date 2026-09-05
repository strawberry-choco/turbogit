//! Smart group rule editor (issue #07): a small modal opened from the
//! sidebar's SMART GROUPS "+" (new rule) or a rule row's pencil (edit).
//!
//! Editing model follows the Settings modal: opening snapshots a draft
//! rule into [`turbogit_app::state::UiState::smart_rule_draft`]; rows edit
//! the draft only. **Save** commits the draft into
//! `UiState::smart_group_rules` (replacing the edited entry, appending a
//! new one) and persists the workspace's `ui.ron`; **Cancel** (and the
//! window X) discards. Save is disabled until the label is non-empty;
//! blank patterns store as "any" (`None`).

use egui::{FontFamily, FontId, RichText, TextEdit, Ui, WidgetInfo, WidgetType};

use super::widgets;
use crate::theme::Palette;
use turbogit_app::smart_rules::SmartGroupRule;
use turbogit_app::state::{AppState, Toast};

const MODAL_WIDTH: f32 = 440.0;
/// Pattern inputs share their row with a caption; the name field does not.
const PATTERN_WIDTH: f32 = 200.0;

/// Render the modal when open; a closed window is a no-op.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.smart_rule_editor_open {
        return;
    }
    // Snapshot a blank draft once per open (the caller pre-seeds the draft
    // when editing an existing rule).
    if state.ui.smart_rule_draft.is_none() {
        state.ui.smart_rule_draft = Some(SmartGroupRule::default());
    }

    let ctx = ui.ctx().clone();
    let mut open = state.ui.smart_rule_editor_open;
    egui::Window::new("Smart Group Rule")
        .open(&mut open)
        .default_width(MODAL_WIDTH)
        .resizable(false)
        .show(&ctx, |ui| body(ui, state));
    // Two close paths meet here: the window X / Esc flip `open`, the footer
    // Cancel cleared the flag from inside the body. Either discards.
    let body_cancelled = !state.ui.smart_rule_editor_open;
    state.ui.smart_rule_editor_open = open && !body_cancelled;
    if !state.ui.smart_rule_editor_open {
        state.ui.smart_rule_draft = None;
        state.ui.smart_rule_editing = None;
    }
}

fn body(ui: &mut Ui, state: &mut AppState) {
    let Some(draft) = state.ui.smart_rule_draft.as_mut() else {
        return;
    };

    ui.add_space(4.0);
    ui.label(
        RichText::new("NAME")
            .strong()
            .font(FontId::new(11.0, FontFamily::Proportional))
            .color(Palette::INK_3),
    );
    labeled_text_input(ui, "Rule name", &mut draft.label, MODAL_WIDTH - 40.0);

    ui.add_space(8.0);
    ui.label(
        RichText::new("PREDICATES — BLANK MEANS ANY")
            .strong()
            .font(FontId::new(11.0, FontFamily::Proportional))
            .color(Palette::INK_3),
    );
    pattern_row(
        ui,
        "Name / path pattern",
        "Rule name/path pattern",
        &mut draft.path_pattern,
    );
    pattern_row(
        ui,
        "Branch pattern",
        "Rule branch pattern",
        &mut draft.branch_pattern,
    );

    ui.add_space(8.0);
    threshold_row(
        ui,
        "Dirty: at least this many changed paths",
        &mut draft.min_dirty,
    );
    threshold_row(
        ui,
        "Ahead: at least this many outgoing commits",
        &mut draft.min_ahead,
    );
    threshold_row(
        ui,
        "Behind: at least this many incoming commits",
        &mut draft.min_behind,
    );

    ui.add_space(10.0);
    footer(ui, state);
}

/// One wildcard pattern row: a single-line input whose blank content reads
/// as "any" (`None`).
fn pattern_row(ui: &mut Ui, caption: &str, label: &str, slot: &mut Option<String>) {
    let mut buf = slot.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new(caption).color(Palette::INK_2));
        let resp = labeled_text_input(ui, label, &mut buf, PATTERN_WIDTH);
        if resp.changed() {
            *slot = Some(buf.clone()).filter(|s| !s.trim().is_empty());
        }
    });
}

/// One count threshold row: an enable checkbox plus a ≥-number stepper.
/// Unchecked stores `None` (no constraint); checking defaults to 1.
fn threshold_row(ui: &mut Ui, caption: &str, slot: &mut Option<usize>) {
    let mut on = slot.is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut on, caption).changed() {
            *slot = on.then_some(1);
        }
        let mut n = slot.unwrap_or(1);
        let resp = ui.add_enabled(
            on,
            egui::DragValue::new(&mut n).range(0..=999_999).prefix("≥ "),
        );
        if resp.changed() {
            *slot = Some(n);
        }
    });
}

fn labeled_text_input(ui: &mut Ui, label: &str, buf: &mut String, width: f32) -> egui::Response {
    // Explicit id_salt: auto ids collide for the pattern rows (each lives
    // in its own horizontal child), which would fuse the inputs.
    let resp = ui.add(
        TextEdit::singleline(buf)
            .id_salt(label)
            .desired_width(width),
    );
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, label));
    resp
}

fn footer(ui: &mut Ui, state: &mut AppState) {
    let valid = state
        .ui
        .smart_rule_draft
        .as_ref()
        .is_some_and(|d| !d.label.trim().is_empty());
    widgets::dialog_footer(ui, |ui| {
        // right-to-left layout: the first button renders rightmost.
        let saved = ui
            .scope(|ui| {
                if !valid {
                    ui.disable();
                }
                widgets::compact_button(ui, "Save")
            })
            .inner;
        if saved.clicked() {
            save_rule(state);
        }
        if widgets::compact_button(ui, "Cancel").clicked() {
            // `show` reconciles this with the window X and drops the draft.
            state.ui.smart_rule_editor_open = false;
        }
    });
}

/// Commit the draft into live state + `.turbogit/ui.ron` and close.
fn save_rule(state: &mut AppState) {
    let Some(mut rule) = state.ui.smart_rule_draft.clone() else {
        return;
    };
    rule.label = rule.label.trim().to_string();
    if rule.label.is_empty() {
        return;
    }
    for pattern in [&mut rule.path_pattern, &mut rule.branch_pattern] {
        *pattern = pattern
            .take()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
    }
    match state.ui.smart_rule_editing {
        Some(ix) if ix < state.ui.smart_group_rules.len() => {
            state.ui.smart_group_rules[ix] = rule;
        }
        _ => state.ui.smart_group_rules.push(rule),
    }
    state.persist_ui();
    state.ui.toast = Some(Toast::success("Smart group rule saved"));
    state.ui.smart_rule_editor_open = false;
}
