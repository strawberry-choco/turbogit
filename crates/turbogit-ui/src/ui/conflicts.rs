use crate::theme::Palette;
use crate::ui::components;
use crate::ui::kit::conflict_pane::Side;
use egui::{RichText, ScrollArea, Ui};
use turbogit_app::conflict;
use turbogit_app::diff_engine;
use turbogit_app::shell_writes::conflict as writes;
use turbogit_app::state::{AppState, Toast};
use turbogit_domain::model::{
    CONFLICT_MARKER_OURS, CONFLICT_MARKER_SEPARATOR, CONFLICT_MARKER_THEIRS, RootId,
};

use std::path::Path;

fn parse_conflicts(content: &str) -> (Vec<(String, String, bool)>, usize) {
    conflict::parse_conflict_markers(content)
}

pub(crate) fn merge_editor_segments(
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

pub(crate) use turbogit_app::conflict::{
    CHOICE_BOTH_OURS_FIRST, CHOICE_BOTH_THEIRS_FIRST, CHOICE_OURS, CHOICE_THEIRS,
    resolved_block_text,
};

/// The opener prologue both conflict surfaces repeat: read the file, build its
/// segments, and seed `conflict_segs` / `conflict_res` together. Handing back
/// the fresh resolution vector is what lets each opener keep composing its own
/// way, display text with the inline placeholder here, the final buffer in
/// the resolver.
///
/// A file that cannot be read sets the toast and NO other field, so a caller
/// that early-returns on `None` leaves the state exactly as it was.
pub(crate) fn seed_conflict_state(
    state: &mut AppState,
    root: &Path,
    path: &Path,
) -> Option<Vec<Option<u8>>> {
    let full = root.join(path);
    let Ok(content) = std::fs::read_to_string(&full) else {
        state.ui.toast = Some(Toast::error("Could not read conflicted file."));
        return None;
    };
    let (segs, _n) = merge_editor_segments(state, &Some(root.to_path_buf()), path, &content);
    let res: Vec<Option<u8>> = segs.iter().filter(|(_, _, c)| *c).map(|_| None).collect();
    state.ui.conflict_segs = segs;
    state.ui.conflict_res = res.clone();
    Some(res)
}

pub(crate) fn open_conflict_editor(state: &mut AppState, root: &Path, path: &Path) {
    let Some(res) = seed_conflict_state(state, root, path) else {
        return;
    };
    state.ui.conflict_text = compose_display(&state.ui.conflict_segs, &res);
    state.ui.conflict_open = Some(path.to_path_buf());
}

pub(crate) fn compose(segs: &[(String, String, bool)], res: &[Option<u8>]) -> String {
    compose_impl(segs, res, false)
}

pub(crate) fn compose_display(segs: &[(String, String, bool)], res: &[Option<u8>]) -> String {
    compose_impl(segs, res, true)
}

fn compose_impl(segs: &[(String, String, bool)], res: &[Option<u8>], placeholder: bool) -> String {
    let mut out = String::new();
    let mut ci = 0usize;
    for (a, b, is_conf) in segs {
        if *is_conf {
            ci += 1;
            match res
                .get(ci - 1)
                .copied()
                .flatten()
                .and_then(|choice| resolved_block_text(a, b, choice))
            {
                Some(text) => out.push_str(&text),
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

fn resolve(state: &mut AppState, seg_idx: usize, choice: u8) {
    state.ui.conflict_res[seg_idx] = Some(choice);
    state.ui.conflict_text = compose_display(&state.ui.conflict_segs, &state.ui.conflict_res);
}

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
    ui.heading("Conflict resolution");
    let root = state.selected_root.clone();
    for path in &conflicted {
        ui.horizontal(|ui| {
            ui.label(path.display().to_string());
            if ui.button("Ours").clicked()
                && let Some(r) = &root
            {
                state.dispatch(writes::accept_ours(r, path));
            }
            if ui.button("Theirs").clicked()
                && let Some(r) = &root
            {
                state.dispatch(writes::accept_theirs(r, path));
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
    let st = state.multi.by_id(&id).map(|r| r.status.clone());
    if ui.button("Resolve all simple").clicked()
        && let (Some(r), Some(status)) = (root.clone(), st.clone())
    {
        state.dispatch(writes::resolve_simple(&r, &status));
    }
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

                ScrollArea::vertical()
                    .id_salt("conflict_segments")
                    .max_height(400.0)
                    .show(ui, |ui| {
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
                                let chosen = state.ui.conflict_res.get(res_i).copied().flatten();
                                let result_text = chosen
                                    .and_then(|choice| resolved_block_text(ours, theirs, choice));
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
                                ui.horizontal(|ui| {
                                    if ui.button(format!("Accept Yours {block}")).clicked() {
                                        resolve(state, res_i, CHOICE_OURS);
                                    }
                                    if ui.button(format!("Accept Theirs {block}")).clicked() {
                                        resolve(state, res_i, CHOICE_THEIRS);
                                    }
                                    if ui.button(format!("Ignore {block}")).clicked() {
                                        resolve(state, res_i, CHOICE_BOTH_OURS_FIRST);
                                    }
                                });
                                ui.add_space(4.0);
                            }
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(
                            components::counted(remaining, "conflict", "conflicts") + " remaining",
                        )
                        .color(Palette::INK_2),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(remaining == 0, egui::Button::new("Apply"))
                            .clicked()
                        {
                            let content = compose(&segs, &state.ui.conflict_res);
                            if let Some(r) = &state.selected_root {
                                state.dispatch(writes::apply_merge_resolution(
                                    r,
                                    path.as_path(),
                                    &content,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_block_text_matches_the_composer_for_every_choice() {
        let ours = "OURS\n";
        let theirs = "THEIRS\n";
        let segs = vec![(ours.to_string(), theirs.to_string(), true)];
        for (choice, want) in [
            (CHOICE_OURS, "OURS\n"),
            (CHOICE_THEIRS, "THEIRS\n"),
            (CHOICE_BOTH_OURS_FIRST, "OURS\nTHEIRS\n"),
            (CHOICE_BOTH_THEIRS_FIRST, "THEIRS\nOURS\n"),
        ] {
            assert_eq!(
                resolved_block_text(ours, theirs, choice).as_deref(),
                Some(want),
                "choice {choice}: the core must compose the block's text"
            );
            assert_eq!(
                compose(&segs, &[Some(choice)]),
                want,
                "choice {choice}: the composer must write the core's text"
            );
        }
    }
}
