//! Conflict resolution (G1–G9): list conflicted files, accept ours/theirs,
//! resolve-all-simple, and the redesigned 3-way merge editor (issue #15):
//! three EQUAL panes Local | Result | Incoming with discrete conflict blocks
//! (marker strips + tinted yours/theirs sections), per-block Accept buttons
//! driving a READ-ONLY composed Result, an "N conflicts remaining" counter,
//! and an Apply gated at zero remaining (spec §8.7; free-text editing is
//! explicitly deferred).

use crate::core::conflict;
use crate::state::{AppState, Toast};
use crate::theme::Palette;
use egui::{Color32, CornerRadius, Margin, Rect, RichText, ScrollArea, Stroke, Ui, Vec2};

/// Parse a file's conflict markers into alternating normal / conflict blocks.
/// Returns (segments, conflict_count) where each segment is
/// `(ours, theirs, is_conflict)`; for normal segments `theirs` is empty.
fn parse_conflicts(content: &str) -> (Vec<(String, String, bool)>, usize) {
    let mut segs: Vec<(String, String, bool)> = Vec::new();
    let mut conflicts = 0usize;
    let mut normal = String::new();
    let mut ours = String::new();
    let mut theirs = String::new();
    // mode: 0 = normal, 1 = inside ours, 2 = inside theirs
    let mut mode = 0u8;
    for line in content.lines() {
        if line.starts_with("<<<<<<<") {
            if !normal.is_empty() {
                segs.push((std::mem::take(&mut normal), String::new(), false));
            }
            mode = 1;
            conflicts += 1;
            ours.clear();
            theirs.clear();
        } else if line.starts_with("=======") && mode == 1 {
            mode = 2;
        } else if line.starts_with(">>>>>>>") && (mode == 1 || mode == 2) {
            segs.push((std::mem::take(&mut ours), std::mem::take(&mut theirs), true));
            mode = 0;
        } else {
            match mode {
                0 => {
                    normal.push_str(line);
                    normal.push('\n');
                }
                1 => {
                    ours.push_str(line);
                    ours.push('\n');
                }
                _ => {
                    theirs.push_str(line);
                    theirs.push('\n');
                }
            }
        }
    }
    if !normal.is_empty() {
        segs.push((normal, String::new(), false));
    }
    (segs, conflicts)
}

/// Compose the final file text from segments + per-conflict resolutions.
///
/// Unresolved blocks fall back to our side; this is only reachable before
/// Apply, which is gated at zero remaining.
fn compose(segs: &[(String, String, bool)], res: &[Option<u8>]) -> String {
    compose_impl(segs, res, false)
}

/// Compose the READ-ONLY display text: unresolved blocks render a visible
/// placeholder so the user sees what is still missing.
fn compose_display(segs: &[(String, String, bool)], res: &[Option<u8>]) -> String {
    compose_impl(segs, res, true)
}

fn compose_impl(segs: &[(String, String, bool)], res: &[Option<u8>], placeholder: bool) -> String {
    let mut out = String::new();
    let mut ci = 0usize;
    for (a, b, is_conf) in segs {
        if *is_conf {
            ci += 1;
            match res.get(ci - 1).copied().flatten() {
                Some(1) => out.push_str(b),
                Some(2) => {
                    out.push_str(a);
                    out.push_str(b);
                }
                Some(_) => out.push_str(a),
                None => {
                    if placeholder {
                        out.push_str("<< unresolved >>\n");
                    } else {
                        out.push_str(a);
                    }
                }
            }
        } else {
            out.push_str(a);
        }
    }
    out
}

/// Per-channel linear blend of `accent` over [`Palette::BG`] at opacity `t`.
/// Mirrored by `tests/redesign_merge.rs` to pin the exact painted colors.
fn tint_over_bg(accent: Color32, t: f32) -> Color32 {
    let bg = Palette::BG;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        mix(bg.r(), accent.r()),
        mix(bg.g(), accent.g()),
        mix(bg.b(), accent.b()),
    )
}

/// Conflict "yours" section background (spec §8.7: STATE_INFO @ ~12% over BG).
fn yours_bg() -> Color32 {
    tint_over_bg(Palette::STATE_INFO, 0.12)
}

/// Conflict "theirs" section background (spec §8.7: STATE_ERROR @ ~12% over BG).
fn theirs_bg() -> Color32 {
    tint_over_bg(Palette::STATE_ERROR, 0.12)
}

/// Conflict marker-strip background (spec §8.7: STATE_WARNING @ ~15% over BG).
fn marker_bg() -> Color32 {
    tint_over_bg(Palette::STATE_WARNING, 0.15)
}

/// Record one block resolution and refresh the composed read-only result.
fn resolve(state: &mut AppState, seg_idx: usize, choice: u8) {
    state.ui.conflict_res[seg_idx] = Some(choice);
    state.ui.conflict_text = compose_display(&state.ui.conflict_segs, &state.ui.conflict_res);
}

/// "N conflicts remaining" footer text (singular-aware).
fn remaining_text(n: usize) -> String {
    match n {
        1 => "1 conflict remaining".to_string(),
        n => format!("{n} conflicts remaining"),
    }
}

/// Pane header band (~28px SURFACE); `focused` adds the 2px BRAND outline
/// that marks the Result pane as the focused surface (spec §8.7).
fn pane_header(ui: &mut Ui, title: &str, focused: bool) {
    let resp = egui::Frame::new()
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

/// Raw-marker strip row: warning-tinted band carrying the conflict glyph.
fn marker_strip(ui: &mut Ui, glyph: &str) {
    egui::Frame::new()
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

/// Tinted side section (yours/theirs) with its 3px colored left border strip.
fn side_section(ui: &mut Ui, text: &str, fill: Color32, strip: Color32) {
    let resp = egui::Frame::new()
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

/// READ-ONLY composed-result cell: chosen text once resolved, a visible
/// placeholder while unresolved; every cell carries the BRAND focus outline.
fn result_cell(ui: &mut Ui, chosen: Option<u8>, ours: &str, theirs: &str) {
    let (text, fill) = match chosen {
        Some(1) => (theirs.to_string(), Palette::SURFACE),
        Some(2) => (format!("{ours}{theirs}"), Palette::SURFACE),
        Some(_) => (ours.to_string(), Palette::SURFACE),
        None => ("<< unresolved >>".to_string(), marker_bg()),
    };
    let resp = egui::Frame::new()
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

/// Render the conflict section inside the Commit tab (only when conflicts exist).
pub fn render(ui: &mut Ui, state: &mut AppState) {
    let id = match &state.selected_root {
        Some(id) => id.clone(),
        None => return,
    };
    let conflicted = state
        .multi
        .by_id(&id)
        .map(|r| r.status.conflicted.clone())
        .unwrap_or_default();
    if conflicted.is_empty() {
        return;
    }

    ui.separator();
    // The canonical "Merge conflicts" group in the changelist tree owns the
    // listing; this section only hosts the resolution tools.
    ui.heading("Conflict resolution");
    let root = state.selected_path();
    for path in &conflicted {
        ui.horizontal(|ui| {
            ui.label(path.display().to_string());
            if ui.button("Ours").clicked() {
                let r = root.clone();
                let p = path.clone();
                state.run_git("Accept ours".into(), move |v| {
                    if let Some(r) = &r {
                        conflict::accept_ours(v, r, &p)
                    } else {
                        Ok(())
                    }
                });
            }
            if ui.button("Theirs").clicked() {
                let r = root.clone();
                let p = path.clone();
                state.run_git("Accept theirs".into(), move |v| {
                    if let Some(r) = &r {
                        conflict::accept_theirs(v, r, &p)
                    } else {
                        Ok(())
                    }
                });
            }
            if ui.button("Merge…").clicked() {
                // Open the structured 3-way editor (Epic E6 / issue #15).
                if let Some(r) = &root {
                    let full = r.join(path);
                    if let Ok(content) = std::fs::read_to_string(&full) {
                        let (segs, _n) = parse_conflicts(&content);
                        let res: Vec<Option<u8>> =
                            segs.iter().filter(|(_, _, c)| *c).map(|_| None).collect();
                        state.ui.conflict_segs = segs;
                        state.ui.conflict_res = res.clone();
                        state.ui.conflict_text = compose_display(&state.ui.conflict_segs, &res);
                        state.ui.conflict_open = Some(path.clone());
                    } else {
                        state.ui.toast = Some(Toast::error("Could not read conflicted file."));
                    }
                }
            }
        });
    }
    if ui.button("Resolve all simple").clicked() {
        let r = root.clone();
        let st = state.multi.by_id(&id).cloned();
        if let (Some(r), Some(root_snap)) = (r, st) {
            let _ = conflict::resolve_all_simple(state.executor.as_ref(), &r, &root_snap.status);
            state.rescan();
        }
    }

    // Structured 3-way merge editor window (issue #15 redesign): three equal
    // panes Local | Result | Incoming over discrete conflict blocks.
    if let Some(path) = state.ui.conflict_open.clone() {
        let ctx = ui.ctx().clone();
        let path_disp = path.display().to_string();
        let mut open = true;
        egui::Window::new(format!("Merge: {path_disp}"))
            .open(&mut open)
            .default_width(980.0)
            .show(&ctx, |ui| {
                let segs = state.ui.conflict_segs.clone();
                let remaining = state.ui.conflict_res.iter().filter(|r| r.is_none()).count();

                // Pane headers: three EQUAL panes; Result is outlined as focused.
                ui.columns(3, |cols| {
                    pane_header(&mut cols[0], "Local (Yours)", false);
                    pane_header(&mut cols[1], "Result", true);
                    pane_header(&mut cols[2], "Incoming (Theirs)", false);
                });

                ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    // Conflict resolutions are indexed by conflict ORDINAL,
                    // not by segment position (normal segments interleave).
                    let mut ci = 0usize;
                    for (ours, theirs, is_conf) in segs.iter() {
                        if !*is_conf {
                            let text = ours.as_str();
                            ui.columns(3, |cols| {
                                for col in cols.iter_mut() {
                                    col.label(
                                        RichText::new(text).monospace().color(Palette::INK_2),
                                    );
                                }
                            });
                        } else {
                            let res_i = ci;
                            ci += 1;
                            let block = res_i + 1;
                            // Marker strips frame the discrete conflict block.
                            ui.columns(3, |cols| {
                                marker_strip(&mut cols[0], "<<<<<<<");
                                marker_strip(&mut cols[1], "=======");
                                marker_strip(&mut cols[2], ">>>>>>>");
                            });
                            // Tinted yours/theirs sections + read-only result.
                            let chosen = state.ui.conflict_res.get(res_i).copied().flatten();
                            ui.columns(3, |cols| {
                                side_section(&mut cols[0], ours, yours_bg(), Palette::STATE_INFO);
                                result_cell(&mut cols[1], chosen, ours, theirs);
                                side_section(
                                    &mut cols[2],
                                    theirs,
                                    theirs_bg(),
                                    Palette::STATE_ERROR,
                                );
                            });
                            // Per-block resolutions drive the composed Result.
                            ui.horizontal(|ui| {
                                if ui.button(format!("Accept Yours {block}")).clicked() {
                                    resolve(state, res_i, 0);
                                }
                                if ui.button(format!("Accept Theirs {block}")).clicked() {
                                    resolve(state, res_i, 1);
                                }
                                if ui.button(format!("Ignore {block}")).clicked() {
                                    resolve(state, res_i, 2);
                                }
                            });
                            ui.add_space(4.0);
                        }
                    }
                });

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(remaining_text(remaining)).color(Palette::INK_2));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Apply enables ONLY at zero remaining and writes the
                        // composed result through the engine's resolution flow.
                        if ui
                            .add_enabled(remaining == 0, egui::Button::new("Apply"))
                            .clicked()
                        {
                            let r = state.selected_path();
                            let p = path.clone();
                            let content = compose(&segs, &state.ui.conflict_res);
                            state.run_git("Apply merge resolution".into(), move |v| {
                                if let Some(r) = &r {
                                    conflict::write_resolution(v, r, &p, &content)
                                } else {
                                    Ok(())
                                }
                            });
                            state.ui.conflict_open = None;
                        }
                        if ui.button("Cancel").clicked() {
                            state.ui.conflict_open = None;
                        }
                    });
                });
            });
        if !open {
            state.ui.conflict_open = None;
        }
    }
}
