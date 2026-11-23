//! The shell frame: the tab strip, the content column, the activity feed,
//! the status bar, and the workspace picker's trigger. Tab structure
//! switches on the tab signal the shell mirrors from `AppState::ui.tab`;
//! every state read lives inside a binding or a region condition.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::OwnedOperation;
use turbogit_app::activity::TimeWindow;
use turbogit_app::operation::{LifecycleVerb, Operation};
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{DisplayName, Root, RootId};

use crate::bridge::AppHandle;
use crate::screens::branch_picker;
use crate::screens::branches;
use crate::screens::changes;
use crate::screens::log;
use crate::screens::submodules;
use crate::screens::workspace_picker;
use crate::screens::worktrees;
use crate::test_ids;
use crate::theme as data;
use crate::ui::icon_button::{IconButtonProps, icon_button};
use crate::ui::progress::{ProgressProps, progress};
use crate::ui::row::{RowProps, RowState, row};
use crate::ui::tab::{TabItem, TabsProps, tabs};

/// The status bar's height, the band the whole shell keeps.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;

/// The five tabs in strip order, with the titles the strip paints.
pub const TAB_TITLES: [(Tab, &str); 5] = [
    (Tab::Commit, "Changes"),
    (Tab::Log, "Log"),
    (Tab::Branches, "Branches"),
    (Tab::Worktrees, "Worktrees"),
    (Tab::Submodules, "Submodules"),
];

/// The shell body over a live state: the top bar, the content column, and
/// the activity feed.
pub fn shell_screen(handle: AppHandle, switcher: TabSwitcher) -> impl IntoElement {
    let t = theme();
    let activity_expanded = signal(handle.peek(|s| s.ui.activity.expanded));
    // The N5 and N6 surfaces have no tab of their own: this band opens their
    // routes, and a tab press returns to the tab's own surface.
    let sheet_route = signal(None::<String>);

    // The worktree and submodule caches fire like the egui shell's did: the
    // Worktrees and Submodules routes read them.
    handle.update(warm_shell_state);

    let tab_for_index = switcher.signal();
    let tab_for_press = switcher.clone();
    let items: Vec<TabItem> = TAB_TITLES
        .iter()
        .map(|(value, title)| {
            let slug = match value {
                Tab::Commit => "Commit",
                Tab::Log => "Log",
                Tab::Branches => "Branches",
                Tab::Worktrees => "Worktrees",
                Tab::Submodules => "Submodules",
            };
            TabItem::new(*title).test_id(&test_ids::tab(slug))
        })
        .collect();
    let strip = tabs(
        TabsProps::new(items, move || {
            TAB_TITLES
                .iter()
                .position(|(value, _)| *value == tab_for_index.get())
                .unwrap_or(0)
        })
        .on_change(move |at| {
            if let Some((value, _)) = TAB_TITLES.get(at) {
                tab_for_press.select(*value);
                sheet_route.set(None);
            }
        }),
    );

    let content_handle = handle.clone();
    let content_route = sheet_route;
    let content_switcher = switcher.clone();
    let content = dynamic(move || {
        if let Some(id) = content_route.get() {
            return crate::screens::route_screen(
                content_handle.clone(),
                &id,
                content_switcher.clone(),
            );
        }
        match switcher.signal().get() {
            Tab::Commit => changes::changes_screen(content_handle.clone()).into_element(),
            Tab::Log => log::log_screen(content_handle.clone(), switcher.clone()).into_element(),
            Tab::Branches => branches::branches_screen(content_handle.clone()).into_element(),
            Tab::Worktrees => worktrees::worktrees_screen(content_handle.clone()).into_element(),
            Tab::Submodules => submodules::submodules_screen(content_handle.clone()).into_element(),
        }
    });

    let mut sheet_band = view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(data::DENSITY_LIST_GAP))
        .px(px(data::PANEL_PADDING))
        .pb(px(data::SPACING_Y))
        .test_id(&test_ids::toolbar("surfaces"));
    for (n6, id, label) in [
        (false, "rewrite", "Rewrite"),
        (false, "conflicts", "Conflicts"),
        (false, "rebase", "Rebase"),
        (false, "rebase_editor", "Edit plan"),
        (false, "cherry_pick", "Cherry"),
        (false, "get_from_branch", "Get from"),
        (false, "patches", "Patches"),
        (false, "unshelve", "Unshelve…"),
        (false, "bulk", "Bulk"),
        (true, "blame", "Blame"),
        (true, "settings", "Settings"),
        (true, "shortcuts", "Shortcuts"),
        (true, "palette", "Actions"),
        (true, "custom_command", "Custom"),
    ] {
        let press_route = sheet_route;
        let prefix = if n6 { "n6" } else { "n5" };
        let band_id = test_ids::toolbar(&format!("{prefix}-{id}"));
        sheet_band = sheet_band.child(
            view()
                .role(AccessibilityRole::Button)
                .label(label)
                .test_id(band_id.as_str())
                .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                .rounded(px(data::CONTROL_RADIUS))
                .bg(t.secondary)
                .child(
                    text(label)
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.secondary_foreground),
                )
                .on_press(move || press_route.set(Some(id.to_string()))),
        );
    }

    let settings_handle = handle.clone();
    let shortcuts_handle = handle.clone();
    let palette_handle = handle.clone();
    let activity_handle = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .child(
            view()
                .flex_col()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(data::SPACING_X))
                        .px(px(data::PANEL_PADDING))
                        .py(px(data::SPACING_Y))
                        .child(strip)
                        .child(view().flex_1())
                        .child(icon_button(
                            IconButtonProps::new("⚙", "Open settings")
                                .test_id(&test_ids::toolbar("settings"))
                                .on_press(move || crate::screens::settings::open(&settings_handle)),
                        ))
                        .child(icon_button(
                            IconButtonProps::new("⌘", "Show shortcuts")
                                .test_id(&test_ids::toolbar("shortcuts"))
                                .on_press(move || {
                                    shortcuts_handle.update(|s| s.ui.shortcuts_open = true)
                                }),
                        ))
                        .child(icon_button(
                            IconButtonProps::new("⌕", "Find actions")
                                .test_id(&test_ids::toolbar("palette"))
                                .on_press(move || {
                                    palette_handle.update(|s| {
                                        s.ui.command_query.clear();
                                        s.ui.command_palette = true;
                                    })
                                }),
                        ))
                        .child(workspace_picker::picker_trigger(handle.clone()))
                        .child(branch_picker::picker_trigger(handle.clone())),
                )
                .child(sheet_band),
        )
        .child(content)
        .child(activity_panel(activity_handle, activity_expanded))
}

/// The `(handle, tab)` pair every tab change goes through: the tab bar's
/// presses and the log's recovery. The signal drives the structure; the
/// state records the choice.
#[derive(Clone)]
pub struct TabSwitcher {
    handle: AppHandle,
    tab: Signal<Tab>,
}

impl TabSwitcher {
    pub fn new(handle: AppHandle, tab: Signal<Tab>) -> Self {
        TabSwitcher { handle, tab }
    }

    /// Selects a tab in the signal and the state, the egui `switch_tab`.
    pub fn select(&self, to: Tab) {
        self.tab.set(to);
        self.handle.update(move |s| switch_tab_state(s, to));
    }

    /// The signal the content region reads.
    pub fn signal(&self) -> Signal<Tab> {
        self.tab
    }
}

fn switch_tab_state(state: &mut AppState, to: Tab) {
    state.ui.tab = to;
    if to == Tab::Branches {
        state.ui.branches_focus_search = true;
    }
    state.save_durable_ui();
}

/// Fires the worktree and submodule reads the shell warms, once per root.
fn warm_shell_state(state: &mut AppState) {
    let ids: Vec<_> = state
        .multi
        .roots
        .iter()
        .map(|root| root.id.clone())
        .collect();
    for id in ids {
        if state.caches.worktrees(&id).is_none() {
            state.fetch_worktrees(id.clone());
        }
        if state.caches.submodules(&id).is_none() {
            state.fetch_submodules(id);
        }
    }
}

/// The activity feed: the title chip that expands it, the clear, window,
/// and repo controls, and one row per visible entry.
fn activity_panel(handle: AppHandle, expanded: Signal<bool>) -> impl IntoElement {
    let t = theme();
    let toggle_handle = handle.clone();
    let toggle = expanded;
    let rows_handle = handle.clone();
    let empty_handle = handle.clone();
    let controls_handle = handle.clone();
    let controls_expanded = expanded;

    view()
        .flex_col()
        .test_id(&test_ids::activity("feed"))
        .bg(t.card)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::SPACING_X))
                .px(px(data::PANEL_PADDING))
                .py(px(DENSE_PAD_Y))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .role(AccessibilityRole::Button)
                        .label(move || {
                            if toggle.get() {
                                "Collapse log".to_string()
                            } else {
                                "Expand log".to_string()
                            }
                        })
                        .test_id(&test_ids::activity("toggle"))
                        .on_press(move || {
                            let next = !toggle.get();
                            toggle.set(next);
                            toggle_handle.update(move |s| s.ui.activity.expanded = next);
                        })
                        .child(
                            text(move || if toggle.get() { "▾" } else { "▸" })
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(t.muted_foreground),
                        )
                        .child(
                            text("LOG")
                                .text_size(px(data::TYPE_CONTROL))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(t.muted_foreground),
                        ),
                )
                .child(when(
                    move || controls_expanded.get(),
                    move || {
                        view()
                            .flex_row()
                            .flex_1()
                            .justify_end()
                            .gap(px(data::SPACING_X))
                            .child(clear_button(controls_handle.clone()))
                            .child(window_button(controls_handle.clone()))
                            .child(repo_button(controls_handle.clone()))
                    },
                )),
        )
        .child(when(
            move || expanded.get(),
            move || {
                let rows_handle = rows_handle.clone();
                let empty_handle = empty_handle.clone();
                view()
                    .flex_col()
                    .max_h(px(200.0))
                    .child(each(
                        move || {
                            rows_handle.read(|s| {
                                s.ui.activity
                                    .visible(chrono::Local::now())
                                    .into_iter()
                                    .rev()
                                    .map(|entry| ActivityRow {
                                        key: format!(
                                            "{}|{}",
                                            entry.at.timestamp_millis(),
                                            entry.message
                                        ),
                                        time: entry.at.format("%H:%M").to_string(),
                                        repo: entry
                                            .repo
                                            .clone()
                                            .unwrap_or_else(|| "all repos".to_string()),
                                        message: entry.message.clone(),
                                        kind: match entry.kind {
                                            turbogit_app::activity::ActivityKind::Success => 0,
                                            turbogit_app::activity::ActivityKind::Warning => 1,
                                            turbogit_app::activity::ActivityKind::Error => 2,
                                        },
                                    })
                                    .collect::<Vec<_>>()
                            })
                        },
                        |entry| entry.key.clone(),
                        move |entry, _| {
                            let t = theme();
                            let color = match entry.get().kind {
                                0 => data::STATE_SUCCESS,
                                1 => data::STATE_WARNING,
                                _ => data::STATE_ERROR,
                            };
                            row(RowProps::new()
                                .state(RowState::Default)
                                .test_id(&test_ids::activity(format!("entry:{}", entry.get().key)))
                                .child(
                                    text(entry.get().time.clone())
                                        .text_size(px(data::TYPE_CONTROL))
                                        .text_color(t.muted_foreground),
                                )
                                .child(
                                    text(entry.get().repo.clone())
                                        .text_size(px(data::TYPE_CONTROL))
                                        .text_color(t.foreground),
                                )
                                .child(
                                    text(move || entry.get().message.clone())
                                        .text_size(px(data::TYPE_BODY))
                                        .text_color(color),
                                ))
                        },
                    ))
                    .child(when(
                        move || {
                            empty_handle
                                .read(|s| s.ui.activity.visible(chrono::Local::now()).is_empty())
                        },
                        move || {
                            let t = theme();
                            view()
                                .px(px(data::PANEL_PADDING))
                                .py(px(data::SPACING_Y))
                                .child(
                                    text("No activity yet")
                                        .text_size(px(data::TYPE_BODY))
                                        .text_color(t.muted_foreground),
                                )
                        },
                    ))
            },
        ))
}

const DENSE_PAD_Y: f32 = 2.0;

fn clear_button(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let clear_handle = handle;
    view()
        .role(AccessibilityRole::Button)
        .label("Clear feed")
        .test_id(&test_ids::activity("clear"))
        .px(px(data::DENSITY_DENSE_BUTTON_PAD_X))
        .py(px(DENSE_PAD_Y))
        .rounded(px(data::CONTROL_RADIUS))
        .text_size(px(data::TYPE_CONTROL))
        .text_color(t.muted_foreground)
        .hover(|s| s.bg(t.muted))
        .on_press(move || clear_handle.update(|s| s.ui.activity.clear()))
        .child(text("Clear feed").text_size(px(data::TYPE_CONTROL)))
}

fn window_button(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let window_handle = handle;
    let window_label_handle = window_handle.clone();
    let window_press_handle = window_handle.clone();
    let window_label_handle_2 = window_label_handle.clone();
    view()
        .role(AccessibilityRole::Button)
        .label(move || SharedString::from(window_label(&window_label_handle_2)))
        .test_id(&test_ids::activity("window"))
        .px(px(data::DENSITY_DENSE_BUTTON_PAD_X))
        .py(px(DENSE_PAD_Y))
        .rounded(px(data::CONTROL_RADIUS))
        .text_size(px(data::TYPE_CONTROL))
        .text_color(t.muted_foreground)
        .hover(|s| s.bg(t.muted))
        .on_press(move || {
            window_press_handle.update(|s| {
                s.ui.activity.window = match s.ui.activity.window {
                    TimeWindow::All => TimeWindow::Last30Min,
                    TimeWindow::Last30Min => TimeWindow::All,
                };
            });
        })
        .child(
            text(move || SharedString::from(window_label(&window_label_handle)))
                .text_size(px(data::TYPE_CONTROL)),
        )
}

fn window_label(handle: &AppHandle) -> String {
    handle.read(|s| match s.ui.activity.window {
        TimeWindow::All => "All time".to_string(),
        TimeWindow::Last30Min => "Last 30 min".to_string(),
    })
}

fn repo_button(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let repo_handle = handle;
    let repo_label_handle = repo_handle.clone();
    let repo_press_handle = repo_handle.clone();
    let repo_label_handle_2 = repo_label_handle.clone();
    view()
        .role(AccessibilityRole::Button)
        .label(move || SharedString::from(repo_label(&repo_label_handle_2)))
        .test_id(&test_ids::activity("repo-filter"))
        .px(px(data::DENSITY_DENSE_BUTTON_PAD_X))
        .py(px(DENSE_PAD_Y))
        .rounded(px(data::CONTROL_RADIUS))
        .text_size(px(data::TYPE_CONTROL))
        .text_color(t.muted_foreground)
        .hover(|s| s.bg(t.muted))
        .on_press(move || repo_press_handle.update(cycle_repo_filter))
        .child(
            text(move || SharedString::from(repo_label(&repo_label_handle)))
                .text_size(px(data::TYPE_CONTROL)),
        )
}

fn repo_label(handle: &AppHandle) -> String {
    handle.read(|s| {
        s.ui.activity
            .repo_filter
            .clone()
            .unwrap_or_else(|| "All repos".to_string())
    })
}

/// The repo filter cycles None → first repo label → … → None, the egui
/// `cycle_repo_filter`.
fn cycle_repo_filter(state: &mut AppState) {
    let labels: Vec<String> = state
        .multi
        .roots
        .iter()
        .map(|r| r.path.display_name_or(&r.path.display().to_string()))
        .collect();
    let current = state.ui.activity.repo_filter.clone();
    state.ui.activity.repo_filter = match current {
        None => labels.into_iter().next(),
        Some(cur) => labels
            .iter()
            .skip_while(|l| *l != &cur)
            .nth(1)
            .cloned()
            .or(None),
    };
}

/// The status bar the whole app keeps, welcome included: the version line,
/// the in-flight announcements, the aggregated counters, and the total.
pub fn status_bar(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let version_handle = handle.clone();
    let total_handle = handle.clone();
    let busy_handle = handle.clone();
    let inflight_handle = handle.clone();
    let inflight_cond = handle.clone();
    let chips_handle = handle.clone();

    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .h(px(STATUS_BAR_HEIGHT))
        .px(px(8.0))
        .bg(t.card)
        .border(px(1.0))
        .border_color(t.border)
        .test_id(&test_ids::status_bar())
        .child(
            text(move || {
                version_handle.read(|s| {
                    format!(
                        "v{} · git {} · {} repos indexed",
                        env!("CARGO_PKG_VERSION"),
                        s.git_version,
                        s.multi.roots.len(),
                    )
                })
            })
            .text_size(px(data::TYPE_CONTROL))
            .text_color(t.foreground),
        )
        .child(when(
            move || !inflight_cond.read(held_by).is_empty(),
            move || inflight_items(inflight_handle.clone()),
        ))
        .child(agg_chips(chips_handle))
        .child(view().flex_1())
        .child(when(
            move || busy_handle.read(|s| s.ui.busy),
            move || {
                progress(
                    ProgressProps::new(0.0)
                        .indeterminate()
                        .accessibility_label("An operation is running")
                        .test_id(&test_ids::status_bar_busy()),
                )
            },
        ))
        .child(
            text(move || total_handle.read(|s| format!("{} total", s.multi.roots.len())))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        )
}

/// The in-flight items, one announcement per holding root, each with its
/// verbs. A verb press dispatches `Operation::Resolve` on that root, the
/// named per-root operation the feed records.
fn inflight_items(handle: AppHandle) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .child(each(
            {
                let handle = handle.clone();
                move || handle.read(inflight_rows)
            },
            |item| item.key.clone(),
            move |item, _| {
                let t = theme();
                let verbs_handle = handle.clone();
                let mut children: Vec<AnyElement> = Vec::new();
                children.push(
                    text(move || item.get().announce.clone())
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(data::STATE_WARNING)
                        .into_element(),
                );
                for (idx, verb) in item.get().verbs.iter().enumerate() {
                    if idx == 1 {
                        // The destructive verb reads as separated from its
                        // pair, the gap the egui bar kept before Continue.
                        children.push(view().w(px(8.0)).into_element());
                    }
                    let verb_handle = verbs_handle.clone();
                    let root = item.get().root.clone();
                    let owner = item.get().owner;
                    let verb = *verb;
                    children.push(
                        view()
                            .role(AccessibilityRole::Button)
                            .label(SharedString::from(verb.label()))
                            .test_id(&test_ids::toolbar(&format!(
                                "verb-{}-{}",
                                verb_label_slug(verb),
                                item.get().root.name()
                            )))
                            .px(px(data::DENSITY_DENSE_BUTTON_PAD_X))
                            .py(px(1.0))
                            .rounded(px(data::CONTROL_RADIUS))
                            .bg(match verb {
                                LifecycleVerb::Abort => t.secondary,
                                LifecycleVerb::Continue => t.accent,
                                LifecycleVerb::Skip => t.muted,
                            })
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(match verb {
                                LifecycleVerb::Abort => data::DANGER,
                                LifecycleVerb::Continue => t.primary,
                                LifecycleVerb::Skip => t.foreground,
                            })
                            .on_press(move || {
                                verb_handle.update(|s| {
                                    s.dispatch(Operation::Resolve {
                                        root: root.clone(),
                                        owner,
                                        verb,
                                    });
                                });
                            })
                            .child(text(verb.label()).text_size(px(data::TYPE_CONTROL)))
                            .into_element(),
                    );
                }
                view().flex_row().items_center().children(children)
            },
        ))
}

/// The test-id slug of a verb, from its short label (`abort`, `skip`).
fn verb_label_slug(verb: LifecycleVerb) -> &'static str {
    match verb {
        LifecycleVerb::Abort => "abort",
        LifecycleVerb::Continue => "continue",
        LifecycleVerb::Skip => "skip",
    }
}

/// One counter chip: its state color and its text.
pub type AggChip = (AdaptiveColor, String);

/// The aggregated status the status bar reads: per-root counts computed the
/// way the egui `AggregatedStatus::compute` computed them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AggregatedStatus {
    pub diverged: usize,
    pub conflicts: usize,
    pub unpulled: usize,
    pub dirty: usize,
    pub total: usize,
}

impl AggregatedStatus {
    pub fn compute(
        roots: &[Root],
        ahead_behind: &dyn Fn(&RootId) -> Option<(usize, usize)>,
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
            if root_is_dirty(&root.status) {
                agg.dirty += 1;
            }
            if let Some((ahead, behind)) = ahead_behind(&root.id) {
                // Any drift is loosely diverged, the counter's reading.
                if ahead > 0 || behind > 0 {
                    agg.diverged += 1;
                }
                if behind > 0 {
                    agg.unpulled += 1;
                }
            }
        }
        agg
    }

    /// The chips in paint order, colored by the repository-state map.
    pub fn chips(&self) -> Vec<AggChip> {
        let mut chips = Vec::new();
        let mut chip = |state: crate::theme::RepoState, text: String| {
            chips.push((state.color(), text));
        };
        if self.diverged > 0 {
            chip(
                crate::theme::RepoState::Diverged,
                format!("{} diverged", self.diverged),
            );
        }
        if self.conflicts > 0 {
            chip(
                crate::theme::RepoState::Conflict,
                format!("{} conflict", self.conflicts),
            );
        }
        if self.unpulled > 0 {
            chip(
                crate::theme::RepoState::Unpulled,
                format!("{} unpulled", self.unpulled),
            );
        }
        if self.dirty > 0 {
            chip(
                crate::theme::RepoState::Dirty,
                format!("{} dirty", self.dirty),
            );
        }
        chips
    }
}

/// A root with local work: a modified or unversioned entry, the egui
/// `RepoState::is_dirty` reading. A conflicted entry is work the resolver
/// owns, and the conflicts counter already names it.
fn root_is_dirty(status: &turbogit_domain::model::RootStatus) -> bool {
    status.modified() + status.unversioned() > 0
}

fn aggregated_status(state: &AppState) -> AggregatedStatus {
    AggregatedStatus::compute(&state.multi.roots, &|id| state.caches.ahead_behind(id))
}

fn agg_chips(handle: AppHandle) -> impl IntoElement {
    each(
        move || handle.read(|s| aggregated_status(s).chips()),
        |chip| chip.1.clone(),
        |chip, _| {
            text(move || chip.get().1.clone())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(chip.get().0)
        },
    )
}

/// One in-flight announcement the feed's each reads: owned data, so the
/// rows compare across ticks.
#[derive(Clone, PartialEq)]
pub struct InFlightRow {
    key: String,
    pub root: RootId,
    pub name: String,
    pub owner: OwnedOperation,
    pub verbs: Vec<LifecycleVerb>,
    pub announce: String,
}

fn inflight_rows(state: &AppState) -> Vec<InFlightRow> {
    held_by(state)
        .into_iter()
        .map(|item| {
            let announce = item.announce();
            let verbs = LifecycleVerb::offered_for(item.owner).to_vec();
            InFlightRow {
                key: item.root.0.display().to_string(),
                root: item.root,
                name: item.name,
                owner: item.owner,
                verbs,
                announce,
            }
        })
        .collect()
}

/// One activity row the feed's each reads: owned data, so the rows compare
/// across ticks.
#[derive(Clone, PartialEq)]
pub struct ActivityRow {
    key: String,
    time: String,
    repo: String,
    message: String,
    kind: u8,
}

/// One in-flight item: a root and the operation git holds it in.
#[derive(Debug, Clone)]
pub struct InFlight {
    pub root: RootId,
    pub name: String,
    pub owner: OwnedOperation,
}

impl InFlight {
    pub fn announce(&self) -> String {
        let name = &self.name;
        match self.owner {
            OwnedOperation::CherryPick => format!("Cherry-picking in {name}"),
            OwnedOperation::Rebase => format!("Rebasing in {name}"),
            OwnedOperation::Merge => format!("Merging in {name}"),
            OwnedOperation::Revert => format!("Reverting in {name}"),
        }
    }

    pub fn verbs(&self) -> &'static [LifecycleVerb] {
        LifecycleVerb::offered_for(self.owner)
    }
}

/// The roots that hold an operation, in registration order.
pub fn items_via(
    roots: &[Root],
    owner_of: &dyn Fn(&Root) -> Option<OwnedOperation>,
) -> Vec<InFlight> {
    roots
        .iter()
        .filter_map(|root| {
            owner_of(root).map(|owner| InFlight {
                root: root.id.clone(),
                name: root.id.name(),
                owner,
            })
        })
        .collect()
}

fn held_by(state: &AppState) -> Vec<InFlight> {
    items_via(&state.multi.roots, &|root| {
        state.holding_operation(root.id.as_path())
    })
}

/// The root a verb acts on: the most specific context that offers it, the
/// egui `target_for` (D10). `None` when no root offers it.
pub fn target_for(
    state: &AppState,
    selected: &[RootId],
    verb: LifecycleVerb,
) -> Option<(RootId, OwnedOperation)> {
    let found = held_by(state);
    let offers = |item: &InFlight| LifecycleVerb::offered_for(item.owner).contains(&verb);
    found
        .iter()
        .find(|item| selected.contains(&item.root) && offers(item))
        .or_else(|| found.iter().find(|item| offers(item)))
        .map(|item| (item.root.clone(), item.owner))
}
