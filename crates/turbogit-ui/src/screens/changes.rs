//! The Changes surface: the Commit tab's Local Changes list. One row set
//! keyed by root and path, the repository groups, the Changes and Merge
//! conflicts buckets, the file filter, and the multi-selection reducer. The
//! preview column and the commit card dock beside and under it.
//!
//! Every structural switch reads a signal the tick rebuilds; a press mutates
//! the state layer only, through the existing operations and confirms.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nsh_base::theme;
use nsh_core::element::desktop::MenuItem;
use nsh_core::prelude::*;
use turbogit_app::root_caches::Affected;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, BlameTarget, CommitSubTab, PendingConfirm, Toast, ToastKind};
use turbogit_domain::model::{Change, ChangeRef, ChangeStatus, RootId};

use crate::bridge::AppHandle;
use crate::screens::{commit, diff_view, shelf};
use crate::test_ids;
use crate::theme as data;
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::icon::Icon;
use crate::ui::icon_button::{IconButtonProps, icon_button};
use crate::ui::input::{InputProps, input};
use crate::ui::pane_header::{PaneHeaderProps, pane_header};
use crate::ui::row::{RowProps, RowState, row};
use crate::ui::tab::{TabItem, TabsProps, tabs};

const TICK_MS: u64 = 120;
const COMMIT_PANEL_WIDTH: f32 = 340.0;
const ROW_HEIGHT: f32 = 26.0;

/// A press over the list, as the reducer sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickKind {
    Replace,
    Toggle,
    Range,
}

/// One row in the order it is painted: the range reducer spans exactly the
/// rows the developer can see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowSlot {
    pub key: PathBuf,
    pub selectable: bool,
}

/// Shift outranks command; command alone toggles; nothing held replaces.
pub fn click_kind(shift: bool, command: bool) -> ClickKind {
    if shift {
        ClickKind::Range
    } else if command {
        ClickKind::Toggle
    } else {
        ClickKind::Replace
    }
}

/// The selection reducer, ported from the egui `changes_selection`.
pub fn apply(
    selection: &mut HashSet<PathBuf>,
    anchor: &mut Option<PathBuf>,
    order: &[RowSlot],
    clicked: &Path,
    kind: ClickKind,
) {
    let Some(clicked_idx) = order.iter().position(|slot| slot.key == clicked) else {
        *anchor = Some(clicked.to_path_buf());
        return;
    };
    match kind {
        ClickKind::Replace => {
            selection.clear();
            if order[clicked_idx].selectable {
                selection.insert(clicked.to_path_buf());
            }
            *anchor = Some(clicked.to_path_buf());
        }
        ClickKind::Toggle => {
            if order[clicked_idx].selectable && !selection.remove(clicked) {
                selection.insert(clicked.to_path_buf());
            }
            *anchor = Some(clicked.to_path_buf());
        }
        ClickKind::Range => match anchor
            .as_ref()
            .and_then(|a| order.iter().position(|slot| slot.key == a.as_path()))
        {
            Some(anchor_idx) => {
                selection.clear();
                let span = anchor_idx.min(clicked_idx)..=anchor_idx.max(clicked_idx);
                for slot in &order[span] {
                    if slot.selectable {
                        selection.insert(slot.key.clone());
                    }
                }
            }
            None => {
                selection.clear();
                if order[clicked_idx].selectable {
                    selection.insert(clicked.to_path_buf());
                }
                *anchor = Some(clicked.to_path_buf());
            }
        },
    }
}

/// The staging state a row's checkbox paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageState {
    None,
    Partial,
    All,
}

impl StageState {
    fn of(c: &Change) -> Self {
        match (c.staged, c.unstaged) {
            (true, false) => Self::All,
            (true, true) => Self::Partial,
            _ => Self::None,
        }
    }

    fn words(self) -> (&'static str, &'static str) {
        match self {
            Self::None => ("Stage", "unstaged"),
            Self::Partial => ("Stage all of", "partially staged"),
            Self::All => ("Unstage", "staged"),
        }
    }

    fn box_state(self) -> CheckboxState {
        match self {
            Self::None => CheckboxState::Unchecked,
            Self::Partial => CheckboxState::Indeterminate,
            Self::All => CheckboxState::Checked,
        }
    }
}

/// One row the list paints: a repository group, a bucket header, or a change.
#[derive(Clone, Debug, PartialEq)]
enum RowData {
    Group {
        root_name: String,
        branch: String,
        count: usize,
        expanded: bool,
    },
    Section {
        label: &'static str,
        count: usize,
    },
    File(Box<FileRow>),
}

#[derive(Clone, Debug, PartialEq)]
struct FileRow {
    root: RootId,
    root_name: String,
    path: PathBuf,
    key: PathBuf,
    status: ChangeStatus,
    stage: StageState,
    selected: bool,
    letter: String,
    location: String,
}

/// The held row set the list binds to: equal windows are equal rows, so the
/// idle tick that rebuilds them every 120ms does not rebuild the list.
#[derive(Clone)]
struct ChangesWindow(Arc<Vec<RowData>>);

impl PartialEq for ChangesWindow {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl std::ops::Deref for ChangesWindow {
    type Target = Vec<RowData>;

    fn deref(&self) -> &Vec<RowData> {
        &self.0
    }
}

const CHANGES: &str = "Changes";
const MERGE_CONFLICTS: &str = "Merge conflicts";

fn badge_letter(c: &Change) -> &'static str {
    match c.status {
        ChangeStatus::Conflicted | ChangeStatus::Copied => "C",
        ChangeStatus::Deleted => "D",
        ChangeStatus::Added | ChangeStatus::Unversioned if !c.unstaged => "N",
        _ => "M",
    }
}

fn location_of(path: &Path) -> String {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| format!("{}/", p.display()))
        .unwrap_or_default()
}

fn rows_of(state: &AppState) -> Vec<RowData> {
    let query = state.ui.file_filter.trim().to_lowercase();
    let mut rows = Vec::new();
    for root in &state.multi.roots {
        let mut files: Vec<&Change> = Vec::new();
        let mut conflicts: Vec<&Change> = Vec::new();
        for c in &root.status.changes {
            match c.status {
                ChangeStatus::Conflicted => conflicts.push(c),
                ChangeStatus::Ignored => {}
                _ => files.push(c),
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        conflicts.sort_by(|a, b| a.path.cmp(&b.path));
        if !query.is_empty() {
            let matches =
                |c: &&Change| c.path.display().to_string().to_lowercase().contains(&query);
            files.retain(matches);
            conflicts.retain(matches);
            if files.is_empty() && conflicts.is_empty() {
                continue;
            }
        }
        let expanded = state.ui.changes_expanded.contains(&root.id);
        let count = files.len() + conflicts.len();
        rows.push(RowData::Group {
            root_name: root.id.name(),
            branch: root
                .current_branch
                .clone()
                .unwrap_or_else(|| "<detached>".to_owned()),
            count,
            expanded,
        });
        if !expanded {
            continue;
        }
        let mut push_bucket = |label: &'static str, bucket: &[&Change]| {
            if bucket.is_empty() {
                return;
            }
            if label == MERGE_CONFLICTS {
                rows.push(RowData::Section {
                    label,
                    count: bucket.len(),
                });
            }
            for c in bucket {
                let key = root.canonical_key(c);
                rows.push(RowData::File(Box::new(FileRow {
                    root: root.id.clone(),
                    root_name: root.id.name(),
                    path: c.path.clone(),
                    key: key.clone(),
                    status: c.status,
                    stage: StageState::of(c),
                    selected: state.ui.selected.contains(&key),
                    letter: badge_letter(c).to_string(),
                    location: location_of(&c.path),
                })));
            }
        };
        push_bucket(CHANGES, &files);
        push_bucket(MERGE_CONFLICTS, &conflicts);
    }
    rows
}

fn staged_color(status: ChangeStatus) -> AdaptiveColor {
    match status {
        ChangeStatus::Deleted => data::STATUS_DELETED,
        ChangeStatus::Conflicted => data::STATE_ERROR,
        ChangeStatus::Added | ChangeStatus::Unversioned => data::STATUS_ADDED,
        ChangeStatus::Renamed | ChangeStatus::Copied => data::ACCENT_TEXT,
        ChangeStatus::Modified => data::STATUS_MODIFIED,
        ChangeStatus::Ignored => data::STATUS_UNVERSIONED,
    }
}

/// The Commit tab over a live state: the sub-tab strip, then the built
/// surface the chosen sub-tab names.
pub fn changes_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let window = signal(ChangesWindow(Arc::new(handle.peek(rows_of))));
    let subtab = signal(handle.peek(|s| subtab_index(s.ui.commit_subtab)));

    let tick = handle.clone();
    let tick_window = window;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let rows = tick.read(rows_of);
        // The fresh allocation is harmless only because a window compares by
        // its rows. Compare by allocation instead and every tick rebuilds the
        // list's cells, and the host loses the native view of the cell it is
        // rebuilding: the first row, a lone repository's group, stopped taking
        // presses and stopped reaching the accessibility tree.
        tick_window.set(ChangesWindow(Arc::new(rows)));
    });

    let strip_handle = handle.clone();
    let strip_subtab = subtab;
    let items = vec![
        TabItem::new("Local Changes").test_id(&test_ids::changes("subtab:local")),
        TabItem::new("Shelf").test_id(&test_ids::changes("subtab:shelf")),
        TabItem::new("Stash").test_id(&test_ids::changes("subtab:stash")),
    ];

    let body_handle = handle.clone();
    let body_subtab = subtab;
    let body_window = window;
    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .test_id(&test_ids::route("changes"))
        .child(
            view()
                .flex_row()
                .items_center()
                .h(px(40.0))
                .px(px(data::PANEL_PADDING))
                .border_b(px(1.0))
                .border_color(data::DIVIDER)
                .child(tabs(
                    TabsProps::new(items, move || body_subtab.get()).on_change(move |at| {
                        // A synchronous set from inside the pressed tab's own
                        // style subscription aborts the handler; one turn
                        // later the list is past that frame.
                        let sub = strip_subtab;
                        spawn(async move {
                            sub.set(at);
                        });
                        let value = match at {
                            1 => CommitSubTab::Shelf,
                            2 => CommitSubTab::Stash,
                            _ => CommitSubTab::LocalChanges,
                        };
                        strip_handle.update(move |s| s.ui.commit_subtab = value);
                    }),
                )),
        )
        .child(dynamic(move || match body_subtab.get() {
            1 => shelf::shelf_pane(body_handle.clone()).into_element(),
            2 => shelf::stash_pane(body_handle.clone()).into_element(),
            _ => local_changes(body_handle.clone(), body_window).into_element(),
        }))
}

fn subtab_index(tab: CommitSubTab) -> usize {
    match tab {
        CommitSubTab::LocalChanges => 0,
        CommitSubTab::Shelf => 1,
        CommitSubTab::Stash => 2,
    }
}

/// The Local Changes body: the list and the docked commit card on the left,
/// the preview on the right.
fn local_changes(handle: AppHandle, window: Signal<ChangesWindow>) -> impl IntoElement {
    let t = theme();
    let list_handle = handle.clone();
    let list_window = window;
    let preview_handle = handle.clone();
    let card_handle = handle.clone();
    view()
        .flex_1()
        .flex_row()
        .gap(px(data::PANEL_PADDING))
        .p(px(data::PANEL_PADDING))
        .child(
            view()
                .w(px(COMMIT_PANEL_WIDTH))
                .flex_col()
                .gap(px(data::PANEL_PADDING))
                .child(changes_card(list_handle, list_window))
                .child(commit::commit_card(card_handle)),
        )
        .child(
            view()
                .flex_1()
                .flex_col()
                .rounded(px(data::CARD_RADIUS))
                .bg(t.card)
                .child(diff_view::preview_pane(preview_handle)),
        )
}

/// The changes card: the header, the filter, and the virtualized row list.
fn changes_card(handle: AppHandle, window: Signal<ChangesWindow>) -> impl IntoElement {
    let t = theme();
    let refresh_handle = handle.clone();
    let expand_handle = handle.clone();
    let rollback_handle = handle.clone();
    let filter_handle = handle.clone();
    let count_handle = handle.clone();
    let list_handle = handle.clone();
    let list_window = window;
    let empty_handle = handle.clone();
    let empty_window = window;
    let list = element_ref();

    let count = move || {
        count_handle.read(|s| {
            s.multi
                .roots
                .iter()
                .flat_map(|r| &r.status.changes)
                .filter(|c| !matches!(c.status, ChangeStatus::Ignored))
                .count()
                .to_string()
        })
    };
    let count_rows = move || window.get().len();

    view()
        .flex_1()
        .flex_col()
        .rounded(px(data::CARD_RADIUS))
        .bg(t.card)
        .test_id(&test_ids::changes("card"))
        .child(pane_header(
            PaneHeaderProps::new("Changes")
                .count_of(count)
                .test_id(&test_ids::changes("header"))
                .action(icon_button(
                    IconButtonProps::new(Icon::Refresh, "Refresh changes")
                        .test_id(&test_ids::changes("refresh"))
                        .on_press({
                            let window = list_window;
                            move || {
                                refresh_handle.update(|s| s.refresh(Affected::All));
                                let rows = refresh_handle.read(rows_of);
                                window.set(ChangesWindow(Arc::new(rows)));
                            }
                        }),
                ))
                .action(icon_button(
                    IconButtonProps::new(Icon::ChevronDown, "Expand all groups")
                        .test_id(&test_ids::changes("expand-all"))
                        .on_press({
                            let expand_handle = expand_handle.clone();
                            let window = list_window;
                            move || {
                                let all_open = expand_handle.peek(|s| {
                                    s.multi
                                        .roots
                                        .iter()
                                        .all(|r| s.ui.changes_expanded.contains(&r.id))
                                });
                                expand_handle.update(move |s| {
                                    if all_open {
                                        s.ui.changes_expanded.clear();
                                    } else {
                                        let ids: Vec<RootId> =
                                            s.multi.roots.iter().map(|r| r.id.clone()).collect();
                                        s.ui.changes_expanded.extend(ids);
                                    }
                                });
                                let rows = expand_handle.read(rows_of);
                                window.set(ChangesWindow(Arc::new(rows)));
                            }
                        }),
                ))
                .action(icon_button(
                    IconButtonProps::new(Icon::Undo, "Rollback")
                        .test_id(&test_ids::changes("rollback"))
                        .on_press(move || rollback_handle.update(discard_in_every_root)),
                )),
        ))
        .child(
            view()
                .px(px(data::PANEL_PADDING))
                .py(px(data::SPACING_Y))
                .child(input(
                    InputProps::new()
                        .placeholder("Filter files")
                        .accessibility_label("Filter files")
                        .on_change(move |text: &str| {
                            let text = text.to_string();
                            filter_handle.update(move |s| s.ui.file_filter = text);
                        })
                        .test_id(&test_ids::changes("filter")),
                )),
        )
        .child(dynamic(move || {
            let _ = window.get();
            let row_handle = list_handle.clone();
            let row_window = list_window;
            // The list rebuilds from the window signal as a whole: a keyed
            // reconciliation duplicates the cell before an insertion (a
            // group that expands between two groups), and this list holds
            // changed files, not the 20k rows the diff list must keep keyed.
            virtual_list(count_rows, move |at: Position| {
                list_row(row_handle.clone(), row_window, at)
            })
            .element_ref(&list)
            .estimated_item_size(px(ROW_HEIGHT))
            .flex_1()
            .test_id(&test_ids::changes("list"))
            .into_element()
        }))
        .child(when(
            // A collapsed group is still rows: the message belongs to a clean
            // tree (or a filter that matches nothing), not to a folded list.
            move || {
                !empty_window.get().iter().any(|row| match row {
                    RowData::File(_) => true,
                    RowData::Group { count, .. } => *count > 0,
                    RowData::Section { .. } => false,
                })
            },
            {
                let peek_handle = empty_handle.clone();
                move || {
                    let t = theme();
                    let reader = peek_handle.clone();
                    text(move || {
                        let filter = reader.peek(|s| s.ui.file_filter.trim().to_owned());
                        if filter.is_empty() {
                            "Nothing to commit, working tree is clean.".to_string()
                        } else {
                            format!("No files match '{filter}'.")
                        }
                    })
                    .text_size(px(data::TYPE_BODY))
                    .text_color(t.muted_foreground)
                    .p(px(data::PANEL_PADDING))
                }
            },
        ))
}

/// One painted row: a repository group, a bucket header, or a change.
fn list_row(handle: AppHandle, window: Signal<ChangesWindow>, at: Position) -> AnyElement {
    // Every painted row rebinds to the window: a cell reused at the same
    // index must answer the state it now holds, not the one it had when it
    // first mounted.
    dynamic(move || list_row_at(handle.clone(), window, at)).into_element()
}

fn list_row_at(handle: AppHandle, window: Signal<ChangesWindow>, at: Position) -> AnyElement {
    let t = theme();
    let row_data = window.with(|rows| rows.get(at.index()).cloned());
    match row_data {
        Some(RowData::Group {
            root_name,
            branch,
            count,
            expanded,
        }) => {
            let press_handle = handle.clone();
            let press_window = window;
            let name = root_name.clone();
            row(RowProps::new()
                .state(RowState::Default)
                .test_id(&test_ids::changes(format!("group:{root_name}")))
                .accessibility_label(format!("Toggle {root_name}"))
                .on_press(move || {
                    let name = name.clone();
                    press_handle.update(move |s| {
                        let id = s
                            .multi
                            .roots
                            .iter()
                            .map(|r| r.id.clone())
                            .find(|id| id.name() == name);
                        if let Some(id) = id {
                            if s.ui.changes_expanded.contains(&id) {
                                s.ui.changes_expanded.remove(&id);
                            } else {
                                s.ui.changes_expanded.insert(id);
                            }
                        }
                    });
                    let rows = press_handle.read(rows_of);
                    press_window.set(ChangesWindow(Arc::new(rows)));
                })
                .child(crate::ui::icon::icon(
                    if expanded {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    data::ICON_SM,
                    t.muted_foreground,
                ))
                .child(
                    text(root_name)
                        .text_size(px(data::TYPE_CONTROL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground)
                        .flex_shrink_0(),
                )
                .child(
                    // A branch name is arbitrary and often long, so it takes
                    // the row's remainder and ellipsizes. `min_w(0)` is what
                    // allows that: a name has no spaces, so its min-content
                    // width is the whole name and the cell would otherwise
                    // refuse to shrink and paint past the panel's edge.
                    text(branch)
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground)
                        .truncate()
                        .min_w(px(0.0))
                        .flex_1(),
                )
                .child(
                    text(count.to_string())
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground)
                        .flex_shrink_0(),
                ))
            .into_element()
        }
        Some(RowData::Section { label, count }) => row(RowProps::new()
            .state(RowState::Default)
            .test_id(&test_ids::changes(format!("section:{label}")))
            .child(
                text(format!("{label} ({count})"))
                    .text_size(px(data::TYPE_SECTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .letter_spacing(px(0.5))
                    .text_color(t.muted_foreground),
            ))
        .into_element(),
        Some(RowData::File(file)) => file_row(handle, window, *file).into_element(),
        None => view().into_element(),
    }
}

/// One changed-file row: the checkbox, the status letter, the name, its
/// directory, the staging verb, and the row menu.
fn file_row(handle: AppHandle, window: Signal<ChangesWindow>, file: FileRow) -> impl IntoElement {
    let t = theme();
    let FileRow {
        root,
        root_name,
        path,
        key,
        status,
        stage,
        selected,
        letter,
        location,
    } = file;
    let ink = staged_color(status);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let path_text = path.display().to_string();

    let check_handle = handle.clone();
    let check_root = root.clone();
    let check_path = path.clone();
    let check_window = window;
    let letter_state = stage;
    let box_state = stage.box_state();
    let check = if status == ChangeStatus::Conflicted {
        // An unmerged path has no index state to toggle; the slot keeps the
        // status letter and the name in their shared column.
        view().w(px(data::TYPE_DETAIL_TITLE)).into_element()
    } else {
        checkbox(
            CheckboxProps::new(move || {
                let _ = letter_state;
                box_state
            })
            .accessibility_label(format!(
                "{} {} ({})",
                stage.words().0,
                path_text,
                stage.words().1
            ))
            .test_id(&test_ids::change_check(&root_name, &path_text))
            .on_change(move |_| {
                // The box acts on the state it paints: a status scan that
                // lands mid-press must not turn the click into its opposite.
                let root = check_root.clone();
                let path = check_path.clone();
                let next = stage != StageState::All;
                check_handle.update(move |s| {
                    s.dispatch(shell_writes::commit::set_staged(&root, &path, next));
                });
                let rows = check_handle.read(rows_of);
                check_window.set(ChangesWindow(Arc::new(rows)));
            }),
        )
        .into_element()
    };

    let press_handle = handle.clone();
    let press_root = root.clone();
    let press_path = path.clone();
    let press_key = key.clone();
    let press_window = window;
    let menu_handle = handle.clone();
    let menu_root = root.clone();
    let menu_path = path.clone();
    let menu_key = key.clone();
    let menu_window = window;

    let mut props = RowProps::new()
        .state(if selected {
            RowState::Selected
        } else {
            RowState::Default
        })
        .test_id(&test_ids::change_row(&root_name, &path_text))
        .accessibility_label(path_text.clone())
        .on_press_event(move |event: &PressEvent| {
            let kind = click_kind(event.modifiers.shift, event.modifiers.cmd);
            let order = painted_order(&press_window);
            let key = press_key.clone();
            let root = press_root.clone();
            let path = press_path.clone();
            press_handle.update(move |s| select_row(s, &order, &key, &root, &path, kind));
        })
        .on_context_menu(move |event: &ContextMenuEvent| {
            let at = event.page_position;
            let target = menu_key.clone();
            let root = menu_root.clone();
            let path = menu_path.clone();
            let order = painted_order(&menu_window);
            let opener = menu_handle.clone();
            let menu_opener = opener.clone();
            let menu_root = root.clone();
            let menu_path = path.clone();
            opener.update(move |s| {
                if !s.ui.selected.contains(&target) {
                    select_row(s, &order, &target, &root, &path, ClickKind::Replace);
                }
                s.ui.preview_change = Some(ChangeRef {
                    root: root.clone(),
                    path: path.clone(),
                });
            });
            // The menu reads the state the press just settled; it opens after
            // the mutation's borrow is gone.
            open_file_menu(&menu_opener, &menu_root, &menu_path, at);
        })
        .child(check)
        .child(
            text(letter)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(ink),
        )
        .child(
            text(name)
                .text_size(px(data::TYPE_BODY))
                .text_color(t.card_foreground)
                .flex_1(),
        );
    if !location.is_empty() {
        props = props.child(
            text(location)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
    }
    props = props.child(
        text(stage.words().0)
            .text_size(px(data::TYPE_CHIP))
            .text_color(t.muted_foreground),
    );
    row(props)
}

fn painted_order(window: &Signal<ChangesWindow>) -> Vec<RowSlot> {
    window.with(|rows| {
        rows.iter()
            .filter_map(|row| match row {
                RowData::File(f) => Some(RowSlot {
                    key: f.key.clone(),
                    selectable: f.status != ChangeStatus::Conflicted,
                }),
                _ => None,
            })
            .collect()
    })
}

fn select_row(
    state: &mut AppState,
    order: &[RowSlot],
    key: &Path,
    root: &RootId,
    path: &Path,
    kind: ClickKind,
) {
    apply(
        &mut state.ui.selected,
        &mut state.ui.selection_anchor,
        order,
        key,
        kind,
    );
    state.ui.preview_change = Some(ChangeRef {
        root: root.clone(),
        path: path.to_path_buf(),
    });
}

fn set_file_staged(state: &mut AppState, root: &RootId, path: &Path) {
    let Some(change) = state.multi.by_id(root).and_then(|r| r.resolve_change(path)) else {
        return;
    };
    let stage = StageState::of(change) != StageState::All;
    state.dispatch(shell_writes::commit::set_staged(root, path, stage));
}

fn selected_changes(state: &AppState, root: &RootId) -> Vec<Change> {
    let mut out = Vec::new();
    if let Some(root) = state.multi.by_id(root) {
        for c in &root.status.changes {
            if state.ui.selected.contains(&root.canonical_key(c)) {
                out.push(c.clone());
            }
        }
    }
    out
}

fn discard_in_every_root(state: &mut AppState) {
    let rows: Vec<(RootId, Vec<Change>)> = state
        .multi
        .roots
        .iter()
        .filter_map(|root| {
            let changes: Vec<Change> = selected_changes(state, &root.id)
                .into_iter()
                .filter(|c| c.status != ChangeStatus::Unversioned)
                .collect();
            (!changes.is_empty()).then(|| (root.id.clone(), changes))
        })
        .collect();
    match rows.len() {
        0 => state.ui.toast = Some(Toast::warning("Select files to discard.")),
        1 => {
            let (root, changes) = rows.into_iter().next().expect("one row");
            state.ui.confirm = Some(PendingConfirm::Discard { root, changes });
        }
        _ => state.ui.confirm = Some(PendingConfirm::DiscardAcross { rows }),
    }
}

fn discard_selected(state: &mut AppState, root: &RootId) {
    let changes: Vec<Change> = selected_changes(state, root)
        .into_iter()
        .filter(|c| c.status != ChangeStatus::Unversioned)
        .collect();
    if changes.is_empty() {
        state.ui.toast = Some(Toast::warning("Select files to discard."));
        return;
    }
    state.ui.confirm = Some(PendingConfirm::Discard {
        root: root.clone(),
        changes,
    });
}

/// The file row's context menu, with the egui gates: the staging verb names
/// the row's own state, an untracked row cannot be discarded or blamed, and
/// the parking verbs need a selection git can diff.
fn open_file_menu(handle: &AppHandle, root: &RootId, path: &Path, at: Point<Points>) {
    let (change, parkable) = handle.peek(|s| {
        let change = s
            .multi
            .by_id(root)
            .and_then(|r| r.resolve_change(path))
            .cloned();
        let parkable = s.multi.by_id(root).is_some_and(|r| {
            r.status.changes.iter().any(|c| {
                c.status != ChangeStatus::Unversioned && s.ui.selected.contains(&r.canonical_key(c))
            })
        });
        (change, parkable)
    });
    let Some(change) = change else {
        return;
    };
    let untracked = change.status == ChangeStatus::Unversioned;
    let conflicted = change.status == ChangeStatus::Conflicted;
    let stage = StageState::of(&change);
    let mut items: Vec<MenuItem> = Vec::new();
    if conflicted {
        items.push(MenuItem::new("Resolve…").disabled(true));
        let ours = handle.clone();
        let root_ours = root.clone();
        let path_ours = path.to_path_buf();
        items.push(MenuItem::new("Take ours").on_select(move || {
            let operation = shell_writes::conflict::accept_ours(&root_ours, &path_ours);
            ours.update(move |s| s.dispatch(operation));
        }));
        let theirs = handle.clone();
        let root_theirs = root.clone();
        let path_theirs = path.to_path_buf();
        items.push(MenuItem::new("Take theirs").on_select(move || {
            let operation = shell_writes::conflict::accept_theirs(&root_theirs, &path_theirs);
            theirs.update(move |s| s.dispatch(operation));
        }));
    } else {
        let stage_handle = handle.clone();
        let stage_root = root.clone();
        let stage_path = path.to_path_buf();
        items.push(
            MenuItem::new(format!("{} file", stage.words().0)).on_select(move || {
                let root = stage_root.clone();
                let path = stage_path.clone();
                stage_handle.update(move |s| {
                    set_file_staged(s, &root, &path);
                });
            }),
        );
    }
    let discard_reason = if conflicted {
        Some("resolve the conflict first")
    } else if untracked {
        Some("an untracked file has no committed version")
    } else {
        None
    };
    match discard_reason {
        Some(_) => items.push(MenuItem::new("Discard changes…").disabled(true)),
        None => {
            let discard_handle = handle.clone();
            let discard_root = root.clone();
            items.push(MenuItem::new("Discard changes…").on_select(move || {
                let root = discard_root.clone();
                discard_handle.update(move |s| discard_selected(s, &root));
            }));
        }
    }
    if !conflicted {
        if parkable {
            let shelve_handle = handle.clone();
            let shelve_root = root.clone();
            items.push(MenuItem::new("Shelve…").on_select(move || {
                let root = shelve_root.clone();
                shelve_handle.update(move |s| s.open_shelve_dialog(Some(root)));
            }));
            let patch_handle = handle.clone();
            let patch_root = root.clone();
            items.push(
                MenuItem::new("Create Patch from Local Changes").on_select(move || {
                    let root = patch_root.clone();
                    patch_handle.update(move |s| {
                        turbogit_app::create_patch::open_from_selection_in(s, &root);
                    });
                }),
            );
            let copy_handle = handle.clone();
            let copy_root = root.clone();
            items.push(
                MenuItem::new("Copy as Patch to Clipboard").on_select(move || {
                    let root = copy_root.clone();
                    copy_handle.update(move |s| {
                        turbogit_app::create_patch::copy_from_selection_in(s, &root);
                    });
                }),
            );
        } else {
            items.push(MenuItem::new("Shelve…").disabled(true));
            items.push(MenuItem::new("Create Patch from Local Changes").disabled(true));
            items.push(MenuItem::new("Copy as Patch to Clipboard").disabled(true));
        }
    }
    match (untracked, conflicted) {
        (true, _) => items.push(MenuItem::new("Show blame").disabled(true)),
        (false, _) => {
            let blame_handle = handle.clone();
            let blame_root = root.clone();
            let blame_path = path.to_path_buf();
            items.push(MenuItem::new("Show blame").on_select(move || {
                let root = blame_root.clone();
                let path = blame_path.clone();
                blame_handle.update(move |s| {
                    s.ui.blame = Some(BlameTarget {
                        root: root.clone(),
                        path: path.clone(),
                        rev: "HEAD".to_owned(),
                    });
                });
            }));
        }
    }
    spawn(async move {
        let _ = nsh_core::element::desktop::popup_menu_at(items, at).await;
    });
}

/// The preview's change navigation: the ordered files the list paints.
pub fn preview_nav(handle: &AppHandle, change: &ChangeRef, delta: isize) {
    let files: Vec<ChangeRef> = handle.peek(|s| {
        let mut out = Vec::new();
        for root in &s.multi.roots {
            let mut paths: Vec<PathBuf> = root
                .status
                .changes
                .iter()
                .filter(|c| c.status != ChangeStatus::Ignored)
                .map(|c| c.path.clone())
                .collect();
            paths.sort();
            out.extend(
                paths
                    .into_iter()
                    .map(|path| ChangeRef::new(root.id.clone(), path)),
            );
        }
        out
    });
    if let Some(at) = files.iter().position(|c| c == change) {
        let next = at as isize + delta;
        if next >= 0
            && let Some(target) = files.get(next as usize)
        {
            handle.update(|s| s.ui.preview_change = Some(target.clone()));
        }
    }
}

/// The toast a failed discard or stage lands; the shared severity map.
pub fn toast_color(kind: ToastKind) -> AdaptiveColor {
    crate::ui::toast::kind_color(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(expanded: bool) -> Vec<RowData> {
        vec![RowData::Group {
            root_name: "alpha".to_owned(),
            branch: "main".to_owned(),
            count: 1,
            expanded,
        }]
    }

    /// The tick hands the list a window it rebuilt from scratch; a window
    /// whose rows did not change has to compare equal to the one on screen,
    /// or the list re-makes its cells on every tick and the host loses the
    /// first row's native view.
    #[test]
    fn a_window_rebuilt_from_the_same_rows_compares_equal() {
        let held = ChangesWindow(Arc::new(group(false)));
        let ticked = ChangesWindow(Arc::new(group(false)));
        assert!(!Arc::ptr_eq(&held.0, &ticked.0), "one allocation each");
        assert!(
            held == ticked,
            "the tick's window matches the one on screen"
        );
    }

    #[test]
    fn a_window_whose_rows_moved_compares_unequal() {
        let collapsed = ChangesWindow(Arc::new(group(false)));
        let expanded = ChangesWindow(Arc::new(group(true)));
        assert!(collapsed != expanded, "the group opened");
    }
}
