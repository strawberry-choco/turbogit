//! Settings modal (issue #16, upgraded in issue #26 per screen 11): a
//! ~768px category-list dialog opened ONLY from the toolbar gear — the
//! tab-strip Settings page was deleted outright (spec §9.1 correction).
//!
//! Editing model: opening snapshots the loaded [`VcsSettings`] into
//! [`UiState::settings_draft`]; rows edit the draft only. The footer follows
//! IDE semantics — **Restore defaults** (screen 11) resets the visible
//! category's fields to their defaults, **Cancel** (and the window X)
//! discards the draft, **Apply** persists via `persistence.rs`, rebuilds the
//! engine behind the seam (ADR-0001) and stays open. Apply is disabled until
//! the draft differs from the loaded settings.
//!
//! Categories (issue #26): General, Git, Update Method, Multi-Root,
//! Protected Branches, Appearance, Advanced — every backed `VcsSettings`
//! field lives on exactly one page; unbacked placeholders render
//! visible-but-disabled with tooltips and never persist.

use egui::{
    Align, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, RichText, Sense, TextEdit, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::widgets;
use crate::theme::Palette;
use turbogit_app::state::{AppState, SettingsCategory, Toast};
use turbogit_domain::model::{
    CleanTreeMethod, DateFormat, GitBackend, IncomingCheckInterval, UpdateMethod, VcsSettings,
};

/// Spec §8.8: large modal, ~768px wide.
const MODAL_WIDTH: f32 = 768.0;
/// Left category list width (spec §8.8: 176px).
const CATEGORY_WIDTH: f32 = 176.0;
/// Right pane width: whatever the modal body offers beyond the category
/// column and the 1px divider. Pinned explicitly — an unconstrained
/// ScrollArea inside a Window feeds back on itself and grows forever.
const PANEL_WIDTH: f32 = MODAL_WIDTH - CATEGORY_WIDTH - 40.0;
/// Fixed body height so both columns align and the window keeps its size.
const PANEL_HEIGHT: f32 = 420.0;
const ROW_HEIGHT: f32 = 26.0;
const INPUT_WIDTH: f32 = 300.0;

const CATEGORIES: [SettingsCategory; 7] = [
    SettingsCategory::General,
    SettingsCategory::Git,
    SettingsCategory::UpdateMethod,
    SettingsCategory::MultiRoot,
    SettingsCategory::ProtectedBranches,
    SettingsCategory::Appearance,
    SettingsCategory::Advanced,
];

/// Render the modal when open; a closed window is a no-op.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.settings_open {
        return;
    }
    // Snapshot the loaded values once per open: the dirty reference every
    // footer decision compares against. A fresh open (the draft only exists
    // while the modal does) also lands back on General.
    if state.ui.settings_draft.is_none() {
        state.ui.settings_draft = Some(state.settings.clone());
        state.ui.settings_category = SettingsCategory::General;
    }

    // Sense the real viewport height from the shell-level clip rect —
    // `RawInput::screen_rect` is not populated by headless harnesses
    // (issue #23).
    let view_h = ui.clip_rect().height();
    let panel_h = PANEL_HEIGHT.min((view_h - 170.0).max(240.0));

    let ctx = ui.ctx().clone();
    let mut open = state.ui.settings_open;
    egui::Window::new("Settings")
        .open(&mut open)
        .default_width(MODAL_WIDTH)
        .resizable(false)
        .show(&ctx, |ui| body(ui, state, panel_h));
    // Two close paths meet here: the window X / Esc flip `open`, the footer
    // Cancel cleared `settings_open` from inside the body. Either discards.
    let body_cancelled = !state.ui.settings_open;
    state.ui.settings_open = open && !body_cancelled;
    if !state.ui.settings_open {
        state.ui.settings_draft = None;
    }
}

// --- Body ---------------------------------------------------------------------

fn body(ui: &mut Ui, state: &mut AppState, panel_h: f32) {
    // The spec's 420px body yields to short viewports so the footer stays on
    // screen instead of being clipped away (issue #23).
    ui.horizontal(|ui| {
        category_list(ui, state, panel_h);
        paint_vertical_line(ui, panel_h);
        settings_panel(ui, state, panel_h);
    });
    ui.add_space(10.0);
    footer(ui, state);
}

// --- Left column: category list -------------------------------------------------

fn category_list(ui: &mut Ui, state: &mut AppState, panel_h: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(CATEGORY_WIDTH, panel_h), Sense::hover());
    let mut col = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(Align::Min)),
    );
    col.add_space(4.0);

    for category in CATEGORIES {
        let clicked = category_row(
            &mut col,
            category.label(),
            state.ui.settings_category == category,
            true,
        );
        if clicked {
            state.ui.settings_category = category;
        }
    }
}

/// One category row. Returns `true` only when an enabled row was clicked.
fn category_row(ui: &mut Ui, label: &str, selected: bool, enabled: bool) -> bool {
    let width = ui.available_width();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), sense);

    let hovered = enabled && response.hovered();
    let fill = widgets::row_fill(selected, hovered);
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    }
    let ink = if selected {
        Palette::BRAND_INK
    } else if !enabled {
        Palette::INK_3
    } else if hovered {
        Palette::INK
    } else {
        Palette::INK_2
    };
    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::new(13.0, FontFamily::Proportional),
        ink,
    );

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    widgets::focus_ring(ui, &response);

    if !enabled {
        response.on_disabled_hover_text("No configurable settings here yet");
        return false;
    }
    response.clicked()
}

// --- Right pane: setting rows ---------------------------------------------------

fn settings_panel(ui: &mut Ui, state: &mut AppState, panel_h: f32) {
    let category = state.ui.settings_category;
    if category == SettingsCategory::Git {
        refresh_version_badge(state);
    }
    // The badge is tiny; clone it out so the draft can stay mutably
    // borrowed while the page renders.
    let version_badge = state.ui.git_version_badge.clone();
    // Fixed-size pane: the ScrollArea scrolls, it never resizes the window.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(PANEL_WIDTH, panel_h), Sense::hover());
    let mut pane = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(Align::Min)),
    );
    if category == SettingsCategory::ProtectedBranches {
        // The chips page also edits the new-pattern input on UiState, so it
        // reaches the draft through `state` instead of a pre-taken `&mut`.
        egui::ScrollArea::vertical()
            .max_height(panel_h)
            .show(&mut pane, |ui| {
                page_title(ui, category.label());
                protected_branches_page(ui, state);
            });
        return;
    }
    let Some(draft) = state.ui.settings_draft.as_mut() else {
        return;
    };
    egui::ScrollArea::vertical()
        .max_height(panel_h)
        .show(&mut pane, |ui| {
            page_title(ui, category.label());
            match category {
                SettingsCategory::General => general_page(ui, draft),
                SettingsCategory::Git => git_page(ui, draft, version_badge.as_ref()),
                SettingsCategory::UpdateMethod => update_method_page(ui, draft),
                SettingsCategory::MultiRoot => multi_root_page(ui, draft),
                SettingsCategory::Appearance => appearance_page(ui, draft),
                SettingsCategory::Advanced => advanced_page(ui, draft),
                SettingsCategory::ProtectedBranches => unreachable!("handled above"),
            }
        });
}

/// Run the live `git --version` check behind the draft executable (issue
/// #26) whenever the draft path changed since the last check.
fn refresh_version_badge(state: &mut AppState) {
    let Some(draft) = state.ui.settings_draft.as_ref() else {
        return;
    };
    let path = draft.git_executable.clone();
    if state.ui.git_version_checked_path.as_deref() == Some(path.as_str()) {
        return;
    }
    let res = turbogit_app::resolve_git_version(draft).map_err(|e| e.to_string());
    state.ui.git_version_badge = Some(res);
    state.ui.git_version_checked_path = Some(path);
}

fn page_title(ui: &mut Ui, title: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(title).strong().size(16.0));
    ui.add_space(6.0);
}

/// General: commit-surface behavior. Screen 11 keeps this page light.
fn general_page(ui: &mut Ui, s: &mut VcsSettings) {
    ui.checkbox(
        &mut s.staging_area,
        "Use staging area instead of classic commit",
    );
    ui.checkbox(
        &mut s.restore_workspace,
        "Restore workspace context on branch switch",
    );
}

/// Git (screen 11): engine backend, the git executable, remote management.
fn git_page(ui: &mut Ui, s: &mut VcsSettings, version_badge: Option<&Result<String, String>>) {
    setting_row(
        ui,
        "Git backend",
        "Auto picks libgit2 for reads and the CLI for anything libgit2 cannot do.",
        |ui| {
            let options = ["CLI", "libgit2", "Auto"];
            let selected = match s.backend {
                GitBackend::Cli => 0,
                GitBackend::Libgit2 => 1,
                GitBackend::Auto => 2,
            };
            if let Some(ix) = widgets::segmented_control(ui, &options, selected) {
                s.backend = match ix {
                    0 => GitBackend::Cli,
                    1 => GitBackend::Libgit2,
                    _ => GitBackend::Auto,
                };
            }
        },
    );

    ui.add_space(6.0);
    setting_row(
        ui,
        "Git executable",
        "Leave empty to use the git found on PATH.",
        |ui| {
            ui.add_space(8.0);
            if widgets::ghost_button(ui, None, "Browse").clicked() {
                // Native file-picker seam, mirroring `AppState::dir_picker`;
                // not wired in v1 — the path stays hand-editable.
            }
            match version_badge {
                Some(Ok(version)) => {
                    widgets::badge(ui, &format!("{version} ✓"), widgets::BadgeKind::Added);
                }
                Some(Err(reason)) => {
                    widgets::badge(ui, "✗", widgets::BadgeKind::Deleted).on_hover_text(reason);
                }
                None => {}
            }
            labeled_text_input(ui, "Git executable", &mut s.git_executable);
        },
    );

    ui.add_space(6.0);
    let remotes = ui
        .scope(|ui| {
            ui.disable();
            widgets::ghost_button(ui, None, "Manage Remotes")
        })
        .inner;
    remotes.on_disabled_hover_text("The remote manager is not available yet");
}

/// Update Method: how incoming commits integrate and how a dirty tree is
/// swept first (screen 11).
fn update_method_page(ui: &mut Ui, s: &mut VcsSettings) {
    setting_row(
        ui,
        "Update method",
        "How incoming commits are integrated when you run Update.",
        |ui| {
            egui::ComboBox::from_id_salt("settings_update_method")
                .selected_text(update_method_label(s.update_method))
                .width(140.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.update_method, UpdateMethod::Merge, "Merge");
                    ui.selectable_value(&mut s.update_method, UpdateMethod::Rebase, "Rebase");
                });
        },
    );

    ui.add_space(6.0);
    setting_row(
        ui,
        "Clean working tree",
        "Sweep uncommitted changes before an update runs.",
        |ui| {
            egui::ComboBox::from_id_salt("settings_clean_tree_method")
                .selected_text(clean_tree_label(s.clean_tree_method))
                .width(140.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.clean_tree_method, CleanTreeMethod::Stash, "Stash");
                    ui.selectable_value(
                        &mut s.clean_tree_method,
                        CleanTreeMethod::Shelve,
                        "Shelve",
                    );
                });
        },
    );

    ui.add_space(6.0);
    setting_row(
        ui,
        "Incoming check",
        "Poll remotes in the background and badge incoming commits in the workspace tree.",
        |ui| {
            let cb = ui.checkbox(&mut s.incoming_poll, "");
            cb.widget_info(|| {
                WidgetInfo::labeled(WidgetType::Checkbox, true, "Check for incoming commits")
            });
        },
    );

    ui.add_space(6.0);
    setting_row(
        ui,
        "Check interval",
        "How often the background check runs.",
        |ui| {
            ui.add_enabled_ui(s.incoming_poll, |ui| {
                egui::ComboBox::from_id_salt("settings_incoming_interval")
                    .selected_text(incoming_interval_label(s.incoming_interval))
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut s.incoming_interval,
                            IncomingCheckInterval::Min15,
                            "Every 15 minutes",
                        );
                        ui.selectable_value(
                            &mut s.incoming_interval,
                            IncomingCheckInterval::Min30,
                            "Every 30 minutes",
                        );
                        ui.selectable_value(
                            &mut s.incoming_interval,
                            IncomingCheckInterval::Min60,
                            "Every hour",
                        );
                    });
            });
        },
    );
}

/// Multi-Root (issue #26): cross-root policy; per-root overrides land later.
fn multi_root_page(ui: &mut Ui, s: &mut VcsSettings) {
    ui.checkbox(
        &mut s.synchronous_branches,
        "Sync branch operations across roots",
    );
}

/// Protected Branches (screen 11): the patterns that gate cascading push,
/// delete, and force operations against matching branches, rendered as
/// removable chips with an add control. Edits land on the draft only; Apply
/// makes the edited set the one every gate reads.
fn protected_branches_page(ui: &mut Ui, state: &mut AppState) {
    ui.label(RichText::new("Protected branch patterns").strong());
    ui.label(
        RichText::new(
            "Cascading push, delete, and force operations refuse to touch matching branches.",
        )
        .small()
        .weak(),
    );
    ui.add_space(4.0);

    let Some(draft) = state.ui.settings_draft.as_mut() else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        for pattern in draft.protected_branch_patterns.clone() {
            pattern_chip(ui, &pattern, draft);
        }
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let input = ui.add(
            TextEdit::singleline(&mut state.ui.settings_new_pattern)
                .desired_width(160.0)
                .hint_text("release/*"),
        );
        input.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "New pattern"));
        if widgets::ghost_button(ui, None, "+ Add pattern").clicked() {
            let p = state.ui.settings_new_pattern.trim().to_string();
            if !p.is_empty() && !draft.protected_branch_patterns.contains(&p) {
                draft.protected_branch_patterns.push(p);
            }
            state.ui.settings_new_pattern.clear();
        }
    });
}

/// One pattern pill with its remove control (`Remove <pattern>` so the
/// control is queryable per chip).
fn pattern_chip(ui: &mut Ui, pattern: &str, draft: &mut VcsSettings) {
    ui.scope(|ui| {
        let frame = egui::Frame::new()
            .fill(Palette::SURFACE_2)
            .corner_radius(CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(6, 2));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(pattern).monospace().size(12.0));
                let remove = ui.add(egui::Button::new(RichText::new("×").size(11.0)).frame(false));
                remove.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, format!("Remove {pattern}"))
                });
                if remove.clicked() {
                    draft.protected_branch_patterns.retain(|p| p != pattern);
                }
            });
        });
    });
}

/// Appearance: log presentation (screen 11's date format + gutter markers).
fn appearance_page(ui: &mut Ui, s: &mut VcsSettings) {
    setting_row(
        ui,
        "Date format",
        "Applies to the log, commit details, and every activity timestamp.",
        |ui| {
            egui::ComboBox::from_id_salt("settings_date_format")
                .selected_text(date_format_label(s.date_format))
                .width(140.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.date_format, DateFormat::Relative, "Relative");
                    ui.selectable_value(&mut s.date_format, DateFormat::Absolute, "Absolute");
                    ui.selectable_value(&mut s.date_format, DateFormat::Iso, "ISO");
                });
        },
    );

    ui.add_space(6.0);
    ui.checkbox(
        &mut s.gutter_markers,
        "Highlight modified lines in the gutter",
    );
}

/// Advanced: commit-policy warnings and rarely-touched knobs.
fn advanced_page(ui: &mut Ui, s: &mut VcsSettings) {
    ui.checkbox(&mut s.warn_crlf, "Warn before committing CRLF");
    ui.checkbox(
        &mut s.warn_detached,
        "Warn when committing in detached HEAD or mid-rebase",
    );
    ui.checkbox(&mut s.no_commit_hooks, "Do not run git commit hooks");

    ui.add_space(6.0);
    setting_row(
        ui,
        "Commit message template",
        "Path to a file whose content pre-fills new commit messages.",
        |ui| {
            labeled_text_input(ui, "Commit message template", &mut s.commit_template);
        },
    );

    ui.add_space(6.0);
    ui.label(RichText::new("CRLF conversion").weak());
    let tooltip = "Not backed by persisted settings yet";
    for (checked, label) in [
        (true, "Convert to LF on commit"),
        (false, "Convert to CRLF on checkout"),
        (false, "No conversion"),
    ] {
        let resp = ui.add_enabled(false, egui::RadioButton::new(checked, label));
        resp.on_disabled_hover_text(tooltip);
    }

    ui.add_space(6.0);
    ui.label(RichText::new("Commit checks").weak());
    for label in ["Run git commit hooks", "Sign-off commits"] {
        let mut off = false;
        let resp = ui.add_enabled(false, egui::Checkbox::new(&mut off, label));
        resp.on_disabled_hover_text(tooltip);
    }
}

fn update_method_label(m: UpdateMethod) -> &'static str {
    match m {
        UpdateMethod::Merge => "Merge",
        UpdateMethod::Rebase => "Rebase",
    }
}

fn clean_tree_label(m: CleanTreeMethod) -> &'static str {
    match m {
        CleanTreeMethod::Stash => "Stash",
        CleanTreeMethod::Shelve => "Shelve",
    }
}

fn incoming_interval_label(m: IncomingCheckInterval) -> &'static str {
    match m {
        IncomingCheckInterval::Min15 => "Every 15 minutes",
        IncomingCheckInterval::Min30 => "Every 30 minutes",
        IncomingCheckInterval::Min60 => "Every hour",
    }
}

fn date_format_label(f: DateFormat) -> &'static str {
    match f {
        DateFormat::Relative => "Relative",
        DateFormat::Absolute => "Absolute",
        DateFormat::Iso => "ISO",
    }
}

/// Row chrome: bold-ish label with optional description on the left, control
/// right-aligned (spec §8.8 row anatomy).
fn setting_row(ui: &mut Ui, label: &str, description: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(label);
            ui.label(RichText::new(description).small().weak());
        });
        ui.with_layout(egui::Layout::right_to_left(Align::Center), control);
    });
}

/// Single-line input made queryable by an explicit accessible label (the
/// repo-wide kittest pattern, cf. `widgets::input_frame`).
fn labeled_text_input(ui: &mut Ui, label: &str, buf: &mut String) -> egui::Response {
    let resp = ui.add(TextEdit::singleline(buf).desired_width(INPUT_WIDTH));
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, label));
    resp
}

fn paint_vertical_line(ui: &mut Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::LINE);
}

// --- Footer ---------------------------------------------------------------------

fn footer(ui: &mut Ui, state: &mut AppState) {
    let dirty = state
        .ui
        .settings_draft
        .as_ref()
        .is_some_and(|d| *d != state.settings);
    widgets::dialog_footer(ui, |ui| {
        // right-to-left layout: the first button renders rightmost.
        let applied = ui
            .scope(|ui| {
                if !dirty {
                    ui.disable();
                }
                widgets::compact_button(ui, "Apply")
            })
            .inner;
        if applied.clicked() {
            apply_settings(state);
        }
        if widgets::compact_button(ui, "Cancel").clicked() {
            // `show` reconciles this with the window X and drops the draft.
            state.ui.settings_open = false;
        }
        // Screen 11: Restore defaults sits on the footer's left and resets
        // the *visible category* only — edits elsewhere survive.
        ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| {
            if widgets::ghost_button(ui, None, "Restore defaults").clicked() {
                restore_category_defaults(state);
            }
        });
    });
}

/// Reset the visible category's fields in the draft to `VcsSettings::default`
/// values (issue #26, screen 11). Never persists by itself; Apply does.
fn restore_category_defaults(state: &mut AppState) {
    let Some(draft) = state.ui.settings_draft.as_mut() else {
        return;
    };
    let d = VcsSettings::default();
    match state.ui.settings_category {
        SettingsCategory::General => {
            draft.staging_area = d.staging_area;
            draft.restore_workspace = d.restore_workspace;
        }
        SettingsCategory::Git => {
            draft.backend = d.backend;
            draft.git_executable = d.git_executable.clone();
        }
        SettingsCategory::UpdateMethod => {
            draft.update_method = d.update_method;
            draft.clean_tree_method = d.clean_tree_method;
            draft.incoming_poll = d.incoming_poll;
            draft.incoming_interval = d.incoming_interval;
        }
        SettingsCategory::MultiRoot => {
            draft.synchronous_branches = d.synchronous_branches;
        }
        SettingsCategory::ProtectedBranches => {
            draft.protected_branch_patterns = d.protected_branch_patterns.clone();
        }
        SettingsCategory::Appearance => {
            draft.date_format = d.date_format;
            draft.gutter_markers = d.gutter_markers;
        }
        SettingsCategory::Advanced => {
            draft.warn_crlf = d.warn_crlf;
            draft.warn_detached = d.warn_detached;
            draft.no_commit_hooks = d.no_commit_hooks;
            draft.commit_template = d.commit_template.clone();
        }
    }
}

/// Persist the draft into live state + `.turbogit/state.ron`, rebuild the
/// engine behind the seam (ADR-0001: a changed git binary or backend applies
/// live), and keep the modal open with a clean draft.
fn apply_settings(state: &mut AppState) {
    let Some(draft) = state.ui.settings_draft.clone() else {
        return;
    };
    state.settings = draft;
    let _ = turbogit_app::persistence::save_settings(&state.project_dir, &state.settings);
    state.rebuild_executor();
    state.persist_ui();
    state.ui.toast = Some(Toast::success("Settings saved"));
}
