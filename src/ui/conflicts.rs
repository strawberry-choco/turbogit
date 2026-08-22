//! Conflict resolution (G1–G9): list conflicted files, accept ours/theirs,
//! resolve-all-simple, and a structured 3-way merge editor
//! (Local | Result | Theirs) with per-conflict resolution (Epic E6).

use crate::core::conflict;
use crate::state::AppState;
use egui::{Color32, ScrollArea, Ui};

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
fn compose(segs: &[(String, String, bool)], res: &[u8]) -> String {
    let mut out = String::new();
    let mut ci = 0usize;
    for (a, b, is_conf) in segs {
        if *is_conf {
            let r = res.get(ci).copied().unwrap_or(0);
            ci += 1;
            let chosen = match r {
                1 => b,
                2 => &format!("{a}{b}"),
                _ => a,
            };
            out.push_str(chosen);
        } else {
            out.push_str(a);
        }
    }
    out
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
    ui.heading(format!("Merge conflicts ({})", conflicted.len()));
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
                // Open the structured 3-way editor (Epic E6).
                if let Some(r) = &root {
                    let full = r.join(path);
                    if let Ok(content) = std::fs::read_to_string(&full) {
                        let (segs, _n) = parse_conflicts(&content);
                        let res: Vec<u8> = segs
                            .iter()
                            .filter(|(_, _, c)| *c)
                            .map(|_| 0u8)
                            .collect();
                        state.ui.conflict_segs = segs;
                        state.ui.conflict_res = res.clone();
                        state.ui.conflict_text = compose(&state.ui.conflict_segs, &res);
                        state.ui.conflict_open = Some(path.clone());
                    } else {
                        state.ui.toast = Some("Could not read conflicted file.".into());
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

    // Structured 3-way merge editor window.
    if let Some(path) = state.ui.conflict_open.clone() {
        let ctx = ui.ctx().clone();
        let path_disp = path.display().to_string();
        let mut open = true;
        egui::Window::new(format!("Merge: {path_disp}"))
            .open(&mut open)
            .default_width(760.0)
            .show(&ctx, |ui| {
                let conflicts: Vec<usize> = state
                    .ui
                    .conflict_segs
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, _, c))| *c)
                    .map(|(i, _)| i)
                    .collect();
                ui.label(format!("{} conflict(s). Pick a side per block.", conflicts.len()));
                ScrollArea::vertical().show(ui, |ui| {
                    for (ci, seg_idx) in conflicts.iter().enumerate() {
                        let (ours, theirs, _is) = &state.ui.conflict_segs[*seg_idx];
                        ui.separator();
                        ui.label(format!("Conflict {}", ci + 1));
                        ui.columns(2, |cols| {
                            cols[0].colored_label(Color32::from_rgb(230, 160, 90), "OURS");
                            cols[1].colored_label(Color32::from_rgb(120, 200, 120), "THEIRS");
                        });
                        ui.columns(2, |cols| {
                            cols[0].colored_label(Color32::from_gray(200), ours);
                            cols[1].colored_label(Color32::from_gray(200), theirs);
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Use ours").clicked() {
                                state.ui.conflict_res[*seg_idx] = 0;
                                state.ui.conflict_text =
                                    compose(&state.ui.conflict_segs, &state.ui.conflict_res);
                            }
                            if ui.button("Use theirs").clicked() {
                                state.ui.conflict_res[*seg_idx] = 1;
                                state.ui.conflict_text =
                                    compose(&state.ui.conflict_segs, &state.ui.conflict_res);
                            }
                            if ui.button("Take both").clicked() {
                                state.ui.conflict_res[*seg_idx] = 2;
                                state.ui.conflict_text =
                                    compose(&state.ui.conflict_segs, &state.ui.conflict_res);
                            }
                        });
                    }
                });
                ui.separator();
                ui.label("Result (editable):");
                ui.text_edit_multiline(&mut state.ui.conflict_text);
                ui.horizontal(|ui| {
                    if ui.button("Save resolution").clicked() {
                        let r = state.selected_path();
                        let p = path.clone();
                        let content = state.ui.conflict_text.clone();
                        state.run_git("Save resolution".into(), move |v| {
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
        if !open {
            state.ui.conflict_open = None;
        }
    }
}
