//! The toast layer: one ToastManager shared by the app, advanced by the
//! root's interval, rendered by a ToastStack at the window's edge.

use std::time::Duration;

use nsh_base::{Anchor, ToastManager, ToastMotion, ToastOptions, theme, toast, toast_stack};
use nsh_core::prelude::*;
use nsh_core::time::now;

use crate::theme as data;

pub type AppToastManager = ToastManager<u64, SharedString>;

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

    /// Pushes a toast. The root's interval advances the lifecycle and syncs
    /// the stack.
    pub fn push(&self, message: SharedString) {
        let id = self.next_id.with(|next| {
            let id = *next;
            *next += 1;
            id
        });
        self.manager.with(|m| {
            m.push(
                id,
                message,
                ToastOptions::timeout(Some(Duration::from_secs(5))),
                now(),
            )
        });
    }

    /// Advances the lifecycle; true when the stack should re-render.
    pub fn advance(&self) -> bool {
        self.manager.with(|m| m.advance(now(), false).changed)
    }

    /// The visible toasts, oldest first, as (id, message).
    pub fn visible(&self, limit: usize) -> Vec<(u64, SharedString)> {
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
pub fn toast_layer(
    shared: ToastShared,
    shown: Signal<Vec<(u64, SharedString)>>,
) -> impl IntoElement {
    let t = theme();
    toast_stack().placement(Anchor::BottomCenter).child(each(
        shown,
        |(id, _)| *id,
        move |item, _| {
            let shared = shared;
            toast()
                .accessibility_label(move || item.get().1.clone())
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
                .child(text(move || item.get().1.clone()).text_size(px(data::TYPE_BODY)))
        },
    ))
}
