//! Cherry-pick: the target-branch dialog, the across-repositories picker
//! with its commit checklist, patch preview, target forecasts, and the
//! fleet run.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::cherry_across::{PickForecast, TargetForecast, TargetRisk};
use turbogit_app::state::Dialog;
use turbogit_domain::model::{BranchKind, Commit, Patch};

use crate::bridge::AppHandle;
use crate::screens::short;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, input};
use crate::ui::row::{RowProps, RowState, row};
use crate::ui::tree::{TreeEntryProps, TreeProps, tree};

pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || {
            show.read(|s| {
                matches!(
                    s.ui.dialog,
                    Some(Dialog::CherryPickTarget | Dialog::CherryPickAcross)
                )
            })
        },
        move || {
            let body = handle.clone();
            dynamic(move || {
                body.read(|_| ());
                match body.peek(|s| s.ui.dialog) {
                    Some(Dialog::CherryPickTarget) => target_dialog(body.clone()).into_element(),
                    Some(Dialog::CherryPickAcross) => across_dialog(body.clone()).into_element(),
                    _ => view().into_element(),
                }
            })
        },
    )
}

fn close(handle: &AppHandle) {
    handle.update(|s| s.ui.dialog = None);
}

fn target_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let selector = handle.peek(|s| s.ui.dlg.cherry_pick_commit.clone());
    let Some((root, commit)) = selector else {
        let cancel = handle.clone();
        return dialog(
            DialogProps::new("cherry-pick-target", "Cherry-pick to branch")
                .on_cancel(move || close(&cancel))
                .child(
                    text("No commit selected.")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                )
                .action("Cancel"),
        )
        .into_element();
    };
    let current = handle.peek(|s| s.multi.by_id(&root).and_then(|r| r.current_branch.clone()));
    let branches = handle
        .peek(|s| {
            s.multi.by_id(&root).map(|r| {
                r.branches
                    .iter()
                    .filter(|b| b.kind == BranchKind::Local)
                    .map(|b| b.name.clone())
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default();
    let mut body = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::cherry("target-body"))
        .child(
            text(format!("Apply {} onto which branch?", short(&commit)))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        );
    if branches.is_empty() {
        body = body.child(
            text("No local branches.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
    }
    for name in branches {
        let is_current = current.as_deref() == Some(name.as_str());
        let protected =
            handle.peek(|s| turbogit_app::sync_service::is_protected(&s.settings, &name));
        let (label, enabled) = if is_current {
            (format!("{name} (current)"), false)
        } else if protected {
            (format!("{name} (protected)"), false)
        } else {
            (name.clone(), true)
        };
        let press = handle.clone();
        let pick_root = root.clone();
        let pick_commit = commit.clone();
        let pick_name = name.clone();
        body = body.child(row(RowProps::new()
            .state(RowState::Default)
            .test_id(&test_ids::cherry(format!("target:{name}")))
            .on_press(move || {
                if !enabled {
                    return;
                }
                let root = pick_root.clone();
                let commit = pick_commit.clone();
                let name = pick_name.clone();
                press.update(move |s| {
                    s.ui.dlg.cherry_pick_commit = None;
                    s.cherry_pick_to(root.clone(), commit.clone(), name);
                    s.ui.dialog = None;
                });
            })
            .child(text(label).text_size(px(data::TYPE_CONTROL)).text_color(
                if enabled {
                    t.foreground
                } else {
                    t.muted_foreground
                },
            ))));
    }
    let cancel = handle.clone();
    let act = handle.clone();
    dialog(
        DialogProps::new("cherry-pick-target", "Cherry-pick to branch")
            .on_cancel(move || close(&cancel))
            .child(body)
            .action("Cancel")
            .on_action(move || close(&act)),
    )
    .into_element()
}

pub fn across_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let Some(source) = handle.peek(|s| s.ui.dlg.cherry_source.clone()) else {
        let cancel = handle.clone();
        return dialog(
            DialogProps::new("cherry-pick-across", "Cherry-pick across repositories")
                .on_cancel(move || close(&cancel))
                .child(
                    text("No source repository selected.")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                )
                .action("Cancel"),
        )
        .into_element();
    };
    let (candidates, selected, query, targets) = handle.peek(|s| {
        (
            s.ui.dlg.cherry_candidates.clone(),
            s.ui.dlg.cherry_commits.clone(),
            s.ui.dlg.cherry_search.clone(),
            s.ui.dlg.cherry_targets.clone(),
        )
    });
    let query = query.to_lowercase();
    let filtered: Vec<Commit> = candidates
        .iter()
        .filter(|commit| {
            query.is_empty()
                || commit.message.to_lowercase().contains(&query)
                || commit.author.name.to_lowercase().contains(&query)
                || commit.id.starts_with(&query)
        })
        .cloned()
        .collect();
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::cherry("across-body"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text(format!("SOURCE {}", source.name()))
                        .text_size(px(data::TYPE_CONTROL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground),
                )
                .child(
                    text(format!(
                        "will apply in order · {} of {} selected",
                        selected.len(),
                        candidates.len()
                    ))
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
                ),
        );
    body = body.child(
        view()
            .flex_col()
            .gap(px(data::SPACING_Y))
            .child(
                text("COMMITS")
                    .text_size(px(data::TYPE_SECTION))
                    .text_color(t.muted_foreground),
            )
            .child(input(
                InputProps::new()
                    .placeholder("Search commits by message, author, or hash")
                    .accessibility_label("Search commits by message, author, or hash")
                    .test_id(&test_ids::cherry("search"))
                    .on_change({
                        let search = handle.clone();
                        move |text: &str| {
                            let text = text.to_string();
                            search.update(move |s| s.ui.dlg.cherry_search = text);
                        }
                    }),
            ))
            .child(commit_tree(handle.clone(), &filtered, &selected)),
    );
    body = body.child(
        text("PATCH PREVIEW")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    body = body.child(preview_pane(handle.clone()));
    body = body.child(
        text("TARGETS")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    body = body.child(targets_table(handle.clone(), &source, &targets));
    let stop = handle.peek(|s| s.ui.dlg.cherry_stop_on_conflict);
    let stop_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(checkbox(
                CheckboxProps::new(move || {
                    if stop {
                        CheckboxState::Checked
                    } else {
                        CheckboxState::Unchecked
                    }
                })
                .accessibility_label("Stop on first conflict")
                .test_id(&test_ids::cherry("stop-on-conflict"))
                .on_change(move |_| {
                    stop_handle.update(|s| {
                        s.ui.dlg.cherry_stop_on_conflict = !s.ui.dlg.cherry_stop_on_conflict
                    });
                }),
            ))
            .child(
                text("Stop on first conflict")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            ),
    );
    let enabled = !selected.is_empty() && !targets.is_empty();
    let run = handle.clone();
    let cancel = handle.clone();
    let label = format!(
        "Cherry-pick to {}",
        if targets.len() == 1 {
            "1 repo".to_string()
        } else {
            format!("{} repos", targets.len())
        }
    );
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(button(
                ButtonProps::new(label)
                    .variant(ButtonVariant::Primary)
                    .disabled(!enabled)
                    .test_id(&test_ids::cherry("run"))
                    .on_press(move || run.update(|s| s.run_cherry_across())),
            ))
            .child(button(
                ButtonProps::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::cherry("cancel"))
                    .on_press(move || close(&cancel)),
            )),
    );
    let across_cancel = handle.clone();
    dialog(
        DialogProps::new("cherry-pick-across", "Cherry-pick across repositories")
            .width(920.0)
            .on_cancel(move || close(&across_cancel))
            .child(body),
    )
    .into_element()
}

fn commit_tree(handle: AppHandle, commits: &[Commit], selected: &[String]) -> impl IntoElement {
    let t = theme();
    let mut items = Vec::new();
    let mut rows: Vec<(SharedString, String)> = Vec::new();
    for (at, commit) in commits.iter().enumerate() {
        let mark = if selected.contains(&commit.id) {
            "☑"
        } else {
            "☐"
        };
        rows.push((commit.id.clone().into(), format!("commit:{at}")));
        items.push(TreeEntryProps::new(
            commit.id.clone(),
            format!("{mark}  {}", commit.subject()),
        ));
    }
    let paint_rows = rows.clone();
    let press_handle = handle.clone();
    tree(
        TreeProps::new(items)
            .accessibility_label("Commits")
            .test_id(&test_ids::cherry("commits"))
            .render_item(move |entry: &nsh_base::TreeEntry| {
                let row_id = paint_rows
                    .iter()
                    .find(|(id, _)| id == &entry.id)
                    .map(|(_, row_id)| row_id.clone())
                    .unwrap_or_default();
                let commit = entry.id.to_string();
                let press = press_handle.clone();
                view()
                    .test_id(&test_ids::cherry(row_id))
                    .on_press(move || {
                        let commit = commit.clone();
                        press.update(move |s| {
                            s.cherry_toggle_commit(commit.clone());
                            s.cherry_focus_commit(commit);
                        });
                    })
                    .child(
                        text(entry.label.clone())
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.foreground),
                    )
                    .into_element()
            }),
    )
}

fn preview_pane(handle: AppHandle) -> AnyElement {
    let t = theme();
    let focus = handle.peek(|s| s.ui.dlg.cherry_focus.clone());
    let preview = handle.peek(|s| s.ui.dlg.cherry_preview.clone());
    let Some(focus) = focus else {
        return view()
            .child(
                text("Select a commit to preview its patch.")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.muted_foreground),
            )
            .into_element();
    };
    let Some(preview) = preview else {
        return view()
            .child(
                text("Loading patch…")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.muted_foreground),
            )
            .into_element();
    };
    let Ok(patch) = preview else {
        return view()
            .child(
                text("Could not load the patch.")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(data::STATE_ERROR),
            )
            .into_element();
    };
    let mut pane = view()
        .flex_col()
        .max_h(px(170.0))
        .test_id(&test_ids::cherry("preview"))
        .child(
            text(short(&focus))
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    pane = pane.child(render_patch(&patch));
    pane.into_element()
}

fn render_patch(patch: &Patch) -> impl IntoElement {
    let t = theme();
    let mut pane = view().flex_col();
    if patch.files.is_empty() {
        pane = pane.child(
            text("The commit touches no files.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
    }
    for file in &patch.files {
        let path = if file.new_path.is_empty() {
            file.old_path.clone()
        } else {
            file.new_path.clone()
        };
        pane = pane
            .child(
                text(path)
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            )
            .child(
                text(file.section_header_line())
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            );
        for hunk in &file.hunks {
            pane = pane.child(
                text(hunk.header_line())
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            );
            for line in &hunk.lines {
                let (prefix, color) = match line.kind {
                    turbogit_domain::model::PatchLineKind::Added => ("+", data::STATE_SUCCESS),
                    turbogit_domain::model::PatchLineKind::Removed => ("-", data::STATE_ERROR),
                    turbogit_domain::model::PatchLineKind::Context => (" ", t.muted_foreground),
                };
                pane = pane.child(
                    text(format!("{prefix}{}", line.text))
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(color),
                );
            }
        }
    }
    pane
}

fn targets_table(
    handle: AppHandle,
    source: &turbogit_domain::model::RootId,
    targets: &[turbogit_domain::model::RootId],
) -> impl IntoElement {
    let t = theme();
    let forecast = handle.peek(|s| s.ui.dlg.cherry_forecast.clone());
    let roots = handle.peek(|s| {
        s.multi
            .roots
            .iter()
            .filter(|r| &r.id != source)
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()
    });
    let mut table = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::cherry("targets"))
        .child(
            view()
                .flex_row()
                .gap(px(data::CELL_GAP))
                .child(
                    text("REPO")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground)
                        .w(px(140.0)),
                )
                .child(
                    text("APPLIES")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground)
                        .w(px(80.0)),
                )
                .child(
                    text("RISK")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground)
                        .w(px(80.0)),
                )
                .child(
                    text("OUTCOME")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground)
                        .flex_1(),
                ),
        );
    for root in roots {
        let checked = targets.contains(&root);
        let name = root.name();
        let forecast_for = forecast
            .as_ref()
            .and_then(|f| f.iter().find(|row| row.root == root).cloned());
        let press = handle.clone();
        let toggle_root = root.clone();
        table = table.child(row(RowProps::new()
            .state(if checked {
                RowState::Selected
            } else {
                RowState::Default
            })
            .test_id(&test_ids::cherry(format!("target:{name}")))
            .on_press(move || {
                let root = toggle_root.clone();
                press.update(move |s| s.cherry_toggle_target(root.clone()));
            })
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(data::CELL_GAP))
                    .child(
                        text(if checked { "☑" } else { "☐" })
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.foreground),
                    )
                    .child(
                        text(name.clone())
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.foreground)
                            .w(px(120.0)),
                    )
                    .child(
                        text(match &forecast_for {
                            Some(t) => format!("{}/{}", t.applies, t.total),
                            None => "—".to_string(),
                        })
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.foreground)
                        .w(px(80.0)),
                    )
                    .child(
                        text(match &forecast_for {
                            Some(t) => risk_text(t).to_string(),
                            None => "—".to_string(),
                        })
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(match &forecast_for {
                            Some(t) => risk_color(t),
                            None => t.muted_foreground,
                        })
                        .w(px(80.0)),
                    )
                    .child(
                        text(match &forecast_for {
                            Some(t) => outcome_text(t),
                            None => "—".to_string(),
                        })
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.foreground)
                        .flex_1(),
                    ),
            )));
    }
    table
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

fn risk_color(t: &TargetForecast) -> AdaptiveColor {
    match t.risk {
        TargetRisk::Low => data::STATE_SUCCESS,
        TargetRisk::Medium => data::STATE_WARNING,
        TargetRisk::High => data::STATE_ERROR,
    }
}

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
        parts.push(format!(
            "{} — will stop for manual resolve",
            if hunks == 1 {
                "1 hunk overlaps".to_string()
            } else {
                format!("{hunks} hunks overlap")
            }
        ));
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

/// The route: the across picker over the first repository, or the target
/// picker for its head commit.
pub fn cherry_pick_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let across = handle.clone();
    let target = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("cherry_pick"))
        .child(
            text("Cherry-pick")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(
            view()
                .flex_row()
                .gap(px(data::SPACING_X))
                .child(button(
                    ButtonProps::new("Cherry-pick across roots…")
                        .variant(ButtonVariant::Primary)
                        .test_id(&test_ids::cherry("open-across"))
                        .on_press(move || open_across(across.clone())),
                ))
                .child(button(
                    ButtonProps::new("Pick the head commit…")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::cherry("open-target"))
                        .on_press(move || open_target(target.clone())),
                )),
        )
        .child(
            text("Choose the commits to copy and the repositories that receive them.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
}

fn open_across(handle: AppHandle) {
    let Some(source) = handle.peek(|s| s.multi.roots.first().map(|r| r.id.clone())) else {
        return;
    };
    handle.update(|s| {
        s.open_cherry_across(source);
        s.ui.dialog = Some(Dialog::CherryPickAcross);
    });
}

fn open_target(handle: AppHandle) {
    let found = handle.peek(|s| {
        s.multi
            .roots
            .first()
            .and_then(|r| s.recent_commits(&r.id, 1).and_then(|c| c.first().cloned()))
            .map(|commit| (s.multi.roots[0].id.clone(), commit.id))
    });
    let Some((root, commit)) = found else {
        return;
    };
    handle.update(move |s| {
        s.ui.dlg.cherry_pick_commit = Some((root, commit));
        s.ui.dialog = Some(Dialog::CherryPickTarget);
    });
}
