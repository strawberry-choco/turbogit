//! Blame: per-line attribution of one file at one revision, over the keyed
//! blame read. A line press lands its commit in the log. The surface is a
//! dialog the shell paints from `ui.blame`; the route paints the same body
//! for the route table.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, TimeZone, Utc};
use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::keyed_read::Read;
use turbogit_app::state::{AppState, BlameTarget, Tab};
use turbogit_domain::model::BlameLine;

use crate::bridge::AppHandle;
use crate::screens::shell::TabSwitcher;
use crate::screens::short;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::dialog::{DialogProps, dialog};

const TICK_MS: u64 = 120;
const WINDOW_WIDTH: f32 = 760.0;
const WINDOW_HEIGHT: f32 = 520.0;

/// One blame verdict, owned so the row signal compares across ticks.
#[derive(Clone, Default, PartialEq)]
struct BlameWindow {
    key: String,
    waiting: bool,
    empty: bool,
    failure: Option<String>,
    lines: Arc<[BlameLine]>,
}

fn plan(state: &mut AppState, target: &BlameTarget) -> BlameWindow {
    let key = format!(
        "{}|{}|{}",
        target.root.0.display(),
        target.rev,
        target.path.display()
    );
    let mut window = BlameWindow {
        key,
        ..Default::default()
    };
    match state.read(target.clone()) {
        Read::Fresh(lines) => {
            window.empty = lines.is_empty();
            window.lines = lines;
        }
        Read::Empty => window.empty = true,
        Read::Waiting => window.waiting = true,
        Read::Failed(message) => window.failure = Some(message),
    }
    window
}

/// The dialog layer: blame over the log while `ui.blame` names a target.
pub fn layer(handle: AppHandle, switcher: TabSwitcher) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.blame.is_some()),
        move || {
            let t = theme();
            let close = handle.clone();
            let body = blame_body(handle.clone(), switcher.clone());
            dialog(
                DialogProps::new("blame", "Blame")
                    .width(WINDOW_WIDTH)
                    .cancel_label("Close")
                    .hide_action(true)
                    .on_cancel(move || close.update(|s| s.ui.blame = None))
                    .child(
                        view()
                            .w(px(WINDOW_WIDTH - 24.0))
                            .h(px(WINDOW_HEIGHT))
                            .flex_col()
                            .bg(t.card)
                            .child(body),
                    ),
            )
        },
    )
}

/// The route's screen: the selected file's blame when one is open, else the
/// launcher that opens it from the log selection.
pub fn blame_screen(handle: AppHandle, switcher: TabSwitcher) -> impl IntoElement {
    let t = theme();
    let open = handle.clone();
    let has_target = handle.peek(|s| s.ui.blame.is_some());
    if has_target {
        return view()
            .flex_1()
            .flex_col()
            .test_id(&test_ids::route("blame"))
            .child(blame_body(handle, switcher));
    }
    let can_open =
        handle.peek(|s| s.ui.selected_commit.is_some() && s.ui.log_selected_file.is_some());
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .test_id(&test_ids::route("blame"))
        .child(
            view()
                .flex_col()
                .items_center()
                .gap(px(data::SPACING_Y))
                .child(
                    text("Blame")
                        .text_size(px(data::TYPE_PANE_TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground),
                )
                .child(
                    text("Select a commit and a changed file in the Log, then open blame.")
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground),
                )
                .child(button(
                    ButtonProps::new("Open blame")
                        .variant(ButtonVariant::Secondary)
                        .disabled(!can_open)
                        .test_id(&test_ids::blame("open"))
                        .on_press(move || {
                            open.update(|s| {
                                let Some((root, cid)) = s.ui.selected_commit.clone() else {
                                    return;
                                };
                                let Some((_, path)) = s.ui.log_selected_file.clone() else {
                                    return;
                                };
                                s.ui.blame = Some(BlameTarget {
                                    root,
                                    path,
                                    rev: cid,
                                });
                                s.ui.blame_caret_line = None;
                            });
                        }),
                )),
        )
}

fn blame_body(handle: AppHandle, switcher: TabSwitcher) -> impl IntoElement {
    let t = theme();
    let current = signal(default_target_plan(handle.clone()));
    let tick = handle.clone();
    let tick_current = current;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let next = tick.update_with(|s| {
            s.ui.blame
                .clone()
                .map(|target| plan(s, &target))
                .unwrap_or_default()
        });
        if next != tick_current.get() {
            tick_current.set(next);
        }
    });

    let target = handle.peek(|s| s.ui.blame.clone());
    let (subject, rev) = target
        .as_ref()
        .map(|target| {
            (
                target.path.display().to_string(),
                short(&target.rev).to_string(),
            )
        })
        .unwrap_or_default();
    let rev_full = target
        .as_ref()
        .map(|target| target.rev.clone())
        .unwrap_or_default();
    let list = element_ref();
    let reveal = handle.clone();
    let reveal_list = list;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let caret = reveal.update_with(|s| s.ui.blame_caret_line.take());
        if let Some(line) = caret {
            reveal_list.scroll_to_index(
                line.saturating_sub(1) as u32,
                percentage(0.5),
                px(0.0),
                false,
            );
        }
    });

    let row_handle = handle.clone();
    let row_target = target.clone();
    let row_rows = current;
    let row_switcher = switcher.clone();
    let list_element = virtual_list(
        move || row_rows.with(|window| window.lines.len()),
        move |at| {
            let index = at.index();
            let line = row_rows.with(|window| window.lines.get(index).cloned());
            let Some(line) = line else {
                return view().into_element();
            };
            let t = theme();
            let selected = line.commit == rev_full;
            let press_handle = row_handle.clone();
            let press_target = row_target.clone();
            let press_switcher = row_switcher.clone();
            let commit = line.commit.clone();
            view()
                .flex_row()
                .items_center()
                .h(px(data::FILE_ROW_HEIGHT))
                .px(px(4.0))
                .bg(if selected { t.accent } else { t.card })
                .role(AccessibilityRole::Button)
                .label(format!(
                    "{} {}",
                    short(&line.commit),
                    line.content.trim_end()
                ))
                .test_id(&test_ids::blame(format!("row:{}", line.line_no)))
                .on_press(move || {
                    let Some(target) = press_target.clone() else {
                        return;
                    };
                    let commit = commit.clone();
                    press_switcher.select(Tab::Log);
                    press_handle.update(move |s| {
                        s.ui.selected_commit = Some((target.root.clone(), commit.clone()));
                        s.ui.log_selected_file = None;
                        s.ui.blame = None;
                        s.ui.log_scroll_to = Some(commit.clone());
                    });
                })
                .child(
                    text(short(&line.commit))
                        .text_size(px(data::TYPE_BODY))
                        .text_color(data::LINK)
                        .w(px(64.0)),
                )
                .child(
                    text(line.author.chars().take(12).collect::<String>())
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground)
                        .w(px(96.0)),
                )
                .child(
                    text(line.content.trim_end().to_string())
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.foreground)
                        .flex_1(),
                )
                .child(
                    text(fmt_age(line.time))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                )
                .into_element()
        },
    )
    .row_gap(px(0.0))
    .estimated_item_size(px(data::FILE_ROW_HEIGHT))
    .flex_1();

    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(
            text(format!("{subject} @ {rev}"))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.primary),
        )
        .child(column_header());
    body = body.child(dynamic(move || {
        let window = current.get();
        if let Some(message) = window.failure.clone() {
            return view()
                .p(px(data::PANEL_PADDING))
                .child(
                    text(message)
                        .text_size(px(data::TYPE_BODY))
                        .text_color(data::STATE_ERROR),
                )
                .into_element();
        }
        if window.waiting {
            return view()
                .p(px(data::PANEL_PADDING))
                .child(
                    text("Computing blame…")
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground),
                )
                .into_element();
        }
        if window.empty {
            return view()
                .p(px(data::PANEL_PADDING))
                .child(
                    text("No blame lines for this revision.")
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground),
                )
                .into_element();
        }
        view()
            .h(px(400.0))
            .flex_col()
            .child(list_element.clone())
            .into_element()
    }));
    body.test_id(&test_ids::blame("body"))
}

fn default_target_plan(handle: AppHandle) -> BlameWindow {
    handle.update_with(|s| {
        s.ui.blame
            .clone()
            .map(|target| plan(s, &target))
            .unwrap_or_default()
    })
}

fn column_header() -> impl IntoElement {
    let t = theme();
    let labels = view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .h(px(data::GROUP_ROW_HEIGHT))
        .px(px(4.0))
        .child(
            text("HASH")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .w(px(64.0)),
        )
        .child(
            text("AUTHOR")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .w(px(96.0)),
        )
        .child(
            text("LINE")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .flex_1(),
        )
        .child(
            text("AGE")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    view()
        .flex_col()
        .child(labels)
        .child(view().h(px(1.0)).bg(t.border))
}

fn fmt_age(t: i64) -> String {
    let now = Local::now().timestamp();
    let d = (now - t).max(0);
    if d < 60 {
        format!("{d}s ago")
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else if d < 86400 {
        format!("{}h ago", d / 3600)
    } else if d < 2592000 {
        format!("{}d ago", d / 86400)
    } else {
        match Utc.timestamp_opt(t, 0) {
            chrono::LocalResult::Single(dt) => {
                let local: DateTime<Local> = DateTime::from(dt);
                local.format("%Y-%m-%d").to_string()
            }
            _ => String::new(),
        }
    }
}
