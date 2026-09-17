//! Push dialog (issue #20): an aggregated outgoing-commit tree over every
//! registered root, fed by [`turbogit_services::sync_service::outgoing_per_root`]
//! behind the executor seam (ADR-0001).
//!
//! Issue #21 adds the safety layer: a Preview button running a REAL
//! `git push --dry-run` through the engine seam with the report shown
//! VERBATIM in-dialog, and protected-branch force-push blocking keyed off the
//! exact Remote/Branch fields — a blocked push never reaches the engine
//! instead of being silently downgraded.
//!
//! Issue #24 layers per-commit selection on top of the aggregated list.
//!
//! Issue #25 adds the PUSH SCOPE segmented control (This repo / Selected N /
//! Project subtree / All M). The control replaces the old "Push current branch
//! only" checkbox — `ThisRepo` keeps the explicit Remote/Branch target fields
//! for the per-repo path, the other variants push every root the scope covers
//! through [`turbogit_services::sync_service::push_roots`]. The dry-run
//! preview runs once per root in scope and aggregates into a single
//! "N commits → R remotes · X refs · Y rejected" line above the verbatim
//! per-root reports. Protected branches in scope render as a remediation
//! banner naming the protected repos and matching pattern, with a one-click
//! "Remove N protected repos from scope" action that excludes them so the
//! push can proceed for the rest.

use crate::theme::Palette;
use egui::{RichText, Ui};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, OutgoingRoot, PushPreview};
use turbogit_domain::error::TgError;
use turbogit_domain::model::{
    BranchKind, ChangeStatus, Commit, LogOpts, Root, RootId, Signature, SignatureState,
};
use turbogit_services::sync_service::{self, PushScope, SubsetPushState};

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Push")
        .open(&mut open)
        .default_width(560.0)
        .show(&ctx, |ui| {
            ensure_outgoing(state);
            ensure_target_defaults(state);

            // Resolved scope: roots covered by the segmented control minus any
            // remediation exclusions (issue #25).
            let scope = scope_roots(state);
            let scope_commits = scope_outgoing(state, &scope);
            let total = outgoing_total(&scope_commits);

            // PUSH SCOPE control sits at the top (screen 10).
            push_scope_control(ui, state);
            ui.separator();

            // Protected-branch remediation banner — names repos + pattern
            // and offers a one-click exclusion (issue #25). The banner is
            // skipped when the scope covers zero protected roots.
            let _protected_count = remediation_banner(ui, state, &scope);

            // The aggregated outgoing tree; root-node clicks filter the
            // changed-files PREVIEW only (ADR-0006), never the batch push.
            outgoing_tree(ui, state);
            changed_files_preview(ui, state);

            // Subset-push banner (issue #24) — explains why the unchecked
            // older commit forces a full push.
            let outgoing_shas: Vec<String> = state
                .ui
                .dlg
                .push_outgoing
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .flat_map(|r| match &r.commits {
                    Ok(cs) => cs.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
                    Err(_) => Vec::new(),
                })
                .collect();
            let subset = sync_service::subset_push_state(
                &outgoing_shas,
                &state.ui.dlg.push_selected_commits,
            );
            state.ui.dlg.push_subset = subset.clone();
            if matches!(state.ui.dlg.push_subset, SubsetPushState::OlderUnchecked) {
                ui.colored_label(
                    Palette::STATE_WARNING,
                    "All commits will be pushed — an unchecked older ancestor forces a full push.",
                );
            }

            // Explicit target fields above the options section (ADR-0007).
            // They only drive the `ThisRepo` scope path (issue #25).
            let scope_is_this_repo = state.ui.dlg.push_scope == PushScope::ThisRepo;
            if scope_is_this_repo {
                ui.horizontal(|ui| {
                    ui.label("Remote:");
                    ui.text_edit_singleline(&mut state.ui.dlg.push_remote);
                });
                ui.horizontal(|ui| {
                    ui.label("Branch:");
                    ui.text_edit_singleline(&mut state.ui.dlg.push_branch);
                });
            }

            ui.checkbox(
                &mut state.ui.dlg.force_push,
                "Force push (--force-with-lease)",
            );
            ui.checkbox(&mut state.ui.dlg.push_tags, "Push tags");
            ui.checkbox(&mut state.ui.dlg.push_no_verify, "Skip pre-push hooks");
            ui.checkbox(
                &mut state.ui.dlg.push_set_upstream,
                "Set upstream (--set-upstream)",
            );
            if state.ui.dlg.push_set_upstream {
                ui.colored_label(Palette::STATE_INFO, "upstream will be set");
            }

            // Safety strip (issue #21): acknowledging force warns about
            // history rewrite; naming a protected branch in the exact Branch
            // field BLOCKS the ThisRepo path outright.
            if scope_is_this_repo {
                let branch = state.ui.dlg.push_branch.clone();
                let force_blocked =
                    state.ui.dlg.force_push && sync_service::is_protected(&state.settings, &branch);
                if force_blocked {
                    ui.colored_label(
                        Palette::STATE_ERROR,
                        format!("⚠ '{branch}' is protected — force-push blocked."),
                    );
                    ui.colored_label(
                        Palette::STATE_ERROR,
                        "Uncheck force push or retarget the Branch field to continue.",
                    );
                } else if state.ui.dlg.force_push {
                    ui.colored_label(
                        Palette::STATE_WARNING,
                        "⚠ Force push rewrites the remote branch (--force-with-lease).",
                    );
                }
            }

            // Aggregated dry-run preview (issue #25): the summary line plus
            // one verbatim report per root in scope.
            if let Some(preview) = state.ui.dlg.push_preview_output.as_ref() {
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "{} commits → {} remotes · {} refs · {} rejected",
                        preview.commits, preview.remotes, preview.refs, preview.rejected
                    ))
                    .small()
                    .strong(),
                );

                preview_reports(ui, preview);
            }

            // Footer: Cancel / Preview / Push. The Push button names the
            // resolved scope (issue #25).
            let force_blocked_this_repo = scope_is_this_repo
                && state.ui.dlg.force_push
                && sync_service::is_protected(&state.settings, &state.ui.dlg.push_branch);
            let label = if scope_is_this_repo {
                "Push".to_string()
            } else {
                action_label(&scope, total)
            };
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close(state);
                }
                if ui.button("Preview dry-run").clicked() {
                    run_preview(state);
                }
                if ui
                    .add_enabled(!force_blocked_this_repo, egui::Button::new(&label))
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
    state.ui.dlg.push_preview_output = None;
    state.ui.dlg.push_scope_excluded.clear();
}

/// Resolved roots in the dialog's current PUSH SCOPE, minus the user's
/// remediation exclusions (issue #25).
fn scope_roots(state: &AppState) -> Vec<Root> {
    state.push_scope_roots()
}

/// Action button label (issue #25): "Push N commits to M repos", or bare
/// "Push" when the scope is empty.
fn action_label(scope: &[Root], total_commits: usize) -> String {
    let n = scope.len();
    if n == 0 {
        return "Push".to_string();
    }
    let noun = if n == 1 { "repo" } else { "repos" };
    format!("Push {total_commits} commits to {n} {noun}")
}
fn outgoing_total(scope_commits: &[(&Root, Vec<Commit>)]) -> usize {
    scope_commits.iter().map(|(_, cs)| cs.len()).sum()
}
fn scope_outgoing<'a>(state: &'a AppState, scope: &'a [Root]) -> Vec<(&'a Root, Vec<Commit>)> {
    let snapshot = state.ui.dlg.push_outgoing.as_deref().unwrap_or(&[]);
    scope
        .iter()
        .filter_map(|r| {
            let entry = snapshot.iter().find(|e| e.id == r.id)?;
            let commits = entry.commits.as_ref().ok()?.clone();
            Some((r, commits))
        })
        .collect()
}

/// Paint the PUSH SCOPE segmented control (issue #25, screen 10).
fn push_scope_control(ui: &mut Ui, state: &mut AppState) {
    let all = state.multi.roots.len();
    let sel = state.ui.repo_selection.len();
    let subtree = state
        .multi
        .roots
        .iter()
        .filter(|r| r.path.starts_with(&state.project_dir))
        .count();
    ui.horizontal(|ui| {
        ui.label(RichText::new("PUSH SCOPE").small().strong());
        let this_label = "This repo";
        if ui
            .selectable_label(state.ui.dlg.push_scope == PushScope::ThisRepo, this_label)
            .clicked()
        {
            state.ui.dlg.push_scope = PushScope::ThisRepo;
            state.ui.dlg.push_scope_excluded.clear();
        }
        let sel_label = format!("Selected {sel}");
        if ui
            .selectable_label(state.ui.dlg.push_scope == PushScope::Selection, &sel_label)
            .clicked()
        {
            state.ui.dlg.push_scope = PushScope::Selection;
            state.ui.dlg.push_scope_excluded.clear();
        }
        let sub_label = format!("Project subtree ({subtree})");
        if ui
            .selectable_label(state.ui.dlg.push_scope == PushScope::Subtree, &sub_label)
            .clicked()
        {
            state.ui.dlg.push_scope = PushScope::Subtree;
            state.ui.dlg.push_scope_excluded.clear();
        }
        let all_label = format!("All {all}");
        if ui
            .selectable_label(state.ui.dlg.push_scope == PushScope::All, &all_label)
            .clicked()
        {
            state.ui.dlg.push_scope = PushScope::All;
            state.ui.dlg.push_scope_excluded.clear();
        }
    });
}

/// Paint the protected-branch remediation banner (issue #25). Returns the
/// number of in-scope protected roots after exclusions so callers can skip
/// downstream handling. The banner names repos + matching pattern; the
/// button excludes those repos and lets the push proceed for the rest.
fn remediation_banner(ui: &mut Ui, state: &mut AppState, scope: &[Root]) -> usize {
    let protected = sync_service::protected_roots(&state.settings, scope);
    if protected.is_empty() {
        return 0;
    }
    let mut names: Vec<&str> = protected.iter().map(|p| p.name.as_str()).collect();
    names.sort();
    let names_str = match names.len() {
        1 => names[0].to_string(),
        2 => format!("{} and {}", names[0], names[1]),
        _ => {
            let head = &names[..names.len() - 1];
            let last = names.last().unwrap();
            format!("{}, and {}", head.join(", "), last)
        }
    };
    let mut seen_patterns: Vec<String> = Vec::new();
    for p in &protected {
        for pat in &p.patterns {
            if !seen_patterns.contains(pat) {
                seen_patterns.push(pat.clone());
            }
        }
    }
    let patterns = seen_patterns.join("|");
    let branch_label = protected[0].branch.clone();
    ui.colored_label(
        Palette::STATE_WARNING,
        format!(
            "Protected branch in scope — {names_str} track {branch_label}, which matches {patterns}"
        ),
    );
    let label: String = if protected.len() == 1 {
        "Remove 1 protected repo from scope".to_string()
    } else {
        format!("Remove {} protected repos from scope", protected.len())
    };
    if ui.button(label).clicked() {
        for p in &protected {
            state.ui.dlg.push_scope_excluded.insert(p.id.clone());
        }
    }
    protected.len()
}

/// Paint the verbatim per-root dry-run reports under the aggregate line
fn preview_reports(ui: &mut Ui, preview: &PushPreview) {
    let has_ok = preview.reports.iter().any(|(_, r)| r.is_ok());
    let has_err = preview.reports.iter().any(|(_, r)| r.is_err());
    if has_ok {
        ui.label("Dry-run report (verbatim):");
    }
    if has_err {
        ui.colored_label(Palette::STATE_ERROR, "Push rejected by git:");
    }
    egui::ScrollArea::vertical()
        .max_height(160.0)
        .show(ui, |ui| {
            for (name, report) in &preview.reports {
                match report {
                    Ok(text) => {
                        ui.label(RichText::new(format!("[{name}]")).small().strong());
                        ui.label(RichText::new(text).monospace().small());
                    }
                    Err(stderr) => {
                        ui.colored_label(Palette::STATE_ERROR, format!("[{name}] rejected:"));
                        ui.label(
                            RichText::new(stderr)
                                .monospace()
                                .small()
                                .color(Palette::STATE_ERROR),
                        );
                    }
                }
            }
        });
}

/// Run a REAL `git push --dry-run` per root in the resolved scope and store
/// the aggregated preview. Verbatim per-root reports and a `N commits → R
/// remotes · X refs · Y rejected` summary (issue #25).
fn run_preview(state: &mut AppState) {
    let scope = scope_roots(state);
    let exec = state.executor.clone();
    let _settings = state.settings.clone();
    let force = state.ui.dlg.force_push;
    let subset = state.ui.dlg.push_subset.clone();
    let narrowed_sha: Option<String> = match &subset {
        SubsetPushState::Suffix { oldest } => Some(oldest.clone()),
        _ => None,
    };
    let scope_commits = scope_outgoing(state, &scope);
    let total: usize = scope_commits.iter().map(|(_, cs)| cs.len()).sum();

    let results: Vec<(String, Result<String, String>)> =
        if scope.len() == 1 && state.ui.dlg.push_scope == PushScope::ThisRepo {
            // Single-root path: keep the old Remote/Branch field semantics.
            let root = &scope[0];
            let branch = state.ui.dlg.push_branch.clone();
            let remote = state.ui.dlg.push_remote.clone();
            let r = exec
                .push_dry_run(&root.path, &remote, &branch, force)
                .map_err(|e| verbatim_stderr(&e));
            vec![(remote, r)]
        } else {
            let refs: Vec<&Root> = scope.iter().collect();
            sync_service::push_dry_run_roots(exec.as_ref(), &refs, force, narrowed_sha.as_deref())
                .into_iter()
                .map(|(remote, res)| (remote, res.map_err(|e| verbatim_stderr(&e))))
                .collect()
        };

    let summary = sync_service::summarize_dry_runs(&results);
    let reports: Vec<(String, Result<String, String>)> = results
        .into_iter()
        .map(|(remote, res)| {
            let name = scope
                .iter()
                .find(|r| {
                    r.branches.iter().any(|b| {
                        b.tracking
                            .as_deref()
                            .and_then(|t| t.split('/').next())
                            .map(|rname| rname == remote.as_str())
                            .unwrap_or(false)
                    })
                })
                .map(|r| {
                    r.path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| r.path.display().to_string())
                })
                .unwrap_or_else(|| remote.clone());
            (name, res)
        })
        .collect();
    state.ui.dlg.push_preview_output = Some(PushPreview {
        commits: total,
        remotes: summary.remotes,
        refs: summary.refs,
        rejected: summary.rejected,
        reports,
    });
}

/// Extract git's verbatim stderr from an engine error.
fn verbatim_stderr(e: &TgError) -> String {
    match e {
        TgError::Cli { stderr, .. } => stderr.clone(),
        other => other.to_string(),
    }
}

/// Build the outgoing-commit snapshot once when the dialog opens. Seeds the
/// commit-selection list with every outgoing SHA so an untouched dialog pushes
/// everything ahead (issue #24).
fn ensure_outgoing(state: &mut AppState) {
    if state.ui.dlg.push_outgoing.is_some() {
        return;
    }
    let exec = state.executor.clone();
    let results = sync_service::outgoing_per_root(exec.as_ref(), &state.multi);
    let mut out = Vec::with_capacity(results.len());
    let mut selected = Vec::new();
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
                            signature: SignatureState::Unsigned,
                        })
                    })
                    .collect::<Vec<_>>())
            }
            Err(e) => Err(e.to_string()),
        };
        if let Ok(cs) = &commits {
            selected.extend(cs.iter().map(|c| c.id.clone()));
        }
        out.push(OutgoingRoot { id, name, commits });
    }
    state.ui.dlg.push_outgoing = Some(out);
    state.ui.dlg.push_selected_commits = selected;
}

fn empty_signature() -> Signature {
    Signature {
        name: String::new(),
        email: String::new(),
        time: 0,
    }
}

/// Prefill Remote/Branch from the selected root's tracking config. Only
/// fills while the remote field is empty so user edits persist across redraws.
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
/// PREVIEW filter only (ADR-0006); they never affect push scope. Issue #24
/// adds per-commit checkboxes plus a live "N selected of M" counter.
fn outgoing_tree(ui: &mut Ui, state: &mut AppState) {
    let mut select_root: Option<Option<RootId>> = None;
    let snapshot = state
        .ui
        .dlg
        .push_outgoing
        .as_deref()
        .unwrap_or(&[])
        .to_vec();
    let total: usize = snapshot
        .iter()
        .map(|r| r.commits.as_ref().map_or(0, |c| c.len()))
        .sum();
    let selected_count = state.ui.dlg.push_selected_commits.len();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{selected_count} selected of {total}"))
                .small()
                .weak(),
        );
    });
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .show(ui, |ui| {
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

            if total == 0 {
                ui.label("No outgoing commits.");
            }

            for entry in &snapshot {
                let n = entry.commits.as_ref().map_or(0, |c| c.len());
                let node = format!("{} — {n} commits ahead", entry.name);
                let selected = state.ui.dlg.push_preview_root.as_ref() == Some(&entry.id);
                ui.indent(entry.id.0.as_os_str(), |ui| {
                    if ui.selectable_label(selected, &node).clicked() {
                        select_root = Some(if selected {
                            None
                        } else {
                            Some(entry.id.clone())
                        });
                    }
                    if let Ok(commits) = &entry.commits {
                        ui.indent((entry.id.0.as_os_str(), "commits"), |ui| {
                            for c in commits {
                                commit_row(ui, state, c);
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

fn commit_row(ui: &mut Ui, state: &mut AppState, c: &Commit) {
    let short = &c.id[..c.id.len().min(7)];
    let subject = c.message.lines().next().unwrap_or("");
    let mut checked = state.ui.dlg.push_selected_commits.contains(&c.id);
    ui.horizontal(|ui| {
        if ui.checkbox(&mut checked, subject).changed() {
            if checked {
                if !state.ui.dlg.push_selected_commits.contains(&c.id) {
                    state.ui.dlg.push_selected_commits.push(c.id.clone());
                }
            } else {
                state.ui.dlg.push_selected_commits.retain(|x| x != &c.id);
            }
        }
        ui.label(RichText::new(short).monospace().color(Palette::BRAND));
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
/// ONLY (ADR-0006).
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

/// Dispatch the push on a worker thread. Issue #25: the resolved scope
/// drives the engine call — `push_roots` for multi-repo scopes (with each
/// root's per-call remote resolution and protected-branch gating), and
/// `push` for the single-repo `ThisRepo` path with explicit Remote/Branch
/// fields. Issue #24: subset selection narrows per root only when the
/// selection is a suffix of the full outgoing list — non-suffix selections
/// fall back to a full push (issue #24 fallback semantics).
fn execute_push(state: &mut AppState) {
    let force = state.ui.dlg.force_push;
    let tags = state.ui.dlg.push_tags;
    let no_verify = state.ui.dlg.push_no_verify;
    let set_upstream = state.ui.dlg.push_set_upstream;
    let settings = state.settings.clone();
    let scope = scope_roots(state);
    let subset = state.ui.dlg.push_subset.clone();
    let narrowed_sha: Option<String> = match &subset {
        SubsetPushState::Suffix { oldest } => Some(oldest.clone()),
        _ => None,
    };

    if state.ui.dlg.push_scope == PushScope::ThisRepo {
        let root = state.selected_path();
        let remote = state.ui.dlg.push_remote.clone();
        let branch = state.ui.dlg.push_branch.clone();
        let sha = narrowed_sha.clone();
        state.run_git(
            "Push".into(),
            Affected::from_optional_root(root.as_deref()),
            move |v| match root {
                Some(r) => sync_service::push(
                    v,
                    &r,
                    &remote,
                    &branch,
                    force,
                    tags,
                    no_verify,
                    set_upstream,
                    sha.as_deref(),
                    &settings,
                ),
                None => Ok(()),
            },
        );
    } else {
        // Multi-repo push (issue #25): push every root in the resolved scope
        // via `push_roots`; per-root remote resolution and protected-branch
        // gating happen inside that service. The roots must outlive the
        // worker thread, so they move into the closure as owned data.
        let owned_roots: Vec<Root> = scope.clone();
        let owned_settings = settings;
        let owned_sha = narrowed_sha;
        state.run_git("Push".into(), Affected::All, move |v| {
            let roots_ref: Vec<&Root> = owned_roots.iter().collect();
            let results = sync_service::push_roots(
                v,
                &roots_ref,
                &owned_settings,
                force,
                tags,
                no_verify,
                set_upstream,
                owned_sha.as_deref(),
            );
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
