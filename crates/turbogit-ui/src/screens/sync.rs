//! The Sync surface: the Push dialog with its consent rows and dry-run
//! preview, the rejection pane with its next step and update scope, and the
//! per-repository fetch and pull controls. Every verb is a state-layer
//! operation; the pane owns no git.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::PushTags;
use turbogit_app::operation::{PushRequest, RecoveryMode, RecoveryPlan};
use turbogit_app::push_session::Focus;
use turbogit_app::state::{AppState, Dialog, PushPreview, PushRejection};
use turbogit_app::sync_service::{self, PushOutcome, UpdateChoice};
use turbogit_domain::model::{CommitId, RootId, UpdateMethod};

use crate::bridge::AppHandle;
use crate::screens::shelf::counted;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::row::{RowProps, RowState, row};

/// The Push dialog and the rejection pane, whichever the state holds.
pub fn sync_layer(handle: AppHandle) -> impl IntoElement {
    let open = handle.clone();
    let dialog_handle = handle.clone();
    let rejection = handle.clone();
    let rejection_cond = handle.clone();
    view()
        .child(when(
            move || open.read(|s| s.ui.dialog == Some(Dialog::Push)),
            move || {
                let inner = dialog_handle.clone();
                dynamic(move || {
                    inner.read(|_| ());
                    push_dialog(inner.clone()).into_element()
                })
            },
        ))
        .child(when(
            move || rejection_cond.read(|s| s.ui.push_rejection.is_some()),
            move || {
                let inner = rejection.clone();
                dynamic(move || {
                    inner.read(|_| ());
                    push_rejection(inner.clone())
                })
            },
        ))
}

/// The Push dialog.
pub fn push_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let rows = handle.peek(|s| s.ui.dlg.push.rows.clone());
    let focus = handle.peek(|s| s.ui.dlg.push.focus.clone());
    let consented = rows.iter().filter(|row| row.consented).count();
    let commits: usize = rows
        .iter()
        .filter(|row| row.consented)
        .map(|row| row.listed_total)
        .sum();
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::sync("push-dialog"))
        .child(
            text("REPOSITORIES")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    if rows.is_empty() {
        body = body.child(
            text("No repositories registered.")
                .text_size(px(data::TYPE_BODY))
                .text_color(t.muted_foreground),
        );
    }
    for (index, push_row) in rows.iter().enumerate() {
        body = body.child(push_row_view(handle.clone(), index, push_row, &focus));
    }
    body = body.child(
        text(format!(
            "{} · {}",
            counted(consented, "repository", "repositories"),
            counted(commits, "commit", "commits")
        ))
        .text_size(px(data::TYPE_CHIP))
        .text_color(t.muted_foreground)
        .test_id(&test_ids::sync("consent-line")),
    );

    body = body.child(
        text("OPTIONS")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    let (no_verify, tags, force) = handle.peek(|s| {
        (
            s.ui.dlg.push.no_verify,
            s.ui.dlg.push.tags,
            s.ui.dlg.push.force,
        )
    });
    let hooks_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(checkbox(
                CheckboxProps::new(move || {
                    if no_verify {
                        CheckboxState::Unchecked
                    } else {
                        CheckboxState::Checked
                    }
                })
                .accessibility_label("Run Git hooks")
                .test_id(&test_ids::sync("hooks"))
                .on_change(move |_| {
                    hooks_handle.update(|s| s.ui.dlg.push.no_verify = !s.ui.dlg.push.no_verify);
                }),
            ))
            .child(
                text("Run Git hooks")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            ),
    );
    let mut tag_rows = view().flex_row().gap(px(4.0));
    for value in [PushTags::None, PushTags::All, PushTags::Follow] {
        tag_rows = tag_rows.child(tag_row(handle.clone(), value, tags == value));
    }
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::SPACING_X))
            .child(
                text("Push tags:")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.muted_foreground),
            )
            .child(tag_rows),
    );
    let force_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(checkbox(
                CheckboxProps::new(move || {
                    if force {
                        CheckboxState::Checked
                    } else {
                        CheckboxState::Unchecked
                    }
                })
                .accessibility_label("Force push (--force-with-lease)")
                .test_id(&test_ids::sync("force"))
                .on_change(move |_| {
                    force_handle.update(|s| s.ui.dlg.push.force = !s.ui.dlg.push.force);
                }),
            ))
            .child(
                text("Force push (--force-with-lease)")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            ),
    );
    if force {
        let protected: Vec<String> = handle.peek(|s| {
            s.ui.dlg
                .push
                .rows
                .iter()
                .filter(|row| row.consented)
                .filter_map(|row| row.branch.value().map(str::to_string))
                .filter(|branch| sync_service::is_protected(&s.settings, branch))
                .collect()
        });
        body = body.child(if protected.is_empty() {
            text("Force push rewrites the remote branch (--force-with-lease).")
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::STATE_WARNING)
                .into_element()
        } else {
            text(format!(
                "{} protected — the push refuses force for that repository.",
                protected.join(", ")
            ))
            .text_size(px(data::TYPE_CHIP))
            .text_color(data::STATE_ERROR)
            .into_element()
        });
    }

    if let Some(preview) = handle.peek(|s| s.ui.dlg.push.preview.clone()) {
        body = body.child(preview_well(preview));
    }

    let nothing = handle.peek(|s| {
        let session = &s.ui.dlg.push;
        let plan = session.payload();
        plan.targets.is_empty()
            || plan.targets.iter().all(|target| {
                session.row(&target.root).is_some_and(|row| {
                    row.listed_total == 0
                        && row.obstacle
                            != Some(turbogit_app::push_session::Obstacle::UnlistedFirstPush)
                })
            })
    });
    let stale = handle.peek(|s| {
        !s.ui
            .dlg
            .push
            .stale_rows(&|id| s.caches.ahead_behind(id).map(|(ahead, _)| ahead))
            .is_empty()
    });
    let dimmed = nothing || stale;
    let label = if dimmed { "Nothing to update" } else { "Push" };
    let push_handle = handle.clone();
    let preview_handle = handle.clone();
    let cancel_handle = handle.clone();
    let push_action =
        crate::ui::dialog::DialogAction::new(label, ButtonVariant::Primary, move || {
            push_handle.update(|s| {
                execute_push(s);
                s.ui.dialog = None;
            });
        });
    let preview_action = crate::ui::dialog::DialogAction::new(
        "Preview dry-run",
        ButtonVariant::Secondary,
        move || {
            preview_handle.update(run_preview);
        },
    );
    let props = DialogProps::new("push", "Push Commits")
        .on_cancel(move || cancel_handle.update(|s| s.ui.dialog = None))
        .child(body)
        .with_action(push_action)
        .with_action(preview_action);
    dialog(props)
}

fn push_row_view(
    handle: AppHandle,
    index: usize,
    row: &turbogit_app::push_session::PushRow,
    focus: &Option<Focus>,
) -> AnyElement {
    let t = theme();
    let name = row.name.clone();
    let consented = row.consented;
    let toggle_handle = handle.clone();
    let target = turbogit_app::push_session::target_line(
        row.local.as_deref(),
        row.remote.value().unwrap_or_default(),
        row.branch.value().unwrap_or_default(),
    );
    let focused = matches!(focus, Some(Focus::Root(id)) if *id == row.id);
    let mut line = RowProps::new()
        .state(if focused {
            RowState::Selected
        } else {
            RowState::Default
        })
        .test_id(&test_ids::sync(format!("row-{name}")))
        .child(checkbox(
            CheckboxProps::new(move || {
                if consented {
                    CheckboxState::Checked
                } else {
                    CheckboxState::Unchecked
                }
            })
            .accessibility_label(format!("Push {name}"))
            .test_id(&test_ids::sync(format!("row-check-{name}")))
            .on_change(move |_| {
                toggle_handle.update(move |s| {
                    if let Some(row) = s.ui.dlg.push.rows.get_mut(index) {
                        row.toggle_consent();
                    }
                });
            }),
        ))
        .child(
            text(name.clone())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        )
        .child(
            text(target.clone())
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        )
        .child(view().flex_1())
        .child(
            text(row.listed_total.to_string())
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    if let Some(obstacle) = row.obstacle {
        line = line.child(
            text(obstacle.label())
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::STATE_WARNING),
        );
    }
    if let Some(error) = row.error.clone() {
        line = line.child(
            text(error)
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::STATE_ERROR),
        );
    }
    let expand_handle = handle;
    let expanded = row.expanded;
    let mut out = view().flex_col().child(crate::ui::row::row(line)).child(
        view().flex_row().justify_end().child(
            view()
                .role(AccessibilityRole::Button)
                .label(if expanded {
                    format!("Collapse {name}")
                } else {
                    format!("Expand {name}")
                })
                .test_id(&test_ids::sync(format!("expand-{name}")))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .on_press(move || {
                    expand_handle.update(move |s| {
                        if let Some(row) = s.ui.dlg.push.rows.get_mut(index) {
                            row.expanded = !row.expanded;
                        }
                    });
                })
                .child(
                    text(if expanded { "▾" } else { "▸" })
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                ),
        ),
    );
    if expanded {
        for commit in &row.commits {
            let short = crate::screens::short(&commit.id);
            out = out.child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(data::CELL_GAP))
                    .pl(px(data::INDENT))
                    .child(
                        text(short)
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(t.muted_foreground),
                    )
                    .child(
                        text(commit.subject.clone())
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.foreground),
                    ),
            );
        }
    }
    out.into_element()
}

fn tag_row(handle: AppHandle, value: PushTags, active: bool) -> impl IntoElement {
    let t = theme();
    let label = value.label();
    row(RowProps::new()
        .state(if active {
            RowState::Selected
        } else {
            RowState::Default
        })
        .test_id(&test_ids::sync(format!("tags-{label}")))
        .on_press(move || {
            handle.update(move |s| s.ui.dlg.push.tags = value);
        })
        .child(
            text(label)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        ))
}

fn preview_well(preview: PushPreview) -> impl IntoElement {
    let t = theme();
    let mut well = view()
        .flex_col()
        .gap(px(2.0))
        .p(px(data::PANEL_PADDING))
        .rounded(px(data::CARD_RADIUS))
        .bg(t.muted)
        .test_id(&test_ids::sync("preview"))
        .child(
            text(format!(
                "{} commits → {} remotes · {} refs · {} rejected",
                preview.commits, preview.remotes, preview.refs, preview.rejected
            ))
            .text_size(px(data::TYPE_CHIP))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(t.foreground),
        );
    for (name, report) in preview.reports {
        match report {
            Ok(report_text) => {
                well = well
                    .child(
                        text(format!("[{name}]"))
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(t.foreground),
                    )
                    .child(
                        text(report_text)
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(t.muted_foreground),
                    );
            }
            Err(stderr) => {
                well = well
                    .child(
                        text(format!("[{name}] rejected:"))
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(data::STATE_ERROR),
                    )
                    .child(
                        text(stderr)
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(data::STATE_ERROR),
                    );
            }
        }
    }
    well
}

/// The preview's exact numbers and verbatim reports.
pub fn run_preview(state: &mut AppState) {
    let plan = state.ui.dlg.push.payload();
    let targets = plan.targets.clone();
    let results = state.push_target_dry_run(&targets, plan.force);
    let summary = sync_service::summarize_dry_runs(&results);
    let commits: usize = targets
        .iter()
        .filter_map(|target| state.ui.dlg.push.row(&target.root))
        .map(|row| row.listed_total)
        .sum();
    let reports: Vec<(String, Result<String, String>)> = targets
        .iter()
        .zip(results)
        .map(|(target, (_, report))| (target.root.name(), report))
        .collect();
    state.ui.dlg.push.preview = Some(PushPreview {
        commits,
        remotes: summary.remotes,
        refs: summary.refs,
        rejected: summary.rejected,
        reports,
    });
}

/// The batch the rows consent to, the only path from rows to git.
pub fn execute_push(state: &mut AppState) {
    let plan = state.ui.dlg.push.payload();
    let mut request = PushRequest::batch(plan.targets.clone(), &state.settings);
    request.force = plan.force;
    request.tags = plan.tags;
    request.no_verify = plan.no_verify;
    request.skipped = plan.blocked.clone();
    state.dispatch_push(request, None);
}

/// The Push dialog's footer verbs, exposed for the route and the lane.
pub fn push_footer(handle: &AppHandle) {
    handle.update(|s| {
        execute_push(s);
        s.ui.dialog = None;
    });
}

/// The rejection pane: the class's own sentence, one next step, the update
/// scope, and the remember opt-in.
fn push_rejection(handle: AppHandle) -> AnyElement {
    let t = theme();
    let Some(rejection) = handle.peek(|s| s.ui.push_rejection.clone()) else {
        return view().into_element();
    };
    if !rejection
        .affected_roots
        .iter()
        .any(|id| handle.peek(|s| s.multi.roots.iter().any(|r| &r.id == id)))
    {
        handle.update(|s| s.ui.push_rejection = None);
    }
    let message = outcome_message(&rejection, &handle);
    let title = window_title(&rejection.outcome);
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::sync("rejection"))
        .child(
            text(message)
                .text_size(px(data::TYPE_BODY))
                .text_color(data::STATE_ERROR),
        );

    let action = next_step(&rejection.outcome);
    let mut footer = view().flex_row().gap(px(data::SPACING_X)).justify_end();
    if let Some(action) = action {
        let run_handle = handle.clone();
        footer = footer.child(button(
            ButtonProps::new(action.verb())
                .variant(ButtonVariant::Primary)
                .test_id(&test_ids::sync("reject-next"))
                .on_press(move || {
                    let rejection = run_handle.peek(|s| s.ui.push_rejection.clone());
                    if let Some(rejection) = rejection {
                        run_next_step(&run_handle, &rejection, action);
                    }
                }),
        ));
    }
    let dismiss_handle = handle.clone();
    footer = footer.child(button(
        ButtonProps::new("Dismiss")
            .variant(ButtonVariant::Ghost)
            .test_id(&test_ids::sync("reject-dismiss"))
            .on_press(move || dismiss_handle.update(|s| s.ui.push_rejection = None)),
    ));

    if matches!(rejection.outcome, PushOutcome::Rejected { .. }) {
        body = body
            .child(
                text("UPDATE SCOPE")
                    .text_size(px(data::TYPE_SECTION))
                    .text_color(t.muted_foreground),
            )
            .child(scope_row(
                handle.clone(),
                UpdateChoice::AffectedOnly,
                !rejection.update_all,
            ))
            .child(scope_row(
                handle.clone(),
                UpdateChoice::AllRepositories,
                rejection.update_all,
            ));
        let remember_handle = handle.clone();
        let remembered = rejection.remember;
        body = body.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(checkbox(
                    CheckboxProps::new(move || {
                        if remembered {
                            CheckboxState::Checked
                        } else {
                            CheckboxState::Unchecked
                        }
                    })
                    .accessibility_label(
                        "Remember the update method choice and silently update in the future",
                    )
                    .test_id(&test_ids::sync("reject-remember"))
                    .on_change(move |_| {
                        remember_handle.update(|s| {
                            if let Some(rejection) = s.ui.push_rejection.as_mut() {
                                rejection.remember = !rejection.remember;
                                let enabled = rejection.remember;
                                s.settings.auto_update_on_push_rejected = enabled;
                                if let Some(draft) = s.ui.settings_draft.as_mut() {
                                    draft.auto_update_on_push_rejected = enabled;
                                }
                                let _ = turbogit_app::persistence::save_settings(
                                    &s.project_dir,
                                    &s.settings,
                                );
                            }
                        });
                    }),
                ))
                .child(
                    text("Remember the update method choice and silently update in the future")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                ),
        );
    }
    body = body.child(footer);
    let close = handle;
    nsh_base::dialog()
        .close_on_escape(true)
        .close_on_backdrop_press(true)
        .on_cancel(move || close.update(|s| s.ui.push_rejection = None))
        .child(view().absolute().inset_0().bg(t.background.opacity(0.4)))
        .child(
            view()
                .mt(px(96.0))
                .self_center()
                .w(px(520.0))
                .flex_col()
                .gap(px(data::SPACING_Y))
                .rounded(px(data::CARD_RADIUS))
                .border(px(1.0))
                .border_color(t.border)
                .bg(t.card)
                .p(px(data::PANEL_PADDING))
                .test_id(&test_ids::sync("reject-box"))
                .child(
                    text(title)
                        .text_size(px(data::TYPE_PANE_TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground),
                )
                .child(body),
        )
        .into_element()
}

fn scope_row(handle: AppHandle, choice: UpdateChoice, active: bool) -> impl IntoElement {
    let t = theme();
    let all = choice == UpdateChoice::AllRepositories;
    row(RowProps::new()
        .state(if active {
            RowState::Selected
        } else {
            RowState::Default
        })
        .test_id(&test_ids::sync(if all {
            "reject-scope-all"
        } else {
            "reject-scope-affected"
        }))
        .on_press(move || {
            handle.update(move |s| {
                if let Some(rejection) = s.ui.push_rejection.as_mut() {
                    rejection.update_all = all;
                }
            });
        })
        .child(
            text(choice.label())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NextAction {
    UpdateAndRetry,
    FetchAndRetry,
    ResolveConflicts,
}

impl NextAction {
    fn verb(self) -> &'static str {
        match self {
            Self::UpdateAndRetry => "Update and retry",
            Self::FetchAndRetry => "Fetch and retry",
            Self::ResolveConflicts => "Resolve conflicts",
        }
    }
}

fn next_step(outcome: &PushOutcome) -> Option<NextAction> {
    match outcome {
        PushOutcome::Rejected { .. } => Some(NextAction::UpdateAndRetry),
        PushOutcome::RejectedByRemote { .. } | PushOutcome::LeaseRejected { .. } => {
            Some(NextAction::FetchAndRetry)
        }
        PushOutcome::UpdateConflicted => Some(NextAction::ResolveConflicts),
        PushOutcome::UpdateUnresolved | PushOutcome::UpdateErrored { .. } => None,
    }
}

fn run_next_step(handle: &AppHandle, rejection: &PushRejection, action: NextAction) {
    match action {
        NextAction::ResolveConflicts => {
            handle.update(|s| s.ui.conflict_resolver_open = true);
        }
        NextAction::UpdateAndRetry | NextAction::FetchAndRetry => {
            let (mode, roots) = handle.peek(|s| {
                let mode = match action {
                    NextAction::UpdateAndRetry => RecoveryMode::Update {
                        method: s.settings.update_method,
                    },
                    _ => RecoveryMode::Fetch,
                };
                let roots: Vec<RootId> = if rejection.update_all {
                    s.multi.roots.iter().map(|r| r.id.clone()).collect()
                } else {
                    rejection.affected_roots.clone()
                };
                (mode, roots)
            });
            let request = rejection.request.clone();
            handle.update(move |s| {
                s.dispatch_push(request.clone(), Some(RecoveryPlan { mode, roots }));
                s.ui.push_rejection = None;
            });
        }
    }
}

fn window_title(outcome: &PushOutcome) -> String {
    match outcome {
        PushOutcome::Rejected { .. } => "Push rejected".to_string(),
        PushOutcome::RejectedByRemote { .. } => "Push rejected by the remote".to_string(),
        PushOutcome::LeaseRejected { .. } => "Force-with-lease push rejected".to_string(),
        PushOutcome::UpdateConflicted => "Push canceled — update conflicted".to_string(),
        PushOutcome::UpdateUnresolved => "Push canceled — conflicts unresolved".to_string(),
        PushOutcome::UpdateErrored { .. } => "Push rejected — update failed".to_string(),
    }
}

fn outcome_message(rejection: &PushRejection, handle: &AppHandle) -> String {
    let method = handle.peek(|s| s.settings.update_method);
    match &rejection.outcome {
        PushOutcome::Rejected { remote, branch } => {
            let head = format!("Push{} was rejected.", named(remote, " to ", ""));
            if rejection.remember {
                format!(
                    "{head} Future pushes of {branch} will update silently using{}.",
                    method_phrase(method)
                )
            } else {
                format!(
                    "{head} Update {branch} to incorporate the remote's changes, then push again."
                )
            }
        }
        PushOutcome::RejectedByRemote { remote, branch } => format!(
            "Push {} was rejected by the remote. Fetch to refresh what the remote holds, then push again.",
            branch_to(branch, remote)
        ),
        PushOutcome::LeaseRejected { remote, branch } => format!(
            "Force-with-lease push {} was rejected: the lease, not the push, is stale. Fetch to refresh it, then push again.",
            branch_to(branch, remote)
        ),
        PushOutcome::UpdateConflicted => {
            "Push was canceled, because there were conflicts during the update. Check that conflicts were resolved correctly, and invoke push again.".to_string()
        }
        PushOutcome::UpdateUnresolved => {
            "Push was canceled, because not all conflicts were resolved during the update. Resolve the conflicts and try pushing again.".to_string()
        }
        PushOutcome::UpdateErrored { detail } => {
            format!("Push was rejected, and update failed with an error: {detail}")
        }
    }
}

fn method_phrase(method: UpdateMethod) -> String {
    match method {
        UpdateMethod::Rebase => " a rebase".to_string(),
        UpdateMethod::Merge => " a merge".to_string(),
    }
}

fn named(value: &str, prefix: &str, suffix: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("{prefix}{value}{suffix}")
    }
}

fn branch_to(branch: &str, remote: &str) -> String {
    match (branch.is_empty(), remote.is_empty()) {
        (false, false) => format!("{branch} to {remote}"),
        (false, true) => branch.to_string(),
        _ => String::new(),
    }
}

/// The Sync route: one row per repository with its drift and its three verbs.
pub fn sync_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let roots = handle.peek(|s| {
        s.multi
            .roots
            .iter()
            .map(|root| {
                let ahead = s.caches.ahead_behind(&root.id).map(|(a, _)| a).unwrap_or(0);
                let behind = s.caches.ahead_behind(&root.id).map(|(_, b)| b).unwrap_or(0);
                (root.id.clone(), ahead, behind)
            })
            .collect::<Vec<_>>()
    });
    let mut list = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::sync("roots"));
    for (root, ahead, behind) in roots {
        list = list.child(sync_root_row(handle.clone(), root, ahead, behind));
    }
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("sync"))
        .child(
            text("Sync")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(list)
}

fn sync_root_row(handle: AppHandle, root: RootId, ahead: usize, behind: usize) -> impl IntoElement {
    let t = theme();
    let name = root.name();
    let fetch_handle = handle.clone();
    let root_fetch = root.clone();
    let pull_handle = handle.clone();
    let root_pull = root.clone();
    let push_handle = handle.clone();
    let root_push = root.clone();
    row(RowProps::new()
        .state(RowState::Default)
        .test_id(&test_ids::sync(format!("root-{name}")))
        .child(
            text(name.clone())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground)
                .flex_1(),
        )
        .child(
            text(format!("↑{ahead} ↓{behind}"))
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        )
        .child(
            view()
                .role(AccessibilityRole::Button)
                .label(format!("Fetch {name}"))
                .test_id(&test_ids::sync(format!("fetch-{name}")))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .on_press(move || {
                    let roots = vec![root_fetch.clone()];
                    fetch_handle.update(move |s| {
                        s.dispatch(turbogit_app::operation::Operation::Fetch {
                            roots: roots.clone(),
                        })
                    });
                })
                .child(
                    text("Fetch")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        )
        .child(
            view()
                .role(AccessibilityRole::Button)
                .label(format!("Pull {name}"))
                .test_id(&test_ids::sync(format!("pull-{name}")))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .on_press(move || {
                    let root = root_pull.clone();
                    pull_handle.update(move |s| s.dispatch_pull("Pull", root, None));
                })
                .child(
                    text("Pull")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        )
        .child(
            view()
                .role(AccessibilityRole::Button)
                .label(format!("Push {name}"))
                .test_id(&test_ids::sync(format!("push-{name}")))
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .bg(t.secondary)
                .on_press(move || {
                    let root = root_push.clone();
                    push_handle.update(move |s| s.open_push_for_roots(&[root]));
                })
                .child(
                    text("Push…")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        ))
}

/// `short` for a commit id, the one spelling the commit rows share.
pub fn commit_short(id: &CommitId) -> String {
    crate::screens::short(id)
}
