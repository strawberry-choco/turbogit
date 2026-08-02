//! Diff viewer (Epic E).
//!
//! Improvements over the original:
//! - **Async + cached**: the diff is computed on a worker thread and cached,
//!   so it no longer re-runs `git diff` synchronously every frame (the old code
//!   froze the UI on large diffs — see `E7`/`J1`).
//! - **Side-by-side** and **unified** layouts (toggle).
//! - **Word-level** intra-line highlighting for changed lines.
//! - **Hunk navigation** (prev / next / jump).

use crate::engine::AppEvent;
use crate::model::DiffOpts;
use crate::state::AppState;
use egui::{Align, Color32, Response, ScrollArea, Ui};
use std::sync::Arc;

/// One parsed line of a unified diff.
enum Row {
    Meta(String),
    Hunk(usize, String), // (hunk index, raw text)
    Context(String),
    Del(String),
    Add(String),
}

/// Build a cache key that uniquely identifies this diff request.
fn diff_key(
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    path: &Option<std::path::PathBuf>,
) -> String {
    format!("{:?}|{:?}|{:?}|{:?}", root, left, right, path)
}

/// Parse unified-diff text into renderable rows, tagging hunk headers with an
/// incrementing index so the UI can navigate between them.
fn parse(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut hunk = 0usize;
    for line in text.lines() {
        if line.starts_with("@@") {
            rows.push(Row::Hunk(hunk, line.to_string()));
            hunk += 1;
        } else if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("---")
            || line.starts_with("+++")
            || line.starts_with("new file")
            || line.starts_with("Binary")
        {
            rows.push(Row::Meta(line.to_string()));
        } else if line.starts_with('+') {
            rows.push(Row::Add(line[1..].to_string()));
        } else if line.starts_with('-') {
            rows.push(Row::Del(line[1..].to_string()));
        } else {
            rows.push(Row::Context(line.to_string()));
        }
    }
    rows
}

/// Longest common (prefix, suffix) lengths between two strings, non-overlapping.
fn common_prefix_suffix(a: &str, b: &str) -> (usize, usize) {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let mut pre = 0;
    while pre < ab.len().min(bb.len()) && ab[pre] == bb[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < (ab.len() - pre).min(bb.len() - pre)
        && ab[ab.len() - 1 - suf] == bb[bb.len() - 1 - suf]
    {
        suf += 1;
    }
    (pre, suf)
}

/// Render a single changed line, highlighting the changed substring.
fn render_changed(ui: &mut Ui, text: &str, base: Color32, changed: Color32, other: &str) {
    let (pre, suf) = common_prefix_suffix(text, other);
    let mid = text.len().saturating_sub(pre + suf);
    if mid == 0 || other.is_empty() {
        ui.colored_label(base, text);
        return;
    }
    let unchanged = format!("{}{}", &text[..pre], &text[text.len() - suf..]);
    let diff = &text[pre..text.len() - suf];
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(base, &unchanged);
        ui.colored_label(changed, diff);
    });
}

/// Trigger an async diff load if the cache is missing/stale and not already loading.
fn ensure_diff(
    ui: &mut Ui,
    state: &mut AppState,
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    path: &Option<std::path::PathBuf>,
) {
    let key = diff_key(root, left, right, path);
    let stale = state.ui.diff_cache.as_ref().map(|(k, _)| k != &key).unwrap_or(true);
    if stale && !state.ui.diff_loading {
        state.ui.diff_loading = true;
        state.ui.diff_error = None;
        let executor: Arc<dyn crate::engine::GitExecutor> = state.vcs.executor.clone();
        let tx = state.tx.clone();
        let root = root.to_path_buf();
        let mut opts = DiffOpts::default();
        opts.left = left.clone();
        opts.right = right.clone();
        opts.path = path.clone();
        std::thread::spawn(move || {
            let res = executor.diff(&root, &opts);
            let _ = tx.send(AppEvent::DiffReady { key, result: res });
        });
    }
    let _ = ui;
}

pub fn render_diff(
    ui: &mut Ui,
    state: &mut AppState,
    left: &Option<String>,
    right: &Option<String>,
    path: &Option<std::path::PathBuf>,
) {
    let root = match state.selected_path() {
        Some(r) => r,
        None => {
            ui.label("No repository selected.");
            return;
        }
    };
    ensure_diff(ui, state, &root, left, right, path);

    let key = diff_key(&root, left, right, path);

    // Layout toggle + hunk navigation controls.
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.ui.diff_side_by_side, "Side-by-side");
        ui.separator();
        if ui.button("⬆ Prev").clicked() && state.ui.diff_current_hunk > 0 {
            state.ui.diff_current_hunk -= 1;
        }
        if ui.button("Next ⬇").clicked() {
            state.ui.diff_current_hunk += 1;
        }
    });

    if let Some(err) = &state.ui.diff_error {
        ui.colored_label(Color32::RED, err);
        return;
    }
    let cached = state.ui.diff_cache.clone();
    let text = match cached {
        Some((k, t)) if k == key => t,
        _ => {
            if state.ui.diff_loading {
                ui.spinner();
                ui.label(" Computing diff…");
            } else {
                ui.label("(no diff)");
            }
            return;
        }
    };

    if text.trim().is_empty() {
        ui.label("(no differences)");
        return;
    }

    let rows = parse(&text);
    let total_hunks = rows.iter().filter(|r| matches!(r, Row::Hunk(_, _))).count();
    if total_hunks > 0 {
        ui.label(format!(
            "Hunk {}/{}",
            state.ui.diff_current_hunk.min(total_hunks.saturating_sub(0)) + 1,
            total_hunks
        ));
    }

    ScrollArea::vertical().show(ui, |ui| {
        if state.ui.diff_side_by_side {
            render_side_by_side(ui, state, &rows);
        } else {
            render_unified(ui, state, &rows);
        }
    });
}

fn render_unified(ui: &mut Ui, state: &mut AppState, rows: &[Row]) {
    let base = ui.style().visuals.text_color();
    for row in rows {
        let mut resp: Option<Response> = None;
        match row {
            Row::Meta(s) => {
                ui.colored_label(Color32::from_gray(140), s);
            }
            Row::Hunk(idx, s) => {
                ui.colored_label(Color32::from_rgb(200, 170, 90), s);
                resp = Some(ui.response());
                if *idx == state.ui.diff_current_hunk {
                    if let Some(r) = resp.take() {
                        r.scroll_to_me(Some(Align::Center));
                    }
                }
            }
            Row::Context(s) => {
                ui.colored_label(base, s);
            }
            Row::Del(s) => {
                ui.colored_label(Color32::from_rgb(230, 120, 110), s);
            }
            Row::Add(s) => {
                ui.colored_label(Color32::from_rgb(120, 200, 120), s);
            }
        }
        let _ = resp;
    }
}

fn render_side_by_side(ui: &mut Ui, state: &mut AppState, rows: &[Row]) {
    // Pair consecutive Del/Add lines; render context/hunk/meta as full-width.
    let mut i = 0;
    while i < rows.len() {
        match &rows[i] {
            Row::Meta(s) => {
                ui.colored_label(Color32::from_gray(140), s);
                i += 1;
            }
            Row::Hunk(idx, s) => {
                ui.colored_label(Color32::from_rgb(200, 170, 90), s);
                if *idx == state.ui.diff_current_hunk {
                    ui.response().scroll_to_me(Some(Align::Center));
                }
                i += 1;
            }
            Row::Context(s) => {
                let fg = ui.style().visuals.text_color();
                ui.columns(2, |cols| {
                    cols[0].colored_label(fg, &format!(" {s}"));
                    cols[1].colored_label(fg, &format!(" {s}"));
                });
                i += 1;
            }
            Row::Del(_d) | Row::Add(_d) => {
                // Gather a run of Del/Add to pair up.
                let mut dels = Vec::new();
                let mut adds = Vec::new();
                while i < rows.len() {
                    match &rows[i] {
                        Row::Del(s) => {
                            dels.push(s.clone());
                            i += 1;
                        }
                        Row::Add(s) => {
                            adds.push(s.clone());
                            i += 1;
                        }
                        _ => break,
                    }
                }
                let pairs = dels.len().max(adds.len());
                for p in 0..pairs {
                    let d = dels.get(p);
                    let a = adds.get(p);
                    ui.columns(2, |cols| {
                        match d {
                            Some(d) => {
                                let other = a.map(|s| s.as_str()).unwrap_or("");
                                render_changed(
                                    &mut cols[0],
                                    d,
                                    Color32::from_rgb(230, 120, 110),
                                    Color32::from_rgb(255, 160, 150),
                                    other,
                                );
                            }
                            None => {
                                cols[0].colored_label(Color32::from_gray(90), " ");
                            }
                        }
                        match a {
                            Some(a) => {
                                let other = d.map(|s| s.as_str()).unwrap_or("");
                                render_changed(
                                    &mut cols[1],
                                    a,
                                    Color32::from_rgb(120, 200, 120),
                                    Color32::from_rgb(160, 255, 160),
                                    other,
                                );
                            }
                            None => {
                                cols[1].colored_label(Color32::from_gray(90), " ");
                            }
                        }
                    });
                }
            }
        }
    }
}
