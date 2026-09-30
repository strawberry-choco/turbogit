//! Conflict resolution (G1–G9): list conflicted files, accept ours/theirs,
//! resolve-all-simple, and the redesigned 3-way merge editor (issue #15):
//! three EQUAL panes Local | Result | Incoming with discrete conflict blocks
//! (marker strips + tinted yours/theirs sections), per-block Accept buttons
//! driving a READ-ONLY composed Result, an "N conflicts remaining" counter,
//! and an Apply gated at zero remaining (spec §8.7; free-text editing is
//! explicitly deferred).

use crate::theme::Palette;
use crate::ui::kit::conflict_pane::Side;
use egui::{RichText, ScrollArea, Ui};
use turbogit_app::operation::Operation;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Toast};
use turbogit_domain::model::{
    CONFLICT_MARKER_OURS, CONFLICT_MARKER_SEPARATOR, CONFLICT_MARKER_THEIRS, RootId,
};
use turbogit_services::conflict;
use turbogit_services::diff_engine;

use std::path::Path;

/// Parse a file's conflict markers into alternating normal / conflict blocks.
/// Returns (segments, conflict_count) where each segment is
/// `(ours, theirs, is_conflict)`; for normal segments `theirs` is empty.
///
/// Thin delegate onto the canonical parser in `core::conflict`, which the
/// Phase L1 parity tests also compare the structured merge against.
fn parse_conflicts(content: &str) -> (Vec<(String, String, bool)>, usize) {
    conflict::parse_conflict_markers(content)
}

/// Segments opening the merge editor (Phase L1): a structured 3-way merge of
/// the index's base/ours/theirs when in-process merges are enabled and all
/// three versions resolve through the engine seam; otherwise the raw-marker
/// parser over the working-tree file, verbatim. Both produce the same
/// `(ours, theirs, is_conflict)` tuple stream, so pane rendering, per-block
/// resolution, and Apply gating are unchanged either way.
fn merge_editor_segments(
    state: &AppState,
    root: &Option<std::path::PathBuf>,
    path: &std::path::Path,
    content: &str,
) -> (Vec<(String, String, bool)>, usize) {
    if state.settings.in_process_diffs
        && let Some(root) = root
        && let Some(v) = state.merge_versions(&RootId(root.clone().into()), path)
    {
        let segs = diff_engine::merge_segments(
            v.base.as_deref().unwrap_or_default(),
            v.ours.as_deref().unwrap_or_default(),
            v.theirs.as_deref().unwrap_or_default(),
        );
        let n = segs.iter().filter(|(_, _, c)| *c).count();
        return (segs, n);
    }
    parse_conflicts(content)
}

/// Open the structured 3-way merge editor for one conflicted file of
/// `root` — the changelist's "Merge…" button and the cascade monitor's
/// Resolve deep link (issue 10) both land here.
pub(crate) fn open_conflict_editor(state: &mut AppState, root: &Path, path: &Path) {
    let full = root.join(path);
    if let Ok(content) = std::fs::read_to_string(&full) {
        let (segs, _n) = merge_editor_segments(state, &Some(root.to_path_buf()), path, &content);
        let res: Vec<Option<u8>> = segs.iter().filter(|(_, _, c)| *c).map(|_| None).collect();
        state.ui.conflict_segs = segs;
        state.ui.conflict_res = res.clone();
        state.ui.conflict_text = compose_display(&state.ui.conflict_segs, &res);
        state.ui.conflict_open = Some(path.to_path_buf());
    } else {
        state.ui.toast = Some(Toast::error("Could not read conflicted file."));
    }
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
                Some(3) => {
                    // "Take both · theirs first" (issue #22): theirs before
                    // ours. Distinct from `Some(2)` which is ours-first
                    // (the existing "Ignore" choice).
                    out.push_str(b);
                    out.push_str(a);
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
    let root = state.selected_root.clone();
    for path in &conflicted {
        ui.horizontal(|ui| {
            ui.label(path.display().to_string());
            if ui.button("Ours").clicked() {
                let r = root.clone();
                let p = path.clone();
                if let Some(r) = r {
                    state.dispatch(Operation::custom(
                        "Accept ours",
                        Affected::Root(r.clone()),
                        move |v| conflict::accept_ours(v, r.as_path(), &p),
                    ));
                }
            }
            if ui.button("Theirs").clicked() {
                let r = root.clone();
                let p = path.clone();
                if let Some(r) = r {
                    state.dispatch(Operation::custom(
                        "Accept theirs",
                        Affected::Root(r.clone()),
                        move |v| conflict::accept_theirs(v, r.as_path(), &p),
                    ));
                }
            }
            if ui.button("Merge…").clicked()
                && let Some(r) = &root
            {
                crate::ui::conflicts::open_conflict_editor(state, r.as_path(), path);
            }
            if ui.button("Resolve…").clicked() {
                // Open the redesigned Resolve Conflicts tool window (issue
                // #22, screen 07): three EQUAL panes with editable Result,
                // per-conflict undo / take-both / prev-next, auto-merged
                // file list, and Abort/Continue merge header actions.
                if let Some(r) = &root {
                    crate::ui::conflict_resolver::open_resolver(state, r.as_path(), path);
                }
            }
        });
    }
    let r = root.clone();
    let st = state.multi.by_id(&id).map(|r| r.status.clone());
    if ui.button("Resolve all simple").clicked()
        && let (Some(r), Some(status)) = (r.clone(), st.clone())
    {
        state.dispatch(Operation::custom(
            "Resolve simple conflicts",
            Affected::Root(r.clone()),
            move |v| {
                // Every per-file outcome counts: a resolution that failed has
                // to surface, not be discarded (issue #04's honest feed).
                let results = conflict::resolve_all_simple(v, r.as_path(), &status);
                match results.into_iter().find_map(|(_, res)| res.err()) {
                    Some(e) => Err(e),
                    None => Ok(()),
                }
            },
        ));
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
                crate::ui::kit::conflict_pane::equal_panes(ui, |cols| {
                    crate::ui::kit::conflict_pane::pane_header(
                        &mut cols[0],
                        "Local (Yours)",
                        false,
                    );
                    crate::ui::kit::conflict_pane::pane_header(&mut cols[1], "Result", true);
                    crate::ui::kit::conflict_pane::pane_header(
                        &mut cols[2],
                        "Incoming (Theirs)",
                        false,
                    );
                });

                ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    // Conflict resolutions are indexed by conflict ORDINAL,
                    // not by segment position (normal segments interleave).
                    let mut ci = 0usize;
                    for (ours, theirs, is_conf) in segs.iter() {
                        if !*is_conf {
                            let text = ours.as_str();
                            crate::ui::kit::conflict_pane::equal_panes(ui, |cols| {
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
                            // The three literals come from `turbogit_domain` —
                            // the crate that also parses them — so this painter
                            // cannot name a different marker than the parser.
                            crate::ui::kit::conflict_pane::equal_panes(ui, |cols| {
                                crate::ui::kit::conflict_pane::marker_strip(
                                    &mut cols[0],
                                    CONFLICT_MARKER_OURS,
                                );
                                crate::ui::kit::conflict_pane::marker_strip(
                                    &mut cols[1],
                                    CONFLICT_MARKER_SEPARATOR,
                                );
                                crate::ui::kit::conflict_pane::marker_strip(
                                    &mut cols[2],
                                    CONFLICT_MARKER_THEIRS,
                                );
                            });
                            // Tinted yours/theirs sections + read-only result.
                            let chosen = state.ui.conflict_res.get(res_i).copied().flatten();
                            // Keep the legacy ordinal mapping (including Some(3)) at
                            // the surface; the kit only receives composed visual text.
                            let result_text = match chosen {
                                Some(1) => Some(theirs.to_owned()),
                                Some(2) => Some(format!("{ours}{theirs}")),
                                Some(3) => Some(format!("{theirs}{ours}")),
                                Some(_) => Some(ours.to_owned()),
                                None => None,
                            };
                            crate::ui::kit::conflict_pane::equal_panes(ui, |cols| {
                                crate::ui::kit::conflict_pane::side_section(
                                    &mut cols[0],
                                    ours,
                                    Side::Local,
                                );
                                crate::ui::kit::conflict_pane::result_cell(
                                    &mut cols[1],
                                    result_text,
                                );
                                crate::ui::kit::conflict_pane::side_section(
                                    &mut cols[2],
                                    theirs,
                                    Side::Incoming,
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
                            let r = state.selected_root.clone();
                            let p = path.clone();
                            let content = compose(&segs, &state.ui.conflict_res);
                            if let Some(r) = r {
                                state.dispatch(Operation::custom(
                                    "Apply merge resolution",
                                    Affected::Root(r.clone()),
                                    move |v| {
                                        conflict::write_resolution(v, r.as_path(), &p, &content)
                                    },
                                ));
                            }
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
