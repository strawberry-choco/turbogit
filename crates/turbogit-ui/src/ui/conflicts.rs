//! Conflict resolution (G1–G9): list conflicted files, accept ours/theirs,
//! resolve-all-simple, and the redesigned 3-way merge editor (issue #15):
//! three EQUAL panes Local | Result | Incoming with discrete conflict blocks
//! (marker strips + tinted yours/theirs sections), per-block Accept buttons
//! driving a READ-ONLY composed Result, an "N conflicts remaining" counter,
//! and an Apply gated at zero remaining (spec §8.7; free-text editing is
//! explicitly deferred).

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

/// Per-conflict resolution choice, as `resolved_block_text` interprets it and
/// as both surfaces' read-only Result composition does. Spelled raw inside
/// this module's own match; the resolver, which carries no raw spelling of its
/// own, imports these names.
pub(crate) const CHOICE_OURS: u8 = 0;
pub(crate) const CHOICE_THEIRS: u8 = 1;
pub(crate) const CHOICE_BOTH_OURS_FIRST: u8 = 2; // existing "Ignore"
pub(crate) const CHOICE_BOTH_THEIRS_FIRST: u8 = 3; // "Take both · theirs first" (issue #22)

/// The text one resolved conflict block shows for `choice` — the single owner
/// of the mapping the composer and both read-only Result previews share.
///
/// The unresolved case is NOT decided here: the caller supplies its own
/// reading of a `None` (the inline editor's `<< unresolved >>` placeholder,
/// the resolver's empty cell).
///
/// `CHOICE_BOTH_THEIRS_FIRST` reads theirs before ours, deliberately distinct
/// from `CHOICE_BOTH_OURS_FIRST` (the existing "Ignore" choice) — issue #22.
pub(crate) fn resolved_block_text(ours: &str, theirs: &str, choice: u8) -> Option<String> {
    match choice {
        CHOICE_THEIRS => Some(theirs.to_owned()),
        CHOICE_BOTH_OURS_FIRST => Some(format!("{ours}{theirs}")),
        CHOICE_BOTH_THEIRS_FIRST => Some(format!("{theirs}{ours}")),
        _ => Some(ours.to_owned()),
    }
}

/// The opener prologue both conflict surfaces repeat: read the file, build its
/// segments, and seed `conflict_segs` / `conflict_res` together. Handing back
/// the fresh resolution vector is what lets each opener keep composing its own
/// way — display text with the inline placeholder here, the final buffer in
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

/// Open the structured 3-way merge editor for one conflicted file of
/// `root` — the changelist's "Merge…" button and the cascade monitor's
/// Resolve deep link (issue 10) both land here.
pub(crate) fn open_conflict_editor(state: &mut AppState, root: &Path, path: &Path) {
    let Some(res) = seed_conflict_state(state, root, path) else {
        return;
    };
    state.ui.conflict_text = compose_display(&state.ui.conflict_segs, &res);
    state.ui.conflict_open = Some(path.to_path_buf());
}

/// Compose the final file text from segments + per-conflict resolutions.
///
/// Unresolved blocks fall back to our side; this is only reachable before
/// Apply, which is gated at zero remaining.
///
/// That fallback is the one behavioural difference the two surfaces keep: the
/// resolver's Result buffer IS this text, so an unresolved block reads as ours
/// there, while the inline editor paints `compose_display`, which renders
/// `<< unresolved >>` instead. One composer, two readings, on purpose.
pub(crate) fn compose(segs: &[(String, String, bool)], res: &[Option<u8>]) -> String {
    compose_impl(segs, res, false)
}

/// Compose the READ-ONLY display text: unresolved blocks render a visible
/// placeholder so the user sees what is still missing.
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

/// Record one block resolution and refresh the composed read-only result.
fn resolve(state: &mut AppState, seg_idx: usize, choice: u8) {
    state.ui.conflict_res[seg_idx] = Some(choice);
    state.ui.conflict_text = compose_display(&state.ui.conflict_segs, &state.ui.conflict_res);
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
                            let result_text =
                                chosen.and_then(|choice| resolved_block_text(ours, theirs, choice));
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
                    // Through the one shared pluralisation helper,
                    // not this file's own conditional.
                    ui.label(
                        RichText::new(
                            components::counted(remaining, "conflict", "conflicts") + " remaining",
                        )
                        .color(Palette::INK_2),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Apply enables ONLY at zero remaining and writes the
                        // composed result through the engine's resolution flow.
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

    /// The rule's one owner, crossed directly at this module's interface: for
    /// each of the four choices the core returns the block's text, and that
    /// text is exactly what the composer writes into the composed buffer for
    /// the same choice.
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
