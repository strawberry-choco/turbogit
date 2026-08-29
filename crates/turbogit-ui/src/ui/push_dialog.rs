//! Push dialog (issue #20): an aggregated outgoing-commit tree over every
//! registered root, fed by [`turbogit_services::sync_service::outgoing_per_root`]
//! behind the executor seam (ADR-0001).
//!
//! Scope semantics follow ADR-0006: Push always executes the batch push across
//! ALL roots with upstreams; clicking a root node filters the changed-files
//! preview ONLY and never narrows what Push executes. "Push current branch
//! only" is the sole scope-narrowing option; it targets the selected root via
//! the explicit Remote/Branch fields, which stay editable above the options
//! section (ADR-0007) because protected-branch force-push blocking keys off
//! the branch name.
//!
//! Issue #21 adds the safety layer: a Preview button running a REAL
//! `git push --dry-run` through the engine seam with the report shown
//! VERBATIM in-dialog, and protected-branch force-push blocking keyed off the
//! exact Remote/Branch fields — a blocked push never reaches the engine
//! instead of being silently downgraded. Deferred: "Push tags".

use crate::theme::Palette;
use egui::{Color32, RichText, Ui};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, OutgoingRoot};
use turbogit_domain::error::TgError;
use turbogit_domain::model::{BranchKind, ChangeStatus, Commit, LogOpts, RootId, Signature};
use turbogit_services::sync_service;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Push")
        .open(&mut open)
        .default_width(520.0)
        .show(&ctx, |ui| {
            ensure_outgoing(state);
            ensure_target_defaults(state);
            outgoing_tree(ui, state);
            changed_files_preview(ui, state);

            // Explicit target fields above the options section (ADR-0007).
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Remote:");
                ui.text_edit_singleline(&mut state.ui.dlg.push_remote);
            });
            ui.horizontal(|ui| {
                ui.label("Branch:");
                ui.text_edit_singleline(&mut state.ui.dlg.push_branch);
            });

            // Options.
            ui.checkbox(
                &mut state.ui.dlg.force_push,
                "Force push (--force-with-lease)",
            );
            ui.checkbox(
                &mut state.ui.dlg.push_current_branch_only,
                "Push current branch only",
            );

            // Safety strips (issue #21): acknowledging force warns about
            // history rewrite; naming a protected branch in the exact Branch
            // field BLOCKS the push outright.
            let branch = state.ui.dlg.push_branch.clone();
            let force_blocked =
                state.ui.dlg.force_push && sync_service::is_protected(&state.settings, &branch);
            if force_blocked {
                ui.colored_label(
                    Color32::RED,
                    format!("⚠ '{branch}' is protected — force-push blocked."),
                );
                ui.colored_label(
                    Color32::RED,
                    "Uncheck force push or retarget the Branch field to continue.",
                );
            } else if state.ui.dlg.force_push {
                ui.colored_label(
                    Color32::YELLOW,
                    "⚠ Force push rewrites the remote branch (--force-with-lease).",
                );
            }

            // Verbatim dry-run report pane (issue #21).
            if let Some(preview) = state.ui.dlg.push_preview_output.as_ref() {
                ui.separator();
                match preview {
                    Ok(report) => {
                        ui.label("Dry-run report (verbatim):");
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .show(ui, |ui| {
                                ui.label(RichText::new(report).monospace().small());
                            });
                    }
                    Err(stderr) => {
                        ui.colored_label(Color32::RED, "Push rejected by git:");
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(stderr)
                                        .monospace()
                                        .small()
                                        .color(Color32::RED),
                                );
                            });
                    }
                }
            }

            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close(state);
                }
                if ui.button("Preview dry-run").clicked() {
                    run_preview(state);
                }
                // A blocked force-push never dispatches: the disabled button
                // keeps the block visible in-dialog instead of silently
                // downgrading to a regular (or refused) push.
                if ui
                    .add_enabled(!force_blocked, egui::Button::new("Push"))
                    .clicked()
                {
                    execute_push(state);
                    close(state);
                }
            });
        });
    if !open {
        close(state);
    }
}

fn close(state: &mut AppState) {
    state.ui.dialog = None;
    state.ui.dlg.push_outgoing = None;
    state.ui.dlg.push_preview_root = None;
    state.ui.dlg.push_current_branch_only = false;
    state.ui.dlg.push_preview_output = None;
}

/// Run a REAL `git push --dry-run` for the explicit Remote/Branch target on
/// the selected root (issue #21), honoring the force acknowledgment, and
/// store the report VERBATIM. Synchronous like the outgoing snapshot builder
/// (a local git subprocess); a rejected push surfaces through the same pane
/// with git's verbatim stderr instead of a toast.
fn run_preview(state: &mut AppState) {
    let output = match state.selected_path() {
        Some(root) => {
            let remote = state.ui.dlg.push_remote.clone();
            let branch = state.ui.dlg.push_branch.clone();
            let force = state.ui.dlg.force_push;
            match state.executor.push_dry_run(&root, &remote, &branch, force) {
                Ok(report) => Ok(report),
                Err(e) => Err(verbatim_stderr(&e)),
            }
        }
        None => Err("No repository selected.".to_string()),
    };
    state.ui.dlg.push_preview_output = Some(output);
}

/// Extract git's verbatim stderr from an engine error; anything that is not
/// a CLI error falls back to its Display text.
fn verbatim_stderr(e: &TgError) -> String {
    match e {
        TgError::Cli { stderr, .. } => stderr.clone(),
        other => other.to_string(),
    }
}

/// Build the outgoing-commit snapshot once per dialog open (synchronous like
/// the interactive-rebase plan builder precedent).
fn ensure_outgoing(state: &mut AppState) {
    if state.ui.dlg.push_outgoing.is_some() {
        return;
    }
    let exec = state.executor.clone();
    let results = sync_service::outgoing_per_root(exec.as_ref(), &state.multi);
    let mut out = Vec::with_capacity(results.len());
    for (id, res) in results {
        let name =
            id.0.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| id.0.display().to_string());
        let commits = match res {
            Ok(ids) => {
                let log = exec.log(&id.0, &LogOpts::default()).unwrap_or_default();
                Ok(ids
                    .into_iter()
                    .map(|cid| {
                        log.iter().find(|c| c.id == cid).cloned().unwrap_or(Commit {
                            id: cid.clone(),
                            parents: Vec::new(),
                            author: empty_signature(),
                            committer: empty_signature(),
                            message: String::new(),
                            time: 0,
                            root: id.clone(),
                        })
                    })
                    .collect::<Vec<_>>())
            }
            Err(e) => Err(e.to_string()),
        };
        out.push(OutgoingRoot { id, name, commits });
    }
    state.ui.dlg.push_outgoing = Some(out);
}

fn empty_signature() -> Signature {
    Signature {
        name: String::new(),
        email: String::new(),
        time: 0,
    }
}

/// Prefill Remote/Branch from the selected root's tracking config (kept from
/// the pre-redesign dialog; only fills while the remote field is empty so
/// user edits persist across redraws).
fn ensure_target_defaults(state: &mut AppState) {
    if !state.ui.dlg.push_remote.is_empty() {
        return;
    }
    if let Some(id) = state.selected_root.clone()
        && let Some(root) = state.multi.by_id(&id)
        && let Some(b) = root.branches.iter().find(|b| {
            b.kind == BranchKind::Local && root.current_branch.as_deref() == Some(&b.name)
        })
    {
        if let Some(t) = &b.tracking {
            let parts: Vec<&str> = t.splitn(2, '/').collect();
            state.ui.dlg.push_remote = parts[0].to_string();
            state.ui.dlg.push_branch = parts.get(1).copied().unwrap_or(b.name.as_str()).to_string();
        } else {
            state.ui.dlg.push_remote = root
                .remotes
                .first()
                .map(|r| r.name.clone())
                .unwrap_or_else(|| "origin".into());
            state.ui.dlg.push_branch = root.current_branch.clone().unwrap_or_default();
        }
    }
}

/// Project node → per-root nodes → commit rows. Root-node clicks set the
/// PREVIEW filter only (ADR-0006); they never affect push scope. The
/// snapshot is borrowed, not cloned (plan §1.5); node clicks are collected
/// during the scroll pass and applied after it (defer pattern).
fn outgoing_tree(ui: &mut Ui, state: &mut AppState) {
    let mut select_root: Option<Option<RootId>> = None;
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .show(ui, |ui| {
            let snapshot = state.ui.dlg.push_outgoing.as_deref().unwrap_or(&[]);
            if snapshot.is_empty() {
                ui.label("No repositories to push.");
                return;
            }
            let project = state
                .project_dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Project".to_string());
            let project_node = format!("{project} (all roots)");
            if ui
                .selectable_label(state.ui.dlg.push_preview_root.is_none(), &project_node)
                .clicked()
            {
                select_root = Some(None);
            }

            let total: usize = snapshot
                .iter()
                .map(|r| r.commits.as_ref().map_or(0, |c| c.len()))
                .sum();
            if total == 0 {
                ui.label("No outgoing commits.");
            }

            for entry in snapshot {
                let n = entry.commits.as_ref().map_or(0, |c| c.len());
                let node = format!("{} — {n} commits ahead", entry.name);
                let selected = state.ui.dlg.push_preview_root.as_ref() == Some(&entry.id);
                ui.indent(entry.id.0.as_os_str(), |ui| {
                    if ui.selectable_label(selected, &node).clicked() {
                        // Toggle: clicking the selected root returns to all roots.
                        select_root = Some(if selected {
                            None
                        } else {
                            Some(entry.id.clone())
                        });
                    }
                    if let Ok(commits) = &entry.commits {
                        ui.indent((entry.id.0.as_os_str(), "commits"), |ui| {
                            for c in commits {
                                commit_row(ui, c);
                            }
                        });
                    }
                });
            }
        });
    if let Some(next) = select_root {
        state.ui.dlg.push_preview_root = next;
    }
}

fn commit_row(ui: &mut Ui, c: &Commit) {
    let short = &c.id[..c.id.len().min(7)];
    let subject = c.message.lines().next().unwrap_or("");
    ui.horizontal(|ui| {
        ui.label(RichText::new(short).monospace().color(Palette::BRAND));
        ui.label(subject);
        let meta = format!("{} · {}", c.author.name, rel_time(c.time));
        ui.label(RichText::new(meta).small().weak());
    });
}

/// Compact relative age for commit rows (display only).
fn rel_time(epoch_secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - epoch_secs).max(0);
    if delta < 60 {
        "just now".into()
    } else if delta < 3600 {
        format!("{delta}m ago")
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3600)
    } else {
        format!("{}d ago", delta / 86_400)
    }
}

/// Changed files across outgoing commits, filtered by the clicked root node
/// ONLY (ADR-0006). Files are cached per (root, commit) behind
/// `caches.ensure_files`. Snapshot and filter are borrowed (plan §1.5); the
/// `dlg` borrow is disjoint from the `caches` fill, so no clone is needed.
fn changed_files_preview(ui: &mut Ui, state: &mut AppState) {
    egui::CollapsingHeader::new("Changed files")
        .default_open(true)
        .show(ui, |ui| {
            let snapshot = state.ui.dlg.push_outgoing.as_deref().unwrap_or(&[]);
            let filter = state.ui.dlg.push_preview_root.as_ref();
            let mut any = false;
            for entry in snapshot
                .iter()
                .filter(|e| filter.is_none_or(|f| f == &e.id))
            {
                let commits = match &entry.commits {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                for c in commits {
                    let files =
                        state
                            .caches
                            .ensure_files(state.executor.as_ref(), &entry.id, &c.id);
                    for ch in files {
                        any = true;
                        ui.label(format!(
                            "{} {}",
                            change_letter(ch.status),
                            ch.path.display()
                        ));
                    }
                }
            }
            if !any {
                ui.label(RichText::new("No changed files.").weak());
            }
        });
}

fn change_letter(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Modified => "M",
        ChangeStatus::Added => "A",
        ChangeStatus::Deleted => "D",
        _ => "?",
    }
}

/// Dispatch the push on a worker thread. Default = batch across ALL roots
/// (ADR-0006); "Push current branch only" narrows to the selected root using
/// the explicit Remote/Branch fields (ADR-0007). Batch failures are
/// aggregated naming each failing root so they surface per-root in the toast.
fn execute_push(state: &mut AppState) {
    let force = state.ui.dlg.force_push;
    let settings = state.settings.clone();
    if state.ui.dlg.push_current_branch_only {
        let root = state.selected_path();
        let remote = state.ui.dlg.push_remote.clone();
        let branch = state.ui.dlg.push_branch.clone();
        state.run_git(
            "Push".into(),
            Affected::from_optional_root(root.as_deref()),
            move |v| match root {
                Some(r) => sync_service::push(v, &r, &remote, &branch, force, &settings),
                None => Ok(()),
            },
        );
    } else {
        let mgr = state.multi.clone();
        // Batch push (ADR-0006) touches every root with an upstream.
        state.run_git("Push".into(), Affected::All, move |v| {
            let results = sync_service::push_all_forced(v, &mgr, &settings, force);
            let failures: Vec<String> = results
                .into_iter()
                .filter_map(|(id, r)| {
                    r.err().map(|e| {
                        let name =
                            id.0.file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_else(|| id.0.display().to_string());
                        format!("{name}: {e}")
                    })
                })
                .collect();
            if failures.is_empty() {
                Ok(())
            } else {
                Err(TgError::Other(format!(
                    "push failed — {}",
                    failures.join("; ")
                )))
            }
        });
    }
}
