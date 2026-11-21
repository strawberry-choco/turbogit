//! The image and binary diff panes: the two decoded sides at their own
//! dimensions with their captions, or the binary caption when no pixels
//! exist. The pixels come from the state layer's `PaneTarget` read; nothing
//! is decoded here.

use std::sync::Arc;
use std::time::Duration;

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::diff_data::PaneSide;
use turbogit_app::diff_model::{FileMeta, PaneKind, pane_kind};
use turbogit_app::keyed_read::{DiffTarget, PaneFlavour, PaneTarget, Read};
use turbogit_app::state::AppState;

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::image::{ImagePaneProps, image_pane};

const TICK_MS: u64 = 120;
const MAX_PANE_HEIGHT: f32 = 360.0;

/// One pane side: its length always, its pixels when they decoded.
#[derive(Clone)]
struct SideView {
    byte_len: u64,
    width: u32,
    height: u32,
    rgba: Option<Arc<[u8]>>,
}

impl PartialEq for SideView {
    fn eq(&self, other: &Self) -> bool {
        self.byte_len == other.byte_len
            && self.width == other.width
            && self.height == other.height
            && match (&self.rgba, &other.rgba) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl SideView {
    fn of(side: &PaneSide) -> Self {
        match &side.image {
            Some(image) => SideView {
                byte_len: side.byte_len,
                width: image.width,
                height: image.height,
                rgba: Some(Arc::from(image.rgba.as_slice())),
            },
            None => SideView {
                byte_len: side.byte_len,
                width: 0,
                height: 0,
                rgba: None,
            },
        }
    }

    fn caption(&self) -> String {
        if self.rgba.is_some() {
            format!(
                "{}×{} · {}",
                self.width,
                self.height,
                human_size(self.byte_len)
            )
        } else {
            human_size(self.byte_len)
        }
    }
}

#[derive(Clone, PartialEq)]
struct PaneWindow {
    old: Option<SideView>,
    new: Option<SideView>,
    loading: bool,
}

fn pane_of(
    state: &mut AppState,
    target: &DiffTarget,
    meta: &FileMeta,
    flavour: PaneFlavour,
) -> PaneWindow {
    let decode = matches!(flavour, PaneFlavour::Image);
    let pane = PaneTarget::new(target.clone(), flavour, meta.clone(), decode);
    match state.read(pane) {
        Read::Fresh(entry) => PaneWindow {
            old: entry.old.as_ref().map(SideView::of),
            new: entry.new.as_ref().map(SideView::of),
            loading: false,
        },
        _ => PaneWindow {
            old: None,
            new: None,
            loading: true,
        },
    }
}

/// The standalone image-diff route: whichever target the shell holds, when
/// it carries a picture or a binary.
pub fn image_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (target, kind) = handle.peek(|s| {
        let target =
            s.ui.diff
                .as_ref()
                .map(|target| crate::screens::diff_view::keyed_target(target, s.ui.diff_policies))
                .or_else(|| {
                    s.ui.preview_change.as_ref().map(|change| {
                        DiffTarget::working_tree(
                            change.root.as_path().to_path_buf(),
                            s.ui.diff_policies,
                            Some(change.path.clone()),
                        )
                    })
                });
        let kind = target
            .as_ref()
            .and_then(|target| s.peek(target.clone()))
            .map(|value| pane_kind(&value.model.files));
        (target, kind)
    });
    let body = match (target, kind) {
        (Some(target), Some(kind @ (PaneKind::Image | PaneKind::Binary))) => {
            diff_target_pane(handle, target, kind).into_element()
        }
        _ => text("Open a picture or binary change to see its pane.")
            .text_size(px(data::TYPE_BODY))
            .text_color(t.muted_foreground)
            .into_element(),
    };
    view()
        .flex_1()
        .flex_col()
        .bg(t.background)
        .test_id(&test_ids::route("image_diff"))
        .child(body)
}

fn meta_of(state: &AppState, target: &DiffTarget) -> Option<FileMeta> {
    state
        .peek(target.clone())
        .and_then(|value| value.model.files.first().cloned())
}

/// The pane for one target, chosen by its pane kind.
pub fn diff_target_pane(handle: AppHandle, target: DiffTarget, kind: PaneKind) -> impl IntoElement {
    let meta = handle.peek(|s| meta_of(s, &target));
    let pane = match (kind, meta) {
        (PaneKind::Image, Some(meta)) => image_side(handle, target, meta).into_element(),
        (PaneKind::Binary, Some(meta)) => binary_side(handle, target, meta).into_element(),
        _ => text("No picture or binary content.")
            .text_size(px(data::TYPE_BODY))
            .into_element(),
    };
    view()
        .flex_1()
        .flex_col()
        .test_id(&test_ids::route("image_diff"))
        .child(pane)
}

fn image_side(handle: AppHandle, target: DiffTarget, meta: FileMeta) -> AnyElement {
    let window = signal(PaneWindow {
        old: None,
        new: None,
        loading: true,
    });
    let tick = handle.clone();
    let tick_target = target.clone();
    let tick_meta = meta.clone();
    let tick_window = window;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let next = tick.update_with(|s| pane_of(s, &tick_target, &tick_meta, PaneFlavour::Image));
        if next != tick_window.get() {
            tick_window.set(next);
        }
    });

    let t = theme();
    let paint = window;
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(data::SPACING_Y))
        .p(px(data::PANEL_PADDING))
        .bg(t.card)
        .test_id(&test_ids::image("image"))
        .child(
            text(
                meta.new_path
                    .clone()
                    .or(meta.old_path.clone())
                    .unwrap_or_default(),
            )
            .text_size(px(data::TYPE_DETAIL_TITLE))
            .text_color(t.card_foreground),
        )
        .child(dynamic(move || {
            let pane = paint.get();
            if pane.loading {
                return text("Loading image…")
                    .text_size(px(data::TYPE_BODY))
                    .text_color(t.muted_foreground)
                    .into_element();
            }
            let has_pixels = pane.old.as_ref().is_some_and(|s| s.rgba.is_some())
                && pane.new.as_ref().is_some_and(|s| s.rgba.is_some());
            if !has_pixels {
                let sizes = match (&pane.old, &pane.new) {
                    (Some(old), Some(new)) => Some((old.byte_len, new.byte_len)),
                    _ => None,
                };
                return binary_caption(sizes).into_element();
            }
            let side = |view: &SideView, id: &str| {
                image_pane(
                    ImagePaneProps::new(view.caption(), MAX_PANE_HEIGHT)
                        .pixels(view.width, view.height, view.rgba.clone().expect("pixels"))
                        .test_id(id),
                )
                .into_element()
            };
            view()
                .flex_row()
                .items_start()
                .gap(px(data::PANEL_PADDING))
                .child(side(
                    pane.old.as_ref().expect("both sides"),
                    &test_ids::image("old"),
                ))
                .child(side(
                    pane.new.as_ref().expect("both sides"),
                    &test_ids::image("new"),
                ))
                .into_element()
        }))
        .into_element()
}

fn binary_side(handle: AppHandle, target: DiffTarget, meta: FileMeta) -> AnyElement {
    let window = signal(PaneWindow {
        old: None,
        new: None,
        loading: true,
    });
    let tick = handle.clone();
    let tick_target = target.clone();
    let tick_meta = meta.clone();
    let tick_window = window;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let next = tick.update_with(|s| pane_of(s, &tick_target, &tick_meta, PaneFlavour::Binary));
        if next != tick_window.get() {
            tick_window.set(next);
        }
    });
    let t = theme();
    let paint = window;
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .p(px(data::PANEL_PADDING))
        .bg(t.card)
        .test_id(&test_ids::image("binary"))
        .child(
            text(
                meta.new_path
                    .clone()
                    .or(meta.old_path.clone())
                    .unwrap_or_default(),
            )
            .text_size(px(data::TYPE_DETAIL_TITLE))
            .text_color(t.card_foreground),
        )
        .child(dynamic(move || {
            let pane = paint.get();
            let sizes = match (&pane.old, &pane.new) {
                (Some(old), Some(new)) => Some((old.byte_len, new.byte_len)),
                _ => None,
            };
            binary_caption(sizes).into_element()
        }))
        .into_element()
}

fn binary_caption(sizes: Option<(u64, u64)>) -> impl IntoElement {
    let t = theme();
    let text_value = match sizes {
        Some((before, after)) => format!(
            "Binary file changed · {} → {}",
            human_size(before),
            human_size(after)
        ),
        None => "Binary file changed".to_owned(),
    };
    text(text_value)
        .text_size(px(data::TYPE_BODY))
        .text_color(t.muted_foreground)
        .test_id(&test_ids::image("caption"))
}

/// `1920` bytes and up, the egui pane caption's units.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if value.round() >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    let name = UNITS[unit];
    if unit == 0 || value >= 10.0 {
        format!("{value:.0} {name}")
    } else {
        format!("{value:.1} {name}")
    }
}
