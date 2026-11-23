//! The toast layer: one ToastManager shared by the app, advanced by the
//! root's interval, rendered by a ToastStack at the window's edge. A toast
//! carries its state kind: the accent bar at its leading edge paints the
//! kind's severity color, and state is never a chip.

use std::time::Duration;

use nsh_base::{Anchor, ToastManager, ToastMotion, ToastOptions, theme, toast, toast_stack};
use nsh_core::prelude::*;
use nsh_core::time::now;

use crate::theme as data;

/// The kind a toast announces. The state layer's own enum: a notice queued
/// in `AppState::ui.toast` joins the shared stack with the kind it was
/// pushed under.
pub type ToastKind = turbogit_app::state::ToastKind;

/// One visible toast: its message and the kind its accent bar wears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastItem {
    pub message: SharedString,
    pub kind: ToastKind,
}

impl ToastItem {
    pub fn info(message: impl Into<SharedString>) -> Self {
        ToastItem {
            message: message.into(),
            kind: ToastKind::Info,
        }
    }
}

/// The accent color of each kind, from the data token layer.
pub fn kind_color(kind: ToastKind) -> AdaptiveColor {
    match kind {
        ToastKind::Success => data::STATE_SUCCESS,
        ToastKind::Warning => data::STATE_WARNING,
        ToastKind::Error => data::STATE_ERROR,
        ToastKind::Info => data::STATE_INFO,
    }
}

pub type AppToastManager = ToastManager<u64, ToastItem>;

/// The manager and the next toast id; `Copy` handles so screens share one
/// layer.
#[derive(Clone, Copy)]
pub struct ToastShared {
    pub manager: Stored<AppToastManager>,
    pub next_id: Stored<u64>,
}

impl ToastShared {
    pub fn new() -> Self {
        ToastShared {
            manager: stored(AppToastManager::new(ToastMotion::sonner())),
            next_id: stored(1_u64),
        }
    }

    /// Pushes an info toast. The root's interval advances the lifecycle and
    /// syncs the stack.
    pub fn push(&self, message: SharedString) {
        self.push_kind(ToastKind::Info, message);
    }

    /// Pushes a toast under a kind.
    pub fn push_kind(&self, kind: ToastKind, message: SharedString) {
        let id = self.next_id.with(|next| {
            let id = *next;
            *next += 1;
            id
        });
        let item = ToastItem { message, kind };
        self.manager.with(|m| {
            m.push(
                id,
                item,
                ToastOptions::timeout(Some(Duration::from_secs(5))),
                now(),
            )
        });
    }

    /// Advances the lifecycle; true when the stack should re-render.
    pub fn advance(&self) -> bool {
        self.manager.with(|m| m.advance(now(), false).changed)
    }

    /// The visible toasts, oldest first, as (id, item).
    pub fn visible(&self, limit: usize) -> Vec<(u64, ToastItem)> {
        self.manager.with(|m| {
            m.visible(limit)
                .map(|(id, value, _)| (*id, value.clone()))
                .collect::<Vec<_>>()
        })
    }
}

impl Default for ToastShared {
    fn default() -> Self {
        Self::new()
    }
}

/// The stack the root mounts inside an Overlay at the bottom edge.
pub fn toast_layer(shared: ToastShared, shown: Signal<Vec<(u64, ToastItem)>>) -> impl IntoElement {
    let t = theme();
    toast_stack().placement(Anchor::BottomCenter).child(each(
        shown,
        |(id, _)| *id,
        move |item, _| {
            let shared = shared;
            let kind = item.get().1.kind;
            toast()
                .accessibility_label(move || item.get().1.message.clone())
                .on_dismiss_request(move || {
                    let id = item.get().0;
                    shared.manager.with(|m| m.dismiss(&id, now()));
                })
                .rounded(px(data::CARD_RADIUS))
                .px_4()
                .py_3()
                .bg(t.card)
                .border(px(1.0))
                .border_color(t.border)
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(data::CELL_GAP))
                        .child(
                            view()
                                .w(px(data::RAIL_WIDTH))
                                .h(px(data::TYPE_PANE_TITLE))
                                .rounded(px(data::MARK_RADIUS))
                                .bg(kind_color(kind)),
                        )
                        .child(
                            text(move || item.get().1.message.clone())
                                .text_size(px(data::TYPE_BODY)),
                        ),
                )
        },
    ))
}
