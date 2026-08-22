//! Git Log tab (H1–H4) + History tab (H5–H8): commit graph list, details,
//! compare, cherry-pick, revert, interactive rebase entry, file/blame history.

use crate::core::history_service;
use crate::model::DateFormat;
use crate::state::{AppState, Dialog};
use chrono::{DateTime, Local, TimeZone, Utc};
use egui::{Color32, ScrollArea, Ui};

/// Distinct lane colors for the commit graph (Epic D1).
const GRAPH_COLORS: &[Color32] = &[
    Color32::from_rgb(80, 140, 230),
    Color32::from_rgb(220, 120, 140),
    Color32::from_rgb(120, 200, 130),
    Color32::from_rgb(200, 170, 90),
    Color32::from_rgb(170, 130, 220),
    Color32::from_rgb(90, 190, 200),
    Color32::from_rgb(230, 150, 90),
    Color32::from_rgb(150, 200, 220),
];

fn fmt_time(t: i64) -> String {
    match Utc.timestamp_opt(t, 0) {
        chrono::LocalResult::Single(dt) => {
            let local: DateTime<Local> = DateTime::from(dt);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        _ => String::new(),
    }
}

/// Render a timestamp according to the configured format (Epic D2).
fn fmt_date(t: i64, mode: DateFormat) -> String {
    match mode {
        DateFormat::Iso => fmt_time(t),
        DateFormat::Absolute => fmt_time(t),
        DateFormat::Relative => {
            let now = Local::now().timestamp();
            let d = now - t;
            if d < 60 {
                format!("{d}s ago")
            } else if d < 3600 {
                format!("{}m ago", d / 60)
            } else if d < 86400 {
                format!("{}h ago", d / 3600)
            } else if d < 2592000 {
                format!("{}d ago", d / 86400)
            } else {
                fmt_time(t)
            }
        }
    }
}

/// Assign each commit a lane color using a lightweight DAG walk so the list
/// reads like a commit graph (Epic D1). Newest-first input assumed.
fn assign_colors(commits: &[crate::model::Commit]) -> std::collections::HashMap<String, usize> {
    use std::collections::HashMap;
    let mut color_of: HashMap<String, usize> = HashMap::new();
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut next_color = 0usize;
    for c in commits {
        let idx = lanes.iter().position(|l| l.as_deref() == Some(&c.id)).unwrap_or_else(|| {
            if let Some(e) = lanes.iter().position(|l| l.is_none()) {
                e
            } else {
                lanes.push(None);
                lanes.len() - 1
            }
        });
        color_of.entry(c.id.clone()).or_insert_with(|| {
            // pick the lane's color, allocating a new one if needed
            let color = if idx < lanes.len() && lanes[idx].is_none() {
                let c = next_color;
                next_color += 1;
                c
            } else {
                idx
            };
            color
        });
        lanes[idx] = c.parents.first().cloned();
        for p in c.parents.iter().skip(1) {
            if let Some(e) = lanes.iter_mut().find(|l| l.is_none()) {
                e.replace(p.clone());
                color_of.entry(p.clone()).or_insert_with(|| {
                    let c = next_color;
                    next_color += 1;
                    c
                });
            } else {
                lanes.push(Some(p.clone()));
                color_of.entry(p.clone()).or_insert_with(|| {
                    let c = next_color;
                    next_color += 1;
                    c
                });
            }
        }
    }
    color_of
}

pub fn show_log(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut state.ui.log_filter);
        ui.label("filter (message / hash / author)");
    });

    let commits = state
        .selected_root
        .as_ref()
        .and_then(|id| state.log_cache.get(id).cloned())
        .unwrap_or_default();
    let filter = state.ui.log_filter.to_lowercase();
    let matched: Vec<_> = commits
        .iter()
        .filter(|c| {
            filter.is_empty()
                || c.message.to_lowercase().contains(&filter)
                || c.id.to_lowercase().contains(&filter)
                || c.author.name.to_lowercase().contains(&filter)
        })
        .collect();
    ui.label(format!("{} commits", matched.len()));

    let colors = assign_colors(&commits);
    let date_mode = state.settings.date_format;
    let id = state.selected_root.clone();

    ScrollArea::vertical().show(ui, |ui| {
        for c in &matched {
            let sel = state.ui.selected_commit.as_ref() == Some(&c.id);
            let color = colors.get(&c.id).map(|i| GRAPH_COLORS[i % GRAPH_COLORS.len()]).unwrap_or(Color32::GRAY);
            let first = c.message.lines().next().unwrap_or("").to_string();
            let short_hash = &c.id[..7.min(c.id.len())];
            ui.horizontal(|ui| {
                // Graph column: colored node (+ merge marker).
                ui.colored_label(color, if c.parents.len() > 1 { "◆" } else { "●" });
                let label = format!(
                    "{short_hash}  {author:<16}  {date}  {msg}",
                    author = c.author.name,
                    date = fmt_date(c.time, date_mode),
                    msg = if first.len() > 50 { &first[..50] } else { &first }
                );
                if ui.selectable_label(sel, label).clicked() {
                    state.ui.selected_commit = Some(c.id.clone());
                }
            });
        }
    });

    if let Some(cid) = state.ui.selected_commit.clone() {
        show_commit_details(ui, state, &cid);
    }
    let _ = id;
}

fn show_commit_details(ui: &mut Ui, state: &mut AppState, cid: &str) {
    let id = match &state.selected_root {
        Some(id) => id.clone(),
        None => return,
    };
    let commit = state
        .log_cache
        .get(&id)
        .and_then(|cs| cs.iter().find(|c| c.id == cid))
        .cloned();
    let root = state.multi.by_id(&id).cloned();
    if let (Some(c), Some(root)) = (commit, root) {
        ui.separator();
        ui.heading("Commit details");
        ui.label(format!("Author: {} <{}>", c.author.name, c.author.email));
        ui.label(format!("Date: {}", fmt_time(c.time)));
        ui.label(c.message.clone());
        ui.horizontal_wrapped(|ui| {
            if ui.button("Compare with parent").clicked() {
                state.ui.diff = Some(crate::state::DiffTarget {
                    root: id.clone(),
                    left: c.parents.first().cloned(),
                    right: Some(c.id.clone()),
                    path: None,
                });
            }
            if ui.button("Cherry-pick").clicked() {
                let rootp = id.0.clone();
                let cid2 = c.id.clone();
                state.run_git("Cherry-pick".into(), move |v| v.cherry_pick(&rootp, &cid2));
            }
            if ui.button("Revert").clicked() {
                let rootp = id.0.clone();
                let cid2 = c.id.clone();
                state.run_git("Revert".into(), move |v| v.revert(&rootp, &cid2));
            }
            if ui.button("Rebase from here…").clicked() {
                state.ui.dialog = Some(Dialog::InteractiveRebase);
            }
            if let Some(url) = history_service::open_on_web(&root, &c.id) {
                if ui.button("Open on web").clicked() {
                    state.ui.toast = Some(format!("URL: {url}"));
                }
            }
        });
        // Render the diff for this commit if it is the active diff target.
        if let Some(d) = state.ui.diff.clone() {
            if d.right.as_deref() == Some(cid) {
                crate::ui::diff::render_diff(ui, state, &d.left, &d.right, &d.path);
            }
        }
    }
}

pub fn show_history(ui: &mut Ui, state: &mut AppState) {
    ui.heading("File / Selection History");
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut state.ui.history_path);
        if ui.button("Show history").clicked() {
            if let Some(id) = &state.selected_root {
                let id = id.clone();
                let path = std::path::PathBuf::from(state.ui.history_path.trim());
                let executor = state.executor.clone();
                let tx = state.tx.clone();
                std::thread::spawn(move || {
                    let res = executor.log(
                        &id.0,
                        &crate::model::LogOpts {
                            path: Some(path),
                            ..Default::default()
                        },
                    );
                    let _ = tx.send(crate::engine::AppEvent::LogLoaded { root: id, commits: res });
                });
            }
        }
        if ui.button("Blame").clicked() {
            let root = state.selected_path();
            let path = std::path::PathBuf::from(state.ui.history_path.trim());
            if let Some(r) = root {
                match state.executor.blame(&r, &path, None) {
                    Ok(lines) => {
                        for l in lines.iter().take(200) {
                            ui.colored_label(
                                Color32::from_gray(150),
                                format!("{} {} {}", &l.commit[..7.min(l.commit.len())], l.author, l.content),
                            );
                        }
                    }
                    Err(e) => {
                        ui.colored_label(Color32::RED, e.to_string());
                    }
                }
            }
        }
    });
    // File history reuses the log cache for the selected root (path-scoped).
    let commits = state
        .selected_root
        .as_ref()
        .and_then(|id| state.log_cache.get(id).cloned())
        .unwrap_or_default();
    ui.separator();
    ui.label(format!("{} commits touching this path", commits.len()));
    ScrollArea::vertical().show(ui, |ui| {
        for c in &commits {
            let first = c.message.lines().next().unwrap_or("").to_string();
            if ui
                .selectable_label(false, format!("{}  {}", &c.id[..7.min(c.id.len())], first))
                .clicked()
            {
                state.ui.diff = Some(crate::state::DiffTarget {
                    root: state.selected_root.clone().unwrap(),
                    left: c.parents.first().cloned(),
                    right: Some(c.id.clone()),
                    path: Some(std::path::PathBuf::from(state.ui.history_path.trim())),
                });
            }
        }
    });
    if let Some(d) = state.ui.diff.clone() {
        if d.path.is_some() {
            crate::ui::diff::render_diff(ui, state, &d.left, &d.right, &d.path);
        }
    }
}
