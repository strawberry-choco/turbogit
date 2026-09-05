//! Cherry-pick across repositories (issue 16, screen 05): a modal dialog
//! over several repos at once. Left: the source repo's commits as a
//! searchable checkbox list in application order. Right: a patch preview
//! rail for the focused commit with per-file navigation. Below: the targets
//! table with the per-repo APPLIES / RISK / OUTCOME prediction. A "Stop on
//! first conflict" policy toggle and an Execute button dispatch through
//! [`turbogit_app::state::AppState::run_cherry_across`]; the run reports
//! through the cherry monitor in [`super::bulk_monitor`].

use egui::{Align, Color32, FontFamily, FontId, Layout, ScrollArea, Ui};

use turbogit_app::state::AppState;
use turbogit_domain::model::Commit;
use turbogit_services::cherry_across::{PickForecast, TargetForecast, TargetRisk};

use crate::theme::Palette;
use crate::ui::widgets;

/// Render the dialog while `ui.dialog` is `Dialog::CherryPickAcross`.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Cherry-pick across repositories")
        .open(&mut open)
        .default_width(920.0)
        .show(&ctx, |ui| body(ui, state));
    if !open {
        state.ui.dialog = None;
    }
}

fn body(ui: &mut Ui, state: &mut AppState) {
    // Snapshot the frame's read-only inputs up front so the handlers can
    // take `&mut AppState` freely (the log window's deferred-action
    // pattern; clicks inside a frame land on the next one).
    let source = state.ui.dlg.cherry_source.clone();
    let candidates = state.ui.dlg.cherry_candidates.clone();
    let selected: Vec<String> = state.ui.dlg.cherry_commits.clone();
    let Some(source) = source else {
        ui.label("No source repository selected.");
        if ui.button("Cancel").clicked() {
            state.ui.dialog = None;
        }
        return;
    };
    let target_rows: Vec<(turbogit_domain::model::RootId, String, bool)> = state
        .multi
        .roots
        .iter()
        .filter(|r| r.id != source)
        .map(|r| {
            let checked = state.ui.dlg.cherry_targets.contains(&r.id);
            (r.id.clone(), r.id.name(), checked)
        })
        .collect();
    let forecast = state.ui.dlg.cherry_forecast.clone();

    ui.horizontal(|ui| {
        ui.strong(format!("SOURCE {}", source.name()));
        ui.label(
            egui::RichText::new(format!(
                "will apply in order · {} of {} selected",
                selected.len(),
                candidates.len()
            ))
            .font(FontId::new(11.0, FontFamily::Proportional))
            .color(Palette::INK_3),
        );
    });
    ui.separator();

    // Commits checklist (left) + patch preview rail (right). Sequential
    // `&mut state` hand-offs: the panes never borrow it across frames.
    ui.columns(2, |columns| {
        commits_pane(&mut columns[0], state, &candidates, &selected);
        preview_pane(&mut columns[1], state);
    });

    ui.add_space(4.0);
    widgets::group_title(ui, "TARGETS");
    targets_table(ui, state, &target_rows, &forecast);

    ui.add_space(8.0);
    ui.separator();

    // Policy + actions.
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut state.ui.dlg.cherry_stop_on_conflict,
            "Stop on first conflict",
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Cancel").clicked() {
                state.ui.dialog = None;
            }
            let enabled = !selected.is_empty() && !target_rows.is_empty();
            let label = if target_rows.len() == 1 {
                "Cherry-pick to 1 repo".to_string()
            } else {
                format!("Cherry-pick to {} repos", target_rows.len())
            };
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                state.run_cherry_across();
            }
        });
    });
}

// --------------------------------------------------------------- commits ---

fn commits_pane(ui: &mut Ui, state: &mut AppState, candidates: &[Commit], selected: &[String]) {
    widgets::group_title(ui, "COMMITS");
    widgets::search_input(
        ui,
        "Search commits by message, author, or hash",
        &mut state.ui.dlg.cherry_search,
    );
    let query = state.ui.dlg.cherry_search.to_lowercase();
    ScrollArea::vertical().max_height(190.0).show(ui, |ui| {
        for commit in candidates {
            let matches = query.is_empty()
                || commit.message.to_lowercase().contains(&query)
                || commit.author.name.to_lowercase().contains(&query)
                || commit.id.starts_with(&query);
            if !matches {
                continue;
            }
            let mut checked = selected.contains(&commit.id);
            if ui
                .checkbox(&mut checked, subject(&commit.message))
                .clicked()
            {
                state.cherry_toggle_commit(commit.id.clone());
                state.cherry_focus_commit(commit.id.clone());
            }
        }
    });
}

fn subject(message: &str) -> String {
    message.lines().next().unwrap_or_default().to_string()
}

// --------------------------------------------------------------- preview ---

/// One `diff --git` section of a patch: the new path, its hunk count, and
/// the verbatim section body.
struct PatchFile {
    path: String,
    hunks: usize,
    body: String,
}

fn split_patch(patch: &str) -> Vec<PatchFile> {
    let mut files = Vec::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest
                .split_once(" b/")
                .map(|(_, new)| new.to_string())
                .unwrap_or_else(|| rest.to_string());
            files.push(PatchFile {
                path,
                hunks: 0,
                body: format!("{line}\n"),
            });
        } else if let Some(file) = files.last_mut() {
            if line.starts_with("@@ ") {
                file.hunks += 1;
            }
            file.body.push_str(line);
            file.body.push('\n');
        }
    }
    files
}

fn preview_pane(ui: &mut Ui, state: &mut AppState) {
    widgets::group_title(ui, "PATCH PREVIEW");
    let Some(focus) = state.ui.dlg.cherry_focus.clone() else {
        ui.label("Select a commit to preview its patch.");
        return;
    };
    let Some(preview) = state.ui.dlg.cherry_preview.clone() else {
        ui.label("Loading patch…");
        return;
    };
    let Ok(patch) = preview else {
        ui.label(egui::RichText::new("Could not load the patch.").color(Palette::STATE_ERROR));
        return;
    };
    let files = split_patch(&patch);
    if files.is_empty() {
        ui.label("The commit touches no files.");
        return;
    }
    // File navigation: clamp the stored index, step with the buttons.
    let idx = state.ui.dlg.cherry_preview_file.min(files.len() - 1);
    ui.horizontal(|ui| {
        if ui.add_enabled(idx > 0, egui::Button::new("‹")).clicked() {
            state.ui.dlg.cherry_preview_file = idx - 1;
        }
        let file = &files[idx];
        ui.label(
            egui::RichText::new(format!(
                "file {} of {} · {} · {} hunk{}",
                idx + 1,
                files.len(),
                file.path,
                file.hunks,
                if file.hunks == 1 { "" } else { "s" }
            ))
            .font(FontId::new(11.0, FontFamily::Proportional)),
        );
        if ui
            .add_enabled(idx + 1 < files.len(), egui::Button::new("›"))
            .clicked()
        {
            state.ui.dlg.cherry_preview_file = idx + 1;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                egui::RichText::new(short(&focus))
                    .font(FontId::new(11.0, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        });
    });
    ScrollArea::vertical().max_height(170.0).show(ui, |ui| {
        for line in files[idx].body.lines() {
            let color = if let Some(_add) = line.strip_prefix('+') {
                if line.starts_with("+++") {
                    Palette::INK_3
                } else {
                    Palette::STATE_SUCCESS
                }
            } else if let Some(del) = line.strip_prefix('-') {
                if line.starts_with("---") || del.is_empty() {
                    Palette::INK_3
                } else {
                    Palette::STATE_ERROR
                }
            } else {
                Palette::INK_2
            };
            ui.label(
                egui::RichText::new(format!("{line} "))
                    .monospace()
                    .font(FontId::new(11.0, FontFamily::Monospace))
                    .color(color),
            );
        }
    });
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

// --------------------------------------------------------------- targets ---

fn targets_table(
    ui: &mut Ui,
    state: &mut AppState,
    rows: &[(turbogit_domain::model::RootId, String, bool)],
    forecast: &Option<Vec<TargetForecast>>,
) {
    const HEADER: FontId = FontId::new(11.0, FontFamily::Proportional);
    let header = |ui: &mut Ui, text: &str| {
        ui.label(egui::RichText::new(text).font(HEADER).color(Palette::INK_3));
    };
    egui::Grid::new("cherry-targets")
        .num_columns(5)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            header(ui, "REPO");
            header(ui, "APPLIES");
            header(ui, "RISK");
            header(ui, "OUTCOME");
            ui.end_row();

            for (id, name, checked) in rows {
                let mut checked = *checked;
                if ui.checkbox(&mut checked, "").clicked() {
                    state.cherry_toggle_target(id.clone());
                }
                ui.label(name.clone());
                let fc = forecast
                    .as_ref()
                    .and_then(|fc| fc.iter().find(|t| &t.root == id));
                match fc {
                    Some(t) => {
                        ui.label(applies_text(t));
                        ui.label(egui::RichText::new(risk_text(t)).color(risk_color(t)));
                        ui.label(
                            egui::RichText::new(outcome_text(t))
                                .font(FontId::new(11.0, FontFamily::Proportional))
                                .color(Palette::INK_2),
                        );
                    }
                    None => {
                        ui.label("—");
                        ui.label("—");
                        ui.label("—");
                    }
                }
                ui.end_row();
            }
        });
}

fn applies_text(t: &TargetForecast) -> String {
    format!("{}/{}", t.applies, t.total)
}

fn risk_text(t: &TargetForecast) -> &'static str {
    if t.blocked.is_some() {
        "—"
    } else {
        match t.risk {
            TargetRisk::Low => "Low",
            TargetRisk::Medium => "Medium",
            TargetRisk::High => "High",
        }
    }
}

fn risk_color(t: &TargetForecast) -> Color32 {
    match t.risk {
        TargetRisk::Low => Palette::STATE_SUCCESS,
        TargetRisk::Medium => Palette::STATE_WARNING,
        TargetRisk::High => Palette::STATE_ERROR,
    }
}

/// The prediction's display-ready outcome, following the spec's examples:
/// "clean apply", "1 hunk overlaps — will stop for manual resolve",
/// "skipped — already present in history", "skipped — working tree is dirty".
fn outcome_text(t: &TargetForecast) -> String {
    if let Some(blocked) = &t.blocked {
        return format!("skipped — {blocked}");
    }
    let present = t
        .picks
        .iter()
        .filter(|p| matches!(p, PickForecast::AlreadyPresent))
        .count();
    let drifted = t
        .picks
        .iter()
        .filter(|p| matches!(p, PickForecast::CleanApplyDrifted))
        .count();
    let clean = t
        .picks
        .iter()
        .filter(|p| matches!(p, PickForecast::CleanApply))
        .count();
    let conflicted: Vec<usize> = t
        .picks
        .iter()
        .filter_map(|p| match p {
            PickForecast::Conflict { failed_hunks } => Some(*failed_hunks),
            _ => None,
        })
        .collect();
    let mut parts: Vec<String> = Vec::new();
    if clean > 0 {
        parts.push("clean apply".to_string());
    }
    if drifted > 0 {
        parts.push(format!("{drifted} apply with drift"));
    }
    if let Some(&hunks) = conflicted.first() {
        if hunks == 1 {
            parts.push("1 hunk overlaps — will stop for manual resolve".to_string());
        } else {
            parts.push(format!(
                "{hunks} hunks overlap — will stop for manual resolve"
            ));
        }
    }
    if present > 0 {
        parts.push(format!("{present} skipped — already present in history"));
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(" · ")
    }
}
