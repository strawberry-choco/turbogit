use crate::theme::Palette;
use crate::ui::components;
use crate::ui::widgets;
use egui::{Align, Layout, Pos2, Rect, RichText, ScrollArea, Ui, UiBuilder, Vec2};
use turbogit_app::operation::PushRequest;
use turbogit_app::state::{AppState, OutgoingRoot, PushPreview};
use turbogit_app::sync_service::{self, PushScope, SubsetPushState};
use turbogit_domain::model::{
    BranchKind, ChangeStatus, Commit, CommitId, DisplayName, Root, RootId, Signature,
    SignatureState,
};

const PANE_BODY_H: f32 = 197.0;

const SCOPE_ORDER: [PushScope; 4] = [
    PushScope::ThisRepo,
    PushScope::Selection,
    PushScope::Subtree,
    PushScope::All,
];

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    // The window's height is capped to the screen. A body that grows past the
    // screen's bottom edge makes egui MOVE the window up, and a window that
    // moves reflows every row under it, so the cap keeps the dialog's own
    // rows where the user last saw them, and the report well scrolls inside
    // the dialog instead of pushing the footer off the screen.
    let max_height = ctx.input(|i| i.content_rect().height()) - 40.0;
    egui::Window::new("Push")
        .open(&mut open)
        .default_width(720.0)
        .max_height(max_height)
        .show(&ctx, |ui| {
            ensure_outgoing(state);
            ensure_target_defaults(state);

            let scope = scope_roots(state);
            let scope_commits = scope_outgoing(state, &scope);
            let total = outgoing_total(&scope_commits);

            push_scope_control(ui, state);

            let _protected_count = remediation_banner(ui, state, &scope);

            push_panes(ui, state);

            let subset = subset_push_state(state);
            state.ui.dlg.push_subset = subset.clone();

            let scope_is_this_repo = state.ui.dlg.push_scope == PushScope::ThisRepo;
            let mut no_target = false;
            if scope_is_this_repo {
                no_target = target_panel(ui, state);
            }

            options_band(ui, state, scope_is_this_repo, &subset);

            if let Some(preview) = state.ui.dlg.push_preview_output.as_ref() {
                dry_run_well(ui, preview);
            }

            let force_blocked_this_repo = scope_is_this_repo
                && state.ui.dlg.force_push
                && sync_service::is_protected(&state.settings, &state.ui.dlg.push_branch);
            let label = if scope_is_this_repo {
                "Push".to_string()
            } else {
                action_label(&scope, total)
            };
            widgets::dialog_footer(ui, |ui| {
                let push = ui
                    .scope(|ui| {
                        if force_blocked_this_repo || no_target {
                            ui.disable();
                        }
                        widgets::compact_primary_button(ui, &label)
                    })
                    .inner;
                if push.clicked() {
                    execute_push(state);
                    close(state);
                }
                if widgets::compact_button(ui, "Preview dry-run").clicked() {
                    run_preview(state);
                }
                if widgets::compact_button(ui, "Cancel").clicked() {
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
    state.ui.dlg.push_define_remote = false;
    state.ui.dlg.push_new_remote_name.clear();
    state.ui.dlg.push_new_remote_url.clear();
    state.ui.dlg.push_remote_add_error = None;
}

fn target_panel(ui: &mut Ui, state: &mut AppState) -> bool {
    widgets::group_title(ui, "TARGET");

    let root = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .cloned();
    let no_remote = root.as_ref().is_some_and(|r| r.remotes.is_empty());
    let defining = no_remote && state.ui.dlg.push_define_remote;
    let remote_field = state.ui.dlg.push_remote.clone();
    let branch_field = state.ui.dlg.push_branch.clone();
    let set_upstream = state.ui.dlg.push_set_upstream;
    let target = root.as_ref().and_then(|r| {
        sync_service::push_target_state(r, &remote_field, &branch_field, set_upstream)
    });
    let cannot_push = root
        .as_ref()
        .is_some_and(|r| r.remotes.is_empty() || sync_service::repository_is_empty(r));

    ui.horizontal(|ui| {
        ui.label("Remote:");
        if !no_remote {
            ui.text_edit_singleline(&mut state.ui.dlg.push_remote);
        } else if defining {
            ui.text_edit_singleline(&mut state.ui.dlg.push_new_remote_name);
        } else if define_remote_link(ui).clicked() {
            state.ui.dlg.push_define_remote = true;
            if state.ui.dlg.push_new_remote_name.trim().is_empty() {
                state.ui.dlg.push_new_remote_name = "origin".to_string();
            }
        }
    });

    if defining {
        ui.horizontal(|ui| {
            ui.label("URL:");
            ui.text_edit_singleline(&mut state.ui.dlg.push_new_remote_url);
            let name = state.ui.dlg.push_new_remote_name.trim().to_string();
            let url = state.ui.dlg.push_new_remote_url.trim().to_string();
            let add = widgets::compact_button_enabled(
                ui,
                "Add Remote",
                !name.is_empty() && !url.is_empty(),
            );
            if add.clicked()
                && let Some(root) = &root
            {
                state.define_remote_for_push(root.id.clone(), name, url);
            }
            if widgets::compact_button(ui, "Cancel").clicked() {
                state.ui.dlg.push_define_remote = false;
                state.ui.dlg.push_remote_add_error = None;
            }
        });
    } else {
        ui.horizontal(|ui| {
            ui.label("Branch:");
            ui.text_edit_singleline(&mut state.ui.dlg.push_branch);
        });
        if target.is_some() || cannot_push {
            ui.horizontal(|ui| {
                if let Some(target) = target {
                    ui.label(RichText::new(target.label()).color(target_ink(target)));
                }
                if cannot_push {
                    ui.label(RichText::new("Cannot push").color(Palette::STATE_WARNING));
                }
            });
        }
    }
    if let Some(detail) = state.ui.dlg.push_remote_add_error.clone() {
        widgets::inline_error(ui, format!("Could not add remote: {detail}"));
    }
    cannot_push
}

fn define_remote_link(ui: &mut Ui) -> egui::Response {
    let ink = ui.style().visuals.hyperlink_color;
    ui.add(egui::Button::new(RichText::new("Define remote").color(ink)).frame(false))
}

fn target_ink(target: sync_service::PushTargetState) -> egui::Color32 {
    match target {
        sync_service::PushTargetState::EmptyRepository => Palette::STATE_WARNING,
        _ => Palette::STATE_INFO,
    }
}

fn options_band(
    ui: &mut Ui,
    state: &mut AppState,
    scope_is_this_repo: bool,
    subset: &SubsetPushState,
) {
    widgets::group_title(ui, "OPTIONS");
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut state.ui.dlg.force_push,
            "Force push (--force-with-lease)",
        );
        ui.checkbox(&mut state.ui.dlg.push_tags, "Push tags");
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.ui.dlg.push_no_verify, "Skip pre-push hooks");
        ui.checkbox(
            &mut state.ui.dlg.push_set_upstream,
            "Set upstream (--set-upstream)",
        );
    });
    if state.ui.dlg.push_set_upstream {
        ui.colored_label(Palette::STATE_INFO, "upstream will be set");
    }
    if matches!(subset, SubsetPushState::OlderUnchecked) {
        ui.colored_label(
            Palette::STATE_WARNING,
            "All commits will be pushed — an unchecked older ancestor forces a full push.",
        );
    }

    if scope_is_this_repo {
        let branch = state.ui.dlg.push_branch.clone();
        let force_blocked =
            state.ui.dlg.force_push && sync_service::is_protected(&state.settings, &branch);
        if force_blocked {
            widgets::inline_error(
                ui,
                format!("⚠ '{branch}' is protected — force-push blocked."),
            );
            widgets::inline_error(
                ui,
                "Uncheck force push or retarget the Branch field to continue.",
            );
        } else if state.ui.dlg.force_push {
            ui.colored_label(
                Palette::STATE_WARNING,
                "⚠ Force push rewrites the remote branch (--force-with-lease).",
            );
        }
    }
}

fn dry_run_well(ui: &mut Ui, preview: &PushPreview) {
    widgets::note(ui, None, |ui| {
        ui.label(
            RichText::new(format!(
                "{} commits → {} remotes · {} refs · {} rejected",
                preview.commits, preview.remotes, preview.refs, preview.rejected
            ))
            .small()
            .strong(),
        );
        preview_reports(ui, preview);
    });
}

fn scope_roots(state: &AppState) -> Vec<Root> {
    state.push_scope_roots()
}

fn action_label(scope: &[Root], total_commits: usize) -> String {
    let n = scope.len();
    if n == 0 {
        return "Push".to_string();
    }
    format!(
        "Push {total_commits} commits to {}",
        components::counted(n, "repo", "repos")
    )
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

fn scope_index(scope: PushScope) -> usize {
    SCOPE_ORDER.iter().position(|s| *s == scope).unwrap_or(0)
}

fn scope_from_index(index: usize) -> PushScope {
    SCOPE_ORDER.get(index).copied().unwrap_or(PushScope::All)
}

fn scope_labels(state: &AppState) -> [String; 4] {
    let selected = state.ui.repo_selection.len();
    let subtree = state
        .multi
        .roots
        .iter()
        .filter(|r| r.path.starts_with(&state.project_dir))
        .count();
    let all = state.multi.roots.len();
    [
        "This repo".to_string(),
        format!("Selected {selected}"),
        format!("Project subtree ({subtree})"),
        format!("All {all}"),
    ]
}

fn push_scope_control(ui: &mut Ui, state: &mut AppState) {
    let labels = scope_labels(state);
    let options: Vec<&str> = labels.iter().map(String::as_str).collect();
    ui.horizontal(|ui| {
        widgets::group_title(ui, "PUSH SCOPE");
        let selected = scope_index(state.ui.dlg.push_scope);
        if let Some(index) = widgets::segmented_control(ui, &options, selected) {
            state.ui.dlg.push_scope = scope_from_index(index);
            state.ui.dlg.push_scope_excluded.clear();
        }
    });
}

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
    widgets::alert_box(
        ui,
        &format!(
            "Protected branch in scope — {names_str} track {branch_label}, which matches {patterns}"
        ),
    );
    let label: String = format!(
        "Remove {} from scope",
        components::counted(protected.len(), "protected repo", "protected repos")
    );
    if widgets::compact_button(ui, &label).clicked() {
        for p in &protected {
            state.ui.dlg.push_scope_excluded.insert(p.id.clone());
        }
    }
    protected.len()
}

fn push_panes(ui: &mut Ui, state: &mut AppState) {
    let row = ui.available_rect_before_wrap();
    let gap = ui.spacing().item_spacing.x;
    let usable = row.width() - gap;
    let left_rect = Rect::from_min_max(
        row.min,
        Pos2::new(row.min.x + usable * 0.55, row.min.y + PANE_BODY_H),
    );
    let right_rect = Rect::from_min_max(
        Pos2::new(left_rect.right() + gap, row.min.y),
        Pos2::new(row.right(), row.min.y + PANE_BODY_H),
    );
    // The row's height belongs to the parent, or the bands below would paint
    // over the panes.
    ui.allocate_space(Vec2::new(row.width(), PANE_BODY_H));

    let mut left = ui.new_child(
        UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    commits_pane(&mut left, state);
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    changed_files_pane(&mut right, state);

    let x = (left_rect.right() + right_rect.left()) / 2.0;
    widgets::edge_rule(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(x, row.min.y),
            Pos2::new(x, row.min.y + PANE_BODY_H),
        ),
        widgets::Edge::Right,
        Palette::RULE_STRUCTURAL,
    );
}

fn commits_pane(ui: &mut Ui, state: &mut AppState) {
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
        widgets::group_title(ui, "COMMITS");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{selected_count} selected of {total}"))
                    .small()
                    .weak(),
            );
        });
    });
    let project = state.project_dir.display_name_lossy_or("Project");
    let body_h = ui.available_height();
    let mut select_root: Option<Option<RootId>> = None;
    // The salt is not decoration: the two panes are sibling children of one
    // Ui, and egui gives sibling children the SAME id, so two scroll areas
    // without their own salts would share one persisted scroll state and
    // trade the scrollbar's visibility back and forth every frame.
    ScrollArea::vertical()
        .id_salt("push_commits")
        .max_height(body_h)
        .show(ui, |ui| {
            if snapshot.is_empty() {
                widgets::empty_state(ui, "No repositories to push.");
                return;
            }
            let project_node = format!("{project} (all roots)");
            if ui
                .selectable_label(state.ui.dlg.push_preview_root.is_none(), &project_node)
                .clicked()
            {
                select_root = Some(None);
            }

            if total == 0 {
                widgets::empty_state(ui, "No outgoing commits.");
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
    let subject = c.subject();
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
        widgets::hash_chip(ui, &widgets::short_commit_ref(&c.id), "");
        let meta = format!("{} · {}", c.author.name, rel_time(c.time));
        ui.label(RichText::new(meta).small().weak());
    });
}

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

fn changed_files_pane(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        widgets::group_title(ui, "CHANGED FILES");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(preview_subject(state)).small().weak());
        });
    });
    let body_h = ui.available_height();
    ScrollArea::vertical()
        .id_salt("push_changed_files")
        .max_height(body_h)
        .show(ui, |ui| {
            let wanted: Vec<(RootId, CommitId)> = state
                .ui
                .dlg
                .push_outgoing
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .filter(|e| {
                    state
                        .ui
                        .dlg
                        .push_preview_root
                        .as_ref()
                        .is_none_or(|f| f == &e.id)
                })
                .flat_map(|e| {
                    e.commits
                        .iter()
                        .flat_map(move |cs| cs.iter().map(move |c| (e.id.clone(), c.id.clone())))
                })
                .collect();
            let mut any = false;
            for (root, commit) in wanted {
                for ch in state.ensure_files(&root, &commit) {
                    any = true;
                    ui.label(format!(
                        "{} {}",
                        change_letter(ch.status),
                        ch.path.display()
                    ));
                }
            }
            if !any {
                widgets::empty_state(ui, "No changed files.");
            }
        });
}

fn preview_subject(state: &AppState) -> String {
    match &state.ui.dlg.push_preview_root {
        None => "All roots".to_string(),
        Some(id) => state
            .ui
            .dlg
            .push_outgoing
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|e| &e.id == id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| id.name()),
    }
}

fn change_letter(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Modified => "M",
        ChangeStatus::Added => "A",
        ChangeStatus::Deleted => "D",
        _ => "?",
    }
}

fn subset_push_state(state: &AppState) -> SubsetPushState {
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
    sync_service::subset_push_state(&outgoing_shas, &state.ui.dlg.push_selected_commits)
}

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
        .id_salt("push_preview_reports")
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

fn run_preview(state: &mut AppState) {
    let scope = scope_roots(state);
    let force = state.ui.dlg.force_push;
    let subset = state.ui.dlg.push_subset.clone();
    let narrowed_sha: Option<String> = match &subset {
        SubsetPushState::Suffix { oldest } => Some(oldest.clone()),
        _ => None,
    };
    let scope_commits = scope_outgoing(state, &scope);
    let total: usize = scope_commits.iter().map(|(_, cs)| cs.len()).sum();

    let single = (scope.len() == 1 && state.ui.dlg.push_scope == PushScope::ThisRepo).then_some((
        state.ui.dlg.push_remote.as_str(),
        state.ui.dlg.push_branch.as_str(),
    ));
    let results = state.push_dry_run_reports(&scope, single, force, narrowed_sha.as_deref());

    let summary = sync_service::summarize_dry_runs(&results);
    let reports: Vec<(String, Result<String, String>)> = results
        .into_iter()
        .map(|(remote, res)| {
            let name = scope
                .iter()
                .find(|r| {
                    r.branches
                        .iter()
                        .any(|b| b.tracking.as_ref().is_some_and(|t| t.remote == remote))
                })
                .map(|r| r.path.display_name_lossy_or(&r.path.display().to_string()))
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

fn ensure_outgoing(state: &mut AppState) {
    if state.ui.dlg.push_outgoing.is_some() {
        return;
    }
    let results = state.outgoing_per_root();
    let mut out = Vec::with_capacity(results.len());
    let mut selected = Vec::new();
    for (id, res) in results {
        let path = id.as_path();
        let name = path.display_name_lossy_or(&path.display().to_string());
        let commits = match res {
            Ok(ids) => {
                let log = state.full_log(&id);
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

fn ensure_target_defaults(state: &mut AppState) {
    let Some(id) = state.selected_root.clone() else {
        return;
    };
    let Some(root) = state.multi.by_id(&id) else {
        return;
    };
    let tracked = root
        .branches
        .iter()
        .find(|b| b.kind == BranchKind::Local && root.current_branch.as_deref() == Some(&b.name))
        .and_then(|b| b.tracking.clone());
    let tracked_remote = tracked.as_ref().map(|t| t.remote.clone());
    let tracked_branch = tracked.map(|t| t.branch);
    let first_remote = root.remotes.first().map(|r| r.name.clone());
    let current_branch = root.current_branch.clone().unwrap_or_default();

    if state.ui.dlg.push_remote.is_empty() {
        state.ui.dlg.push_remote = tracked_remote.or(first_remote).unwrap_or_default();
    }
    if state.ui.dlg.push_branch.is_empty() {
        state.ui.dlg.push_branch = tracked_branch.unwrap_or(current_branch);
    }
}

fn execute_push(state: &mut AppState) {
    let scope = scope_roots(state);
    let narrowed_sha: Option<String> = match &state.ui.dlg.push_subset {
        SubsetPushState::Suffix { oldest } => Some(oldest.clone()),
        _ => None,
    };
    let Some(request) = push_request(state, &scope, narrowed_sha) else {
        return;
    };
    state.dispatch_push(request, None);
}

pub(crate) fn push_request(
    state: &AppState,
    scope: &[Root],
    narrowed_sha: Option<String>,
) -> Option<PushRequest> {
    let mut request = if state.ui.dlg.push_scope == PushScope::ThisRepo {
        let root = state.selected_root.clone()?;
        PushRequest::single(
            root,
            state.ui.dlg.push_remote.clone(),
            state.ui.dlg.push_branch.clone(),
            &state.settings,
        )
    } else {
        PushRequest::scope(scope.to_vec(), &state.settings)
    };
    request.force = state.ui.dlg.force_push;
    request.tags = state.ui.dlg.push_tags;
    request.no_verify = state.ui.dlg.push_no_verify;
    request.set_upstream = state.ui.dlg.push_set_upstream;
    request.selected_oldest = narrowed_sha;
    Some(request)
}
