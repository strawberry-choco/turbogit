//! Redesigned conflict resolver (issue #22, screen 07).
//!
//! Dedicated tool window with three EQUAL panes Local | Result | Incoming,
//! per-conflict actions (Take ours / Take theirs / Take both · theirs first),
//! per-conflict Undo, Prev/Next navigation, Auto-advance to next conflict,
//! "Apply to all N remaining", a left list splitting auto-merged files from
//! conflicted files, and header actions Abort merge / Continue merge.
//!
//! The Result pane is a FREE-TEXT editable TextEdit: when the user has
//! touched it, the typed text IS the resolved content on Apply (matches
//! screen 07's "Edit manually" button). When untouched, the resolved content
//! is the auto-composed buffer built from per-conflict choices.
//!
//! Conflict parsing reuses [`turbogit_services::conflict::parse_conflict_markers`]
//! over the working-tree file's markers; the structured 3-way merge path
//! (`diff_engine::merge_segments`) is unchanged.

use crate::theme::Palette;
use egui::{
    Color32, CornerRadius, Frame, Key, Margin, Modifiers, Rect, RichText, ScrollArea, Stroke,
    TextEdit, Ui, Vec2,
};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Toast};
use turbogit_services::conflict;

use std::path::{Path, PathBuf};

/// Per-conflict resolution choice (matches [`crate::ui::conflicts::compose_impl`]).
const CHOICE_OURS: u8 = 0;
const CHOICE_THEIRS: u8 = 1;
const CHOICE_BOTH_OURS_FIRST: u8 = 2; // existing "Ignore"
const CHOICE_BOTH_THEIRS_FIRST: u8 = 3; // "Take both · theirs first" (issue #22)

/// Build the segments list for the resolver's selected file, using the
/// in-process 3-way merge when enabled and available, otherwise the raw
/// marker parser (same path the inline editor uses).
fn merge_editor_segments(
    state: &AppState,
    root: &Option<PathBuf>,
    path: &Path,
    content: &str,
) -> (Vec<(String, String, bool)>, usize) {
    if state.settings.in_process_diffs
        && let Some(root) = root
        && let Ok(v) = conflict::read_versions(state.executor.as_ref(), root, path)
    {
        let segs = turbogit_services::diff_engine::merge_segments(&v.base, &v.ours, &v.theirs);
        let n = segs.iter().filter(|(_, _, c)| *c).count();
        return (segs, n);
    }
    conflict::parse_conflict_markers(content)
}

/// Compose the final file text from segments + per-conflict resolutions.
/// Unresolved blocks fall back to our side; this is only reachable before
/// Apply, which is gated at zero remaining.
fn compose(segs: &[(String, String, bool)], res: &[Option<u8>]) -> String {
    let mut out = String::new();
    let mut ci = 0usize;
    for (a, b, is_conf) in segs {
        if *is_conf {
            ci += 1;
            match res.get(ci - 1).copied().flatten() {
                Some(CHOICE_THEIRS) => out.push_str(b),
                Some(CHOICE_BOTH_OURS_FIRST) => {
                    out.push_str(a);
                    out.push_str(b);
                }
                Some(CHOICE_BOTH_THEIRS_FIRST) => {
                    out.push_str(b);
                    out.push_str(a);
                }
                Some(_) => out.push_str(a),
                None => out.push_str(a),
            }
        } else {
            out.push_str(a);
        }
    }
    out
}

/// Per-channel linear blend of `accent` over [`Palette::BG`] at opacity `t`.
fn tint_over_bg(accent: Color32, t: f32) -> Color32 {
    let bg = Palette::BG;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        mix(bg.r(), accent.r()),
        mix(bg.g(), accent.g()),
        mix(bg.b(), accent.b()),
    )
}

fn yours_bg() -> Color32 {
    tint_over_bg(Palette::STATE_INFO, 0.12)
}
fn theirs_bg() -> Color32 {
    tint_over_bg(Palette::STATE_ERROR, 0.12)
}
fn marker_bg() -> Color32 {
    tint_over_bg(Palette::STATE_WARNING, 0.15)
}

fn pane_header(ui: &mut Ui, title: &str, focused: bool) {
    let resp = Frame::new()
        .fill(Palette::SURFACE)
        .inner_margin(Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(16.0);
            ui.label(RichText::new(title).strong().size(12.0).color(Palette::INK));
        });
    if focused {
        ui.painter().rect_stroke(
            resp.response.rect,
            CornerRadius::same(2),
            Stroke::new(2.0, Palette::BRAND),
            egui::StrokeKind::Inside,
        );
    }
}

fn marker_strip(ui: &mut Ui, glyph: &str) {
    Frame::new()
        .fill(marker_bg())
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(glyph)
                    .monospace()
                    .size(10.0)
                    .color(Palette::STATE_WARNING),
            );
        });
}

fn side_section(ui: &mut Ui, text: &str, fill: Color32, strip: Color32) {
    let resp = Frame::new()
        .fill(fill)
        .inner_margin(Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(text).monospace().color(Palette::INK));
        });
    let r = resp.response.rect;
    ui.painter().rect_filled(
        Rect::from_min_size(r.left_top(), Vec2::new(3.0, r.height())),
        0.0,
        strip,
    );
}

/// READ-ONLY composed-result cell.
fn result_cell(ui: &mut Ui, chosen: Option<u8>, ours: &str, theirs: &str) {
    let (text, fill) = match chosen {
        Some(CHOICE_THEIRS) => (theirs.to_string(), Palette::SURFACE),
        Some(CHOICE_BOTH_OURS_FIRST) => (format!("{ours}{theirs}"), Palette::SURFACE),
        Some(CHOICE_BOTH_THEIRS_FIRST) => (format!("{theirs}{ours}"), Palette::SURFACE),
        Some(_) => (ours.to_string(), Palette::SURFACE),
        None => ("<< unresolved >>".to_string(), marker_bg()),
    };
    let resp = Frame::new()
        .fill(fill)
        .inner_margin(Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(text).monospace().color(Palette::INK));
        });
    ui.painter().rect_stroke(
        resp.response.rect,
        CornerRadius::same(2),
        Stroke::new(2.0, Palette::BRAND),
        egui::StrokeKind::Inside,
    );
}

/// Editable Result cell for the ACTIVE conflict. Wires a multiline TextEdit
/// over `state.ui.conflict_text`; typing flips `conflict_resolver_edited`
/// so Apply uses the typed text verbatim.
fn result_text_edit(ui: &mut Ui, state: &mut AppState) {
    let mut text = state.ui.conflict_text.clone();
    let resp = TextEdit::multiline(&mut text)
        .desired_width(ui.available_width())
        .desired_rows(6)
        .font(egui::TextStyle::Monospace)
        .show(ui);
    if resp.response.changed() {
        state.ui.conflict_text = text;
        state.ui.conflict_resolver_edited = true;
    }
}

/// Tinted side section carrying the conflict glyph + per-conflict action
/// buttons. The active conflict (per `state.ui.conflict_resolver_active_idx`)
/// is the one whose buttons render in the footer; others render compact
/// "Take ours" / "Take theirs" inline buttons.
fn conflict_block(
    ui: &mut Ui,
    state: &mut AppState,
    ci: usize,
    block: usize,
    ours: &str,
    theirs: &str,
) {
    let res_i = ci;
    let active = state.ui.conflict_resolver_active_idx == res_i;

    // Marker strips frame the discrete conflict block.
    ui.columns(3, |cols| {
        marker_strip(&mut cols[0], "<<<<<<<");
        marker_strip(&mut cols[1], "=======");
        marker_strip(&mut cols[2], ">>>>>>>");
    });
    // Tinted yours/theirs + editable Result.
    let chosen = state.ui.conflict_res.get(res_i).copied().flatten();
    ui.columns(3, |cols| {
        side_section(&mut cols[0], ours, yours_bg(), Palette::STATE_INFO);
        if active {
            result_text_edit(&mut cols[1], state);
        } else {
            result_cell(&mut cols[1], chosen, ours, theirs);
        }
        side_section(&mut cols[2], theirs, theirs_bg(), Palette::STATE_ERROR);
    });
    if active {
        // Per-conflict action row (only for the active conflict).
        ui.horizontal(|ui| {
            if ui.button(format!("Take ours {block}")).clicked() {
                resolve(state, res_i, CHOICE_OURS);
            }
            if ui.button(format!("Take theirs {block}")).clicked() {
                resolve(state, res_i, CHOICE_THEIRS);
            }
            if ui
                .button(format!("Take both {block}"))
                .on_hover_text("Theirs first")
                .clicked()
            {
                resolve(state, res_i, CHOICE_BOTH_THEIRS_FIRST);
            }
            // Undo is enabled whenever the conflict currently carries a
            // resolution — Undo always takes the conflict back to
            // unresolved. The undo stack records the prior choice so
            // future revisions can step through history.
            let can_undo = state
                .ui
                .conflict_res
                .get(res_i)
                .copied()
                .flatten()
                .is_some();
            if ui
                .add_enabled(can_undo, egui::Button::new(format!("Undo {block}")))
                .clicked()
            {
                undo_resolution(state, res_i);
            }
        });
    } else {
        // Compact row for non-active conflicts.
        ui.horizontal(|ui| {
            ui.label(format!("Conflict {block}"));
            if ui.button("Take ours").clicked() {
                resolve(state, res_i, CHOICE_OURS);
            }
            if ui.button("Take theirs").clicked() {
                resolve(state, res_i, CHOICE_THEIRS);
            }
            if ui.button("Take both").clicked() {
                resolve(state, res_i, CHOICE_BOTH_THEIRS_FIRST);
            }
        });
    }
    ui.add_space(4.0);
}

/// Record one block resolution: update the active choice, push the prior
/// choice onto the undo stack, refresh the composed buffer (unless the
/// user has free-text edited it), and honor Auto-advance.
fn resolve(state: &mut AppState, seg_idx: usize, choice: u8) {
    while state.ui.conflict_resolver_undo.len() <= seg_idx {
        state.ui.conflict_resolver_undo.push(None);
    }
    let prev = state.ui.conflict_res.get(seg_idx).copied().flatten();
    state.ui.conflict_resolver_undo[seg_idx] = prev;

    state.ui.conflict_res[seg_idx] = Some(choice);
    if !state.ui.conflict_resolver_edited {
        state.ui.conflict_text = compose(&state.ui.conflict_segs, &state.ui.conflict_res);
    }

    if state.ui.conflict_resolver_auto_advance {
        advance_active(state);
    }
}

fn undo_resolution(state: &mut AppState, seg_idx: usize) {
    // always takes you back to unresolved.
    state.ui.conflict_res[seg_idx] = None;
    if !state.ui.conflict_resolver_edited {
        state.ui.conflict_text = compose(&state.ui.conflict_segs, &state.ui.conflict_res);
    }
}
/// Move the active cursor to the next still-unresolved conflict (wrapping
/// at the end). No-op if every conflict is already resolved.
fn advance_active(state: &mut AppState) {
    let n = state.ui.conflict_res.len();
    if n == 0 {
        return;
    }
    let cur = state.ui.conflict_resolver_active_idx.min(n - 1);
    let mut i = (cur + 1) % n;
    let start = i;
    loop {
        if state.ui.conflict_res[i].is_none() {
            state.ui.conflict_resolver_active_idx = i;
            return;
        }
        i = (i + 1) % n;
        if i == start {
            return;
        }
    }
}

/// Open the redesigned resolver for one conflicted file.
pub(crate) fn open_resolver(state: &mut AppState, root: &Path, path: &Path) {
    let full = root.join(path);
    if let Ok(content) = std::fs::read_to_string(&full) {
        let (segs, _n) = merge_editor_segments(state, &Some(root.to_path_buf()), path, &content);
        let res: Vec<Option<u8>> = segs.iter().filter(|(_, _, c)| *c).map(|_| None).collect();
        state.ui.conflict_segs = segs;
        state.ui.conflict_res = res.clone();
        state.ui.conflict_resolver_undo = res.iter().map(|_| None).collect();
        state.ui.conflict_text = compose(&state.ui.conflict_segs, &state.ui.conflict_res);
        state.ui.conflict_resolver_edited = false;
        state.ui.conflict_resolver_active_idx = 0;
        state.ui.conflict_resolver_selected = Some(path.to_path_buf());
        state.ui.conflict_resolver_open = true;
    } else {
        state.ui.toast = Some(Toast::error("Could not read conflicted file."));
    }
}

/// Render the redesigned Resolve Conflicts tool window. No-op when the
/// resolver is closed or no root has any conflicts.
pub fn render(ui: &mut Ui, state: &mut AppState) {
    let id = match &state.selected_root {
        Some(id) => id.clone(),
        None => return,
    };
    let conflicted: Vec<PathBuf> = state
        .multi
        .by_id(&id)
        .map(|r| r.status.conflicted.clone())
        .unwrap_or_default();
    if conflicted.is_empty() && !state.ui.conflict_resolver_open {
        return;
    }
    // Refresh auto-merged + merge-in-progress on every render; cheap.
    let root = state.selected_path();
    if let Some(r) = &root {
        if r.join(".git").join("MERGE_HEAD").exists() {
            state.ui.merge_in_progress = true;
            if let Ok(list) = state.executor.merge_auto_merged_files(r, &conflicted) {
                state.ui.auto_merged_files = list;
            }
        } else {
            state.ui.merge_in_progress = false;
            state.ui.auto_merged_files.clear();
        }
    }

    if !state.ui.conflict_resolver_open {
        return;
    }
    let mut keep_open = true;

    egui::Window::new("Resolve conflicts")
        .open(&mut keep_open)
        .default_width(1100.0)
        .show(ui.ctx(), |ui| {
            render_resolver_body(ui, state, &conflicted);
        });
    if !keep_open {
        state.ui.conflict_resolver_open = false;
    }
}
fn render_file_list(ui: &mut Ui, state: &mut AppState, conflicted: &[PathBuf]) {
    ui.label(
        RichText::new("CONFLICTED FILES")
            .size(11.0)
            .color(Palette::INK_2),
    );
    ui.label(RichText::new(format!("{} / {} resolved", 0, conflicted.len())).color(Palette::INK_2));
    for path in conflicted {
        let label = path.display().to_string();
        let is_selected = state.ui.conflict_resolver_selected.as_ref() == Some(path);
        let resp = ui.selectable_label(is_selected, label.clone());
        if resp.clicked()
            && let Some(root) = state.selected_path()
        {
            open_resolver(state, &root, path);
        }
    }

    ui.add_space(8.0);
    let n_auto = state.ui.auto_merged_files.len();
    ui.label(
        RichText::new("AUTO-MERGED")
            .size(11.0)
            .color(Palette::INK_2),
    );
    ui.label(RichText::new(format!("{n_auto} files")).color(Palette::INK_2));
    for path in &state.ui.auto_merged_files {
        ui.label(path.display().to_string());
    }
}

fn render_resolver_body(ui: &mut Ui, state: &mut AppState, conflicted: &[PathBuf]) {
    // Header
    let path_disp = state
        .ui
        .conflict_resolver_selected
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no file)".to_string());
    let unresolved = state.ui.conflict_res.iter().filter(|r| r.is_none()).count();
    let total = state.ui.conflict_res.len();
    let counter = if total == 0 {
        "0 conflicts".to_string()
    } else if unresolved == 1 {
        "1 conflict remaining".to_string()
    } else {
        format!("{unresolved} conflicts remaining")
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(path_disp).strong().color(Palette::INK));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.ui.merge_in_progress && ui.button("Abort merge").clicked() {
                abort_merge(state);
            }
            if state.ui.merge_in_progress
                && ui
                    .add_enabled(unresolved == 0, egui::Button::new("Continue merge"))
                    .clicked()
            {
                continue_merge(state);
            }
        });
    });
    ui.label(RichText::new(counter).color(Palette::INK_2));
    ui.separator();

    // Left file list (auto-sized) + right panes.
    ui.horizontal(|ui| {
        ui.allocate_ui(egui::vec2(240.0, ui.available_height()), |ui| {
            render_file_list(ui, state, conflicted);
        });
        ui.vertical(|ui| {
            render_panes(ui, state);
        });
    });

    ui.separator();
    // Footer
    let total = state.ui.conflict_res.len();
    let n_remaining = state.ui.conflict_res.iter().filter(|r| r.is_none()).count();
    let active = state.ui.conflict_resolver_active_idx;
    ui.horizontal(|ui| {
        let label = format!("Conflict {}", active + 1);
        ui.label(RichText::new(label).color(Palette::INK_2));
        if ui
            .add_enabled(active > 0, egui::Button::new("< Prev"))
            .clicked()
        {
            state.ui.conflict_resolver_active_idx -= 1;
        }
        if ui
            .add_enabled(active + 1 < total, egui::Button::new("Next >"))
            .clicked()
        {
            state.ui.conflict_resolver_active_idx += 1;
        }
        if ui.button("Edit manually").clicked() {
            state.ui.conflict_resolver_edited = true;
        }
        // Show the button whenever at least one OTHER unresolved conflict
        // exists beyond the active one (the active choice gets propagated
        // to those). When the active conflict is itself unresolved, the
        // prior choice (or ours) is used; otherwise the active choice.
        if n_remaining > 0
            && ui
                .button(format!("Apply to all {n_remaining} remaining"))
                .clicked()
        {
            apply_to_all_remaining(state);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(unresolved == 0, egui::Button::new("Apply"))
                .clicked()
            {
                apply_resolution(state);
            }
            ui.checkbox(
                &mut state.ui.conflict_resolver_auto_advance,
                "Auto-advance to next conflict",
            );
        });
    });
    for path in conflicted {
        let label = path.display().to_string();
        let is_selected = state.ui.conflict_resolver_selected.as_ref() == Some(path);
        let resp = ui.selectable_label(is_selected, label.clone());
        if resp.clicked()
            && let Some(root) = state.selected_path()
        {
            open_resolver(state, &root, path);
        }
    }

    ui.add_space(8.0);
    let n_auto = state.ui.auto_merged_files.len();
    ui.label(
        RichText::new("AUTO-MERGED")
            .size(11.0)
            .color(Palette::INK_2),
    );
    ui.label(RichText::new(format!("{n_auto} files")).color(Palette::INK_2));
    for path in &state.ui.auto_merged_files {
        ui.label(path.display().to_string());
    }
}

fn render_panes(ui: &mut Ui, state: &mut AppState) {
    let segs = state.ui.conflict_segs.clone();
    // Pane headers.
    ui.columns(3, |cols| {
        pane_header(&mut cols[0], "Local (Yours)", false);
        pane_header(&mut cols[1], "Result", true);
        pane_header(&mut cols[2], "Incoming (Theirs)", false);
    });

    // Keyboard navigation: Alt+Up / Alt+Down move the active cursor.
    if ui.input(|i| i.key_pressed(Key::ArrowUp) && i.modifiers == Modifiers::ALT)
        && state.ui.conflict_resolver_active_idx > 0
    {
        state.ui.conflict_resolver_active_idx -= 1;
    }
    if ui.input(|i| i.key_pressed(Key::ArrowDown) && i.modifiers == Modifiers::ALT)
        && state.ui.conflict_resolver_active_idx + 1 < state.ui.conflict_res.len()
    {
        state.ui.conflict_resolver_active_idx += 1;
    }

    let mut ci = 0usize;
    for (ours, theirs, is_conf) in segs.iter() {
        if !*is_conf {
            let text = ours.as_str();
            ui.columns(3, |cols| {
                for col in cols.iter_mut() {
                    col.label(RichText::new(text).monospace().color(Palette::INK_2));
                }
            });
        } else {
            let block = ci + 1;
            conflict_block(ui, state, ci, block, ours, theirs);
            ci += 1;
        }
    }
}

/// "Apply to all N remaining": copies the active conflict's last-applied
/// choice (or ours as a fallback) to every still-unresolved conflict.
fn apply_to_all_remaining(state: &mut AppState) {
    let active = state.ui.conflict_resolver_active_idx;
    let choice = state
        .ui
        .conflict_resolver_undo
        .get(active)
        .copied()
        .flatten()
        .or_else(|| state.ui.conflict_res.get(active).copied().flatten())
        .unwrap_or(CHOICE_OURS);
    for i in 0..state.ui.conflict_res.len() {
        if state.ui.conflict_res[i].is_none() {
            state.ui.conflict_resolver_undo[i] = state.ui.conflict_res[i];
            state.ui.conflict_res[i] = Some(choice);
        }
    }
    if !state.ui.conflict_resolver_edited {
        state.ui.conflict_text = compose(&state.ui.conflict_segs, &state.ui.conflict_res);
    }
}

fn apply_resolution(state: &mut AppState) {
    let content = state.ui.conflict_text.clone();
    let r = state.selected_path();
    let p = state.ui.conflict_resolver_selected.clone();
    state.run_git(
        "Apply merge resolution".into(),
        Affected::from_optional_root(r.as_deref()),
        move |v| {
            if let (Some(r), Some(p)) = (r.as_ref(), p.as_ref()) {
                conflict::write_resolution(v, r, p, &content)
            } else {
                Ok(())
            }
        },
    );
    // Keep the resolver open while a merge is still pending Continue —
    // the file is staged but the merge commit hasn't been written yet.
    if !state.ui.merge_in_progress {
        state.ui.conflict_resolver_open = false;
    }
}

fn abort_merge(state: &mut AppState) {
    let r = state.selected_path();
    state.run_git(
        "Abort merge".into(),
        Affected::from_optional_root(r.as_deref()),
        move |v| {
            if let Some(r) = r.as_ref() {
                v.abort(r, "merge")
            } else {
                Ok(())
            }
        },
    );
    state.ui.conflict_resolver_open = false;
}

fn continue_merge(state: &mut AppState) {
    let r = state.selected_path();
    state.run_git(
        "Continue merge".into(),
        Affected::from_optional_root(r.as_deref()),
        move |v| {
            if let Some(r) = r.as_ref() {
                v.continue_op(r, "merge")
            } else {
                Ok(())
            }
        },
    );
    state.ui.conflict_resolver_open = false;
}

// Touch ScrollArea import so the helper signature stays available for the
// redesigned resolver's optional future scrolling panes.
#[allow(dead_code)]
fn _scroll_used() {
    let _ = ScrollArea::vertical;
}
