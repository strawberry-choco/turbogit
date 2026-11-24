//! The log screen: the branches pane, the commits pane over a virtual list
//! with a per-row lane strip, the changed-files and commit-details column,
//! the filter bar, the scope chips, the scroll intents, and paging. Every
//! list reads the held [`LogDisplay`] through its signal; selection,
//! hover, and the open context menu key by root and commit, never by row
//! index.

use std::rc::Rc;
use std::sync::Arc;

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::OwnedOperation;
use turbogit_app::log_display::{LogDisplay, listing_roots};
use turbogit_app::log_filters::{Fold, ref_bands};
use turbogit_app::log_states::{self, LogRecovery, LogState};
use turbogit_app::root_caches::{LogRead, LogScope, file_stat};
use turbogit_app::state::{
    AppState, BlameTarget, DiffTarget, NewBranchBase, PendingConfirm, Toast,
};
use turbogit_app::sync_service;
use turbogit_domain::model::{BranchKind, ChangeStatus, Commit, PatchOrigin, RootId};

use crate::bridge::AppHandle;
use crate::screens::log_graph::{ROW_HEIGHT, STRIP_WIDTH, lane_strip};
use crate::screens::shell::TabSwitcher;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::icon::Icon;
use crate::ui::input::{InputProps, input};
use crate::ui::pane_header::{PaneHeaderProps, pane_header};
use crate::ui::popover::{PopoverProps, popover};
use crate::ui::row::{RowProps, RowState, row};
use crate::ui::tree::{TreeEntryProps, TreeProps, tree};

/// The log's cadence: the reads, the display sync, the scope mirror, and
/// the scroll intents serve off one tick.
const TICK_MS: u64 = 200;

/// The pane widths the resizable frame starts at.
const BRANCHES_WIDTH: f32 = 420.0;
const BRANCHES_MIN_WIDTH: f32 = 140.0;
const FILES_WIDTH: f32 = 344.0;
const FILES_MIN_WIDTH: f32 = 180.0;

/// The filter scopes the dropdown row mirrors, one snapshot per tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterSnapshot {
    pub ref_scope: Option<String>,
    pub ref_carriers: Option<(usize, usize)>,
    pub path_scope: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    pub roots_all: bool,
    pub roots_selected: usize,
    pub roots_total: usize,
    pub query: bool,
}

impl FilterSnapshot {
    fn of(state: &AppState) -> Self {
        let selected = state.ui.log_roots.selected(&state.multi.roots).len();
        let carriers = listing_roots(state).len();
        FilterSnapshot {
            ref_scope: state.ui.log_ref_scope.clone(),
            ref_carriers: (carriers < selected).then_some((carriers, selected)),
            path_scope: state
                .ui
                .log_path_scope
                .as_ref()
                .map(|(_, path)| path.display().to_string()),
            author: state.ui.log_author_filter.clone(),
            date: (!state.ui.log_date_filter.is_all())
                .then(|| state.ui.log_date_filter.label().to_string()),
            roots_all: state.ui.log_roots.is_all(),
            roots_selected: selected,
            roots_total: state.multi.roots.len(),
            query: !state.ui.log_filter.trim().is_empty(),
        }
    }
}

/// The held display the log's bindings read, keyed by the allocation
/// `sync_log_display` hands out: the same window compares equal, so the
/// signal fires only when a rebuild landed.
#[derive(Clone)]
pub struct DisplayWindow(Arc<LogDisplay>);

impl PartialEq for DisplayWindow {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::ops::Deref for DisplayWindow {
    type Target = LogDisplay;

    fn deref(&self) -> &LogDisplay {
        &self.0
    }
}

/// The log over a live state. `switcher` moves the tab when a recovery
/// verb sends the user to the Commit tab.
pub fn log_screen(handle: AppHandle, _switcher: TabSwitcher) -> impl IntoElement {
    let t = theme();
    let display = signal(DisplayWindow(handle.update_with(|s| {
        ensure_log_data(s, false);
        s.sync_log_display()
    })));
    let filters = signal(handle.peek(FilterSnapshot::of));
    let list = element_ref();

    let tick = handle.clone();
    let tick_display = display;
    let tick_filters = filters;
    let tick_list = list;
    let tick_count = signal(0_u64);
    set_interval(std::time::Duration::from_millis(TICK_MS), move || {
        let n = tick_count.get() + 1;
        tick_count.set(n);
        // An empty history is the one read that has to be re-asked on its
        // own: the first commit can arrive with no input at all.
        let retry_empty = n.is_multiple_of(10);
        let arc = tick.update_with(|s| {
            ensure_log_data(s, retry_empty);
            s.sync_log_display()
        });
        tick_display.set(DisplayWindow(arc));
        let snapshot = tick.peek(FilterSnapshot::of);
        tick_filters.set(snapshot);
        serve_scroll_intents(&tick, &tick_display, &tick_list);
    });

    bind_log_keys(handle.clone(), list);

    let branches_handle = handle.clone();
    let commits_handle = handle.clone();
    let right_handle = handle.clone();
    let bar_handle = handle.clone();
    let bar_filters = filters;
    let commits_display = display;
    let commits_filters = filters;
    let commits_list = list;

    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .test_id(&test_ids::route("log"))
        .child(
            nsh_base::h_resizable()
                .flex_1()
                .child(
                    nsh_base::resizable_panel()
                        .size_range(Points(BRANCHES_MIN_WIDTH)..Points(BRANCHES_WIDTH))
                        .child(branches_pane(branches_handle).into_element()),
                )
                .child(
                    nsh_base::resizable_panel().child(
                        commits_pane(
                            commits_handle,
                            commits_display,
                            commits_filters,
                            commits_list,
                        )
                        .into_element(),
                    ),
                )
                .child(
                    nsh_base::resizable_panel()
                        .size_range(Points(FILES_MIN_WIDTH)..Points(FILES_WIDTH))
                        .child(right_column(right_handle).into_element()),
                ),
        )
        .child(filter_bar(bar_handle, bar_filters))
}

/// The reads the commits pane's listing needs, the egui `ensure_log_data`.
/// `retry_unborn` re-asks a repository whose read failed because it has no
/// commits: the first commit is the only way out of that state, and it
/// arrives without the app doing anything.
fn ensure_log_data(state: &mut AppState, retry_unborn: bool) {
    state.ensure_ref_evidence();
    let ref_ids: Vec<RootId> = state
        .multi
        .roots
        .iter()
        .map(|root| root.id.clone())
        .collect();
    for id in ref_ids {
        if !state.caches.refs_loaded(&id) {
            state.fetch_refs(id);
        }
    }
    if let Some((root, cid)) = state.ui.selected_commit.clone() {
        state.ensure_files(&root, &cid);
        state.fetch_file_stats(root, cid);
    }
    if let Some(key) = state.log_filter_key() {
        for id in listing_roots(state) {
            state.ensure_log(&id, LogScope::Combined(key.clone()));
        }
        return;
    }
    if let Some((root, path)) = state.ui.log_path_scope.clone() {
        state.ensure_log(&root, LogScope::Path(path));
    }
    if let Some(ref_name) = state.ui.log_ref_scope.clone() {
        for id in listing_roots(state) {
            state.ensure_log(&id, LogScope::Ref(ref_name.clone()));
        }
    }
    let query = state.ui.log_filter.trim().to_string();
    if !query.is_empty() {
        for id in listing_roots(state) {
            state.ensure_log(&id, LogScope::Search(query.clone()));
        }
    }
    let scoped = state.log_filter_key().is_some()
        || state.ui.log_path_scope.is_some()
        || state.ui.log_ref_scope.is_some()
        || !query.is_empty();
    if scoped {
        return;
    }
    // The unscoped listing has no scope read of its own, so nothing else
    // asks git for it on first paint. A failed read stays failed until the
    // user retries, exactly as a scoped one does.
    let failed_reads = log_states::failed_listing_reads(state);
    let unborn: Vec<LogRead> = if retry_unborn {
        failed_reads
            .iter()
            .filter(|read| state.caches.log_failure(read).is_some_and(unborn_message))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    for read in &unborn {
        state.retry_log_read(read);
    }
    if retry_unborn
        && state.log_filter_key().is_none()
        && state.ui.log_ref_scope.is_none()
        && state.ui.log_path_scope.is_none()
        && state.ui.log_filter.trim().is_empty()
    {
        let empty: Vec<RootId> = listing_roots(state)
            .into_iter()
            .filter(|id| state.caches.log(id).is_some_and(|window| window.is_empty()))
            .collect();
        for id in empty {
            state.fetch_log(id);
        }
    }
    let failed: Vec<RootId> = failed_reads
        .into_iter()
        .filter_map(|read| match read {
            LogRead::Commits(id) => Some(id),
            _ => None,
        })
        .collect();
    for id in listing_roots(state) {
        if state.caches.log(&id).is_none() && !failed.contains(&id) {
            state.fetch_log(id);
        }
    }
}

/// Whether a read failed because the repository has no commits at all.
fn unborn_message(message: &str) -> bool {
    message.contains("does not have any commits yet") || message.contains("no commits yet")
}

/// The scroll intents the tick serves: a named commit scrolls to centre; a
/// commit the filters hide raises the jump band instead.
fn serve_scroll_intents(handle: &AppHandle, display: &Signal<DisplayWindow>, list: &ElementRef) {
    let target = handle.update_with(|s| s.ui.log_scroll_to.take());
    let Some(cid) = target else { return };
    let index = display.with(|d| d.rows().iter().position(|row| row.id == cid));
    match index {
        Some(index) => {
            list.scroll_to_index(index as u32, percentage(0.5), px(0.0), false);
        }
        None => {
            handle.update(move |s| {
                if let Some(root) = hidden_jump_target(s, &cid) {
                    s.ui.log_hidden_jump = Some((root, cid));
                }
            });
        }
    }
}

/// The first listing root that holds the commit, the jump band's target.
fn hidden_jump_target(state: &AppState, cid: &str) -> Option<RootId> {
    listing_roots(state)
        .iter()
        .find(|root| find_commit(state, root, cid).is_some())
        .cloned()
}

/// The commit in the read set the view holds, the egui `find_commit`: the
/// scoped name owns the answer, and a root outside the read set answers
/// `None` rather than a stale window's row.
fn find_commit<'a>(state: &'a AppState, root: &RootId, cid: &str) -> Option<&'a Commit> {
    if let Some(key) = state.log_filter_key() {
        return state
            .caches
            .combined_log(root, &key)
            .and_then(|commits| commits.iter().find(|c| c.id == cid));
    }
    if let Some(ref_name) = &state.ui.log_ref_scope {
        if !listing_roots(state)
            .iter()
            .any(|candidate| candidate == root)
        {
            return None;
        }
        return state
            .caches
            .ref_log(root, ref_name)
            .and_then(|commits| commits.iter().find(|c| c.id == cid));
    }
    if let Some((_, path)) = &state.ui.log_path_scope
        && let Some(commits) = state.caches.path_log(root, path)
    {
        return commits.iter().find(|c| c.id == cid);
    }
    state
        .caches
        .log(root)
        .and_then(|commits| commits.iter().find(|c| c.id == cid))
}

/// The arrow-key bindings of the log: they act while the log tab is active.
fn bind_log_keys(handle: AppHandle, list: ElementRef) {
    let step = move |handle: &AppHandle, list: &ElementRef, delta: isize, edge: f32| {
        let active = handle.peek(|s| s.ui.tab) == turbogit_app::state::Tab::Log;
        if !active {
            return;
        }
        let moved = handle.update_with(|s| move_selection(s, delta));
        if let Some(index) = moved {
            list.scroll_to_index(index as u32, percentage(edge), px(0.0), false);
        }
    };
    let down_handle = handle.clone();
    let down_list = list;
    on_key("down", move |_| {
        step(&down_handle, &down_list, 1, 0.5);
    });
    let up_handle = handle.clone();
    let up_list = list;
    on_key("up", move |_| {
        step(&up_handle, &up_list, -1, 0.5);
    });
    let home_handle = handle.clone();
    let home_list = list;
    on_key("home", move |_| {
        let active = home_handle.peek(|s| s.ui.tab) == turbogit_app::state::Tab::Log;
        if active && let Some(index) = home_handle.update_with(|s| select_edge(s, true)) {
            home_list.scroll_to_index(index as u32, percentage(0.0), px(0.0), false);
        }
    });
    let end_handle = handle.clone();
    let end_list = list;
    on_key("end", move |_| {
        let active = end_handle.peek(|s| s.ui.tab) == turbogit_app::state::Tab::Log;
        if active && let Some(index) = end_handle.update_with(|s| select_edge(s, false)) {
            end_list.scroll_to_index(index as u32, percentage(1.0), px(0.0), false);
        }
    });
}

/// Moves the selection one row, clamped at the ends, and answers the
/// selected row's index.
fn move_selection(state: &mut AppState, delta: isize) -> Option<usize> {
    let display = state.sync_log_display();
    let rows = display.rows();
    let at = state.ui.selected_commit.as_ref().and_then(|(root, id)| {
        rows.iter()
            .position(|row| row.id == *id && row.root == *root)
    });
    let next = match at {
        None => 0,
        Some(at) => {
            let next = at as isize + delta;
            next.clamp(0, rows.len().saturating_sub(1) as isize) as usize
        }
    };
    select_row_at(state, rows, next)
}

/// Selects the first (or last) row of the held window.
fn select_edge(state: &mut AppState, first: bool) -> Option<usize> {
    let display = state.sync_log_display();
    let rows = display.rows();
    let index = if first {
        0
    } else {
        rows.len().saturating_sub(1)
    };
    select_row_at(state, rows, index)
}

fn select_row_at(
    state: &mut AppState,
    rows: &[turbogit_app::log_display::LogRow],
    index: usize,
) -> Option<usize> {
    if let Some(row) = rows.get(index) {
        state.ui.selected_commit = Some((row.root.clone(), row.id.clone()));
        state.ui.log_selected_file = None;
        Some(index)
    } else {
        None
    }
}

/// The commits pane: the header, the scope chips, the search input, the
/// virtualized commit list with its lane strip, and the paging footer.
fn commits_pane(
    handle: AppHandle,
    display: Signal<DisplayWindow>,
    filters: Signal<FilterSnapshot>,
    list: ElementRef,
) -> impl IntoElement {
    let t = theme();
    let chips_handle = handle.clone();
    let chips_filters = filters;
    let search_handle = handle.clone();
    let list_handle = handle.clone();
    let row_handle = handle.clone();
    let row_display = display;
    let state_slot_handle = handle.clone();
    let jump_handle = handle.clone();
    let jump_cond = handle.clone();
    let footer_handle = handle.clone();
    let footer_display = display;

    let count = move || display.with(|d| d.len());
    let rows = move |at: Position| commit_row(row_handle.clone(), row_display, at);
    let keys_handle = row_display;
    let list_element = virtual_list(count, rows)
        .keys(move || {
            keys_handle.with(|d| {
                d.rows()
                    .iter()
                    .map(|row| format!("{}:{}", row.root.0.display(), row.id))
                    .collect::<Vec<_>>()
            })
        })
        .element_ref(&list)
        .estimated_item_size(px(ROW_HEIGHT))
        .flex_1()
        .test_id(&test_ids::log("list"))
        .on_load_more(move || {
            list_handle.update(load_more_windows);
        });

    view()
        .flex_1()
        .flex_col()
        .bg(t.card)
        .border_x(px(1.0))
        .border_color(data::DIVIDER)
        .child(pane_header(PaneHeaderProps::new("Commits")))
        .child(scope_chips(chips_handle, chips_filters))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::SPACING_X))
                .px(px(data::PANEL_PADDING))
                .py(px(data::SPACING_Y))
                .child(
                    view().flex_1().child(input(
                        InputProps::new()
                            .placeholder("Search commits")
                            .accessibility_label("Search commits")
                            .on_change(move |text: &str| {
                                let text = text.to_string();
                                search_handle.update(move |s| s.ui.log_filter = text);
                            })
                            .test_id(&test_ids::log("filter")),
                    )),
                ),
        )
        .child(read_slot(&handle, log_states::filtering))
        .child(list_element)
        .child(when(
            move || jump_cond.read(|s| s.ui.log_hidden_jump.is_some()),
            move || hidden_jump_band(jump_handle.clone()),
        ))
        .child(read_slot(&state_slot_handle, move |s| {
            log_states::commit_list(s, display.with(|d| d.is_empty()))
        }))
        .child(paging_footer(footer_handle, footer_display))
}

/// One commit row of the virtual list.
fn commit_row(handle: AppHandle, display: Signal<DisplayWindow>, at: Position) -> impl IntoElement {
    let t = theme();
    let selected_handle = handle.clone();
    let selected_display = display;
    let canvas_display = display;
    let hash_display = display;
    let author_display = display;
    let subject_display = display;
    let pill_display = display;
    let date_display = display;
    let id_display = display;
    let gutter_handle = handle.clone();
    let stripe_handle = handle.clone();
    let press_handle = handle.clone();
    let menu_handle = handle.clone();
    let menu_display = display;
    let row_accent = t.accent;
    // The row sits on its pane's surface: an explicit fill would punch a
    // darker well through the card under the list.
    let row_background = AdaptiveColor::TRANSPARENT;
    let row_muted_ink = t.muted_foreground;
    let row_link_ink = data::LINK;
    let row_body_ink = t.foreground;

    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .h(px(ROW_HEIGHT))
        .px(px(8.0))
        .on_press(move || {
            press_handle.update_with(|s| {
                let display = s.sync_log_display();
                if let Some(row) = display.row(at.index()) {
                    s.ui.selected_commit = Some((row.root.clone(), row.id.clone()));
                    s.ui.log_selected_file = None;
                    s.ui.log_commit_menu = None;
                }
            });
        })
        .on_context_menu(move |event: &ContextMenuEvent| {
            let Some(target) = menu_display.with(|d| {
                d.row(at.index())
                    .map(|row| (row.root.clone(), row.id.clone()))
            }) else {
                return;
            };
            open_commit_menu(menu_handle.clone(), target.0, target.1, event.page_position);
        })
        .bg(move || {
            let row = selected_display.with(|d| {
                d.row(at.index())
                    .map(|row| (row.root.clone(), row.id.clone()))
            });
            let selected = selected_handle.read(|s| {
                row.as_ref().is_some_and(|(root, id)| {
                    s.ui.selected_commit
                        .as_ref()
                        .is_some_and(|(sel_root, sel_id)| sel_root == root && sel_id == id)
                })
            });
            if selected { row_accent } else { row_background }
        })
        .child(
            canvas(move || {
                canvas_display.with(|d| {
                    d.row(at.index())
                        .map(|row| lane_strip(d.lane(&row.id), row.parents.len() > 1))
                        .unwrap_or_else(|| lane_strip(None, false))
                })
            })
            .w(px(STRIP_WIDTH))
            .h(px(ROW_HEIGHT)),
        )
        .child(root_stripe(stripe_handle, gutter_handle, display, at))
        .child(
            text(move || {
                hash_display.with(|d| {
                    d.row(at.index())
                        .map(|row| short_sha(&row.id))
                        .unwrap_or_default()
                })
            })
            .text_size(px(data::TYPE_CONTROL))
            .text_color(row_link_ink),
        )
        .child(
            text(move || {
                author_display.with(|d| {
                    d.row(at.index())
                        .map(|row| truncate(&row.author.name, 10))
                        .unwrap_or_default()
                })
            })
            .text_size(px(data::TYPE_CONTROL))
            .text_color(row_muted_ink)
            .truncate()
            .w(px(84.0))
            .flex_shrink_0(),
        )
        .child(
            text(move || {
                subject_display.with(|d| {
                    d.row(at.index())
                        .map(|row| truncate(row.subject(), 44))
                        .unwrap_or_default()
                })
            })
            .text_size(px(data::TYPE_BODY))
            .text_color(row_body_ink)
            .truncate()
            .flex_1(),
        )
        .child(ref_pill(handle.clone(), pill_display, at))
        .child(
            text(move || {
                date_display.with(|d| {
                    d.row(at.index())
                        .map(|row| fmt_date(row.time))
                        .unwrap_or_default()
                })
            })
            .text_size(px(data::TYPE_CONTROL))
            .text_color(row_muted_ink)
            .truncate()
            .flex_shrink_0(),
        )
        .test_id(&id_display.with(|d| {
            d.row(at.index())
                .map(|row| test_ids::commit_row(&row.root.name(), &short_sha(&row.id)))
                .unwrap_or_default()
        }))
}

/// The multi-root gutter stripe: one cell per listed root, only when the
/// read set holds more than one repository.
fn root_stripe(
    stripe_handle: AppHandle,
    gutter_handle: AppHandle,
    display: Signal<DisplayWindow>,
    at: Position,
) -> impl IntoElement {
    let cond_handle = gutter_handle;
    let paint_handle = stripe_handle;
    let paint_display = display;
    when(
        move || cond_handle.read(shows_root_gutter),
        move || {
            let paint_handle = paint_handle.clone();
            let paint_display = paint_display;
            view()
                .w(px(data::RAIL_WIDTH))
                .h(px(ROW_HEIGHT))
                .bg(move || {
                    let root =
                        paint_display.with(|d| d.row(at.index()).map(|row| row.root.clone()));
                    let index = paint_handle.read(|s| {
                        root.as_ref()
                            .map(|root| root_index_of(s, root))
                            .unwrap_or(0)
                    });
                    data::LANES[index % data::LANES.len()]
                })
                .into_element()
        },
    )
    .otherwise(|| view().w(px(0.0)))
}

/// Whether the gutter shows: a read set of more than one repository, with
/// the bare path scope's single-root read exempt.
fn shows_root_gutter(state: &AppState) -> bool {
    if state.ui.log_path_scope.is_some()
        && state.ui.log_ref_scope.is_none()
        && state.log_filter_key().is_none()
    {
        return false;
    }
    listing_roots(state).len() > 1
}

fn root_index_of(state: &AppState, root: &RootId) -> usize {
    state
        .multi
        .roots
        .iter()
        .position(|r| &r.id == root)
        .unwrap_or(0)
}

/// The collapsed ref marker: one pill per row with refs; the refs' hover
/// card lands with N4's branch surfaces.
fn ref_pill(handle: AppHandle, display: Signal<DisplayWindow>, at: Position) -> impl IntoElement {
    let t = theme();
    let cond_handle = handle.clone();
    let body_handle = handle;
    let body_display = display;
    let cond_display = body_display;
    when(
        move || {
            let refs = cond_display.with(|d| {
                d.row(at.index())
                    .map(|row| (row.root.clone(), row.id.clone()))
            });
            cond_handle.read(|s| {
                refs.as_ref()
                    .is_some_and(|(root, id)| !s.caches.refs_for(root, id).is_empty())
            })
        },
        move || {
            let body_handle = body_handle.clone();
            let body_display = body_display;
            text(move || {
                let refs = body_display.with(|d| {
                    d.row(at.index())
                        .map(|row| (row.root.clone(), row.id.clone()))
                });
                body_handle.read(|s| {
                    refs.as_ref()
                        .map(|(root, id)| s.caches.refs_for(root, id).len().to_string())
                        .unwrap_or_default()
                })
            })
            .text_size(px(data::TYPE_CHIP))
            .text_color(t.muted_foreground)
        },
    )
}

/// The scope chips over the commits pane: the ref and path scopes with
/// their remove controls.
fn scope_chips(handle: AppHandle, filters: Signal<FilterSnapshot>) -> impl IntoElement {
    let t = theme();
    let remove_ref = handle.clone();
    let remove_path = handle.clone();
    dynamic(move || {
        let snapshot = filters.get();
        let mut children: Vec<AnyElement> = Vec::new();
        if let Some(name) = snapshot.ref_scope.clone() {
            let carriers = snapshot
                .ref_carriers
                .map(|(carriers, selected)| format!(" · {carriers} of {selected} repos"))
                .unwrap_or_default();
            let remove = remove_ref.clone();
            children.push(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .test_id(&test_ids::log("ref-chip"))
                    .child(
                        text(format!("Ref filter: {name}{carriers}"))
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.primary),
                    )
                    .child(
                        view()
                            .role(AccessibilityRole::Button)
                            .label("Remove ref filter")
                            .test_id(&test_ids::log("remove-ref"))
                            .on_press(move || remove.update(|s| s.ui.log_ref_scope = None))
                            .child(text("×").text_size(px(data::TYPE_CONTROL))),
                    )
                    .into_element(),
            );
        }
        if let Some(path) = snapshot.path_scope.clone() {
            let remove = remove_path.clone();
            children.push(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .test_id(&test_ids::log("path-chip"))
                    .child(
                        text(format!("Path filter: {path}"))
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.primary),
                    )
                    .child(
                        view()
                            .role(AccessibilityRole::Button)
                            .label("Remove path filter")
                            .test_id(&test_ids::log("remove-path"))
                            .on_press(move || remove.update(|s| s.ui.log_path_scope = None))
                            .child(text("×").text_size(px(data::TYPE_CONTROL))),
                    )
                    .into_element(),
            );
        }
        view()
            .flex_row()
            .items_center()
            .gap(px(data::SPACING_X))
            .children(children)
    })
}

/// The paging footer: the shown count and the explicit Load more.
fn paging_footer(handle: AppHandle, display: Signal<DisplayWindow>) -> impl IntoElement {
    let t = theme();
    let shown_display = display;
    let more_handle = handle.clone();
    let more_cond = handle.clone();
    view()
        .flex_row()
        .items_center()
        .justify_end()
        .gap(px(data::SPACING_X))
        .h(px(28.0))
        .px(px(data::PANEL_PADDING))
        .border_t(px(1.0))
        .border_color(data::DIVIDER)
        .child(
            text(move || format!("{} shown", shown_display.with(|d| d.len())))
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .test_id(&test_ids::log("shown")),
        )
        .child(when(
            move || more_cond.read(may_have_more),
            move || {
                let more_handle = more_handle.clone();
                button(
                    ButtonProps::new("Load more")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::log("load-more"))
                        .on_press(move || more_handle.update(load_more_windows)),
                )
            },
        ))
}

fn may_have_more(state: &AppState) -> bool {
    listing_roots(state)
        .iter()
        .any(|id| state.caches.log_has_more(id))
}

/// Pages every window the listing reads, the production load-more path.
fn load_more_windows(state: &mut AppState) {
    state.load_more_log();
}

/// The failing or waiting read's slot: the state's label, the reason, and
/// the recovery verbs the state offers.
fn read_slot(
    handle: &AppHandle,
    state_of: impl Fn(&AppState) -> LogState + Clone + 'static,
) -> impl IntoElement {
    let handle = handle.clone();
    let cond_handle = handle.clone();
    when(
        {
            let state_of = state_of.clone();
            move || {
                cond_handle.read(|s| {
                    let state = state_of(s);
                    !state.is_default() && !matches!(state, LogState::Ready)
                })
            }
        },
        move || {
            let t = theme();
            let press_handle = handle.clone();
            let state_of = state_of.clone();
            let (label, reason, recovery) = handle.peek(|s| {
                let state = state_of(s);
                if matches!(state, LogState::NothingCommitted)
                    || matches!(&state, LogState::CommitsFailed(message) if unborn_message(message))
                {
                    return (
                        "This repository has no commits yet.".to_string(),
                        None,
                        state.recovery(),
                    );
                }
                (
                    state.label().to_string(),
                    state.reason().map(str::to_string),
                    state.recovery(),
                )
            });
            view()
                .flex_col()
                .items_center()
                .gap(px(data::SPACING_Y))
                .p(px(data::PANEL_PADDING))
                .child(
                    text(label)
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground)
                        .test_id(&test_ids::log("state")),
                )
                .children(reason.map(|reason| {
                    text(reason)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(data::STATE_ERROR)
                        .into_element()
                }))
                .children(recovery.map(move |verb| {
                    button(
                        ButtonProps::new(verb.label())
                            .variant(ButtonVariant::Secondary)
                            .test_id(&test_ids::log("recovery"))
                            .on_press(move || {
                                press_handle.update_with(|s| apply_log_recovery(s, verb));
                            }),
                    )
                    .into_element()
                }))
        },
    )
}

/// The recovery verb's landing: refresh, reset, or the Commit tab.
fn apply_log_recovery(state: &mut AppState, verb: LogRecovery) {
    match verb {
        LogRecovery::Refresh => state.refresh_log_listing(),
        LogRecovery::ResetFilters => state.reset_log_filters(),
        LogRecovery::CommitLocalChanges => state.ui.tab = turbogit_app::state::Tab::Commit,
    }
}

/// The hidden jump band: the commit the user asked for does not match the
/// current filters, and two verbs say so.
fn hidden_jump_band(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let escape_handle = handle.clone();
    let diff_handle = handle.clone();
    view()
        .flex_col()
        .items_center()
        .gap(px(data::SPACING_Y))
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::log("jump-band"))
        .child(
            text("The commit you asked for does not match the current filters.")
                .text_size(px(data::TYPE_BODY))
                .text_color(t.muted_foreground),
        )
        .child(
            view().flex_row().gap(px(data::SPACING_X)).children([
                button(
                    ButtonProps::new("View and reset filters")
                        .test_id(&test_ids::log("jump-reset"))
                        .on_press(move || {
                            escape_handle.update(|s| {
                                if let Some((root, cid)) = s.ui.log_hidden_jump.clone() {
                                    s.reset_log_filters();
                                    s.ui.log_hidden_jump = None;
                                    s.ui.selected_commit = Some((root, cid.clone()));
                                    s.ui.log_scroll_to = Some(cid);
                                }
                            });
                        }),
                )
                .into_element(),
                button(
                    ButtonProps::new("View in Diff")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::log("jump-diff"))
                        .on_press(move || {
                            diff_handle.update(|s| {
                                if let Some((root, cid)) = s.ui.log_hidden_jump.clone() {
                                    let parent = find_commit(s, &root, &cid)
                                        .and_then(|c| c.parents.first().cloned());
                                    s.ui.diff = Some(DiffTarget::commit(root, parent, cid, None));
                                }
                            });
                        }),
                )
                .into_element(),
            ]),
        )
}

/// The filter bar: the four dropdown popovers and Clear.
fn filter_bar(handle: AppHandle, filters: Signal<FilterSnapshot>) -> impl IntoElement {
    let t = theme();
    let branch_handle = handle.clone();
    let user_handle = handle.clone();
    let date_handle = handle.clone();
    let paths_handle = handle.clone();
    let clear_handle = handle.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .h(px(34.0))
        .px(px(data::PANEL_PADDING))
        .bg(t.card)
        .border_t(px(1.0))
        .border_color(data::DIVIDER)
        .child(branch_dropdown(branch_handle, filters))
        .child(user_dropdown(user_handle, filters))
        .child(date_dropdown(date_handle, filters))
        .child(paths_dropdown(paths_handle, filters))
        .child(clear_button(clear_handle, filters))
}

/// The Clear button: live while any filter holds, and reset means back to
/// rest for the whole filter set, the checked roots included.
fn clear_button(handle: AppHandle, filters: Signal<FilterSnapshot>) -> impl IntoElement {
    let t = theme();
    let (live_fill, idle_fill, ink, hover_fill) = (t.secondary, t.muted, t.foreground, t.accent);
    let press_handle = handle.clone();
    let active_filters = filters;
    view()
        .role(AccessibilityRole::Button)
        .label("Clear")
        .test_id(&test_ids::log("clear"))
        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
        .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(move || {
            if filters_active(&active_filters.get()) {
                live_fill
            } else {
                idle_fill
            }
        })
        .text_size(px(data::TYPE_CONTROL))
        .text_color(ink)
        .hover(move |s| s.bg(hover_fill))
        .on_press(move || press_handle.update(|s| s.reset_log_filters()))
        .child(text("Clear").text_size(px(data::TYPE_CONTROL)))
}

/// Whether any dimension the Clear button resets holds a value.
fn filters_active(snapshot: &FilterSnapshot) -> bool {
    snapshot.ref_scope.is_some()
        || snapshot.author.is_some()
        || snapshot.date.is_some()
        || snapshot.path_scope.is_some()
        || !snapshot.roots_all
        || snapshot.query
}

/// One filter dropdown: the trigger paints `{base}: {value}` while a value
/// is set, and the popover holds the picker's rows.
fn filter_dropdown(
    handle: AppHandle,
    filters: Signal<FilterSnapshot>,
    base: &'static str,
    slug: &'static str,
    value_of: impl Fn(&FilterSnapshot) -> Option<String> + 'static,
    content: impl Fn(AppHandle) -> AnyElement + 'static,
) -> impl IntoElement {
    let t = theme();
    let (live_fill, rest_fill, caret_ink) =
        (t.accent, AdaptiveColor::TRANSPARENT, t.muted_foreground);
    let value_of = Rc::new(value_of);
    let value_for_bg = value_of.clone();
    let trigger_filters = filters;
    let bg_filters = filters;
    let trigger = view()
        .role(AccessibilityRole::Button)
        .label(base)
        .test_id(&test_ids::log(format!("filter-{slug}")))
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .px(px(7.0))
        .py(px(3.0))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(move || {
            if value_for_bg(&bg_filters.get()).is_some() {
                live_fill
            } else {
                rest_fill
            }
        })
        .text_size(px(data::TYPE_CONTROL))
        .text_color(t.muted_foreground)
        .hover(|s| s.bg(t.muted).text_color(t.foreground))
        .child(text(move || match value_of(&trigger_filters.get()) {
            Some(value) => format!("{base}: {value}"),
            None => base.to_string(),
        }))
        .child(crate::ui::icon::icon(
            crate::ui::icon::Icon::ChevronDown,
            data::ICON_SM,
            caret_ink,
        ));
    popover(
        PopoverProps::new(trigger)
            .width(280.0)
            .child(content(handle)),
    )
}

fn branch_dropdown(handle: AppHandle, filters: Signal<FilterSnapshot>) -> impl IntoElement {
    filter_dropdown(
        handle.clone(),
        filters,
        "Branch",
        "branch",
        |snapshot| snapshot.ref_scope.clone(),
        move |handle| branch_menu(handle).into_element(),
    )
}

/// The Branch menu: All, the typed rev, and the ref bands with their fold
/// memory, deduped across the selected repositories.
fn branch_menu(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let all_handle = handle.clone();
    let all_press = handle.clone();
    let rev_text = signal(String::new());
    let input_rev = rev_text;
    let apply_rev = rev_text;
    let apply_handle = handle.clone();
    let bands_handle = handle.clone();
    let bands_rows = handle.clone();

    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(menu_row(
            "All branches",
            move || all_handle.peek(|s| s.ui.log_ref_scope.is_none()),
            move || {
                all_press.update(|s| {
                    s.ui.log_ref_scope = None;
                    s.ui.selected_commit = None;
                    s.ui.log_selected_file = None;
                });
            },
        ))
        .child(
            view().flex_row().gap(px(data::SPACING_X)).children([
                view()
                    .flex_1()
                    .child(input(
                        InputProps::new()
                            .placeholder("Branch name")
                            .accessibility_label("Branch name")
                            .on_change(move |text: &str| input_rev.set(text.to_string()))
                            .test_id(&test_ids::log("branch-input")),
                    ))
                    .into_element(),
                button(
                    ButtonProps::new("Apply")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::log("branch-apply"))
                        .on_press(move || {
                            let text = apply_rev.get().trim().to_string();
                            apply_rev.set(String::new());
                            let handle = apply_handle.clone();
                            handle.update(move |s| set_branch(s, &text));
                        }),
                )
                .into_element(),
            ]),
        )
        .child(each(
            move || {
                bands_rows.read(|s| {
                    let selected = s.ui.log_roots.selected(&s.multi.roots);
                    ref_bands(&selected, s.caches.tags_by_root(), &s.ui.log_ref_folds)
                        .into_iter()
                        .flat_map(|band| {
                            let mut rows = vec![BandRow::Header {
                                title: band.title.to_string(),
                                open: band.open,
                                count: band.revs.len(),
                                group: band.group,
                            }];
                            if band.open {
                                rows.extend(band.revs.iter().map(|rev| BandRow::Rev(rev.clone())));
                            }
                            rows
                        })
                        .collect::<Vec<_>>()
                })
            },
            |row| match row {
                BandRow::Header { title, .. } => format!("header:{title}"),
                BandRow::Rev(rev) => format!("rev:{rev}"),
            },
            move |row, _| match row.get() {
                BandRow::Header {
                    title,
                    open,
                    count,
                    group,
                } => {
                    band_header_row(bands_handle.clone(), title, open, count, group).into_element()
                }
                BandRow::Rev(rev) => rev_row(bands_handle.clone(), rev).into_element(),
            },
        ))
        .bg(t.card)
}

/// One row kind of the Branch menu: a band header or a rev.
#[derive(Clone, PartialEq)]
enum BandRow {
    Header {
        title: String,
        open: bool,
        count: usize,
        group: turbogit_app::log_filters::RefGroup,
    },
    Rev(String),
}

/// A band's header strip: the band name, its count, and the fold toggle.
fn band_header_row(
    handle: AppHandle,
    title: String,
    open: bool,
    count: usize,
    group: turbogit_app::log_filters::RefGroup,
) -> impl IntoElement {
    let t = theme();
    let press_handle = handle;
    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .role(AccessibilityRole::Button)
        .label(title.clone())
        .test_id(&test_ids::log(format!("band-{title}")))
        .on_press(move || {
            press_handle.update(move |s| {
                let fold = if open { Fold::Closed } else { Fold::Open };
                s.ui.log_ref_folds.insert(group, fold);
            });
        })
        .child(
            text(format!("{} ({count})", title.to_uppercase()))
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            text(if open { "▾" } else { "▸" })
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
}

/// One rev row of the Branch menu: the name the scope takes verbatim.
fn rev_row(handle: AppHandle, rev: String) -> impl IntoElement {
    let press_handle = handle.clone();
    let state_handle = handle.clone();
    let rev_for_state = rev.clone();
    let rev_for_id = rev.clone();
    let rev_for_press = rev.clone();
    row(RowProps::new()
        .state_of(move || {
            let active = state_handle
                .peek(|s| s.ui.log_ref_scope.as_deref() == Some(rev_for_state.as_str()));
            if active {
                RowState::Selected
            } else {
                RowState::Default
            }
        })
        .test_id(&test_ids::log(format!("ref-{rev_for_id}")))
        .on_press({
            let rev = rev_for_press.clone();
            move || press_handle.update(|s| set_branch(s, &rev))
        })
        .child(
            text(rev.clone())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::ACCENT_TEXT),
        ))
}

/// A typed rev IS the scope: the read fans out to the carriers at paint.
fn set_branch(state: &mut AppState, rev: &str) {
    if rev.is_empty() {
        state.ui.log_ref_scope = None;
    } else {
        state.ui.log_ref_scope = Some(rev.to_owned());
    }
    state.ui.log_branch_input.clear();
    state.ui.selected_commit = None;
    state.ui.log_selected_file = None;
}

fn user_dropdown(handle: AppHandle, filters: Signal<FilterSnapshot>) -> impl IntoElement {
    filter_dropdown(
        handle.clone(),
        filters,
        "User",
        "user",
        |snapshot| snapshot.author.clone(),
        move |handle| user_menu(handle).into_element(),
    )
}

/// The User menu: All, the typed name, and the authors the loaded windows
/// hold.
fn user_menu(handle: AppHandle) -> impl IntoElement {
    let all_handle = handle.clone();
    let all_press = handle.clone();
    let names_handle = handle.clone();
    let names_rows = handle.clone();
    let submit_handle = handle.clone();
    let input_text = signal(String::new());
    let apply_text = input_text;
    let apply_handle = handle.clone();

    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(menu_row(
            "All users",
            move || all_handle.peek(|s| s.ui.log_author_filter.is_none()),
            move || {
                all_press.update(|s| s.ui.log_author_filter = None);
            },
        ))
        .child(
            view().flex_row().gap(px(data::SPACING_X)).children([
                view()
                    .flex_1()
                    .child(input(
                        InputProps::new()
                            .placeholder("User name")
                            .accessibility_label("User name")
                            .on_change(move |text: &str| input_text.set(text.to_string()))
                            .on_submit(move |text: &str| {
                                let text = text.trim().to_string();
                                apply_text.set(String::new());
                                submit_handle.update(move |s| {
                                    s.ui.log_author_filter = (!text.is_empty()).then_some(text);
                                });
                            })
                            .test_id(&test_ids::log("user-input")),
                    ))
                    .into_element(),
                button(
                    ButtonProps::new("Apply")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::log("user-apply"))
                        .on_press(move || {
                            let text = apply_text.get().trim().to_string();
                            apply_text.set(String::new());
                            apply_handle.update(move |s| {
                                s.ui.log_author_filter = (!text.is_empty()).then_some(text);
                            });
                        }),
                )
                .into_element(),
            ]),
        )
        .child(each(
            move || {
                names_rows.read(|s| {
                    let mut names: Vec<String> = s
                        .multi
                        .roots
                        .iter()
                        .filter_map(|root| s.caches.log(&root.id))
                        .flat_map(|commits| commits.iter().map(|c| c.author.name.clone()))
                        .collect();
                    names.sort();
                    names.dedup();
                    names
                })
            },
            |name| name.clone(),
            move |name, _| {
                let pick_handle = names_handle.clone();
                let ink = theme().card_foreground;
                row(RowProps::new()
                    .state(RowState::Default)
                    .test_id(&test_ids::log(format!("author-{}", name.get())))
                    .on_press(move || {
                        let name = name.get();
                        pick_handle.update(move |s| s.ui.log_author_filter = Some(name));
                    })
                    .child(
                        text(move || SharedString::from(name.get().clone()))
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(ink),
                    ))
            },
        ))
}

fn date_dropdown(handle: AppHandle, filters: Signal<FilterSnapshot>) -> impl IntoElement {
    filter_dropdown(
        handle.clone(),
        filters,
        "Date",
        "date",
        |snapshot| snapshot.date.clone(),
        move |handle| date_menu(handle).into_element(),
    )
}

/// The Date menu: the presets the composite read takes. The picked-day
/// grid lands with the date picker phase (N4's construct).
fn date_menu(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let all_handle = handle.clone();
    let all_press = handle.clone();
    let day_handle = handle.clone();
    let day_cond = handle.clone();
    let week_handle = handle.clone();
    let week_cond = handle.clone();
    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(menu_row(
            "All time",
            move || all_handle.peek(|s| s.ui.log_date_filter.is_all()),
            move || {
                all_press.update(|s| {
                    s.ui.log_date_filter = turbogit_app::log_filters::DateFilter::All;
                });
            },
        ))
        .child(menu_row(
            "Last 24 hours",
            move || {
                matches!(
                    day_cond.peek(|s| s.ui.log_date_filter),
                    turbogit_app::log_filters::DateFilter::Last24Hours { .. }
                )
            },
            move || {
                day_handle.update(|s| {
                    s.ui.log_date_filter = turbogit_app::log_filters::DateFilter::Last24Hours {
                        since: chrono::Utc::now().timestamp() - 86_400,
                    };
                });
            },
        ))
        .child(menu_row(
            "Last 7 days",
            move || {
                matches!(
                    week_cond.peek(|s| s.ui.log_date_filter),
                    turbogit_app::log_filters::DateFilter::Last7Days { .. }
                )
            },
            move || {
                week_handle.update(|s| {
                    s.ui.log_date_filter = turbogit_app::log_filters::DateFilter::Last7Days {
                        since: chrono::Utc::now().timestamp() - 7 * 86_400,
                    };
                });
            },
        ))
        .child(
            text("A picked day arrives with the date picker phase.")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        )
}

fn paths_dropdown(handle: AppHandle, filters: Signal<FilterSnapshot>) -> impl IntoElement {
    filter_dropdown(
        handle.clone(),
        filters,
        "Paths",
        "paths",
        |snapshot| {
            if snapshot.roots_all {
                snapshot.path_scope.clone()
            } else {
                Some(if snapshot.roots_selected == 0 {
                    "No repos".to_string()
                } else {
                    format!(
                        "{} of {} repos",
                        snapshot.roots_selected, snapshot.roots_total
                    )
                })
            }
        },
        move |handle| paths_menu(handle).into_element(),
    )
}

/// The Paths menu: the typed path scope and the Roots block, a set the
/// rows toggle without closing.
fn paths_menu(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let any_handle = handle.clone();
    let any_press = handle.clone();
    let submit_handle = handle.clone();
    let input_text = signal(String::new());
    let apply_text = input_text;
    let apply_handle = handle.clone();
    let roots_handle = handle.clone();
    let roots_rows = handle.clone();
    let all_roots_cond = handle.clone();
    let all_roots_press = handle.clone();

    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(menu_row(
            "Any path",
            move || any_handle.peek(|s| s.ui.log_path_scope.is_none()),
            move || {
                any_press.update(|s| s.ui.log_path_scope = None);
            },
        ))
        .child(
            view().flex_row().gap(px(data::SPACING_X)).children([
                view()
                    .flex_1()
                    .child(input(
                        InputProps::new()
                            .placeholder("Path")
                            .accessibility_label("Path")
                            .on_change(move |text: &str| input_text.set(text.to_string()))
                            .on_submit(move |text: &str| {
                                let text = text.trim().to_string();
                                apply_text.set(String::new());
                                submit_handle.update(move |s| set_path_scope(s, &text));
                            })
                            .test_id(&test_ids::log("path-input")),
                    ))
                    .into_element(),
                button(
                    ButtonProps::new("Apply")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::log("path-apply"))
                        .on_press(move || {
                            let text = apply_text.get().trim().to_string();
                            apply_text.set(String::new());
                            apply_handle.update(move |s| set_path_scope(s, &text));
                        }),
                )
                .into_element(),
            ]),
        )
        .child(
            text("Roots")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(menu_row(
            "All repositories",
            move || all_roots_cond.peek(|s| s.ui.log_roots.is_all()),
            move || {
                all_roots_press.update(|s| s.ui.log_roots.select_all());
            },
        ))
        .child(each(
            move || {
                roots_rows.read(|s| {
                    s.multi
                        .roots
                        .iter()
                        .map(|root| (root.id.name(), root.id.clone()))
                        .collect::<Vec<_>>()
                })
            },
            |(name, _)| name.clone(),
            move |item, _| {
                let t = theme();
                let press_handle = roots_handle.clone();
                let state_handle = roots_handle.clone();
                let name = item.get().0.clone();
                let root_id = item.get().1.clone();
                let root_for_state = root_id.clone();
                row(RowProps::new()
                    .state_of(move || {
                        let carries =
                            state_handle.peek(|s| s.ui.log_roots.carries(&root_for_state));
                        if carries {
                            RowState::Selected
                        } else {
                            RowState::Default
                        }
                    })
                    .test_id(&test_ids::log(format!("root-{name}")))
                    .on_press({
                        let root_id = root_id.clone();
                        move || {
                            press_handle.update(|s| {
                                s.ui.log_roots.toggle(&root_id, &s.multi.roots);
                            });
                        }
                    })
                    .child(
                        text(move || SharedString::from(item.get().0.clone()))
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.card_foreground),
                    ))
            },
        ))
}

/// A typed path names no repository: read the one the selection is about,
/// and fall back to the first registered root at rest.
fn set_path_scope(state: &mut AppState, text: &str) {
    if text.is_empty() {
        state.ui.log_path_scope = None;
        return;
    }
    let root = state
        .ui
        .selected_commit
        .as_ref()
        .map(|(root, _)| root.clone())
        .or_else(|| state.multi.roots.first().map(|root| root.id.clone()));
    state.ui.log_path_scope = root.map(|root| (root, std::path::PathBuf::from(text)));
}

/// One quiet menu row: the label, its active fill, and its press.
fn menu_row(
    label: &'static str,
    active: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let t = theme();
    let (live_fill, rest_fill, ink, hover_fill) = (t.accent, t.card, t.card_foreground, t.accent);
    view()
        .flex_row()
        .items_center()
        .role(AccessibilityRole::Button)
        .label(label)
        .test_id(&test_ids::log(format!("menu-{label}")))
        .px(px(data::DENSITY_DENSE_BUTTON_PAD_X))
        .py(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(move || if active() { live_fill } else { rest_fill })
        .text_size(px(data::TYPE_CONTROL))
        .text_color(ink)
        .hover(move |s| s.bg(hover_fill))
        .on_press(on_press)
        .child(text(label).text_size(px(data::TYPE_CONTROL)))
}

/// The branches pane: the search input and the tree of every registered
/// root's refs.
fn branches_pane(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let search_handle = handle.clone();
    let tree_handle = handle.clone();
    let tree_filter = signal(String::new());
    let input_filter = tree_filter;
    view()
        .flex_1()
        .flex_col()
        .bg(t.card)
        .child(pane_header(PaneHeaderProps::new("Branches")))
        .child(
            view()
                .px(px(data::PANEL_PADDING))
                .py(px(data::SPACING_Y))
                .child(
                    input(
                        InputProps::new()
                            .placeholder("Search branches")
                            .accessibility_label("Search branches")
                            .on_change(move |text: &str| {
                                let text = text.to_string();
                                input_filter.set(text.clone());
                                search_handle.update(move |s| s.ui.log_branch_filter = text);
                            })
                            .test_id(&test_ids::log("branch-search")),
                    )
                    .into_element(),
                ),
        )
        .child(
            view()
                .flex_1()
                .px(px(6.0))
                .pb(px(6.0))
                .child(dynamic(move || {
                    branch_tree(tree_handle.clone(), tree_filter)
                })),
        )
}

/// The tree the branches pane paints: one entry per root, its local
/// branches, remotes, and tags under group bands, filtered by the search.
/// A branch row's press names that root's branch: the name goes into the
/// scope and the selection narrows to that root.
fn branch_tree(handle: AppHandle, filter_signal: Signal<String>) -> impl IntoElement {
    let t = theme();
    let filter = filter_signal.get().trim().to_lowercase();
    let (items, expansions) = handle.peek(|s| {
        let tags = s.caches.tags_by_root().clone();
        let mut expansions: Vec<SharedString> = Vec::new();
        let items = s
            .multi
            .roots
            .iter()
            .map(|root| {
                expansions.push(root.id.0.display().to_string().into());
                expansions.push(format!("{}:group:local", root.id.name()).into());
                let mut root_entry =
                    TreeEntryProps::new(root.id.0.display().to_string(), root.id.name());
                let local: Vec<String> = root
                    .branches
                    .iter()
                    .filter(|b| b.kind == BranchKind::Local)
                    .map(|b| b.name.clone())
                    .filter(|name| filter.is_empty() || name.to_lowercase().contains(&filter))
                    .collect();
                if !local.is_empty() {
                    let mut group = TreeEntryProps::new(
                        format!("{}:group:local", root.id.name()),
                        "Local branches".to_string(),
                    );
                    for name in local {
                        group = group.child(TreeEntryProps::new(
                            format!("{}:branch:{name}", root.id.name()),
                            name,
                        ));
                    }
                    root_entry = root_entry.child(group);
                }
                let remote: Vec<String> = root
                    .branches
                    .iter()
                    .filter(|b| b.kind == BranchKind::Remote)
                    .map(|b| match &b.remote {
                        Some(remote) => format!("{remote}/{}", b.name),
                        None => b.name.clone(),
                    })
                    .filter(|name| filter.is_empty() || name.to_lowercase().contains(&filter))
                    .collect();
                if !remote.is_empty() {
                    let mut group = TreeEntryProps::new(
                        format!("{}:group:remote", root.id.name()),
                        "Remote branches".to_string(),
                    );
                    for name in remote {
                        group = group.child(TreeEntryProps::new(
                            format!("{}:branch:{name}", root.id.name()),
                            name,
                        ));
                    }
                    root_entry = root_entry.child(group);
                }
                let tag_names: Vec<String> = tags
                    .get(&root.id)
                    .map(|tags| tags.iter().map(|(name, _)| name.clone()).collect())
                    .unwrap_or_default();
                let tags: Vec<String> = tag_names
                    .into_iter()
                    .filter(|name| filter.is_empty() || name.to_lowercase().contains(&filter))
                    .collect();
                if !tags.is_empty() {
                    let mut group = TreeEntryProps::new(
                        format!("{}:group:tags", root.id.name()),
                        "Tag refs".to_string(),
                    );
                    for name in tags {
                        group = group.child(TreeEntryProps::new(
                            format!("{}:branch:{name}", root.id.name()),
                            name,
                        ));
                    }
                    root_entry = root_entry.child(group);
                }
                root_entry
            })
            .collect::<Vec<_>>();
        (items, expansions)
    });
    let mut props = TreeProps::new(items).accessibility_label("Branches");
    for id in expansions {
        props = props.default_expanded(id);
    }
    let select_handle = handle.clone();
    props = props.on_select(move |id| {
        select_handle.update(|s| select_branch_row(s, id));
    });
    // The GUI tree paints the app's row; this is where the depth indent,
    // the branch mark, and the group label's quieter ink come from.
    let ink = t;
    props = props.render_item(move |entry: &nsh_base::TreeEntry| {
        let depth = entry.depth as f32;
        let (mark, size, color) = match entry.depth {
            0 => (Icon::Folder, data::ICON_SM, ink.foreground),
            1 => (Icon::Layers, data::ICON_SM, ink.muted_foreground),
            _ => (Icon::Branch, data::ICON_SM, ink.foreground),
        };
        view()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .pl(px(depth * data::INDENT))
            .child(crate::ui::icon::icon(mark, size, color))
            .child(
                text(entry.label.clone())
                    .text_size(px(match entry.depth {
                        0 => data::TYPE_BODY,
                        1 => data::TYPE_CHIP,
                        _ => data::TYPE_BODY,
                    }))
                    .font_weight(match entry.depth {
                        0 => FontWeight::SEMIBOLD,
                        1 => FontWeight::SEMIBOLD,
                        _ => FontWeight::NORMAL,
                    })
                    .text_color(color),
            )
            .into_element()
    });
    tree(props)
}

/// Selects a branch row: the name becomes the ref scope and the read set
/// narrows to that repository, the egui `apply_log_tree_event`. Group and
/// repository rows are not branches and do nothing.
fn select_branch_row(state: &mut AppState, id: &str) {
    let Some((root, name)) = state.multi.roots.iter().find_map(|root| {
        id.strip_prefix(&format!("{}:branch:", root.id.name()))
            .map(|name| (root.id.clone(), name))
    }) else {
        return;
    };
    if name.is_empty() {
        return;
    }
    state.ui.log_ref_scope = Some(name.to_string());
    state.ui.log_roots = turbogit_app::log_filters::RootFilter::Only(vec![root]);
    state.ui.selected_commit = None;
    state.ui.log_selected_file = None;
}

/// The right column: the changed files above, the commit details below.
fn right_column(handle: AppHandle) -> impl IntoElement {
    let files_handle = handle.clone();
    let details_handle = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .child(view().flex_1().child(files_pane(files_handle)))
        .child(
            view()
                .h(px(280.0))
                .border_t(px(1.0))
                .border_color(data::DIVIDER)
                .child(details_pane(details_handle)),
        )
}

/// The changed-files pane: the selected commit's files with their status
/// ink, the churn counts, and the file row's verbs.
fn files_pane(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let pane_muted_ink = t.muted_foreground;
    let count_handle = handle.clone();
    let filter_handle = handle.clone();
    let rows_handle = handle.clone();
    let rows_items = handle.clone();
    let empty_handle = handle.clone();
    let selection_handle = handle.clone();

    view()
        .flex_1()
        .flex_col()
        .bg(t.card)
        .child(pane_header(PaneHeaderProps::new("Changed files").count_of(
            move || {
                count_handle.read(|s| {
                    s.ui.selected_commit
                        .as_ref()
                        .and_then(|(root, cid)| {
                            s.caches
                                .files_for(root, cid)
                                .map(|files| files.len().to_string())
                        })
                        .unwrap_or_default()
                })
            },
        )))
        .child(
            view()
                .px(px(data::PANEL_PADDING))
                .py(px(data::SPACING_Y))
                .child(
                    input(
                        InputProps::new()
                            .placeholder("Filter changed files")
                            .accessibility_label("Filter changed files")
                            .on_change(move |text: &str| {
                                let text = text.to_string();
                                filter_handle.update(move |s| s.ui.log_file_filter = text);
                            })
                            .test_id(&test_ids::log("file-filter")),
                    )
                    .into_element(),
                ),
        )
        .child(when(
            {
                let selection_handle = selection_handle;
                move || selection_handle.read(|s| s.ui.selected_commit.is_none())
            },
            move || {
                text("Select a commit to see its changed files.")
                    .text_size(px(data::TYPE_BODY))
                    .text_color(pane_muted_ink)
            },
        ))
        .child(each(
            move || {
                rows_items.read(|s| {
                    let filter = s.ui.log_file_filter.trim().to_lowercase();
                    let selected = s.ui.log_selected_file.clone();
                    let (root, files, stats) = match s.ui.selected_commit.as_ref() {
                        Some((root, cid)) => (
                            root.clone(),
                            s.caches.files_for(root, cid).map(|files| files.to_vec()),
                            s.caches
                                .file_stats_for(root, cid)
                                .map(|stats| stats.to_vec()),
                        ),
                        None => (
                            RootId(std::sync::Arc::from(std::path::Path::new(""))),
                            None,
                            None,
                        ),
                    };
                    files
                        .map(|files| {
                            files
                                .iter()
                                .filter(|change| {
                                    filter.is_empty()
                                        || change
                                            .path
                                            .to_string_lossy()
                                            .to_lowercase()
                                            .contains(&filter)
                                })
                                .map(|change| {
                                    let stat = stats
                                        .as_deref()
                                        .and_then(|stats| file_stat(stats, &change.path));
                                    FileRowData {
                                        path: change.path.clone(),
                                        dir: change
                                            .path
                                            .parent()
                                            .filter(|p| !p.as_os_str().is_empty())
                                            .map(|p| p.display().to_string())
                                            .unwrap_or_default(),
                                        short: change.status.short().to_string(),
                                        status: change.status,
                                        selected: selected.as_ref().is_some_and(
                                            |(sel_root, sel_path)| {
                                                sel_path == &change.path && sel_root == &root
                                            },
                                        ),
                                        stat,
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
            },
            |row| row.path.display().to_string(),
            move |row, _| file_row(rows_handle.clone(), row.get()),
        ))
        .child(when(
            {
                let empty_handle = empty_handle;
                move || {
                    empty_handle.read(|s| {
                        s.ui.selected_commit.is_some()
                            && s.ui
                                .selected_commit
                                .as_ref()
                                .and_then(|(root, cid)| s.caches.files_for(root, cid))
                                .is_some_and(|files| files.is_empty())
                    })
                }
            },
            move || {
                text("No changed files.")
                    .text_size(px(data::TYPE_BODY))
                    .text_color(pane_muted_ink)
            },
        ))
}

/// One changed-file row the each reads: owned data, so the rows compare
/// across ticks.
#[derive(Clone, PartialEq)]
struct FileRowData {
    path: std::path::PathBuf,
    dir: String,
    short: String,
    status: ChangeStatus,
    selected: bool,
    stat: Option<(usize, usize)>,
}

/// One changed-file row: the status ink, the name, its directory, the
/// churn counts, and the row's verbs.
fn file_row(handle: AppHandle, row_data: FileRowData) -> impl IntoElement {
    let t = theme();
    let select_handle = handle.clone();
    let diff_handle = handle.clone();
    let blame_handle = handle.clone();
    let menu_handle = handle.clone();
    let menu_handle_2 = handle.clone();
    let FileRowData {
        path,
        dir,
        short,
        status,
        selected,
        stat,
    } = row_data;
    let (added, removed) = stat.unwrap_or((0, 0));
    let status_ink = match status {
        ChangeStatus::Added => data::STATUS_ADDED,
        ChangeStatus::Deleted => data::STATUS_DELETED,
        ChangeStatus::Modified => data::STATUS_MODIFIED,
        _ => data::STATUS_UNVERSIONED,
    };
    let path_display = path.display().to_string();
    let body_ink = t.card_foreground;
    let muted_ink = t.muted_foreground;
    let root_name = handle.peek(|s| {
        s.ui.selected_commit
            .as_ref()
            .map(|(root, _)| root.name())
            .unwrap_or_default()
    });
    let row_element = row(RowProps::new()
        .state(if selected {
            RowState::Selected
        } else {
            RowState::Default
        })
        .test_id(&test_ids::file_row(&root_name, &path_display))
        .on_press({
            let path = path.clone();
            move || {
                let path = path.clone();
                select_handle.update(move |s| {
                    if let Some((root, _)) = s.ui.selected_commit.clone() {
                        s.ui.log_selected_file = Some((root, path));
                    }
                });
            }
        })
        .on_context_menu({
            let path = path.clone();
            move |event| {
                let change = turbogit_domain::model::Change {
                    path: path.clone(),
                    ..test_change_for_menu()
                };
                open_file_menu(menu_handle.clone(), change, event.page_position);
            }
        })
        .child(
            text(short.clone())
                .text_size(px(data::TYPE_CHIP))
                .text_color(status_ink),
        )
        .child(
            text(path_display.clone())
                .text_size(px(data::TYPE_BODY))
                .text_color(body_ink),
        )
        .child(
            text(dir.clone())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(muted_ink),
        )
        .child(
            text(if added > 0 {
                format!("+{added}")
            } else {
                Default::default()
            })
            .text_size(px(data::TYPE_CONTROL))
            .text_color(data::STATE_SUCCESS),
        )
        .child(
            text(if removed > 0 {
                format!("−{removed}")
            } else {
                Default::default()
            })
            .text_size(px(data::TYPE_CONTROL))
            .text_color(data::STATE_ERROR),
        ));
    view().flex_col().gap(px(2.0)).child(row_element).child(
        view().flex_row().gap(px(data::SPACING_X)).children([
            button(
                ButtonProps::new("Blame")
                    .variant(ButtonVariant::Ghost)
                    .size(crate::ui::ControlSize::Compact)
                    .test_id(&test_ids::log("file-blame"))
                    .on_press({
                        let path = path.clone();
                        move || {
                            let path = path.clone();
                            blame_handle.update(move |s| {
                                if let Some((root, cid)) = s.ui.selected_commit.clone() {
                                    s.ui.blame = Some(BlameTarget {
                                        root,
                                        path: path.clone(),
                                        rev: cid,
                                    });
                                    s.ui.blame_caret_line = None;
                                }
                            });
                        }
                    }),
            )
            .into_element(),
            button(
                ButtonProps::new("Open diff")
                    .variant(ButtonVariant::Ghost)
                    .size(crate::ui::ControlSize::Compact)
                    .test_id(&test_ids::log("file-diff"))
                    .on_press({
                        let path = path.clone();
                        move || {
                            let path = path.clone();
                            diff_handle.update(move |s| {
                                if let Some((root, cid)) = s.ui.selected_commit.clone() {
                                    let parent = find_commit(s, &root, &cid)
                                        .and_then(|c| c.parents.first().cloned());
                                    s.ui.log_selected_file = Some((root.clone(), path.clone()));
                                    s.ui.diff =
                                        Some(DiffTarget::commit(root, parent, cid, Some(path)));
                                }
                            });
                        }
                    }),
            )
            .into_element(),
            button(
                ButtonProps::new("Full path history")
                    .variant(ButtonVariant::Ghost)
                    .size(crate::ui::ControlSize::Compact)
                    .test_id(&test_ids::log("file-history"))
                    .on_press({
                        let path = path.clone();
                        move || {
                            let path = path.clone();
                            menu_handle_2.update(move |s| {
                                if let Some((root, _)) = s.ui.selected_commit.clone() {
                                    s.ui.log_path_scope = Some((root, path));
                                    s.ui.selected_commit = None;
                                    s.ui.log_selected_file = None;
                                }
                            });
                        }
                    }),
            )
            .into_element(),
        ]),
    )
}

/// The change shape the file row's menu opens on: the menu reads the path
/// and the selection, nothing else the status carries.
fn test_change_for_menu() -> turbogit_domain::model::Change {
    turbogit_domain::model::Change {
        path: std::path::PathBuf::new(),
        status: ChangeStatus::Modified,
        chunks: Vec::new(),
        staged: false,
        unstaged: false,
        orig_path: None,
    }
}

/// The commit-details pane: the subject, the hash, the fact rows, the
/// churn summary, the body, and the parent link.
fn details_pane(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let selection_handle = handle.clone();
    let subject_handle = handle.clone();
    let facts_handle = handle.clone();
    let churn_handle = handle.clone();
    let body_handle = handle.clone();
    let parent_press = handle.clone();
    let parent_cond = handle.clone();
    let detail_ink = t.card_foreground;
    let detail_muted_ink = t.muted_foreground;

    view()
        .flex_1()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .bg(t.card)
        .p(px(8.0))
        .child(pane_header(PaneHeaderProps::new("Commit details")))
        .child(when(
            {
                let selection_handle = selection_handle;
                move || selection_handle.read(|s| s.ui.selected_commit.is_none())
            },
            move || {
                text("Select a commit…")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(detail_muted_ink)
            },
        ))
        .child(
            text(move || {
                subject_handle.read(|s| {
                    s.ui.selected_commit
                        .as_ref()
                        .and_then(|(root, cid)| find_commit(s, root, cid))
                        .map(|commit| commit.subject().to_string())
                        .unwrap_or_default()
                })
            })
            .text_size(px(data::TYPE_DETAIL_TITLE))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(detail_ink),
        )
        .child(
            text(move || {
                facts_handle.read(|s| {
                    s.ui.selected_commit
                        .as_ref()
                        .and_then(|(root, cid)| find_commit(s, root, cid))
                        .map(|commit| {
                            format!(
                                "{} · Author {} <{}> · Date {}\nCommitter {} <{}> · Parents {}",
                                short_sha(&commit.id),
                                commit.author.name,
                                commit.author.email,
                                fmt_time(commit.time),
                                commit.committer.name,
                                commit.committer.email,
                                if commit.parents.is_empty() {
                                    "—".to_string()
                                } else {
                                    commit
                                        .parents
                                        .iter()
                                        .map(|p| short_sha(p))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                },
                            )
                        })
                        .unwrap_or_default()
                })
            })
            .text_size(px(data::TYPE_BODY))
            .text_color(t.card_foreground),
        )
        .child(
            text(move || {
                churn_handle.read(|s| {
                    s.ui.selected_commit
                        .as_ref()
                        .and_then(|(root, cid)| s.caches.file_stats_for(root, cid))
                        .map(|stats| {
                            let added: usize = stats.iter().filter_map(|f| f.insertions).sum();
                            let removed: usize = stats.iter().filter_map(|f| f.deletions).sum();
                            format!("+{added} −{removed}")
                        })
                        .unwrap_or_default()
                })
            })
            .text_size(px(data::TYPE_CONTROL))
            .text_color(data::STATE_SUCCESS)
            .test_id(&test_ids::log("churn")),
        )
        .child(
            text(move || {
                body_handle.read(|s| {
                    s.ui.selected_commit
                        .as_ref()
                        .and_then(|(root, cid)| find_commit(s, root, cid))
                        .map(|commit| {
                            commit
                                .message
                                .lines()
                                .skip(1)
                                .filter(|line| !line.trim().is_empty())
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default()
                })
            })
            .text_size(px(data::TYPE_BODY))
            .text_color(detail_ink),
        )
        .child(when(
            move || {
                parent_cond.read(|s| {
                    s.ui.selected_commit
                        .as_ref()
                        .and_then(|(root, cid)| find_commit(s, root, cid))
                        .is_some_and(|commit| !commit.parents.is_empty())
                })
            },
            move || {
                let parent_handle = parent_press.clone();
                button(
                    ButtonProps::new("Select parent")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::log("select-parent"))
                        .on_press(move || {
                            parent_handle.update(|s| {
                                if let Some((root, cid)) = s.ui.selected_commit.clone()
                                    && let Some(parent) = find_commit(s, &root, &cid)
                                        .and_then(|c| c.parents.first().cloned())
                                {
                                    s.ui.selected_commit = Some((root.clone(), parent.clone()));
                                    s.ui.log_selected_file = None;
                                    s.ui.log_scroll_to = Some(parent);
                                }
                            });
                        }),
                )
                .into_element()
            },
        ))
}

/// Opens the commit context menu at the pointer: the facts the gating
/// reads are the root's own at menu-open time.
fn open_commit_menu(handle: AppHandle, root: RootId, cid: String, at: Point<Points>) {
    handle.update(|s| {
        s.ui.selected_commit = Some((root.clone(), cid.clone()));
        s.ui.log_commit_menu = Some((root.clone(), cid.clone()));
    });
    let items = commit_menu_items(&handle, &root, &cid);
    spawn(async move {
        let _ = nsh_core::element::desktop::popup_menu_at(items, at).await;
    });
}

/// The file row's menu: blame, history, create patch.
fn open_file_menu(handle: AppHandle, change: turbogit_domain::model::Change, at: Point<Points>) {
    use nsh_core::element::desktop::MenuItem;
    let path = change.path.clone();
    let items = vec![
        MenuItem::new("Show blame").on_select({
            let handle = handle.clone();
            let path = path.clone();
            move || {
                handle.update(|s| {
                    if let Some((root, cid)) = s.ui.selected_commit.clone() {
                        s.ui.blame = Some(BlameTarget {
                            root,
                            path: path.clone(),
                            rev: cid,
                        });
                        s.ui.blame_caret_line = None;
                    }
                });
            }
        }),
        MenuItem::new("Show history for file…").on_select({
            let handle = handle.clone();
            let path = path.clone();
            move || {
                handle.update(|s| {
                    if let Some((root, _)) = s.ui.selected_commit.clone() {
                        s.ui.log_path_scope = Some((root, path.clone()));
                        s.ui.selected_commit = None;
                        s.ui.log_selected_file = None;
                    }
                });
            }
        }),
        MenuItem::new("Create patch").on_select({
            let handle = handle.clone();
            let path = path.clone();
            move || {
                handle.update(|s| {
                    if let Some((root, cid)) = s.ui.selected_commit.clone() {
                        turbogit_app::create_patch::open_on(
                            s,
                            root,
                            PatchOrigin::FileRevision {
                                commit: cid,
                                path: path.clone(),
                            },
                        );
                    }
                });
            }
        }),
    ];
    spawn(async move {
        let _ = nsh_core::element::desktop::popup_menu_at(items, at).await;
    });
}

/// The commit context menu's items with the gating the facts carry: the
/// twelve verbs in the egui order, a disabled item keeping its reason.
fn commit_menu_items(handle: &AppHandle, root: &RootId, cid: &str) -> Vec<DesktopMenuItem> {
    let facts = handle.peek(|s| {
        find_commit(s, root, cid).map(|commit| (commit.clone(), commit_facts(s, root, cid, commit)))
    });
    let Some((commit, facts)) = facts else {
        return Vec::new();
    };
    let dirty_reason = facts
        .dirty
        .then_some("Resolve the uncommitted changes first");
    let protected_reason =
        (facts.protected_branch && !facts.dirty).then_some("the current branch is protected");
    let across_reason = if !facts.multi_root {
        Some("only one repository is open")
    } else {
        dirty_reason
    };

    let rewrite = |_verb: RewriteVerb| -> Option<&'static str> { blocked(&facts, &commit) };

    let apply_handle = handle.clone();
    let root = root.clone();
    let cid = cid.to_string();
    let apply = move |action: CommitMenuAction| {
        let handle = apply_handle.clone();
        let root = root.clone();
        let cid = cid.clone();
        handle.update(move |s| apply_commit_action(s, root, cid, action));
    };
    let apply = Rc::new(apply);

    vec![
        menu_item(
            "Copy hash",
            dirty_reason,
            apply.clone(),
            CommitMenuAction::CopyHash,
        ),
        menu_item(
            "Copy commit message",
            None,
            apply.clone(),
            CommitMenuAction::CopyMessage,
        ),
        menu_item(
            "Create patch",
            None,
            apply.clone(),
            CommitMenuAction::CreatePatch,
        ),
        menu_item(
            "Cherry-pick to…",
            dirty_reason,
            apply.clone(),
            CommitMenuAction::CherryPickTo,
        ),
        menu_item(
            "Cherry-pick across…",
            across_reason,
            apply.clone(),
            CommitMenuAction::CherryPickAcross,
        ),
        menu_item(
            "Checkout",
            dirty_reason,
            apply.clone(),
            CommitMenuAction::Checkout,
        ),
        menu_item(
            "Revert commit",
            protected_reason,
            apply.clone(),
            CommitMenuAction::RevertCommit,
        ),
        menu_item(
            "Drop commit",
            rewrite(RewriteVerb::Drop),
            apply.clone(),
            CommitMenuAction::DropCommit,
        ),
        menu_item(
            "Reword commit",
            rewrite(RewriteVerb::Reword),
            apply.clone(),
            CommitMenuAction::RewordCommit,
        ),
        menu_item(
            "New branch",
            None,
            apply.clone(),
            CommitMenuAction::NewBranch,
        ),
        menu_item("New tag", None, apply.clone(), CommitMenuAction::NewTag),
        menu_item(
            "Interactive rebase from here…",
            rewrite(RewriteVerb::Interactive),
            apply,
            CommitMenuAction::InteractiveRebase,
        ),
    ]
}

type DesktopMenuItem = nsh_core::element::desktop::MenuItem;

fn menu_item(
    label: &'static str,
    disabled_reason: Option<&'static str>,
    apply: Rc<dyn Fn(CommitMenuAction)>,
    action: CommitMenuAction,
) -> DesktopMenuItem {
    match disabled_reason {
        Some(_) => DesktopMenuItem::new(label).disabled(true),
        None => {
            let apply = apply.clone();
            DesktopMenuItem::new(label).on_select(move || apply(action))
        }
    }
}

/// The facts the commit menu's gating reads, the egui `commit_facts`.
struct CommitFacts {
    dirty: bool,
    protected_branch: bool,
    on_current_branch: bool,
    multi_root: bool,
    owner: Option<OwnedOperation>,
    scope: Scope,
}

#[derive(Clone, Copy)]
enum RewriteVerb {
    Drop,
    Reword,
    Interactive,
}

enum Scope {
    Own,
    Several,
    External,
}

fn commit_facts(state: &AppState, root: &RootId, cid: &str, commit: &Commit) -> CommitFacts {
    let _ = commit;
    let snapshot = state.multi.by_id(root);
    let selection: std::collections::HashSet<RootId> =
        state.selected_change_roots().into_iter().collect();
    let scope = if selection.is_empty() || selection.contains(root) {
        Scope::Own
    } else if selection.len() > 1 {
        Scope::Several
    } else {
        Scope::External
    };
    CommitFacts {
        dirty: snapshot.is_some_and(|r| !r.status.changes.is_empty()),
        protected_branch: snapshot
            .and_then(|r| r.current_branch.clone())
            .is_some_and(|b| sync_service::is_protected(&state.settings, &b)),
        on_current_branch: snapshot.is_some_and(|r| r.current_branch.is_some())
            && state
                .caches
                .log(root)
                .is_some_and(|cs| cs.iter().any(|c| c.id == cid)),
        multi_root: state.multi.roots.len() > 1,
        owner: state.holding_operation(root.as_path()),
        scope,
    }
}

/// The gating a rewrite verb is refused with, the egui
/// `rewrite_gating::blocked`.
fn blocked(facts: &CommitFacts, commit: &Commit) -> Option<&'static str> {
    if facts.owner.is_some() {
        return Some("the repository holds an operation");
    }
    if commit.parents.len() > 1 {
        return Some("a merge commit cannot be rewritten");
    }
    if commit.parents.is_empty() {
        return Some("a root commit cannot be rewritten");
    }
    if !facts.on_current_branch {
        return Some("not on the current branch");
    }
    match facts.scope {
        Scope::Several => Some("the rewrite runs in one repository; several are in scope"),
        Scope::External => Some("the commit is in a repository outside the ones in scope"),
        Scope::Own => {
            if facts.protected_branch {
                Some("the current branch is protected")
            } else {
                None
            }
        }
    }
}

#[derive(Clone, Copy)]
enum CommitMenuAction {
    CopyHash,
    CopyMessage,
    CreatePatch,
    CherryPickTo,
    CherryPickAcross,
    Checkout,
    RevertCommit,
    DropCommit,
    RewordCommit,
    NewBranch,
    NewTag,
    InteractiveRebase,
}

/// Runs the menu verb the row named, the egui `apply_commit_action`.
fn apply_commit_action(state: &mut AppState, root: RootId, cid: String, action: CommitMenuAction) {
    state.ui.log_scroll_to = Some(cid.clone());
    match action {
        CommitMenuAction::CopyHash => {
            nsh_core::element::set_clipboard(cid.clone());
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success(format!("Copied {}", short_sha(&cid))));
        }
        CommitMenuAction::CopyMessage => {
            let message = find_commit(state, &root, &cid)
                .map(|c| c.message.clone())
                .unwrap_or_default();
            nsh_core::element::set_clipboard(message);
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success("Copied commit message"));
        }
        CommitMenuAction::CreatePatch => {
            turbogit_app::create_patch::open_on(
                state,
                root.clone(),
                PatchOrigin::Commit { id: cid.clone() },
            );
        }
        CommitMenuAction::CherryPickTo => {
            state.ui.dlg.cherry_pick_commit = Some((root.clone(), cid.clone()));
            state.ui.dialog = Some(turbogit_app::state::Dialog::CherryPickTarget);
        }
        CommitMenuAction::CherryPickAcross => {
            state.open_cherry_across(root.clone());
            state.ui.dialog = Some(turbogit_app::state::Dialog::CherryPickAcross);
        }
        CommitMenuAction::RevertCommit => {
            state.ui.confirm = Some(PendingConfirm::RevertCommit {
                root: root.clone(),
                commit: cid.clone(),
            });
        }
        CommitMenuAction::NewBranch => {
            state.open_new_branch(
                Some(root.clone()),
                NewBranchBase::Commit(cid.clone()),
                false,
                None,
            );
        }
        CommitMenuAction::Checkout => {
            let dirty = state
                .multi
                .by_id(&root)
                .is_some_and(|r| !r.status.changes.is_empty());
            if dirty {
                state.ui.confirm = Some(PendingConfirm::CheckoutDirty {
                    root: root.clone(),
                    target: cid.clone(),
                    kind: turbogit_domain::model::BranchKind::Local,
                    detach: true,
                });
            } else {
                state.checkout_detached_op(&root, &cid);
            }
        }
        CommitMenuAction::NewTag => {
            state.open_tag_dialog(Some(root.clone()), Some(&cid));
        }
        CommitMenuAction::DropCommit => {
            state.open_rewrite_preflight(&root, &cid, turbogit_app::state::HistoryVerb::Drop);
        }
        CommitMenuAction::RewordCommit => {
            state.open_reword_editor(&root, &cid);
        }
        CommitMenuAction::InteractiveRebase => {
            state.open_interactive_rebase(&root, &cid);
        }
    }
}

/// The short hash the log paints: seven chars, the egui `short_commit_ref`.
fn short_sha(id: &str) -> String {
    id.chars().take(7).collect()
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn fmt_time(t: i64) -> String {
    use chrono::TimeZone;
    match chrono::Utc.timestamp_opt(t, 0) {
        chrono::LocalResult::Single(dt) => {
            let local: chrono::DateTime<chrono::Local> = chrono::DateTime::from(dt);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        _ => String::new(),
    }
}

fn fmt_date(t: i64) -> String {
    fmt_time(t)
}
