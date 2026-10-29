use egui::{
    Align, Color32, Frame, Key, Layout, Margin, Panel, Pos2, Rect, RichText, Ui, UiBuilder,
    WidgetInfo, WidgetType,
};

use super::components;
use super::icons::Icon;
use super::widgets;
use crate::theme::Palette;
use turbogit_app::root_caches::{Affected, LogRead};
use turbogit_app::state::{AppState, Dialog, Tab};
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Minimum work-area width at which the workspace sidebar renders
/// (issue #05): below it the log's minimum pane sizes cannot hold.
pub const MIN_SIDEBAR_WINDOW_WIDTH: f32 = 1000.0;

pub fn render(ui: &mut Ui, state: &mut AppState) {
    handle_shortcuts(ui, state);

    if !state.show_welcome() {
        ensure_worktree_data(state);
        if state.ui.tab == Tab::Worktrees {
            state.ensure_worktree_probes();
        }
    }

    let status_hint = widgets::take_status_hint(ui.ctx());
    if state.ui.show_status_bar {
        render_status_bar(ui, state, status_hint.as_deref());
    }

    let shell_frame = egui::Frame::central_panel(ui.style()).inner_margin(Margin::ZERO);
    egui::CentralPanel::default()
        .frame(shell_frame)
        .show(ui, |ui| {
            if state.show_welcome() {
                super::welcome::show(ui, state);
            } else {
                let body = ui.available_rect_before_wrap();
                let activity_h = if state.ui.activity.expanded {
                    super::activity_panel::ACTIVITY_HEIGHT
                } else {
                    super::activity_panel::ACTIVITY_COLLAPSED_HEIGHT
                };
                let activity_rect =
                    Rect::from_min_max(Pos2::new(body.min.x, body.max.y - activity_h), body.max);
                let work_rect =
                    Rect::from_min_max(body.min, Pos2::new(body.max.x, activity_rect.min.y));
                let sidebar_visible = work_rect.width() >= MIN_SIDEBAR_WINDOW_WIDTH;
                let sidebar_w = if sidebar_visible {
                    super::sidebar::SIDEBAR_WIDTH
                } else {
                    0.0
                };
                let sidebar_rect = Rect::from_min_max(
                    work_rect.min,
                    Pos2::new(work_rect.min.x + sidebar_w, work_rect.max.y),
                );
                let right_rect = Rect::from_min_max(
                    Pos2::new(sidebar_rect.max.x, work_rect.min.y),
                    work_rect.max,
                );

                let mut sidebar_ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(sidebar_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                if sidebar_visible {
                    super::sidebar::show(&mut sidebar_ui, state);
                }
                ui.advance_cursor_after_rect(sidebar_rect);
                let mut column_ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(right_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                let tabs = shell_tabs(state);
                let active = state.ui.tab;
                widgets::tab_view(
                    &mut column_ui,
                    state,
                    &tabs,
                    active,
                    switch_tab,
                    |ui, state| {
                        let settings = widgets::icon_button(ui, Icon::SETTINGS);
                        settings.widget_info(|| {
                            WidgetInfo::labeled(WidgetType::Button, true, "Open settings")
                        });
                        if settings.clicked() {
                            state.ui.settings_open = true;
                        }
                        let shortcuts = widgets::icon_button(ui, Icon::KEYBOARD);
                        shortcuts.widget_info(|| {
                            WidgetInfo::labeled(WidgetType::Button, true, "Show shortcuts")
                        });
                        if shortcuts.clicked() {
                            state.ui.shortcuts_open = true;
                        }
                    },
                    show_tool_window,
                );
                ui.advance_cursor_after_rect(right_rect);
                let mut activity_ui = ui.new_child(
                    UiBuilder::new()
                        .max_rect(activity_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                super::activity_panel::show(&mut activity_ui, state);
                ui.advance_cursor_after_rect(activity_rect);
            }
        });

    // While an async op or a keyed read is in flight, keep frames coming so its
    // completion (OpCompleted → refresh → reload, or a settled cache value)
    // lands without waiting for unrelated input, the headless harness relies
    // on the same signal `app.rs` gets from drain_events in production
    // (spec R2 story 8). `busy` is dispatch's; `read_pending` is the reads',
    // and it says only that *a* surface is working, not which.
    if state.ui.busy || state.read_pending() {
        ui.ctx().request_repaint();
    }
}

fn handle_shortcuts(ui: &mut Ui, state: &mut AppState) {
    let dialog_open = state.ui.dialog.is_some()
        || state.ui.settings_open
        || state.ui.shortcuts_open
        || state.ui.confirm.is_some()
        || state.ui.conflict_open.is_some()
        || state.ui.bulk_op.is_some()
        || state.ui.bulk_run.is_some();

    if !dialog_open && ui.input(|i| i.key_pressed(Key::Backtick) && i.modifiers.alt) {
        state.ui.vcs_popup = !state.ui.vcs_popup;
    }

    let popup_open = state.ui.vcs_popup
        || state.ui.command_palette
        || state.ui.branches_popup
        || state.ui.branches_tree.context_menu.is_some();
    if dialog_open {
        return;
    }

    let ks = ui.input(|i| Shortcut {
        commit: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(Key::K),
        push: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::K),
        refresh: i.modifiers.ctrl && i.key_pressed(Key::T),
        find: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::A),
    });
    if ks.commit {
        switch_tab(state, Tab::Commit);
    }
    if ks.push {
        state.ui.dialog = Some(Dialog::Push);
    }
    if ks.refresh {
        state.refresh(Affected::All);
    }
    if ks.find {
        state.ui.command_palette = true;
        state.ui.command_query.clear();
    }

    if popup_open {
        return;
    }

    let slash = ui.input(|i| i.key_pressed(Key::Slash) && !i.modifiers.any());
    if slash
        && state.ui.tab == Tab::Commit
        && !state.show_welcome()
        && ui.ctx().memory(|m| m.focused().is_none())
    {
        state.ui.focus_file_filter = true;
    }

    let (f7_next, f7_prev) = ui.input(|i| {
        (
            i.key_pressed(Key::F7) && !i.modifiers.shift,
            i.key_pressed(Key::F7) && i.modifiers.shift,
        )
    });
    if f7_next {
        apply_hunk_nav(state, super::hunk_nav::Dir::Next);
    } else if f7_prev {
        apply_hunk_nav(state, super::hunk_nav::Dir::Prev);
    }

    let diff_keys = ui.input(|i| {
        (
            i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::D),
            i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::Tab),
            i.key_pressed(Key::F4) && !i.modifiers.any(),
        )
    });
    let diff_preview_open =
        state.ui.tab == Tab::Commit && !state.show_welcome() && state.ui.preview_change.is_some();
    if diff_preview_open {
        if diff_keys.0 {
            state.ui.diff_options_open = !state.ui.diff_options_open;
        }
        if diff_keys.1 {
            state.ui.diff_pane = state.ui.diff_pane.opposite();
            ui.ctx().request_repaint();
        }
        if diff_keys.2 {
            jump_to_source(state);
        }
    }

    let space = ui.input(|i| i.key_pressed(Key::Space) && !i.modifiers.any());
    if space
        && state.ui.tab == Tab::Commit
        && !state.show_welcome()
        && ui.ctx().memory(|m| m.focused().is_none())
    {
        super::commit_window::toggle_previewed_row_checkbox(state);
    }
}

fn apply_hunk_nav(state: &mut AppState, dir: super::hunk_nav::Dir) {
    if state.ui.tab != Tab::Commit || state.show_welcome() || state.ui.preview_change.is_none() {
        return;
    }
    let total = super::diff::preview_hunk_count(state);
    if total == 0 {
        return;
    }

    let files = super::commit_window::active_subtab_files(state);
    let current_file = files
        .iter()
        .position(|p| Some(p) == state.ui.preview_change.as_ref());
    let counts: Vec<usize> = files
        .iter()
        .map(|p| {
            if Some(p) == state.ui.preview_change.as_ref() {
                total
            } else {
                1
            }
        })
        .collect();
    let has_adjacent_file = current_file.is_some_and(|from| {
        super::hunk_nav::adjacent_file_with_hunks(&counts, from, dir).is_some()
    });

    let now = std::time::Instant::now();
    match super::hunk_nav::advance_hunk(
        state.ui.diff_current_hunk,
        total,
        dir,
        now,
        state.ui.hunk_nav_armed_edge,
        super::hunk_nav::EDGE_WINDOW,
        has_adjacent_file,
    ) {
        super::hunk_nav::Outcome::Moved(idx) => {
            state.ui.diff_current_hunk = idx;
            state.ui.hunk_nav_armed_edge = None;
        }
        super::hunk_nav::Outcome::Nudge => {
            let hint = match dir {
                super::hunk_nav::Dir::Next => "Press again for next file",
                super::hunk_nav::Dir::Prev => "Press again for previous file",
            };
            state.ui.toast = Some(turbogit_app::state::Toast::info(hint));
            state.ui.hunk_nav_armed_edge = Some((dir, now));
        }
        super::hunk_nav::Outcome::CrossFile => {
            state.ui.hunk_nav_armed_edge = None;
            if let Some(from) = current_file
                && let Some(target) = super::hunk_nav::adjacent_file_with_hunks(&counts, from, dir)
                && let Some(path) = files.get(target)
            {
                state.ui.preview_change = Some(path.clone());
            }
        }
    }
}

/// `F4`. Jump to Source, with the caret preserved.
///
/// The line is **derived** from the display model on the pane that holds the caret
/// (`preview_caret_line`), which is what makes the jump land where the
/// reader was looking instead of at the top of the file. Blame is this app's
/// source surface, one row per line of the file, with the commit that introduced
/// it, and it floats in its own window over the tab the reader is on, so the
/// tab stays put and the line travels in `blame_caret_line` for the window to
/// aim at.
///
/// Working-tree preview only, and so blame at `HEAD`: the pair being read here is
/// the working tree's, and the committed side is the only side with an author per
/// line. A pane with no numbered row at the caret, an insertion read as the
/// `Before` side, has no line to jump to, and the key does nothing.
fn jump_to_source(state: &mut AppState) {
    let Some((change, line)) = super::diff::preview_caret_line(state) else {
        return;
    };
    state.ui.blame = Some(turbogit_app::state::BlameTarget {
        root: change.root,
        path: change.path,
        rev: "HEAD".to_owned(),
    });
    state.ui.blame_caret_line = Some(line);
}

struct Shortcut {
    commit: bool,
    push: bool,
    refresh: bool,
    find: bool,
}
fn tab_badge_count(state: &AppState, tab: Tab) -> Option<usize> {
    let id = state.selected_root.as_ref()?;
    let n = match tab {
        Tab::Worktrees => state.caches.worktrees(id)?.len(),
        Tab::Submodules => state.caches.submodules(id)?.len(),
        _ => return None,
    };
    (n > 0).then_some(n)
}

fn ensure_worktree_data(state: &mut AppState) {
    if let Some(id) = state.selected_root.clone() {
        if state.caches.worktrees(&id).is_none() {
            state.fetch_worktrees(id.clone());
        }
        if state.caches.submodules(&id).is_none() {
            state.fetch_submodules(id);
        }
    }
}

pub(crate) fn switch_tab(state: &mut AppState, tab: Tab) {
    state.ui.tab = tab;
    if tab == Tab::Branches {
        state.ui.branches_focus_search = true;
    }
    state.save_durable_ui();
}

const SHELL_TAB_ITEMS: [widgets::TabItem<'static, Tab>; 5] = [
    widgets::TabItem {
        id: Tab::Commit,
        title: "Changes",
        icon: Some(Icon::GIT_COMMIT),
        end_icon: None,
        badge: None,
    },
    widgets::TabItem {
        id: Tab::Log,
        title: "Log",
        icon: Some(Icon::GIT_BRANCH),
        end_icon: None,
        badge: None,
    },
    widgets::TabItem {
        id: Tab::Branches,
        title: "Branches",
        icon: Some(Icon::GIT_BRANCH),
        end_icon: None,
        badge: None,
    },
    widgets::TabItem {
        id: Tab::Worktrees,
        title: "Worktrees",
        icon: Some(Icon::FOLDER),
        end_icon: None,
        badge: None,
    },
    widgets::TabItem {
        id: Tab::Submodules,
        title: "Submodules",
        icon: Some(Icon::FOLDER_GIT),
        end_icon: None,
        badge: None,
    },
];

fn shell_tabs(state: &AppState) -> [widgets::TabItem<'static, Tab>; 5] {
    let mut tabs = SHELL_TAB_ITEMS;
    for item in tabs.iter_mut() {
        item.badge = tab_badge_count(state, item.id);
    }
    tabs
}

fn render_status_bar(ui: &mut Ui, state: &mut AppState, hint: Option<&str>) {
    let agg = AggregatedStatus::compute(&state.multi.roots, &|id| state.caches.ahead_behind(id));

    Panel::bottom("status_bar")
        .exact_size(STATUS_BAR_HEIGHT)
        .frame(
            Frame::new()
                .fill(Palette::SURFACE)
                .inner_margin(Margin::symmetric(8, 0)),
        )
        .show(ui, |ui| {
            ui.style_mut().spacing.button_padding = crate::theme::DENSITY_DENSE_BUTTON;
            let band = ui.max_rect();
            // The right cluster gets a rect **measured from the panel's own
            // right edge** and sized to its own content, then lays out left to
            // right inside it. A `Layout::right_to_left` cannot do this job: a
            // `ui.label` is a *wrapping* label, so it claims the whole
            // remaining width and then anchors its galley at that rect's
            // **right** edge, which puts the text's left edge at the panel's
            // right inset and runs it off the window. At the default 1024
            // headless width "1 total" painted at x 1008..1040 inside a panel
            // whose content ends at 1008, so 32 points of the label were
            // outside the frame. This bug predates every change in this ticket;
            // it is what the "no clipping at the default size" criterion finds.
            //
            // Measuring from the right edge cannot drift: however far the left
            // cluster grows, the total's right edge stays at the inset.
            let total_text = status_total_text(&agg);
            let total_w = ui
                .painter()
                .layout_no_wrap(
                    total_text.clone(),
                    crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
                    STATUS_BAR_INK,
                )
                .size()
                .x;
            let spinner_w = if state.ui.busy {
                ui.spacing().interact_size.y + ui.spacing().item_spacing.x
            } else {
                0.0
            };
            let split = (band.max.x - total_w - spinner_w).max(band.min.x);
            let left_rect = Rect::from_min_max(band.min, Pos2::new(split, band.max.y));
            let right_rect = Rect::from_min_max(Pos2::new(split, band.min.y), band.max);
            ui.scope_builder(UiBuilder::new().max_rect(left_rect), |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if let Some(reason) = hint {
                        status_hint(ui, reason);
                    }
                    super::status_inflight::render(ui, state);
                    version_line(ui, state);
                    aggregated_status_chips(ui, &agg);
                    granular_status_chips(ui, state);
                });
            });
            ui.scope_builder(UiBuilder::new().max_rect(right_rect), |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if state.ui.busy {
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(total_text)
                            .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
                            .color(STATUS_BAR_INK),
                    );
                });
            });
            widgets::edge_rule(
                ui.painter(),
                ui.max_rect(),
                widgets::Edge::Top,
                Palette::LINE,
            );
        });
}

fn status_hint(ui: &mut Ui, reason: &str) {
    ui.label(
        RichText::new(reason)
            .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
            .color(Palette::INK),
    );
    ui.label(
        RichText::new("·")
            .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
            .color(STATUS_BAR_INK),
    );
}

/// Version / git-status line (issue #34): app version, resolved git
/// version, and the total indexed repo count, leftmost in the cluster,
/// ahead of the aggregated counters, at the same quiet ink every other status
/// line wears. It paints on every screen: the status bar is the one chrome
/// band Welcome keeps.
///
/// **The ink is `INK_2`, not `INK_3`, and that is a legality answer rather than
/// a preference.** The band this line paints on is `Palette::SURFACE`, a
/// *raised* surface, and ticket 02 narrowed `INK_3`'s legal set to the app
/// background, the content surface and the sidebar surface. On `SURFACE` it
/// measures 4.20:1, under AA, and the token layer's own instruction for a
/// caller in that position is to step *up*. `INK_2` measures 6.58:1 there. The
/// step-up is why every status-bar line shares one ink: the bar is one raised
/// band, so it gets one step.
fn version_line(ui: &mut Ui, state: &AppState) {
    ui.label(
        RichText::new(format!(
            "v{} · git {} · {} repos indexed",
            env!("CARGO_PKG_VERSION"),
            state.git_version,
            state.multi.roots.len(),
        ))
        .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
        .color(Palette::INK_2),
    );
}

const STATUS_BAR_INK: Color32 = Palette::INK_2;

fn granular_status_chips(ui: &mut Ui, state: &AppState) {
    if state.show_welcome() {
        return;
    }
    let chip = |ui: &mut Ui, color: Color32, text: String| {
        ui.label(
            RichText::new("·")
                .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
                .color(STATUS_BAR_INK),
        );
        ui.colored_label(color, text);
    };
    let scope = if state.ui.repo_selection.is_empty() {
        1
    } else {
        state.ui.repo_selection.len()
    };
    chip(
        ui,
        STATUS_BAR_INK,
        format!("{} in scope", components::counted(scope, "repo", "repos")),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CounterChip {
    Diverged,
    Unpulled,
    Dirty,
}

impl CounterChip {
    fn state(self) -> crate::theme::RepoState {
        use crate::theme::RepoState as S;
        match self {
            CounterChip::Diverged => S::Diverged,
            CounterChip::Unpulled => S::Unpulled,
            CounterChip::Dirty => S::Dirty,
        }
    }

    fn color(self) -> Color32 {
        self.state().color()
    }
}

#[derive(Default)]
struct AggregatedStatus {
    diverged: usize,
    conflicts: usize,
    unpulled: usize,
    archived: usize,
    dirty: usize,
    total: usize,
}

impl AggregatedStatus {
    fn compute(
        roots: &[turbogit_domain::model::Root],
        ahead_behind: &dyn Fn(&turbogit_domain::model::RootId) -> Option<(usize, usize)>,
    ) -> Self {
        let mut agg = AggregatedStatus {
            total: roots.len(),
            ..Default::default()
        };
        for root in roots {
            if !root.status.conflicted.is_empty() {
                agg.conflicts += 1;
                agg.dirty += 1;
            }
            if crate::theme::RepoState::is_dirty(&root.status) {
                agg.dirty += 1;
            }
            if let Some((ahead, behind)) = ahead_behind(&root.id) {
                if crate::theme::DivergenceMeaning::AnyDrift.is_diverged(ahead, behind) {
                    agg.diverged += 1;
                }
                if behind > 0 {
                    agg.unpulled += 1;
                }
            }
        }
        agg
    }
}

fn aggregated_status_chips(ui: &mut Ui, agg: &AggregatedStatus) {
    let mut first = true;
    let mut chip = |ui: &mut Ui, color: Color32, text: String| {
        if !first {
            ui.label(
                RichText::new("·")
                    .font(crate::theme::chrome_font(crate::theme::TYPE_CONTROL))
                    .color(STATUS_BAR_INK),
            );
        }
        first = false;
        ui.colored_label(color, text);
    };
    if agg.diverged > 0 {
        chip(
            ui,
            CounterChip::Diverged.color(),
            format!("{} diverged", agg.diverged),
        );
    }
    if agg.conflicts > 0 {
        chip(
            ui,
            crate::theme::RepoState::Conflict.color(),
            format!("{} conflict", agg.conflicts),
        );
    }
    if agg.unpulled > 0 {
        chip(
            ui,
            CounterChip::Unpulled.color(),
            format!("{} unpulled", agg.unpulled),
        );
    }
    if agg.archived > 0 {
        chip(
            ui,
            Palette::STATE_INFO,
            format!("{} archived", agg.archived),
        );
    }
    if agg.dirty > 0 {
        chip(
            ui,
            CounterChip::Dirty.color(),
            format!("{} dirty", agg.dirty),
        );
    }
}

fn status_total_text(agg: &AggregatedStatus) -> String {
    format!("{} total", agg.total)
}

fn show_tool_window(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.repo_selection.is_empty() {
        super::multi_selection::show_summary(ui, state);
        return;
    }
    if state.ui.tab == Tab::Log && state.selected_root.is_some() {
        let id = state.selected_root.clone().unwrap();
        if state.caches.log(&id).is_none()
            && !state.caches.has_log_failure(&LogRead::Commits(id.clone()))
        {
            state.fetch_log(id);
        }
    }
    match state.ui.tab {
        Tab::Commit => super::commit_window::show(ui, state),
        Tab::Log => super::log_window::show_log(ui, state),
        Tab::Branches => super::branches::show(ui, state),
        Tab::Worktrees => super::worktrees::show(ui, state),
        Tab::Submodules => super::submodules::show(ui, state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use turbogit_domain::model::{Change, ChangeStatus, Root, RootId, RootStatus};

    fn root(path: &str, branch: Option<&str>) -> Root {
        Root {
            id: RootId(PathBuf::from(path).into()),
            path: PathBuf::from(path),
            remotes: vec![],
            branches: vec![],
            current_branch: branch.map(str::to_string),
            head: None,
            status: RootStatus::default(),
        }
    }

    fn change(path: &str, status: ChangeStatus) -> Change {
        Change {
            path: PathBuf::from(path),
            status,
            chunks: vec![],
            staged: false,
            unstaged: false,
            orig_path: None,
        }
    }

    #[test]
    fn counter_chip_reserves_orange_for_unpulled_and_dirty() {
        assert_eq!(CounterChip::Diverged.color(), Palette::STATUS_DIVERGED);
        assert_eq!(CounterChip::Unpulled.color(), Palette::COUNTER);
        assert_eq!(CounterChip::Dirty.color(), Palette::COUNTER);
    }

    #[test]
    fn counter_orange_only_colors_the_unpulled_and_dirty_surfaces() {
        for kind in [
            CounterChip::Diverged,
            CounterChip::Unpulled,
            CounterChip::Dirty,
        ] {
            let is_counter = matches!(kind, CounterChip::Unpulled | CounterChip::Dirty);
            assert_eq!(
                kind.color() == Palette::COUNTER,
                is_counter,
                "{kind:?} must use COUNTER exactly when it is a dirty/unpulled counter"
            );
        }
    }

    #[test]
    fn computes_diverged_unpulled_and_dirty_counts_from_root_state() {
        let mut dirty = root("/w/dirty", Some("main"));
        dirty.status.changes = vec![change("a.txt", ChangeStatus::Modified)];
        let mut conflicted = root("/w/conflicted", Some("main"));
        conflicted.status.conflicted = vec![PathBuf::from("c.txt")];
        let diverged = root("/w/diverged", Some("main"));
        let unpulled = root("/w/unpulled", Some("main"));
        let clean = root("/w/clean", Some("main"));
        let roots = vec![dirty, conflicted, diverged, unpulled, clean];

        let agg = AggregatedStatus::compute(&roots, &|id| {
            let p = id.0.as_os_str();
            if p == "/w/diverged" {
                Some((1, 1))
            } else if p == "/w/unpulled" {
                Some((0, 2))
            } else {
                Some((0, 0))
            }
        });

        assert_eq!(agg.total, 5);
        assert_eq!(
            agg.dirty, 2,
            "modified and conflicted roots both count as dirty"
        );
        assert_eq!(agg.conflicts, 1);
        assert_eq!(
            agg.diverged, 2,
            "any ahead/behind drift is loosely diverged"
        );
        assert_eq!(
            agg.unpulled, 2,
            "behind roots count as unpulled whether or not also diverged"
        );
        assert_eq!(agg.archived, 0);
    }
}
