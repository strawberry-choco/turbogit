//! VCS Operations Popup (M1) + Command Palette / "Find Action" (Epic F5).
//!
//! `Alt+\`` opens the context-sensitive VCS operations list. `Ctrl+Shift+A`
//! opens the command palette: a fuzzy-searchable list of every action, the
//! IntelliJ "Find Action" hallmark.

use crate::core::partial::{self, HunkSelection, Selection};
use crate::model::ChangeStatus;
use crate::root_caches::Affected;
use crate::state::{AppState, Dialog, Tab, Toast};
use egui::Ui;

/// Every globally-invokable action, reused by both the VCS popup and the
/// command palette — and by the shell's Git menu (issue #9).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Fetch,
    Pull,
    Push,
    Branches,
    NewBranch,
    Merge,
    Rebase,
    Stash,
    Shelve,
    Tag,
    CommitTab,
    Settings,
    Clone,
    // Shell-navigation extras (issue #22 / ADR-0011): palette-only. The VCS
    // operations popup keeps its exact pre-existing action set.
    GoToLog,
    OpenWelcome,
    ToggleToolbar,
    // Partial-staging verbs (spec R2): palette-only, operating on the diff
    // viewer's hovered hunk. The VCS operations popup set stays frozen.
    StageHunk,
    UnstageHunk,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Refresh => "Refresh",
            Action::Fetch => "Fetch",
            Action::Pull => "Pull",
            Action::Push => "Push…",
            Action::Branches => "Branches…",
            Action::NewBranch => "New Branch…",
            Action::Merge => "Merge…",
            Action::Rebase => "Rebase…",
            Action::Stash => "Stash…",
            Action::Shelve => "Shelve…",
            Action::Tag => "Tag…",
            Action::CommitTab => "Go to Commit",
            Action::Settings => "Settings…",
            Action::Clone => "Clone…",
            Action::GoToLog => "Go to Log",
            Action::OpenWelcome => "Open Welcome",
            Action::ToggleToolbar => "Toggle Toolbar",
            Action::StageHunk => "Stage Hunk",
            Action::UnstageHunk => "Unstage Hunk",
        }
    }

    /// The VCS operations popup's action set: exactly the pre-existing list
    /// (issue #22 keeps it unchanged while the palette grows).
    pub fn all() -> &'static [Action] {
        &[
            Action::Refresh,
            Action::Fetch,
            Action::Pull,
            Action::Push,
            Action::Branches,
            Action::NewBranch,
            Action::Merge,
            Action::Rebase,
            Action::Stash,
            Action::Shelve,
            Action::Tag,
            Action::CommitTab,
            Action::Settings,
            Action::Clone,
        ]
    }

    /// The command palette's action set (ADR-0011): every existing entry plus
    /// the shell-navigation actions the new shell makes meaningful.
    pub fn palette_actions() -> &'static [Action] {
        &[
            Action::Refresh,
            Action::Fetch,
            Action::Pull,
            Action::Push,
            Action::Branches,
            Action::NewBranch,
            Action::Merge,
            Action::Rebase,
            Action::Stash,
            Action::Shelve,
            Action::Tag,
            Action::CommitTab,
            Action::Settings,
            Action::Clone,
            Action::GoToLog,
            Action::OpenWelcome,
            Action::ToggleToolbar,
            Action::StageHunk,
            Action::UnstageHunk,
        ]
    }
}

pub fn run_action(state: &mut AppState, action: Action) {
    let root = state.selected_path();
    match action {
        // Manual refresh (decision 8): the full scoped refresh — drops every
        // cache entry (decorations and path history included) and rescans.
        Action::Refresh => state.refresh(Affected::All),
        Action::Fetch => {
            let r = root.clone();
            state.run_git(
                "Fetch".into(),
                Affected::from_optional_root(r.as_deref()),
                move |v| {
                    if let Some(r) = &r {
                        v.fetch(r, None)
                    } else {
                        Ok(())
                    }
                },
            );
        }
        Action::Pull => {
            let r = root.clone();
            let rebase = state.settings.update_method == crate::model::UpdateMethod::Rebase;
            state.run_git(
                "Pull".into(),
                Affected::from_optional_root(r.as_deref()),
                move |v| {
                    if let Some(r) = &r {
                        v.pull(r, rebase)
                    } else {
                        Ok(())
                    }
                },
            );
        }
        Action::Push => state.ui.dialog = Some(Dialog::Push),
        Action::Branches => state.ui.branches_popup = true,
        Action::NewBranch => state.ui.dialog = Some(Dialog::NewBranch),
        Action::Merge => state.ui.dialog = Some(Dialog::Merge),
        Action::Rebase => state.ui.dialog = Some(Dialog::Rebase),
        Action::Stash => state.ui.dialog = Some(Dialog::Stash),
        Action::Shelve => state.ui.dialog = Some(Dialog::Shelve),
        Action::Tag => state.ui.dialog = Some(Dialog::Tag),
        Action::CommitTab => state.ui.tab = Tab::Commit,
        Action::Settings => state.ui.settings_open = true,
        Action::Clone => {
            // The clone flow lives on the Welcome screen (ticket #10).
            state.ui.toast = Some(Toast::info("Clone opens from the Welcome screen."));
        }
        // Shell navigation (issue #22 / ADR-0011).
        Action::GoToLog => {
            state.ui.welcome_visible = false;
            state.ui.tab = Tab::Log;
        }
        Action::OpenWelcome => state.ui.welcome_visible = true,
        Action::ToggleToolbar => state.ui.show_toolbar = !state.ui.show_toolbar,
        // Partial-staging verbs (spec R2): stage/unstage the whole hunk the
        // pointer last rested on in the diff viewer. Every input — root,
        // preview target, hovered hunk, cached diff text — must exist, or the
        // verb is a silent no-op; conflicted files are blocked by the core op.
        Action::StageHunk | Action::UnstageHunk => {
            let stage = action == Action::StageHunk;
            let Some(root) = state.selected_path() else {
                return;
            };
            let Some(path) = state.ui.preview_change.clone() else {
                return;
            };
            let Some(hunk) = state.ui.hovered_hunk else {
                return;
            };
            let Some(diff_text) = crate::ui::diff::cached_preview_diff(state, &path) else {
                return;
            };
            let status = crate::ui::diff::preview_status(state, Some(&path));
            let selection = Selection {
                hunks: [(hunk, HunkSelection::Whole)].into_iter().collect(),
            };
            let label = if stage { "Stage hunk" } else { "Unstage hunk" };
            // Untracked files stage via intent-to-add + forward apply using
            // their repo-relative path; unstage keeps the plain reverse-apply.
            let untracked = stage && status == ChangeStatus::Unversioned;
            // Post-op the viewer settles on the remaining unstaged changes
            // (spec R2 story 8).
            crate::ui::diff::settle_preview_on_unstaged(state);
            // Story 9: completion decides exclusions/focus for this file.
            state.ui.pending_granular = Some(path.clone());
            state.run_git(
                label.to_owned(),
                Affected::from_optional_root(Some(root.as_path())),
                move |v| {
                    if untracked {
                        partial::stage_untracked_selection(
                            v,
                            &root,
                            std::slice::from_ref(&path),
                            &diff_text,
                            &selection,
                            status,
                        )
                    } else if stage {
                        partial::stage_selection(v, &root, &diff_text, &selection, status)
                    } else {
                        partial::unstage_selection(v, &root, &diff_text, &selection, status)
                    }
                },
            );
        }
    }
}

pub fn vcs_operations(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.vcs_popup {
        return;
    }
    let ctx = ui.ctx().clone();
    let mut open = state.ui.vcs_popup;
    // Popup chrome (issue #22, spec §10): SURFACE fill, LINE border, radius 8
    // — mapped centrally via `theme::configure_style`; fixed-size like the
    // palette. The action list itself is untouched.
    egui::Window::new("VCS Operations")
        .open(&mut open)
        .resizable(false)
        .show(&ctx, |ui| {
            for a in Action::all() {
                if ui.button(a.label()).clicked() {
                    run_action(state, *a);
                }
            }
        });
    if !open {
        state.ui.vcs_popup = false;
    }
}

/// Command palette (Epic F5 / "Find Action"). Searchable, keyboard-friendly.
pub fn command_palette(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.command_palette {
        return;
    }
    let ctx = ui.ctx().clone();

    // Compact rows so the full action set fits unscrolled (issue #22: every
    // entry must stay listed and reachable, ADR-0011).
    let row_h = 24.0;
    let q = state.ui.command_query.to_lowercase();
    let match_count = Action::palette_actions()
        .iter()
        .filter(|a| q.is_empty() || a.label().to_lowercase().contains(&q))
        .count();
    let view_h = ctx.input(|i| i.viewport_rect().height());
    let list_h = ((match_count as f32 + 1.0) * row_h).min((view_h - 220.0).max(120.0));

    let mut open = true;
    egui::Window::new("Find Action")
        .open(&mut open)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
        .default_width(420.0)
        // Floor the outer window height so the Resize state's remembered
        // size can never clamp the list below the full action set (the
        // Branches-popup lesson, applied at the Window level).
        .min_height(105.0 + list_h)
        .resizable(false)
        .show(&ctx, |ui| {
            ui.text_edit_singleline(&mut state.ui.command_query)
                .request_focus();
            ui.separator();
            let matches: Vec<Action> = Action::palette_actions()
                .iter()
                .copied()
                .filter(|a| q.is_empty() || a.label().to_lowercase().contains(&q))
                .collect();
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.spacing_mut().button_padding = egui::vec2(8.0, 2.0);
            ui.spacing_mut().interact_size.y = 16.0;
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), list_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(list_h)
                        .show(ui, |ui| {
                            for &a in &matches {
                                if ui.selectable_label(false, a.label()).clicked() {
                                    run_action(state, a);
                                    state.ui.command_palette = false;
                                }
                            }
                            if matches.is_empty() {
                                ui.label("No matching actions.");
                            }
                        });
                },
            );
        });
    if !open {
        state.ui.command_palette = false;
    }
}
