//! Redesigned conflict resolver (issue #22, screen 07).
//!
//! Dedicated tool window with three EQUAL panes Local | Result | Incoming,
//! per-conflict actions (Take ours / Take theirs / Take both · theirs first),
//! per-conflict Undo, Prev/Next navigation, Auto-advance to next conflict,
//! "Apply to all N remaining" (which names both its outcomes — the negative
//! one included), a left list whose rows carry each file's
//! file-level clash kind (`both modified`, `deleted by them`, … — the seven
//! vocabulary kinds, a different axis from the per-hunk marker grammar), and
//! header actions Abort merge / Continue merge.
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
use crate::ui::components::{self, KitButton};
use crate::ui::kit::conflict_pane::{self, Side};
use egui::{Key, Modifiers, RichText, TextEdit, Ui};
use turbogit_app::operation::Operation;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, BulkApplyScope, ConflictBulkReport, Toast};
use turbogit_domain::model::{
    CONFLICT_MARKER_OURS, CONFLICT_MARKER_SEPARATOR, CONFLICT_MARKER_THEIRS, ConflictKind, RootId,
};
use turbogit_engine_api::OwnedOperation;
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
        && let Some(v) = state.merge_versions(&RootId(root.clone().into()), path)
    {
        let segs = turbogit_services::diff_engine::merge_segments(
            v.base.as_deref().unwrap_or_default(),
            v.ours.as_deref().unwrap_or_default(),
            v.theirs.as_deref().unwrap_or_default(),
        );
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
    let chosen = state.ui.conflict_res.get(res_i).copied().flatten();

    // A resolved block keeps only the side(s) its resolution kept and drops
    // the marker strips — the absent side is the resolution's visual marker,
    // not a word, so a resolved block reads differently at a glance.
    if chosen.is_none() {
        // Marker strips frame the discrete conflict block. The three literals come
        // from `turbogit_domain` — the crate that also parses them — so the painter
        // cannot name a different marker than the parser does.
        conflict_pane::equal_panes(ui, |cols| {
            conflict_pane::marker_strip(&mut cols[0], CONFLICT_MARKER_OURS);
            conflict_pane::marker_strip(&mut cols[1], CONFLICT_MARKER_SEPARATOR);
            conflict_pane::marker_strip(&mut cols[2], CONFLICT_MARKER_THEIRS);
        });
    }
    // Tinted yours/theirs + editable Result.
    conflict_pane::equal_panes(ui, |cols| {
        if chosen != Some(CHOICE_THEIRS) {
            conflict_pane::side_section(&mut cols[0], ours, Side::Local);
        }
        if active {
            result_text_edit(&mut cols[1], state);
        } else {
            // Choice interpretation stays resolver-owned; only the inactive
            // read-only path allocates text for the shared presentation kit.
            let result_text = match chosen {
                Some(CHOICE_THEIRS) => Some(theirs.to_owned()),
                Some(CHOICE_BOTH_OURS_FIRST) => Some(format!("{ours}{theirs}")),
                Some(CHOICE_BOTH_THEIRS_FIRST) => Some(format!("{theirs}{ours}")),
                Some(_) => Some(ours.to_owned()),
                None => None,
            };
            conflict_pane::result_cell(&mut cols[1], result_text);
        }
        if chosen != Some(CHOICE_OURS) {
            conflict_pane::side_section(&mut cols[2], theirs, Side::Incoming);
        }
    });
    if active {
        // Per-conflict action row (only for the active conflict).
        ui.horizontal(|ui| {
            if ui.button(format!("Take ours {block}")).clicked() {
                accept_side(state, res_i, CHOICE_OURS);
            }
            if ui.button(format!("Take theirs {block}")).clicked() {
                accept_side(state, res_i, CHOICE_THEIRS);
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
            let can_undo = chosen.is_some();
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
                accept_side(state, res_i, CHOICE_OURS);
            }
            if ui.button("Take theirs").clicked() {
                accept_side(state, res_i, CHOICE_THEIRS);
            }
            if ui.button("Take both").clicked() {
                resolve(state, res_i, CHOICE_BOTH_THEIRS_FIRST);
            }
        });
    }
    ui.add_space(4.0);
}

/// Whether the selected file already carries merged changes: any conflict
/// resolved, or the Result pane free-edited. Per FILE, not per conflict —
/// matching the reference's "This file already contains merged changes"
/// (the per-file vs per-conflict trigger is UNVERIFIED against the running
/// product and assumed per file).
fn file_has_merged_changes(state: &AppState) -> bool {
    state.ui.conflict_resolver_edited || state.ui.conflict_res.iter().any(|r| r.is_some())
}

/// One take-ours / take-theirs click (the resolver's Accept Yours / Accept
/// Theirs). On a file that already carries merged changes this confirms
/// first — the pending take is only applied by the confirm's dangerous
/// affirmative (`Discard and Accept`); elsewhere it resolves directly.
fn accept_side(state: &mut AppState, seg_idx: usize, choice: u8) {
    if file_has_merged_changes(state) {
        state.ui.conflict_overwrite_pending = Some((seg_idx, choice));
    } else {
        resolve(state, seg_idx, choice);
    }
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
        state.ui.conflict_overwrite_pending = None;
        // A bulk verdict belongs to the file it was clicked on; a new file
        // starts with nothing reported.
        state.ui.conflict_bulk_report = None;
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
    // Each conflicted path paired with its file-level clash kind:
    // `None` when the engine does not classify, and the row then carries no
    // word rather than a guessed one.
    let conflicted: Vec<(PathBuf, Option<ConflictKind>)> = state
        .multi
        .by_id(&id)
        .map(|r| {
            r.status
                .conflicted
                .iter()
                .map(|p| (p.clone(), r.status.conflict_kind(p)))
                .collect()
        })
        .unwrap_or_default();
    if conflicted.is_empty() && !state.ui.conflict_resolver_open {
        return;
    }
    // Refresh auto-merged + merge-in-progress on every render. Asked of the
    // engine, not of `.git/MERGE_HEAD` here: a second reader of git's markers
    // would be a second answer, and this one decides whether the window stays
    // open. Cheap enough to ask in the frame because it is a `stat` of the
    // repository's git directory and never a git process, and it is asked only
    // once this root has conflicts or the window is already open.
    let root = state.selected_path();
    if let Some(r) = &root {
        if matches!(state.holding_operation(r), Some(OwnedOperation::Merge)) {
            state.ui.merge_in_progress = true;
            let root_id = RootId(r.clone().into());
            let conflicted_paths: Vec<PathBuf> =
                conflicted.iter().map(|(p, _)| p.clone()).collect();
            if let Some(list) = state.auto_merged_files(&root_id, &conflicted_paths) {
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
    render_overwrite_confirm(ui, state);
}

/// The `Overwrite Changes` confirm. The affirmative is the
/// dangerous sentence (`Discard and Accept`) and wears the primary kit fill;
/// the safe option (`Keep Merged Changes`) does nothing. Naming carries the
/// safety — the button order is UNVERIFIED against the reference and kept in
/// dangerous-then-safe reading order.
fn render_overwrite_confirm(ui: &mut Ui, state: &mut AppState) {
    let Some((seg, choice)) = state.ui.conflict_overwrite_pending else {
        return;
    };
    let side = if choice == CHOICE_THEIRS {
        "theirs"
    } else {
        "yours"
    };
    let mut open = true;
    egui::Window::new("Overwrite Changes")
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .resizable(false)
        .show(ui.ctx(), |ui| {
            ui.label("This file already has merged changes.");
            ui.label(format!(
                "Accepting the {side} side will discard and overwrite them."
            ));
            ui.horizontal(|ui| {
                if components::kit_button(ui, KitButton::Primary, "Discard and Accept").clicked() {
                    state.ui.conflict_overwrite_pending = None;
                    resolve(state, seg, choice);
                }
                if overwrite_safe_button(ui, "Keep Merged Changes").clicked() {
                    state.ui.conflict_overwrite_pending = None;
                }
            });
        });
    if !open {
        state.ui.conflict_overwrite_pending = None;
    }
}

/// The confirm's safe option. Not a kit variant — the four kit fills have no
/// `SURFACE_2` idle — so this is an egui button filled with the raised-on-card
/// token, which aliases `SURFACE_2`: the two confirm options stay visibly
/// different rects without minting a fifth kit kind.
fn overwrite_safe_button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(label)).fill(Palette::RAISED_ON_CARD))
}
/// The status word a file-list row carries for its file-level clash kind.
/// TurboGit's own strings for the seven vocabulary
/// kinds the reference names — deliberately NOT the editor's per-hunk marker
/// grammar (`Modified line` / `Conflicting lines`), which is a different axis.
fn kind_word(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::BothDeleted => "both deleted",
        ConflictKind::BothAdded => "both added",
        ConflictKind::BothModified => "both modified",
        ConflictKind::DeletedByUs => "deleted by you",
        ConflictKind::DeletedByThem => "deleted by them",
        ConflictKind::AddedByUs => "added by you",
        ConflictKind::AddedByThem => "added by them",
    }
}

fn render_file_list(
    ui: &mut Ui,
    state: &mut AppState,
    conflicted: &[(PathBuf, Option<ConflictKind>)],
) {
    ui.label(
        RichText::new("CONFLICTED FILES")
            .size(crate::theme::TYPE_CONTROL)
            .color(Palette::INK_2),
    );
    ui.label(RichText::new(format!("{} / {} resolved", 0, conflicted.len())).color(Palette::INK_2));
    for (path, kind) in conflicted {
        let label = path.display().to_string();
        let is_selected = state.ui.conflict_resolver_selected.as_ref() == Some(path);
        // The kind word rides the row so a merge can be triaged by reading the
        // list. A row with no classified kind (an engine that does
        // not report kinds) shows the path alone rather than a guessed word.
        let row = match kind {
            Some(kind) => format!("{label} — {}", kind_word(*kind)),
            None => label,
        };
        let resp = ui.selectable_label(is_selected, row);
        if resp.clicked()
            && let Some(root) = state.selected_path()
        {
            let path = path.clone();
            open_resolver(state, &root, &path);
        }
    }

    ui.add_space(8.0);
    let n_auto = state.ui.auto_merged_files.len();
    ui.label(
        RichText::new("AUTO-MERGED")
            .size(crate::theme::TYPE_CONTROL)
            .color(Palette::INK_2),
    );
    ui.label(RichText::new(components::counted(n_auto, "file")).color(Palette::INK_2));
    for path in &state.ui.auto_merged_files {
        ui.label(path.display().to_string());
    }
}

/// The bulk control: `Apply to all N remaining` with its scope option — the
/// deliberate fold of the reference's `Apply Non-Conflicting Changes` /
/// `Resolve All Simple Conflicts` pair into one control, the side scope being
/// an option on it rather than two more buttons. It
/// sits in the file column under the lists because the footer row is the
/// window's tightest fit: an added footer row lays out past the window's
/// clip rect, where it still exists but can be neither seen nor clicked.
///
/// The button stays visible at zero remaining: a click
/// with nothing left to resolve REPORTS the negative result under the
/// control instead of staying silent, which a control that vanished could
/// never do. The scope option — meaningless with nothing remaining — is
/// what hides.
fn render_bulk_control(ui: &mut Ui, state: &mut AppState) {
    let n_remaining = state.ui.conflict_res.iter().filter(|r| r.is_none()).count();
    ui.add_space(8.0);
    ui.separator();
    if n_remaining > 0 {
        ui.label(RichText::new("Fill remaining with").color(Palette::INK_2));
        ui.radio_value(
            &mut state.ui.conflict_resolver_scope,
            BulkApplyScope::ActiveChoice,
            "Active choice",
        );
        ui.radio_value(
            &mut state.ui.conflict_resolver_scope,
            BulkApplyScope::Ours,
            "Ours",
        );
        ui.radio_value(
            &mut state.ui.conflict_resolver_scope,
            BulkApplyScope::Theirs,
            "Theirs",
        );
    }
    if ui
        .button(format!("Apply to all {n_remaining} remaining"))
        .clicked()
    {
        apply_to_all_remaining(state);
    }
    // The bulk apply's outcome, named: the negative result is
    // reported, not left silent — the outcome that regresses silently.
    match state.ui.conflict_bulk_report {
        Some(ConflictBulkReport::AllResolved) => {
            ui.label(
                RichText::new("All conflicts were resolved automatically").color(Palette::INK_2),
            );
        }
        Some(ConflictBulkReport::NoneResolved) => {
            ui.label(
                RichText::new("No conflicts were resolved automatically").color(Palette::INK_2),
            );
        }
        None => {}
    }
}

fn render_resolver_body(
    ui: &mut Ui,
    state: &mut AppState,
    conflicted: &[(PathBuf, Option<ConflictKind>)],
) {
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
    } else {
        format!("{} remaining", components::counted(unresolved, "conflict"))
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

    // Left file list (auto-sized) + right panes. The column layout is
    // spelled out: `allocate_ui` alone inherits the caller's layout, so
    // inside this horizontal row the file list would lay out left-to-right
    // and overflow its 240pt strip, starving the panes of width.
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(240.0, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                render_file_list(ui, state, conflicted);
                render_bulk_control(ui, state);
            },
        );
        ui.vertical(|ui| {
            render_panes(ui, state);
        });
    });

    ui.separator();
    // Footer
    let total = state.ui.conflict_res.len();
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The finish control is WITHHELD while anything is unresolved —
            // no fill paints for its slot at all, not a tinted one. A
            // disabled control in this design is withheld, not greyed: Quiet's
            // disabled fill is TRANSPARENT, so one kit kind at two states
            // carries both looks without minting a fifth button flavour.
            ui.scope(|ui| {
                if unresolved > 0 {
                    ui.disable();
                }
                let resp = components::kit_button(
                    ui,
                    if unresolved == 0 {
                        KitButton::Primary
                    } else {
                        KitButton::Quiet
                    },
                    "Apply",
                );
                if unresolved == 0 && resp.clicked() {
                    apply_resolution(state);
                }
            });
            ui.checkbox(
                &mut state.ui.conflict_resolver_auto_advance,
                "Auto-advance to next conflict",
            );
        });
    });
}

fn render_panes(ui: &mut Ui, state: &mut AppState) {
    let segs = state.ui.conflict_segs.clone();
    // Pane headers.
    conflict_pane::equal_panes(ui, |cols| {
        conflict_pane::pane_header(&mut cols[0], "Local (Yours)", false);
        conflict_pane::pane_header(&mut cols[1], "Result", true);
        conflict_pane::pane_header(&mut cols[2], "Incoming (Theirs)", false);
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
            conflict_pane::equal_panes(ui, |cols| {
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

/// "Apply to all N remaining": fills every still-unresolved conflict with
/// the scoped choice — the active conflict's last-applied choice (ours as a
/// fallback), or the side the scope option pins.
fn apply_to_all_remaining(state: &mut AppState) {
    let active = state.ui.conflict_resolver_active_idx;
    let choice = match state.ui.conflict_resolver_scope {
        BulkApplyScope::Ours => CHOICE_OURS,
        BulkApplyScope::Theirs => CHOICE_THEIRS,
        BulkApplyScope::ActiveChoice => state
            .ui
            .conflict_resolver_undo
            .get(active)
            .copied()
            .flatten()
            .or_else(|| state.ui.conflict_res.get(active).copied().flatten())
            .unwrap_or(CHOICE_OURS),
    };
    let mut filled = 0usize;
    for i in 0..state.ui.conflict_res.len() {
        if state.ui.conflict_res[i].is_none() {
            state.ui.conflict_resolver_undo[i] = state.ui.conflict_res[i];
            state.ui.conflict_res[i] = Some(choice);
            filled += 1;
        }
    }
    if !state.ui.conflict_resolver_edited {
        state.ui.conflict_text = compose(&state.ui.conflict_segs, &state.ui.conflict_res);
    }
    // Both outcomes are named — including "nothing to do", which
    // the reference reports rather than leaves silent. One file's bulk fill
    // resolves all-or-nothing, so no partial outcome exists here to report.
    state.ui.conflict_bulk_report = Some(if filled == 0 {
        ConflictBulkReport::NoneResolved
    } else {
        ConflictBulkReport::AllResolved
    });
}

fn apply_resolution(state: &mut AppState) {
    let content = state.ui.conflict_text.clone();
    let r = state.selected_root.clone();
    let p = state.ui.conflict_resolver_selected.clone();
    if let (Some(r), Some(p)) = (r, p) {
        state.dispatch(Operation::custom(
            "Apply merge resolution",
            Affected::Root(r.clone()),
            move |v| conflict::write_resolution(v, r.as_path(), &p, &content),
        ));
    }
    // Keep the resolver open while a merge is still pending Continue —
    // the file is staged but the merge commit hasn't been written yet.
    if !state.ui.merge_in_progress {
        state.ui.conflict_resolver_open = false;
    }
}

/// The held state the resolver's two header verbs resolve.
///
/// Read from git rather than hard-wired to `merge`: the resolver opens on a
/// merge handoff today, but a paused rebase leaves the same unmerged paths, and
/// `git merge --abort` in a repository that is rebasing is a second failure on
/// top of the one being recovered. Falls back to `Merge` when nothing is held
/// anymore, which is what the header says and what the operation then reports.
fn held_operation(state: &turbogit_app::state::AppState) -> OwnedOperation {
    state
        .selected_root
        .as_ref()
        .and_then(|root| state.holding_operation(root.as_path()))
        .unwrap_or(OwnedOperation::Merge)
}

fn abort_merge(state: &mut AppState) {
    let owner = held_operation(state);
    if let Some(r) = state.selected_root.clone() {
        // The named in-flight resolution, shared with the status
        // bar's item and the palette entry: one verb, one operation, so the
        // three routes cannot drift into three different dispatch shapes.
        state.dispatch(Operation::Resolve {
            root: r,
            owner,
            verb: turbogit_app::operation::LifecycleVerb::Abort,
        });
    }
    state.ui.conflict_resolver_open = false;
}

fn continue_merge(state: &mut AppState) {
    let owner = held_operation(state);
    if let Some(r) = state.selected_root.clone() {
        state.dispatch(Operation::Resolve {
            root: r,
            owner,
            verb: turbogit_app::operation::LifecycleVerb::Continue,
        });
    }
    state.ui.conflict_resolver_open = false;
}
